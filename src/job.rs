//! Windows Job Objects — the primitive rush's own
//! `docs/WINDOWS_JOB_CONTROL.md` designs background-job tracking against
//! (`&`, `jobs`, `wait`, `kill`, `$!`). A Job Object groups processes for
//! lifetime management, the closest Windows analog of a POSIX process group
//! for this purpose (not a perfect match — see that design doc's
//! "Terminology" note). Assigning a suspended process (see
//! [`crate::process::spawn_suspended`]) to a job *before* resuming its main
//! thread, with [`set_kill_on_close`] applied, means closing the job handle
//! (e.g. on shell exit) kills every process in it — the Windows analog of
//! the process-group-wide cleanup rush's Unix `job.rs` gets from signals.
//! Any child the job's own member processes spawn inherits job membership
//! automatically (a job property, not something a child opts into), which
//! is what makes this correct for a whole subtree, not just one process.
//!
//! [`create_completion_port`]/[`associate_completion_port`]/
//! [`wait_for_message`] close a real gap in the above: [`process_ids`] is
//! only ever a poll, never a push — there is no Unix-`SIGCHLD` equivalent
//! for "a job member just exited" without them. Associating a job with an
//! I/O completion port (a mechanism otherwise entirely about file I/O, not
//! process lifecycle — Windows repurposes it here rather than defining a
//! job-specific notification primitive) makes the OS itself post a message
//! every time a member process is created, exits, or the job empties out,
//! which [`wait_for_message`] blocks for. As with every other primitive in
//! this crate, deciding what a caller does with a given [`JOB_OBJECT_MSG_EXIT_PROCESS`]
//! (etc.) is the caller's policy — this module only delivers the raw message.

use crate::error::Win32Error;
use crate::handle::RawHandle;

extern crate alloc;
use alloc::vec::Vec;

/// `SetInformationJobObject`'s `LimitFlags` bit: close the job handle (or
/// have every handle to it close, e.g. on process exit) and every process
/// still assigned to it terminates.
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

/// `QueryInformationJobObject`/`SetInformationJobObject`'s
/// `JobObjectExtendedLimitInformation` class.
const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: u32 = 9;
/// `QueryInformationJobObject`'s `JobObjectBasicProcessIdList` class.
const JOB_OBJECT_BASIC_PROCESS_ID_LIST_CLASS: u32 = 3;
/// `SetInformationJobObject`'s `JobObjectAssociateCompletionPortInformation`
/// class.
const JOB_OBJECT_ASSOCIATE_COMPLETION_PORT_CLASS: u32 = 7;

/// `GetQueuedCompletionStatus`'s own documented "no packet queued within the
/// timeout" code, surfaced via `GetLastError()` on a `FALSE` return — the
/// same numeric value `process::wait`/`console::wait_readable` already
/// compare against locally (not one of `error.rs`'s named `ERROR_*`
/// constants, since it's a wait-outcome code reused as an error rather than
/// an ordinary `GetLastError` failure).
const WAIT_TIMEOUT: u32 = 258;

/// A member process was created — includes the job's own initial process.
pub const JOB_OBJECT_MSG_NEW_PROCESS: u32 = 6;
/// A member process exited normally. `pid` names which one.
pub const JOB_OBJECT_MSG_EXIT_PROCESS: u32 = 7;
/// A member process exited abnormally (e.g. crashed, or was terminated).
pub const JOB_OBJECT_MSG_ABNORMAL_EXIT_PROCESS: u32 = 8;
/// The job has no member processes left — the clearest "this job is done"
/// signal, the nearest analog to a Unix process group with no members left.
pub const JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO: u32 = 4;
/// The job as a whole exceeded its total user-mode CPU time limit (only
/// meaningful if such a limit was set, which this crate doesn't currently
/// expose a way to set).
pub const JOB_OBJECT_MSG_END_OF_JOB_TIME: u32 = 1;
/// A single process exceeded its per-process user-mode CPU time limit (same
/// caveat as [`JOB_OBJECT_MSG_END_OF_JOB_TIME`]).
pub const JOB_OBJECT_MSG_END_OF_PROCESS_TIME: u32 = 2;
/// The job's active-process-count limit was exceeded (same caveat).
pub const JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT: u32 = 3;
/// A process tried to exceed its memory limit (same caveat).
pub const JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT: u32 = 9;
/// The job as a whole tried to exceed its memory limit (same caveat).
pub const JOB_OBJECT_MSG_JOB_MEMORY_LIMIT: u32 = 10;

// Layouts below verified against mingw-w64's `winnt.h` (`_IO_COUNTERS`,
// `_JOBOBJECT_BASIC_LIMIT_INFORMATION`, `_JOBOBJECT_EXTENDED_LIMIT_INFORMATION`,
// `_JOBOBJECT_BASIC_PROCESS_ID_LIST`) the same way as `process.rs`'s
// structs: a `_Static_assert`-based probe compiled against the real header,
// not hand-computed padding.

/// `IO_COUNTERS`: `size_of` 48, `align_of` 8 on x86_64.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct IoCounters {
    read_operation_count: u64,
    write_operation_count: u64,
    other_operation_count: u64,
    read_transfer_count: u64,
    write_transfer_count: u64,
    other_transfer_count: u64,
}
const _: () = assert!(core::mem::size_of::<IoCounters>() == 48);
const _: () = assert!(core::mem::align_of::<IoCounters>() == 8);

/// `JOBOBJECT_BASIC_LIMIT_INFORMATION`: `size_of` 64, `align_of` 8.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct JobObjectBasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}
const _: () = assert!(core::mem::size_of::<JobObjectBasicLimitInformation>() == 64);
const _: () = assert!(core::mem::align_of::<JobObjectBasicLimitInformation>() == 8);
const _: () = assert!(core::mem::offset_of!(JobObjectBasicLimitInformation, limit_flags) == 16);

/// `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`: `size_of` 144, `align_of` 8.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct JobObjectExtendedLimitInformation {
    basic_limit_information: JobObjectBasicLimitInformation,
    io_info: IoCounters,
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}
const _: () = assert!(core::mem::size_of::<JobObjectExtendedLimitInformation>() == 144);
const _: () = assert!(core::mem::align_of::<JobObjectExtendedLimitInformation>() == 8);
const _: () = assert!(core::mem::offset_of!(JobObjectExtendedLimitInformation, io_info) == 64);
const _: () =
    assert!(core::mem::offset_of!(JobObjectExtendedLimitInformation, process_memory_limit) == 112);

/// `JOBOBJECT_ASSOCIATE_COMPLETION_PORT`: `size_of` 16, `align_of` 8 on
/// x86_64 — two pointer-sized fields, no padding.
#[repr(C)]
#[derive(Clone, Copy)]
struct JobObjectAssociateCompletionPort {
    completion_key: *mut core::ffi::c_void,
    completion_port: RawHandle,
}
const _: () = assert!(core::mem::size_of::<JobObjectAssociateCompletionPort>() == 16);
const _: () = assert!(core::mem::align_of::<JobObjectAssociateCompletionPort>() == 8);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(job_attributes: *const core::ffi::c_void, name: *const u16) -> RawHandle;
    fn AssignProcessToJobObject(job: RawHandle, process: RawHandle) -> i32;
    fn TerminateJobObject(job: RawHandle, exit_code: u32) -> i32;
    fn SetInformationJobObject(
        job: RawHandle,
        job_object_information_class: u32,
        job_object_information: *const core::ffi::c_void,
        job_object_information_length: u32,
    ) -> i32;
    fn QueryInformationJobObject(
        job: RawHandle,
        job_object_information_class: u32,
        job_object_information: *mut core::ffi::c_void,
        job_object_information_length: u32,
        return_length: *mut u32,
    ) -> i32;
    fn CreateIoCompletionPort(
        file_handle: RawHandle,
        existing_completion_port: RawHandle,
        completion_key: usize,
        number_of_concurrent_threads: u32,
    ) -> RawHandle;
    fn GetQueuedCompletionStatus(
        completion_port: RawHandle,
        bytes_transferred: *mut u32,
        completion_key: *mut usize,
        overlapped: *mut *mut core::ffi::c_void,
        milliseconds: u32,
    ) -> i32;
}

/// Create a new, anonymous Job Object — one shell background job's worth of
/// process-group-equivalent tracking. No process is assigned yet; pair with
/// [`assign`] (before resuming a suspended process — see
/// [`crate::process::spawn_suspended`]) and, almost always,
/// [`set_kill_on_close`].
pub fn create() -> Result<RawHandle, Win32Error> {
    // SAFETY: both arguments are documented-valid NULLs (default security
    // attributes, anonymous/unnamed job).
    let job = unsafe { CreateJobObjectW(core::ptr::null(), core::ptr::null()) };
    if job.is_null() {
        Err(Win32Error::last())
    } else {
        Ok(job)
    }
}

/// Set `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` on `job`: every process still
/// assigned to it terminates when its last handle closes, e.g. via
/// [`crate::handle::close`] on shell exit — the Windows analog of the
/// process-group-wide cleanup rush's Unix `job.rs` gets from signals.
///
/// # Safety
///
/// `job` must be a currently-open, valid Job Object handle.
pub unsafe fn set_kill_on_close(job: RawHandle) -> Result<(), Win32Error> {
    let info = JobObjectExtendedLimitInformation {
        basic_limit_information: JobObjectBasicLimitInformation {
            limit_flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: `job` is caller-supplied per this function's own safety
    // contract; `info` is a valid, correctly-sized, initialized struct of
    // exactly the type `JobObjectExtendedLimitInformation` names.
    let ok = unsafe {
        SetInformationJobObject(
            job,
            JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
            (&info as *const JobObjectExtendedLimitInformation).cast(),
            core::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
        )
    };
    if ok == 0 {
        Err(Win32Error::last())
    } else {
        Ok(())
    }
}

/// Clear `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` on `job` (the reverse of
/// [`set_kill_on_close`]): its member processes survive every handle to
/// `job` closing, including implicitly at this process's own exit —
/// backs `disown`. Without this, there's no way to let a tracked job
/// outlive the shell: a job handle is closed exactly like any other
/// handle when its owning process terminates, so kill-on-close would
/// still fire even if a caller simply stopped tracking the job and closed
/// its own handle to it, or just exited without closing anything at all.
///
/// # Safety
///
/// `job` must be a currently-open, valid Job Object handle.
pub unsafe fn clear_kill_on_close(job: RawHandle) -> Result<(), Win32Error> {
    let info = JobObjectExtendedLimitInformation::default();
    // SAFETY: `job` is caller-supplied per this function's own safety
    // contract; `info` is a valid, correctly-sized, initialized struct of
    // exactly the type `JobObjectExtendedLimitInformation` names.
    let ok = unsafe {
        SetInformationJobObject(
            job,
            JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
            (&info as *const JobObjectExtendedLimitInformation).cast(),
            core::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
        )
    };
    if ok == 0 {
        Err(Win32Error::last())
    } else {
        Ok(())
    }
}

/// Assign `process` to `job`. Any child `process` itself later spawns
/// inherits job membership automatically. Must happen before a
/// `CREATE_SUSPENDED`-started `process` is resumed (see
/// [`crate::process::spawn_suspended`]/[`crate::process::resume`]) for job
/// membership to be guaranteed before anything in the process tree runs.
///
/// # Safety
///
/// `job` and `process` must both be currently-open, valid handles.
pub unsafe fn assign(job: RawHandle, process: RawHandle) -> Result<(), Win32Error> {
    // SAFETY: both handles are caller-supplied per this function's own
    // safety contract.
    let ok = unsafe { AssignProcessToJobObject(job, process) };
    if ok == 0 {
        Err(Win32Error::last())
    } else {
        Ok(())
    }
}

/// Terminate every process currently assigned to `job` with `exit_code` —
/// backs `kill %n`. Unlike closing the job handle with
/// [`set_kill_on_close`] set, this doesn't require giving up the handle.
///
/// # Safety
///
/// `job` must be a currently-open, valid Job Object handle.
pub unsafe fn terminate(job: RawHandle, exit_code: u32) -> Result<(), Win32Error> {
    // SAFETY: `job` is caller-supplied per this function's own safety
    // contract.
    let ok = unsafe { TerminateJobObject(job, exit_code) };
    if ok == 0 {
        Err(Win32Error::last())
    } else {
        Ok(())
    }
}

/// The pids of every process currently assigned to `job` — one way to poll
/// "is this job still running" (an empty result) for `wait`/`jobs`, in the
/// absence of a blocking wait-for-job-completion primitive as simple as
/// Unix `SIGCHLD` (see rush's `docs/WINDOWS_JOB_CONTROL.md` for the
/// polling-vs-completion-port discussion).
///
/// # Safety
///
/// `job` must be a currently-open, valid Job Object handle.
pub unsafe fn process_ids(job: RawHandle) -> Result<Vec<usize>, Win32Error> {
    const HEADER_LEN: usize = 8; // NumberOfAssignedProcesses + NumberOfProcessIdsInList
    let mut capacity: u32 = 32;
    loop {
        let buf_len = HEADER_LEN + capacity as usize * core::mem::size_of::<usize>();
        let mut buf: Vec<u8> = alloc::vec![0u8; buf_len];
        let mut returned_len: u32 = 0;
        // SAFETY: `job` is caller-supplied per this function's own safety
        // contract; `buf` is a valid, zeroed, `buf_len`-byte buffer;
        // `returned_len` is a valid out-pointer.
        let ok = unsafe {
            QueryInformationJobObject(
                job,
                JOB_OBJECT_BASIC_PROCESS_ID_LIST_CLASS,
                buf.as_mut_ptr().cast(),
                buf_len as u32,
                &mut returned_len,
            )
        };
        if ok == 0 {
            let err = Win32Error::last();
            if err == Win32Error::ERROR_MORE_DATA {
                // NumberOfAssignedProcesses is documented to be filled in
                // correctly even when the list itself didn't fit.
                // SAFETY: `buf` holds at least `HEADER_LEN` initialized
                // bytes even on this "too small" failure path.
                let assigned = unsafe { core::ptr::read_unaligned(buf.as_ptr().cast::<u32>()) };
                capacity = assigned.max(capacity * 2);
                continue;
            }
            return Err(err);
        }
        // SAFETY: a successful call guarantees the header and
        // `number_of_process_ids_in_list` pid-sized entries after it are
        // initialized.
        let in_list =
            unsafe { core::ptr::read_unaligned(buf.as_ptr().add(4).cast::<u32>()) } as usize;
        let pids_ptr = unsafe { buf.as_ptr().add(HEADER_LEN).cast::<usize>() };
        let pids = unsafe { core::slice::from_raw_parts(pids_ptr, in_list) };
        return Ok(pids.to_vec());
    }
}

/// Create a fresh I/O completion port not yet tied to any file or job —
/// `CreateIoCompletionPort` with no file handle to associate yet
/// (`INVALID_HANDLE_VALUE`) and no existing port to attach to. Pair with
/// [`associate_completion_port`] to start receiving a job's messages on it,
/// and [`wait_for_message`] to block for one.
pub fn create_completion_port() -> Result<RawHandle, Win32Error> {
    // `INVALID_HANDLE_VALUE` — all bits set, the documented sentinel
    // `CreateIoCompletionPort` requires in `FileHandle` when creating a new,
    // standalone port rather than associating an existing file/device
    // handle with one.
    let invalid_handle_value = usize::MAX as RawHandle;
    // SAFETY: `invalid_handle_value` is the documented sentinel above, not a
    // real handle being dereferenced; `existing_completion_port = NULL` and
    // `completion_key = 0` are the documented inputs for "create a new
    // port"; `number_of_concurrent_threads = 1` is a plain value (this
    // crate's callers poll `wait_for_message` from a single thread).
    let port = unsafe { CreateIoCompletionPort(invalid_handle_value, core::ptr::null_mut(), 0, 1) };
    if port.is_null() {
        Err(Win32Error::last())
    } else {
        Ok(port)
    }
}

/// Associate `job` with `completion_port`: from this call on, the OS posts a
/// message to the port every time a member process is created or exits, or
/// the job empties out — the push counterpart to [`process_ids`]'s poll,
/// and this crate's closest analog to Unix `SIGCHLD` for job membership
/// changes. `completion_key` is an arbitrary caller-chosen value echoed back
/// by [`wait_for_message`] unchanged, letting a caller sharing one port
/// across multiple jobs tell which job a given message came from.
///
/// # Safety
///
/// `job` and `completion_port` must both be currently-open, valid handles
/// (`completion_port` from [`create_completion_port`]).
pub unsafe fn associate_completion_port(
    job: RawHandle,
    completion_port: RawHandle,
    completion_key: usize,
) -> Result<(), Win32Error> {
    let info = JobObjectAssociateCompletionPort {
        completion_key: completion_key as *mut core::ffi::c_void,
        completion_port,
    };
    // SAFETY: `job` and `completion_port` are caller-supplied per this
    // function's own safety contract; `info` is a valid, correctly-sized,
    // initialized struct of exactly the type
    // `JobObjectAssociateCompletionPortInformation` names.
    let ok = unsafe {
        SetInformationJobObject(
            job,
            JOB_OBJECT_ASSOCIATE_COMPLETION_PORT_CLASS,
            (&info as *const JobObjectAssociateCompletionPort).cast(),
            core::mem::size_of::<JobObjectAssociateCompletionPort>() as u32,
        )
    };
    if ok == 0 {
        Err(Win32Error::last())
    } else {
        Ok(())
    }
}

/// One job-lifecycle notification — `message` is one of the
/// `JOB_OBJECT_MSG_*` constants (e.g. [`JOB_OBJECT_MSG_EXIT_PROCESS`]),
/// `pid` is the process it concerns (meaningful for the per-process
/// messages; `0` for job-wide ones like [`JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO`],
/// which carry no specific pid), and `completion_key` echoes back whatever
/// [`associate_completion_port`] was called with for the job this message
/// came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobMessage {
    pub completion_key: usize,
    pub message: u32,
    pub pid: u32,
}

/// Block on `completion_port` for up to `timeout_ms` (`None` = wait
/// forever), returning the next queued job-lifecycle message —
/// `GetQueuedCompletionStatus`. Returns `Some(message)` if one arrived
/// within the timeout, `None` on timeout — call with `Some(0)` for a
/// non-blocking poll, matching [`crate::process::wait`]'s own `Option<u32>`
/// timeout convention.
///
/// For job notifications specifically, Windows repurposes
/// `GetQueuedCompletionStatus`'s `lpOverlapped` out-parameter (ordinarily a
/// real `OVERLAPPED` pointer for file I/O) to instead carry the process ID
/// as a plain integer value — a documented quirk of this API being reused
/// for job notifications rather than its original file-I/O purpose, not
/// something this wrapper invents.
///
/// # Safety
///
/// `completion_port` must be a currently-open, valid I/O completion port
/// handle (from [`create_completion_port`]).
pub unsafe fn wait_for_message(
    completion_port: RawHandle,
    timeout_ms: Option<u32>,
) -> Result<Option<JobMessage>, Win32Error> {
    const INFINITE: u32 = 0xFFFF_FFFF;

    let mut bytes_transferred: u32 = 0;
    let mut completion_key: usize = 0;
    let mut overlapped: *mut core::ffi::c_void = core::ptr::null_mut();
    // SAFETY: `completion_port` is caller-supplied per this function's own
    // safety contract; `bytes_transferred`/`completion_key`/`overlapped` are
    // valid out-pointers to correctly-typed locals.
    let ok = unsafe {
        GetQueuedCompletionStatus(
            completion_port,
            &mut bytes_transferred,
            &mut completion_key,
            &mut overlapped,
            timeout_ms.unwrap_or(INFINITE),
        )
    };
    if ok == 0 {
        let err = Win32Error::last();
        return if err.code() == WAIT_TIMEOUT {
            Ok(None)
        } else {
            Err(err)
        };
    }
    Ok(Some(JobMessage {
        completion_key,
        message: bytes_transferred,
        pid: overlapped as usize as u32,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process;

    #[test]
    fn create_terminate_and_kill_on_close_round_trip() {
        let job = create().expect("CreateJobObjectW should succeed");
        // SAFETY: `job` is freshly created and valid.
        unsafe { set_kill_on_close(job).expect("SetInformationJobObject should succeed") };

        // SAFETY: a hand-built, correctly quoted command line for a
        // well-known long-running system command — this test terminates it
        // via the job before it would exit on its own.
        let spawned = unsafe {
            process::spawn_suspended("cmd.exe /c ping -n 30 127.0.0.1 >nul", true, false, None)
        }
        .expect("CreateProcessW should succeed");

        // SAFETY: `job`/`spawned.process` are both freshly created, valid
        // handles; assignment happens before `resume`, so job membership is
        // guaranteed before the process runs.
        unsafe { assign(job, spawned.process).expect("AssignProcessToJobObject should succeed") };
        // SAFETY: `spawned.thread` is a freshly created, valid,
        // not-yet-resumed thread handle.
        unsafe { process::resume(spawned.thread).expect("ResumeThread should succeed") };

        // SAFETY: `job` is a valid handle with exactly one assigned process.
        let ids_before =
            unsafe { process_ids(job) }.expect("QueryInformationJobObject should succeed");
        assert_eq!(ids_before, alloc::vec![spawned.process_id as usize]);

        // SAFETY: `job` is a valid handle; this is the operation under
        // test.
        unsafe { terminate(job, 1).expect("TerminateJobObject should succeed") };

        // SAFETY: `spawned.process` is still a valid handle — terminating
        // via the job doesn't invalidate the handle, only the process.
        let exit = unsafe { process::wait(spawned.process, Some(5_000)) }.unwrap();
        assert_eq!(exit, Some(1));

        // SAFETY: both handles are valid and each closed exactly once.
        unsafe {
            crate::handle::close(spawned.process).unwrap();
            crate::handle::close(spawned.thread).unwrap();
            crate::handle::close(job).unwrap();
        }
    }

    #[test]
    fn clear_kill_on_close_lets_the_process_survive_closing_the_job_handle() {
        let job = create().expect("CreateJobObjectW should succeed");
        // SAFETY: `job` is freshly created and valid.
        unsafe { set_kill_on_close(job).expect("SetInformationJobObject should succeed") };

        // SAFETY: a hand-built, correctly quoted command line for a
        // well-known system binary.
        let spawned = unsafe { process::spawn_suspended("cmd.exe /c exit 3", false, false, None) }
            .expect("CreateProcessW should succeed");
        // SAFETY: `job`/`spawned.process` are both freshly created, valid
        // handles; assignment happens before `resume`.
        unsafe { assign(job, spawned.process).expect("AssignProcessToJobObject should succeed") };
        // SAFETY: `spawned.thread` is a freshly created, valid,
        // not-yet-resumed thread handle.
        unsafe { process::resume(spawned.thread).expect("ResumeThread should succeed") };

        // SAFETY: `job` is a valid handle; this is the operation under
        // test — reversing kill-on-close before the handle closes below.
        unsafe { clear_kill_on_close(job).expect("SetInformationJobObject should succeed") };
        // SAFETY: `job` is a valid, currently-open handle, closed exactly
        // once. Without `clear_kill_on_close` above actually working,
        // this would terminate `spawned.process` immediately instead of
        // letting it run to its own natural exit.
        unsafe { crate::handle::close(job).unwrap() };

        // SAFETY: `spawned.process` is still a valid handle — closing the
        // job handle doesn't invalidate it, only (were kill-on-close still
        // set) the process it names.
        let exit = unsafe { process::wait(spawned.process, Some(5_000)) }.unwrap();
        assert_eq!(exit, Some(3));

        // SAFETY: both handles are valid and each closed exactly once.
        unsafe {
            crate::handle::close(spawned.process).unwrap();
            crate::handle::close(spawned.thread).unwrap();
        }
    }

    #[test]
    fn associate_completion_port_reports_process_exit() {
        let job = create().expect("CreateJobObjectW should succeed");
        let port = create_completion_port().expect("CreateIoCompletionPort should succeed");
        // SAFETY: both handles are freshly created and valid.
        unsafe { associate_completion_port(job, port, 42) }
            .expect("SetInformationJobObject should succeed");

        // SAFETY: a hand-built, correctly quoted command line for a
        // well-known system binary.
        let spawned = unsafe { process::spawn_suspended("cmd.exe /c exit 5", false, false, None) }
            .expect("CreateProcessW should succeed");
        // SAFETY: `job`/`spawned.process` are both freshly created, valid
        // handles; assignment happens before `resume`.
        unsafe { assign(job, spawned.process) }.expect("AssignProcessToJobObject should succeed");
        // SAFETY: `spawned.thread` is freshly created, valid, not yet
        // resumed.
        unsafe { process::resume(spawned.thread) }.expect("ResumeThread should succeed");

        // The OS posts several messages across this one process's lifecycle
        // (at least NEW_PROCESS and EXIT_PROCESS, usually followed by
        // ACTIVE_PROCESS_ZERO once the job empties) — loop until the
        // specific one this test cares about arrives, bounded so a real
        // regression fails outright instead of hanging.
        let mut saw_exit = false;
        for _ in 0..10 {
            // SAFETY: `port` is a valid, currently-open completion port
            // handle.
            let msg = unsafe { wait_for_message(port, Some(5_000)) }
                .expect("GetQueuedCompletionStatus should succeed")
                .expect("a message should arrive well within the timeout");
            assert_eq!(
                msg.completion_key, 42,
                "completion_key should echo back what associate_completion_port was called with"
            );
            if msg.message == JOB_OBJECT_MSG_EXIT_PROCESS {
                assert_eq!(msg.pid, spawned.process_id);
                saw_exit = true;
                break;
            }
        }
        assert!(saw_exit, "should have observed JOB_OBJECT_MSG_EXIT_PROCESS");

        // SAFETY: every handle here is valid and each closed exactly once.
        unsafe {
            crate::handle::close(spawned.process).unwrap();
            crate::handle::close(spawned.thread).unwrap();
            crate::handle::close(job).unwrap();
            crate::handle::close(port).unwrap();
        }
    }

    #[test]
    fn wait_for_message_times_out_with_nothing_queued() {
        let port = create_completion_port().expect("CreateIoCompletionPort should succeed");
        // SAFETY: `port` is freshly created and valid.
        let result = unsafe { wait_for_message(port, Some(0)) }
            .expect("GetQueuedCompletionStatus should succeed");
        assert_eq!(result, None);
        // SAFETY: `port` is a valid, currently-open handle, closed exactly
        // once.
        unsafe { crate::handle::close(port).unwrap() };
    }
}
