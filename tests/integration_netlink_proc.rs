// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0
//! Linux runtime test for the netlink proc connector subscriber.
//!
//! Per `5spot-emergency-reclaim-by-process-match.md` Phase 2 rung 2 +
//! GitHub issue #40 acceptance criteria: "Runtime verified on a real
//! Linux node: JVM launch produces a match within <100 ms."
//!
//! ## Run
//!
//! ```bash
//! # Linux only — needs CAP_NET_ADMIN. On a normal user account this
//! # will fail at Subscriber::new(). Run as root, or grant the cap on
//! # the binary, or run inside a privileged container:
//! sudo cargo test --test integration_netlink_proc -- --ignored
//! ```
//!
//! On macOS / Windows the test compiles to a no-op and reports
//! "ignored" — there is no netlink to verify against.

#![cfg(target_os = "linux")]

use five_spot::netlink_proc::{NetlinkError, ProcEvent, Subscriber};
use std::process::Command;
use std::time::{Duration, Instant};

/// End-to-end: open a netlink subscriber, spawn a child via Command,
/// assert a `ProcEvent::Exec` arrives within 100 ms with the child's
/// pid in the payload.
///
/// `#[ignore]` because:
/// - Requires `CAP_NET_ADMIN` (root, file capability, or privileged
///   container) — `cargo test` cannot grant it itself.
/// - Subscribes to *all* exec events on the host. On a busy machine
///   the assertion that we see *our* pid is racy by definition; we
///   accept that risk by using a recognisable child command and
///   waiting for *any* matching event.
#[test]
#[ignore]
fn netlink_subscriber_observes_spawned_child_within_100ms() {
    let mut sub = match Subscriber::new() {
        Ok(s) => s,
        Err(NetlinkError::Io(e)) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            panic!(
                "Subscriber::new() failed with EPERM — this test requires CAP_NET_ADMIN. \
                 Run with `sudo cargo test --test integration_netlink_proc -- --ignored` or \
                 grant the cap on the test binary."
            );
        }
        Err(e) => panic!("Subscriber::new() failed: {e}"),
    };

    // Spawn a child whose pid we can recognise. `/bin/true` is universally
    // available, exits immediately, and produces exactly one EXEC event.
    let started = Instant::now();
    let mut child = Command::new("/bin/true").spawn().expect("spawn /bin/true");
    let target_pid = child.id();

    // Drain events for up to 1 s, looking for our child's exec.
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut saw_target = false;
    let mut first_seen_at: Option<Duration> = None;
    while Instant::now() < deadline {
        match sub.next_event() {
            Ok(Some(ProcEvent::Exec { pid, .. })) if pid == target_pid => {
                saw_target = true;
                first_seen_at = Some(started.elapsed());
                break;
            }
            Ok(Some(_)) | Ok(None) => continue,
            Err(e) => panic!("recv failed: {e}"),
        }
    }

    // Reap the child so it doesn't linger as a zombie. /bin/true exits
    // immediately, so wait() returns ~instantly.
    let _ = child.wait();

    assert!(
        saw_target,
        "did not observe our /bin/true child (pid {target_pid}) via netlink within 1 s"
    );
    let latency = first_seen_at.expect("set on success path");
    println!(
        "netlink saw pid {target_pid} after {} ms",
        latency.as_millis()
    );
    // Acceptance criterion: 100 ms. Add slack for CI flake (kernel
    // scheduler + spawn overhead). If this consistently fails on the
    // floor, raise the bound but investigate first.
    assert!(
        latency < Duration::from_millis(100),
        "netlink exec-detection latency {} ms exceeded the 100 ms acceptance criterion \
         from issue #40",
        latency.as_millis()
    );
}

/// Sanity check: the subscriber observes *some* event in 100 ms when
/// the host is doing anything at all (cron, systemd timers, login
/// shells, etc.). Less strict than the targeted child test — useful
/// as a quick smoke test that the netlink dance succeeded even when
/// running on a quiet test host.
#[test]
#[ignore]
fn netlink_subscriber_observes_at_least_one_event_within_100ms() {
    let mut sub = match Subscriber::new() {
        Ok(s) => s,
        Err(e) => panic!("Subscriber::new() failed (need CAP_NET_ADMIN): {e}"),
    };

    // Drive at least one event by spawning a quick child.
    let _ = Command::new("/bin/true").spawn().and_then(|mut c| c.wait());

    let deadline = Instant::now() + Duration::from_millis(100);
    while Instant::now() < deadline {
        match sub.next_event() {
            Ok(Some(_)) => return, // any event proves the subscription works
            Ok(None) => continue,
            Err(e) => panic!("recv failed: {e}"),
        }
    }
    panic!("no proc events observed in 100 ms — subscription may not be active");
}
