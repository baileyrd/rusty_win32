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
}
