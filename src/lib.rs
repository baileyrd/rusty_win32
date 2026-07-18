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
//! Phase 2: [`handle`] —
//! `DuplicateHandle`/`CreatePipe`/`SetHandleInformation`/`CloseHandle`, the
//! primitive rush's own fd-3-and-up gap needs (rush still has to grow its
//! own integer-to-`HANDLE` map on top; this crate provides the raw
//! primitives, not that map).
//!
//! Phase 3: [`process`] and [`job`] — `spawn_suspended`/`resume`/`wait` (a
//! raw `CreateProcessW` path `std::process::Command` can't provide, needed
//! only for the suspend-then-assign-to-job sequencing below) and Windows
//! Job Objects
//! (`CreateJobObjectW`/`AssignProcessToJobObject`/`SetInformationJobObject`/
//! `TerminateJobObject`/`QueryInformationJobObject`), the primitives rush's
//! own `docs/WINDOWS_JOB_CONTROL.md` designs its background-job tracking
//! (`&`, `jobs`, `wait`, `kill`, `$!`) against.
//!
//! Phase 5: [`time`] — `now_monotonic`/`now_realtime` via
//! `QueryPerformanceCounter`/`GetSystemTimePreciseAsFileTime`, a genuine
//! parallel to `rusty_libc::vdso`'s "read kernel-shared memory instead of
//! syscalling" trick. Lowest priority in the analysis doc: no rush call
//! site needs it (rush uses `std::time` exclusively, and std's own Windows
//! backend already uses `QueryPerformanceCounter` internally) — this
//! exists for `rusty_lines`/completeness, not an open rush gap.
//!
//! Phase 4 (this crate's current state — implemented *after* Phase 5,
//! once its actual shape was confirmed rather than guessed): raw-mode
//! primitives added to [`console`] —
//! [`console::get_mode`]/[`console::set_mode`] (`GetConsoleMode`/
//! `SetConsoleMode`), [`console::read`] (`ReadFile`), [`console::wait_readable`]
//! (`WaitForSingleObject`), and [`console::window_size`]
//! (`GetConsoleScreenBufferInfo`). **Deliberately not ConPTY**: the
//! original handoff doc's sketch ("`CreatePseudoConsole`-backed raw mode")
//! turned out to be the wrong primitive after actually reading
//! `rusty_lines`' source rather than assuming — `CreatePseudoConsole` hosts
//! a *child* process's console session (what a terminal emulator does);
//! `rusty_lines` reads from its own inherited stdin, exactly the way the
//! Unix backend calls `tcgetattr`/`tcsetattr` on its own fd. The real
//! analog of `tcgetattr`/`tcsetattr` is `GetConsoleMode`/`SetConsoleMode`,
//! and `ENABLE_VIRTUAL_TERMINAL_INPUT` (Windows 10+, within this crate's
//! existing floor) is what makes `ReadFile` on a console handle deliver a
//! Unix-tty-like VT/ANSI byte stream instead of requiring
//! `ReadConsoleInputW`'s structured records. This also corrected a factual
//! error the original phasing carried: rush's own
//! `docs/WINDOWS_BACKEND_ANALYSIS.md` had claimed Windows Ctrl-C-at-idle-prompt
//! already worked via `rusty_lines`; it doesn't — `rusty_lines`' non-Unix
//! path has no Ctrl-C handling at all today (see that doc's own
//! corrected text). As with `rusty_libc`'s `tcgetattr`/`tcsetattr`, this
//! crate exposes the primitive only — deciding which mode bits constitute
//! "raw mode" is `rusty_lines`' policy, not this crate's.
//!
//! Safe wrappers return `Result<T, Win32Error>`; a raw Win32 error code
//! never escapes unwrapped. `unsafe` is confined to the `extern "system"`
//! FFI declarations and functions that take a caller-supplied raw handle or
//! an unquoted command line (`handle`'s and `job`'s handle-taking
//! functions, `process::spawn_suspended`/`resume`/`wait`) — everything else
//! is safe.

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

#[cfg(windows)]
pub mod process;
#[cfg(windows)]
pub use process::{SpawnedProcess, current_pid, resume, spawn_suspended, wait};

#[cfg(windows)]
pub mod job;

#[cfg(windows)]
pub mod time;
#[cfg(windows)]
pub use time::{Timespec, now_monotonic, now_realtime};
