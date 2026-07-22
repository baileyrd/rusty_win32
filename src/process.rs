//! Raw `CreateProcessW` spawn + wait — narrowly scoped to what Job-Object-
//! integrated background jobs need (rush's
//! `docs/WINDOWS_BACKEND_ANALYSIS.md` §4.1), not a replacement for
//! `std::process::Command`. Ordinary foreground spawn/wait already works
//! via `std::process::Command`, which resolves to the same underlying
//! `CreateProcessW`/`WaitForSingleObject` calls — keep using it for that.
//! The one thing it can't do is hand back the child's *thread* handle,
//! which starting a process suspended (`CREATE_SUSPENDED`) requires for
//! `resume` afterward: a process must be assigned to a
//! [`crate::job`] object *before* its main thread runs, and there is no
//! stable way to reach a `std::process::Child`'s thread handle.
//!
//! Command-line construction (Windows argv quoting) is the caller's
//! responsibility. `std::process::Command` already solves that correctly
//! and there's no public API to reuse its escaping logic here, so
//! [`spawn_suspended`] takes an already-built command-line string rather
//! than reimplementing argument quoting.

use crate::error::Win32Error;
use crate::handle::RawHandle;

extern crate alloc;
use alloc::vec::Vec;

const CREATE_SUSPENDED: u32 = 0x0000_0004;
const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
const INFINITE: u32 = 0xFFFF_FFFF;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 258;

// Layout verified against mingw-w64's processthreadsapi.h
// (`_STARTUPINFOW`): `size_of` 104, `align_of` 8 on x86_64, checked by
// compiling a `_Static_assert`-based probe against the real header rather
// than hand-computed padding.
#[repr(C)]
#[derive(Default)]
struct StartupInfoW {
    cb: u32,
    lp_reserved: *mut u16,
    lp_desktop: *mut u16,
    lp_title: *mut u16,
    dw_x: u32,
    dw_y: u32,
    dw_x_size: u32,
    dw_y_size: u32,
    dw_x_count_chars: u32,
    dw_y_count_chars: u32,
    dw_fill_attribute: u32,
    dw_flags: u32,
    w_show_window: u16,
    cb_reserved2: u16,
    lp_reserved2: *mut u8,
    h_std_input: RawHandle,
    h_std_output: RawHandle,
    h_std_error: RawHandle,
}

const _: () = assert!(core::mem::size_of::<StartupInfoW>() == 104);
const _: () = assert!(core::mem::align_of::<StartupInfoW>() == 8);

// Layout verified the same way against `_PROCESS_INFORMATION`: `size_of`
// 24, `align_of` 8 on x86_64.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct ProcessInformationRaw {
    h_process: RawHandle,
    h_thread: RawHandle,
    dw_process_id: u32,
    dw_thread_id: u32,
}

const _: () = assert!(core::mem::size_of::<ProcessInformationRaw>() == 24);
const _: () = assert!(core::mem::align_of::<ProcessInformationRaw>() == 8);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateProcessW(
        application_name: *const u16,
        command_line: *mut u16,
        process_attributes: *const core::ffi::c_void,
        thread_attributes: *const core::ffi::c_void,
        inherit_handles: i32,
        creation_flags: u32,
        environment: *const core::ffi::c_void,
        current_directory: *const u16,
        startup_info: *mut StartupInfoW,
        process_information: *mut ProcessInformationRaw,
    ) -> i32;
    fn ResumeThread(thread: RawHandle) -> u32;
    fn WaitForSingleObject(handle: RawHandle, milliseconds: u32) -> u32;
    fn WaitForMultipleObjects(
        count: u32,
        handles: *const RawHandle,
        wait_all: i32,
        milliseconds: u32,
    ) -> u32;
    fn GetExitCodeProcess(process: RawHandle, exit_code: *mut u32) -> i32;
    fn GetCurrentProcessId() -> u32;
    fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> RawHandle;
    fn TerminateProcess(process: RawHandle, exit_code: u32) -> i32;
    fn GetEnvironmentStringsW() -> *mut u16;
    fn FreeEnvironmentStringsW(penv: *mut u16) -> i32;
}

/// `OpenProcess`'s access-rights bit letting the returned handle be passed to
/// [`terminate`].
pub const PROCESS_TERMINATE: u32 = 0x0001;
/// `OpenProcess`'s access-rights bit letting the returned handle be passed to
/// [`wait`]/[`wait_any`] (a process handle must carry this right to be
/// waitable at all).
pub const SYNCHRONIZE: u32 = 0x0010_0000;

/// `WaitForMultipleObjects`'s own hard cap on `nCount` — a documented Win32
/// limit, not one this crate invents. [`wait_any`] passing more than this
/// reports [`Win32Error::ERROR_INVALID_PARAMETER`], the same code the real
/// call fails with, rather than this crate pre-validating and inventing its
/// own error for the same condition.
pub const MAXIMUM_WAIT_OBJECTS: usize = 64;

/// A process started by [`spawn_suspended`], still suspended until
/// [`resume`] is called on `thread`.
#[derive(Debug, Clone, Copy)]
pub struct SpawnedProcess {
    pub process: RawHandle,
    pub thread: RawHandle,
    pub process_id: u32,
    pub thread_id: u32,
}

/// Start `command_line` suspended (`CREATE_SUSPENDED`) — its main thread
/// does not run until [`resume`] is called on the returned
/// [`SpawnedProcess::thread`]. The gap between this call and `resume` is
/// exactly the window in which a caller assigns the process to a
/// [`crate::job`] object, so job membership is guaranteed before the
/// process (or any child it later spawns) executes a single instruction.
///
/// `inherit_handles` controls whether the child inherits the calling
/// process's currently-inheritable standard handles (see
/// [`crate::handle::set_inheritable`]) — there is no `STARTUPINFOW`
/// std-handle override here; redirect by swapping the parent's std-handle
/// slots before spawning, matching `winstdio`'s existing model in rush.
///
/// `environment` overrides the child's environment block; `None` inherits
/// the calling process's own environment unchanged (`CreateProcessW`'s
/// default when its `lpEnvironment` argument is null). A caller tracking
/// its own variable table separately from the real OS environment (rush's
/// `vars` module never calls `std::env::set_var`, so the two can diverge
/// after `export`/`unset`) needs `Some(..)` — build the block with
/// [`environment_block`] rather than hand-rolling it, since `CreateProcessW`
/// walks it by embedded NULs, not by a length this function could otherwise
/// validate.
///
/// # Safety
///
/// `command_line` must already be a valid, correctly-quoted Windows
/// command line for the target program (this function does no argument
/// quoting or escaping of its own). `environment`, if `Some`, must be a
/// well-formed Windows environment block — a sequence of NUL-terminated
/// `"NAME=value"` UTF-16 strings followed by one additional NUL — since
/// `CreateProcessW` reads it by scanning for that terminator, not by the
/// slice's own length; [`environment_block`] always produces one.
pub unsafe fn spawn_suspended(
    command_line: &str,
    inherit_handles: bool,
    environment: Option<&[u16]>,
) -> Result<SpawnedProcess, Win32Error> {
    // `CreateProcessW` is documented as possibly writing into this buffer
    // (e.g. inserting a terminating NUL if `lpApplicationName` is NULL and
    // `lpCommandLine`'s first token exceeds `MAX_PATH`), so a `&str`'s
    // read-only pointer isn't sufficient — this must be an owned, mutable
    // buffer.
    let mut wide: Vec<u16> = command_line
        .encode_utf16()
        .chain(core::iter::once(0))
        .collect();

    let mut startup_info = StartupInfoW {
        cb: core::mem::size_of::<StartupInfoW>() as u32,
        ..Default::default()
    };
    let mut process_info = ProcessInformationRaw::default();

    // `CREATE_UNICODE_ENVIRONMENT` is required whenever an explicit
    // environment block is passed — without it, `CreateProcessW` expects
    // an ANSI (8-bit) block instead and misreads ours.
    let creation_flags = if environment.is_some() {
        CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT
    } else {
        CREATE_SUSPENDED
    };
    let env_ptr = environment.map_or(core::ptr::null(), |e| {
        e.as_ptr().cast::<core::ffi::c_void>()
    });

    // SAFETY: `wide` is a valid, mutable, NUL-terminated UTF-16 buffer;
    // `startup_info`/`process_info` are valid, correctly-sized out
    // pointers with `cb` set as `CreateProcessW` requires; `env_ptr` is
    // either null (inherit) or a well-formed double-NUL-terminated block
    // per this function's own safety contract; every other pointer
    // argument is a documented-valid NULL (default security attributes,
    // no explicit application name/current directory override).
    let ok = unsafe {
        CreateProcessW(
            core::ptr::null(),
            wide.as_mut_ptr(),
            core::ptr::null(),
            core::ptr::null(),
            i32::from(inherit_handles),
            creation_flags,
            env_ptr,
            core::ptr::null(),
            &mut startup_info,
            &mut process_info,
        )
    };
    if ok == 0 {
        return Err(Win32Error::last());
    }
    Ok(SpawnedProcess {
        process: process_info.h_process,
        thread: process_info.h_thread,
        process_id: process_info.dw_process_id,
        thread_id: process_info.dw_thread_id,
    })
}

/// Build a Windows environment block for [`spawn_suspended`]'s
/// `environment` parameter: each `("NAME", "value")` pair encoded as a
/// NUL-terminated UTF-16 `"NAME=value"` string, back to back, with one
/// additional NUL terminating the whole block — the exact format
/// `CreateProcessW` requires when `CREATE_UNICODE_ENVIRONMENT` is set.
///
/// `vars` order is preserved as given; callers with a name appearing more
/// than once should dedupe first (`CreateProcessW`'s own behavior on a
/// block with a duplicate name is unspecified by its docs).
pub fn environment_block<'a>(vars: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<u16> {
    let mut block: Vec<u16> = Vec::new();
    for (name, value) in vars {
        block.extend(name.encode_utf16());
        block.push(u16::from(b'='));
        block.extend(value.encode_utf16());
        block.push(0);
    }
    block.push(0);
    if block.len() == 1 {
        // Zero variables: still terminate with two NULs total, not one —
        // documented Win32 behavior for an empty environment block.
        block.push(0);
    }
    block
}

/// Snapshot the calling process's real environment as
/// `(name, value)` pairs — `GetEnvironmentStringsW`, the read-back
/// counterpart to [`environment_block`]. Exists for a caller that needs to
/// *seed* its own variable table from the real inherited environment at
/// startup (unlike `spawn_suspended`'s `environment` parameter, which only
/// ever *writes* a block for a child) — a caller tracking its own table
/// separately from the OS environment (as `rush`'s `vars` module does)
/// needs exactly this once, before it starts tracking exports/unsets
/// itself.
///
/// Includes Windows' own `=C:=C:\path`-style hidden per-drive
/// current-directory entries (name `=C:`, `=D:`, ...) exactly as
/// `GetEnvironmentStringsW` reports them, rather than this wrapper
/// silently filtering them — deciding whether a caller's variable table
/// should carry these is the caller's policy, the same way this crate
/// exposes raw `FILE_ATTRIBUTE_*`/`ENABLE_*` bits without deciding what
/// they mean.
pub fn environment_snapshot()
-> Result<Vec<(alloc::string::String, alloc::string::String)>, Win32Error> {
    // SAFETY: `GetEnvironmentStringsW` takes no arguments; a `NULL` return
    // is its own documented (if practically unreachable) failure mode,
    // handled below rather than assumed away.
    let ptr = unsafe { GetEnvironmentStringsW() };
    if ptr.is_null() {
        return Err(Win32Error::last());
    }

    let mut pairs = Vec::new();
    let mut entry_start = 0usize;
    let mut i = 0usize;
    // SAFETY: `ptr` is the just-returned, valid pointer to a block
    // documented as a sequence of NUL-terminated UTF-16 strings ending in
    // one additional NUL; this walk reads only up through that terminator.
    unsafe {
        loop {
            if *ptr.add(i) == 0 {
                if i == entry_start {
                    // An empty entry marks the block's own end (the
                    // additional NUL after the last real "NAME=value").
                    break;
                }
                let entry = core::slice::from_raw_parts(ptr.add(entry_start), i - entry_start);
                if let Some(pair) = parse_environment_entry(entry) {
                    pairs.push(pair);
                }
                entry_start = i + 1;
            }
            i += 1;
        }
    }
    // SAFETY: `ptr` was returned by `GetEnvironmentStringsW` above and not
    // used again after this.
    unsafe { FreeEnvironmentStringsW(ptr) };
    Ok(pairs)
}

/// Split one `GetEnvironmentStringsW` entry (already known non-empty, no
/// terminating NUL included) into `(name, value)`. The search for `=`
/// deliberately skips index 0: Windows' own hidden `=C:=C:\path` per-drive
/// current-directory entries carry a leading `=` that's part of the name,
/// not a separator — the real, and only, separator is the *next* `=` after
/// that.
fn parse_environment_entry(
    units: &[u16],
) -> Option<(alloc::string::String, alloc::string::String)> {
    let equals_index = 1 + units.get(1..)?.iter().position(|&u| u == u16::from(b'='))?;
    Some((
        alloc::string::String::from_utf16_lossy(&units[..equals_index]),
        alloc::string::String::from_utf16_lossy(&units[equals_index + 1..]),
    ))
}

/// Resume a thread suspended by [`spawn_suspended`] (or any other
/// `CREATE_SUSPENDED`-started thread this crate hands back a handle to).
///
/// # Safety
///
/// `thread` must be a currently-open, valid thread handle.
pub unsafe fn resume(thread: RawHandle) -> Result<(), Win32Error> {
    // SAFETY: `thread` is caller-supplied per this function's own safety
    // contract; `ResumeThread` has no further precondition.
    let previous_suspend_count = unsafe { ResumeThread(thread) };
    if previous_suspend_count == u32::MAX {
        Err(Win32Error::last())
    } else {
        Ok(())
    }
}

/// Block on `process` for up to `timeout_ms` (`None` = wait forever, the
/// Windows analog of a blocking Unix `waitpid`). Returns `Some(exit_code)`
/// if the process exited within the timeout, `None` on timeout — call with
/// `Some(0)` for a non-blocking poll (the Windows analog of
/// `waitpid(WNOHANG)`).
///
/// # Safety
///
/// `process` must be a currently-open, valid process handle.
pub unsafe fn wait(process: RawHandle, timeout_ms: Option<u32>) -> Result<Option<u32>, Win32Error> {
    // SAFETY: `process` is caller-supplied per this function's own safety
    // contract.
    let result = unsafe { WaitForSingleObject(process, timeout_ms.unwrap_or(INFINITE)) };
    match result {
        WAIT_OBJECT_0 => {
            let mut code: u32 = 0;
            // SAFETY: `process` is the same valid handle just waited on;
            // `code` is a valid out-pointer.
            let ok = unsafe { GetExitCodeProcess(process, &mut code) };
            if ok == 0 {
                Err(Win32Error::last())
            } else {
                Ok(Some(code))
            }
        }
        WAIT_TIMEOUT => Ok(None),
        _ => Err(Win32Error::last()),
    }
}

/// Block on whichever of `processes` exits *first*, for up to `timeout_ms`
/// (`None` = wait forever) — the Windows analog of a blocking Unix
/// `waitpid(-1, ...)` restricted to a known set of pids, and the multi-handle
/// counterpart to [`wait`]. Returns `Some((index, exit_code))` naming the
/// first-signaled handle's position in `processes` and its exit code, or
/// `None` on timeout; call with `Some(0)` for a non-blocking poll across the
/// whole set at once, rather than looping [`wait`] over each handle in turn.
///
/// `processes` must be non-empty and no longer than
/// [`MAXIMUM_WAIT_OBJECTS`] — `WaitForMultipleObjects`'s own documented
/// limit on how many handles a single call accepts; passing more (or zero)
/// reports [`Win32Error::ERROR_INVALID_PARAMETER`], the same failure the raw
/// call itself would report, not a distinct error this crate invents.
///
/// If more than one handle is already signaled at the moment of the call,
/// which index comes back is `WaitForMultipleObjects`'s own documented
/// choice (the lowest signaled index), not something this wrapper adds
/// logic for.
///
/// # Safety
///
/// Every handle in `processes` must be currently-open and valid.
pub unsafe fn wait_any(
    processes: &[RawHandle],
    timeout_ms: Option<u32>,
) -> Result<Option<(usize, u32)>, Win32Error> {
    // SAFETY: `processes` is a caller-supplied slice of valid handles per
    // this function's own safety contract; `processes.as_ptr()`/`.len()`
    // describe that same slice, a valid input `WaitForMultipleObjects`
    // documents (including reporting `ERROR_INVALID_PARAMETER` itself for
    // an empty or oversized one, rather than this wrapper pre-checking).
    let result = unsafe {
        WaitForMultipleObjects(
            processes.len() as u32,
            processes.as_ptr(),
            0,
            timeout_ms.unwrap_or(INFINITE),
        )
    };
    const WAIT_FAILED: u32 = 0xFFFF_FFFF;
    match result {
        WAIT_TIMEOUT => Ok(None),
        WAIT_FAILED => Err(Win32Error::last()),
        index if (index as usize) < processes.len() => {
            let process = processes[index as usize];
            let mut code: u32 = 0;
            // SAFETY: `process` is the same valid handle just signaled;
            // `code` is a valid out-pointer.
            let ok = unsafe { GetExitCodeProcess(process, &mut code) };
            if ok == 0 {
                Err(Win32Error::last())
            } else {
                Ok(Some((index as usize, code)))
            }
        }
        // A signaled-index return outside `0..processes.len()` only
        // happens for the abandoned-mutex range (`WAIT_ABANDONED_0..`),
        // which can't occur for process handles — process objects are
        // never abandoned the way a mutex is. Reported as the raw code
        // rather than silently treated as a timeout or a panic.
        other => Err(Win32Error::from_raw(other)),
    }
}

/// The calling process's own pid — the Windows analog of Unix `getpid`.
pub fn current_pid() -> u32 {
    // SAFETY: `GetCurrentProcessId` takes no arguments and has no
    // preconditions.
    unsafe { GetCurrentProcessId() }
}

/// Open a handle to the process named by `pid` — `OpenProcess`, needed for
/// `kill <pid>` on a pid a caller only knows numerically (e.g. read back
/// from `jobs`/`$!`), not one of this crate's own [`SpawnedProcess`] handles
/// from [`spawn_suspended`]. `desired_access` should be the narrowest set of
/// rights the caller actually needs — [`PROCESS_TERMINATE`] alone for a
/// handle that will only ever be passed to [`terminate`], or
/// `PROCESS_TERMINATE | SYNCHRONIZE` for one that will also be passed to
/// [`wait`]/[`wait_any`].
pub fn open_by_pid(pid: u32, desired_access: u32) -> Result<RawHandle, Win32Error> {
    // SAFETY: `desired_access` is a plain access-rights bitmask, not a
    // pointer; `inherit_handle = FALSE` (0) is a documented valid input;
    // `pid` is caller-supplied and `OpenProcess` itself reports an unknown
    // or inaccessible one as an ordinary `NULL`/`GetLastError` failure, not
    // undefined behavior.
    let handle = unsafe { OpenProcess(desired_access, 0, pid) };
    if handle.is_null() {
        Err(Win32Error::last())
    } else {
        Ok(handle)
    }
}

/// Terminate `process` with `exit_code` — `TerminateProcess`, the
/// single-process counterpart to [`crate::job::terminate`] (which kills
/// every process in a job at once). Needed for `kill <pid>` against a
/// process this crate didn't itself spawn into a job — one opened via
/// [`open_by_pid`] instead.
///
/// # Safety
///
/// `process` must be a currently-open, valid process handle with the
/// `PROCESS_TERMINATE` access right.
pub unsafe fn terminate(process: RawHandle, exit_code: u32) -> Result<(), Win32Error> {
    // SAFETY: `process` is caller-supplied per this function's own safety
    // contract; `exit_code` is a plain value, not a pointer.
    let ok = unsafe { TerminateProcess(process, exit_code) };
    if ok == 0 {
        Err(Win32Error::last())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_suspended_resume_wait_round_trip() {
        // The handoff doc's own suggested first proof: spawn a real
        // command, confirm the exit code round-trips.
        // SAFETY: a hand-built, correctly quoted command line for a
        // well-known system binary.
        let spawned = unsafe { spawn_suspended("cmd.exe /c exit 7", false, None) }
            .expect("CreateProcessW should succeed");
        assert_ne!(spawned.process_id, 0);
        assert_ne!(spawned.thread_id, 0);

        // Still suspended: a zero-timeout wait must time out, not report
        // an exit — proves CREATE_SUSPENDED actually held the thread.
        // SAFETY: `spawned.process` is a freshly created, valid handle.
        let still_suspended = unsafe { wait(spawned.process, Some(0)) }.unwrap();
        assert_eq!(still_suspended, None);

        // SAFETY: `spawned.thread` is a freshly created, valid,
        // not-yet-resumed thread handle.
        unsafe { resume(spawned.thread) }.expect("ResumeThread should succeed");

        // SAFETY: `spawned.process` is a valid, currently-open handle.
        let exit_code = unsafe { wait(spawned.process, None) }.unwrap();
        assert_eq!(exit_code, Some(7));

        // SAFETY: both handles are valid and each closed exactly once.
        unsafe {
            crate::handle::close(spawned.process).unwrap();
            crate::handle::close(spawned.thread).unwrap();
        }
    }

    #[test]
    fn wait_any_reports_the_first_process_to_exit() {
        // Two processes, both started suspended; only one is ever resumed,
        // so which one `wait_any` reports is deterministic rather than a
        // race between two real running processes.
        // SAFETY: hand-built, correctly quoted command lines for a
        // well-known system binary.
        let a = unsafe { spawn_suspended("cmd.exe /c exit 3", false, None) }
            .expect("CreateProcessW should succeed");
        let b = unsafe { spawn_suspended("cmd.exe /c exit 9", false, None) }
            .expect("CreateProcessW should succeed");

        // SAFETY: `b.thread` is freshly created, valid, not yet resumed.
        unsafe { resume(b.thread) }.expect("ResumeThread should succeed");

        // SAFETY: both process handles are valid, currently-open, and
        // distinct.
        let (index, code) = unsafe { wait_any(&[a.process, b.process], None) }
            .unwrap()
            .expect("one process should have exited");
        assert_eq!(index, 1, "expected b (index 1), the only resumed process");
        assert_eq!(code, 9);

        // `a` was never resumed — resume it now so its own exit code (and
        // the still-suspended thread) don't leak into the test process
        // list, then clean up every handle.
        // SAFETY: `a.thread` is freshly created, valid, not yet resumed.
        unsafe { resume(a.thread) }.expect("ResumeThread should succeed");
        // SAFETY: `a.process` is a valid, currently-open handle.
        unsafe { wait(a.process, None) }.unwrap();
        // SAFETY: every handle here is valid and each closed exactly once.
        unsafe {
            crate::handle::close(a.process).unwrap();
            crate::handle::close(a.thread).unwrap();
            crate::handle::close(b.process).unwrap();
            crate::handle::close(b.thread).unwrap();
        }
    }

    #[test]
    fn wait_any_times_out_when_nothing_has_exited() {
        // SAFETY: a hand-built, correctly quoted command line for a
        // well-known system binary.
        let spawned = unsafe { spawn_suspended("cmd.exe /c exit 0", false, None) }
            .expect("CreateProcessW should succeed");

        // Still suspended, so a zero-timeout wait_any must time out.
        // SAFETY: `spawned.process` is a freshly created, valid handle.
        let timed_out = unsafe { wait_any(&[spawned.process], Some(0)) }.unwrap();
        assert_eq!(timed_out, None);

        // SAFETY: `spawned.thread` is freshly created, valid, not yet
        // resumed.
        unsafe { resume(spawned.thread) }.expect("ResumeThread should succeed");
        // SAFETY: `spawned.process` is a valid, currently-open handle.
        unsafe { wait(spawned.process, None) }.unwrap();
        // SAFETY: both handles are valid and each closed exactly once.
        unsafe {
            crate::handle::close(spawned.process).unwrap();
            crate::handle::close(spawned.thread).unwrap();
        }
    }

    #[test]
    fn wait_any_rejects_an_empty_slice() {
        // Matches `WaitForMultipleObjects`'s own documented behavior for
        // `nCount == 0` — this wrapper doesn't pre-validate and invent a
        // distinct error for the same condition.
        let err = unsafe { wait_any(&[], None) }.unwrap_err();
        assert_eq!(err, Win32Error::ERROR_INVALID_PARAMETER);
    }

    #[test]
    fn current_pid_is_nonzero() {
        assert_ne!(current_pid(), 0);
    }

    #[test]
    fn open_by_pid_then_terminate_kills_a_process_known_only_by_pid() {
        // The scenario this pair exists for: a pid read back numerically
        // (e.g. from `jobs`/`$!`), with no `SpawnedProcess` handle in hand —
        // open a fresh handle from just the pid, terminate through *that*
        // handle, and confirm the original process handle still reports the
        // resulting exit code.
        // SAFETY: a hand-built, correctly quoted command line for a
        // well-known long-running system command.
        let spawned =
            unsafe { spawn_suspended("cmd.exe /c ping -n 30 127.0.0.1 >nul", false, None) }
                .expect("CreateProcessW should succeed");
        // SAFETY: `spawned.thread` is freshly created, valid, not yet
        // resumed.
        unsafe { resume(spawned.thread) }.expect("ResumeThread should succeed");

        let opened = open_by_pid(spawned.process_id, PROCESS_TERMINATE | SYNCHRONIZE)
            .expect("OpenProcess should succeed for a live pid this test itself just started");
        assert_ne!(
            opened, spawned.process,
            "OpenProcess should hand back an independent handle value, not the original one"
        );

        // SAFETY: `opened` is a freshly opened, valid handle with
        // PROCESS_TERMINATE.
        unsafe { terminate(opened, 42) }.expect("TerminateProcess should succeed");

        // SAFETY: `spawned.process` is still a valid handle — terminating
        // via a *different* handle to the same process doesn't invalidate
        // it, only the process itself.
        let exit = unsafe { wait(spawned.process, Some(5_000)) }.unwrap();
        assert_eq!(exit, Some(42));

        // SAFETY: every handle here is valid and each closed exactly once.
        unsafe {
            crate::handle::close(opened).unwrap();
            crate::handle::close(spawned.process).unwrap();
            crate::handle::close(spawned.thread).unwrap();
        }
    }

    #[test]
    fn open_by_pid_fails_for_pid_zero() {
        // Pid 0 (the System Idle Process) is documented to never be
        // openable via `OpenProcess` — a stable, deterministic "this pid
        // does not resolve to an openable process" case to test against,
        // unlike an arbitrary made-up pid that could coincidentally collide
        // with something real on a given machine.
        assert!(open_by_pid(0, PROCESS_TERMINATE).is_err());
    }

    #[test]
    fn spawn_suspended_with_environment_overrides_the_childs_view() {
        // A minimal environment block containing exactly one variable: if
        // it reaches the child, `if defined` sees it and the process exits
        // 42; if `environment` were silently ignored (e.g. the flag/pointer
        // wiring regressed), the child would see none of our variables and
        // exit 1 instead — the round trip a plain "does it compile" check
        // wouldn't catch.
        let block = environment_block(core::iter::once(("RUSTY_WIN32_TEST_VAR", "1")));
        // SAFETY: a hand-built, correctly quoted command line for a
        // well-known system binary; `block` was built by
        // `environment_block`, which always double-NUL-terminates.
        let spawned = unsafe {
            spawn_suspended(
                "cmd.exe /c if defined RUSTY_WIN32_TEST_VAR (exit 42) else (exit 1)",
                false,
                Some(&block),
            )
        }
        .expect("CreateProcessW should succeed");

        // SAFETY: `spawned.thread` is freshly created, not yet resumed.
        unsafe { resume(spawned.thread) }.expect("ResumeThread should succeed");
        // SAFETY: `spawned.process` is a valid, currently-open handle.
        let exit_code = unsafe { wait(spawned.process, None) }.unwrap();
        assert_eq!(exit_code, Some(42));

        // SAFETY: both handles are valid and each closed exactly once.
        unsafe {
            crate::handle::close(spawned.process).unwrap();
            crate::handle::close(spawned.thread).unwrap();
        }
    }

    #[test]
    fn environment_block_is_double_nul_terminated() {
        let block = environment_block(core::iter::empty());
        assert_eq!(block, alloc::vec![0u16, 0u16]);

        let block = environment_block([("A", "1"), ("B", "2")].into_iter());
        let text: alloc::string::String =
            char::decode_utf16(block[..block.len() - 1].iter().copied())
                .map(|r| r.unwrap())
                .collect();
        assert_eq!(text, "A=1\0B=2\0");
        assert_eq!(block.last(), Some(&0u16));
    }

    #[test]
    fn environment_snapshot_includes_a_well_known_system_variable() {
        let pairs = environment_snapshot().expect("GetEnvironmentStringsW should succeed");
        assert!(
            pairs
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("SystemRoot")),
            "every real Windows process should have SystemRoot in its environment"
        );
    }

    #[test]
    fn environment_snapshot_includes_a_variable_this_process_just_set() {
        // SAFETY: this crate's CI runs its test suite with
        // RUST_TEST_THREADS=1 (see .github/workflows/ci.yml), so no other
        // test can concurrently read/write the real environment while this
        // one does.
        unsafe { std::env::set_var("RUSTY_WIN32_ENV_SNAPSHOT_TEST", "hello") };

        let pairs = environment_snapshot().expect("GetEnvironmentStringsW should succeed");
        let found = pairs
            .iter()
            .find(|(name, _)| name == "RUSTY_WIN32_ENV_SNAPSHOT_TEST");
        assert_eq!(
            found.map(|(_, value)| value.as_str()),
            Some("hello"),
            "the snapshot should include a variable this process just set"
        );

        // SAFETY: see above.
        unsafe { std::env::remove_var("RUSTY_WIN32_ENV_SNAPSHOT_TEST") };
    }
}
