//! Windows Sockets (Winsock2) — `winsock2.h`, a new module added in
//! round 2, previously excluded by `ARCHITECTURE.md`'s non-goals (see
//! `gap-analysis.md`'s "Round 2: previously out-of-scope subsystems"
//! sweep), now in scope per explicit round-2 direction.
//!
//! Scope: basic TCP/UDP client+server socket programming, the same core
//! subset `rusty_libc` wraps for POSIX sockets. Overlapped/IOCP-based
//! async I/O, `WSAPoll`, and protocol-specific options beyond the
//! ordinary set are all explicitly out of scope for this first pass.
//!
//! This first piece is Winsock's own load/unload lifecycle —
//! `WSAStartup`/`WSACleanup`, the one primitive with no POSIX/
//! `rusty_libc` analog: every other Winsock call is documented undefined
//! behavior before a matching `WSAStartup` or after `WSACleanup`.
//! Windows reference-counts nested `WSAStartup`/`WSACleanup` pairs
//! internally, so no shared guard/RAII type is needed here — two plain
//! functions, matching this crate's existing no-`Drop`-anywhere
//! convention (`volume::FindVolumes`/`security::PathSecurityInfo`/
//! `security::BuiltAcl` are the only exceptions, none of which apply to
//! a process-global load count like this one).

#[link(name = "ws2_32")]
unsafe extern "system" {
    fn WSAStartup(version_requested: u16, wsa_data: *mut WsaData) -> i32;
    fn WSACleanup() -> i32;
    fn WSAGetLastError() -> i32;
}

// WSADATA (64-bit layout, per mingw-w64's own `psdk_inc/_wsadata.h`):
// `size_of` 408 — verified field-by-field with a compiled
// `_Static_assert` probe. Never read by this crate: `startup`'s only
// interesting output (the error code, if any) comes back as
// `WSAStartup`'s own return value, matching this crate's existing
// "reports failure via its own return value directly" LSTATUS-style
// convention — so this is scratch space only, the same treatment
// `service::control`'s `ServiceStatusRaw` gets.
#[repr(C)]
struct WsaData {
    version: u16,
    high_version: u16,
    max_sockets: u16,
    max_udp_dg: u16,
    vendor_info: *mut u8,
    description: [u8; 257],
    system_status: [u8; 129],
}
const _: () = assert!(core::mem::size_of::<WsaData>() == 408);

/// `MAKEWORD(2, 2)` — Winsock 2.2, the version every modern Windows
/// ships and the only one this crate requests.
const WINSOCK_VERSION_2_2: u16 = 0x0202;

/// Initialize Winsock — `WSAStartup`, requesting version 2.2 (the
/// version every modern Windows ships). Must be called at least once
/// before any other function in this module; Windows reference-counts
/// nested calls internally, so calling this more than once (matched by
/// an equal number of [`cleanup`] calls) is documented as safe, not a
/// caller error this crate needs to guard against.
///
/// Reports failure via its own return value directly — never
/// `GetLastError`/`WSAGetLastError` — so a nonzero return is passed
/// straight to [`crate::error::Win32Error::from_raw`] rather than
/// `Win32Error::last`.
pub fn startup() -> Result<(), crate::error::Win32Error> {
    let mut wsa_data = core::mem::MaybeUninit::<WsaData>::uninit();
    // SAFETY: `wsa_data` is a valid, correctly-sized out-buffer;
    // `WSAStartup` fully initializes it on success, and its contents are
    // otherwise never read by this crate.
    let status = unsafe { WSAStartup(WINSOCK_VERSION_2_2, wsa_data.as_mut_ptr()) };
    if status != 0 {
        Err(crate::error::Win32Error::from_raw(status as u32))
    } else {
        Ok(())
    }
}

/// Tear down Winsock — `WSACleanup`. Every [`startup`] call must be
/// matched by exactly one `cleanup` call (Windows reference-counts
/// nested pairs internally); calling any other function in this module
/// after the reference count reaches zero is documented undefined
/// behavior.
///
/// Unlike [`startup`], failure is reported the ordinary
/// `GetLastError`-equivalent way — `WSAGetLastError`, a distinct
/// per-thread error slot Winsock keeps separately from the regular
/// `GetLastError`/`SetLastError` one.
pub fn cleanup() -> Result<(), crate::error::Win32Error> {
    // SAFETY: `WSACleanup` takes no arguments.
    let status = unsafe { WSACleanup() };
    if status != 0 {
        // SAFETY: `WSAGetLastError` takes no arguments; calling it
        // immediately after a failing Winsock call is documented to
        // report that same call's error.
        let err = unsafe { WSAGetLastError() };
        Err(crate::error::Win32Error::from_raw(err as u32))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_then_cleanup_round_trips() {
        startup().expect("WSAStartup should succeed requesting Winsock 2.2");
        cleanup().expect("WSACleanup should succeed matching the startup call above");
    }

    #[test]
    fn nested_startup_cleanup_pairs_are_reference_counted() {
        // Windows documents WSAStartup/WSACleanup as reference-counted:
        // two startups followed by two cleanups should both succeed,
        // rather than the second cleanup failing once the "real" count
        // has already reached zero after the first.
        startup().expect("first WSAStartup should succeed");
        startup().expect("nested WSAStartup should also succeed");
        cleanup().expect("first WSACleanup should succeed");
        cleanup().expect("second WSACleanup should succeed, matching the nested startup");
    }
}
