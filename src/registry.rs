//! Windows Registry access (`winreg.h`) — a new module added in round 2,
//! previously excluded by `ARCHITECTURE.md`'s non-goals (see
//! `gap-analysis.md`'s "Round 2: previously out-of-scope subsystems"
//! sweep), now in scope per explicit direction. No current `rush` feature
//! asks for this yet.
//!
//! Started with just the five predefined root keys every other registry
//! call starts from (`RegOpenKeyExW(HKEY_LOCAL_MACHINE, ...)`, etc.); this
//! piece adds [`open_key`]/[`close_key`] — the query/set/delete surface is
//! still follow-up work.
//!
//! `HKEY` gets its own [`HKey`] type rather than reusing
//! [`crate::handle::RawHandle`]: a registry key handle is closed via
//! `RegCloseKey`, not `CloseHandle`, so treating it as an interchangeable
//! `RawHandle` would invite calling the wrong close function on it.
//! `advapi32.dll` (this crate's second non-`kernel32` link, after
//! `console`'s `user32.dll`) is where every registry function lives —
//! flagged as an expected future addition in this crate's own README
//! before this module existed.

extern crate alloc;
use alloc::vec::Vec;

/// A registry key handle — `HKEY`. Closed via [`close_key`], never
/// `CloseHandle`/[`crate::handle::close`] — kept as its own type for
/// exactly that reason, distinct from [`crate::handle::RawHandle`].
pub type HKey = *mut core::ffi::c_void;

#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegOpenKeyExW(
        key: HKey,
        sub_key: *const u16,
        options: u32,
        sam_desired: u32,
        result: *mut HKey,
    ) -> i32;
    fn RegCloseKey(key: HKey) -> i32;
}

/// `REGSAM` access-mask bits for [`open_key`] (and every later registry
/// call taking an `access: u32`) — exposed raw and policy-free, matching
/// this crate's existing convention for other bitmask fields
/// (`fs::FILE_ATTRIBUTE_*`, `console::ENABLE_*`). Verified against
/// mingw-w64's own `winnt.h` macros with a compiled `_Static_assert`
/// probe, the same discipline `HKEY_CLASSES_ROOT` etc. above used.
pub const KEY_QUERY_VALUE: u32 = 0x0001;
pub const KEY_READ: u32 = 0x0002_0019;
pub const KEY_WRITE: u32 = 0x0002_0006;
pub const KEY_ALL_ACCESS: u32 = 0x000F_003F;

// The five predefined roots below are `HKEY`-typed sentinel values, not
// real object handles — Windows defines each as a small integer cast
// through `(LONG)`, i.e. a *signed 32-bit* value, before widening to the
// pointer-sized `HKEY`. On a 64-bit target that widening sign-extends:
// `0x80000000` as `LONG` is negative, so the real bit pattern is
// `0xFFFFFFFF80000000`, not `0x0000000080000000`. Verified against
// mingw-w64's own `winreg.h` macros with a compiled `_Static_assert`
// probe (`x86_64-w64-mingw32-gcc`), the same discipline this crate uses
// for every other struct/constant layout it can't check any other way.

/// Classes and file associations — a merged view of
/// `HKEY_LOCAL_MACHINE\Software\Classes` and
/// `HKEY_CURRENT_USER\Software\Classes`.
pub const HKEY_CLASSES_ROOT: HKey = 0xFFFF_FFFF_8000_0000usize as HKey;
/// The current user's own settings (their profile-scoped hive).
pub const HKEY_CURRENT_USER: HKey = 0xFFFF_FFFF_8000_0001usize as HKey;
/// Machine-wide settings, shared by every user on this computer.
pub const HKEY_LOCAL_MACHINE: HKey = 0xFFFF_FFFF_8000_0002usize as HKey;
/// Every loaded user profile's hive, keyed by SID
/// (`HKEY_USERS\<SID>\...`) — `HKEY_CURRENT_USER` is a per-process alias
/// into one entry here.
pub const HKEY_USERS: HKey = 0xFFFF_FFFF_8000_0003usize as HKey;
/// The active hardware profile, a subset of `HKEY_LOCAL_MACHINE` exposed
/// as its own root for legacy reasons — mostly a historical artifact on
/// modern Windows, which no longer really supports multiple hardware
/// profiles.
pub const HKEY_CURRENT_CONFIG: HKey = 0xFFFF_FFFF_8000_0005usize as HKey;

/// Open a subkey of `parent` — `RegOpenKeyExW`. The registry analog of
/// [`crate::handle::duplicate`]'s "start from a handle you already have":
/// every deeper registry access starts by opening a subkey of one of the
/// five predefined roots above (or of a key this function already
/// returned), same as opening a nested directory one path component at a
/// time. `access` is a `KEY_*` bitmask (e.g. [`KEY_READ`]).
///
/// Unlike most of this crate's Win32 wrappers, `RegOpenKeyExW` reports
/// failure via its own `LSTATUS` return value directly — never
/// `GetLastError` — so a nonzero return is passed straight to
/// [`crate::error::Win32Error::from_raw`] rather than
/// `Win32Error::last()`.
///
/// # Safety
///
/// `parent` must be a currently-valid `HKey` — one of the predefined
/// roots above, or a key this function previously returned that hasn't
/// been closed yet.
pub unsafe fn open_key(
    parent: HKey,
    subkey: &str,
    access: u32,
) -> Result<HKey, crate::error::Win32Error> {
    let wide: Vec<u16> = subkey.encode_utf16().chain(core::iter::once(0)).collect();
    let mut result: HKey = core::ptr::null_mut();
    // SAFETY: `parent` is caller-supplied per this function's own safety
    // contract; `wide` is a valid, NUL-terminated UTF-16 string live for
    // the whole call; `result` is a valid out-pointer.
    let status = unsafe { RegOpenKeyExW(parent, wide.as_ptr(), 0, access, &mut result) };
    if status == 0 {
        Ok(result)
    } else {
        Err(crate::error::Win32Error::from_raw(status as u32))
    }
}

/// Close a key handle previously returned by [`open_key`] — `RegCloseKey`.
/// Never call this on one of the five predefined roots above: they are
/// sentinel values, not real handles the system reference-counts, and
/// `RegCloseKey` on one is a documented no-op that still returns success,
/// so this crate's own "must close what you open" discipline doesn't
/// actually apply to them the way it does to an [`open_key`] result.
///
/// # Safety
///
/// `key` must be a currently-open handle returned by [`open_key`], not yet
/// closed.
pub unsafe fn close_key(key: HKey) -> Result<(), crate::error::Win32Error> {
    // SAFETY: `key` is caller-supplied per this function's own safety
    // contract.
    let status = unsafe { RegCloseKey(key) };
    if status == 0 {
        Ok(())
    } else {
        Err(crate::error::Win32Error::from_raw(status as u32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_keys_are_distinct_and_match_the_documented_sign_extended_sentinels() {
        assert_eq!(HKEY_CLASSES_ROOT as usize, 0xFFFF_FFFF_8000_0000);
        assert_eq!(HKEY_CURRENT_USER as usize, 0xFFFF_FFFF_8000_0001);
        assert_eq!(HKEY_LOCAL_MACHINE as usize, 0xFFFF_FFFF_8000_0002);
        assert_eq!(HKEY_USERS as usize, 0xFFFF_FFFF_8000_0003);
        assert_eq!(HKEY_CURRENT_CONFIG as usize, 0xFFFF_FFFF_8000_0005);

        let roots = [
            HKEY_CLASSES_ROOT,
            HKEY_CURRENT_USER,
            HKEY_LOCAL_MACHINE,
            HKEY_USERS,
            HKEY_CURRENT_CONFIG,
        ];
        for (i, a) in roots.iter().enumerate() {
            for (j, b) in roots.iter().enumerate() {
                assert_eq!(
                    i == j,
                    a == b,
                    "roots at {i} and {j} should differ unless equal indices"
                );
            }
        }
    }

    #[test]
    fn open_key_then_close_key_round_trips_on_a_well_known_subkey() {
        // `SOFTWARE\Microsoft\Windows\CurrentVersion` exists on every
        // Windows install (it's where things like the OS build number
        // live) — a stable, always-present target that doesn't depend on
        // anything this crate itself created.
        // SAFETY: `HKEY_LOCAL_MACHINE` is a predefined, always-valid root.
        let key = unsafe {
            open_key(
                HKEY_LOCAL_MACHINE,
                "SOFTWARE\\Microsoft\\Windows\\CurrentVersion",
                KEY_READ,
            )
        }
        .expect("RegOpenKeyExW should succeed for a well-known, always-present subkey");
        assert!(!key.is_null());
        // SAFETY: `key` was just opened above and not yet closed.
        unsafe { close_key(key) }.expect("RegCloseKey should succeed");
    }

    #[test]
    fn open_key_fails_for_a_nonexistent_subkey() {
        // SAFETY: `HKEY_LOCAL_MACHINE` is a predefined, always-valid root.
        let err = unsafe {
            open_key(
                HKEY_LOCAL_MACHINE,
                "Software\\ThisSubkeyDefinitelyDoesNotExist12345",
                KEY_READ,
            )
        }
        .expect_err("RegOpenKeyExW should fail for a nonexistent subkey");
        assert_eq!(err, crate::error::Win32Error::ERROR_FILE_NOT_FOUND);
    }
}
