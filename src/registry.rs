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
    fn RegCreateKeyExW(
        key: HKey,
        sub_key: *const u16,
        reserved: u32,
        class: *mut u16,
        options: u32,
        sam_desired: u32,
        security_attributes: *const core::ffi::c_void,
        result: *mut HKey,
        disposition: *mut u32,
    ) -> i32;
    fn RegQueryValueExW(
        key: HKey,
        value_name: *const u16,
        reserved: *mut u32,
        value_type: *mut u32,
        data: *mut u8,
        data_size: *mut u32,
    ) -> i32;
}

/// `RegCreateKeyExW`'s `dwOptions`: an ordinary, disk-persisted key —
/// this crate's only supported option (`REG_OPTION_VOLATILE`/
/// `REG_OPTION_BACKUP_RESTORE`/etc. are out of scope, same as this
/// crate's other modules only covering the common case).
const REG_OPTION_NON_VOLATILE: u32 = 0x0000_0000;
/// `RegCreateKeyExW`'s `lpdwDisposition` out-values — verified against
/// mingw-w64's own `winnt.h` macros.
const REG_OPENED_EXISTING_KEY: u32 = 0x0000_0002;

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

/// Which of the two things [`create_key`] actually did —
/// `RegCreateKeyExW`'s `lpdwDisposition` out-parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyDisposition {
    /// The subkey didn't already exist; this call created it.
    CreatedNewKey,
    /// The subkey already existed; this call just opened it, same as
    /// [`open_key`] would have.
    OpenedExistingKey,
}

/// Open `subkey` under `parent`, creating it first if it doesn't already
/// exist — `RegCreateKeyExW`, an idempotent "ensure this key exists" in
/// one call rather than an [`open_key`] a caller would otherwise have to
/// fall back from on `ERROR_FILE_NOT_FOUND`. The returned
/// [`KeyDisposition`] reports which of the two actually happened.
///
/// # Safety
///
/// `parent` must be a currently-valid `HKey` — one of the predefined
/// roots in this module, or a key [`open_key`]/this function previously
/// returned that hasn't been closed yet.
pub unsafe fn create_key(
    parent: HKey,
    subkey: &str,
    access: u32,
) -> Result<(HKey, KeyDisposition), crate::error::Win32Error> {
    let wide: Vec<u16> = subkey.encode_utf16().chain(core::iter::once(0)).collect();
    let mut result: HKey = core::ptr::null_mut();
    let mut disposition: u32 = 0;
    // SAFETY: `parent` is caller-supplied per this function's own safety
    // contract; `wide` is a valid, NUL-terminated UTF-16 string live for
    // the whole call; `result`/`disposition` are valid out-pointers;
    // `class`/`security_attributes` are documented-optional and null here.
    let status = unsafe {
        RegCreateKeyExW(
            parent,
            wide.as_ptr(),
            0,
            core::ptr::null_mut(),
            REG_OPTION_NON_VOLATILE,
            access,
            core::ptr::null(),
            &mut result,
            &mut disposition,
        )
    };
    if status != 0 {
        return Err(crate::error::Win32Error::from_raw(status as u32));
    }
    let disposition = if disposition == REG_OPENED_EXISTING_KEY {
        KeyDisposition::OpenedExistingKey
    } else {
        KeyDisposition::CreatedNewKey
    };
    Ok((result, disposition))
}

/// A registry value's data, decoded per its `dwType` — the seven kinds
/// this crate covers (`REG_LINK`/`REG_RESOURCE_LIST`/etc. are out of
/// scope; a symbolic-key-link and hardware-resource-descriptor format
/// respectively, neither meaningful outside their own narrow subsystems).
#[derive(Debug, Clone, PartialEq)]
pub enum RegistryValue {
    /// `REG_NONE` — no defined type, or a genuinely empty value.
    None,
    /// `REG_SZ` — an ordinary NUL-terminated string.
    Sz(alloc::string::String),
    /// `REG_EXPAND_SZ` — a string containing unexpanded `%ENVVAR%`
    /// references (e.g. `%SystemRoot%\system32`); expanding it is the
    /// caller's job (`ExpandEnvironmentStringsW`, out of this issue's
    /// scope), not this function's — it hands back the raw text exactly
    /// as stored, same as `REG_SZ`.
    ExpandSz(alloc::string::String),
    /// `REG_DWORD` — a 32-bit integer.
    Dword(u32),
    /// `REG_QWORD` — a 64-bit integer.
    Qword(u64),
    /// `REG_BINARY` — an opaque byte blob with no further structure this
    /// crate decodes.
    Binary(Vec<u8>),
    /// `REG_MULTI_SZ` — a list of strings (the on-disk encoding is a
    /// sequence of NUL-terminated UTF-16 strings, itself terminated by an
    /// extra NUL; already split apart here).
    MultiSz(Vec<alloc::string::String>),
}

/// `RegQueryValueExW`'s `dwType` out-values this crate decodes into
/// [`RegistryValue`] variants — verified against mingw-w64's own
/// `winnt.h` macros with a compiled `_Static_assert` probe. Anything else
/// (`REG_LINK`, `REG_RESOURCE_LIST`, …) falls back to
/// [`RegistryValue::None`] rather than failing outright, since a caller
/// asking for an ordinary value's data shouldn't have to know about every
/// obscure type up front.
const REG_SZ: u32 = 1;
const REG_EXPAND_SZ: u32 = 2;
const REG_BINARY: u32 = 3;
const REG_DWORD: u32 = 4;
const REG_MULTI_SZ: u32 = 7;
const REG_QWORD: u32 = 11;

fn decode_wide_string(buf: &[u8]) -> alloc::string::String {
    let mut units: Vec<u16> = buf
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    // A registry `REG_SZ`/`REG_EXPAND_SZ` value is *usually* stored with a
    // trailing NUL folded into its byte count, but the API docs
    // explicitly warn this isn't guaranteed — strip one if present rather
    // than assuming either way.
    if units.last() == Some(&0) {
        units.pop();
    }
    alloc::string::String::from_utf16_lossy(&units)
}

fn decode_multi_sz(buf: &[u8]) -> Vec<alloc::string::String> {
    let units: Vec<u16> = buf
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let mut strings = Vec::new();
    let mut start = 0;
    for (i, &unit) in units.iter().enumerate() {
        if unit == 0 {
            // A run of length zero is either two adjacent NULs (an empty
            // string genuinely stored in the list) or the final
            // terminating NUL right after the last real string's own —
            // either way, `REG_MULTI_SZ`'s own double-NUL end marker
            // means this loop should stop adding entries once it sees
            // one, not emit a trailing empty string for it.
            if i > start {
                strings.push(alloc::string::String::from_utf16_lossy(&units[start..i]));
            }
            start = i + 1;
        }
    }
    strings
}

fn decode_u32_le(buf: &[u8]) -> u32 {
    let mut bytes = [0u8; 4];
    let n = buf.len().min(4);
    bytes[..n].copy_from_slice(&buf[..n]);
    u32::from_le_bytes(bytes)
}

fn decode_u64_le(buf: &[u8]) -> u64 {
    let mut bytes = [0u8; 8];
    let n = buf.len().min(8);
    bytes[..n].copy_from_slice(&buf[..n]);
    u64::from_le_bytes(bytes)
}

/// Read `name`'s value data from `key` — `RegQueryValueExW`, decoded into
/// a [`RegistryValue`] rather than a raw byte blob plus a separate type
/// code. Uses the query-size-then-allocate idiom this crate already uses
/// elsewhere (`path::search_path`, `fs::final_path`): a first call with a
/// null data pointer reports the exact required size (and the value's
/// type) without needing to guess a starting buffer size, then a second
/// call actually reads the data into a correctly-sized buffer.
///
/// # Safety
///
/// `key` must be a currently-valid `HKey` — one of the predefined roots
/// in this module, or a key [`open_key`]/[`create_key`] previously
/// returned that hasn't been closed yet.
pub unsafe fn query_value(
    key: HKey,
    name: &str,
) -> Result<RegistryValue, crate::error::Win32Error> {
    let wide_name: Vec<u16> = name.encode_utf16().chain(core::iter::once(0)).collect();

    let mut value_type: u32 = 0;
    let mut size: u32 = 0;
    // SAFETY: `key` is caller-supplied per this function's own safety
    // contract; `wide_name` is a valid, NUL-terminated UTF-16 string live
    // for the whole call; `value_type`/`size` are valid out-pointers. A
    // null `lpData` with a non-null `lpcbData` is documented to report
    // just the type and required size, without touching any data buffer.
    let status = unsafe {
        RegQueryValueExW(
            key,
            wide_name.as_ptr(),
            core::ptr::null_mut(),
            &mut value_type,
            core::ptr::null_mut(),
            &mut size,
        )
    };
    if status != 0 {
        return Err(crate::error::Win32Error::from_raw(status as u32));
    }

    let mut buf: Vec<u8> = alloc::vec![0u8; size as usize];
    let mut actual_size = size;
    // SAFETY: `key`/`wide_name` as above; `buf` is a valid,
    // `size`-byte writable buffer matched by `actual_size` naming its
    // exact length; `value_type`/`actual_size` are valid out-pointers.
    let status = unsafe {
        RegQueryValueExW(
            key,
            wide_name.as_ptr(),
            core::ptr::null_mut(),
            &mut value_type,
            buf.as_mut_ptr(),
            &mut actual_size,
        )
    };
    if status != 0 {
        return Err(crate::error::Win32Error::from_raw(status as u32));
    }
    buf.truncate(actual_size as usize);

    Ok(match value_type {
        REG_SZ => RegistryValue::Sz(decode_wide_string(&buf)),
        REG_EXPAND_SZ => RegistryValue::ExpandSz(decode_wide_string(&buf)),
        REG_DWORD => RegistryValue::Dword(decode_u32_le(&buf)),
        REG_QWORD => RegistryValue::Qword(decode_u64_le(&buf)),
        REG_MULTI_SZ => RegistryValue::MultiSz(decode_multi_sz(&buf)),
        REG_BINARY => RegistryValue::Binary(buf),
        _ => RegistryValue::None,
    })
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

    #[test]
    fn create_key_reports_created_then_opened_disposition() {
        // `HKEY_CURRENT_USER\Software` is writable by an ordinary,
        // non-elevated process — unlike `HKEY_LOCAL_MACHINE`, which
        // `open_key`'s tests above only ever read from.
        //
        // Uniquified by this test process's own pid: this crate's CI job
        // runs `cargo test` twice (once per feature set) on the *same*
        // Windows VM, and unlike a temp-file-backed test, there's no
        // `delete_key` yet to clean this up afterward — a fixed subkey
        // name created by the first `cargo test` invocation would still
        // exist when the second invocation's own instance of this test
        // ran, making its "first call creates" assertion fail with
        // `OpenedExistingKey` instead. Confirmed by exactly that failure
        // in this crate's own CI.
        let subkey = format!(
            "Software\\rusty_win32_registry_test_create_key_{}",
            std::process::id()
        );
        let subkey = subkey.as_str();

        // SAFETY: `HKEY_CURRENT_USER` is a predefined, always-valid root.
        let (first, first_disposition) =
            unsafe { create_key(HKEY_CURRENT_USER, subkey, KEY_ALL_ACCESS) }
                .expect("RegCreateKeyExW should succeed creating a new subkey");
        assert!(!first.is_null());
        assert_eq!(first_disposition, KeyDisposition::CreatedNewKey);
        // SAFETY: `first` was just opened above and not yet closed.
        unsafe { close_key(first) }.expect("RegCloseKey should succeed");

        // Calling it again on the exact same subkey — which the call
        // above just made exist — should report the other disposition,
        // deterministically regardless of what order tests in this
        // module happen to run in.
        // SAFETY: same predefined root.
        let (second, second_disposition) =
            unsafe { create_key(HKEY_CURRENT_USER, subkey, KEY_ALL_ACCESS) }
                .expect("RegCreateKeyExW should succeed opening the now-existing subkey");
        assert!(!second.is_null());
        assert_eq!(second_disposition, KeyDisposition::OpenedExistingKey);
        // SAFETY: `second` was just opened above and not yet closed.
        unsafe { close_key(second) }.expect("RegCloseKey should succeed");
    }

    /// Every value read below lives under this exact, well-documented
    /// key — present on every Windows 10/11/Server install since these
    /// values were introduced (Windows 10, 2015), long before this
    /// crate's own `windows-latest` CI target existed.
    const CURRENT_VERSION_KEY: &str = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion";

    #[test]
    fn query_value_reads_a_well_known_reg_sz_value() {
        // SAFETY: `HKEY_LOCAL_MACHINE` is a predefined, always-valid root.
        let key = unsafe { open_key(HKEY_LOCAL_MACHINE, CURRENT_VERSION_KEY, KEY_READ) }
            .expect("RegOpenKeyExW should succeed for a well-known, always-present subkey");
        // SAFETY: `key` was just opened above and stays open for this
        // whole call.
        let value = unsafe { query_value(key, "ProductName") }
            .expect("RegQueryValueExW should succeed reading a well-known REG_SZ value");
        match value {
            RegistryValue::Sz(s) => assert!(!s.is_empty(), "ProductName should be non-empty"),
            other => panic!("expected RegistryValue::Sz, got {other:?}"),
        }
        // SAFETY: `key` is still the same valid, currently-open handle.
        unsafe { close_key(key) }.expect("RegCloseKey should succeed");
    }

    #[test]
    fn query_value_reads_a_well_known_reg_dword_value() {
        // SAFETY: `HKEY_LOCAL_MACHINE` is a predefined, always-valid root.
        let key = unsafe { open_key(HKEY_LOCAL_MACHINE, CURRENT_VERSION_KEY, KEY_READ) }
            .expect("RegOpenKeyExW should succeed for a well-known, always-present subkey");
        // SAFETY: `key` was just opened above and stays open for this
        // whole call.
        let value = unsafe { query_value(key, "CurrentMajorVersionNumber") }
            .expect("RegQueryValueExW should succeed reading a well-known REG_DWORD value");
        match value {
            RegistryValue::Dword(n) => {
                assert!(n > 0, "CurrentMajorVersionNumber should be nonzero")
            }
            other => panic!("expected RegistryValue::Dword, got {other:?}"),
        }
        // SAFETY: `key` is still the same valid, currently-open handle.
        unsafe { close_key(key) }.expect("RegCloseKey should succeed");
    }

    #[test]
    fn query_value_fails_for_a_nonexistent_value_name() {
        // SAFETY: `HKEY_LOCAL_MACHINE` is a predefined, always-valid root.
        let key = unsafe { open_key(HKEY_LOCAL_MACHINE, CURRENT_VERSION_KEY, KEY_READ) }
            .expect("RegOpenKeyExW should succeed for a well-known, always-present subkey");
        // SAFETY: `key` was just opened above and stays open for this
        // whole call.
        let err = unsafe { query_value(key, "ThisValueNameDefinitelyDoesNotExist12345") }
            .expect_err("RegQueryValueExW should fail for a nonexistent value name");
        assert_eq!(err, crate::error::Win32Error::ERROR_FILE_NOT_FOUND);
        // SAFETY: `key` is still the same valid, currently-open handle.
        unsafe { close_key(key) }.expect("RegCloseKey should succeed");
    }
}
