# Gap analysis: rusty_win32 vs. the Win32 API surface (parity-loop, 2026-07-23)

Reference surface: the real Win32 API, as declared in mingw-w64's C headers
(`/usr/x86_64-w64-mingw32/include/`) — used as a local, reliable proxy for
`windows-sys`/`windows` (both are generated from the same official Win32
metadata Microsoft publishes, so function names/signatures match). A literal
`cargo public-api` diff wasn't used: `rusty_win32` wraps raw Win32 calls under
its own friendlier names rather than mirroring windows-sys's flat, 1:1-named
surface, so symbol-name diffing doesn't apply the way it does for
`rusty_libc` vs. the `libc` crate — this is a function-level coverage sweep
instead (same spirit, adapted mechanism).

Already wrapped (47 functions across `console`/`error`/`fs`/`handle`/`job`/
`path`/`pipe`/`process`/`time`/`volume`/`watch`) and already explicitly
out-of-scope per `ARCHITECTURE.md` (registry, ACLs/security descriptors,
service control, networking, ConPTY) are excluded below — see the sweep's
own notes for the full excluded list.

Platform column is omitted: every row is Windows-only (the whole crate is).
Breaking column is omitted: every row is a pure addition (no existing public
signature needs to change for any of these).

## Strong candidates — concrete rush/rusty_lines consumer

| Symbol | Category | Reference | Est. size | Notes |
| --- | --- | --- | --- | --- |
| `GetCurrentDirectoryW`/`SetCurrentDirectoryW` | fn pair | `processenv.h` | S | The actual Win32 primitive behind `cd`/`pwd` — confirmed nothing in `src/` wraps CWD get/set at all today. |
| `FindFirstFileW`/`FindNextFileW`/`FindClose` | fn group | `fileapi.h` | L | Directory listing — `fs.rs` only stats individual paths today; no way to enumerate a directory's contents, needed by any future `ls`/tab-completion/glob that walks a directory. |
| `GetEnvironmentVariableW`/`SetEnvironmentVariableW` | fn pair | `processenv.h` | S each | Single-variable get/set — `environment_snapshot` (issue #19) only reads a full snapshot; `export`/`unset`/single-`$VAR` mutation needs live per-variable access. |
| `GetStdHandle`/`SetStdHandle` | fn pair | `processenv.h` | S each | `process.rs`'s own doc comment for `spawn_suspended` says redirection happens by "swapping the parent's std-handle slots before spawning" — this is that primitive, not currently owned by this crate. |
| `GetFullPathNameW` | fn | `fileapi.h` | M | Relative→absolute path canonicalization — `path.rs` has 8.3 short/long conversion but nothing for plain absolute-path resolution (`cd`/`pwd` internals). |
| `GetProcessId` | fn | `processthreadsapi.h` | S | HANDLE→pid, the reverse of already-wrapped pid→HANDLE (`open_by_pid`, issue #13) — needed anywhere rush holds a `HANDLE` and needs to report/print its pid. |
| `QueryFullProcessImageNameW` | fn | `processthreadsapi.h` | M | `list_processes`'s `ProcessEntry.exe_file` (issue #21) is a bare filename only — this is the gap for a `ps`-style listing that wants each process's full path. |
| `IsProcessInJob` | fn | `jobapi.h` | S | Windows nests every child a job member spawns into that same job automatically — checking this before `AssignProcessToJobObject` avoids surprise failures when rush is itself already inside an ambient job (common on CI runners/Windows Terminal). Directly relevant to the existing job-object process-group model (issues #14/#38). |
| `SetConsoleTitleW`/`GetConsoleTitleW` | fn pair | `wincon.h` | S each | Terminal title bar showing cwd/running command — the Windows analog of xterm's OSC title-setting escape sequence, a common shell feature. |
| `SetConsoleCursorPosition` | fn | `wincon.h` | S | `console.rs` only *reads* cursor position (`GetConsoleScreenBufferInfo`) — a raw-mode line editor (`rusty_lines`) doing multi-line prompt redraws needs to *move* the cursor too. |
| `FillConsoleOutputCharacterW`/`FillConsoleOutputAttribute` | fn pair | `wincon.h` | S each | Clear-to-end-of-line-style redraws — a common line-editor primitive for erasing stale characters after a shorter re-render. |
| `FlushConsoleInputBuffer` | fn | `wincon.h` | S | Discards pending input events — e.g. dropping stale keystrokes buffered during a slow command so they don't replay into the next prompt (common after Ctrl-C). |
| `GetNumberOfConsoleInputEvents` | fn | `wincon.h` | S | Non-blocking "how much is queued" check, the input-side parallel to already-wrapped `pipe_bytes_available` (`PeekNamedPipe`) for pipes. |
| `DisconnectNamedPipe` | fn | `namedpipeapi.h` | S | `pipe.rs`'s own gap: `create_server`/`connect_server` has no way to disconnect and reset a served pipe instance for reuse — currently single-use only. |
| `GetHandleInformation` | fn | `handleapi.h` | S | Read-side counterpart to already-wrapped `SetHandleInformation` (`handle::set_inheritable`) — verifying a handle's current inherit-flag state. |
| `GetDiskFreeSpaceExW` | fn | `fileapi.h` | S | `volume.rs` wraps volume metadata (`GetVolumeInformationW`) but not free-space — needed for a `df`-style builtin. |
| `Sleep` | fn | `synchapi.h` | S | Direct, trivial primitive behind a `sleep`/`usleep` builtin. |
| `GetSystemInfo` | fn | `sysinfoapi.h` | S | `dwNumberOfProcessors` is the primitive behind an `nproc`-equivalent builtin. |
| `GetComputerNameW`/`GetComputerNameExW` | fn | `sysinfoapi.h` | S | Hostname — `$HOSTNAME`, a shell prompt, or a `hostname` builtin. |
| `GlobalMemoryStatusEx` | fn | `sysinfoapi.h` | S | System memory totals/load — a `free`-style builtin or resource reporting. |
| `SetErrorMode` | fn | `errhandlingapi.h` | S | Without `SEM_FAILCRITICALERRORS`, a hardware/media error (e.g. an empty removable drive) pops a blocking GUI dialog that freezes a non-interactive shell/script run — real robustness gap for rush's startup path. |

## Moderate candidates — plausible builtins, not yet built in rush

| Symbol | Category | Reference | Est. size | Notes |
| --- | --- | --- | --- | --- |
| `CopyFileW`/`CopyFileExW` | fn | `winbase.h` | M | Backs a `cp` builtin. |
| `MoveFileW`/`MoveFileExW` | fn | `winbase.h` | M | Backs `mv`, cross-volume-safe rename. |
| `DeleteFileW` | fn | `winbase.h` | S | Backs `rm` (files). |
| `CreateDirectoryW`/`RemoveDirectoryW` | fn pair | `fileapi.h` | S each | Backs `mkdir`/`rmdir`. |
| `CreateHardLinkW` | fn | `winbase.h` | S | Hard-link creation, non-symbolic counterpart to already-wrapped `CreateSymbolicLinkW` — backs `ln` (no `-s`). |
| `GetTempPathW`/`GetTempFileNameW` | fn pair | `fileapi.h` | S each | Heredoc scratch files, or a `mktemp` builtin. |
| `GetPriorityClass`/`SetPriorityClass` | fn pair | `processthreadsapi.h` | S each (M pair) | Windows analog of `nice`/`renice` for background jobs. |
| `SuspendThread` + `Thread32First`/`Thread32Next` | fn group | `processthreadsapi.h`/`tlhelp32.h` | M | Closest Windows equivalent to `SIGSTOP`/`SIGCONT` (`bg`/`fg`/Ctrl-Z-style job stop) — Windows has no process-level stop primitive, needs per-thread suspend + enumeration. |
| `SetNamedPipeHandleState` | fn | `namedpipeapi.h` | M | Byte/message mode switch + blocking-behavior adjustment for named pipes — would let `pipe.rs` support non-blocking reads like `pipe_bytes_available` does for anonymous pipes. |
| `CreateMutexW`/`ReleaseMutex`/`OpenMutexW` | fn group | `synchapi.h` | M | Named cross-process mutex — Windows analog of `flock`, e.g. guarding concurrent writes to a shared history file from multiple rush instances. |
| `LockFileEx`/`UnlockFileEx` | fn pair | `fileapi.h` | M | Advisory file locking — alternative to the `CreateMutexW` option above for the same shared-history-file use case. |

## Weak/no clear consumer — listed for completeness, not recommended to file

`GetProcessAffinityMask`/`SetProcessAffinityMask`, `GetExitCodeThread`,
`GetThreadTimes`, `CreateSemaphoreW`, `WaitForSingleObjectEx`/
`WaitForMultipleObjectsEx`, `SleepEx`, `SignalObjectAndWait`,
`FindFirstVolumeW`/`FindNextVolumeW`/`FindVolumeClose`, `GetVolumePathNameW`,
`GetCompressedFileSizeW`, `CompareObjectHandles`, `GetNamedPipeInfo`,
`TransactNamedPipe`, `CallNamedPipeW`, `OpenJobObjectW`, `GetTickCount64`,
`GetSystemDirectoryW`/`GetWindowsDirectoryW`, `GetLogicalProcessorInformation`,
`SetLastError` (internal-hardening only), `AddVectoredExceptionHandler`/
`SetUnhandledExceptionFilter`, `GetConsoleProcessList`, `GetConsoleWindow`,
`SetConsoleTextAttribute` (superseded by VT sequences this crate already
supports), `ReadConsoleW`/`WriteConsoleW` (conflicts with the crate's
documented choice of `ReadFile` over `ReadConsole`), `AllocConsole`/
`FreeConsole`/`AttachConsole`, `GetLargestConsoleWindowSize`/
`SetConsoleScreenBufferSize`/`SetConsoleWindowInfo`.

## Explicitly reconfirmed out of scope (no new evidence, matches ARCHITECTURE.md)

Registry access, security descriptors/ACLs, service control, networking,
ConPTY/`CreatePseudoConsole`.
