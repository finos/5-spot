// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0
//! # Netlink proc connector — rung 2 of the reclaim-agent detection ladder
//!
//! Subscribes to the Linux kernel's process-event stream via the
//! [proc connector] over `NETLINK_CONNECTOR`. On every `exec(2)` the
//! kernel pushes a `proc_event { what = PROC_EVENT_EXEC, … }` message;
//! the subscriber parses it and yields a [`ProcEvent::Exec`]. The
//! reclaim-agent's main loop then resolves the pid via the existing
//! [`crate::reclaim_agent::match_pid`] helper, reusing all of rung 1's
//! match logic — only the *event source* differs.
//!
//! See `~/dev/roadmaps/completed-5spot-emergency-reclaim-by-process-match.md`
//! Phase 2.c and GitHub issue #40 for the design rationale and the
//! deferred-then-shipped history.
//!
//! ## Layering
//!
//! - **Portable parsers** — [`parse_cn_msg`], [`parse_proc_event`],
//!   the [`ProcEvent`] enum, and [`NetlinkError`] are pure
//!   byte-shovellers and have no platform `cfg`. They run on macOS in
//!   the test suite via hand-crafted byte sequences (see
//!   [`netlink_proc_tests`](../netlink_proc_tests/index.html)).
//! - **Subscriber** — [`Subscriber`] opens a netlink socket, joins the
//!   `CN_IDX_PROC` multicast group, and sends the
//!   `PROC_CN_MCAST_LISTEN` control message. The implementation is
//!   `#[cfg(target_os = "linux")]`; on every other platform the
//!   constructor returns [`NetlinkError::Unsupported`] so the rest of
//!   the binary still compiles and reports a useful error at startup
//!   when the operator passes `--detector=netlink` on a non-Linux box.
//!
//! The split is deliberate: tests for the (load-bearing) wire-format
//! decisions run anywhere `cargo test` works, while the (small,
//! mechanical) socket-shuffle code is the only part that needs a
//! Linux runtime to verify. The runtime test lives in
//! `tests/integration_netlink_proc.rs` and is `#[ignore]` so a
//! macOS dev workflow stays hermetic.
//!
//! ## Kernel + capability requirements
//!
//! - **Kernel:** Linux ≥ 2.6.15 with `CONFIG_PROC_EVENTS=y`. Default
//!   `y` on every mainstream distro (Debian, Ubuntu, RHEL/CentOS,
//!   Alpine with stock kernel, Talos, Bottlerocket, Chainguard
//!   host kernels).
//! - **Capability:** `CAP_NET_ADMIN` on the agent process. Granted
//!   at the container level via `securityContext.capabilities.add`
//!   in `deploy/node-agent/daemonset.yaml` (drop ALL, then add the
//!   one cap we need).
//! - **Architecture:** identical netlink ABI on x86_64, aarch64,
//!   armv7, ppc64le — endian + alignment match the kernel that
//!   emits the events because we run on the same host. Cross-arch
//!   container builds work without per-target code.
//! - **Not supported:** macOS, Windows, illumos, BSDs. Subscriber
//!   constructor returns [`NetlinkError::Unsupported`] on every
//!   non-Linux target.
//!
//! ## Wire layout (kernel-emitted, native-endian on every Linux target)
//!
//! ```text
//!   nlmsghdr        (16 bytes)  — read+skipped by Subscriber::next_event
//!     nlmsg_len     u32         — total length including this header
//!     nlmsg_type    u16         — NLMSG_DONE for proc-connector frames
//!     nlmsg_flags   u16
//!     nlmsg_seq     u32
//!     nlmsg_pid     u32
//!   cn_msg          (20 bytes)  — parsed by parse_cn_msg
//!     id.idx        u32         — CN_IDX_PROC = 1
//!     id.val        u32         — CN_VAL_PROC = 1
//!     seq           u32
//!     ack           u32
//!     len           u16         — payload length (24 for exec events)
//!     flags         u16
//!   proc_event      (16 + variant) — parsed by parse_proc_event
//!     what          u32         — PROC_EVENT_EXEC = 0x00000002
//!     cpu           u32         — emitting CPU; ignored at our level
//!     timestamp_ns  u64         — kernel monotonic; not wall-clock
//!     <variant payload>
//!     exec.pid      i32         — process_pid (single thread)
//!     exec.tgid     i32         — process_tgid (thread-group leader)
//! ```
//!
//! Each kernel message arrives as one datagram on the netlink
//! socket. See [Limitations](#limitations) below for the
//! multi-message-per-datagram corner case.
//!
//! ## Examples
//!
//! Parsing a hand-crafted exec event (works on any platform — useful
//! for tests, debugging, and replay tooling):
//!
//! ```
//! use five_spot::netlink_proc::{parse_cn_msg, parse_proc_event, ProcEvent, CN_IDX_PROC, CN_VAL_PROC};
//!
//! // Build a `cn_msg` carrying an exec event for pid 4242.
//! let mut frame = Vec::new();
//! frame.extend_from_slice(&CN_IDX_PROC.to_le_bytes());     // id.idx
//! frame.extend_from_slice(&CN_VAL_PROC.to_le_bytes());     // id.val
//! frame.extend_from_slice(&[0u8; 8]);                      // seq + ack
//! frame.extend_from_slice(&24u16.to_le_bytes());           // payload length
//! frame.extend_from_slice(&[0u8; 2]);                      // flags
//! // proc_event payload (24 bytes: what + cpu + timestamp + pid + tgid)
//! frame.extend_from_slice(&0x0000_0002u32.to_le_bytes());  // what = PROC_EVENT_EXEC
//! frame.extend_from_slice(&[0u8; 12]);                     // cpu (4) + timestamp_ns (8)
//! frame.extend_from_slice(&4242u32.to_le_bytes());         // exec.pid
//! frame.extend_from_slice(&4242u32.to_le_bytes());         // exec.tgid
//!
//! let payload = parse_cn_msg(&frame).expect("well-formed cn_msg");
//! let evt = parse_proc_event(payload).expect("well-formed proc_event");
//! assert_eq!(evt, ProcEvent::Exec { pid: 4242, tgid: 4242 });
//! ```
//!
//! Subscribing on a real Linux node (omitted as a doctest because it
//! requires `CAP_NET_ADMIN` and a kernel — see
//! `tests/integration_netlink_proc.rs` for a runnable test):
//!
//! ```ignore
//! use five_spot::netlink_proc::{Subscriber, ProcEvent};
//!
//! let mut sub = Subscriber::new()?;            // requires CAP_NET_ADMIN
//! loop {
//!     match sub.next_event()? {
//!         Some(ProcEvent::Exec { pid, .. }) => {
//!             // Resolve via /proc/<pid>/{comm,cmdline} using the
//!             // existing rung-1 helper — match logic is shared.
//!             if let Some(m) = five_spot::reclaim_agent::match_pid(
//!                 std::path::Path::new("/proc"), pid, &cfg
//!             ) {
//!                 // ... PATCH the Node with reclaim annotations.
//!             }
//!         }
//!         Some(ProcEvent::Other { .. }) | None => continue,
//!     }
//! }
//! # Ok::<(), five_spot::netlink_proc::NetlinkError>(())
//! ```
//!
//! ## Limitations
//!
//! - **One message per `recv` call.** [`Subscriber::next_event`]
//!   currently treats the receive buffer as a single
//!   `nlmsghdr + cn_msg + proc_event` triple. Under sustained burst
//!   load the kernel *may* pack multiple connector messages into a
//!   single datagram, in which case messages after the first in the
//!   buffer are silently dropped. In practice connector messages
//!   ride one-per-datagram on multicast sockets; the rung-1 `/proc`
//!   poll backstops any miss. If you observe drops in production,
//!   refactor to iterate via `nlmsg_len` until the buffer is
//!   exhausted.
//! - **Single subscriber per process.** The kernel happily delivers
//!   to multiple subscribers, but the agent only opens one socket
//!   per pod (one pod per node, per the DaemonSet). No coordination
//!   needed.
//! - **No replay on subscriber restart.** Events are pushed live; a
//!   crash + restart of the agent loses any events that fired during
//!   the gap. Combined with the rung-1 fallback (which polls every
//!   250 ms regardless), the worst case after a restart is one poll
//!   interval of detection latency, not a missed reclaim.
//! - **`PROC_EVENT_FORK` is not consumed.** Only `EXEC` events trigger
//!   match attempts. A long-lived process that `fork`s a worker that
//!   never `execve`s won't be matched on the worker's pid (the worker
//!   inherits the parent's `comm` until it execs, so `match_pid`
//!   would still match if the parent did). Acceptable: every match
//!   target this codebase cares about (JVMs, IDEs, compilers) execs
//!   on startup.
//!
//! ## Cancellation and clean shutdown
//!
//! Drop the [`Subscriber`] to close the socket; the kernel auto-removes
//! us from the multicast group. The reclaim-agent uses
//! `tokio::task::spawn_blocking` to host the synchronous `recv`
//! loop, and cancels via the existing per-node ConfigMap watch
//! channel — see `src/bin/reclaim_agent.rs::run_netlink_scanner`.
//!
//! [proc connector]: https://www.kernel.org/doc/html/latest/driver-api/connector.html

// ============================================================================
// Constants — wire values (kernel `linux/connector.h`, `linux/cn_proc.h`)
//
// These are the *kernel* wire values, not Rust idioms. Renaming any of
// them silently breaks every prior recording / replay tool that
// depends on the byte layout. Matched 1:1 against the upstream
// headers; bump only when the kernel does.
// ============================================================================

/// Connector index for the proc-event subsystem.
///
/// From `linux/connector.h`:
/// `#define CN_IDX_PROC 0x1`. Identifies the proc-connector subsystem
/// in the `cn_msg.id.idx` field on every event we receive and in the
/// control message we send to start the subscription.
pub const CN_IDX_PROC: u32 = 1;

/// Connector value for the proc-event subsystem.
///
/// From `linux/connector.h`:
/// `#define CN_VAL_PROC 0x1`. The other half of the connector id
/// pair; together with [`CN_IDX_PROC`] uniquely names the
/// proc-connector subsystem.
pub const CN_VAL_PROC: u32 = 1;

/// `PROC_EVENT_EXEC` discriminant — `0x0000_0002` from
/// `linux/cn_proc.h`. The kernel writes this to `proc_event.what` on
/// every successful `execve(2)`.
///
/// Visibility is `pub(crate)` rather than `pub` so the test file in
/// `netlink_proc_tests.rs` can synthesize exec frames without
/// re-deriving the value, while external callers go through the
/// typed [`ProcEvent::Exec`] variant instead of pattern-matching
/// raw bytes.
pub(crate) const PROC_EVENT_EXEC_RAW: u32 = 0x0000_0002;

/// `PROC_CN_MCAST_LISTEN` operation — the payload of the control
/// message we send to the kernel after binding the netlink socket
/// to start receiving events.
///
/// Without this control message, binding alone does not produce
/// events: the kernel ignores subscribers that have not explicitly
/// opted in. The complementary op is `PROC_CN_MCAST_IGNORE = 0x2`
/// (unsubscribe); we don't use it because dropping the [`Subscriber`]
/// closes the socket and the kernel auto-removes us.
#[cfg(target_os = "linux")]
const PROC_CN_MCAST_LISTEN: u32 = 0x0000_0001;

/// Length of a `cn_msg` header in bytes.
///
/// Layout: `id.idx` (4) + `id.val` (4) + `seq` (4) + `ack` (4) +
/// `len` (2) + `flags` (2) = 20 bytes. The `len` field is the
/// declared length of the payload that follows; the parser
/// cross-checks it against the buffer's actual remaining size.
const CN_MSG_HEADER_LEN: usize = 20;

/// Length of the fixed portion of a `proc_event` in bytes.
///
/// Layout: `what` (4) + `cpu` (4) + `timestamp_ns` (8) = 16 bytes.
/// Variant-specific payload follows immediately after; for the only
/// variant we decode (`PROC_EVENT_EXEC`) the additional payload is
/// [`PROC_EVENT_EXEC_PAYLOAD_LEN`] bytes.
const PROC_EVENT_HEADER_LEN: usize = 16;

/// Length of the `exec` variant payload in bytes.
///
/// Layout: `process_pid` (4) + `process_tgid` (4) = 8 bytes. Other
/// `proc_event` variants (FORK, EXIT, COMM, etc.) carry different
/// payloads; we don't decode them — the parser classifies them as
/// [`ProcEvent::Other`] and the caller ignores them.
const PROC_EVENT_EXEC_PAYLOAD_LEN: usize = 8;

// ============================================================================
// Public types
// ============================================================================

/// A decoded proc-connector event.
///
/// Only [`ProcEvent::Exec`] carries pid/tgid because that is the only
/// event the agent acts on. Every other `what` value is preserved as
/// [`ProcEvent::Other`] so the caller can log / count it without the
/// parser silently dropping kernel data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcEvent {
    /// `PROC_EVENT_EXEC` — a process called `execve(2)`.
    Exec {
        /// Process pid (`process_pid` field).
        pid: u32,
        /// Thread-group leader pid (`process_tgid` field). Equal to
        /// `pid` for single-threaded execs, may differ for
        /// multi-threaded ones.
        tgid: u32,
    },
    /// Any other event type. The raw `what` discriminant is preserved
    /// so callers can log / classify without re-reading the buffer.
    Other {
        /// Raw `what` value as written by the kernel.
        what: u32,
    },
}

/// Errors returned by the netlink-proc-connector parsers and subscriber.
#[derive(Debug, thiserror::Error)]
pub enum NetlinkError {
    /// The buffer was shorter than the parser needed at this offset.
    /// The struct fields name the failure precisely so a malformed
    /// frame can be diagnosed without re-reading.
    #[error("buffer too short: expected at least {expected} bytes, got {actual}")]
    Truncated {
        /// Minimum byte count the parser required.
        expected: usize,
        /// Actual byte count the parser was given.
        actual: usize,
    },
    /// The `cn_msg` carried a connector id that doesn't belong to the
    /// proc-event subsystem. Either the socket was bound to a wrong
    /// group or a kernel/wire bug delivered a message we never
    /// subscribed to. Either way: drop and log.
    #[error("invalid cn_msg id: idx={idx} val={val} (expected idx=1 val=1)")]
    InvalidId {
        /// Observed `cn_msg.id.idx`.
        idx: u32,
        /// Observed `cn_msg.id.val`.
        val: u32,
    },
    /// I/O failure on the netlink socket (Linux subscriber only).
    #[cfg(target_os = "linux")]
    #[error("netlink i/o: {0}")]
    Io(#[from] std::io::Error),
    /// The current platform does not support the netlink proc
    /// connector. Returned by [`Subscriber::new`] on non-Linux so the
    /// binary still links and reports a useful error at startup.
    #[error("netlink proc connector is not supported on this platform (Linux-only)")]
    Unsupported,
}

// ============================================================================
// Portable parsers (no #[cfg]; runnable on macOS, ARM, x86_64)
// ============================================================================

/// Parse a `cn_msg` frame and return its payload slice.
///
/// Validates the connector id `(idx=1, val=1)` and the declared
/// payload length against the buffer's actual size. Does **not** parse
/// the payload itself — pass the returned slice to [`parse_proc_event`]
/// (or any future event parser) to interpret it.
///
/// # Errors
/// - [`NetlinkError::Truncated`] when the buffer is shorter than the
///   20-byte header or shorter than `header.len + 20`.
/// - [`NetlinkError::InvalidId`] when the connector id does not match
///   the proc-event subsystem.
pub fn parse_cn_msg(bytes: &[u8]) -> Result<&[u8], NetlinkError> {
    if bytes.len() < CN_MSG_HEADER_LEN {
        return Err(NetlinkError::Truncated {
            expected: CN_MSG_HEADER_LEN,
            actual: bytes.len(),
        });
    }
    let idx = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let val = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if idx != CN_IDX_PROC || val != CN_VAL_PROC {
        return Err(NetlinkError::InvalidId { idx, val });
    }
    let len = u16::from_le_bytes(bytes[16..18].try_into().unwrap()) as usize;
    let needed = CN_MSG_HEADER_LEN + len;
    if bytes.len() < needed {
        return Err(NetlinkError::Truncated {
            expected: needed,
            actual: bytes.len(),
        });
    }
    Ok(&bytes[CN_MSG_HEADER_LEN..needed])
}

/// Parse a `proc_event` payload (as returned by [`parse_cn_msg`]) and
/// classify it.
///
/// Only `PROC_EVENT_EXEC` is decoded into [`ProcEvent::Exec`]; every
/// other `what` value becomes [`ProcEvent::Other`] so callers can
/// observe but ignore. We never silently drop kernel data.
///
/// # Errors
/// - [`NetlinkError::Truncated`] when the buffer is shorter than the
///   16-byte fixed header, or — for an exec event — shorter than the
///   16-byte header plus the 8-byte exec payload.
pub fn parse_proc_event(bytes: &[u8]) -> Result<ProcEvent, NetlinkError> {
    if bytes.len() < PROC_EVENT_HEADER_LEN {
        return Err(NetlinkError::Truncated {
            expected: PROC_EVENT_HEADER_LEN,
            actual: bytes.len(),
        });
    }
    let what = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    // bytes[4..8]   = cpu (ignored — diagnostic only)
    // bytes[8..16]  = timestamp_ns (ignored — kernel monotonic, not wall-clock)

    if what != PROC_EVENT_EXEC_RAW {
        return Ok(ProcEvent::Other { what });
    }

    let exec_end = PROC_EVENT_HEADER_LEN + PROC_EVENT_EXEC_PAYLOAD_LEN;
    if bytes.len() < exec_end {
        return Err(NetlinkError::Truncated {
            expected: exec_end,
            actual: bytes.len(),
        });
    }
    let pid = u32::from_le_bytes(
        bytes[PROC_EVENT_HEADER_LEN..PROC_EVENT_HEADER_LEN + 4]
            .try_into()
            .unwrap(),
    );
    let tgid = u32::from_le_bytes(
        bytes[PROC_EVENT_HEADER_LEN + 4..exec_end]
            .try_into()
            .unwrap(),
    );
    Ok(ProcEvent::Exec { pid, tgid })
}

// ============================================================================
// Subscriber — Linux implementation
// ============================================================================

/// Linux-only socket / bind / recv implementation of [`Subscriber`].
///
/// Kept private; the only export is [`Subscriber`] re-exported just
/// below the `mod` block so the public API surface is the same on
/// every platform. Splitting the impl into its own private module
/// keeps the `nix` import (Linux-only target dep) out of the
/// portable parser layer.
#[cfg(target_os = "linux")]
mod linux_impl {
    use super::{
        parse_cn_msg, parse_proc_event, NetlinkError, ProcEvent, CN_IDX_PROC, CN_VAL_PROC,
        PROC_CN_MCAST_LISTEN,
    };
    use nix::sys::socket::{bind, recv, send, MsgFlags, NetlinkAddr};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    /// Receive buffer size in bytes.
    ///
    /// Sizing rationale: `nlmsghdr` (16) + `cn_msg` header (20) +
    /// largest known `proc_event` payload (~40 for the COREDUMP
    /// variant) ≈ 80 bytes per message. 4 KiB gives ~50× headroom
    /// for any future variant the kernel may add and for the
    /// (rare-in-practice) case where the kernel batches multiple
    /// connector messages into one datagram. The buffer is owned
    /// by the [`Subscriber`] so the per-call allocation cost is
    /// zero — `recv(2)` writes into the same array on every event.
    const RECV_BUF_LEN: usize = 4096;

    /// Length of the fixed netlink message header in bytes.
    ///
    /// Layout (`linux/netlink.h::struct nlmsghdr`):
    /// `nlmsg_len` (4) + `nlmsg_type` (2) + `nlmsg_flags` (2) +
    /// `nlmsg_seq` (4) + `nlmsg_pid` (4) = 16.
    const NLMSGHDR_LEN: usize = 16;

    /// `NLMSG_DONE` from `linux/netlink.h` — signals "end of a
    /// multipart message." Used as the `nlmsg_type` on the outbound
    /// control message because the kernel ignores the type field on
    /// proc-connector subscribe traffic but rejects malformed netlink
    /// frames; `NLMSG_DONE` is a valid type and cannot be confused
    /// with `NLMSG_ERROR (2)` or `NLMSG_NOOP (1)`.
    const NLMSG_DONE: u16 = 3;

    /// Subscriber for kernel proc-connector events.
    ///
    /// Construction:
    /// 1. Opens an `AF_NETLINK` / `SOCK_DGRAM` socket on the
    ///    `NETLINK_CONNECTOR` protocol.
    /// 2. Binds with `nl_pid = 0` (let the kernel pick our address)
    ///    and `nl_groups = CN_IDX_PROC` (subscribe to the
    ///    proc-connector multicast group).
    /// 3. Sends a `PROC_CN_MCAST_LISTEN` control message — required
    ///    to actually start receiving events.
    ///
    /// The socket and the receive buffer are owned by the
    /// `Subscriber`; the buffer is reused across `recv` calls so
    /// steady-state allocation is zero.
    ///
    /// **Drop behaviour:** dropping the `Subscriber` closes the file
    /// descriptor (the `OwnedFd` runs `close(2)`), and the kernel
    /// auto-removes us from the multicast group. No explicit
    /// unsubscribe call is needed.
    ///
    /// **Thread safety:** `Subscriber` is `!Sync` because of the
    /// mutable buffer — `next_event` takes `&mut self`. Multiple
    /// subscribers in the same process work fine; the kernel
    /// delivers to all of them.
    ///
    /// **Blocking:** every call to [`Subscriber::next_event`] blocks
    /// the calling thread until the kernel pushes an event. Hosts
    /// with idle process tables can wait indefinitely. The
    /// reclaim-agent uses `tokio::task::spawn_blocking` to keep this
    /// off the tokio worker pool.
    pub struct Subscriber {
        /// Owned netlink socket file descriptor. Closed on `Drop`.
        fd: OwnedFd,
        /// Reusable receive buffer; reused across every `next_event`
        /// call to avoid per-call allocation.
        buf: [u8; RECV_BUF_LEN],
    }

    impl Subscriber {
        /// Open a netlink socket, join the `CN_IDX_PROC` multicast
        /// group, and start the listen subscription. After this
        /// returns `Ok`, the kernel will push every `exec(2)` (and
        /// every other `proc_event` variant) on the host into our
        /// socket.
        ///
        /// # Capability
        /// Requires `CAP_NET_ADMIN`. Without it, `bind(2)` returns
        /// `EPERM` and this function returns
        /// [`NetlinkError::Io`] wrapping the `EPERM` error. Grant
        /// the cap via the container's `securityContext.capabilities.add`
        /// (already done in `deploy/node-agent/daemonset.yaml`).
        ///
        /// # Kernel
        /// Requires `CONFIG_PROC_EVENTS=y` in the running kernel.
        /// On a kernel built without it, `socket(2)` succeeds but
        /// no events are ever delivered. There is no programmatic
        /// way to detect this from userspace; deployment-time
        /// validation is the safety net.
        ///
        /// # Errors
        /// Surfaces any underlying `socket(2)` / `bind(2)` / `send(2)`
        /// failure as [`NetlinkError::Io`]. Common cases:
        /// - `EPERM` — missing `CAP_NET_ADMIN`.
        /// - `EAFNOSUPPORT` — kernel built without `CONFIG_NETLINK`.
        /// - `EPROTONOSUPPORT` — kernel built without
        ///   `CONFIG_CONNECTOR`.
        pub fn new() -> Result<Self, NetlinkError> {
            // `nix::sys::socket::SockProtocol` does not expose
            // `NETLINK_CONNECTOR` (protocol 11). Verified missing in
            // nix 0.30.x AND nix 0.31.x (the latest as of 2026-05).
            // The other netlink protocols are enumerated
            // (`NetlinkRoute`, `NetlinkAudit`, `NetlinkSCSITransport`,
            // `NetlinkGeneric`, `NetlinkSockDiag`, `NetlinkRDMA`, …)
            // but the connector slot was missed upstream — bumping
            // the dep does not fix it. Drop to libc for this one call
            // and wrap the resulting fd in `OwnedFd` so the rest of
            // the lifecycle (Drop → close(2)) still goes through the
            // safe Rust layer.
            //
            // These are the only two `unsafe` blocks in the entire
            // 5-Spot codebase. Both are confined to this function and
            // operate on values that never leave the call (the raw fd
            // is consumed into an `OwnedFd` before any other code can
            // observe it). The invariants below are the audit
            // surface — reviewing them once is sufficient because
            // there are no other unsafe sites to coordinate with.

            // SAFETY:
            // - `socket(2)` is a pure POSIX syscall with no aliasing
            //   or memory preconditions that Rust can violate; all
            //   three arguments are integer constants exported by
            //   libc and validated at compile time.
            // - The return value is either a valid, owned, non-negative
            //   file descriptor or `-1` with `errno` set. We check for
            //   the error case immediately on the next line and never
            //   use a negative value as an fd.
            // - `errno` is read via `Error::last_os_error()` only on
            //   the error branch and only on the same thread that
            //   made the syscall, so the value is well-defined.
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            let raw_fd = unsafe {
                nix::libc::socket(
                    nix::libc::AF_NETLINK,
                    nix::libc::SOCK_DGRAM | nix::libc::SOCK_CLOEXEC,
                    nix::libc::NETLINK_CONNECTOR,
                )
            };
            if raw_fd < 0 {
                return Err(NetlinkError::Io(std::io::Error::last_os_error()));
            }
            // SAFETY:
            // - `raw_fd` was just returned by `socket(2)` above and
            //   verified `>= 0` on the previous line.
            // - The fd is exclusively owned by this call frame: it was
            //   allocated by the kernel inline in this function and
            //   has not been duplicated, leaked, or stored anywhere
            //   else.
            // - `OwnedFd::from_raw_fd`'s safety contract requires the
            //   caller to transfer ownership; we do exactly that — the
            //   raw fd is never used again as an integer after this
            //   line. The `OwnedFd` will `close(2)` it on `Drop`,
            //   satisfying the close-exactly-once invariant.
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };

            // pid=0 lets the kernel pick our netlink address;
            // groups=CN_IDX_PROC subscribes to the proc-event multicast
            // group (bit 0).
            let addr = NetlinkAddr::new(0, CN_IDX_PROC);
            bind(fd.as_raw_fd(), &addr).map_err(io_err)?;

            // Send PROC_CN_MCAST_LISTEN. Without this, binding alone is
            // not enough — the kernel ignores subscribers that have not
            // explicitly opted in.
            let mut frame = [0u8; NLMSGHDR_LEN + 20 + 4];
            let total_len = u32::try_from(frame.len()).expect("frame len fits u32");
            // nlmsghdr
            frame[0..4].copy_from_slice(&total_len.to_le_bytes());
            frame[4..6].copy_from_slice(&NLMSG_DONE.to_le_bytes());
            // flags (0), seq (0), pid (0) — already zero-initialised.
            // cn_msg
            frame[NLMSGHDR_LEN..NLMSGHDR_LEN + 4].copy_from_slice(&CN_IDX_PROC.to_le_bytes());
            frame[NLMSGHDR_LEN + 4..NLMSGHDR_LEN + 8].copy_from_slice(&CN_VAL_PROC.to_le_bytes());
            // seq, ack, flags — zero. len = 4 (uint32 payload).
            let payload_len: u16 = 4;
            frame[NLMSGHDR_LEN + 16..NLMSGHDR_LEN + 18].copy_from_slice(&payload_len.to_le_bytes());
            // payload
            frame[NLMSGHDR_LEN + 20..NLMSGHDR_LEN + 24]
                .copy_from_slice(&PROC_CN_MCAST_LISTEN.to_le_bytes());

            send(fd.as_raw_fd(), &frame, MsgFlags::empty()).map_err(io_err)?;

            Ok(Self {
                fd,
                buf: [0u8; RECV_BUF_LEN],
            })
        }

        /// Block until the kernel pushes the next event, then return it.
        ///
        /// **Blocking semantics:** the underlying `recv(2)` call
        /// blocks the calling thread until a datagram arrives. There
        /// is no built-in timeout. Hosts with no exec activity can
        /// wait indefinitely. The reclaim-agent's
        /// `run_netlink_scanner` runs this loop on
        /// `tokio::task::spawn_blocking` and cancels via dropping the
        /// `Subscriber` from the parent task.
        ///
        /// **Return value semantics:**
        /// - `Ok(Some(ProcEvent::Exec { pid, tgid }))` — a successful
        ///   `execve(2)` was observed.
        /// - `Ok(Some(ProcEvent::Other { what }))` — any other event
        ///   variant (FORK, EXIT, COMM, …). Surfaced rather than
        ///   silently dropped so the caller can log / count if
        ///   desired.
        /// - `Ok(None)` — a frame arrived but failed to parse (too
        ///   short, wrong connector id, malformed `proc_event`).
        ///   Logged at `warn` level; the caller loops and tries
        ///   again. Recoverable per-message.
        ///
        /// **Per-message logging:** parse failures emit a
        /// `tracing::warn!` with the error before returning
        /// `Ok(None)`. Callers should NOT add their own warning
        /// for the `None` case to avoid duplicate log entries.
        ///
        /// **Multi-message-per-datagram caveat:** see the module-level
        /// "Limitations" section. We currently parse only the first
        /// message in each `recv` buffer. Subsequent messages in the
        /// same datagram are silently dropped — under sustained
        /// burst load, rung 1 (`/proc` poll) is the backstop.
        ///
        /// # Errors
        /// Returns [`NetlinkError::Io`] only on socket-level failures
        /// the caller cannot recover from (closed fd, EINTR-on-shutdown,
        /// `ENOBUFS` from a sustained kernel push faster than userspace
        /// drain, etc.). Per-message parse failures are mapped to
        /// `Ok(None)` with a `tracing::warn!` for visibility.
        pub fn next_event(&mut self) -> Result<Option<ProcEvent>, NetlinkError> {
            let n = recv(self.fd.as_raw_fd(), &mut self.buf, MsgFlags::empty()).map_err(io_err)?;
            if n < NLMSGHDR_LEN {
                tracing::warn!(bytes = n, "netlink frame shorter than nlmsghdr; dropping");
                return Ok(None);
            }
            // Skip the netlink message header — it carries length /
            // type / flags / seq / pid that the proc-connector doesn't
            // need at this layer.
            let cn_bytes = &self.buf[NLMSGHDR_LEN..n];
            let payload = match parse_cn_msg(cn_bytes) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(error = %e, "cn_msg parse error; dropping frame");
                    return Ok(None);
                }
            };
            match parse_proc_event(payload) {
                Ok(evt) => Ok(Some(evt)),
                Err(e) => {
                    tracing::warn!(error = %e, "proc_event parse error; dropping frame");
                    Ok(None)
                }
            }
        }
    }

    fn io_err(e: nix::errno::Errno) -> NetlinkError {
        NetlinkError::Io(std::io::Error::from_raw_os_error(e as i32))
    }
}

#[cfg(target_os = "linux")]
pub use linux_impl::Subscriber;

// ============================================================================
// Subscriber — non-Linux stub
// ============================================================================

/// Non-Linux compile stub for [`Subscriber`].
///
/// Why a stub at all: the reclaim-agent binary is built for macOS
/// and Linux; the macOS build runs in dev/CI but never in
/// production, where the agent only ever runs on Linux nodes via
/// the DaemonSet. Stubbing the constructor keeps `cargo build`,
/// `cargo test`, and `cargo clippy` working on macOS without any
/// `#[cfg(target_os = "linux")]` gymnastics at the bin level —
/// the bin can call `Subscriber::new()` unconditionally and let
/// the runtime check (returns [`NetlinkError::Unsupported`]) tell
/// the operator their platform doesn't match their `--detector`
/// flag.
#[cfg(not(target_os = "linux"))]
mod stub_impl {
    use super::{NetlinkError, ProcEvent};

    /// Compile-only stub of [`Subscriber`] for non-Linux targets.
    ///
    /// The constructor immediately returns
    /// [`NetlinkError::Unsupported`]. This lets the binary link cleanly
    /// on macOS / Windows builds (CI smoke tests, local dev) while
    /// surfacing a clear error at startup if an operator passes
    /// `--detector=netlink` on a platform without netlink.
    ///
    /// The unit `PhantomData` field exists only so the struct has
    /// non-zero "shape" for trait-impl purposes; no instance can ever
    /// be constructed because [`Subscriber::new`] never returns
    /// `Ok(Self)`.
    #[derive(Debug)]
    pub struct Subscriber {
        // Field present so `Subscriber::next_event` has somewhere to
        // pretend to live; never constructed because `new()` never
        // returns Ok.
        _marker: std::marker::PhantomData<()>,
    }

    impl Subscriber {
        /// Always errors on non-Linux.
        ///
        /// # Errors
        /// Always returns [`NetlinkError::Unsupported`].
        pub fn new() -> Result<Self, NetlinkError> {
            Err(NetlinkError::Unsupported)
        }

        /// Unreachable in practice — `Subscriber` cannot be constructed
        /// on this platform (the constructor refuses), so any code
        /// path that holds a `&mut Subscriber` was crafted from
        /// nothing. Defined for API parity so the bin compiles
        /// uniformly across platforms.
        ///
        /// # Errors
        /// Always returns [`NetlinkError::Unsupported`]; in practice it
        /// is unreachable because [`Subscriber::new`] never returns
        /// `Ok`.
        pub fn next_event(&mut self) -> Result<Option<ProcEvent>, NetlinkError> {
            Err(NetlinkError::Unsupported)
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub use stub_impl::Subscriber;

#[cfg(test)]
#[path = "netlink_proc_tests.rs"]
mod tests;
