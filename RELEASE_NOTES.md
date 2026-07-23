# Release Notes

One entry per merged PR against `main`, reverse chronological. No version tags
exist yet (still pre-1.0/unreleased on crates.io), so this tracks by PR rather
than by tag — see `CHANGELOG.md` for the `[Unreleased]` rollup once a tag ships.

---

## PR #212 — console: add screen-buffer/window write side (GetLargestConsoleWindowSize/SetConsoleScreenBufferSize/SetConsoleWindowInfo)
**2026-07-23** · [#212](https://github.com/baileyrd/rusty_win32/pull/212)

- **Added:** `console::largest_window_size`/`console::set_screen_buffer_size`/
  `console::set_window_info` (`GetLargestConsoleWindowSize`/
  `SetConsoleScreenBufferSize`/`SetConsoleWindowInfo`), closing issue #141 —
  the write side of console geometry, complementing the existing read-only
  `window_size`. `Coord`/`SmallRect` (previously private, backing
  `set_cursor_position`/`window_size` internally) are now public types
  too, since these three functions' signatures need them. Another round-2
  "weak/no clear consumer" item (`gap-analysis.md`); no current `rush`
  feature asks for this.

## PR #211 — console: add alloc/free/attach (AllocConsole/FreeConsole/AttachConsole)
**2026-07-23** · [#211](https://github.com/baileyrd/rusty_win32/pull/211)

- **Added:** `console::alloc`/`console::free`/`console::attach`
  (`AllocConsole`/`FreeConsole`/`AttachConsole`), closing issue #140 — lets a
  GUI-subsystem process acquire a console on demand, release it, or attach
  to another process's (`attach(None)` maps to `ATTACH_PARENT_PROCESS`).
  This crate's own test helper (`ensure_console_stdin`) already used
  `AllocConsole` internally; it now calls through the public `alloc()`
  instead of a private duplicate extern. Another round-2 "weak/no clear
  consumer" item (`gap-analysis.md`); no current `rush` feature asks for
  this.
- **Fixed:** a CI-caught bug in this PR's own new test for `attach` — it
  called `free()` on the shared test-process console, then attempted to
  `attach()` to a child process's inherited console, which failed
  (`Win32Error(31)`) in this hosting environment. Because the test never
  restored a console on that failure path, every later test run
  afterward (alphabetically) inherited a console-less process, breaking
  an unrelated, pre-existing test
  (`ctrl_c_event_cannot_be_scoped_to_a_process_group`) that implicitly
  depends on one being attached. Replaced with a non-destructive test of
  `attach`'s documented error path instead (`AttachConsole` rejects the
  call outright when the caller already has a console) — this never
  detaches the shared console, so it can't leak a broken state into
  later tests.

## PR #210 — console: add window_handle (GetConsoleWindow)
**2026-07-23** · [#210](https://github.com/baileyrd/rusty_win32/pull/210)

- **Added:** `console::window_handle` (`GetConsoleWindow`), closing issue
  #139 — the `HWND` of the console window attached to the calling
  process, if any (`None` for a headless/service process). Manipulating
  the window itself via ordinary `user32` window APIs is out of this
  crate's scope beyond returning the handle. Another round-2 "weak/no
  clear consumer" item (`gap-analysis.md`); no current `rush` feature
  asks for this.
- **Fixed:** a genuine (not flaky) test assumption caught by CI: the
  original test asserted `GetConsoleWindow` always returns a real `HWND`
  once a console exists, but this crate's own `windows-latest` CI runner
  is itself a real, deterministic counterexample — a console can exist
  and work fine for I/O with no window attached (e.g. a non-interactive
  CI session). Rewrote the test to check repeated-call consistency
  instead of asserting `Some`.

## PR #209 — console: add process_list (GetConsoleProcessList)
**2026-07-23** · [#209](https://github.com/baileyrd/rusty_win32/pull/209)

- **Fixed:** a CI-caught race in
  `process::tests::thread_times_reports_plausible_creation_and_exit_timestamps`
  (added in PR #193) — the same shape as the earlier `thread_exit_code`
  fix (PR #198): the process handle becoming signaled doesn't guarantee
  `GetThreadTimes`' `exit` field is already populated for the thread
  itself. Fixed by opening the thread handle with `SYNCHRONIZE` too and
  explicitly waiting on it before reading the times. Unrelated to this
  PR's own diff; also unrelated to a separate CI infra hang on the same
  PR (an `apt-get install mingw-w64` step stalled for ~19 min — resolved
  by cancelling and rerunning, no code change needed for that one).
- **Added:** `console::process_list` (`GetConsoleProcessList`), closing
  issue #138 — the pids of every process currently attached to the
  calling process's console, e.g. for an "is anything else still
  attached to this console" check. Another round-2 "weak/no clear
  consumer" item (`gap-analysis.md`); no current `rush` feature asks for
  this.

## PR #208 — process: add exception-handler hooks (AddVectoredExceptionHandler/SetUnhandledExceptionFilter)
**2026-07-23** · [#208](https://github.com/baileyrd/rusty_win32/pull/208)

- **Added:** `process::add_vectored_exception_handler`/
  `remove_vectored_exception_handler` (`AddVectoredExceptionHandler`/
  `RemoveVectoredExceptionHandler`) and
  `process::set_unhandled_exception_filter` (`SetUnhandledExceptionFilter`)
  plus `ExceptionPointers`/`VectoredExceptionHandler`/
  `TopLevelExceptionFilter`/`EXCEPTION_CONTINUE_EXECUTION`/
  `EXCEPTION_CONTINUE_SEARCH`/`EXCEPTION_EXECUTE_HANDLER`, closing issue
  #137 — structured-exception-handling hooks, the closest Windows analog
  to installing a Unix `SIGSEGV`/`SIGABRT` handler. `EXCEPTION_RECORD`/
  `CONTEXT` aren't decoded (a variable-length trailing array and a large,
  architecture-specific register dump respectively) — only the two raw
  pointers `EXCEPTION_POINTERS` itself carries are exposed. Tested via
  `RaiseException` with a custom application-defined exception code — a
  safe, deterministic way to exercise a handler without a real CPU fault.
  Another round-2 "weak/no clear consumer" item (`gap-analysis.md`); no
  current `rush` feature asks for this.

## PR #207 — process: add logical_processor_information (GetLogicalProcessorInformation)
**2026-07-23** · [#207](https://github.com/baileyrd/rusty_win32/pull/207)

- **Added:** `process::logical_processor_information`
  (`GetLogicalProcessorInformation`) plus `LogicalProcessorInformation`/
  `ProcessorRelationship`, closing issue #136 — detailed CPU topology
  (cores/NUMA nodes/cache) beyond `process::logical_processor_count`'s
  single number, using the query-size-then-allocate pattern this crate
  already uses elsewhere. Only `processor_mask`/`relationship` are
  exposed (not cache/NUMA-specific fields) — a future consumer needing
  those can extend it. Another round-2 "weak/no clear consumer" item
  (`gap-analysis.md`); no current `rush` feature asks for this.

## PR #206 — path: add system_directory/windows_directory (GetSystemDirectoryW/GetWindowsDirectoryW)
**2026-07-23** · [#206](https://github.com/baileyrd/rusty_win32/pull/206)

- **Added:** `path::system_directory`/`path::windows_directory`
  (`GetSystemDirectoryW`/`GetWindowsDirectoryW`), closing issue #135 —
  standard well-known-location primitives (`C:\Windows\System32`/
  `C:\Windows`), the Windows analog of resolving `/usr/bin`. Another
  round-2 "weak/no clear consumer" item (`gap-analysis.md`); no current
  `rush` feature asks for this.

## PR #205 — process: add tick_count (GetTickCount64)
**2026-07-23** · [#205](https://github.com/baileyrd/rusty_win32/pull/205)

- **Added:** `process::tick_count` (`GetTickCount64`), closing issue #134
  — milliseconds elapsed since the system started, a coarser, simpler
  monotonic counter alongside `time::now_monotonic`'s
  `QueryPerformanceCounter`-backed high-resolution one. Another round-2
  "weak/no clear consumer" item (`gap-analysis.md`); no current `rush`
  feature asks for this.

## PR #204 — job: add open_by_name (OpenJobObjectW)
**2026-07-23** · [#204](https://github.com/baileyrd/rusty_win32/pull/204)

- **Added:** `job::open_by_name` (`OpenJobObjectW`) plus
  `JOB_OBJECT_ALL_ACCESS`, closing issue #133 — the reverse direction of
  `job::create`, which only ever makes anonymous jobs. Another round-2
  "weak/no clear consumer" item (`gap-analysis.md`); no current `rush`
  feature asks for this.

## PR #203 — pipe: add transact/call (TransactNamedPipe/CallNamedPipeW)
**2026-07-23** · [#203](https://github.com/baileyrd/rusty_win32/pull/203)

- **Added:** `pipe::transact`/`pipe::call` (`TransactNamedPipe`/
  `CallNamedPipeW`), closing issue #132 — one-shot message-mode pipe
  transactions (write-then-read in a single call) for a simple
  request-response protocol; `call` additionally combines
  `wait_for_server`/`open_client`/`transact`/close for a caller that only
  needs one round trip. Another round-2 "weak/no clear consumer" item
  (`gap-analysis.md`); no current `rush` feature asks for this.
- **Fixed:** a real CI hang (not a fast failure) in this PR's own new
  tests: `transact`/`call`'s pipe was created with `PIPE_READMODE_BYTE`
  even though `PIPE_TYPE_MESSAGE` was set, and `TransactNamedPipe`/
  `CallNamedPipeW` are fully synchronous with no timeout — a read/write
  mode mismatch left the call blocked forever instead of erroring.
  Switched both new tests' pipes to `PIPE_READMODE_MESSAGE`. Caught by an
  abnormally long `windows-latest` CI run (~25 min vs. this workflow's
  usual ~1 min), cancelled and re-run after the fix.
- **Fixed:** the same two tests still hung a second `windows-latest` run
  after the fix above (~21 min), confirming the deadlock risk wasn't
  fully understood. Rather than guess a third specific cause, both tests
  now run their server/client halves on independent threads
  communicating over a channel with a 10-second `recv_timeout`, instead
  of a plain `.join()` with no bound — a real deadlock now fails the test
  in ~10s with a clear message instead of hanging the whole CI job again.
- **Fixed:** the watchdog above then reported the real root cause in
  ~11s — `TransactNamedPipe` failed with a genuine `ERROR_BAD_PIPE`
  (230), not a hang. The `call` test passed, isolating the bug to
  `transact`: a freshly-opened client handle defaults to *byte* read
  mode regardless of the server's creation-time `dwPipeMode` —
  `TransactNamedPipe` specifically requires the *calling handle itself*
  to be in message read mode. Fixed by calling `set_pipe_mode` on the
  client (switching it to `PIPE_READMODE_MESSAGE`) before `transact`.

## PR #202 — pipe: add pipe_info (GetNamedPipeInfo)
**2026-07-23** · [#202](https://github.com/baileyrd/rusty_win32/pull/202)

- **Added:** `pipe::pipe_info` (`GetNamedPipeInfo`) plus a new `PipeInfo`
  struct, closing issue #131 — reads back a named pipe's own type/mode/
  buffer-size configuration, the read-side counterpart to
  `pipe::create_server`'s creation-time parameters. Works on either a
  server handle or a client handle. Another round-2 "weak/no clear
  consumer" item (`gap-analysis.md`); no current `rush` feature asks for
  this.

## PR #201 — handle: add same_object (CompareObjectHandles)
**2026-07-23** · [#201](https://github.com/baileyrd/rusty_win32/pull/201)

- **Added:** `handle::same_object` (`CompareObjectHandles`), closing issue
  #130 — the documented-correct way to ask Windows whether two handle
  values refer to the same kernel object, since comparing raw handle
  values isn't guaranteed reliable (values get reused, and `duplicate`
  can validly produce a second value for the same object). Another
  round-2 "weak/no clear consumer" item (`gap-analysis.md`); no current
  `rush` feature asks for this.
- **Fixed:** a real CI-caught linker failure (`LNK2019: unresolved
  external symbol __imp_CompareObjectHandles`) on this crate's own
  `windows-latest` runner: some Windows SDK versions' `kernel32.lib`
  import library omits a static stub for this symbol despite it being a
  real, always-present `kernel32.dll` export. Fixed by resolving it via
  `GetProcAddress` at call time instead of this crate's usual static
  `#[link]` import — new territory for this crate, but scoped to this one
  function. `same_object`'s signature changed to `Result<bool,
  Win32Error>` to report the (practically unreachable) case where the
  lookup itself fails.
- **Fixed:** a second CI-caught issue in the same run, once the link
  failure above was fixed: `GetProcAddress` against `kernel32.dll` alone
  reported `ERROR_PROC_NOT_FOUND` on this runner too — `kernel32.dll`
  doesn't always forward this symbol by name in a way `GetProcAddress`
  resolves, even though the function genuinely exists (implemented in
  `KernelBase.dll`). Fixed by trying `KernelBase.dll` as a fallback module
  if the `kernel32.dll` lookup fails.

## PR #200 — fs: add compressed_file_size (GetCompressedFileSizeW)
**2026-07-23** · [#200](https://github.com/baileyrd/rusty_win32/pull/200)

- **Added:** `fs::compressed_file_size` (`GetCompressedFileSizeW`), closing
  issue #129 — the on-disk (compressed) size of a file vs. `fs::stat`'s
  logical size, meaningful only for an NTFS-compressed file. Disambiguates
  the call's `INVALID_FILE_SIZE` sentinel from a legitimate all-ones
  low-order size via `GetLastError()`, per its documented contract.
  Another round-2 "weak/no clear consumer" item (`gap-analysis.md`); no
  current `rush` feature asks for this.

## PR #199 — volume: add volume_path_name (GetVolumePathNameW)
**2026-07-23** · [#199](https://github.com/baileyrd/rusty_win32/pull/199)

- **Added:** `volume::volume_path_name` (`GetVolumePathNameW`), closing
  issue #128 — maps an arbitrary file/directory path to the root path of
  the volume it's on, the reverse direction of `volume_information`/
  `disk_free_space`'s own root-path parameter. Another round-2 "weak/no
  clear consumer" item (`gap-analysis.md`); no current `rush` feature asks
  for this.

## PR #198 — volume: add find_volumes (FindFirstVolumeW/FindNextVolumeW/FindVolumeClose)
**2026-07-23** · [#198](https://github.com/baileyrd/rusty_win32/pull/198)

- **Fixed:** a CI-caught race in
  `process::tests::thread_exit_code_reports_still_active_then_the_real_code`
  (added in PR #192): the process handle becoming signaled after
  `TerminateProcess` doesn't guarantee `GetExitCodeThread` already
  reflects the thread's final exit code — Windows doesn't document those
  two transitions as atomic with each other. Fixed by opening the thread
  handle with `SYNCHRONIZE` too and explicitly waiting on it (via
  `handle::wait_single_ex`, PR #195) before reading the exit code.
- **Fixed:** a second CI-caught race, in the same run, in
  `job::tests::accounting_reports_process_counts_after_a_process_exits`
  (added in PR #38): the same shape of issue — the process handle becoming
  signaled doesn't guarantee the job object's own `active_processes`
  bookkeeping has already been decremented. Fixed with a short bounded
  poll (up to 1s) instead of asserting on the very first read.
- **Added:** `volume::find_volumes` (`FindFirstVolumeW`/`FindNextVolumeW`/
  `FindVolumeClose`), closing issue #127 — enumerates every volume by its
  stable GUID path (`\\?\Volume{GUID}\`), independent of drive-letter
  assignment, unlike `volume::logical_drives`. Mirrors `fs::read_dir`'s
  `ReadDir` iterator shape (`FindClose`-on-drop). Another round-2 "weak/no
  clear consumer" item (`gap-analysis.md`); no current `rush` feature asks
  for this.

## PR #197 — handle: add signal_and_wait (SignalObjectAndWait)
**2026-07-23** · [#197](https://github.com/baileyrd/rusty_win32/pull/197)

- **Added:** `handle::signal_and_wait` (`SignalObjectAndWait`), closing
  issue #126 — atomically signals one synchronization object (mutex,
  semaphore, or event) and waits on another, avoiding the race a caller
  would otherwise accept making two separate calls. Reuses the `WaitResult`
  type `wait_single_ex`/`wait_multiple_ex` (issue #124) introduced. Another
  round-2 "weak/no clear consumer" item (`gap-analysis.md`); no current
  `rush` feature asks for this.

## PR #196 — process: add sleep_ms_ex (SleepEx)
**2026-07-23** · [#196](https://github.com/baileyrd/rusty_win32/pull/196)

- **Added:** `process::sleep_ms_ex` (`SleepEx`), closing issue #125 — the
  alertable-sleep variant of `process::sleep_ms` (`Sleep`), reporting
  `WAIT_IO_COMPLETION` if an APC woke the sleep early rather than the full
  duration elapsing. Another round-2 "weak/no clear consumer" item
  (`gap-analysis.md`); no current `rush` feature uses APCs.

## PR #195 — handle: add wait_single_ex/wait_multiple_ex (WaitForSingleObjectEx/WaitForMultipleObjectsEx)
**2026-07-23** · [#195](https://github.com/baileyrd/rusty_win32/pull/195)

- **Added:** `handle::wait_single_ex`/`handle::wait_multiple_ex` plus a new
  `WaitResult` enum (`Signaled`/`Abandoned`/`TimedOut`/`IoCompletion`),
  closing issue #124 — alertable-wait variants of the plain waits already
  used throughout this crate (`console::wait_readable`,
  `process::wait`/`wait_any`), adding `alertable` for APC wakeups. Unlike
  `process::wait_any` (scoped to process handles, which are never
  abandoned), this generic pair also reports `WaitResult::Abandoned` for a
  mutex whose owner terminated without releasing it. Another round-2
  "weak/no clear consumer" item (`gap-analysis.md`); no current `rush`
  feature uses APCs.

## PR #194 — handle: add create_semaphore/release_semaphore (CreateSemaphoreW/ReleaseSemaphore)
**2026-07-23** · [#194](https://github.com/baileyrd/rusty_win32/pull/194)

- **Added:** `handle::create_semaphore`/`handle::release_semaphore`
  (`CreateSemaphoreW`/`ReleaseSemaphore`), closing issue #123 — a counting
  semaphore, alongside the already-wrapped mutex (`handle::create_mutex`)
  the other standard Win32 synchronization primitive. Acquiring reuses
  this crate's existing `WaitForSingleObject`-shaped wait primitives
  (`console::wait_readable`), the same pattern `create_mutex` already
  established. Another round-2 "weak/no clear consumer" item
  (`gap-analysis.md`); no current `rush` feature asks for this.

## PR #193 — process: add thread_times (GetThreadTimes)
**2026-07-23** · [#193](https://github.com/baileyrd/rusty_win32/pull/193)

- **Added:** `process::thread_times` (`GetThreadTimes`), closing issue #122
  — the thread-level counterpart to `process::times` (`GetProcessTimes`),
  reusing the same `FileTime`-to-`Timespec` conversions. Another round-2
  "weak/no clear consumer" item (`gap-analysis.md`); no current `rush`
  feature asks for this.

## PR #192 — process: add thread_exit_code (GetExitCodeThread)
**2026-07-23** · [#192](https://github.com/baileyrd/rusty_win32/pull/192)

- **Added:** `process::thread_exit_code` (`GetExitCodeThread`) plus
  `THREAD_QUERY_INFORMATION`, closing issue #121 — the thread-level
  counterpart to `wait`'s process exit code (`GetExitCodeProcess`). Reports
  Windows' own `STILL_ACTIVE` (`259`) sentinel for a not-yet-exited thread
  rather than inventing a distinct error. Another round-2 "weak/no clear
  consumer" item (`gap-analysis.md`); no current `rush` feature asks for
  this.

## PR #191 — process: add affinity/set_affinity (GetProcessAffinityMask/SetProcessAffinityMask)
**2026-07-23** · [#191](https://github.com/baileyrd/rusty_win32/pull/191)

- **Added:** `process::affinity`/`process::set_affinity`
  (`GetProcessAffinityMask`/`SetProcessAffinityMask`), closing issue #120 —
  the first of the round-2 "weak/no clear consumer" items added per
  explicit direction (`gap-analysis.md`), with the usual "needs a `rush`/
  `rusty_lines` consumer" gate dropped for this round. The Windows analog
  of `sched_getaffinity`/`taskset`. No current `rush` feature asks for
  this; filed for Win32 parity/completeness.

## PR #118 — fs: add lock_file/unlock_file (LockFileEx/UnlockFileEx)
**2026-07-23** · [#118](https://github.com/baileyrd/rusty_win32/pull/118)

- **Added:** `fs::lock_file`/`fs::unlock_file` (`LockFileEx`/`UnlockFileEx`),
  closing issue #85 — the last of the 32 issues filed from the
  parity-loop's Win32 coverage sweep (`gap-analysis.md`). The Windows
  analog of `flock` at the file level, an alternative to
  `handle::create_mutex`'s named-mutex option for the same
  shared-history-file-locking use case. Reuses the same `OVERLAPPED`
  shape `watch.rs` already models (duplicated locally per this crate's
  per-module convention), since `LockFileEx`/`UnlockFileEx` require a
  non-NULL `OVERLAPPED` even for an ordinary synchronous handle, using it
  only to carry the 64-bit lock offset. No current `rush` feature asks
  for this; filed for tracking.

## PR #117 — handle: add create_mutex/release_mutex (CreateMutexW/ReleaseMutex)
**2026-07-23** · [#117](https://github.com/baileyrd/rusty_win32/pull/117)

- **Added:** `handle::create_mutex`/`handle::release_mutex`
  (`CreateMutexW`/`ReleaseMutex`), closing issue #84 from the parity-loop
  sweep — the Windows analog of `flock`'s cross-process locking, as a
  standalone kernel object rather than a file-descriptor operation.
  Acquiring an existing mutex is already covered by this crate's
  `WaitForSingleObject`-shaped wait primitives (`console::wait_readable`
  generalizes to any waitable handle, not just console ones) once a handle
  is in hand — no new wait wrapper needed. No current `rush` feature asks
  for this; filed for tracking (a plausible use case is guarding
  concurrent writes to a shared history file from multiple shell
  instances).

## PR #116 — pipe: add set_pipe_mode (SetNamedPipeHandleState)
**2026-07-23** · [#116](https://github.com/baileyrd/rusty_win32/pull/116)

- **Added:** `pipe::set_pipe_mode` (`SetNamedPipeHandleState`), closing
  issue #83 from the parity-loop sweep — exposes the raw
  `PIPE_NOWAIT`/`PIPE_READMODE_*` bits `pipe.rs` already defines, covering
  two things at once: switching between byte/message read mode after
  creation, and `PIPE_NOWAIT`, the named-pipe equivalent of the
  non-blocking check `handle::pipe_bytes_available` (`PeekNamedPipe`)
  already gives anonymous pipes.

## PR #115 — process: add list_threads/open_thread/suspend_thread (Thread32First/Thread32Next/OpenThread/SuspendThread)
**2026-07-23** · [#115](https://github.com/baileyrd/rusty_win32/pull/115)

- **Added:** `process::list_threads` (`CreateToolhelp32Snapshot`
  `TH32CS_SNAPTHREAD`/`Thread32First`/`Thread32Next`),
  `process::open_thread` (`OpenThread`), and `process::suspend_thread`
  (`SuspendThread`) plus `THREAD_SUSPEND_RESUME` — closing issue #82 from
  the parity-loop sweep. Windows has no process-wide stop primitive the
  way Unix `SIGSTOP` does; pausing every thread a process owns
  individually is the closest equivalent, the missing "pause it instead"
  counterpart to `job::terminate`'s "kill a whole job." `resume` (already
  wrapped for `spawn_suspended`'s own use) is the `SIGCONT` half — no new
  resume wrapper needed. `THREADENTRY32`'s layout independently verified
  via a compiled mingw-w64 C probe. Filed for a future `bg`/`fg`/Ctrl-Z-
  style feature currently out of `rush`'s scope, not to enable it now.

## PR #114 — process: add priority_class/set_priority_class (GetPriorityClass/SetPriorityClass)
**2026-07-23** · [#114](https://github.com/baileyrd/rusty_win32/pull/114)

- **Added:** `process::priority_class`/`process::set_priority_class`
  (`GetPriorityClass`/`SetPriorityClass`) plus the `*_PRIORITY_CLASS`
  constants (`IDLE`/`BELOW_NORMAL`/`NORMAL`/`ABOVE_NORMAL`/`HIGH`/
  `REALTIME`), closing issue #81 from the parity-loop sweep — the Windows
  analog of `nice`/`renice`. No current `rush` feature asks for this, but
  it's the natural primitive if a `nice`/`renice`-style feature is ever
  added.

## PR #113 — path: add temp_path/temp_file_name (GetTempPathW/GetTempFileNameW)
**2026-07-23** · [#113](https://github.com/baileyrd/rusty_win32/pull/113)

- **Added:** `path::temp_path`/`path::temp_file_name`
  (`GetTempPathW`/`GetTempFileNameW`), closing issue #80 from the
  parity-loop sweep — needed for heredoc scratch files or a `mktemp`
  builtin. `temp_file_name` documents a real Windows quirk rather than
  working around it silently: `GetTempFileNameW` also *creates* the
  (empty) file as a side effect, unlike a POSIX `mktemp`-style name
  generator, which only reserves a name.

## PR #112 — fs: add create_hard_link (CreateHardLinkW)
**2026-07-23** · [#112](https://github.com/baileyrd/rusty_win32/pull/112)

- **Added:** `fs::create_hard_link` (`CreateHardLinkW`), closing issue #79
  from the parity-loop sweep — `ln`'s (without `-s`) Windows counterpart,
  the non-symbolic counterpart to `create_symlink` (issue #18). Both paths
  must be on the same volume and `target_path` must already exist, a
  documented `CreateHardLinkW` restriction this wrapper doesn't check
  itself.

## PR #111 — fs: add create_directory/remove_directory (CreateDirectoryW/RemoveDirectoryW)
**2026-07-23** · [#111](https://github.com/baileyrd/rusty_win32/pull/111)

- **Added:** `fs::create_directory`/`fs::remove_directory`
  (`CreateDirectoryW`/`RemoveDirectoryW`), closing issue #78 from the
  parity-loop sweep — the primitives behind `mkdir`/`rmdir` builtins.
  `create_directory` only creates the final path component (no `mkdir -p`
  behavior); `remove_directory` requires an empty directory (no `rm -rf`
  recursion). Filed for tracking, not urgency: `rush` has no `mkdir`/`rmdir`
  builtins today.

## PR #110 — fs: add delete_file (DeleteFileW)
**2026-07-23** · [#110](https://github.com/baileyrd/rusty_win32/pull/110)

- **Added:** `fs::delete_file` (`DeleteFileW`), closing issue #77 from the
  parity-loop sweep — the primitive behind an `rm` builtin. Only removes
  files, not directories, matching `DeleteFileW`'s own scope. Filed for
  tracking, not urgency: `rush` has no `rm` builtin today.

## PR #109 — fs: add move_file (MoveFileExW)
**2026-07-23** · [#109](https://github.com/baileyrd/rusty_win32/pull/109)

- **Added:** `fs::move_file` (`MoveFileExW`) plus the
  `MOVEFILE_REPLACE_EXISTING`/`MOVEFILE_COPY_ALLOWED` flag constants,
  closing issue #76 from the parity-loop sweep — the primitive behind an
  `mv` builtin. `MOVEFILE_COPY_ALLOWED` also covers cross-volume moves,
  which `std::fs::rename` doesn't on Windows. Filed for tracking, not
  urgency: `rush` has no `mv` builtin today.

## PR #108 — fs: add copy_file (CopyFileW)
**2026-07-23** · [#108](https://github.com/baileyrd/rusty_win32/pull/108)

- **Added:** `fs::copy_file` (`CopyFileW`), closing issue #75 from the
  parity-loop sweep — the primitive behind a `cp` builtin. `fail_if_exists`
  refuses to overwrite an already-existing destination
  ([`Win32Error::ERROR_FILE_EXISTS`]) rather than this crate deciding that
  policy itself. Filed for tracking, not urgency: `rush` has no `cp`
  builtin today (likely uses `std::fs::copy`), so this is a lateral/optional
  addition unless a lower-level primitive is ever wanted.

## PR #107 — process: add set_error_mode (SetErrorMode)
**2026-07-23** · [#107](https://github.com/baileyrd/rusty_win32/pull/107)

- **Added:** `process::set_error_mode` (`SetErrorMode`) plus the
  `SEM_FAILCRITICALERRORS`/`SEM_NOOPENFILEERRORBOX` constants, closing
  issue #74 from the parity-loop sweep. Without
  `SEM_FAILCRITICALERRORS`, a hardware/media error (e.g. an empty
  removable drive, a network path that's gone away) pops a blocking GUI
  dialog that freezes the whole process — including a non-interactive
  script run with no one there to click it. A real robustness gap worth
  closing early in a shell's own startup path, not just a nice-to-have.

## PR #106 — process: add memory_status (GlobalMemoryStatusEx)
**2026-07-23** · [#106](https://github.com/baileyrd/rusty_win32/pull/106)

- **Added:** `process::memory_status` (`GlobalMemoryStatusEx`), returning a
  `MemoryStatus` (memory load percentage, total/available physical memory,
  total/available page-file space, total/available virtual address space)
  — closing issue #73 from the parity-loop sweep, the primitive behind a
  `free`-style builtin or general resource reporting. `MEMORYSTATUSEX`'s
  layout independently verified via a compiled mingw-w64 C probe.

## PR #105 — process: add computer_name (GetComputerNameW)
**2026-07-23** · [#105](https://github.com/baileyrd/rusty_win32/pull/105)

- **Added:** `process::computer_name` (`GetComputerNameW`), closing issue
  #72 from the parity-loop sweep — the primitive behind `$HOSTNAME`, a
  shell prompt, or a `hostname` builtin. Adds
  `Win32Error::ERROR_BUFFER_OVERFLOW` (111), needed to distinguish the
  documented "buffer too small, retry with the reported exact size"
  failure from a real error.

## PR #104 — process: add logical_processor_count (GetSystemInfo)
**2026-07-23** · [#104](https://github.com/baileyrd/rusty_win32/pull/104)

- **Added:** `process::logical_processor_count` (`GetSystemInfo`'s
  `dwNumberOfProcessors`), closing issue #71 from the parity-loop sweep —
  the primitive behind an `nproc`-equivalent builtin. No `Result`:
  `GetSystemInfo` has no documented failure mode, matching this crate's
  already-established "never fails" pattern. `SYSTEM_INFO`'s layout
  independently verified via a compiled mingw-w64 C probe, matching this
  crate's usual FFI-struct verification discipline.

## PR #103 — process: add sleep_ms (Sleep)
**2026-07-23** · [#103](https://github.com/baileyrd/rusty_win32/pull/103)

- **Added:** `process::sleep_ms` (`Sleep`), closing issue #70 from the
  parity-loop sweep — the direct primitive behind a `sleep`/`usleep`
  builtin. No `Result`: `Sleep` has no documented failure mode, matching
  this crate's already-established "never fails" pattern (e.g.
  `GetDriveTypeW`).

## PR #102 — volume: add disk_free_space (GetDiskFreeSpaceExW)
**2026-07-23** · [#102](https://github.com/baileyrd/rusty_win32/pull/102)

- **Added:** `volume::disk_free_space` (`GetDiskFreeSpaceExW`), returning a
  `DiskFreeSpace` (`free_bytes_available_to_caller`/`total_bytes`/
  `total_free_bytes`) — closing issue #69 from the parity-loop sweep.
  `volume.rs` already wrapped volume metadata (`GetVolumeInformationW`,
  issue #41) but not free/total space, needed for a `df`-style builtin.

## PR #101 — handle: add handle_information (GetHandleInformation)
**2026-07-23** · [#101](https://github.com/baileyrd/rusty_win32/pull/101)

- **Added:** `handle::handle_information` (`GetHandleInformation`), closing
  issue #68 from the parity-loop sweep. The read-side counterpart to
  `set_inheritable`'s write-only `SetHandleInformation` wrapper — e.g. to
  verify a redirection setup before/after marking a handle inheritable.
  Returns the raw flags bitmask unmodified, matching this crate's existing
  policy-free convention for other raw bitmask fields.

## PR #100 — pipe: add disconnect_server (DisconnectNamedPipe)
**2026-07-23** · [#100](https://github.com/baileyrd/rusty_win32/pull/100)

- **Added:** `pipe::disconnect_server` (`DisconnectNamedPipe`), closing
  issue #67 from the parity-loop sweep. `pipe::create_server`/
  `connect_server` (issue #39) had no way to disconnect and reset a served
  pipe instance for reuse with the next client — a served instance could
  only ever be used once before the whole server had to be recreated.

## PR #99 — console: add pending_input_events (GetNumberOfConsoleInputEvents)
**2026-07-23** · [#99](https://github.com/baileyrd/rusty_win32/pull/99)

- **Added:** `console::pending_input_events`
  (`GetNumberOfConsoleInputEvents`), closing issue #66 from the parity-loop
  sweep — a non-blocking "how many events are queued" check for console
  input, the console-input analog of `handle::pipe_bytes_available`
  (`PeekNamedPipe`) for pipes. `wait_readable` can only answer "is at least
  one event ready" (and blocks for a nonzero timeout); this is an
  instantaneous depth check.

## PR #98 — console: add flush_input (FlushConsoleInputBuffer)
**2026-07-23** · [#98](https://github.com/baileyrd/rusty_win32/pull/98)

- **Added:** `console::flush_input` (`FlushConsoleInputBuffer`), closing
  issue #65 from the parity-loop sweep. Discards every currently-queued,
  not-yet-read input event on a console input handle — dropping stale
  keystrokes buffered during a slow command so they don't replay into the
  next prompt, most noticeable right after `Ctrl-C` interrupts something
  while a user kept typing.

## PR #97 — console: add fill_char/fill_attribute (FillConsoleOutputCharacterW/FillConsoleOutputAttribute)
**2026-07-23** · [#97](https://github.com/baileyrd/rusty_win32/pull/97)

- **Added:** `console::fill_char`/`console::fill_attribute`
  (`FillConsoleOutputCharacterW`/`FillConsoleOutputAttribute`), closing
  issue #64 from the parity-loop sweep. A common line-editor primitive:
  erasing stale characters (and their color/attribute bits) after a
  shorter re-render, e.g. when a redrawn prompt line is shorter than what
  it's replacing — the role a VT `\x1b[K` escape plays for a caller that
  assumes that path, which this crate doesn't assume for every consumer.
  Both return the number of cells actually written, which is less than the
  requested count if the write runs past the end of the screen buffer.

## PR #96 — console: add set_cursor_position (SetConsoleCursorPosition)
**2026-07-23** · [#96](https://github.com/baileyrd/rusty_win32/pull/96)

- **Added:** `console::set_cursor_position` (`SetConsoleCursorPosition`),
  closing issue #63 from the parity-loop sweep. `console.rs` previously only
  *read* cursor position via `window_size`'s underlying
  `GetConsoleScreenBufferInfo` call; a raw-mode line editor (`rusty_lines`)
  doing multi-line prompt redraws needs to reposition the cursor directly,
  with no VT-escape-sequence fallback assumed anywhere else in this crate.
  Reuses the existing private `Coord` struct shape already modeled for
  `window_size`.

## PR #95 — console: add title/set_title (GetConsoleTitleW/SetConsoleTitleW)
**2026-07-23** · [#95](https://github.com/baileyrd/rusty_win32/pull/95)

- **Added:** `console::title`/`console::set_title`
  (`GetConsoleTitleW`/`SetConsoleTitleW`), closing issue #62 from the
  parity-loop sweep — the Windows analog of xterm's OSC title-setting
  escape sequence, a common shell feature (showing cwd/running command in
  the window title) this crate had no primitive for at all before this.
  `title` reports an empty string, not an error, for a console with no
  title set, handling the same "zero return means either buffer-too-small
  or empty" quirk `process::get_env_var` already handles for
  `GetEnvironmentVariableW`.

## PR #94 — job: add is_in_job (IsProcessInJob)
**2026-07-23** · [#94](https://github.com/baileyrd/rusty_win32/pull/94)

- **Added:** `job::is_in_job` (`IsProcessInJob`), closing issue #61 from the
  parity-loop sweep. Checks whether a process already belongs to a given
  job — or, with `job: None`, to *any* job — before calling `assign`.
  Windows automatically nests every child a job member spawns into that
  same job, and some environments (GitHub Actions' Windows runners among
  them, per rush's own `docs/WINDOWS_JOB_CONTROL.md`) start a process
  already job-scoped by an ambient job wrapping the whole step's process
  tree, which would otherwise surface as a surprise `AssignProcessToJobObject`
  failure.

## PR #93 — process: add image_path (QueryFullProcessImageNameW)
**2026-07-23** · [#93](https://github.com/baileyrd/rusty_win32/pull/93)

- **Added:** `process::image_path` (`QueryFullProcessImageNameW`), closing
  issue #60 from the parity-loop sweep. Completes `list_processes`'s
  `ProcessEntry::exe_file` (issue #21, `PROCESSENTRY32W.szExeFile`), which
  is only ever a bare filename, not a full path — needed for a `ps`-style
  listing that wants to show each process's real executable path. Unlike
  this crate's other growing-buffer calls, `QueryFullProcessImageNameW`
  doesn't report the size actually required on a "buffer too small"
  failure, so this doubles the buffer and retries instead of growing to an
  exact reported size.

## PR #92 — process: add process_id_of (GetProcessId)
**2026-07-23** · [#92](https://github.com/baileyrd/rusty_win32/pull/92)

- **Added:** `process::process_id_of` (`GetProcessId`), the reverse of
  `open_by_pid`'s (issue #13) pid-to-`HANDLE` mapping — closing issue #59
  from the parity-loop sweep. Needed anywhere rush holds a `HANDLE` (e.g.
  `spawn_suspended`'s own `process` handle) and needs to report/print its
  numeric pid without having cached it separately.

## PR #91 — path: add full_path (GetFullPathNameW)
**2026-07-23** · [#91](https://github.com/baileyrd/rusty_win32/pull/91)

- **Added:** `path::full_path` (`GetFullPathNameW`), resolving a relative
  path (or one with `.`/`..` components) to its fully qualified absolute
  form — closing issue #58 from the parity-loop sweep. Follows
  `search_path`/`short_path`/`long_path`'s existing two-attempt
  growth-buffer pattern. Unlike `short_path`/`long_path`, purely lexical —
  `GetFullPathNameW` never touches the filesystem, so it succeeds even for
  a path that doesn't exist.

## PR #90 — handle: add get_std_handle/set_std_handle (GetStdHandle/SetStdHandle)
**2026-07-23** · [#90](https://github.com/baileyrd/rusty_win32/pull/90)

- **Added:** `handle::get_std_handle`/`handle::set_std_handle`
  (`GetStdHandle`/`SetStdHandle`) plus the `STD_INPUT_HANDLE`/
  `STD_OUTPUT_HANDLE`/`STD_ERROR_HANDLE` slot constants, closing issue #57
  from the parity-loop sweep. `process.rs`'s own `spawn_suspended` doc
  comment already described redirection as "swapping the parent's
  std-handle slots before spawning, matching `winstdio`'s existing model in
  rush" — this crate previously assumed that primitive without owning it.
  `get_std_handle` returns `Ok(None)` (not `Err`) for `GetStdHandle`'s
  documented "no handle assigned" `NULL` outcome, distinct from an actual
  call failure (`INVALID_HANDLE_VALUE`).

## PR #89 — process: add get_env_var/set_env_var (GetEnvironmentVariableW/SetEnvironmentVariableW)
**2026-07-23** · [#89](https://github.com/baileyrd/rusty_win32/pull/89)

- **Added:** `process::get_env_var`/`process::set_env_var` — live single-
  variable environment access, closing issue #56 from the parity-loop sweep.
  Complements `environment_snapshot`'s full-block read (issue #19): `export
  NAME=value`/`unset NAME`/reading one `$VAR` need per-variable get/set, not
  just a startup-time snapshot. `get_env_var` reports `Ok(None)` for an unset
  variable (matching `path::search_path`'s "not found isn't an error"
  convention) and handles `GetEnvironmentVariableW`'s documented quirk where
  a set-but-empty variable also returns 0, distinguished from "not found"
  only by `GetLastError` reporting success. `set_env_var`'s `value: None`
  deletes the variable, per `SetEnvironmentVariableW`'s own contract.

## PR #88 — fs: add read_dir (FindFirstFileW/FindNextFileW/FindClose)
**2026-07-23** · [#88](https://github.com/baileyrd/rusty_win32/pull/88)

- **Added:** `fs::read_dir`, returning a `ReadDir` iterator of `DirEntry`
  (name, attributes, size, and the three `FILETIME` timestamps) — the Win32
  primitive behind directory listing, closing issue #55 from the parity-loop
  sweep. Follows the same "opening call already returned the first item"
  shape as `process::list_processes`'s `Process32FirstW` loop; the search
  handle closes via `FindClose` on `Drop`. Matches Unix `readdir` in
  reporting `.`/`..` as real entries rather than filtering them.

## PR #87 — path: add current_dir/set_current_dir (GetCurrentDirectoryW/SetCurrentDirectoryW)
**2026-07-23** · [#87](https://github.com/baileyrd/rusty_win32/pull/87)

- **Added:** `path::current_dir`/`path::set_current_dir` — the actual Win32
  primitives behind `cd`/`pwd`. Closes issue #54, the first item worked from
  a new parity-loop pass against the real Win32 API surface
  (`gap-analysis.md`, PR #86) — a systematic function-level sweep (mingw-w64
  headers as a local proxy for `windows-sys`) rather than the round-2
  assessment's needs-driven inventory. Surprising finding from that sweep:
  nothing in this crate wrapped either primitive at all before this.

## PR #86 — Add gap-analysis.md: parity-loop assessment vs. the real Win32 API surface
**2026-07-23** · [#86](https://github.com/baileyrd/rusty_win32/pull/86)

- **Added:** `gap-analysis.md`, a function-level Win32 API coverage sweep.
  32 candidate gaps identified (21 with a concrete rush/rusty_lines
  consumer, 11 plausible-but-not-yet-built coreutils-style builtins), filed
  as issues #54–#85. ~25 additional candidates found with no clear
  consumer, listed but not filed. Registry/ACLs/services/networking/ConPTY
  reconfirmed out of scope.

## PR #53 — job: narrow process_ids to Vec<u32>, matching every other pid in this crate
**2026-07-23** · [#53](https://github.com/baileyrd/rusty_win32/pull/53)

- **Changed:** `job::process_ids` now returns `Vec<u32>` instead of
  `Vec<usize>` — closes the round-2 assessment's API-consistency wart.
  Every other pid-carrying value in the public surface (`ProcessEntry.pid`,
  `JobMessage.pid`, `SpawnedProcess.process_id`, `open_by_pid`'s parameter)
  was already `u32`; `process_ids` alone exposed the raw
  `JOBOBJECT_BASIC_PROCESS_ID_LIST` wire format's pointer-sized
  (`ULONG_PTR`) width, which exists for struct alignment, not because a
  pid is ever wider than 32 bits. Breaking, pre-1.0.

## PR #52 — Add watch module: filesystem change notification (ReadDirectoryChangesW)
**2026-07-23** · [#52](https://github.com/baileyrd/rusty_win32/pull/52)

- **Added:** `watch::open_directory`/`watch::read_changes`, wrapping
  `ReadDirectoryChangesW` — closes the round-2 assessment's final item, and
  the only one that genuinely required `OVERLAPPED` I/O. No current `rush`
  feature (no file-watch builtin) asks for this; added as a standard
  building block a maturing shell eventually wants.
- `ReadDirectoryChangesW` has no way to bound how long it blocks other
  than overlapped completion — `read_changes` wraps that behind the same
  `Option<u32>` timeout convention `process::wait` already uses, cancelling
  the pending read via `CancelIoEx` on timeout so a caller never risks an
  unbounded hang.
- A buffer overflow (more change records in one burst than the internal
  64 KiB buffer holds) reports `ERROR_NOTIFY_ENUM_DIR` rather than a
  silently truncated result — Windows' own signal that changes were
  missed. `error.rs` gained this named constant.

## PR #51 — path: add short_path/long_path (GetShortPathNameW/GetLongPathNameW)
**2026-07-23** · [#51](https://github.com/baileyrd/rusty_win32/pull/51)

- **Added:** `path::short_path`/`path::long_path`, normalizing between a
  legacy 8.3 short name (e.g. `PROGRA~1`) and its long form — closes the
  round-2 assessment's last speculative item. A rare but real source of
  path-comparison surprises this crate's reparse-point-aware `fs::final_path`
  doesn't otherwise cover; no known consumer today.
- Both reuse `search_path`'s existing two-attempt buffer-growth pattern.

## PR #50 — Add volume module: drive/volume enumeration (GetLogicalDrives/GetDriveTypeW/GetVolumeInformationW)
**2026-07-23** · [#50](https://github.com/baileyrd/rusty_win32/pull/50)

- **Added:** `volume::logical_drives`/`drive_type`/`volume_information`,
  closing the round-2 assessment's remaining speculative item — a
  distinctly Windows-shaped gap (multi-root filesystem model, no Unix
  analog at all) rather than a fix for any current `rush`/`rusty_lines`
  need.
- `drive_type` never fails (matches `GetDriveTypeW`'s own contract — no
  `GetLastError` failure mode exists for it).
- `VolumeInformation`'s `file_system_flags` is exposed as a raw bitmask,
  matching this crate's existing policy-free convention for other raw
  bitmask fields (`fs::FILE_ATTRIBUTE_*`, `console::ENABLE_*`).

## PR #49 — console: add write_key_events for non-character virtual-key codes
**2026-07-23** · [#49](https://github.com/baileyrd/rusty_win32/pull/49)

- **Added:** `console::write_key_events`, extending `write_char_events`'s
  test-input-synthesis technique to non-character keys (arrows, Home/End,
  function keys, …) that carry no `uChar` at all — closes the round-2
  assessment's last item. Blocked a Windows-side test suite for
  `rusty_lines`' history/cursor/keymap navigation until now.
- Adds the `VK_*` virtual-key-code constants and `ENHANCED_KEY` (auto-set
  for the navigation-cluster keys, matching what a real keyboard driver
  always sets for them).
- Looks up a real hardware scan code via `MapVirtualKeyW` rather than
  leaving it `0` — this crate's first non-`kernel32` link (`user32.dll`),
  an expansion the README's own module docs already anticipated (alongside
  `advapi32.dll`).
- New test empirically proves the left-arrow key round-trips through
  `ENABLE_VIRTUAL_TERMINAL_INPUT` translation as the standard VT100
  cursor-left escape sequence (`\x1b[D`).

## PR #48 — Add pipe module: named pipes (CreateNamedPipeW/ConnectNamedPipeW/WaitNamedPipeW)
**2026-07-23** · [#48](https://github.com/baileyrd/rusty_win32/pull/48)

- **Added:** `pipe::create_server`/`connect_server`/`wait_for_server`/
  `open_client`, wrapping `CreateNamedPipeW`/`ConnectNamedPipeW`/
  `WaitNamedPipeW`/`CreateFileW` — closes the round-2 assessment's named-pipe
  gap. `handle::create_pipe`'s anonymous pipes have no name an arbitrary
  already-running program can open; rush's own `docs/WINDOWS_JOB_CONTROL.md`
  and `docs/CAPABILITY_GAPS.md` both name this as the missing primitive
  blocking process substitution (`<(cmd)`) and `coproc` on Windows.
- `connect_server` treats the documented `ERROR_PIPE_CONNECTED` race
  (client connects before the server calls `ConnectNamedPipeW`) as success,
  not a failure — the same pattern `process::list_processes` already uses
  for `ERROR_NO_MORE_FILES`.
- No `OVERLAPPED` support yet, matching this crate's existing synchronous-
  I/O convention elsewhere (`handle`'s anonymous pipes, `console::read`).
- `error.rs` gained `ERROR_PIPE_CONNECTED`/`ERROR_PIPE_BUSY` named
  constants.

## PR #47 — job: add resource-limit set/query and CPU/IO accounting
**2026-07-23** · [#47](https://github.com/baileyrd/rusty_win32/pull/47)

- **Added:** `job::set_resource_limits`/`job::limits` (memory, per-process
  and per-job CPU-time, and active-process-count limits) and
  `job::accounting` (`JobObjectBasicAndIoAccountingInformation`) — closes
  the round-2 assessment's Job-Object item. `rush`'s `ulimit` is flat
  "not supported" on Windows today; Job-Object limits are that crate's own
  documented answer for the only realistic partial fix, and the struct
  fields these use were already modeled bit-for-bit for
  `set_kill_on_close`, just never set beyond its one `LimitFlags` bit.
- `job::accounting` is Windows' real analog of POSIX `cutime`/`cstime`: CPU
  time aggregated across every process a job has *ever* contained,
  including already-exited ones — unlike `process::times`, which only
  reports one still-open process handle's own times.
- **Note:** `set_resource_limits` replaces the job's entire limit-info
  block in one call, same as `set_kill_on_close`/`clear_kill_on_close` —
  documented as a caveat rather than solved, since combining both concerns
  in one `SetInformationJobObject` call is a separate primitive this PR
  doesn't add.
- `Timespec` (`time.rs`) gained a `Default` impl, needed for
  `JobAccounting`'s derive.

## PR #46 — process: add GetProcessTimes wrapper (process::times)
**2026-07-23** · [#46](https://github.com/baileyrd/rusty_win32/pull/46)

- **Added:** `process::times`, wrapping `GetProcessTimes` — closes the
  round-2 assessment's other must-have. Without this, rush's `time` builtin
  had no way to report real per-child CPU time on Windows and always
  printed a hardcoded zero — a visibly wrong output, not merely a missing
  feature.
- `kernel_time`/`user_time` are elapsed *durations* since process creation,
  not wall-clock timestamps — reuses `Timespec`'s shape the same way
  `time::now_monotonic`'s result already does for a non-wall-clock value.
- `PROCESS_QUERY_LIMITED_INFORMATION` added as the narrowest `OpenProcess`
  access right this call actually needs.

## PR #45 — process/console: new-process-group spawn + targeted Ctrl-Break delivery
**2026-07-23** · [#45](https://github.com/baileyrd/rusty_win32/pull/45)

- **Added:** `process::spawn_suspended`'s `new_process_group` parameter
  (`CREATE_NEW_PROCESS_GROUP`) and `console::generate_ctrl_event`
  (`GenerateConsoleCtrlEvent`) — closes the round-2 capability assessment's
  top-ranked gap: nothing previously let a caller interrupt one child
  without hitting every process attached to the console at once.
- `CTRL_C_EVENT` can only ever be broadcast console-wide by Windows' own
  design (documented and tested: a nonzero process-group id fails with
  `ERROR_INVALID_PARAMETER`); targeting one child's group needs
  `CTRL_BREAK_EVENT` instead.
- **Changed:** `spawn_suspended`'s signature (new `new_process_group: bool`
  parameter) — a breaking change, acceptable pre-1.0.
- Note: several PRs (#22–#35) shipped between this entry and PR #9 below
  without a `RELEASE_NOTES.md` entry each — a backlog gap, not something
  this entry backfills; see `docs/CAPABILITY_ASSESSMENT.md` for that work's
  own record instead.

## PR #9 — process: add wait_any, a WaitForMultipleObjects(bWaitAll=FALSE) wrapper
**2026-07-18** · [#9](https://github.com/baileyrd/rusty_win32/pull/9)

- **Added:** `process::wait_any`, blocking on whichever of a slice of process
  handles exits first — the multi-handle counterpart to `process::wait`,
  needed for rush's `wait -n` without looping a zero-timeout `wait` over every
  tracked handle and sleeping between sweeps.
- Bounded by `WaitForMultipleObjects`'s own `MAXIMUM_WAIT_OBJECTS` (64) limit;
  exceeding it reports `ERROR_INVALID_PARAMETER` (the real call's own error),
  not a crate-invented one.

## PR #8 — job: add clear_kill_on_close, the reverse of set_kill_on_close
**2026-07-18** · [#8](https://github.com/baileyrd/rusty_win32/pull/8)

- **Added:** `job::clear_kill_on_close`, letting a job's member processes
  survive every handle to the job closing — including implicitly at the
  shell's own exit. Backs the `disown` builtin: without this, a caller
  couldn't just stop tracking a job and drop its handle, since kill-on-close
  would still fire.

## PR #7 — process: let spawn_suspended override the child's environment block
**2026-07-18** · [#7](https://github.com/baileyrd/rusty_win32/pull/7)

- **Added:** `process::environment_block` plus an `environment` parameter on
  `spawn_suspended` to hand a `CREATE_SUSPENDED` child an explicit,
  from-scratch environment block instead of inheriting the parent's real OS
  environment. Needed because rush's `vars` module never calls
  `std::env::set_var`/`remove_var` — it keeps its own exported-variable table,
  which can otherwise silently diverge from what a spawned child would
  inherit by default.

## PR #6 — Add console::write_char_events (WriteConsoleInputW) for test-driven input synthesis
**2026-07-18** · [#6](https://github.com/baileyrd/rusty_win32/pull/6)

- **Added:** `console::write_char_events`, synthesizing real console key
  events via `WriteConsoleInputW` — the standard technique console
  automation tools use to inject keystrokes.
- Not a rush/`rusty_lines` production need on its own: this exists so a test
  can drive a raw-mode reader through its real Windows I/O path end to end
  (the Windows analog of writing into one end of a Unix pty), without
  needing ConPTY.
- Its own test empirically proves the `WriteConsoleInputW` →
  `ENABLE_VIRTUAL_TERMINAL_INPUT` → `ReadFile` round trip reproduces the same
  bytes a real keypress would.

## PR #5 — Phase 4: raw-mode console primitives (GetConsoleMode/SetConsoleMode)
**2026-07-17** · [#5](https://github.com/baileyrd/rusty_win32/pull/5)

- **Added:** `console::get_mode`/`set_mode` (`GetConsoleMode`/`SetConsoleMode`,
  the Windows analog of `tcgetattr`/`tcsetattr`), `console::read` (raw
  `ReadFile`), `console::wait_readable` (`WaitForSingleObject`, the analog of
  `poll` on a single console handle), and `console::window_size`
  (`GetConsoleScreenBufferInfo`, the analog of `TIOCGWINSZ`).
- **Fixed:** switched the test suite's console-handle acquisition from
  `GetStdHandle` to `CreateFileW("CONIN$"/"CONOUT$", ...)` after
  `GetStdHandle` kept returning a stale redirected handle on `windows-latest`
  CI even after `AllocConsole` attached a real console.
- Corrected an assumption carried over from rush's own backend-analysis doc:
  Windows Ctrl-C-at-idle-prompt does **not** already work via `rusty_lines`'s
  non-Unix path — that path has no Ctrl-C handling at all without this.
- Deliberately not ConPTY: `CreatePseudoConsole` hosts a *child* process's
  console session (what a terminal emulator does), not a process reading its
  own inherited stdin the way `rusty_lines` does — `GetConsoleMode`/
  `SetConsoleMode` is the actual analog of `tcgetattr`/`tcsetattr` here.

## PR #4 — Phase 5: time module (QueryPerformanceCounter/GetSystemTimePreciseAsFileTime)
**2026-07-17** · [#4](https://github.com/baileyrd/rusty_win32/pull/4)

- **Added:** `time::now_monotonic`/`time::now_realtime` — the Windows analog
  of `rusty_libc::vdso`'s "read kernel-shared memory instead of syscalling"
  fast path (`QueryPerformanceCounter` is documented to be backed by the same
  `KUSER_SHARED_DATA` page).
- Lowest-priority module per rush's own backend analysis: no `cfg(not(unix))`
  call site in rush needs it today (rush uses `std::time` exclusively, and
  std's own Windows backend already uses `QueryPerformanceCounter`
  internally) — added for `rusty_lines`/completeness, not an open rush gap.

## PR #3 — Phase 3: process + job modules (spawn_suspended, Job Objects)
**2026-07-17** · [#3](https://github.com/baileyrd/rusty_win32/pull/3)

- **Added:** raw `CreateProcessW`-based `process::spawn_suspended`/`resume`/
  `wait`, plus the full `job` module (`create`/`assign`/`set_kill_on_close`/
  `terminate`/`process_ids`) — the primitives rush's Windows background-job
  design (`&`, `jobs`, `wait`, `kill`, `$!`) is built against. Narrowly
  scoped to what job-object-integrated spawning needs, not a replacement for
  `std::process::Command` (ordinary foreground spawn/wait already works via
  `std::process::Command`, which resolves to the same underlying calls).

## PR #2 — Phase 2: handle module (DuplicateHandle/CreatePipe/SetHandleInformation/CloseHandle)
**2026-07-17** · [#2](https://github.com/baileyrd/rusty_win32/pull/2)

- **Added:** `handle::create_pipe`/`duplicate`/`set_inheritable`/`close` — the
  Windows counterpart of Unix `dup`/`pipe2`/`close`, closing rush's
  fd-3-and-up gap at the raw-primitive level. The integer-to-`HANDLE` map
  that gives fd 3+ and `{name}>` varfd redirects any meaning stays a
  follow-up in rush itself, deliberately not this crate.

## PR #1 — Bootstrap rusty_win32: Phase 1 (Win32Error, console ctrl handler)
**2026-07-17** · [#1](https://github.com/baileyrd/rusty_win32/pull/1)

- **Added:** `error::Win32Error` (a `GetLastError()` wrapper with named
  `ERROR_*` constants, `Display`, `core::error::Error`, and an opt-in `std`
  feature adding `From<Win32Error> for std::io::Error`) and
  `console::install_ctrl_handler`/`remove_ctrl_handler`
  (`SetConsoleCtrlHandler`) — closing rush's single highest-value,
  lowest-risk Windows gap: `trap 'cmd' TERM` was silently accepted on Windows
  but had nothing installed to ever fire it.
- Established the crate's shape: `#![no_std]`-where-possible, `extern
  "system"` FFI against `kernel32.dll`, safe wrappers returning
  `Result<T, Win32Error>` with `unsafe` confined to FFI declarations and
  raw-handle-taking functions.
