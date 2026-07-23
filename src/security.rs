//! Windows file/directory security (owner + DACL) — `aclapi.h`, a new
//! module added in round 2, previously excluded by `ARCHITECTURE.md`'s
//! non-goals (see `gap-analysis.md`'s "Round 2: previously out-of-scope
//! subsystems" sweep), now in scope per explicit direction. No current
//! `rush` feature asks for this yet.
//!
//! Scope: file/directory owner and DACL inspection+modification — an
//! `icacls`/`ls -l`/`chmod`/`chown`-equivalent, not a from-scratch
//! reimplementation of the whole Windows security model. SACLs/auditing,
//! privilege/token manipulation, impersonation, and low-level
//! absolute-SD plumbing are all explicitly out of scope (see
//! `gap-analysis.md`'s own design notes for this module).
//!
//! This first piece is the core round trip: path → owner `PSID`/DACL
//! `PACL`, and back. A `PSID`/`PACL`/security-descriptor blob is
//! Windows' famously variable-length, self-describing structure —
//! treated as an opaque pointer manipulated only through the OS's own
//! accessor functions, never a locally-defined fixed-layout struct the
//! way this crate mirrors most other Win32 structs.

extern crate alloc;
use alloc::vec::Vec;

#[link(name = "advapi32")]
unsafe extern "system" {
    fn GetNamedSecurityInfoW(
        object_name: *const u16,
        object_type: u32,
        security_info: u32,
        owner: *mut *mut core::ffi::c_void,
        group: *mut *mut core::ffi::c_void,
        dacl: *mut *mut core::ffi::c_void,
        sacl: *mut *mut core::ffi::c_void,
        security_descriptor: *mut *mut core::ffi::c_void,
    ) -> u32;
    fn SetNamedSecurityInfoW(
        object_name: *mut u16,
        object_type: u32,
        security_info: u32,
        owner: *mut core::ffi::c_void,
        group: *mut core::ffi::c_void,
        dacl: *mut core::ffi::c_void,
        sacl: *mut core::ffi::c_void,
    ) -> u32;
    fn GetAclInformation(
        acl: *const Acl,
        acl_information: *mut core::ffi::c_void,
        acl_information_length: u32,
        acl_information_class: u32,
    ) -> i32;
    fn GetAce(acl: *const Acl, ace_index: u32, ace: *mut *mut core::ffi::c_void) -> i32;
    fn SetEntriesInAclW(
        count: u32,
        entries: *const ExplicitAccess,
        old_acl: *const Acl,
        new_acl: *mut *mut Acl,
    ) -> u32;
    fn BuildTrusteeWithSidW(trustee: *mut Trustee, sid: *mut core::ffi::c_void);
    fn BuildTrusteeWithNameW(trustee: *mut Trustee, name: *mut u16);
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LocalFree(mem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
}

// ACL: `size_of` 8, `align_of` 2 on x86_64 — verified against
// mingw-w64's own `winnt.h` with a compiled `_Static_assert` probe, the
// same discipline this crate uses for every other struct layout it can't
// check any other way. A real, variable-length ACL has its ACEs packed
// immediately after this fixed header — not represented as a Rust field,
// the same "fixed-header-only" treatment `fs.rs`'s
// `ReparseDataBufferSymlinkHeader` already uses for a different
// variable-length Win32 structure. This crate never reads `Acl`'s own
// fields directly (not even `ace_count`) — always through
// `GetAclInformation`/`GetAce`, matching `gap-analysis.md`'s design
// notes for this module: an ACL is manipulated only through its own
// accessor functions, the same as a `PSID`.
#[repr(C)]
pub struct Acl {
    acl_revision: u8,
    sbz1: u8,
    acl_size: u16,
    ace_count: u16,
    sbz2: u16,
}
const _: () = assert!(core::mem::size_of::<Acl>() == 8);
const _: () = assert!(core::mem::align_of::<Acl>() == 2);

// ACE_HEADER: `size_of` 4 — every ACE (allow, deny, audit, …) starts
// with this fixed prefix regardless of its own specific shape.
#[repr(C)]
#[derive(Clone, Copy)]
struct AceHeader {
    ace_type: u8,
    ace_flags: u8,
    ace_size: u16,
}
const _: () = assert!(core::mem::size_of::<AceHeader>() == 4);

/// `ACE_HEADER.AceType`'s two ordinary kinds — the only ones this module
/// decodes further (audit, object-specific, callback, and other rarer
/// ACE types are out of scope; [`acl_entries`] still reports them, as
/// `Other`, rather than silently dropping them from the list).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AceKind {
    /// `ACCESS_ALLOWED_ACE_TYPE`.
    Allow,
    /// `ACCESS_DENIED_ACE_TYPE`.
    Deny,
    /// Any other `AceType` this module doesn't decode further.
    Other(u8),
}
const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
const ACCESS_DENIED_ACE_TYPE: u8 = 1;

/// `GetAclInformation`'s `ACL_INFORMATION_CLASS` value this module
/// needs — the ACE count (plus byte-usage figures this crate doesn't
/// otherwise expose).
const ACL_SIZE_INFORMATION_CLASS: u32 = 2;

// ACL_SIZE_INFORMATION: `size_of` 12 — three plain `DWORD`s, no padding.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct AclSizeInformation {
    ace_count: u32,
    acl_bytes_in_use: u32,
    acl_bytes_free: u32,
}
const _: () = assert!(core::mem::size_of::<AclSizeInformation>() == 12);

/// One decoded ACE from [`acl_entries`] — allow/deny ACEs share the same
/// `ACCESS_ALLOWED_ACE`-shaped layout (`Header`/`Mask`/a trailing `SID`),
/// so both are represented the same way here; `kind` distinguishes
/// which.
#[derive(Debug, Clone, Copy)]
pub struct AclEntry {
    pub kind: AceKind,
    /// `ACE_HEADER.AceFlags` — inheritance/inherited/audit bits, exposed
    /// raw and policy-free, matching this crate's existing convention
    /// for other bitmask fields.
    pub flags: u8,
    /// `ACCESS_MASK` — this ACE's permission bits. Only meaningful for
    /// [`AceKind::Allow`]/[`AceKind::Deny`]; `0` for any other kind
    /// (which has no `Mask` field at this same offset to begin with).
    pub mask: u32,
    /// A `PSID` pointing into the ACL's own memory (not separately
    /// owned/freed) — opaque, never decoded structurally by this crate.
    /// `None` for any ACE kind other than
    /// [`AceKind::Allow`]/[`AceKind::Deny`].
    pub sid: Option<*const core::ffi::c_void>,
}

/// `SE_OBJECT_TYPE`'s file-object variant — this module's only supported
/// object type (out of the many `SetNamedSecurityInfoW` itself supports:
/// services, printers, registry keys, …), matching its file/directory-
/// only scope. Verified against mingw-w64's own `accctrl.h`.
const SE_FILE_OBJECT: u32 = 1;

/// `SECURITY_INFORMATION` bits for [`path_security_info`] — which parts
/// of an object's security descriptor to fetch. Exposed raw and
/// policy-free, matching this crate's existing convention for other
/// bitmask fields (`registry::KEY_*`, `fs::FILE_ATTRIBUTE_*`).
/// `SecurityInfoFlags` is a plain alias, not a distinct type. Verified
/// against mingw-w64's own `winnt.h` macros with a compiled
/// `_Static_assert` probe.
pub type SecurityInfoFlags = u32;
pub const OWNER_SECURITY_INFORMATION: SecurityInfoFlags = 0x0000_0001;
pub const GROUP_SECURITY_INFORMATION: SecurityInfoFlags = 0x0000_0002;
pub const DACL_SECURITY_INFORMATION: SecurityInfoFlags = 0x0000_0004;

/// A path's owner/DACL, as returned by [`path_security_info`] (or built
/// by hand via [`PathSecurityInfo::from_raw_parts`] to pass to
/// [`set_path_security_info`]). `owner`/`dacl` are opaque `PSID`/`PACL`
/// pointers this crate never decodes structurally — pass them to
/// another Win32 API (`LookupAccountSidW`, a future `security` module
/// item) to make sense of them, or straight back into
/// [`set_path_security_info`] to write them onto a (possibly different)
/// path.
#[derive(Debug)]
pub struct PathSecurityInfo {
    // The whole `PSECURITY_DESCRIPTOR` block `GetNamedSecurityInfoW`
    // allocated — `owner`/`dacl` below point *into* this same block (a
    // self-relative security descriptor), so freeing it on `Drop` frees
    // them too; they are never separately `LocalFree`d. `None` (nothing
    // to free) for a value built by hand via `from_raw_parts` rather
    // than returned from `path_security_info`.
    security_descriptor: Option<*mut core::ffi::c_void>,
    owner: Option<*mut core::ffi::c_void>,
    dacl: Option<*mut core::ffi::c_void>,
}

impl PathSecurityInfo {
    /// Build a `PathSecurityInfo` from raw, already-valid `PSID`/`PACL`
    /// pointers obtained elsewhere — for a caller constructing a
    /// security descriptor to apply via [`set_path_security_info`]
    /// rather than one read back via [`path_security_info`]. Neither
    /// pointer is freed on `Drop`: this crate has no allocator-tracking
    /// to know whether it safely can, so ownership/lifetime of whatever
    /// `owner`/`dacl` point at remains entirely the caller's
    /// responsibility.
    ///
    /// # Safety
    ///
    /// `owner`, if `Some`, must be a valid `PSID` for as long as this
    /// value exists and for every operation it's passed to. Same for
    /// `dacl` as a valid `PACL`.
    pub unsafe fn from_raw_parts(
        owner: Option<*mut core::ffi::c_void>,
        dacl: Option<*mut core::ffi::c_void>,
    ) -> Self {
        PathSecurityInfo {
            security_descriptor: None,
            owner,
            dacl,
        }
    }

    /// The object's owner `PSID`, if [`path_security_info`] was asked
    /// for it (`OWNER_SECURITY_INFORMATION` in its `info` flags) —
    /// opaque, never decoded structurally by this crate.
    pub fn owner(&self) -> Option<*mut core::ffi::c_void> {
        self.owner
    }

    /// The object's DACL `PACL`, if [`path_security_info`] was asked for
    /// it (`DACL_SECURITY_INFORMATION` in its `info` flags) — opaque,
    /// never decoded structurally by this crate.
    pub fn dacl(&self) -> Option<*mut core::ffi::c_void> {
        self.dacl
    }
}

impl Drop for PathSecurityInfo {
    fn drop(&mut self) {
        if let Some(sd) = self.security_descriptor {
            // SAFETY: `sd` is the exact `PSECURITY_DESCRIPTOR` pointer
            // `GetNamedSecurityInfoW` allocated for this value and
            // hasn't been freed yet — freed here exactly once, on this
            // value's only path to being dropped.
            let _ = unsafe { LocalFree(sd) };
        }
    }
}

/// Read `path`'s owner and/or DACL — `GetNamedSecurityInfoW`. `info`
/// selects which parts to fetch (e.g. [`OWNER_SECURITY_INFORMATION`] `|`
/// [`DACL_SECURITY_INFORMATION`]); a part not requested comes back as
/// `None` from the result's own accessor, not an error.
///
/// Reports failure via its own return value directly — never
/// `GetLastError` — so a nonzero return is passed straight to
/// [`crate::error::Win32Error::from_raw`] rather than `Win32Error::last`.
pub fn path_security_info(
    path: &str,
    info: SecurityInfoFlags,
) -> Result<PathSecurityInfo, crate::error::Win32Error> {
    let wide: Vec<u16> = path.encode_utf16().chain(core::iter::once(0)).collect();
    let mut owner: *mut core::ffi::c_void = core::ptr::null_mut();
    let mut group: *mut core::ffi::c_void = core::ptr::null_mut();
    let mut dacl: *mut core::ffi::c_void = core::ptr::null_mut();
    let mut security_descriptor: *mut core::ffi::c_void = core::ptr::null_mut();
    // SAFETY: `wide` is a valid, NUL-terminated UTF-16 string live for
    // the whole call; every out-pointer is a valid pointer to one of
    // this function's own locals; `sacl` is null since this module's
    // scope explicitly excludes SACLs.
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            info,
            &mut owner,
            &mut group,
            &mut dacl,
            core::ptr::null_mut(),
            &mut security_descriptor,
        )
    };
    if status != 0 {
        return Err(crate::error::Win32Error::from_raw(status));
    }
    Ok(PathSecurityInfo {
        security_descriptor: if security_descriptor.is_null() {
            None
        } else {
            Some(security_descriptor)
        },
        owner: if owner.is_null() { None } else { Some(owner) },
        dacl: if dacl.is_null() { None } else { Some(dacl) },
    })
}

/// Write `path`'s owner and/or DACL — `SetNamedSecurityInfoW`. Which
/// parts actually get written is inferred from `info` itself: `owner()`
/// present sets the owner, `dacl()` present sets the DACL; a part that's
/// `None` is left untouched on `path` (`SetNamedSecurityInfoW`'s own
/// null-pointer-means-"don't touch this part" convention, not a
/// separate flags parameter this crate would otherwise have to keep in
/// sync with `info`'s own contents).
///
/// # Safety
///
/// Every non-`None` pointer `info` carries (`owner()`/`dacl()`) must
/// still be a valid `PSID`/`PACL` at the time of this call.
pub unsafe fn set_path_security_info(
    path: &str,
    info: &PathSecurityInfo,
) -> Result<(), crate::error::Win32Error> {
    let mut wide: Vec<u16> = path.encode_utf16().chain(core::iter::once(0)).collect();
    let mut security_info: u32 = 0;
    if info.owner.is_some() {
        security_info |= OWNER_SECURITY_INFORMATION;
    }
    if info.dacl.is_some() {
        security_info |= DACL_SECURITY_INFORMATION;
    }
    // SAFETY: `wide` is a valid, mutable, NUL-terminated UTF-16 buffer
    // (`SetNamedSecurityInfoW` takes `LPWSTR`, not `LPCWSTR`, though it
    // doesn't document actually mutating it); `info.owner`/`info.dacl`
    // are valid per this function's own safety contract, or null
    // (documented as "leave this part unchanged"); `group`/`sacl` are
    // always null, since this module doesn't track a group SID and
    // explicitly excludes SACLs.
    let status = unsafe {
        SetNamedSecurityInfoW(
            wide.as_mut_ptr(),
            SE_FILE_OBJECT,
            security_info,
            info.owner.unwrap_or(core::ptr::null_mut()),
            core::ptr::null_mut(),
            info.dacl.unwrap_or(core::ptr::null_mut()),
            core::ptr::null_mut(),
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(crate::error::Win32Error::from_raw(status))
    }
}

/// Enumerate `acl`'s ACEs — `GetAclInformation` (for the ACE count) plus
/// one `GetAce` call per entry — turning an opaque DACL/SACL into the
/// human-readable permission list `icacls`/`ls -l` displays. Each `GetAce`
/// call hands back a pointer into `acl`'s own existing memory (not a
/// fresh allocation), so the returned entries' [`AclEntry::sid`] pointers
/// stay valid only as long as `acl` itself does.
///
/// # Safety
///
/// `acl` must be a valid, currently-live `PACL` — e.g. one
/// [`PathSecurityInfo::dacl`] returned, kept alive (the `PathSecurityInfo`
/// it came from not yet dropped) for as long as this call and the
/// returned entries are in use.
pub unsafe fn acl_entries(
    acl: *const Acl,
) -> Result<alloc::vec::Vec<AclEntry>, crate::error::Win32Error> {
    let mut size_info = AclSizeInformation::default();
    // SAFETY: `acl` is caller-supplied per this function's own safety
    // contract; `size_info` is a valid out-pointer of exactly the size
    // named by `acl_information_length`.
    let ok = unsafe {
        GetAclInformation(
            acl,
            (&mut size_info as *mut AclSizeInformation).cast(),
            core::mem::size_of::<AclSizeInformation>() as u32,
            ACL_SIZE_INFORMATION_CLASS,
        )
    };
    if ok == 0 {
        return Err(crate::error::Win32Error::last());
    }

    let mut entries = alloc::vec::Vec::with_capacity(size_info.ace_count as usize);
    for index in 0..size_info.ace_count {
        let mut ace_ptr: *mut core::ffi::c_void = core::ptr::null_mut();
        // SAFETY: `acl` is the same caller-supplied, valid `PACL`;
        // `index` is within `0..ace_count`, the range `GetAclInformation`
        // just reported; `ace_ptr` is a valid out-pointer.
        let ok = unsafe { GetAce(acl, index, &mut ace_ptr) };
        if ok == 0 {
            return Err(crate::error::Win32Error::last());
        }
        // SAFETY: a successful `GetAce` guarantees `ace_ptr` points to a
        // real ACE at least `ACE_HEADER`-sized (every ACE, regardless of
        // its specific kind, starts with this fixed prefix).
        let header: AceHeader = unsafe { core::ptr::read_unaligned(ace_ptr.cast()) };
        let kind = match header.ace_type {
            ACCESS_ALLOWED_ACE_TYPE => AceKind::Allow,
            ACCESS_DENIED_ACE_TYPE => AceKind::Deny,
            other => AceKind::Other(other),
        };
        let (mask, sid) = if matches!(kind, AceKind::Allow | AceKind::Deny) {
            // SAFETY: an allow/deny ACE shares `ACCESS_ALLOWED_ACE`'s
            // layout, guaranteed at least 12 bytes (`Header` + `Mask` +
            // the trailing SID's first `DWORD`) by Windows itself for
            // these two ACE types.
            let mask: u32 = unsafe { core::ptr::read_unaligned(ace_ptr.byte_add(4).cast()) };
            // SAFETY: same guarantee — byte offset 8 is
            // `ACCESS_ALLOWED_ACE::SidStart`, the trailing SID's start.
            let sid = unsafe { ace_ptr.byte_add(8) }.cast_const();
            (mask, Some(sid))
        } else {
            (0, None)
        };
        entries.push(AclEntry {
            kind,
            flags: header.ace_flags,
            mask,
            sid,
        });
    }
    Ok(entries)
}

/// `TRUSTEE_FORM` — whether a [`Trustee`]'s name field holds a `PSID` or
/// a wide string name. Only these two ordinary forms are supported
/// (`TRUSTEE_BAD_FORM` and the object-specific `_AND_SID`/`_AND_NAME`
/// variants are out of scope). Verified against mingw-w64's own
/// `aclapi.h` with a compiled `_Static_assert` probe.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrusteeForm {
    Sid = 0,
    Name = 1,
}

/// `TRUSTEE_TYPE` — what kind of principal a [`Trustee`] names. Only
/// the commonly-used variants are exposed; the rest (`TRUSTEE_IS_DOMAIN`/
/// `_ALIAS`/`_DELETED`/`_INVALID`/`_COMPUTER`) aren't needed for an
/// ordinary user/group ACL entry.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrusteeType {
    Unknown = 0,
    User = 1,
    Group = 2,
    WellKnownGroup = 5,
}

// TRUSTEE_W: `size_of` 32, `align_of` 8 — verified against mingw-w64's
// own `aclapi.h` with a compiled `_Static_assert` probe. Genuinely
// fixed-size, unlike `Acl`/`PSID` — an ordinary FFI-mirror struct with
// full field access, per `gap-analysis.md`'s design notes for this
// module.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Trustee {
    // "Multiple trustee" chaining is an obscure, rarely-used mechanism
    // this crate doesn't support — `BuildTrusteeWithSidW`/
    // `BuildTrusteeWithNameW` always set these two fields to "none"
    // themselves, so they stay private rather than exposing a knob with
    // only one ever-correct value.
    multiple_trustee: *mut Trustee,
    multiple_trustee_operation: i32,
    pub trustee_form: TrusteeForm,
    pub trustee_type: TrusteeType,
    /// The real `TRUSTEE_W::ptstrName` field — either a `PSID` (when
    /// `trustee_form` is [`TrusteeForm::Sid`]) or a NUL-terminated wide
    /// string (when [`TrusteeForm::Name`]), reinterpreted either way
    /// through this same `LPWCH`-typed slot, matching the real
    /// `TRUSTEE_W`'s own union-by-convention use of this field.
    pub name: *mut u16,
}

/// Build a `Trustee` naming a `PSID` — `BuildTrusteeWithSidW`. The
/// resulting `Trustee`'s own `name` pointer is `sid` itself (matching
/// `TRUSTEE_W`'s `ptstrName` field's `TrusteeForm::Sid`-reinterpreted-
/// as-`PSID` convention) — nothing is copied.
///
/// `TrusteeType::Unknown` matches `BuildTrusteeWithSidW`'s own behavior:
/// it always sets `TrusteeType` to `TRUSTEE_IS_UNKNOWN`, never inspecting
/// `sid` to guess whether it names a user/group/well-known-group. A
/// caller wanting a specific [`TrusteeType`] recorded sets `.trustee_type`
/// on the result afterward (not exposed as a parameter here, matching
/// `BuildTrusteeWithSidW`'s own real signature).
///
/// # Safety
///
/// `sid` must be a valid `PSID` for as long as the returned `Trustee`
/// (and anything it's passed to, e.g. an [`ExplicitAccess`] entry given
/// to [`build_acl`]) is in use.
pub unsafe fn build_trustee_with_sid(sid: *mut core::ffi::c_void) -> Trustee {
    let mut trustee = core::mem::MaybeUninit::<Trustee>::uninit();
    // SAFETY: `sid` is caller-supplied per this function's own safety
    // contract; `trustee` is a valid, correctly-sized out-pointer.
    unsafe { BuildTrusteeWithSidW(trustee.as_mut_ptr(), sid) };
    // SAFETY: `BuildTrusteeWithSidW` is documented to always fully
    // initialize every field of the `TRUSTEE_W` it's given.
    unsafe { trustee.assume_init() }
}

/// Build a `Trustee` naming a wide-string principal (e.g. `"DOMAIN\name"`,
/// or a `S-1-5-...` SID string) — `BuildTrusteeWithNameW`. Unlike
/// [`build_trustee_with_sid`], this doesn't copy `name`'s bytes either —
/// the returned `Trustee`'s own `name` pointer is `name.as_mut_ptr()`
/// itself. Takes an already-built, NUL-terminated wide buffer (not `&str`,
/// diverging from this issue's literal signature) for exactly that
/// reason: an internally-built temporary `Vec<u16>` would be freed the
/// moment this function returned, leaving the result's `name` pointer
/// dangling — the caller must own a buffer that actually outlives the
/// `Trustee`.
///
/// # Safety
///
/// `name` must be a valid, NUL-terminated UTF-16 string, kept alive (not
/// dropped or reallocated) for as long as the returned `Trustee` (and
/// anything it's passed to) is in use.
pub unsafe fn build_trustee_with_name(name: &mut [u16]) -> Trustee {
    let mut trustee = core::mem::MaybeUninit::<Trustee>::uninit();
    // SAFETY: `name` is caller-supplied per this function's own safety
    // contract; `trustee` is a valid, correctly-sized out-pointer.
    unsafe { BuildTrusteeWithNameW(trustee.as_mut_ptr(), name.as_mut_ptr()) };
    // SAFETY: `BuildTrusteeWithNameW` is documented to always fully
    // initialize every field of the `TRUSTEE_W` it's given.
    unsafe { trustee.assume_init() }
}

/// `ACCESS_MODE` — what an [`ExplicitAccess`] entry does to the ACL being
/// built. Only the four ordinary modes are exposed
/// (`SET_AUDIT_SUCCESS`/`SET_AUDIT_FAILURE` are SACL/auditing-only,
/// explicitly out of this module's scope).
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    Grant = 1,
    Set = 2,
    Deny = 3,
    Revoke = 4,
}

// EXPLICIT_ACCESS_W: `size_of` 48, `Trustee` at offset 16 — verified
// against mingw-w64's own `aclapi.h` with a compiled `_Static_assert`
// probe. Genuinely fixed-size, an ordinary FFI-mirror struct with full
// field access, the same as `Trustee` above.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ExplicitAccess {
    pub access_permissions: u32,
    pub access_mode: AccessMode,
    pub inheritance: u32,
    pub trustee: Trustee,
}
const _: () = assert!(core::mem::size_of::<ExplicitAccess>() == 48);
const _: () = assert!(core::mem::offset_of!(ExplicitAccess, trustee) == 16);

/// [`build_acl`]'s result — the new `PACL` `SetEntriesInAclW` allocated,
/// freed via `LocalFree` on `Drop` the same way [`PathSecurityInfo`]
/// frees its security-descriptor block.
pub struct BuiltAcl {
    acl: *mut Acl,
}

impl BuiltAcl {
    /// The built ACL, ready to pass to [`set_path_security_info`] (via
    /// [`PathSecurityInfo::from_raw_parts`]) or [`acl_entries`].
    pub fn as_ptr(&self) -> *const Acl {
        self.acl
    }
}

impl Drop for BuiltAcl {
    fn drop(&mut self) {
        // SAFETY: `self.acl` is the exact `PACL` pointer
        // `SetEntriesInAclW` allocated for this value and hasn't been
        // freed yet — freed here exactly once, on this value's only path
        // to being dropped.
        let _ = unsafe { LocalFree(self.acl.cast()) };
    }
}

/// Build a new ACL from `existing` (if any) plus `entries` —
/// `SetEntriesInAclW`, the primitive behind `icacls /grant`/`/deny`.
/// `existing = None` builds a fresh ACL from just `entries`, with
/// nothing carried over.
///
/// Reports failure via its own return value directly — never
/// `GetLastError` — so a nonzero return is passed straight to
/// [`crate::error::Win32Error::from_raw`] rather than `Win32Error::last`.
///
/// # Safety
///
/// `existing`, if `Some`, must be a valid `PACL`. Every [`Trustee`] in
/// `entries` must carry a still-valid `PSID`/name pointer for the
/// duration of this call.
pub unsafe fn build_acl(
    existing: Option<*const Acl>,
    entries: &[ExplicitAccess],
) -> Result<BuiltAcl, crate::error::Win32Error> {
    let mut new_acl: *mut Acl = core::ptr::null_mut();
    // SAFETY: `entries` is a valid slice, passed with its own exact
    // length; `existing` is caller-supplied per this function's own
    // safety contract, or null (documented as "build from just
    // `entries`"); `new_acl` is a valid out-pointer.
    let status = unsafe {
        SetEntriesInAclW(
            entries.len() as u32,
            entries.as_ptr(),
            existing.unwrap_or(core::ptr::null()),
            &mut new_acl,
        )
    };
    if status != 0 {
        return Err(crate::error::Win32Error::from_raw(status));
    }
    Ok(BuiltAcl { acl: new_acl })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_security_info_reports_an_owner_and_dacl_for_a_real_file() {
        let path = std::env::temp_dir().join("rusty_win32_security_test_path_info.txt");
        std::fs::write(&path, b"hello").expect("creating the test file should succeed");
        let path_str = path.to_str().expect("temp path should be valid UTF-8");

        let info = path_security_info(
            path_str,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
        )
        .expect("GetNamedSecurityInfoW should succeed for a real file");
        assert!(
            info.owner().is_some(),
            "a real file should have an owner SID"
        );
        assert!(info.dacl().is_some(), "a real file should have a DACL");

        std::fs::remove_file(&path).expect("removing the test file should succeed");
    }

    #[test]
    fn set_path_security_info_round_trips_the_files_own_owner_and_dacl() {
        let path = std::env::temp_dir().join("rusty_win32_security_test_set_info.txt");
        std::fs::write(&path, b"hello").expect("creating the test file should succeed");
        let path_str = path.to_str().expect("temp path should be valid UTF-8");

        let info = path_security_info(
            path_str,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
        )
        .expect("GetNamedSecurityInfoW should succeed for a real file");

        // Re-applying a file's own current owner/DACL back onto itself
        // is always permitted (no elevated privilege needed to set an
        // object's owner to one you already are, or a DACL you already
        // fully control) — a safe, self-contained round trip that
        // doesn't depend on SID-construction primitives this crate
        // doesn't have yet (`well_known_sid`/`ConvertStringSidToSidW`
        // are later round-2 items).
        // SAFETY: `info`'s pointers are still valid — it hasn't been
        // dropped, and the file they describe hasn't changed since.
        unsafe { set_path_security_info(path_str, &info) }
            .expect("SetNamedSecurityInfoW should succeed re-applying the file's own owner/DACL");

        std::fs::remove_file(&path).expect("removing the test file should succeed");
    }

    #[test]
    fn path_security_info_fails_for_a_nonexistent_path() {
        let path =
            std::env::temp_dir().join("rusty_win32_security_test_missing_definitely_not_here.txt");
        let path_str = path.to_str().expect("temp path should be valid UTF-8");

        let err = path_security_info(path_str, OWNER_SECURITY_INFORMATION)
            .expect_err("GetNamedSecurityInfoW should fail for a nonexistent path");
        assert_eq!(err, crate::error::Win32Error::ERROR_FILE_NOT_FOUND);
    }

    #[test]
    fn acl_entries_reports_at_least_one_recognized_ace_for_a_real_files_dacl() {
        let path = std::env::temp_dir().join("rusty_win32_security_test_acl_entries.txt");
        std::fs::write(&path, b"hello").expect("creating the test file should succeed");
        let path_str = path.to_str().expect("temp path should be valid UTF-8");

        let info = path_security_info(path_str, DACL_SECURITY_INFORMATION)
            .expect("GetNamedSecurityInfoW should succeed for a real file");
        let dacl = info
            .dacl()
            .expect("a real file should have a DACL")
            .cast_const()
            .cast::<Acl>();

        // SAFETY: `dacl` is a valid `PACL` from `info`, which stays alive
        // (not yet dropped) for this whole call.
        let entries =
            unsafe { acl_entries(dacl) }.expect("GetAclInformation/GetAce should succeed");
        assert!(
            !entries.is_empty(),
            "a real file's DACL should have at least one ACE"
        );
        assert!(
            entries
                .iter()
                .any(|e| matches!(e.kind, AceKind::Allow | AceKind::Deny)),
            "a real file's DACL should have at least one ordinary allow/deny ACE, got: {entries:?}"
        );
        for entry in &entries {
            if matches!(entry.kind, AceKind::Allow | AceKind::Deny) {
                assert!(
                    entry.sid.is_some(),
                    "an allow/deny ACE should always carry a SID"
                );
            }
        }

        std::fs::remove_file(&path).expect("removing the test file should succeed");
    }

    #[test]
    fn build_acl_produces_an_acl_with_the_requested_grant_entry() {
        let path = std::env::temp_dir().join("rusty_win32_security_test_build_acl.txt");
        std::fs::write(&path, b"hello").expect("creating the test file should succeed");
        let path_str = path.to_str().expect("temp path should be valid UTF-8");

        // No `well_known_sid`/`ConvertStringSidToSidW` yet (later round-2
        // items) to construct a fresh SID from scratch — this file's own
        // owner is a real, already-obtained SID good enough to build a
        // self-contained test around.
        let info = path_security_info(path_str, OWNER_SECURITY_INFORMATION)
            .expect("GetNamedSecurityInfoW should succeed for a real file");
        let owner = info.owner().expect("a real file should have an owner SID");

        const GENERIC_READ: u32 = 0x8000_0000;
        // SAFETY: `owner` is a valid `PSID` from `info`, which stays
        // alive (not yet dropped) for this whole test.
        let mut trustee = unsafe { build_trustee_with_sid(owner) };
        trustee.trustee_type = TrusteeType::User;
        let entry = ExplicitAccess {
            access_permissions: GENERIC_READ,
            access_mode: AccessMode::Grant,
            inheritance: 0,
            trustee,
        };

        // SAFETY: `entries` (just built above) carries a still-valid
        // `PSID` pointer; `existing` is `None`.
        let built = unsafe { build_acl(None, &[entry]) }
            .expect("SetEntriesInAclW should succeed building a fresh ACL");

        // SAFETY: `built` is still alive (not yet dropped), so its ACL
        // pointer is still valid.
        let acl_entries_found = unsafe { acl_entries(built.as_ptr()) }
            .expect("GetAclInformation/GetAce should succeed");
        assert_eq!(
            acl_entries_found.len(),
            1,
            "a fresh ACL built from exactly one entry should have exactly one ACE"
        );
        assert_eq!(acl_entries_found[0].kind, AceKind::Allow);
        assert_eq!(acl_entries_found[0].mask, GENERIC_READ);

        std::fs::remove_file(&path).expect("removing the test file should succeed");
    }

    #[test]
    fn build_trustee_with_sid_names_the_given_sid_as_unknown_type() {
        let path = std::env::temp_dir().join("rusty_win32_security_test_build_trustee_sid.txt");
        std::fs::write(&path, b"hello").expect("creating the test file should succeed");
        let path_str = path.to_str().expect("temp path should be valid UTF-8");

        let info = path_security_info(path_str, OWNER_SECURITY_INFORMATION)
            .expect("GetNamedSecurityInfoW should succeed for a real file");
        let owner = info.owner().expect("a real file should have an owner SID");

        // SAFETY: `owner` is a valid `PSID` from `info`, which stays
        // alive (not yet dropped) for this whole test.
        let trustee = unsafe { build_trustee_with_sid(owner) };
        assert_eq!(trustee.trustee_form, TrusteeForm::Sid);
        // `BuildTrusteeWithSidW` always reports `TRUSTEE_IS_UNKNOWN` —
        // it never inspects the SID to guess a more specific type.
        assert_eq!(trustee.trustee_type, TrusteeType::Unknown);
        assert_eq!(
            trustee.name.cast::<core::ffi::c_void>(),
            owner,
            "the trustee's name pointer should be the SID itself, not a copy"
        );

        std::fs::remove_file(&path).expect("removing the test file should succeed");
    }

    #[test]
    fn build_trustee_with_name_names_the_given_wide_string() {
        let mut wide: alloc::vec::Vec<u16> = "Everyone"
            .encode_utf16()
            .chain(core::iter::once(0))
            .collect();
        let expected_ptr = wide.as_mut_ptr();

        // SAFETY: `wide` is a valid, NUL-terminated UTF-16 buffer that
        // stays alive (not dropped/reallocated) for this whole test.
        let trustee = unsafe { build_trustee_with_name(&mut wide) };
        assert_eq!(trustee.trustee_form, TrusteeForm::Name);
        assert_eq!(
            trustee.name, expected_ptr,
            "the trustee's name pointer should be the wide buffer itself, not a copy"
        );
    }
}
