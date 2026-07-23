//! Windows Registry access (`winreg.h`) — a new module added in round 2,
//! previously excluded by `ARCHITECTURE.md`'s non-goals (see
//! `gap-analysis.md`'s "Round 2: previously out-of-scope subsystems"
//! sweep), now in scope per explicit direction. No current `rush` feature
//! asks for this yet.
//!
//! This first piece is just the five predefined root keys every other
//! registry call starts from (`RegOpenKeyExW(HKEY_LOCAL_MACHINE, ...)`,
//! etc.) — the actual open/query/set/delete surface is follow-up work.
//!
//! `HKEY` gets its own [`HKey`] type rather than reusing
//! [`crate::handle::RawHandle`]: a registry key handle is closed via
//! `RegCloseKey`, not `CloseHandle`, so treating it as an interchangeable
//! `RawHandle` would invite calling the wrong close function on it.

/// A registry key handle — `HKEY`. Closed via `RegCloseKey` (a follow-up
/// module item), never `CloseHandle`/[`crate::handle::close`] — kept as
/// its own type for exactly that reason, distinct from
/// [`crate::handle::RawHandle`].
pub type HKey = *mut core::ffi::c_void;

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
}
