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
