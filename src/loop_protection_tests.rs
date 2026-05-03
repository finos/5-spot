// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0
//! Tests for the rapid-re-reclaim loop-protection helpers.

#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::constants::{RAPID_RE_RECLAIM_MAX_TRACKED, RAPID_RE_RECLAIM_WINDOW_SECS};
    use chrono::{Duration, TimeZone, Utc};
    use std::collections::VecDeque;

    fn t(secs: i64) -> chrono::DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).single().expect("valid ts")
    }

    // ─────────────────────────────────────────────────────────────────
    // prune_old_reclaim_events
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn prune_empty_is_noop() {
        let mut events: VecDeque<chrono::DateTime<Utc>> = VecDeque::new();
        prune_old_reclaim_events(&mut events, t(1_000_000));
        assert!(events.is_empty());
    }

    #[test]
    fn prune_keeps_all_events_within_window() {
        let now = t(1_000_000);
        let mut events: VecDeque<_> = [
            now - Duration::seconds(1),
            now - Duration::seconds(60),
            now - Duration::seconds(RAPID_RE_RECLAIM_WINDOW_SECS - 1),
        ]
        .into_iter()
        .collect();
        prune_old_reclaim_events(&mut events, now);
        assert_eq!(events.len(), 3, "all entries within window must survive");
    }

    #[test]
    fn prune_drops_all_when_every_entry_is_older() {
        let now = t(1_000_000);
        let mut events: VecDeque<_> = [
            now - Duration::seconds(RAPID_RE_RECLAIM_WINDOW_SECS + 1),
            now - Duration::seconds(RAPID_RE_RECLAIM_WINDOW_SECS + 60),
        ]
        .into_iter()
        .collect();
        prune_old_reclaim_events(&mut events, now);
        assert!(events.is_empty(), "all stale entries must be dropped");
    }

    #[test]
    fn prune_drops_only_stale_front_entries() {
        let now = t(1_000_000);
        let mut events: VecDeque<_> = [
            now - Duration::seconds(RAPID_RE_RECLAIM_WINDOW_SECS + 100), // stale
            now - Duration::seconds(RAPID_RE_RECLAIM_WINDOW_SECS + 1),   // stale
            now - Duration::seconds(60),                                 // fresh
            now - Duration::seconds(1),                                  // fresh
        ]
        .into_iter()
        .collect();
        prune_old_reclaim_events(&mut events, now);
        assert_eq!(events.len(), 2);
    }

    // ─────────────────────────────────────────────────────────────────
    // record_emergency_reclaim_event
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn record_first_event_does_not_trigger_warning() {
        let mut events = VecDeque::new();
        let breach = record_emergency_reclaim_event(&mut events, t(1_000_000));
        assert!(!breach, "single event must not breach threshold of 3");
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn record_two_events_does_not_trigger_warning() {
        let mut events = VecDeque::new();
        record_emergency_reclaim_event(&mut events, t(1_000_000));
        let breach = record_emergency_reclaim_event(&mut events, t(1_000_001));
        assert!(!breach, "two events must not breach threshold of 3");
    }

    #[test]
    fn record_three_events_within_window_triggers_warning() {
        let mut events = VecDeque::new();
        let now = 1_000_000;
        record_emergency_reclaim_event(&mut events, t(now));
        record_emergency_reclaim_event(&mut events, t(now + 10));
        let breach = record_emergency_reclaim_event(&mut events, t(now + 20));
        assert!(breach, "third event within window must breach");
    }

    #[test]
    fn record_three_events_spread_outside_window_does_not_trigger() {
        let mut events = VecDeque::new();
        let base = 1_000_000;
        record_emergency_reclaim_event(&mut events, t(base));
        record_emergency_reclaim_event(&mut events, t(base + RAPID_RE_RECLAIM_WINDOW_SECS + 1));
        // First event is now stale and pruned; only two remain after
        // the second push. Third push has count = 2 (one stale, one
        // fresh, plus this one = 2 fresh).
        let breach = record_emergency_reclaim_event(
            &mut events,
            t(base + 2 * (RAPID_RE_RECLAIM_WINDOW_SECS + 1)),
        );
        assert!(
            !breach,
            "events outside window must be pruned and not count toward threshold"
        );
    }

    #[test]
    fn record_caps_at_max_tracked_dropping_oldest() {
        let mut events = VecDeque::new();
        let base = 1_000_000;
        // Push more than the cap.
        for i in 0..(RAPID_RE_RECLAIM_MAX_TRACKED + 5) {
            // Use sub-second offsets via integer seconds (chrono timestamps are
            // 1-second granular here). Increment by 1 to keep ordering monotone.
            #[allow(clippy::cast_possible_wrap)]
            record_emergency_reclaim_event(&mut events, t(base + i as i64));
        }
        assert_eq!(
            events.len(),
            RAPID_RE_RECLAIM_MAX_TRACKED,
            "deque must be capped at RAPID_RE_RECLAIM_MAX_TRACKED entries"
        );
        // The oldest survivor must be the (5)th push since we dropped 5 from the front.
        let oldest = *events.front().unwrap();
        let expected_oldest = t(base + 5);
        assert_eq!(oldest, expected_oldest);
    }

    // ─────────────────────────────────────────────────────────────────
    // should_warn_rapid_re_reclaim
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn should_warn_returns_false_for_empty_deque() {
        let events = VecDeque::new();
        assert!(!should_warn_rapid_re_reclaim(&events, t(1_000_000)));
    }

    #[test]
    fn should_warn_returns_false_below_threshold() {
        let now = t(1_000_000);
        let events: VecDeque<_> = [now - Duration::seconds(1), now - Duration::seconds(10)]
            .into_iter()
            .collect();
        assert!(!should_warn_rapid_re_reclaim(&events, now));
    }

    #[test]
    fn should_warn_returns_true_at_threshold() {
        let now = t(1_000_000);
        let events: VecDeque<_> = [
            now - Duration::seconds(1),
            now - Duration::seconds(10),
            now - Duration::seconds(20),
        ]
        .into_iter()
        .collect();
        assert!(should_warn_rapid_re_reclaim(&events, now));
    }

    #[test]
    fn should_warn_only_counts_events_within_window() {
        let now = t(1_000_000);
        let events: VecDeque<_> = [
            now - Duration::seconds(RAPID_RE_RECLAIM_WINDOW_SECS + 100), // stale
            now - Duration::seconds(60),                                 // fresh
            now - Duration::seconds(1),                                  // fresh
        ]
        .into_iter()
        .collect();
        // 2 within window, threshold is 3 — no warn.
        assert!(!should_warn_rapid_re_reclaim(&events, now));
    }

    #[test]
    fn should_warn_does_not_mutate_input() {
        let now = t(1_000_000);
        let events: VecDeque<_> = [
            now - Duration::seconds(RAPID_RE_RECLAIM_WINDOW_SECS + 100),
            now - Duration::seconds(60),
            now - Duration::seconds(1),
        ]
        .into_iter()
        .collect();
        let len_before = events.len();
        let _ = should_warn_rapid_re_reclaim(&events, now);
        assert_eq!(
            events.len(),
            len_before,
            "should_warn must take &VecDeque, not mutate"
        );
    }
}
