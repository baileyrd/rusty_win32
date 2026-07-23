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
- `handle::handle_information` (`GetHandleInformation`), the read-side
  counterpart to `set_inheritable` — another parity-loop find.
- `volume::disk_free_space` (`GetDiskFreeSpaceExW`), free/total space for a
  `df`-style builtin — another parity-loop find.
- `process::sleep_ms` (`Sleep`), the direct primitive behind a
  `sleep`/`usleep` builtin — another parity-loop find.
- `process::logical_processor_count` (`GetSystemInfo`'s
  `dwNumberOfProcessors`), the primitive behind an `nproc`-equivalent
  builtin — another parity-loop find.
- `process::computer_name` (`GetComputerNameW`), the primitive behind
  `$HOSTNAME`/a `hostname` builtin — another parity-loop find.
- `process::memory_status` (`GlobalMemoryStatusEx`), system-wide memory
  totals/load for a `free`-style builtin — another parity-loop find.
- `process::set_error_mode` (`SetErrorMode`) plus
  `SEM_FAILCRITICALERRORS`/`SEM_NOOPENFILEERRORBOX`, suppressing blocking
  GUI error dialogs that would otherwise freeze a non-interactive script
  run — another parity-loop find.
- `fs::copy_file` (`CopyFileW`), the primitive behind a `cp` builtin —
  another parity-loop find.
- `fs::move_file` (`MoveFileExW`) plus `MOVEFILE_REPLACE_EXISTING`/
  `MOVEFILE_COPY_ALLOWED`, the primitive behind an `mv` builtin (covering
  cross-volume moves, unlike `std::fs::rename` on Windows) — another
  parity-loop find.
- `fs::delete_file` (`DeleteFileW`), the primitive behind an `rm` builtin —
  another parity-loop find.
- `fs::create_directory`/`fs::remove_directory`
  (`CreateDirectoryW`/`RemoveDirectoryW`), the primitives behind
  `mkdir`/`rmdir` builtins — another parity-loop find.
- `fs::create_hard_link` (`CreateHardLinkW`), `ln` without `-s` — another
  parity-loop find.
- `path::temp_path`/`path::temp_file_name`
  (`GetTempPathW`/`GetTempFileNameW`), for heredoc scratch files or a
  `mktemp` builtin — another parity-loop find.
- `process::priority_class`/`process::set_priority_class`
  (`GetPriorityClass`/`SetPriorityClass`) plus the `*_PRIORITY_CLASS`
  constants, the Windows analog of `nice`/`renice` — another parity-loop
  find.
- `process::list_threads`/`process::open_thread`/`process::suspend_thread`
  (`Thread32First`/`Thread32Next`/`OpenThread`/`SuspendThread`) plus
  `THREAD_SUSPEND_RESUME`, the closest Windows equivalent to a
  process-wide `SIGSTOP` — another parity-loop find.
- `pipe::set_pipe_mode` (`SetNamedPipeHandleState`), non-blocking mode and
  byte/message-mode switching for named pipes — another parity-loop find.
- `handle::create_mutex`/`handle::release_mutex`
  (`CreateMutexW`/`ReleaseMutex`), the Windows analog of `flock`'s
  cross-process locking — another parity-loop find.
- `fs::lock_file`/`fs::unlock_file` (`LockFileEx`/`UnlockFileEx`), advisory
  file-level locking — the last of the 32 parity-loop finds from
  `gap-analysis.md`.
- `process::affinity`/`process::set_affinity` (`GetProcessAffinityMask`/
  `SetProcessAffinityMask`), the Windows analog of `sched_getaffinity`/
  `taskset` — first of the round-2 "weak/no clear consumer" items added per
  explicit direction (`gap-analysis.md`), not because any consumer currently
  wants it.
- `process::thread_exit_code` (`GetExitCodeThread`) plus
  `THREAD_QUERY_INFORMATION`, the thread-level counterpart to `wait`'s
  process exit code — another round-2 item.
- `process::thread_times` (`GetThreadTimes`), the thread-level counterpart
  to `process::times` — another round-2 item.
- `handle::create_semaphore`/`handle::release_semaphore`
  (`CreateSemaphoreW`/`ReleaseSemaphore`), a counting semaphore alongside
  the already-wrapped mutex — another round-2 item.
- `handle::wait_single_ex`/`handle::wait_multiple_ex`
  (`WaitForSingleObjectEx`/`WaitForMultipleObjectsEx`) plus `WaitResult`,
  alertable-wait variants of the plain waits already used throughout this
  crate — another round-2 item.
- `process::sleep_ms_ex` (`SleepEx`), the alertable-sleep variant of
  `process::sleep_ms` — another round-2 item.
- `handle::signal_and_wait` (`SignalObjectAndWait`), atomically signaling
  one synchronization object and waiting on another — another round-2
  item.
- `volume::find_volumes` (`FindFirstVolumeW`/`FindNextVolumeW`/
  `FindVolumeClose`), enumerating every volume by its stable GUID path,
  independent of drive-letter assignment — another round-2 item.
- `volume::volume_path_name` (`GetVolumePathNameW`), mapping an arbitrary
  path to the root path of the volume it's on — another round-2 item.
- `fs::compressed_file_size` (`GetCompressedFileSizeW`), the on-disk
  (NTFS-compressed) size of a file vs. `fs::stat`'s logical size —
  another round-2 item.
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
