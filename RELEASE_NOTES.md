# Release Notes

One entry per merged PR against `main`, reverse chronological. No version tags
exist yet (still pre-1.0/unreleased on crates.io), so this tracks by PR rather
than by tag — see `CHANGELOG.md` for the `[Unreleased]` rollup once a tag ships.

---

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
