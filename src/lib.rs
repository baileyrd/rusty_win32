//! `rusty_win32` — a `#![no_std]`-where-possible, minimal-dependency Windows
//! backend for [rush](https://github.com/baileyrd/rush)'s `sys::win32`, the
//! Windows counterpart to
//! [rusty_libc](https://github.com/baileyrd/rusty_libc) (Linux). Not a
//! shared codebase with `rusty_libc` — Windows has no equivalent of "a
//! stable syscall number," so this crate's primitive is "a documented DLL
//! export" reached via `extern "system"` FFI, not raw `asm!` syscalls. See
//! `docs/WINDOWS_BACKEND_ANALYSIS.md` in the rush repo for the
//! primitive-by-primitive analysis this crate's module boundaries and
//! phasing are derived from.
//!
//! Phase 1: [`error::Win32Error`] and [`console::install_ctrl_handler`] —
//! closing rush's single highest-value, lowest-risk Windows gap identified
//! by that analysis: `trap 'cmd' TERM` is silently accepted on Windows today
//! but has nothing installed to ever fire it.
//!
//! Phase 2 (this crate's current state): [`handle`] —
//! `DuplicateHandle`/`CreatePipe`/`SetHandleInformation`/`CloseHandle`, the
//! primitive rush's own fd-3-and-up gap needs (rush still has to grow its
//! own integer-to-`HANDLE` map on top; this crate provides the raw
//! primitives, not that map). Everything else the analysis doc scopes —
//! `process`/`job` for background jobs (Job Objects), ConPTY — is future
//! work, not yet started.
//!
//! Safe wrappers return `Result<T, Win32Error>`; a raw Win32 error code
//! never escapes unwrapped. `unsafe` is confined to the `extern "system"`
//! FFI declarations and the one-line calls through them — the public API is
//! safe.

#![cfg_attr(not(any(test, feature = "std")), no_std)]

pub mod error;
pub use error::Win32Error;

#[cfg(windows)]
pub mod console;
#[cfg(windows)]
pub use console::{HandlerRoutine, install_ctrl_handler, remove_ctrl_handler};

#[cfg(windows)]
pub mod handle;
#[cfg(windows)]
pub use handle::{RawHandle, close, create_pipe, duplicate, set_inheritable};
