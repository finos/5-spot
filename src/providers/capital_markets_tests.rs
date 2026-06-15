// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::crd::{CapitalMarketsScheduleSpec, EarlyClose, TradingSession};

    const NY: &str = "America/New_York";

    /// NYSE-like regular session: Mon–Fri, 09:00–15:00 local (hour 15 is the
    /// last active hour; the market closes at the end of 15:00).
    fn nyse_spec() -> CapitalMarketsScheduleSpec {
        CapitalMarketsScheduleSpec {
            timezone: NY.to_string(),
            sessions: vec![TradingSession {
                days_of_week: vec!["mon-fri".to_string()],
                hours_of_day: vec!["9-15".to_string()],
            }],
            holidays: vec!["2026-07-03".to_string()],
            early_closes: vec![EarlyClose {
                date: "2026-11-27".to_string(),
                close_hour: 12,
            }],
        }
    }

    // ── is_active_at ─────────────────────────────────────────────────────────

    #[test]
    fn test_active_midsession_weekday() {
        // Wed 2026-06-10 11:00 ET — in session.
        let t = local_instant(NY, 2026, 6, 10, 11, 0);
        assert!(is_active_at(&nyse_spec(), t).unwrap());
    }

    #[test]
    fn test_inactive_before_open() {
        let t = local_instant(NY, 2026, 6, 10, 8, 0);
        assert!(!is_active_at(&nyse_spec(), t).unwrap());
    }

    #[test]
    fn test_inactive_after_close() {
        // 16:00 ET — past the 09–15 window.
        let t = local_instant(NY, 2026, 6, 10, 16, 0);
        assert!(!is_active_at(&nyse_spec(), t).unwrap());
    }

    #[test]
    fn test_inactive_weekend() {
        // Sat 2026-06-13 11:00 ET.
        let t = local_instant(NY, 2026, 6, 13, 11, 0);
        assert!(!is_active_at(&nyse_spec(), t).unwrap());
    }

    #[test]
    fn test_inactive_on_holiday_even_during_session_hours() {
        // 2026-07-03 (Fri) 11:00 ET is a session hour but a holiday.
        let t = local_instant(NY, 2026, 7, 3, 11, 0);
        assert!(!is_active_at(&nyse_spec(), t).unwrap());
    }

    #[test]
    fn test_early_close_active_up_to_close_hour() {
        // 2026-11-27 (Fri) early close at 12: 12:00 still active…
        let t = local_instant(NY, 2026, 11, 27, 12, 0);
        assert!(is_active_at(&nyse_spec(), t).unwrap());
    }

    #[test]
    fn test_early_close_inactive_after_close_hour() {
        // …13:00 is closed (would be a normal session hour otherwise).
        let t = local_instant(NY, 2026, 11, 27, 13, 0);
        assert!(!is_active_at(&nyse_spec(), t).unwrap());
    }

    #[test]
    fn test_invalid_timezone_errors() {
        let mut spec = nyse_spec();
        spec.timezone = "Not/AZone".to_string();
        let t = local_instant(NY, 2026, 6, 10, 11, 0);
        assert!(matches!(
            is_active_at(&spec, t),
            Err(ProviderError::InvalidTimezone(_))
        ));
    }

    #[test]
    fn test_no_sessions_is_always_inactive() {
        let spec = CapitalMarketsScheduleSpec {
            timezone: NY.to_string(),
            sessions: vec![],
            holidays: vec![],
            early_closes: vec![],
        };
        let t = local_instant(NY, 2026, 6, 10, 11, 0);
        assert!(!is_active_at(&spec, t).unwrap());
    }

    // ── next_transition ──────────────────────────────────────────────────────

    #[test]
    fn test_next_transition_from_open_is_the_close_boundary() {
        // Wed 11:00 ET, open until end of 15:00 ⇒ next transition at 16:00 ET.
        let from = local_instant(NY, 2026, 6, 10, 11, 0);
        let next = next_transition(&nyse_spec(), from).unwrap().unwrap();
        assert_eq!(next, local_instant(NY, 2026, 6, 10, 16, 0));
    }

    #[test]
    fn test_next_transition_from_closed_is_the_open_boundary() {
        // Wed 08:00 ET, opens at 09:00 ⇒ next transition at 09:00 ET.
        let from = local_instant(NY, 2026, 6, 10, 8, 0);
        let next = next_transition(&nyse_spec(), from).unwrap().unwrap();
        assert_eq!(next, local_instant(NY, 2026, 6, 10, 9, 0));
    }

    #[test]
    fn test_next_transition_skips_weekend_to_monday_open() {
        // Fri 2026-06-12 16:00 ET (closed) ⇒ next open is Mon 2026-06-15 09:00.
        let from = local_instant(NY, 2026, 6, 12, 16, 0);
        let next = next_transition(&nyse_spec(), from).unwrap().unwrap();
        assert_eq!(next, local_instant(NY, 2026, 6, 15, 9, 0));
    }

    #[test]
    fn test_next_transition_none_when_no_sessions() {
        let spec = CapitalMarketsScheduleSpec {
            timezone: NY.to_string(),
            sessions: vec![],
            holidays: vec![],
            early_closes: vec![],
        };
        let from = local_instant(NY, 2026, 6, 10, 11, 0);
        assert!(next_transition(&spec, from).unwrap().is_none());
    }
}
