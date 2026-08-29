// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::crd::TimeBasedSpotScheduleSpec;

    const NY: &str = "America/New_York";

    /// Weekdays 09:00–17:00 local (hour 17 is the last active hour).
    fn weekday_spec() -> TimeBasedSpotScheduleSpec {
        TimeBasedSpotScheduleSpec {
            days_of_week: vec!["mon-fri".to_string()],
            hours_of_day: vec!["9-17".to_string()],
            timezone: NY.to_string(),
            enabled: true,
        }
    }

    // ── is_active_at ─────────────────────────────────────────────────────────

    #[test]
    fn test_active_midwindow_weekday() {
        // Wed 2026-06-10 11:00 ET — inside the window.
        let t = local_instant(NY, 2026, 6, 10, 11, 0);
        assert!(is_active_at(&weekday_spec(), t).unwrap());
    }

    #[test]
    fn test_inactive_before_window() {
        let t = local_instant(NY, 2026, 6, 10, 8, 0);
        assert!(!is_active_at(&weekday_spec(), t).unwrap());
    }

    #[test]
    fn test_inactive_after_window() {
        let t = local_instant(NY, 2026, 6, 10, 18, 0);
        assert!(!is_active_at(&weekday_spec(), t).unwrap());
    }

    #[test]
    fn test_inactive_weekend() {
        // Sat 2026-06-13 11:00 ET.
        let t = local_instant(NY, 2026, 6, 13, 11, 0);
        assert!(!is_active_at(&weekday_spec(), t).unwrap());
    }

    #[test]
    fn test_disabled_is_never_active_even_in_window() {
        let mut spec = weekday_spec();
        spec.enabled = false;
        let t = local_instant(NY, 2026, 6, 10, 11, 0);
        assert!(!is_active_at(&spec, t).unwrap());
    }

    #[test]
    fn test_empty_days_is_inactive() {
        let spec = TimeBasedSpotScheduleSpec {
            days_of_week: vec![],
            hours_of_day: vec!["9-17".to_string()],
            timezone: NY.to_string(),
            enabled: true,
        };
        let t = local_instant(NY, 2026, 6, 10, 11, 0);
        assert!(!is_active_at(&spec, t).unwrap());
    }

    #[test]
    fn test_empty_hours_is_inactive() {
        let spec = TimeBasedSpotScheduleSpec {
            days_of_week: vec!["mon-fri".to_string()],
            hours_of_day: vec![],
            timezone: NY.to_string(),
            enabled: true,
        };
        let t = local_instant(NY, 2026, 6, 10, 11, 0);
        assert!(!is_active_at(&spec, t).unwrap());
    }

    #[test]
    fn test_timezone_is_honoured() {
        // 10:00 UTC on Wed is 06:00 EDT (before the NY 9–17 window) but inside a
        // UTC 9–17 window — same instant, opposite verdicts depending on tz.
        let spec_utc = TimeBasedSpotScheduleSpec {
            days_of_week: vec!["mon-fri".to_string()],
            hours_of_day: vec!["9-17".to_string()],
            timezone: "UTC".to_string(),
            enabled: true,
        };
        let t = local_instant("UTC", 2026, 6, 10, 10, 0);
        assert!(is_active_at(&spec_utc, t).unwrap());
        assert!(!is_active_at(&weekday_spec(), t).unwrap());
    }

    #[test]
    fn test_invalid_timezone_errors() {
        let mut spec = weekday_spec();
        spec.timezone = "Not/AZone".to_string();
        let t = local_instant(NY, 2026, 6, 10, 11, 0);
        assert!(matches!(
            is_active_at(&spec, t),
            Err(ProviderError::InvalidTimezone(_))
        ));
    }

    #[test]
    fn test_invalid_window_errors() {
        let mut spec = weekday_spec();
        spec.days_of_week = vec!["funday".to_string()];
        let t = local_instant(NY, 2026, 6, 10, 11, 0);
        assert!(matches!(
            is_active_at(&spec, t),
            Err(ProviderError::Window(_))
        ));
    }

    // ── next_transition ──────────────────────────────────────────────────────

    #[test]
    fn test_next_transition_from_inside_window_is_the_close() {
        // Wed 11:00 ET, window closes at end of 17:00 → next flip at 18:00 ET.
        let from = local_instant(NY, 2026, 6, 10, 11, 0);
        let next = next_transition(&weekday_spec(), from).unwrap().unwrap();
        assert_eq!(next, local_instant(NY, 2026, 6, 10, 18, 0));
    }

    #[test]
    fn test_next_transition_from_before_open_is_the_open() {
        // Wed 08:00 ET → opens at 09:00 ET.
        let from = local_instant(NY, 2026, 6, 10, 8, 0);
        let next = next_transition(&weekday_spec(), from).unwrap().unwrap();
        assert_eq!(next, local_instant(NY, 2026, 6, 10, 9, 0));
    }

    #[test]
    fn test_next_transition_none_when_never_active() {
        // Disabled never flips within the horizon.
        let mut spec = weekday_spec();
        spec.enabled = false;
        let from = local_instant(NY, 2026, 6, 10, 8, 0);
        assert!(next_transition(&spec, from).unwrap().is_none());
    }

    // ── compute_status ───────────────────────────────────────────────────────
    //
    // Regression coverage for the hot-loop bug: the `Ready` condition's own
    // `lastTransitionTime` used to be set to `now` on every reconcile
    // regardless of whether anything transitioned, so the computed status
    // never equalled the stored one and `patch_status` always wrote a change
    // — which re-triggered the watch and reconciled again immediately.

    fn status_with(active: bool, last_transition_time: &str) -> TimeBasedSpotScheduleStatus {
        TimeBasedSpotScheduleStatus {
            active,
            last_transition_time: Some(last_transition_time.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn test_compute_status_no_transition_keeps_previous_timestamp() {
        // Already active, still active at `now` — nothing transitioned, so
        // both lastTransitionTime fields must keep the *old* timestamp, not
        // be rewritten to `now`.
        let previous = status_with(true, "2026-01-01T00:00:00+00:00");
        let now = local_instant(NY, 2026, 6, 10, 11, 0);

        let status = compute_status(&weekday_spec(), Some(&previous), Some(1), now).unwrap();

        assert_eq!(status["lastTransitionTime"], "2026-01-01T00:00:00+00:00");
        assert_eq!(
            status["conditions"][0]["lastTransitionTime"],
            "2026-01-01T00:00:00+00:00"
        );
    }

    #[test]
    fn test_compute_status_is_idempotent_across_repeated_reconciles() {
        // The literal hot-loop regression test: feeding compute_status's own
        // output back in as `previous` (as a real reconcile loop would, one
        // resourceVersion apart) must produce byte-for-byte identical output
        // when nothing about the world has changed between calls.
        let now = local_instant(NY, 2026, 6, 10, 11, 0);
        let first = compute_status(&weekday_spec(), None, Some(1), now).unwrap();

        let previous = TimeBasedSpotScheduleStatus {
            active: first["active"].as_bool().unwrap(),
            last_transition_time: first["lastTransitionTime"].as_str().map(String::from),
            ..Default::default()
        };
        // A later reconcile, still inside the same window.
        let later = now + Duration::from_secs(5);
        let second = compute_status(&weekday_spec(), Some(&previous), Some(1), later).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn test_compute_status_transition_sets_now() {
        // Was inactive, now active — a real transition, so both
        // lastTransitionTime fields must move to `now`.
        let previous = status_with(false, "2026-01-01T00:00:00+00:00");
        let now = local_instant(NY, 2026, 6, 10, 11, 0);

        let status = compute_status(&weekday_spec(), Some(&previous), Some(1), now).unwrap();

        let expected = now.to_rfc3339();
        assert_eq!(status["lastTransitionTime"], expected);
        assert_eq!(status["conditions"][0]["lastTransitionTime"], expected);
    }

    #[test]
    fn test_compute_status_first_reconcile_with_no_previous_status_sets_now() {
        let now = local_instant(NY, 2026, 6, 10, 11, 0);

        let status = compute_status(&weekday_spec(), None, Some(1), now).unwrap();

        let expected = now.to_rfc3339();
        assert_eq!(status["lastTransitionTime"], expected);
        assert_eq!(status["conditions"][0]["lastTransitionTime"], expected);
    }
}
