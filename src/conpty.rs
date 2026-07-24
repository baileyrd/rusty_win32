//! ConPTY — Windows' pseudoconsole API (`wincon.h`'s `HPCON` family), a
//! new module added in round 2. Previously excluded for an architectural-
//! boundary reason (`rusty_lines` reads its own inherited stdin rather
//! than hosting a child's terminal), not a "no consumer" one — now in
//! scope per explicit round-2 direction, for a terminal-emulator-hosting
//! shell (or anything wanting to spawn a fully-interactive child, not
//! just redirected stdio).
//!
//! `CreatePseudoConsole`/`ResizePseudoConsole`/`ClosePseudoConsole` are
//! declared in mingw-w64's headers but gated behind `NTDDI_VERSION >=
//! NTDDI_WIN10_RS5` (Windows 10 1809) — a C probe needs
//! `-D_WIN32_WINNT=0x0A00 -DWINVER=0x0A00 -DNTDDI_VERSION=0x0A000006` to
//! see past that guard. This only affects a C header's *declaration*
//! visibility, not this crate's own `unsafe extern "system"` bindings
//! (which name the real symbols directly) or linking: `libkernel32.a`'s
//! import library already carries stub symbols for all three
//! unconditionally, verified with `nm`.
//!
//! This first piece is the pseudoconsole lifecycle itself — create,
//! live-resize, close — plus [`Hpcon`] and
//! [`PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`] (originally filed as a
//! separate issue, folded in here since [`create`]'s own signature
//! already needs the `Hpcon` type: the same real, signature-level
//! dependency that combined `net::bind`/`SocketAddr` into one PR earlier
//! in round 2). `InitializeProcThreadAttributeList`/
//! `UpdateProcThreadAttribute`/`DeleteProcThreadAttributeList` and
//! `process::spawn_suspended_with_pseudoconsole` are later round-2 items.

use crate::console::Coord;
use crate::error::Win32Error;
use crate::handle::RawHandle;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreatePseudoConsole(
        size: Coord,
        input: RawHandle,
        output: RawHandle,
        flags: u32,
        hpc: *mut Hpcon,
    ) -> i32;
    fn ResizePseudoConsole(hpc: Hpcon, size: Coord) -> i32;
    fn ClosePseudoConsole(hpc: Hpcon);
}

/// An opaque pseudoconsole handle — `HPCON`, verified as an 8-byte
/// pointer-shaped value against mingw-w64's own `wincon.h`. A distinct
/// type from [`crate::handle::RawHandle`]: its only valid destructor is
/// [`close`]/`ClosePseudoConsole`, never `CloseHandle`.
pub type Hpcon = *mut core::ffi::c_void;

/// The `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE` attribute id — the value a
/// later round-2 item (issue #188's `AttributeList::update`) needs to
/// bind an [`Hpcon`] into a process's extended startup info before
/// `CreateProcessW`. Computed by Windows' own `ProcThreadAttributeValue`
/// macro from `ProcThreadAttributePseudoConsole` (`22`) plus the
/// `PROC_THREAD_ATTRIBUTE_INPUT` bit (`0x0002_0000`); verified against
/// mingw-w64's own `winbase.h` with a compiled `_Static_assert` probe.
pub const PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE: usize = 0x0002_0016;

const HRESULT_WIN32_FACILITY_MASK: u32 = 0xFFFF_0000;
const HRESULT_WIN32_FACILITY: u32 = 0x8007_0000;

/// Unwrap an `HRESULT` back to the plain Win32 error code it was built
/// from. `CreatePseudoConsole`/`ResizePseudoConsole` are this crate's
/// only `HRESULT`-returning functions — every other wrapper reports
/// failure the ordinary `GetLastError()` way. Windows documents both as
/// reporting failure via `HRESULT_FROM_WIN32` of the underlying Win32
/// error, so this reverses that same, well-defined transform rather than
/// exposing a raw `HRESULT` this crate's `Win32Error`/`FormatMessageW`
/// machinery wouldn't recognize.
fn win32_error_from_hresult(hr: i32) -> Win32Error {
    let hr = hr as u32;
    if hr & HRESULT_WIN32_FACILITY_MASK == HRESULT_WIN32_FACILITY {
        Win32Error::from_raw(hr & 0xFFFF)
    } else {
        Win32Error::from_raw(hr)
    }
}

/// Create a pseudoconsole bound to a pipe pair — `CreatePseudoConsole`.
/// `input` is the read end of a pipe the caller writes keystrokes/input
/// into (the pseudoconsole reads from it on the hosted application's
/// behalf); `output` is the write end of a pipe the pseudoconsole
/// renders the hosted application's screen output into (the caller
/// reads from that pipe's other end). `size` is the initial
/// character-cell size.
///
/// `CreatePseudoConsole` duplicates `input`/`output` internally — per
/// Windows' own documented pattern, the caller should close its own
/// copies of `input`/`output` right after this call succeeds (via
/// [`crate::handle::close`]), keeping only the pipe pair's *other* ends
/// (the ones it uses to actually talk to the hosted application) open.
///
/// # Safety
///
/// `input`/`output` must be currently-open, valid pipe handles (e.g.
/// from [`crate::handle::create_pipe`]).
pub unsafe fn create(
    input: RawHandle,
    output: RawHandle,
    size: Coord,
) -> Result<Hpcon, Win32Error> {
    let mut hpc: Hpcon = core::ptr::null_mut();
    // SAFETY: `input`/`output` are caller-supplied per this function's
    // own safety contract; `hpc` is a valid out-pointer; `flags = 0`
    // requests no optional behavior (this module supports none yet).
    let hr = unsafe { CreatePseudoConsole(size, input, output, 0, &mut hpc) };
    if hr < 0 {
        Err(win32_error_from_hresult(hr))
    } else {
        Ok(hpc)
    }
}

/// Live-resize a pseudoconsole — `ResizePseudoConsole`, for a terminal-
/// size-change event.
///
/// # Safety
///
/// `hpc` must be a currently-open, valid pseudoconsole handle from
/// [`create`], not yet [`close`]-d.
pub unsafe fn resize(hpc: Hpcon, size: Coord) -> Result<(), Win32Error> {
    // SAFETY: `hpc` is caller-supplied per this function's own safety
    // contract; `size` is a plain-old-data value, not a pointer.
    let hr = unsafe { ResizePseudoConsole(hpc, size) };
    if hr < 0 {
        Err(win32_error_from_hresult(hr))
    } else {
        Ok(())
    }
}

/// Tear down a pseudoconsole — `ClosePseudoConsole`. Windows documents
/// this as blocking until any hosted application has exited and its
/// output has drained; it also closes its own internally-duplicated
/// copies of the `input`/`output` handles passed to [`create`] (not the
/// caller's own copies of those same two handles, already closed per
/// [`create`]'s own documented pattern, and not the pipe pair's other
/// two ends, which remain the caller's responsibility to close).
///
/// # Safety
///
/// `hpc` must be a currently-open, valid pseudoconsole handle from
/// [`create`], not already closed.
pub unsafe fn close(hpc: Hpcon) {
    // SAFETY: `hpc` is caller-supplied per this function's own safety
    // contract.
    unsafe { ClosePseudoConsole(hpc) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handle;

    #[test]
    fn create_resize_close_round_trips_a_pseudoconsole() {
        let (input_read, input_write) = handle::create_pipe()
            .expect("create_pipe should succeed for the pseudoconsole's input pipe");
        let (output_read, output_write) = handle::create_pipe()
            .expect("create_pipe should succeed for the pseudoconsole's output pipe");

        let size = Coord { x: 80, y: 25 };
        // SAFETY: `input_read`/`output_write` are freshly created, open
        // pipe handles from the calls above.
        let hpc = unsafe { create(input_read, output_write, size) }
            .expect("CreatePseudoConsole should succeed with a fresh pipe pair");
        assert!(
            !hpc.is_null(),
            "a successful CreatePseudoConsole should report a non-null HPCON"
        );

        // CreatePseudoConsole duplicated input_read/output_write
        // internally -- close this test's own copies now, per the
        // documented pattern noted on `create`.
        // SAFETY: `input_read`/`output_write` are still open and were
        // only ever passed to `create` above, never closed.
        unsafe { handle::close(input_read) }
            .expect("closing this test's own copy of the input pipe's read end should succeed");
        unsafe { handle::close(output_write) }
            .expect("closing this test's own copy of the output pipe's write end should succeed");

        let new_size = Coord { x: 100, y: 40 };
        // SAFETY: `hpc` was just created above and hasn't been closed
        // yet.
        unsafe { resize(hpc, new_size) }.expect("ResizePseudoConsole should succeed");

        // SAFETY: `hpc` is still open from the calls above.
        unsafe { close(hpc) };

        // SAFETY: `input_write`/`output_read` are this test's own
        // remaining pipe ends, never passed to `create`/`close` above.
        unsafe { handle::close(input_write) }
            .expect("closing the input pipe's write end should succeed");
        unsafe { handle::close(output_read) }
            .expect("closing the output pipe's read end should succeed");
    }
}
