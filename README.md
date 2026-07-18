# rusty_win32

A `#![no_std]`-where-possible, minimal-dependency, **Windows-only** Rust
crate that gives [rush](https://github.com/baileyrd/rush) a `sys::win32`
backend — the Windows counterpart to
[rusty_libc](https://github.com/baileyrd/rusty_libc), which does the same
job for Linux. Same philosophy, different platform: Windows guarantees no
stable syscall numbers, only stable, documented DLL exports, so this crate
is `extern "system"` FFI against `kernel32.dll` (and later `advapi32.dll`),
not raw `asm!` — see the crate's own module docs (`src/lib.rs`) for why this
isn't a port of `rusty_libc`'s architecture.

## Status: Phase 1

- [`error::Win32Error`] — a `GetLastError()` wrapper with named `ERROR_*`
  constants, `Display`, `core::error::Error`, and an opt-in `std` feature
  adding `From<Win32Error> for std::io::Error`.
- [`console::install_ctrl_handler`] / [`console::remove_ctrl_handler`] —
  `SetConsoleCtrlHandler`, closing rush's single highest-value, lowest-risk
  Windows gap: `trap 'cmd' TERM` is accepted but silently never fires today,
  for lack of anything to install a handler.

Everything else — `process`/`job` (Job Objects, for background jobs),
`handle` (the fd-3-and-up gap), ConPTY — is future work, not yet started.
See `docs/WINDOWS_BACKEND_ANALYSIS.md` in the rush repo for the full
primitive-by-primitive analysis and phasing this crate is being built
against.

## Testing

Real behavioral testing needs a Windows machine — this crate is developed
from a Linux sandbox with no way to execute a Windows binary, so:

- `cargo test`/`cargo clippy` on the host target cover the platform-neutral
  logic that doesn't touch a real Win32 call (`error.rs`'s `Display`/
  `From<Win32Error> for std::io::Error` logic).
- `cargo check --target x86_64-pc-windows-gnu` (via `mingw-w64`, see
  `.cargo/config.toml`) is a fast compile-only sanity check for everything
  else, including `console.rs`'s `#[cfg(windows)]`-gated code.
- The real gate is CI's `windows-latest` job, which actually runs every
  test, `console.rs`'s included.

## License

MIT
