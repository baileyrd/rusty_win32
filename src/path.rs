//! `SearchPathW` — Windows' `PATH`-search primitive, and (looped over each
//! `PATHEXT` entry) the closest Windows analog of "is this file executable"
//! that `rush`'s command lookup has. Unix `rusty_libc` can answer that
//! question with a `stat` mode-bit check; Windows has no executable bit at
//! all — a file is runnable purely by extension and registration, and
//! `PATHEXT` (`.COM;.EXE;.BAT;.CMD;...`) is the documented list of
//! extensions a bare command name (`foo`, not `foo.exe`) is tried against,
//! in order, until one resolves. Without this primitive, a bare-name
//! invocation either has to be special-cased away or silently fails to
//! resolve on Windows the way it does on Unix.

use crate::error::Win32Error;

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

/// A generous starting buffer size, in UTF-16 units — `MAX_PATH`. Grown to
/// whatever `SearchPathW` reports as actually required for a longer
/// (`\\?\`-prefixed or just long) path.
const MAX_PATH: usize = 260;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SearchPathW(
        path: *const u16,
        file_name: *const u16,
        extension: *const u16,
        buffer_length: u32,
        buffer: *mut u16,
        file_part: *mut *mut u16,
    ) -> u32;
    fn GetShortPathNameW(long_path: *const u16, short_path: *mut u16, buffer_length: u32) -> u32;
    fn GetLongPathNameW(short_path: *const u16, long_path: *mut u16, buffer_length: u32) -> u32;
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(core::iter::once(0)).collect()
}

/// Search for `file_name`, appending `extension` if `file_name` has none of
/// its own and `extension` is given — `SearchPathW`, the primitive Windows'
/// own `PATH`-search relies on. `search_dirs` is a `;`-separated directory
/// list to search instead of the standard order (the calling process's own
/// directory, the Windows system directories, then the `PATH` environment
/// variable); pass `None` for that standard order.
///
/// Returns `Ok(None)` if nothing matched (`ERROR_FILE_NOT_FOUND`) rather than
/// treating "not found" as an error — the same convention
/// [`crate::process::wait`] uses for an ordinary timeout — and the full
/// resolved path (not just a bool) on a match.
pub fn search_path(
    search_dirs: Option<&str>,
    file_name: &str,
    extension: Option<&str>,
) -> Result<Option<String>, Win32Error> {
    let dirs_wide = search_dirs.map(to_wide);
    let name_wide = to_wide(file_name);
    let ext_wide = extension.map(to_wide);

    let dirs_ptr = dirs_wide.as_ref().map_or(core::ptr::null(), |v| v.as_ptr());
    let ext_ptr = ext_wide.as_ref().map_or(core::ptr::null(), |v| v.as_ptr());

    let mut buf: Vec<u16> = alloc::vec![0u16; MAX_PATH];
    // At most two attempts: an initial `MAX_PATH`-sized try, then one retry
    // sized exactly to whatever `SearchPathW` reports as actually required
    // (its own documented behavior on "buffer too small" is to report the
    // exact needed size, not merely "too small") — not an unbounded loop.
    for _ in 0..2 {
        // SAFETY: `dirs_ptr`/`ext_ptr` are each either null or a valid,
        // NUL-terminated UTF-16 string owned by this call's own locals;
        // `name_wide` is a valid, NUL-terminated UTF-16 string; `buf` is a
        // valid, `buf.len()`-element writable buffer; the final `file_part`
        // out-parameter is a documented-valid NULL (this wrapper only wants
        // the full path, not a pointer to the filename portion within it).
        let needed = unsafe {
            SearchPathW(
                dirs_ptr,
                name_wide.as_ptr(),
                ext_ptr,
                buf.len() as u32,
                buf.as_mut_ptr(),
                core::ptr::null_mut(),
            )
        };
        if needed == 0 {
            let err = Win32Error::last();
            return if err == Win32Error::ERROR_FILE_NOT_FOUND {
                Ok(None)
            } else {
                Err(err)
            };
        }
        if (needed as usize) > buf.len() {
            buf.resize(needed as usize, 0);
            continue;
        }
        return Ok(Some(String::from_utf16_lossy(&buf[..needed as usize])));
    }
    // Unreachable in practice: a second `SearchPathW` call sized exactly to
    // the first call's own reported requirement fails only if the target
    // changed size *again* between the two calls, a real but vanishingly
    // unlikely race this wrapper doesn't retry indefinitely for.
    Err(Win32Error::ERROR_INSUFFICIENT_BUFFER)
}

/// Resolve a command name the way Windows itself resolves one: if `command`
/// already names a file extension, search for it as-is; otherwise, try each
/// `;`-separated extension in `pathext` (the standard `PATHEXT` environment
/// variable's own format, e.g. `".COM;.EXE;.BAT;.CMD"`) in order via
/// [`search_path`], returning the first match.
///
/// `pathext` is caller-supplied rather than read from the real environment —
/// matching [`crate::process::spawn_suspended`]'s own `environment`
/// parameter — since a caller tracking its own variable table separately
/// from the OS environment (as `rush`'s `vars` module does) needs to pass
/// *its* `PATHEXT`, not have this function silently read the real one out
/// from under it.
pub fn resolve_command(command: &str, pathext: &str) -> Result<Option<String>, Win32Error> {
    let has_extension = command
        .rsplit(['\\', '/'])
        .next()
        .is_some_and(|base| base.contains('.'));
    if has_extension {
        return search_path(None, command, None);
    }
    for ext in pathext.split(';').filter(|e| !e.is_empty()) {
        if let Some(found) = search_path(None, command, Some(ext))? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// Convert `path` to its legacy 8.3 short-name form (e.g.
/// `C:\Program Files` → `C:\PROGRA~1`) — `GetShortPathNameW`. `path` must
/// name an existing file or directory: Windows generates the short name
/// from the real on-disk entry, not by string manipulation alone (and a
/// volume that was formatted, or has since been configured, without
/// 8.3-name generation reports the long name unchanged rather than
/// failing).
pub fn short_path(path: &str) -> Result<String, Win32Error> {
    let wide = to_wide(path);
    let mut buf: Vec<u16> = alloc::vec![0u16; MAX_PATH];
    // Same two-attempt growth pattern as `search_path`: an initial
    // `MAX_PATH`-sized try, then one retry sized exactly to whatever
    // `GetShortPathNameW` reports as actually required.
    for _ in 0..2 {
        // SAFETY: `wide` is a valid, NUL-terminated UTF-16 string; `buf` is
        // a valid, `buf.len()`-element writable buffer.
        let needed =
            unsafe { GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
        if needed == 0 {
            return Err(Win32Error::last());
        }
        if (needed as usize) > buf.len() {
            buf.resize(needed as usize, 0);
            continue;
        }
        // `GetShortPathNameW`'s returned length excludes the terminating
        // NUL on success, the same convention `search_path` already relies
        // on for `SearchPathW`.
        return Ok(String::from_utf16_lossy(&buf[..needed as usize]));
    }
    Err(Win32Error::ERROR_INSUFFICIENT_BUFFER)
}

/// Convert `path` (which may already be a long name, an 8.3 short name, or
/// a mix of either per path component) to its fully long-name form —
/// `GetLongPathNameW`, the reverse of [`short_path`]. Like `short_path`,
/// `path` must name an existing file or directory.
pub fn long_path(path: &str) -> Result<String, Win32Error> {
    let wide = to_wide(path);
    let mut buf: Vec<u16> = alloc::vec![0u16; MAX_PATH];
    // Same two-attempt growth pattern as `search_path`/`short_path`.
    for _ in 0..2 {
        // SAFETY: `wide` is a valid, NUL-terminated UTF-16 string; `buf` is
        // a valid, `buf.len()`-element writable buffer.
        let needed = unsafe { GetLongPathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
        if needed == 0 {
            return Err(Win32Error::last());
        }
        if (needed as usize) > buf.len() {
            buf.resize(needed as usize, 0);
            continue;
        }
        // Same excludes-the-NUL convention as `short_path`/`search_path`.
        return Ok(String::from_utf16_lossy(&buf[..needed as usize]));
    }
    Err(Win32Error::ERROR_INSUFFICIENT_BUFFER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_path_finds_a_well_known_system_binary_by_full_name() {
        let found = search_path(None, "cmd.exe", None)
            .expect("SearchPathW should succeed")
            .expect("cmd.exe should be found on PATH/system directories");
        assert!(found.to_ascii_lowercase().ends_with("cmd.exe"));
    }

    #[test]
    fn search_path_reports_none_for_a_nonexistent_file() {
        let found = search_path(
            None,
            "this-file-should-not-exist-rusty-win32-test.exe",
            None,
        )
        .expect("SearchPathW should succeed even when nothing matches");
        assert_eq!(found, None);
    }

    #[test]
    fn resolve_command_finds_a_bare_name_via_pathext() {
        let found = resolve_command("cmd", ".COM;.EXE;.BAT;.CMD")
            .expect("resolution should succeed")
            .expect("cmd should resolve to cmd.exe via PATHEXT");
        assert!(found.to_ascii_lowercase().ends_with("cmd.exe"));
    }

    #[test]
    fn resolve_command_skips_pathext_when_an_extension_is_already_given() {
        let found = resolve_command("cmd.exe", ".BAT;.CMD") // deliberately missing .EXE
            .expect("resolution should succeed")
            .expect("an explicit extension should search as-is, ignoring pathext");
        assert!(found.to_ascii_lowercase().ends_with("cmd.exe"));
    }

    #[test]
    fn resolve_command_reports_none_when_no_pathext_entry_matches() {
        let found = resolve_command(
            "this-command-should-not-exist-rusty-win32-test",
            ".COM;.EXE;.BAT;.CMD",
        )
        .expect("resolution should succeed even when nothing matches");
        assert_eq!(found, None);
    }

    #[test]
    fn short_path_of_program_files_has_no_spaces() {
        // An 8.3 short name never contains a space by construction — a
        // robust, version-independent assertion rather than pinning the
        // exact generated name (`PROGRA~1` vs `PROGRA~2` depends on what
        // else is installed on the CI runner).
        let short = short_path(r"C:\Program Files").expect("GetShortPathNameW should succeed");
        assert!(
            !short.contains(' '),
            "an 8.3 short name should never contain a space, got: {short}"
        );
    }

    #[test]
    fn long_path_of_the_short_form_round_trips_back_to_program_files() {
        let short = short_path(r"C:\Program Files").expect("GetShortPathNameW should succeed");
        let long = long_path(&short).expect("GetLongPathNameW should succeed");
        assert!(
            long.to_ascii_lowercase().ends_with("program files"),
            "got: {long}"
        );
    }

    #[test]
    fn short_path_fails_for_a_nonexistent_path() {
        let err = short_path(r"C:\this-path-should-not-exist-rusty-win32-test").unwrap_err();
        assert_eq!(err, Win32Error::ERROR_FILE_NOT_FOUND);
    }
}
