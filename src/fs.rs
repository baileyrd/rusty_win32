//! `GetFileAttributesExW`/`GetFileInformationByHandle` — the Windows
//! counterpart of Unix `stat`/`fstat`. `rusty_libc` can answer "is this a
//! directory/is this executable/how big is this file" with a `stat` mode
//! and size field; this module is the Windows analog, needed for `ls`,
//! globbing, and the `[ -d/-f/... ]` test operators to work there too.
//!
//! Deciding what a given `FILE_ATTRIBUTE_*` bit or field means to a caller
//! (e.g. "is this a directory") is the caller's policy — this module only
//! exposes the raw attributes, the same way [`crate::console`]'s
//! `ENABLE_*` bits are exposed without a raw-mode recipe baked in.
//!
//! This module's several-item surface (two functions, two result structs,
//! and the `FILE_ATTRIBUTE_*` constants) is deliberately not re-exported at
//! the crate root, the same reasoning [`crate::job`] documents for its own
//! multi-item surface: reach it via `rusty_win32::fs::*`.

use crate::error::Win32Error;
use crate::handle::RawHandle;
use crate::time::Timespec;

extern crate alloc;
use alloc::vec::Vec;

// FILE_ATTRIBUTE_* bits (GetFileAttributesExW's/GetFileInformationByHandle's
// dwFileAttributes) this crate has an immediate use for. Windows defines
// several more (COMPRESSED, ENCRYPTED, TEMPORARY, ...) — out of scope until
// something actually needs them.
pub const FILE_ATTRIBUTE_READONLY: u32 = 0x0001;
pub const FILE_ATTRIBUTE_HIDDEN: u32 = 0x0002;
pub const FILE_ATTRIBUTE_SYSTEM: u32 = 0x0004;
pub const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0010;
pub const FILE_ATTRIBUTE_ARCHIVE: u32 = 0x0020;
pub const FILE_ATTRIBUTE_NORMAL: u32 = 0x0080;
/// Set on symlinks/junctions/mount points — the bit a future symlink-aware
/// `stat`-vs-`lstat` distinction would key on.
pub const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;

/// `GetFileAttributesExW`'s `GET_FILEEX_INFO_LEVELS::GetFileExInfoStandard`
/// — the only info level Windows currently defines.
const GET_FILE_EX_INFO_STANDARD: u32 = 0;

/// `CreateSymbolicLinkW`'s `dwFlags`: the target is a directory, not a file
/// — Windows symlinks (unlike Unix ones) need to know this up front rather
/// than discovering it by following the link.
pub const SYMBOLIC_LINK_FLAG_DIRECTORY: u32 = 0x1;
/// Lets `CreateSymbolicLinkW` succeed without elevation, given either
/// Developer Mode enabled or the `SeCreateSymbolicLinkPrivilege` user right
/// — [`create_symlink`] always includes this bit internally, since there's
/// no reason a caller would ever want to *forbid* the unprivileged path
/// when the OS supports it (this crate's Windows 10 1809+ floor postdates
/// its introduction).
const SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE: u32 = 0x2;

const FILE_SHARE_READ: u32 = 0x0000_0001;
const FILE_SHARE_WRITE: u32 = 0x0000_0002;
const OPEN_EXISTING: u32 = 3;
/// `CreateFileW`'s flag to open the reparse point itself (its stored
/// target) rather than following it — the Windows analog of `O_NOFOLLOW`,
/// and the primitive [`readlink`] needs to report a link's target instead
/// of whatever the link resolves to.
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
/// `CreateFileW`'s flag required to open a directory (a plain `CreateFileW`
/// on a directory path otherwise fails) — needed here since a directory
/// symlink/junction is exactly the reparse-point case [`readlink`] targets.
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

/// `DeviceIoControl`'s IO control code to read a reparse point's raw target
/// data — there is no ordinary Win32 API for this; it's an NT-native
/// `DeviceIoControl` request, the same way Windows repurposes I/O
/// completion ports for job notifications in [`crate::job`] rather than
/// defining a dedicated high-level function.
const FSCTL_GET_REPARSE_POINT: u32 = 0x0009_00A8;
/// The maximum size `DeviceIoControl(FSCTL_GET_REPARSE_POINT, ...)` ever
/// needs — a documented Windows constant, not one this crate invents.
const MAXIMUM_REPARSE_DATA_BUFFER_SIZE: usize = 16 * 1024;
/// `REPARSE_DATA_BUFFER::ReparseTag` value for a symlink (as opposed to a
/// junction/mount point or a vendor-specific reparse point) — the only tag
/// [`readlink`] currently understands how to parse.
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;

// FILETIME: `size_of` 8, `align_of` 4 on x86_64 — mirrors `time.rs`'s
// private struct of the same shape; duplicated locally rather than shared,
// matching this crate's existing per-module-locality convention for tiny
// FFI-mirror structs (e.g. `WAIT_TIMEOUT` is redefined in `process.rs`,
// `console.rs`, and `job.rs` rather than centralized).
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct FileTime {
    low: u32,
    high: u32,
}
const _: () = assert!(core::mem::size_of::<FileTime>() == 8);
const _: () = assert!(core::mem::align_of::<FileTime>() == 4);

/// 100ns ticks between the FILETIME epoch (1601-01-01) and the Unix epoch
/// (1970-01-01) — the same standard conversion constant `time.rs` uses.
const FILETIME_UNIX_EPOCH_DIFF_100NS: i64 = 116_444_736_000_000_000;
const HUNDRED_NS_PER_SEC: i64 = 10_000_000;
const NANOS_PER_HUNDRED_NS: i64 = 100;

fn filetime_to_timespec(ft: FileTime) -> Timespec {
    let ticks_100ns =
        (i64::from(ft.high) << 32 | i64::from(ft.low)) - FILETIME_UNIX_EPOCH_DIFF_100NS;
    let secs = ticks_100ns.div_euclid(HUNDRED_NS_PER_SEC);
    let remainder_100ns = ticks_100ns.rem_euclid(HUNDRED_NS_PER_SEC);
    Timespec {
        secs,
        nanos: (remainder_100ns * NANOS_PER_HUNDRED_NS) as u32,
    }
}

// WIN32_FILE_ATTRIBUTE_DATA: `size_of` 36, `align_of` 4 on x86_64. Verified
// against mingw-w64's `minwinbase.h` the same way as every other struct in
// this crate (a `_Static_assert` probe compiled with
// `x86_64-w64-mingw32-gcc` against the real header).
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Win32FileAttributeData {
    file_attributes: u32,
    creation_time: FileTime,
    last_access_time: FileTime,
    last_write_time: FileTime,
    file_size_high: u32,
    file_size_low: u32,
}
const _: () = assert!(core::mem::size_of::<Win32FileAttributeData>() == 36);
const _: () = assert!(core::mem::align_of::<Win32FileAttributeData>() == 4);

// BY_HANDLE_FILE_INFORMATION: `size_of` 52, `align_of` 4 on x86_64. Verified
// the same way.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct ByHandleFileInformation {
    file_attributes: u32,
    creation_time: FileTime,
    last_access_time: FileTime,
    last_write_time: FileTime,
    volume_serial_number: u32,
    file_size_high: u32,
    file_size_low: u32,
    number_of_links: u32,
    file_index_high: u32,
    file_index_low: u32,
}
const _: () = assert!(core::mem::size_of::<ByHandleFileInformation>() == 52);
const _: () = assert!(core::mem::align_of::<ByHandleFileInformation>() == 4);

// REPARSE_DATA_BUFFER's fixed header, symlink variant only (the
// `SymbolicLinkReparseBuffer` arm of the real struct's union — this crate
// doesn't parse `MountPointReparseBuffer`/`GenericReparseBuffer`, see
// `readlink`'s own doc comment): `size_of`/offset-of-`Flags` verified
// against mingw-w64's `ddk/ntifs.h` the same way as every other struct in
// this crate (a `_Static_assert` probe compiled with
// `x86_64-w64-mingw32-gcc` against the real header — mingw-w64 ships this
// one under its DDK headers, not the ordinary Win32 set, since reparse
// points are an NT-native mechanism with no ordinary Win32 struct for it).
// `PathBuffer` (variable-length UTF-16 data) immediately follows this fixed
// 20-byte header; not represented as a Rust field since its length isn't
// known until `reparse_data_length` is read.
#[repr(C)]
#[derive(Clone, Copy)]
struct ReparseDataBufferSymlinkHeader {
    reparse_tag: u32,
    reparse_data_length: u16,
    reserved: u16,
    substitute_name_offset: u16,
    substitute_name_length: u16,
    print_name_offset: u16,
    print_name_length: u16,
    flags: u32,
}
const _: () = assert!(core::mem::size_of::<ReparseDataBufferSymlinkHeader>() == 20);
const _: () = assert!(core::mem::offset_of!(ReparseDataBufferSymlinkHeader, flags) == 16);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetFileAttributesExW(
        file_name: *const u16,
        info_level_id: u32,
        file_information: *mut core::ffi::c_void,
    ) -> i32;
    fn GetFileInformationByHandle(
        file: RawHandle,
        file_information: *mut ByHandleFileInformation,
    ) -> i32;
    fn CreateFileW(
        file_name: *const u16,
        desired_access: u32,
        share_mode: u32,
        security_attributes: *const core::ffi::c_void,
        creation_disposition: u32,
        flags_and_attributes: u32,
        template_file: RawHandle,
    ) -> RawHandle;
    // `CreateSymbolicLinkW` is documented to return `BOOLEAN` (an 8-bit
    // value), not the ordinary 32-bit `BOOL` every other function in this
    // crate returns — declared as `u8` rather than `i32` so this doesn't
    // read undefined bits above the byte the real ABI actually guarantees.
    fn CreateSymbolicLinkW(
        symlink_file_name: *const u16,
        target_file_name: *const u16,
        flags: u32,
    ) -> u8;
    fn GetFinalPathNameByHandleW(
        file: RawHandle,
        file_path: *mut u16,
        file_path_size: u32,
        flags: u32,
    ) -> u32;
    fn DeviceIoControl(
        device: RawHandle,
        io_control_code: u32,
        in_buffer: *const core::ffi::c_void,
        in_buffer_size: u32,
        out_buffer: *mut core::ffi::c_void,
        out_buffer_size: u32,
        bytes_returned: *mut u32,
        overlapped: *mut core::ffi::c_void,
    ) -> i32;
}

/// `stat`-by-path result — attributes, timestamps, and size, via
/// `GetFileAttributesExW`. Does not require opening a handle to the file
/// first (unlike [`stat_by_handle`]), the same tradeoff Unix `stat` makes
/// over `fstat`.
#[derive(Debug, Clone, Copy)]
pub struct FileInfo {
    pub attributes: u32,
    pub creation_time: Timespec,
    pub last_access_time: Timespec,
    pub last_write_time: Timespec,
    pub size: u64,
}

/// `stat`-by-open-handle result — everything [`FileInfo`] has, plus
/// `volume_serial_number`/`file_index` (Windows' closest analog of a Unix
/// `(st_dev, st_ino)` pair — together they identify a file uniquely on its
/// volume, e.g. for detecting hardlinks) and `link_count`, none of which
/// `GetFileAttributesExW` reports.
#[derive(Debug, Clone, Copy)]
pub struct FileInfoByHandle {
    pub attributes: u32,
    pub creation_time: Timespec,
    pub last_access_time: Timespec,
    pub last_write_time: Timespec,
    pub size: u64,
    pub volume_serial_number: u32,
    pub link_count: u32,
    pub file_index: u64,
}

/// `stat` a path — `GetFileAttributesExW`. Fails with
/// [`Win32Error::ERROR_FILE_NOT_FOUND`]/[`Win32Error::ERROR_PATH_NOT_FOUND`]
/// (matching the real call's own documented behavior) if nothing exists at
/// `path`, rather than this wrapper inventing a distinct "not found" result.
pub fn stat(path: &str) -> Result<FileInfo, Win32Error> {
    let wide: Vec<u16> = path.encode_utf16().chain(core::iter::once(0)).collect();
    let mut data = Win32FileAttributeData::default();
    // SAFETY: `wide` is a valid, NUL-terminated UTF-16 string; `data` is a
    // valid, correctly-sized out-pointer matching what
    // `GetFileExInfoStandard` requires.
    let ok = unsafe {
        GetFileAttributesExW(
            wide.as_ptr(),
            GET_FILE_EX_INFO_STANDARD,
            (&mut data as *mut Win32FileAttributeData).cast(),
        )
    };
    if ok == 0 {
        return Err(Win32Error::last());
    }
    Ok(FileInfo {
        attributes: data.file_attributes,
        creation_time: filetime_to_timespec(data.creation_time),
        last_access_time: filetime_to_timespec(data.last_access_time),
        last_write_time: filetime_to_timespec(data.last_write_time),
        size: (u64::from(data.file_size_high) << 32) | u64::from(data.file_size_low),
    })
}

/// `fstat` an already-open handle — `GetFileInformationByHandle`. Reports
/// more than [`stat`] does (volume serial number, link count, file index)
/// since an open handle lets Windows answer a couple of questions a bare
/// path can't.
///
/// # Safety
///
/// `handle` must be a currently-open, valid handle to a file (not a
/// console, pipe, or other non-file handle type — `GetFileInformationByHandle`
/// only supports file handles).
pub unsafe fn stat_by_handle(handle: RawHandle) -> Result<FileInfoByHandle, Win32Error> {
    let mut info = ByHandleFileInformation::default();
    // SAFETY: `handle` is caller-supplied per this function's own safety
    // contract; `info` is a valid, correctly-sized out-pointer.
    let ok = unsafe { GetFileInformationByHandle(handle, &mut info) };
    if ok == 0 {
        return Err(Win32Error::last());
    }
    Ok(FileInfoByHandle {
        attributes: info.file_attributes,
        creation_time: filetime_to_timespec(info.creation_time),
        last_access_time: filetime_to_timespec(info.last_access_time),
        last_write_time: filetime_to_timespec(info.last_write_time),
        size: (u64::from(info.file_size_high) << 32) | u64::from(info.file_size_low),
        volume_serial_number: info.volume_serial_number,
        link_count: info.number_of_links,
        file_index: (u64::from(info.file_index_high) << 32) | u64::from(info.file_index_low),
    })
}

/// Create a symbolic link at `link_path` pointing at `target_path` —
/// `CreateSymbolicLinkW`, `ln -s`'s Windows counterpart. `target_is_directory`
/// must say whether `target_path` names a directory: unlike Unix, Windows
/// needs to know this up front rather than discovering it by following the
/// link, since a directory symlink and a file symlink are different reparse
/// point subtypes.
///
/// Always requests `SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE`, so this
/// succeeds without elevation given Developer Mode enabled or the
/// `SeCreateSymbolicLinkPrivilege` user right — without it, creating a
/// symlink at all requires an elevated (administrator) process.
pub fn create_symlink(
    link_path: &str,
    target_path: &str,
    target_is_directory: bool,
) -> Result<(), Win32Error> {
    let link_wide: Vec<u16> = link_path
        .encode_utf16()
        .chain(core::iter::once(0))
        .collect();
    let target_wide: Vec<u16> = target_path
        .encode_utf16()
        .chain(core::iter::once(0))
        .collect();
    let flags = SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE
        | if target_is_directory {
            SYMBOLIC_LINK_FLAG_DIRECTORY
        } else {
            0
        };
    // SAFETY: `link_wide`/`target_wide` are valid, NUL-terminated UTF-16
    // strings; `flags` is a plain bitmask, not a pointer.
    let ok = unsafe { CreateSymbolicLinkW(link_wide.as_ptr(), target_wide.as_ptr(), flags) };
    if ok == 0 {
        Err(Win32Error::last())
    } else {
        Ok(())
    }
}

/// The fully resolved path an open handle refers to — `GetFinalPathNameByHandleW`,
/// walking through any symlink/junction along the way (unlike [`readlink`],
/// which reports a single link's own stored target without following it).
/// Rust's own `std::fs::canonicalize` uses this exact call on Windows for
/// the same reason. Returns a `\\?\`-prefixed path (`GetFinalPathNameByHandleW`'s
/// default normalization), not a plain drive-letter one.
///
/// # Safety
///
/// `handle` must be a currently-open, valid file handle.
pub unsafe fn final_path(handle: RawHandle) -> Result<alloc::string::String, Win32Error> {
    let mut buf: Vec<u16> = alloc::vec![0u16; 260];
    // At most two attempts: an initial try, then one retry sized exactly to
    // whatever `GetFinalPathNameByHandleW` reports as actually required —
    // matching `path::search_path`'s own growing-buffer pattern.
    for _ in 0..2 {
        // SAFETY: `handle` is caller-supplied per this function's own
        // safety contract; `buf` is a valid, `buf.len()`-element writable
        // buffer.
        let needed =
            unsafe { GetFinalPathNameByHandleW(handle, buf.as_mut_ptr(), buf.len() as u32, 0) };
        if needed == 0 {
            return Err(Win32Error::last());
        }
        if (needed as usize) > buf.len() {
            buf.resize(needed as usize, 0);
            continue;
        }
        return Ok(alloc::string::String::from_utf16_lossy(
            &buf[..needed as usize],
        ));
    }
    // Unreachable in practice, matching `path::search_path`'s own reasoning
    // for this exact fallback.
    Err(Win32Error::ERROR_INSUFFICIENT_BUFFER)
}

/// Read a symlink's own stored target, without following it — the Windows
/// analog of Unix `readlink`. Only understands `IO_REPARSE_TAG_SYMLINK`
/// (an ordinary symlink); junctions/mount points and vendor-specific
/// reparse points report [`Win32Error::ERROR_NOT_SUPPORTED`] rather than
/// this function misinterpreting their differently-shaped data — a
/// deliberate scope cut, not an oversight (nothing in this crate's current
/// scope needs junction support).
///
/// Returns the link's "print name" (the human-readable form Windows itself
/// shows for a symlink, e.g. in Explorer or `dir`), not its "substitute
/// name" (an NT-native, sometimes `\??\`-prefixed absolute form used
/// internally for resolution) — the print name is the closer analog of
/// what Unix `readlink` reports, since it's the same string
/// [`create_symlink`]'s own `target_path` argument produces.
pub fn readlink(path: &str) -> Result<alloc::string::String, Win32Error> {
    let wide: Vec<u16> = path.encode_utf16().chain(core::iter::once(0)).collect();
    // SAFETY: `wide` is a valid, NUL-terminated UTF-16 string;
    // `desired_access = 0` (query-only, no read/write) and
    // `FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS` are
    // documented-valid inputs for opening a reparse point (file or
    // directory) without following it or requiring its target to exist.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            core::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            core::ptr::null_mut(),
        )
    };
    if handle.is_null() || handle as isize == -1 {
        return Err(Win32Error::last());
    }

    let mut buf: Vec<u8> = alloc::vec![0u8; MAXIMUM_REPARSE_DATA_BUFFER_SIZE];
    let mut bytes_returned: u32 = 0;
    // SAFETY: `handle` was just successfully opened above; `buf` is a
    // valid, `buf.len()`-byte writable buffer, sized to Windows' own
    // documented maximum for this request so it's never too small; the
    // request takes no input buffer (`in_buffer = NULL`, `in_buffer_size =
    // 0`), a documented-valid combination for `FSCTL_GET_REPARSE_POINT`.
    let ok = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_GET_REPARSE_POINT,
            core::ptr::null(),
            0,
            buf.as_mut_ptr().cast(),
            buf.len() as u32,
            &mut bytes_returned,
            core::ptr::null_mut(),
        )
    };
    if ok == 0 {
        let err = Win32Error::last();
        // SAFETY: `handle` is valid and not used again after this.
        let _ = unsafe { crate::handle::close(handle) };
        return Err(err);
    }
    // SAFETY: `handle` is valid and not used again after this point.
    let _ = unsafe { crate::handle::close(handle) };

    // SAFETY: a successful `DeviceIoControl` call guarantees at least this
    // fixed 20-byte header is initialized; `read_unaligned` doesn't require
    // `buf`'s allocation to happen to satisfy the header's own alignment.
    let header: ReparseDataBufferSymlinkHeader =
        unsafe { core::ptr::read_unaligned(buf.as_ptr().cast()) };
    if header.reparse_tag != IO_REPARSE_TAG_SYMLINK {
        return Err(Win32Error::ERROR_NOT_SUPPORTED);
    }

    // `PathBuffer` starts immediately after this fixed header;
    // `*_offset`/`*_length` are documented as byte offsets/lengths relative
    // to the start of `PathBuffer`, not the whole struct.
    const PATH_BUFFER_START: usize = 20;
    let print_name_start = PATH_BUFFER_START + header.print_name_offset as usize;
    let print_name_end = print_name_start + header.print_name_length as usize;
    let print_name_bytes = &buf[print_name_start..print_name_end];
    // Reconstructed manually from raw bytes (rather than casting to a
    // `[u16]` slice) since `buf`'s allocation isn't guaranteed 2-byte
    // aligned — the same reason `job::process_ids` reads its own
    // variable-length buffer field-by-field instead of casting.
    let print_name_units: Vec<u16> = print_name_bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    Ok(char::decode_utf16(print_name_units)
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_reports_a_well_known_directory_as_a_directory() {
        let info = stat("C:\\Windows").expect("GetFileAttributesExW should succeed");
        assert!(
            info.attributes & FILE_ATTRIBUTE_DIRECTORY != 0,
            "C:\\Windows should be reported as a directory"
        );
    }

    #[test]
    fn stat_reports_none_for_a_nonexistent_path() {
        let err = stat("C:\\this-path-should-not-exist-rusty-win32-test\\nope")
            .expect_err("a nonexistent path should fail, not succeed");
        assert!(
            err == Win32Error::ERROR_FILE_NOT_FOUND || err == Win32Error::ERROR_PATH_NOT_FOUND,
            "expected a not-found error, got {err:?}"
        );
    }

    #[test]
    fn stat_reports_a_plausible_size_and_non_directory_attributes_for_a_real_file() {
        let path = std::env::temp_dir().join("rusty_win32_fs_stat_test.txt");
        std::fs::write(&path, b"rusty_win32").expect("writing the test file should succeed");

        let info = stat(path.to_str().unwrap()).expect("GetFileAttributesExW should succeed");
        assert_eq!(info.size, "rusty_win32".len() as u64);
        assert_eq!(
            info.attributes & FILE_ATTRIBUTE_DIRECTORY,
            0,
            "a plain file must not carry the directory attribute"
        );

        std::fs::remove_file(&path).expect("cleaning up the test file should succeed");
    }

    #[test]
    fn stat_by_handle_reports_a_plausible_size_and_link_count_for_a_real_file() {
        use std::os::windows::io::AsRawHandle;

        let path = std::env::temp_dir().join("rusty_win32_fs_stat_by_handle_test.txt");
        std::fs::write(&path, b"rusty_win32_fs").expect("writing the test file should succeed");

        let file = std::fs::File::open(&path).expect("opening the test file should succeed");
        // SAFETY: `file`'s raw handle is a currently-open, valid handle to
        // a real file, owned by `file` for the duration of this call.
        let info = unsafe { stat_by_handle(file.as_raw_handle() as RawHandle) }
            .expect("GetFileInformationByHandle should succeed");
        assert_eq!(info.size, "rusty_win32_fs".len() as u64);
        assert_eq!(
            info.attributes & FILE_ATTRIBUTE_DIRECTORY,
            0,
            "a plain file must not carry the directory attribute"
        );
        assert!(
            info.link_count >= 1,
            "an ordinary file should report at least one link"
        );

        drop(file);
        std::fs::remove_file(&path).expect("cleaning up the test file should succeed");
    }

    #[test]
    fn create_symlink_then_readlink_round_trips() {
        let target = std::env::temp_dir().join("rusty_win32_fs_symlink_target.txt");
        let link = std::env::temp_dir().join("rusty_win32_fs_symlink_link.txt");
        // Clean up any leftovers from a previous failed run before starting,
        // so this test is re-runnable rather than failing on "already
        // exists" the second time.
        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_file(&target);

        std::fs::write(&target, b"rusty_win32 symlink target")
            .expect("writing the target file should succeed");
        let target_str = target.to_str().unwrap();
        let link_str = link.to_str().unwrap();

        create_symlink(link_str, target_str, false).expect(
            "CreateSymbolicLinkW should succeed (this CI runner is expected to have either \
             Developer Mode or admin rights granting SeCreateSymbolicLinkPrivilege)",
        );

        let reported_target =
            readlink(link_str).expect("DeviceIoControl(FSCTL_GET_REPARSE_POINT) should succeed");
        assert_eq!(
            reported_target, target_str,
            "readlink should report exactly the target create_symlink was given"
        );

        // The symlink itself must not be reported as a directory (it's a
        // file symlink), and following it via a plain stat should reach
        // the real target's actual content size.
        let link_info = stat(link_str).expect("GetFileAttributesExW on the symlink should succeed");
        assert_eq!(link_info.size, "rusty_win32 symlink target".len() as u64);

        std::fs::remove_file(&link).expect("cleaning up the symlink should succeed");
        std::fs::remove_file(&target).expect("cleaning up the target file should succeed");
    }

    #[test]
    fn readlink_reports_not_supported_for_a_plain_file() {
        let path = std::env::temp_dir().join("rusty_win32_fs_not_a_symlink.txt");
        std::fs::write(&path, b"plain file, not a reparse point")
            .expect("writing the test file should succeed");

        let err = readlink(path.to_str().unwrap())
            .expect_err("readlink on a plain file should fail, not succeed");
        assert_eq!(err, Win32Error::ERROR_NOT_SUPPORTED);

        std::fs::remove_file(&path).expect("cleaning up the test file should succeed");
    }

    #[test]
    fn final_path_resolves_to_the_files_own_name() {
        use std::os::windows::io::AsRawHandle;

        let path = std::env::temp_dir().join("rusty_win32_fs_final_path_test.txt");
        std::fs::write(&path, b"rusty_win32").expect("writing the test file should succeed");

        let file = std::fs::File::open(&path).expect("opening the test file should succeed");
        // SAFETY: `file`'s raw handle is a currently-open, valid file
        // handle, owned by `file` for the duration of this call.
        let resolved = unsafe { final_path(file.as_raw_handle() as RawHandle) }
            .expect("GetFinalPathNameByHandleW should succeed");
        assert!(
            resolved.ends_with("rusty_win32_fs_final_path_test.txt"),
            "resolved path {resolved:?} should end with the file's own name"
        );

        drop(file);
        std::fs::remove_file(&path).expect("cleaning up the test file should succeed");
    }
}
