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
### Changed
- `process::spawn_suspended` takes a new `new_process_group: bool` parameter
  (breaking, pre-1.0).
### Fixed
### Security

<!-- ## [0.1.0] - YYYY-MM-DD
### Added
- Initial release -->
