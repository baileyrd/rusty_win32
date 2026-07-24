//! Windows Sockets (Winsock2) — `winsock2.h`, a new module added in
//! round 2, previously excluded by `ARCHITECTURE.md`'s non-goals (see
//! `gap-analysis.md`'s "Round 2: previously out-of-scope subsystems"
//! sweep), now in scope per explicit round-2 direction.
//!
//! Scope: basic TCP/UDP client+server socket programming, the same core
//! subset `rusty_libc` wraps for POSIX sockets. Overlapped/IOCP-based
//! async I/O, `WSAPoll`, and protocol-specific options beyond the
//! ordinary set are all explicitly out of scope for this first pass.
//!
//! This first piece is Winsock's own load/unload lifecycle —
//! `WSAStartup`/`WSACleanup`, the one primitive with no POSIX/
//! `rusty_libc` analog: every other Winsock call is documented undefined
//! behavior before a matching `WSAStartup` or after `WSACleanup`.
//! Windows reference-counts nested `WSAStartup`/`WSACleanup` pairs
//! internally, so no shared guard/RAII type is needed here — two plain
//! functions, matching this crate's existing no-`Drop`-anywhere
//! convention (`volume::FindVolumes`/`security::PathSecurityInfo`/
//! `security::BuiltAcl` are the only exceptions, none of which apply to
//! a process-global load count like this one).

#[link(name = "ws2_32")]
unsafe extern "system" {
    fn WSAStartup(version_requested: u16, wsa_data: *mut WsaData) -> i32;
    fn WSACleanup() -> i32;
    fn WSAGetLastError() -> i32;
    // The real Win32/BSD-sockets symbol is lowercase `socket`, which
    // would otherwise collide with this module's own `socket` wrapper
    // function below -- `#[link_name]` keeps the real symbol name for
    // linking while giving the Rust binding a distinct identifier.
    #[link_name = "socket"]
    fn raw_socket(address_family: i32, kind: i32, protocol: i32) -> usize;
    fn closesocket(sock: usize) -> i32;
    // Same lowercase-symbol collision as `socket` above -- `bind` would
    // otherwise clash with this module's own `bind` wrapper function.
    #[link_name = "bind"]
    fn raw_bind(sock: usize, name: *const u8, namelen: i32) -> i32;
    // Same lowercase-symbol collision as `socket`/`bind` above -- `listen`
    // would otherwise clash with this module's own `listen` wrapper
    // function.
    #[link_name = "listen"]
    fn raw_listen(sock: usize, backlog: i32) -> i32;
}

/// `INVALID_SOCKET` — the sentinel `socket` returns on failure (real
/// error code obtained separately via `WSAGetLastError`). Verified
/// against mingw-w64's own `winsock2.h` with a compiled `_Static_assert`
/// probe.
const INVALID_SOCKET: usize = usize::MAX;

/// A raw Windows `SOCKET` — matching `std::os::windows::io::RawSocket`
/// and mingw's own `SOCKET` typedef (`UINT_PTR`, pointer-sized). A
/// distinct handle namespace from [`crate::handle::RawHandle`]: a
/// `SOCKET` is closed via [`close_socket`]/`closesocket`, never
/// `CloseHandle`.
pub type RawSocket = usize;

/// `AF_INET`/`AF_INET6` — the two address families this module
/// supports (out of the many `socket` itself accepts: `AF_UNIX`/
/// `AF_IPX`/`AF_BTH`/… are all out of scope). Verified against
/// mingw-w64's own `winsock2.h` with a compiled `_Static_assert` probe.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressFamily {
    Inet = 2,
    Inet6 = 23,
}

/// `SOCK_STREAM`/`SOCK_DGRAM` — the two socket types this module
/// supports (`SOCK_RAW`/`SOCK_RDM`/`SOCK_SEQPACKET` are out of scope).
/// Verified against mingw-w64's own `winsock2.h` with a compiled
/// `_Static_assert` probe.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketKind {
    Stream = 1,
    Dgram = 2,
}

/// `IPPROTO_TCP`/`IPPROTO_UDP` — the two protocols this module supports.
/// Verified against mingw-w64's own `winsock2.h` with a compiled
/// `_Static_assert` probe.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp = 6,
    Udp = 17,
}

/// Create a new socket — `socket`. Requires [`startup`] to have been
/// called first (undefined behavior otherwise, per this module's own
/// scope note).
pub fn socket(
    family: AddressFamily,
    kind: SocketKind,
    protocol: Protocol,
) -> Result<RawSocket, crate::error::Win32Error> {
    // SAFETY: `family`/`kind`/`protocol` are plain enum-backed integer
    // values, not pointers.
    let sock = unsafe { raw_socket(family as i32, kind as i32, protocol as i32) };
    if sock == INVALID_SOCKET {
        // SAFETY: `WSAGetLastError` takes no arguments; calling it
        // immediately after a failing Winsock call is documented to
        // report that same call's error.
        Err(crate::error::Win32Error::from_raw(
            unsafe { WSAGetLastError() } as u32,
        ))
    } else {
        Ok(sock)
    }
}

/// Close a socket opened by [`socket`] — `closesocket`. Never
/// [`crate::handle::close`]/`CloseHandle`: a `SOCKET`'s destructor is
/// always this one.
///
/// # Safety
///
/// `sock` must be a currently-open, valid socket from [`socket`], not
/// already closed.
pub unsafe fn close_socket(sock: RawSocket) -> Result<(), crate::error::Win32Error> {
    // SAFETY: `sock` is caller-supplied per this function's own safety
    // contract.
    let ok = unsafe { closesocket(sock) };
    if ok != 0 {
        // SAFETY: `WSAGetLastError` takes no arguments; calling it
        // immediately after a failing Winsock call is documented to
        // report that same call's error.
        Err(crate::error::Win32Error::from_raw(
            unsafe { WSAGetLastError() } as u32,
        ))
    } else {
        Ok(())
    }
}

// WSADATA (64-bit layout, per mingw-w64's own `psdk_inc/_wsadata.h`):
// `size_of` 408 — verified field-by-field with a compiled
// `_Static_assert` probe. Never read by this crate: `startup`'s only
// interesting output (the error code, if any) comes back as
// `WSAStartup`'s own return value, matching this crate's existing
// "reports failure via its own return value directly" LSTATUS-style
// convention — so this is scratch space only, the same treatment
// `service::control`'s `ServiceStatusRaw` gets.
#[repr(C)]
struct WsaData {
    version: u16,
    high_version: u16,
    max_sockets: u16,
    max_udp_dg: u16,
    vendor_info: *mut u8,
    description: [u8; 257],
    system_status: [u8; 129],
}
const _: () = assert!(core::mem::size_of::<WsaData>() == 408);

/// `MAKEWORD(2, 2)` — Winsock 2.2, the version every modern Windows
/// ships and the only one this crate requests.
const WINSOCK_VERSION_2_2: u16 = 0x0202;

/// Initialize Winsock — `WSAStartup`, requesting version 2.2 (the
/// version every modern Windows ships). Must be called at least once
/// before any other function in this module; Windows reference-counts
/// nested calls internally, so calling this more than once (matched by
/// an equal number of [`cleanup`] calls) is documented as safe, not a
/// caller error this crate needs to guard against.
///
/// Reports failure via its own return value directly — never
/// `GetLastError`/`WSAGetLastError` — so a nonzero return is passed
/// straight to [`crate::error::Win32Error::from_raw`] rather than
/// `Win32Error::last`.
pub fn startup() -> Result<(), crate::error::Win32Error> {
    let mut wsa_data = core::mem::MaybeUninit::<WsaData>::uninit();
    // SAFETY: `wsa_data` is a valid, correctly-sized out-buffer;
    // `WSAStartup` fully initializes it on success, and its contents are
    // otherwise never read by this crate.
    let status = unsafe { WSAStartup(WINSOCK_VERSION_2_2, wsa_data.as_mut_ptr()) };
    if status != 0 {
        Err(crate::error::Win32Error::from_raw(status as u32))
    } else {
        Ok(())
    }
}

/// Tear down Winsock — `WSACleanup`. Every [`startup`] call must be
/// matched by exactly one `cleanup` call (Windows reference-counts
/// nested pairs internally); calling any other function in this module
/// after the reference count reaches zero is documented undefined
/// behavior.
///
/// Unlike [`startup`], failure is reported the ordinary
/// `GetLastError`-equivalent way — `WSAGetLastError`, a distinct
/// per-thread error slot Winsock keeps separately from the regular
/// `GetLastError`/`SetLastError` one.
pub fn cleanup() -> Result<(), crate::error::Win32Error> {
    // SAFETY: `WSACleanup` takes no arguments.
    let status = unsafe { WSACleanup() };
    if status != 0 {
        // SAFETY: `WSAGetLastError` takes no arguments; calling it
        // immediately after a failing Winsock call is documented to
        // report that same call's error.
        let err = unsafe { WSAGetLastError() };
        Err(crate::error::Win32Error::from_raw(err as u32))
    } else {
        Ok(())
    }
}

/// A local or peer socket address, IPv4 or IPv6 — the `{ip, port}`
/// representation every address-taking function in this module
/// (`bind`/`connect`/`accept`/`sendto`/`recvfrom`/`local_addr`/
/// `peer_addr`) uses, backed by [`to_sockaddr`]/[`from_sockaddr`]
/// converting to/from the real `sockaddr_in`/`sockaddr_in6` wire
/// format. `ip` octets are stored exactly as they appear on the wire
/// (already address-order, not a multi-byte integer needing an
/// endian conversion) — only `port` (and, for IPv6, nothing else) needs
/// network-byte-order handling, done internally by `to_sockaddr`/
/// `from_sockaddr`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketAddr {
    V4 {
        ip: [u8; 4],
        port: u16,
    },
    V6 {
        ip: [u8; 16],
        port: u16,
        /// `sin6_flowinfo` — an opaque 32-bit value most callers leave
        /// `0`; exposed raw and policy-free like this crate's other
        /// bitmask-shaped fields, never interpreted or byte-swapped by
        /// this module.
        flow_info: u32,
        /// `sin6_scope_id` — the IPv6 zone/interface index for
        /// link-local addresses; `0` for a global address. Exposed raw,
        /// same treatment as `flow_info`.
        scope_id: u32,
    },
}

// sockaddr_in: `size_of` 16 — verified field-by-field against
// mingw-w64's own `psdk_inc/_ip_types.h` with a compiled
// `_Static_assert` probe.
#[repr(C)]
#[derive(Clone, Copy)]
struct SockAddrIn {
    family: i16,
    port: u16,
    addr: [u8; 4],
    zero: [u8; 8],
}
const _: () = assert!(core::mem::size_of::<SockAddrIn>() == 16);

// sockaddr_in6: `size_of` 28 — verified field-by-field against
// mingw-w64's own `ws2ipdef.h` with a compiled `_Static_assert` probe.
#[repr(C)]
#[derive(Clone, Copy)]
struct SockAddrIn6 {
    family: i16,
    port: u16,
    flow_info: u32,
    addr: [u8; 16],
    scope_id: u32,
}
const _: () = assert!(core::mem::size_of::<SockAddrIn6>() == 28);

/// A `sockaddr`-shaped byte buffer big enough for either `sockaddr_in`
/// or `sockaddr_in6`, plus the real length to pass as a Win32
/// `namelen`/`addrlen` parameter — the encoded form [`to_sockaddr`]
/// produces, ready to hand to `bind`/`connect`/… as a `(*const u8,
/// i32)` pair via [`RawSockAddr::as_ptr`]/[`RawSockAddr::len`].
pub(crate) struct RawSockAddr {
    bytes: [u8; 28],
    len: i32,
}

impl RawSockAddr {
    pub(crate) fn as_ptr(&self) -> *const u8 {
        self.bytes.as_ptr()
    }

    pub(crate) fn len(&self) -> i32 {
        self.len
    }
}

/// Encode `addr` into its real `sockaddr_in`/`sockaddr_in6` wire form —
/// backing every address-taking function in this module. The reverse of
/// [`from_sockaddr`].
pub(crate) fn to_sockaddr(addr: &SocketAddr) -> RawSockAddr {
    let mut bytes = [0u8; 28];
    let len = match *addr {
        SocketAddr::V4 { ip, port } => {
            let raw = SockAddrIn {
                family: AddressFamily::Inet as i16,
                port: port.to_be(),
                addr: ip,
                zero: [0; 8],
            };
            let size = core::mem::size_of::<SockAddrIn>();
            // SAFETY: `raw` is a plain-old-data `#[repr(C)]` value (only
            // integer/byte-array fields, no padding this crate reads
            // uninitialized), valid to reinterpret as its own `size_of`
            // bytes.
            let raw_bytes =
                unsafe { core::slice::from_raw_parts((&raw as *const SockAddrIn).cast(), size) };
            bytes[..size].copy_from_slice(raw_bytes);
            size as i32
        }
        SocketAddr::V6 {
            ip,
            port,
            flow_info,
            scope_id,
        } => {
            let raw = SockAddrIn6 {
                family: AddressFamily::Inet6 as i16,
                port: port.to_be(),
                flow_info,
                addr: ip,
                scope_id,
            };
            let size = core::mem::size_of::<SockAddrIn6>();
            // SAFETY: same reasoning as the `SockAddrIn` case above.
            let raw_bytes =
                unsafe { core::slice::from_raw_parts((&raw as *const SockAddrIn6).cast(), size) };
            bytes[..size].copy_from_slice(raw_bytes);
            size as i32
        }
    };
    RawSockAddr { bytes, len }
}

/// Decode a `sockaddr_in`/`sockaddr_in6` wire-format buffer back into a
/// [`SocketAddr`] — the reverse of [`to_sockaddr`], used by functions
/// that report a peer/local address (`accept`/`recvfrom`/`local_addr`/
/// `peer_addr`).
///
/// # Safety
///
/// `ptr` must point to at least `len` readable bytes, and (if `len` is
/// large enough to name one) a valid `sin_family`/`sin6_family` at
/// offset `0`.
// Not yet called outside this module's own tests: its real callers
// (`accept`/`recvfrom`/`local_addr`/`peer_addr`) are later round-2
// items, not yet implemented -- built now alongside `to_sockaddr`
// since both directions are this module's shared address plumbing
// (see this crate's own issue #185).
#[allow(dead_code)]
pub(crate) unsafe fn from_sockaddr(
    ptr: *const u8,
    len: i32,
) -> Result<SocketAddr, crate::error::Win32Error> {
    let len = len as usize;
    if len >= core::mem::size_of::<i16>() {
        // SAFETY: `ptr` is caller-supplied per this function's own
        // safety contract, with at least `size_of::<i16>()` bytes
        // readable (just checked above).
        let family = unsafe { core::ptr::read_unaligned(ptr.cast::<i16>()) };
        if family as i32 == AddressFamily::Inet as i32 && len >= core::mem::size_of::<SockAddrIn>()
        {
            // SAFETY: `ptr` has at least `size_of::<SockAddrIn>()`
            // readable bytes, just checked above.
            let raw: SockAddrIn = unsafe { core::ptr::read_unaligned(ptr.cast()) };
            return Ok(SocketAddr::V4 {
                ip: raw.addr,
                port: u16::from_be(raw.port),
            });
        }
        if family as i32 == AddressFamily::Inet6 as i32
            && len >= core::mem::size_of::<SockAddrIn6>()
        {
            // SAFETY: `ptr` has at least `size_of::<SockAddrIn6>()`
            // readable bytes, just checked above.
            let raw: SockAddrIn6 = unsafe { core::ptr::read_unaligned(ptr.cast()) };
            return Ok(SocketAddr::V6 {
                ip: raw.addr,
                port: u16::from_be(raw.port),
                flow_info: raw.flow_info,
                scope_id: raw.scope_id,
            });
        }
    }
    Err(crate::error::Win32Error::ERROR_INVALID_PARAMETER)
}

/// Attach a local address/port to a socket — `bind`, needed before
/// [`socket`]'s result can accept incoming connections ([`SocketKind::Stream`])
/// or datagrams ([`SocketKind::Dgram`]) on a specific address, or before
/// a UDP socket sends from a fixed source port.
///
/// # Safety
///
/// `sock` must be a currently-open, valid socket from [`socket`].
pub unsafe fn bind(sock: RawSocket, addr: &SocketAddr) -> Result<(), crate::error::Win32Error> {
    let raw = to_sockaddr(addr);
    // SAFETY: `sock` is caller-supplied per this function's own safety
    // contract; `raw` is a valid `sockaddr`-shaped buffer with `raw.len()`
    // naming its exact encoded length.
    let ok = unsafe { raw_bind(sock, raw.as_ptr(), raw.len()) };
    if ok != 0 {
        // SAFETY: `WSAGetLastError` takes no arguments; calling it
        // immediately after a failing Winsock call is documented to
        // report that same call's error.
        Err(crate::error::Win32Error::from_raw(
            unsafe { WSAGetLastError() } as u32,
        ))
    } else {
        Ok(())
    }
}

/// Mark a bound TCP socket passive/listening — `listen`, needed before
/// [`socket`]'s result (already [`bind`]-ed) can accept incoming
/// connections. `backlog` is the maximum length of the pending-
/// connection queue, passed through to `listen` unmodified — this crate
/// applies no policy (e.g. clamping) to it.
///
/// # Safety
///
/// `sock` must be a currently-open, valid, already-[`bind`]-ed
/// [`SocketKind::Stream`] socket from [`socket`].
pub unsafe fn listen(sock: RawSocket, backlog: i32) -> Result<(), crate::error::Win32Error> {
    // SAFETY: `sock` is caller-supplied per this function's own safety
    // contract; `backlog` is a plain integer, not a pointer.
    let ok = unsafe { raw_listen(sock, backlog) };
    if ok != 0 {
        // SAFETY: `WSAGetLastError` takes no arguments; calling it
        // immediately after a failing Winsock call is documented to
        // report that same call's error.
        Err(crate::error::Win32Error::from_raw(
            unsafe { WSAGetLastError() } as u32,
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_then_cleanup_round_trips() {
        startup().expect("WSAStartup should succeed requesting Winsock 2.2");
        cleanup().expect("WSACleanup should succeed matching the startup call above");
    }

    #[test]
    fn nested_startup_cleanup_pairs_are_reference_counted() {
        // Windows documents WSAStartup/WSACleanup as reference-counted:
        // two startups followed by two cleanups should both succeed,
        // rather than the second cleanup failing once the "real" count
        // has already reached zero after the first.
        startup().expect("first WSAStartup should succeed");
        startup().expect("nested WSAStartup should also succeed");
        cleanup().expect("first WSACleanup should succeed");
        cleanup().expect("second WSACleanup should succeed, matching the nested startup");
    }

    #[test]
    fn socket_then_close_socket_round_trips_for_tcp_and_udp() {
        startup().expect("WSAStartup should succeed requesting Winsock 2.2");

        let tcp = socket(AddressFamily::Inet, SocketKind::Stream, Protocol::Tcp)
            .expect("socket should succeed creating a TCP/IPv4 socket");
        // SAFETY: `tcp` was just created above and hasn't been closed
        // yet.
        unsafe { close_socket(tcp) }
            .expect("closesocket should succeed on a freshly created socket");

        let udp = socket(AddressFamily::Inet, SocketKind::Dgram, Protocol::Udp)
            .expect("socket should succeed creating a UDP/IPv4 socket");
        // SAFETY: `udp` was just created above and hasn't been closed
        // yet.
        unsafe { close_socket(udp) }
            .expect("closesocket should succeed on a freshly created socket");

        cleanup().expect("WSACleanup should succeed matching the startup call above");
    }

    #[test]
    fn socket_supports_ipv6() {
        startup().expect("WSAStartup should succeed requesting Winsock 2.2");

        let sock = socket(AddressFamily::Inet6, SocketKind::Stream, Protocol::Tcp)
            .expect("socket should succeed creating a TCP/IPv6 socket");
        // SAFETY: `sock` was just created above and hasn't been closed
        // yet.
        unsafe { close_socket(sock) }
            .expect("closesocket should succeed on a freshly created socket");

        cleanup().expect("WSACleanup should succeed matching the startup call above");
    }

    #[test]
    fn to_sockaddr_then_from_sockaddr_round_trips_an_ipv4_address() {
        let addr = SocketAddr::V4 {
            ip: [127, 0, 0, 1],
            port: 8080,
        };
        let raw = to_sockaddr(&addr);
        assert_eq!(raw.len(), 16, "an encoded IPv4 sockaddr should be 16 bytes");

        // SAFETY: `raw` was just filled with exactly `raw.len()` valid
        // bytes above.
        let decoded = unsafe { from_sockaddr(raw.as_ptr(), raw.len()) }
            .expect("decoding a just-encoded sockaddr should succeed");
        assert_eq!(decoded, addr);
    }

    #[test]
    fn to_sockaddr_then_from_sockaddr_round_trips_an_ipv6_address() {
        let addr = SocketAddr::V6 {
            ip: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
            port: 9090,
            flow_info: 0,
            scope_id: 0,
        };
        let raw = to_sockaddr(&addr);
        assert_eq!(raw.len(), 28, "an encoded IPv6 sockaddr should be 28 bytes");

        // SAFETY: `raw` was just filled with exactly `raw.len()` valid
        // bytes above.
        let decoded = unsafe { from_sockaddr(raw.as_ptr(), raw.len()) }
            .expect("decoding a just-encoded sockaddr should succeed");
        assert_eq!(decoded, addr);
    }

    #[test]
    fn to_sockaddr_stores_the_port_in_network_byte_order() {
        let addr = SocketAddr::V4 {
            ip: [10, 0, 0, 1],
            port: 0x1234,
        };
        let raw = to_sockaddr(&addr);
        // SAFETY: `raw` was just filled with exactly `raw.len()` valid
        // bytes above; `sin_port` is at byte offset 2 in `sockaddr_in`.
        let port_bytes = unsafe { core::slice::from_raw_parts(raw.as_ptr().add(2), 2) };
        assert_eq!(
            port_bytes,
            &[0x12, 0x34],
            "sin_port should be big-endian (network byte order)"
        );
    }

    #[test]
    fn from_sockaddr_fails_for_an_unrecognized_address_family() {
        let bytes = [0u8; 16];
        // SAFETY: `bytes` is a valid 16-byte buffer; its first two bytes
        // (`sin_family`, all zero) don't match `AF_INET`/`AF_INET6`.
        let err = unsafe { from_sockaddr(bytes.as_ptr(), bytes.len() as i32) }
            .expect_err("from_sockaddr should fail for an unrecognized address family");
        assert_eq!(err, crate::error::Win32Error::ERROR_INVALID_PARAMETER);
    }

    #[test]
    fn bind_then_listen_succeeds_on_a_loopback_tcp_socket() {
        startup().expect("WSAStartup should succeed requesting Winsock 2.2");

        let sock = socket(AddressFamily::Inet, SocketKind::Stream, Protocol::Tcp)
            .expect("socket should succeed creating a TCP/IPv4 socket");
        let addr = SocketAddr::V4 {
            ip: [127, 0, 0, 1],
            // Port 0 asks Windows to assign any free ephemeral port --
            // this test only needs bind/listen to succeed, not a
            // specific port number.
            port: 0,
        };
        // SAFETY: `sock` was just created above and hasn't been closed
        // yet.
        unsafe { bind(sock, &addr) }.expect("bind should succeed on 127.0.0.1:0");
        // SAFETY: `sock` is still open, now bound.
        unsafe { listen(sock, 5) }.expect("listen should succeed on a freshly bound TCP socket");

        // SAFETY: `sock` was just created above and hasn't been closed
        // yet.
        unsafe { close_socket(sock) }
            .expect("closesocket should succeed on a freshly created socket");

        cleanup().expect("WSACleanup should succeed matching the startup call above");
    }
}
