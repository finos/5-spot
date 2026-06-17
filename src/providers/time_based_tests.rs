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
}
