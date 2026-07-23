//! `DuplicateHandle`/`CreatePipe`/`SetHandleInformation`/`CloseHandle` — the
//! Windows counterpart of Unix `dup`/`pipe2`/`close`, and the primitive rush
//! needs to close its fd-3-and-up gap (see rush's
//! `docs/WINDOWS_BACKEND_ANALYSIS.md` §4.2). Windows has no kernel-level
//! small-integer fd table the way Unix does — only the three std-handle
//! slots [`get_std_handle`]/[`set_std_handle`] read and swap, which this
//! crate's [`crate::console`] sibling doesn't otherwise touch. This module
//! provides the raw handle primitives; a rush-owned integer-to-`HANDLE` map
//! to give fd 3+ and `{name}>` varfd redirects any meaning is a follow-up in
//! rush itself, not this crate (see the analysis doc's §4.2 for why that
//! split is deliberate).
//!
//! A Windows `HANDLE` is non-inheritable by default — the inverse of Unix's
//! `CLOEXEC`-by-default-absent convention, where a descriptor is inherited
//! unless explicitly marked `FD_CLOEXEC`. [`set_inheritable`] is the
//! `SetHandleInformation`/`HANDLE_FLAG_INHERIT` call that flips a specific
//! handle the other way, for the one end of a pipe (or duplicated handle) a
//! spawned child should actually see.

use crate::error::Win32Error;

/// A raw Win32 `HANDLE` (same representation `std::os::windows::io` uses).
pub type RawHandle = *mut core::ffi::c_void;

/// `SetHandleInformation`'s `dwMask`/`dwFlags` bit for handle inheritance.
const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
/// `DuplicateHandle`'s `dwOptions` bit: ignore `dwDesiredAccess` and
/// duplicate with the source handle's own access rights.
const DUPLICATE_SAME_ACCESS: u32 = 0x0000_0002;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> RawHandle;
    fn CreatePipe(
        read_pipe: *mut RawHandle,
        write_pipe: *mut RawHandle,
        pipe_attributes: *const core::ffi::c_void,
        size: u32,
    ) -> i32;
    fn DuplicateHandle(
        source_process: RawHandle,
        source_handle: RawHandle,
        target_process: RawHandle,
        target_handle: *mut RawHandle,
        desired_access: u32,
        inherit_handle: i32,
        options: u32,
    ) -> i32;
    fn SetHandleInformation(object: RawHandle, mask: u32, flags: u32) -> i32;
    fn GetHandleInformation(object: RawHandle, flags: *mut u32) -> i32;
    fn CloseHandle(object: RawHandle) -> i32;
    fn PeekNamedPipe(
        named_pipe: RawHandle,
        buffer: *mut u8,
        buffer_size: u32,
        bytes_read: *mut u32,
        total_bytes_avail: *mut u32,
        bytes_left_this_message: *mut u32,
    ) -> i32;
    fn GetStdHandle(std_handle: u32) -> RawHandle;
    fn SetStdHandle(std_handle: u32, handle: RawHandle) -> i32;
    fn CreateMutexW(
        mutex_attributes: *const core::ffi::c_void,
        initial_owner: i32,
        name: *const u16,
    ) -> RawHandle;
    fn ReleaseMutex(mutex: RawHandle) -> i32;
}

/// `GetStdHandle`/`SetStdHandle`'s `nStdHandle` slot selector for the
/// process's standard input. Defined as `(DWORD)-10` — a negative index
/// cast to an unsigned parameter type, not a real handle table offset.
pub const STD_INPUT_HANDLE: u32 = 0xFFFF_FFF6;
/// `GetStdHandle`/`SetStdHandle`'s `nStdHandle` slot selector for the
/// process's standard output. Defined as `(DWORD)-11`.
pub const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5;
/// `GetStdHandle`/`SetStdHandle`'s `nStdHandle` slot selector for the
/// process's standard error. Defined as `(DWORD)-12`.
pub const STD_ERROR_HANDLE: u32 = 0xFFFF_FFF4;

/// `GetStdHandle`'s own sentinel for "this call itself failed" — distinct
/// from a `NULL` return, which means the slot has no handle assigned rather
/// than that the call failed.
const INVALID_HANDLE_VALUE: isize = -1;

/// Create an anonymous pipe, returning `(read_handle, write_handle)`.
/// Neither end is inheritable by a spawned child yet — pass whichever end a
/// child needs through [`set_inheritable`] first, matching Windows'
/// non-inheritable-by-default convention rather than plumbing a
/// `SECURITY_ATTRIBUTES` struct through this call.
pub fn create_pipe() -> Result<(RawHandle, RawHandle), Win32Error> {
    let mut read_handle: RawHandle = core::ptr::null_mut();
    let mut write_handle: RawHandle = core::ptr::null_mut();
    // SAFETY: both out-pointers are valid, non-null, and point at
    // appropriately-sized `RawHandle` locals; `pipe_attributes = NULL`
    // requests default (non-inheritable) security attributes, a documented
    // valid input, not a dereferenced null.
    let ok = unsafe { CreatePipe(&mut read_handle, &mut write_handle, core::ptr::null(), 0) };
    if ok == 0 {
        Err(Win32Error::last())
    } else {
        Ok((read_handle, write_handle))
    }
}

/// Duplicate `handle` within the current process — the Windows analog of
/// Unix `dup`. The duplicate has the same access rights as `handle` and is
/// independently closeable; closing one does not affect the other. Set
/// `inheritable` to `true` if the duplicate should be visible to a
/// subsequently spawned child (e.g. the pipe end handed to a coprocess).
///
/// # Safety
///
/// `handle` must be a currently-open, valid handle owned by the caller (or
/// otherwise known to be safe to pass here) — Windows doesn't guarantee a
/// stale or reused handle value fails cleanly the way a Unix `dup` on a
/// closed fd does in every case (pseudo-handles and handle-value reuse are
/// both real edge cases).
pub unsafe fn duplicate(handle: RawHandle, inheritable: bool) -> Result<RawHandle, Win32Error> {
    let mut target: RawHandle = core::ptr::null_mut();
    // SAFETY: `handle` is a caller-supplied, presumed-valid handle (the
    // caller's responsibility, same as every other function here); source
    // and target process handles are both the current process's pseudo
    // handle, a documented, always-valid constant; `target` is a valid
    // out-pointer.
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            handle,
            GetCurrentProcess(),
            &mut target,
            0,
            i32::from(inheritable),
            DUPLICATE_SAME_ACCESS,
        )
    };
    if ok == 0 {
        Err(Win32Error::last())
    } else {
        Ok(target)
    }
}

/// Mark `handle` inheritable (visible to a child spawned after this call
/// with handle inheritance enabled) or not — `SetHandleInformation` with
/// `HANDLE_FLAG_INHERIT`, the inverse of Unix's `FD_CLOEXEC`: a Windows
/// handle starts non-inheritable, so this is the "opt in" call, not an
/// "opt out" one.
///
/// # Safety
///
/// `handle` must be a currently-open, valid handle owned by the caller.
pub unsafe fn set_inheritable(handle: RawHandle, inheritable: bool) -> Result<(), Win32Error> {
    // SAFETY: `handle` is caller-supplied; the flags are a plain bitmask,
    // not a pointer.
    let ok = unsafe {
        SetHandleInformation(
            handle,
            HANDLE_FLAG_INHERIT,
            if inheritable { HANDLE_FLAG_INHERIT } else { 0 },
        )
    };
    if ok == 0 {
        Err(Win32Error::last())
    } else {
        Ok(())
    }
}

/// Read `handle`'s current handle-information flags — `GetHandleInformation`,
/// the read-side counterpart to [`set_inheritable`]'s write-only
/// `SetHandleInformation` wrapper. Returns the raw flags bitmask
/// unmodified (the `HANDLE_FLAG_INHERIT` bit [`set_inheritable`] itself
/// toggles, plus `HANDLE_FLAG_PROTECT_FROM_CLOSE`, which this crate doesn't
/// otherwise expose) — deciding what to do with it is the caller's policy,
/// the same way this crate exposes other raw bitmask fields
/// (`FILE_ATTRIBUTE_*`, `ENABLE_*`) without deciding what they mean.
///
/// # Safety
///
/// `handle` must be a currently-open, valid handle.
pub unsafe fn handle_information(handle: RawHandle) -> Result<u32, Win32Error> {
    let mut flags: u32 = 0;
    // SAFETY: `handle` is caller-supplied per this function's own safety
    // contract; `flags` is a valid out-pointer.
    let ok = unsafe { GetHandleInformation(handle, &mut flags) };
    if ok == 0 {
        Err(Win32Error::last())
    } else {
        Ok(flags)
    }
}

/// Close `handle` — the Windows analog of Unix `close`. Closing an
/// already-closed or otherwise invalid handle fails with
/// [`Win32Error::ERROR_INVALID_HANDLE`] rather than being a silent no-op.
///
/// # Safety
///
/// `handle` must be a currently-open, valid handle owned by the caller, not
/// used again (by this crate or anything else) after this call returns —
/// the same "don't use it again" obligation Unix `close` places on a raw fd.
pub unsafe fn close(handle: RawHandle) -> Result<(), Win32Error> {
    // SAFETY: `handle` is caller-supplied; `CloseHandle` has no precondition
    // beyond "a handle value", and reports an invalid one as a normal
    // `FALSE`/`GetLastError` failure rather than undefined behavior.
    let ok = unsafe { CloseHandle(handle) };
    if ok == 0 {
        Err(Win32Error::last())
    } else {
        Ok(())
    }
}

/// How many bytes are currently available to read from `pipe_read_handle`
/// without blocking — `PeekNamedPipe`, the pipe-specific analog of
/// [`crate::console::wait_readable`]'s `WaitForSingleObject`-based check.
/// An anonymous pipe read end from [`create_pipe`] isn't usable the same way
/// `wait_readable` uses a console input handle — Windows' answer for "is
/// there data yet, don't block" on a pipe is this call instead. Does not
/// consume any data: a subsequent real read still sees every byte this
/// reports as available.
///
/// # Safety
///
/// `pipe_read_handle` must be a currently-open, valid handle to the read end
/// of a pipe.
pub unsafe fn pipe_bytes_available(pipe_read_handle: RawHandle) -> Result<u32, Win32Error> {
    let mut total_avail: u32 = 0;
    // SAFETY: `pipe_read_handle` is caller-supplied per this function's own
    // safety contract; passing NULL for the buffer/bytes-read/
    // bytes-left-this-message out-parameters is documented valid when the
    // caller only wants the total-available count (`buffer_size = 0`);
    // `total_avail` is a valid out-pointer.
    let ok = unsafe {
        PeekNamedPipe(
            pipe_read_handle,
            core::ptr::null_mut(),
            0,
            core::ptr::null_mut(),
            &mut total_avail,
            core::ptr::null_mut(),
        )
    };
    if ok == 0 {
        Err(Win32Error::last())
    } else {
        Ok(total_avail)
    }
}

/// Read one of the calling process's standard handle slots (`slot`: one of
/// [`STD_INPUT_HANDLE`]/[`STD_OUTPUT_HANDLE`]/[`STD_ERROR_HANDLE`]) —
/// `GetStdHandle`, the primitive `spawn_suspended`'s own doc comment
/// describes redirection as built on ("swapping the parent's std-handle
/// slots before spawning"). `Ok(None)` means the slot has no handle
/// assigned (`GetStdHandle`'s documented `NULL`-without-`GetLastError`-
/// failure outcome, e.g. a GUI process with no console), distinct from
/// `Err`, which means the call itself failed
/// (`INVALID_HANDLE_VALUE`).
pub fn get_std_handle(slot: u32) -> Result<Option<RawHandle>, Win32Error> {
    // SAFETY: `slot` is a plain `DWORD` selector, not a pointer; any `u32`
    // value is a valid (if possibly unrecognized) argument to
    // `GetStdHandle`.
    let handle = unsafe { GetStdHandle(slot) };
    if handle as isize == INVALID_HANDLE_VALUE {
        Err(Win32Error::last())
    } else if handle.is_null() {
        Ok(None)
    } else {
        Ok(Some(handle))
    }
}

/// Replace one of the calling process's standard handle slots — `SetStdHandle`,
/// the other half of the "swap the parent's std-handle slots before
/// spawning" redirection model [`get_std_handle`]'s own doc references.
/// Affects only this process's own view (and anything a subsequent
/// `CreateProcessW`-style spawn inherits from it); it does not duplicate or
/// close the handle previously in that slot.
///
/// # Safety
///
/// `handle` must be a currently-open, valid handle (or a documented pseudo-
/// handle) that outlives its use as a standard handle — this call does not
/// take ownership of it, matching `SetStdHandle`'s own documented contract.
pub unsafe fn set_std_handle(slot: u32, handle: RawHandle) -> Result<(), Win32Error> {
    // SAFETY: `slot` is a plain `DWORD` selector; `handle` is caller-supplied
    // per this function's own safety contract.
    let ok = unsafe { SetStdHandle(slot, handle) };
    if ok == 0 {
        Err(Win32Error::last())
    } else {
        Ok(())
    }
}

/// Create (or open, if `name` already names an existing one) a named or
/// unnamed mutex — `CreateMutexW`, the Windows analog of `flock`'s
/// cross-process locking, but as a standalone kernel object rather than a
/// file-descriptor operation. `name`, if given, makes the mutex visible to
/// any process that names the same string (e.g. guarding concurrent writes
/// to a shared history file from multiple shell instances); `None` creates
/// an unnamed mutex only this process (and anything it hands the returned
/// handle to) can reach. `initial_owner` requests immediate ownership,
/// skipping a separate wait — the same shortcut `CreateMutexW`'s own
/// `bInitialOwner` parameter offers. Acquiring an *existing* mutex is
/// already covered by this crate's `WaitForSingleObject`-shaped wait
/// primitives (e.g. [`crate::process::wait`]'s underlying call, or
/// [`crate::console::wait_readable`]) once a handle is in hand — this
/// function only creates/opens the object itself.
pub fn create_mutex(name: Option<&str>, initial_owner: bool) -> Result<RawHandle, Win32Error> {
    extern crate alloc;
    let wide: Option<alloc::vec::Vec<u16>> =
        name.map(|n| n.encode_utf16().chain(core::iter::once(0)).collect());
    let name_ptr = wide.as_ref().map_or(core::ptr::null(), |w| w.as_ptr());
    // SAFETY: `mutex_attributes = NULL` requests default (non-inheritable)
    // security attributes, a documented valid input; `name_ptr` is either
    // NULL (documented as "create an unnamed mutex") or a valid,
    // NUL-terminated UTF-16 string kept alive for the duration of this call.
    let handle = unsafe { CreateMutexW(core::ptr::null(), i32::from(initial_owner), name_ptr) };
    if handle.is_null() {
        Err(Win32Error::last())
    } else {
        Ok(handle)
    }
}

/// Release ownership of `mutex`, previously acquired via a
/// `WaitForSingleObject`-shaped wait (or [`create_mutex`]'s own
/// `initial_owner: true`) — `ReleaseMutex`.
///
/// # Safety
///
/// `mutex` must be a currently-open, valid mutex handle currently owned by
/// the calling thread.
pub unsafe fn release_mutex(mutex: RawHandle) -> Result<(), Win32Error> {
    // SAFETY: `mutex` is caller-supplied per this function's own safety
    // contract.
    let ok = unsafe { ReleaseMutex(mutex) };
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
    fn create_pipe_returns_two_distinct_handles() {
        let (read_handle, write_handle) = create_pipe().expect("CreatePipe should succeed");
        assert!(!read_handle.is_null());
        assert!(!write_handle.is_null());
        assert_ne!(read_handle, write_handle);
        // SAFETY: both handles are freshly created, valid, and not used
        // again after this.
        unsafe {
            close(read_handle).unwrap();
            close(write_handle).unwrap();
        }
    }

    #[test]
    fn pipe_carries_bytes_end_to_end() {
        use std::io::{Read, Write};
        use std::os::windows::io::{FromRawHandle, OwnedHandle};

        let (read_handle, write_handle) = create_pipe().expect("CreatePipe should succeed");
        // SAFETY: both handles are freshly created, valid, and uniquely
        // owned by this test — nothing else holds or will close them.
        let mut reader =
            std::fs::File::from(unsafe { OwnedHandle::from_raw_handle(read_handle as _) });
        let mut writer =
            std::fs::File::from(unsafe { OwnedHandle::from_raw_handle(write_handle as _) });

        writer.write_all(b"rusty_win32").unwrap();
        drop(writer); // close the write end so the read below sees EOF

        let mut got = std::string::String::new();
        reader.read_to_string(&mut got).unwrap();
        assert_eq!(got, "rusty_win32");
    }

    #[test]
    fn duplicate_produces_an_independently_closeable_handle() {
        let (read_handle, write_handle) = create_pipe().expect("CreatePipe should succeed");
        // SAFETY: `write_handle` is freshly created and valid.
        let dup =
            unsafe { duplicate(write_handle, false) }.expect("DuplicateHandle should succeed");
        assert_ne!(dup, write_handle);

        // SAFETY: all three handles are valid and each is closed exactly
        // once; `dup` and `write_handle` are independent handles to the
        // same object, so closing one doesn't invalidate the other.
        unsafe {
            close(write_handle).unwrap();
            close(dup).unwrap();
            close(read_handle).unwrap();
        }
    }

    #[test]
    fn set_inheritable_round_trips() {
        let (read_handle, write_handle) = create_pipe().expect("CreatePipe should succeed");
        // SAFETY: `write_handle`/`read_handle` are freshly created and valid
        // for the duration of this test.
        unsafe {
            set_inheritable(write_handle, true).expect("marking inheritable should succeed");
            set_inheritable(write_handle, false).expect("clearing inheritable should succeed");
            close(read_handle).unwrap();
            close(write_handle).unwrap();
        }
    }

    #[test]
    fn handle_information_reflects_set_inheritable() {
        let (read_handle, write_handle) = create_pipe().expect("CreatePipe should succeed");
        // SAFETY: `write_handle` is freshly created and valid for the
        // duration of this test.
        unsafe {
            set_inheritable(write_handle, true).expect("marking inheritable should succeed");
            let flags_when_set =
                handle_information(write_handle).expect("GetHandleInformation should succeed");
            assert_ne!(
                flags_when_set & HANDLE_FLAG_INHERIT,
                0,
                "the inherit bit should be set after set_inheritable(true)"
            );

            set_inheritable(write_handle, false).expect("clearing inheritable should succeed");
            let flags_when_cleared =
                handle_information(write_handle).expect("GetHandleInformation should succeed");
            assert_eq!(
                flags_when_cleared & HANDLE_FLAG_INHERIT,
                0,
                "the inherit bit should be clear after set_inheritable(false)"
            );

            close(read_handle).unwrap();
            close(write_handle).unwrap();
        }
    }

    #[test]
    fn closing_an_already_closed_handle_fails() {
        // Exact error code isn't part of `CloseHandle`'s documented
        // contract (a reused handle value is possible in principle), so
        // this only pins "fails", not a specific `Win32Error`.
        let (read_handle, write_handle) = create_pipe().expect("CreatePipe should succeed");
        // SAFETY: the double-close on `write_handle` is the specific
        // documented-failure case under test, not a real use-after-close;
        // nothing else touches this handle value in between.
        unsafe {
            close(write_handle).unwrap();
            assert!(close(write_handle).is_err());
            close(read_handle).unwrap();
        }
    }

    #[test]
    fn pipe_bytes_available_reports_zero_for_an_empty_pipe() {
        let (read_handle, write_handle) = create_pipe().expect("CreatePipe should succeed");
        // SAFETY: `read_handle` is freshly created and valid.
        let avail =
            unsafe { pipe_bytes_available(read_handle) }.expect("PeekNamedPipe should succeed");
        assert_eq!(avail, 0);
        // SAFETY: both handles are valid and each closed exactly once.
        unsafe {
            close(read_handle).unwrap();
            close(write_handle).unwrap();
        }
    }

    #[test]
    fn pipe_bytes_available_reports_written_data_without_consuming_it() {
        use std::io::{Read, Write};
        use std::os::windows::io::{FromRawHandle, OwnedHandle};

        let (read_handle, write_handle) = create_pipe().expect("CreatePipe should succeed");
        // SAFETY: `write_handle` is freshly created, valid, and uniquely
        // owned by this test.
        let mut writer =
            std::fs::File::from(unsafe { OwnedHandle::from_raw_handle(write_handle as _) });
        writer.write_all(b"rusty_win32").unwrap();

        // SAFETY: `read_handle` is freshly created, valid, and still open
        // (not yet wrapped/moved into an owning `File` below).
        let avail =
            unsafe { pipe_bytes_available(read_handle) }.expect("PeekNamedPipe should succeed");
        assert_eq!(avail, "rusty_win32".len() as u32);

        drop(writer); // close the write end so the read below sees EOF after the data

        // SAFETY: `read_handle` is still the same valid handle; nothing else
        // holds or will close it.
        let mut reader =
            std::fs::File::from(unsafe { OwnedHandle::from_raw_handle(read_handle as _) });
        let mut got = std::string::String::new();
        reader.read_to_string(&mut got).unwrap();
        assert_eq!(
            got, "rusty_win32",
            "peeking must not have consumed any bytes"
        );
    }

    #[test]
    fn get_std_handle_succeeds_for_every_standard_slot() {
        for slot in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            get_std_handle(slot).expect("GetStdHandle should succeed for a well-known slot");
        }
    }

    #[test]
    fn set_std_handle_then_get_std_handle_round_trips() {
        let original = get_std_handle(STD_INPUT_HANDLE).expect("GetStdHandle should succeed");
        let (read_end, write_end) = create_pipe().expect("CreatePipe should succeed");

        // SAFETY: `read_end` is a just-created, valid handle that outlives
        // its use as the standard-input slot for the duration of this test.
        unsafe { set_std_handle(STD_INPUT_HANDLE, read_end) }.expect("SetStdHandle should succeed");
        let swapped = get_std_handle(STD_INPUT_HANDLE).expect("GetStdHandle should succeed");
        assert_eq!(swapped, Some(read_end));

        // SAFETY: `original` (if any) is the process's own handle from
        // before this test began, or NULL to restore "no handle assigned" —
        // either way a safe value to hand back to `SetStdHandle`.
        unsafe { set_std_handle(STD_INPUT_HANDLE, original.unwrap_or(core::ptr::null_mut())) }
            .expect("SetStdHandle should succeed restoring the original handle");

        // SAFETY: both pipe ends are still open and not used again.
        unsafe {
            let _ = close(read_end);
            let _ = close(write_end);
        }
    }

    #[test]
    fn create_mutex_then_release_mutex_round_trips() {
        let mutex = create_mutex(None, false).expect("CreateMutexW should succeed");

        // SAFETY: `mutex` is a freshly created, valid, waitable handle.
        let acquired = unsafe { crate::console::wait_readable(mutex, 0) }
            .expect("WaitForSingleObject should succeed acquiring an unowned mutex");
        assert!(
            acquired,
            "an unowned mutex should be immediately acquirable"
        );

        // SAFETY: `mutex` is a valid mutex handle currently owned by this
        // thread (just acquired above); this is the operation under test.
        unsafe { release_mutex(mutex) }.expect("ReleaseMutex should succeed");

        // SAFETY: `mutex` is still a valid, currently-open handle, closed
        // exactly once and not used again after this.
        unsafe { close(mutex).unwrap() };
    }

    #[test]
    fn create_mutex_with_initial_owner_starts_already_owned() {
        let mutex = create_mutex(Some("rusty_win32_test_mutex_initial_owner"), true)
            .expect("CreateMutexW should succeed");

        // SAFETY: `mutex` is valid and currently owned by this thread
        // (`initial_owner: true` above).
        unsafe { release_mutex(mutex) }
            .expect("ReleaseMutex should succeed releasing initial ownership");

        // SAFETY: `mutex` is still a valid, currently-open handle, closed
        // exactly once and not used again after this.
        unsafe { close(mutex).unwrap() };
    }
}
