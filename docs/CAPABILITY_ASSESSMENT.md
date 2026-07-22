# Capability assessment (2026-07-22)

A from-the-code audit of what `rusty_win32` currently provides, what state it's
actually in versus what the README claims, and what's still missing for it to
be a complete `sys::win32` backend for `rush` (and, secondarily, for
`rusty_lines`). Written from this crate's own source and doc comments only —
`rush`'s `docs/WINDOWS_BACKEND_ANALYSIS.md` and `docs/WINDOWS_JOB_CONTROL.md`
live in a different repository not in scope for this pass, so items below are
derived independently and should be cross-checked against those docs rather
than assumed to supersede them.

## 1. Documentation is stale

`README.md` still says **"Status: Phase 1"** and lists only `error` and
`console::install_ctrl_handler`/`remove_ctrl_handler`. The actual code is far
past that:

- Phase 2 (`handle`): done — `create_pipe`, `duplicate`, `set_inheritable`, `close`.
- Phase 3 (`process`/`job`): done — `spawn_suspended`/`resume`/`wait`/`wait_any`,
  `environment_block`, full Job Object lifecycle (`create`, `assign`,
  `set_kill_on_close`/`clear_kill_on_close`, `terminate`, `process_ids`).
- Phase 4 (`console` raw mode): done — `get_mode`/`set_mode`, `read`,
  `wait_readable`, `window_size`, `write_char_events`.
- Phase 5 (`time`): done — `now_monotonic`/`now_realtime`.

**Action item:** update `README.md`'s status section to match `lib.rs`'s own
module-doc history — the crate is at (informally) Phase 6, not Phase 1. This
is the single lowest-effort, highest-visibility fix here.

## 2. Minor API-surface inconsistency

`lib.rs` re-exports the *original* function from each phase at the crate root
but not every later addition to the same module:

- `process::wait_any` and `process::environment_block` are not re-exported
  (only `current_pid`, `resume`, `spawn_suspended`, `wait` are).
- `job` has no crate-root re-exports at all — every item (`create`, `assign`,
  `set_kill_on_close`, `clear_kill_on_close`, `terminate`, `process_ids`) is
  reached only via `rusty_win32::job::*`.

Not a functional gap, but worth a decision: either re-export consistently, or
document that `job`'s multi-item surface is deliberately namespaced (unlike
the single/few-item modules) and leave `process`'s two stragglers as an
oversight to fix.

## 3. Real functional gaps

Ranked roughly by how likely `rush` is to need them soon, based on what a
POSIX-shell backend needs that this crate doesn't yet have.

### High priority

- **PATH/command resolution is Unix-shaped, not Windows-shaped.** Nothing
  here wraps `SearchPathW` or replicates `PATHEXT`-based resolution
  (`.exe`/`.cmd`/`.bat`/`.com`/…). Unix "is this file executable" is a mode
  bit `rusty_libc` can just `stat`; Windows has no such bit — executability is
  extension- and-registration-based. Without this primitive, `rush`'s command
  lookup either has to reinvent `PATHEXT` scanning itself or misbehaves on
  Windows for bare `foo` (vs `foo.exe`) invocations. This is arguably the
  single biggest remaining *correctness* gap for interactive use, not just a
  nice-to-have.
- **No way to signal/kill a process rusty_win32 didn't spawn.** `job::terminate`
  kills everything in a job; there's no `process`-level `TerminateProcess`
  wrapper and no `OpenProcess`-by-pid. If `rush` ever needs `kill <pid>` for a
  pid it only knows numerically (not one of its own `SpawnedProcess` handles),
  there's currently no primitive for it.
- **Job-object completion is poll-only.** `job::process_ids` is documented as
  "one way to poll" for job emptiness; there's no
  `SetInformationJobObject(JobObjectAssociateCompletionPortInformation)` wiring
  for event-driven "a job member just exited" notification. `wait_any` covers
  waiting on a *known, bounded* set of process handles (≤ `MAXIMUM_WAIT_OBJECTS`
  = 64) but doesn't scale past that and doesn't give job-level exit
  notifications the way Unix `SIGCHLD` does. Worth resolving against
  whatever `WINDOWS_JOB_CONTROL.md` already decided before building it blind.
- **No non-blocking read primitine for redirected (non-console) input.**
  `console::wait_readable` wraps `WaitForSingleObject`, which is
  console-input-handle-specific in this crate's docs. An anonymous pipe read
  end (from `handle::create_pipe`, e.g. a background job's captured stdout) is
  not usable the same way for a "is there data yet, don't block" check —
  Windows' answer for pipes is `PeekNamedPipe`, which this crate doesn't wrap
  yet. Needed for any job-control feature that wants to peek at a background
  job's output without blocking the shell's own prompt loop.

### Medium priority

- **Console codepage is never touched.** Nothing here calls
  `SetConsoleOutputCP`/`SetConsoleCP(CP_UTF8)`. A legacy-codepage console can
  mis-render non-ASCII bytes even once VT processing/raw mode is otherwise
  correct — this is the Windows analog of making sure a Unix terminal's locale
  is UTF-8, and currently entirely unaddressed.
- **No filesystem "stat" primitives.** No wrapper for
  `GetFileAttributesExW`/`GetFileInformationByHandle` (size, timestamps,
  reparse-point/directory bits) — the Windows counterpart of whatever
  `rusty_libc` exposes for `stat`/`lstat`. `ls`, globbing, and `[ -d/-f/-L ]`
  test operators all eventually need this on Windows too.
- **No symlink/reparse-point support.** `CreateSymbolicLinkW`,
  `GetFinalPathNameByHandleW` (for canonicalizing through reparse points), and
  reading a reparse point's target are all absent. Needed for `ln -s`
  parity and for any path-resolution code that has to walk through a
  Windows symlink or junction correctly.
- **Environment snapshot on startup.** `process::environment_block` builds an
  environment block to *hand to a child*, but there's no wrapper to read the
  *current* process's real environment back out (`GetEnvironmentVariableW` or
  an `GetEnvironmentStringsW` snapshot). `rush`'s own variable table needs to
  be seeded from the real inherited environment at shell startup somehow;
  right now that seeding path has no primitive here unless it's handled
  entirely on the `std::env` side already available to `rush` without this
  crate's help (worth confirming which is actually intended before treating
  this as a gap).
- **Error messages are a fixed, hand-maintained table.** `Win32Error`'s
  `Display` only knows ~25 hardcoded codes; anything else prints "unknown
  Win32 error N" even though `FormatMessageW` could produce the real system
  text for virtually any code. Given the crate already has an `alloc`-capable
  path (used in `process.rs`/`job.rs`/`console.rs`), a `FormatMessageW`-backed
  `Win32Error::message() -> alloc::string::String` (independent of the `std`
  feature) would be a meaningful error-quality upgrade without giving up the
  fast, allocation-free `Display` path for the common/named codes.

### Lower priority / explicitly out of scope for now

- **ConPTY** — deliberately not implemented; the module docs already record
  *why* (it's the wrong primitive for `rusty_lines`' own-process raw-mode
  reads, not a host-a-child-terminal use case). No change recommended here
  unless `rush` grows an actual need to host a child's console session (e.g.
  a `script`-like recorder), which is a different feature than anything this
  crate currently targets.
- **Process enumeration** (`CreateToolhelp32Snapshot`/`Process32First/Next`)
  for a `ps`-equivalent — plausible future need, but nothing in this crate's
  existing scope calls for it yet.
- **Networking, registry, and service-management APIs** — no evidence any
  current or near-term `rush`/`rusty_lines` feature needs these; flagging
  only so they're recognized as consciously deferred, not overlooked.

## 4. Suggested next phase

If picking one item to build next, **PATH/command resolution
(`PATHEXT`-aware `SearchPathW` wrapper)** is the strongest candidate: it's the
only gap above that changes whether ordinary interactive command execution
behaves correctly on Windows at all, versus everything else here being a
robustness/ergonomics/observability improvement on already-working
functionality. `process`-level `TerminateProcess` and `PeekNamedPipe` are the
next two most load-bearing for job control specifically.
