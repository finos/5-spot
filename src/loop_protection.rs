// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0
//! # Rapid-re-reclaim loop protection
//!
//! Detects the "user re-enables a `ScheduledMachine` whose conflicting
//! process is still running" failure mode (`5spot-emergency-reclaim-by-process-match.md`
//! Phase 4 follow-up). When the controller observes ≥
//! [`crate::constants::RAPID_RE_RECLAIM_THRESHOLD`] reclaim events for
//! the same SM within
//! [`crate::constants::RAPID_RE_RECLAIM_WINDOW_SECS`], it emits a
//! `RapidReReclaim` Warning Event and bumps
//! `fivespot_rapid_re_reclaims_total{namespace, name}`.
//!
//! ## Design — in-memory only, deliberately
//!
//! The tracker lives in [`crate::reconcilers::Context`] as a
//! `Mutex<HashMap<…, VecDeque<…>>>` rather than in CRD status. A
//! controller restart resets the count: "rapid" then means "rapid since
//! the most recent restart." This trades persistence for simplicity:
//!
//! - No CRD schema bump (which would require coordinated rollout in a
//!   regulated environment).
//! - No status-patch round trip on every reclaim.
//! - The signal is operator-actionable on a *trend* basis (Prometheus
//!   counter), so a single missed warning does not lose audit value.
//!
//! ## Pure helpers
//!
//! All decisions live in pure functions on `&mut VecDeque` so the
//! tests can drive every branch without a controller or a clock —
//! callers pass an explicit `now` and the helpers do the rest.

use crate::constants::{
    RAPID_RE_RECLAIM_MAX_TRACKED, RAPID_RE_RECLAIM_THRESHOLD, RAPID_RE_RECLAIM_WINDOW_SECS,
};
use chrono::{DateTime, Duration, Utc};
use std::collections::VecDeque;

/// Drop reclaim timestamps older than the configured window from the
/// front of the deque. Pure; no I/O.
///
/// Operates on the deque in place — caller passes the same deque
/// repeatedly across reconciles. Newer entries are at the back; once
/// the front is within the window, all subsequent entries are too
/// (timestamps are appended in monotonic order).
pub fn prune_old_reclaim_events(events: &mut VecDeque<DateTime<Utc>>, now: DateTime<Utc>) {
    let cutoff = now - Duration::seconds(RAPID_RE_RECLAIM_WINDOW_SECS);
    while events.front().is_some_and(|t| *t < cutoff) {
        events.pop_front();
    }
}

/// Append `now` to `events`, prune entries older than the window, and
/// cap the deque at [`RAPID_RE_RECLAIM_MAX_TRACKED`] (oldest dropped).
/// Returns `true` when the post-append count meets or exceeds
/// [`RAPID_RE_RECLAIM_THRESHOLD`] — the caller's signal to emit the
/// `RapidReReclaim` Warning Event and bump the metric.
pub fn record_emergency_reclaim_event(
    events: &mut VecDeque<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    prune_old_reclaim_events(events, now);
    events.push_back(now);
    while events.len() > RAPID_RE_RECLAIM_MAX_TRACKED {
        events.pop_front();
    }
    events.len() >= RAPID_RE_RECLAIM_THRESHOLD
}

/// Read-only check: would `events` (pruned to the window) constitute
/// a rapid-re-reclaim loop right now? Pure; used by tests and by
/// debug-log decisions that should not mutate the deque.
#[must_use]
pub fn should_warn_rapid_re_reclaim(events: &VecDeque<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    let cutoff = now - Duration::seconds(RAPID_RE_RECLAIM_WINDOW_SECS);
    events.iter().filter(|t| **t >= cutoff).count() >= RAPID_RE_RECLAIM_THRESHOLD
}

#[cfg(test)]
#[path = "loop_protection_tests.rs"]
mod tests;
