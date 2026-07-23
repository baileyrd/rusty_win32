//! Windows service control — `winsvc.h`, a new module added in round 2,
//! previously excluded by `ARCHITECTURE.md`'s non-goals (see
//! `gap-analysis.md`'s "Round 2: previously out-of-scope subsystems"
//! sweep), now in scope per explicit round-2 direction.
//!
//! Scope: a `systemctl`-equivalent — list/query/start/stop a named
//! service. Installing/removing services (`CreateServiceW`/
//! `DeleteService`), driver services, and service-controller-side
//! (writing a service's own `main`) support are all explicitly out of
//! scope.
//!
//! This first piece is the SCM/service handle lifecycle. An `SC_HANDLE`
//! is reused as this crate's existing [`crate::handle::RawHandle`]
//! (`*mut c_void`) rather than a distinct type — it's ABI-compatible
//! (`DECLARE_HANDLE`-based, exactly like `HANDLE` itself), and every
//! function in this module accepts/returns it, so a separate wrapper
//! type would add nothing; its destructor is `CloseServiceHandle`,
//! never `CloseHandle`, so it must never be passed to
//! [`crate::handle`]'s own close functions.

extern crate alloc;
use crate::error::Win32Error;
use crate::handle::RawHandle;
use alloc::vec::Vec;

#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenSCManagerW(
        machine_name: *const u16,
        database_name: *const u16,
        desired_access: u32,
    ) -> RawHandle;
    fn OpenServiceW(scm: RawHandle, service_name: *const u16, desired_access: u32) -> RawHandle;
    fn CloseServiceHandle(handle: RawHandle) -> i32;
}

/// `SC_MANAGER_CONNECT` — the minimal access right needed to open a
/// connection to the SCM at all. Verified against mingw-w64's own
/// `winsvc.h` with a compiled `_Static_assert` probe.
pub const SC_MANAGER_CONNECT: u32 = 0x0001;

/// `SERVICE_QUERY_CONFIG`. Verified against mingw-w64's own `winsvc.h`
/// with a compiled `_Static_assert` probe.
pub const SERVICE_QUERY_CONFIG: u32 = 0x0001;

/// `SERVICE_QUERY_STATUS`. Verified against mingw-w64's own `winsvc.h`
/// with a compiled `_Static_assert` probe.
pub const SERVICE_QUERY_STATUS: u32 = 0x0004;

/// Connect to the local Service Control Manager — `OpenSCManagerW`, the
/// entry point every other function in this module needs (directly, or
/// via a handle [`open_service`] opened from one). Always connects to
/// the local machine's active services database — this module has no
/// remote-machine or alternate-database (`SERVICES_FAILED_DATABASE`)
/// support.
pub fn open_manager(access: u32) -> Result<RawHandle, Win32Error> {
    // SAFETY: a null machine/database name is documented to mean "the
    // local machine's active services database"; `access` is a plain
    // access-rights bitmask, not a pointer.
    let handle = unsafe { OpenSCManagerW(core::ptr::null(), core::ptr::null(), access) };
    if handle.is_null() {
        Err(Win32Error::last())
    } else {
        Ok(handle)
    }
}

/// Open a handle to the service named `name` — `OpenServiceW`, the
/// `service` module's equivalent of [`crate::process::open_by_pid`]:
/// turning a name a caller only knows as a string into a handle real
/// operations (status/start/stop) can be performed on.
///
/// # Safety
///
/// `scm` must be a currently-open, valid SCM handle from [`open_manager`]
/// with at least [`SC_MANAGER_CONNECT`].
pub unsafe fn open_service(
    scm: RawHandle,
    name: &str,
    access: u32,
) -> Result<RawHandle, Win32Error> {
    let wide: Vec<u16> = name.encode_utf16().chain(core::iter::once(0)).collect();
    // SAFETY: `scm` is caller-supplied per this function's own safety
    // contract; `wide` is a valid, NUL-terminated UTF-16 string live for
    // the whole call.
    let handle = unsafe { OpenServiceW(scm, wide.as_ptr(), access) };
    if handle.is_null() {
        Err(Win32Error::last())
    } else {
        Ok(handle)
    }
}

/// Close a handle opened by [`open_manager`] or [`open_service`] —
/// `CloseServiceHandle`. Never [`crate::handle`]'s own close function:
/// an `SC_HANDLE`'s destructor is always this one, never `CloseHandle`.
///
/// # Safety
///
/// `handle` must be a currently-open, valid handle from [`open_manager`]
/// or [`open_service`], not already closed.
pub unsafe fn close(handle: RawHandle) -> Result<(), Win32Error> {
    // SAFETY: `handle` is caller-supplied per this function's own safety
    // contract.
    let ok = unsafe { CloseServiceHandle(handle) };
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
    fn open_manager_then_close_round_trips() {
        let scm = open_manager(SC_MANAGER_CONNECT)
            .expect("OpenSCManagerW should succeed with SC_MANAGER_CONNECT");
        // SAFETY: `scm` was just opened above and hasn't been closed yet.
        unsafe { close(scm) }
            .expect("CloseServiceHandle should succeed on a freshly opened SCM handle");
    }

    #[test]
    fn open_service_then_close_round_trips_on_the_event_log_service() {
        let scm = open_manager(SC_MANAGER_CONNECT)
            .expect("OpenSCManagerW should succeed with SC_MANAGER_CONNECT");

        // "EventLog" (Windows Event Log) is a core OS service present
        // and installed on every Windows edition, including Server
        // Core -- a safe choice for a CI test that shouldn't depend on
        // optional roles/features being present.
        // SAFETY: `scm` is valid and open from the call just above.
        let service = unsafe { open_service(scm, "EventLog", SERVICE_QUERY_STATUS) }
            .expect("OpenServiceW should succeed for the well-known EventLog service");
        // SAFETY: `service` was just opened above and hasn't been closed
        // yet.
        unsafe { close(service) }
            .expect("CloseServiceHandle should succeed on a freshly opened service handle");

        // SAFETY: `scm` is still open (not yet closed).
        unsafe { close(scm) }.expect("CloseServiceHandle should succeed on the SCM handle");
    }

    #[test]
    fn open_service_fails_for_a_nonexistent_service_name() {
        let scm = open_manager(SC_MANAGER_CONNECT)
            .expect("OpenSCManagerW should succeed with SC_MANAGER_CONNECT");

        // SAFETY: `scm` is valid and open from the call just above.
        let err = unsafe {
            open_service(
                scm,
                "rusty_win32_definitely_not_a_real_service",
                SERVICE_QUERY_STATUS,
            )
        }
        .expect_err("OpenServiceW should fail for a nonexistent service name");
        assert_eq!(err, Win32Error::ERROR_SERVICE_DOES_NOT_EXIST);

        // SAFETY: `scm` is still open (not yet closed).
        unsafe { close(scm) }.expect("CloseServiceHandle should succeed on the SCM handle");
    }
}
