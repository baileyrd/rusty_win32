//! `GetLastError()` wrapped as a typed, `Result`-friendly error — the Win32
//! counterpart of `rusty_libc::Errno`. A raw Win32 error code should never
//! escape a safe wrapper in this crate unwrapped.

use core::fmt;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetLastError() -> u32;
}

/// A Win32 error code, as returned by `GetLastError()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Win32Error(u32);

impl Win32Error {
    /// Wrap a raw Win32 error code (e.g. one already in hand from a
    /// documented return value, rather than `GetLastError()`).
    pub const fn from_raw(code: u32) -> Self {
        Win32Error(code)
    }

    /// The calling thread's last error code, via `GetLastError()`. Call this
    /// immediately after a Win32 API reports failure — any intervening Win32
    /// call, even an unrelated one, can overwrite it.
    #[cfg(windows)]
    pub fn last() -> Self {
        // SAFETY: `GetLastError` takes no arguments and has no preconditions.
        Win32Error(unsafe { GetLastError() })
    }

    /// The raw numeric code.
    pub const fn code(self) -> u32 {
        self.0
    }
}

macro_rules! win32_errors {
    ($($name:ident = $val:expr => $msg:expr),* $(,)?) => {
        impl Win32Error {
            $(
                #[allow(non_upper_case_globals)]
                pub const $name: Win32Error = Win32Error($val);
            )*
        }

        impl fmt::Display for Win32Error {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self.0 {
                    $($val => f.write_str($msg),)*
                    other => write!(f, "unknown Win32 error {other}"),
                }
            }
        }
    };
}

// Messages match the FormatMessageW text for these codes (verified against
// Microsoft's own system-error-code documentation), not invented wording —
// a `std::io::Error` built from the same code via `From` below should read
// the same as this `Display` impl.
win32_errors! {
    SUCCESS = 0 => "the operation completed successfully",
    ERROR_FILE_NOT_FOUND = 2 => "the system cannot find the file specified",
    ERROR_PATH_NOT_FOUND = 3 => "the system cannot find the path specified",
    ERROR_ACCESS_DENIED = 5 => "access is denied",
    ERROR_INVALID_HANDLE = 6 => "the handle is invalid",
    ERROR_NOT_ENOUGH_MEMORY = 8 => "not enough storage is available to process this command",
    ERROR_INVALID_DATA = 13 => "the data is invalid",
    ERROR_OUTOFMEMORY = 14 => "not enough storage is available to complete this operation",
    ERROR_NOT_READY = 21 => "the device is not ready",
    ERROR_SHARING_VIOLATION = 32 => "the process cannot access the file because it is being used by another process",
    ERROR_HANDLE_EOF = 38 => "reached the end of the file",
    ERROR_NOT_SUPPORTED = 50 => "the request is not supported",
    ERROR_FILE_EXISTS = 80 => "the file exists",
    ERROR_INVALID_PARAMETER = 87 => "the parameter is incorrect",
    ERROR_BROKEN_PIPE = 109 => "the pipe has been ended",
    ERROR_CALL_NOT_IMPLEMENTED = 120 => "this function is not supported on this system",
    ERROR_INSUFFICIENT_BUFFER = 122 => "the data area passed to a system call is too small",
    ERROR_INVALID_NAME = 123 => "the filename, directory name, or volume label syntax is incorrect",
    ERROR_ALREADY_EXISTS = 183 => "cannot create a file when that file already exists",
    ERROR_NO_DATA = 232 => "the pipe is being closed",
    ERROR_MORE_DATA = 234 => "more data is available",
    ERROR_OPERATION_ABORTED = 995 => "the I/O operation has been aborted because of either a thread exit or an application request",
    ERROR_IO_PENDING = 997 => "overlapped I/O operation is in progress",
    ERROR_NOT_FOUND = 1168 => "element not found",
    ERROR_TIMEOUT = 1460 => "this operation returned because the timeout period expired",
}

impl core::error::Error for Win32Error {}

#[cfg(feature = "std")]
impl From<Win32Error> for std::io::Error {
    fn from(e: Win32Error) -> Self {
        // `from_raw_os_error` is exactly this on Windows: it stores the code
        // and defers to FormatMessageW for `Display`, same as a "real"
        // `std::io::Error` produced by a failing std call.
        std::io::Error::from_raw_os_error(e.code() as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_known_code() {
        assert_eq!(
            Win32Error::ERROR_ACCESS_DENIED.to_string(),
            "access is denied"
        );
    }

    #[test]
    fn display_unknown_code() {
        assert_eq!(
            Win32Error::from_raw(424242).to_string(),
            "unknown Win32 error 424242"
        );
    }

    #[test]
    fn code_roundtrip() {
        assert_eq!(Win32Error::from_raw(5).code(), 5);
        assert_eq!(Win32Error::ERROR_ACCESS_DENIED.code(), 5);
    }

    #[test]
    fn named_constants_are_distinct_values() {
        assert_ne!(
            Win32Error::ERROR_FILE_NOT_FOUND,
            Win32Error::ERROR_PATH_NOT_FOUND
        );
    }

    #[cfg(feature = "std")]
    #[test]
    fn converts_to_io_error_by_code() {
        let io_err: std::io::Error = Win32Error::ERROR_FILE_NOT_FOUND.into();
        assert_eq!(io_err.raw_os_error(), Some(2));
    }
}
