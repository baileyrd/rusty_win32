# Changelog

All notable changes to this repo are documented here.
Format: Added / Changed / Deprecated / Removed / Fixed / Security, newest first.

## [Unreleased]
### Added
- `process::spawn_suspended`'s `new_process_group` parameter and
  `console::generate_ctrl_event`, for interrupting one background child via
  a targeted `CTRL_BREAK_EVENT` instead of affecting the whole console.
- `process::times` (`GetProcessTimes`), CPU-time accounting for a process
  handle — creation/exit wall-clock timestamps plus kernel/user elapsed
  duration.
- `job::set_resource_limits`/`job::limits` (memory/CPU-time/active-process
  Job Object limits) and `job::accounting`
  (`JobObjectBasicAndIoAccountingInformation`), the narrow subset of `ulimit`
  a Job Object can enforce, plus lifetime CPU/IO accounting.
- `pipe` module: named pipes (`CreateNamedPipeW`/`ConnectNamedPipeW`/
  `WaitNamedPipeW`/`CreateFileW`), the primitive rush's deferred process
  substitution (`<(cmd)`) and `coproc` support need on Windows.
- `console::write_key_events` (`WriteConsoleInputW` for non-character keys —
  arrows, Home/End, function keys, …) plus the `VK_*`/`ENHANCED_KEY`
  constants it uses. First non-`kernel32` link in this crate
  (`user32.dll`'s `MapVirtualKeyW`).
- `volume` module: drive/volume enumeration (`GetLogicalDrives`/
  `GetDriveTypeW`/`GetVolumeInformationW`) — a distinctly Windows-shaped
  primitive (multi-root filesystem, no Unix analog) with no current
  consumer, added for completeness.
- `path::short_path`/`path::long_path` (`GetShortPathNameW`/
  `GetLongPathNameW`), normalizing between a legacy 8.3 short name and its
  long form.
- `watch` module: filesystem change notification (`ReadDirectoryChangesW`),
  wrapped in `OVERLAPPED` I/O with a `process::wait`-style `Option<u32>`
  timeout — this crate's first genuinely overlapped primitive, since
  `ReadDirectoryChangesW` has no other way to bound how long it blocks.
- `path::current_dir`/`path::set_current_dir` (`GetCurrentDirectoryW`/
  `SetCurrentDirectoryW`) — the actual Win32 primitives behind `cd`/`pwd`,
  found by a parity-loop pass against the real Win32 API surface
  (`gap-analysis.md`) rather than the round-2 needs-driven assessment above.
- `fs::read_dir` (`FindFirstFileW`/`FindNextFileW`/`FindClose`), a `ReadDir`
  iterator of `DirEntry` for listing a directory's contents — another
  parity-loop find.
- `process::get_env_var`/`process::set_env_var`
  (`GetEnvironmentVariableW`/`SetEnvironmentVariableW`), live single-variable
  environment access to back `export`/`unset`/single-`$VAR` reads — another
  parity-loop find.
- `handle::get_std_handle`/`handle::set_std_handle`
  (`GetStdHandle`/`SetStdHandle`) plus the `STD_*_HANDLE` slot constants —
  the primitive `process.rs`'s own `spawn_suspended` doc comment already
  described but this crate didn't yet own; another parity-loop find.
- `path::full_path` (`GetFullPathNameW`), resolving a relative path to its
  absolute form — another parity-loop find.
- `process::process_id_of` (`GetProcessId`), the reverse of `open_by_pid`'s
  pid-to-`HANDLE` mapping — another parity-loop find.
- `process::image_path` (`QueryFullProcessImageNameW`), the full executable
  path for a process handle, completing `list_processes`'s bare-filename-only
  `exe_file` — another parity-loop find.
- `job::is_in_job` (`IsProcessInJob`), checking job membership before
  `assign` to avoid a surprise failure under an ambient job (e.g. GitHub
  Actions' Windows runners) — another parity-loop find.
- `console::title`/`console::set_title`
  (`GetConsoleTitleW`/`SetConsoleTitleW`), the Windows analog of xterm's OSC
  title-setting escape sequence — another parity-loop find.
- `console::set_cursor_position` (`SetConsoleCursorPosition`), the write
  side of `window_size`'s cursor-position read — another parity-loop find.
- `console::fill_char`/`console::fill_attribute`
  (`FillConsoleOutputCharacterW`/`FillConsoleOutputAttribute`), a
  clear-to-end-of-line-style redraw primitive — another parity-loop find.
- `console::flush_input` (`FlushConsoleInputBuffer`), discarding stale
  queued keystrokes (e.g. after Ctrl-C) — another parity-loop find.
- `console::pending_input_events` (`GetNumberOfConsoleInputEvents`), a
  non-blocking queued-input-depth check — another parity-loop find.
- `pipe::disconnect_server` (`DisconnectNamedPipe`), letting a served pipe
  instance be reset and reused for a second client instead of requiring a
  fresh server — another parity-loop find.
### Changed
- `process::spawn_suspended` takes a new `new_process_group: bool` parameter
  (breaking, pre-1.0).
- `job::process_ids` now returns `Vec<u32>` instead of `Vec<usize>`,
  matching every other pid-carrying value in this crate (breaking,
  pre-1.0).
### Fixed
### Security

<!-- ## [0.1.0] - YYYY-MM-DD
### Added
- Initial release -->
