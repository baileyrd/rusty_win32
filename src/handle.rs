//! `DuplicateHandle`/`CreatePipe`/`SetHandleInformation`/`CloseHandle` — the
//! Windows counterpart of Unix `dup`/`pipe2`/`close`, and the primitive rush
//! needs to close its fd-3-and-up gap (see rush's
//! `docs/WINDOWS_BACKEND_ANALYSIS.md` §4.2). Windows has no kernel-level
//! small-integer fd table the way Unix does — only the three std-handle
//! slots this crate's [`crate::console`] sibling doesn't touch and
//! `winstdio` (in rush itself) already owns. This module provides the raw
//! handle primitives; a rush-owned integer-to-`HANDLE` map to give fd 3+ and
//! `{name}>` varfd redirects any meaning is a follow-up in rush itself, not
//! this crate (see the analysis doc's §4.2 for why that split is
//! deliberate).
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
    fn CloseHandle(object: RawHandle) -> i32;
    fn PeekNamedPipe(
        named_pipe: RawHandle,
        buffer: *mut u8,
        buffer_size: u32,
        bytes_read: *mut u32,
        total_bytes_avail: *mut u32,
        bytes_left_this_message: *mut u32,
    ) -> i32;
}

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
}
