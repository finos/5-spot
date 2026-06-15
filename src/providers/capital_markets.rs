// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # CapitalMarketsSchedule provider controller (ADR 0006, Phase 5)
//!
//! The **reference** spot-schedule provider: it reconciles
//! [`CapitalMarketsSchedule`](crate::crd::CapitalMarketsSchedule) objects and
//! publishes the duck-typed `status.active` that a
//! `ScheduledMachine.spec.spotSchedule` consumes. It computes activity from a
//! declarative **exchange calendar** — trading sessions, statutory holidays, and
//! early-close days — evaluated in the schedule's configured timezone.
//!
//! ## Event-driven, single timed requeue (not a poll loop)
//!
//! Each reconcile computes the current `active` value *and* the **next calendar
//! transition** ([`next_transition`]), then requeues exactly once at that
//! boundary. Spec edits arrive as watch events (the `Controller` watches the
//! CRD). There is no fixed polling interval — the only timer is the requeue
//! scheduled at the next session/holiday boundary, mirroring how the main 5-Spot
//! controller is event-driven.
//!
//! ## No network calls
//!
//! The calendar lives entirely in `spec` (declarative `sessions` / `holidays` /
//! `earlyCloses`). Operators sync real exchange calendars into the `spec` via
//! GitOps; this controller makes no outbound calls. Transition detection is
//! **hour-granular** (sessions are expressed in whole hours), so the requeue
//! lands within an hour of the true boundary for whole-hour-offset exchange
//! timezones (NYSE, LSE, TSE, TSX); the `active` value always self-corrects on
//! the next reconcile.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Datelike, Timelike, Utc};
use chrono_tz::Tz;
use futures::StreamExt;
use kube::api::{Api, Patch, PatchParams};
use kube::runtime::controller::Action;
use kube::runtime::{watcher, Controller};
use kube::{Client, ResourceExt};
use serde_json::json;
use tracing::{info, warn};

use crate::constants::{
    CONDITION_STATUS_TRUE, CONDITION_TYPE_READY, PROVIDER_ERROR_REQUEUE_SECS,
    PROVIDER_FALLBACK_REQUEUE_SECS, PROVIDER_TRANSITION_HORIZON_HOURS,
    REASON_CAPITAL_MARKETS_SESSION_CLOSED, REASON_CAPITAL_MARKETS_SESSION_OPEN,
};
use crate::crd::{
    parse_day_ranges, parse_hour_ranges, CapitalMarketsSchedule, CapitalMarketsScheduleSpec,
};

/// Errors raised while reconciling a `CapitalMarketsSchedule`.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// `spec.timezone` is not a valid IANA timezone.
    #[error("invalid timezone: {0}")]
    InvalidTimezone(String),

    /// A `sessions[*].daysOfWeek` / `hoursOfDay` entry failed to parse.
    #[error("invalid session calendar: {0}")]
    Calendar(String),

    /// The object has no namespace (should be impossible for a namespaced CRD).
    #[error("CapitalMarketsSchedule must be namespaced")]
    Unnamespaced,

    /// A Kubernetes API call failed.
    #[error("Kubernetes API error: {0}")]
    Kube(#[from] kube::Error),
}

/// Controller context — just the management-cluster client.
pub struct CapitalMarketsContext {
    /// Kubernetes client authenticated as the provider's ServiceAccount.
    pub client: Client,
}

/// Whether the market is **in session** at `instant`, per the schedule's
/// calendar (pure — no I/O).
///
/// Order of evaluation: a holiday closes the whole day; otherwise the instant
/// must fall inside some `sessions[*]` window (day **and** hour); an early-close
/// override then closes the market after `closeHour` on that date.
///
/// # Errors
/// [`ProviderError::InvalidTimezone`] or [`ProviderError::Calendar`].
pub fn is_active_at(
    spec: &CapitalMarketsScheduleSpec,
    instant: DateTime<Utc>,
) -> Result<bool, ProviderError> {
    let tz: Tz = spec
        .timezone
        .parse()
        .map_err(|_| ProviderError::InvalidTimezone(spec.timezone.clone()))?;
    let local = instant.with_timezone(&tz);
    let date = local.format("%Y-%m-%d").to_string();

    // A statutory holiday closes the market for the whole day.
    if spec.holidays.iter().any(|holiday| holiday == &date) {
        return Ok(false);
    }

    #[allow(clippy::cast_possible_truncation)]
    let weekday = local.weekday().num_days_from_monday() as u8;
    #[allow(clippy::cast_possible_truncation)]
    let hour = local.hour() as u8;

    let mut in_session = false;
    for session in &spec.sessions {
        let days = parse_day_ranges(&session.days_of_week).map_err(ProviderError::Calendar)?;
        let hours = parse_hour_ranges(&session.hours_of_day).map_err(ProviderError::Calendar)?;
        if days.contains(&weekday) && hours.contains(&hour) {
            in_session = true;
            break;
        }
    }
    if !in_session {
        return Ok(false);
    }

    // Early close: the market shuts at the END of `closeHour` on that date, so
    // any hour strictly after it is closed even though the normal session covers
    // it.
    if let Some(early_close) = spec.early_closes.iter().find(|ec| ec.date == date) {
        if hour > early_close.close_hour {
            return Ok(false);
        }
    }

    Ok(true)
}

/// The next UTC instant at which [`is_active_at`] flips, scanning hour-by-hour
/// from the hour after `from` up to [`PROVIDER_TRANSITION_HORIZON_HOURS`].
/// Returns `None` if no transition occurs within the horizon (e.g. a schedule
/// with no sessions, or a long holiday stretch) — the caller then uses a
/// fallback requeue. Pure — no I/O.
///
/// # Errors
/// Propagates [`is_active_at`] errors.
pub fn next_transition(
    spec: &CapitalMarketsScheduleSpec,
    from: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, ProviderError> {
    let current = is_active_at(spec, from)?;

    // Start at the next whole UTC hour boundary.
    let mut candidate = from
        .with_minute(0)
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0))
        .unwrap_or(from)
        + Duration::from_secs(3600);

    for _ in 0..PROVIDER_TRANSITION_HORIZON_HOURS {
        if is_active_at(spec, candidate)? != current {
            return Ok(Some(candidate));
        }
        candidate += Duration::from_secs(3600);
    }
    Ok(None)
}

/// Reconcile one `CapitalMarketsSchedule`: compute `active` + the next
/// transition, patch `status`, and requeue at the boundary.
///
/// # Errors
/// Returns [`ProviderError`] on calendar/timezone parse failure or a status
/// patch API error; the [`error_policy`] requeues with back-off.
pub async fn reconcile(
    cms: Arc<CapitalMarketsSchedule>,
    ctx: Arc<CapitalMarketsContext>,
) -> Result<Action, ProviderError> {
    let now = Utc::now();
    let namespace = cms.namespace().ok_or(ProviderError::Unnamespaced)?;
    let name = cms.name_any();

    let active = is_active_at(&cms.spec, now)?;
    let next = next_transition(&cms.spec, now)?;

    let previous_active = cms.status.as_ref().map(|status| status.active);
    let transitioned = previous_active != Some(active);
    let last_transition_time = if transitioned {
        Some(now.to_rfc3339())
    } else {
        cms.status
            .as_ref()
            .and_then(|status| status.last_transition_time.clone())
    };

    let (reason, message) = if active {
        (REASON_CAPITAL_MARKETS_SESSION_OPEN, "market session open")
    } else {
        (
            REASON_CAPITAL_MARKETS_SESSION_CLOSED,
            "market session closed",
        )
    };

    let mut status = json!({
        "active": active,
        "observedGeneration": cms.metadata.generation,
        "conditions": [{
            "type": CONDITION_TYPE_READY,
            "status": CONDITION_STATUS_TRUE,
            "reason": reason,
            "message": message,
            "lastTransitionTime": now.to_rfc3339(),
        }],
    });
    if let Some(time) = last_transition_time {
        status["lastTransitionTime"] = json!(time);
    }
    if let Some(next) = next {
        status["nextTransitionTime"] = json!(next.to_rfc3339());
    }

    let api: Api<CapitalMarketsSchedule> = Api::namespaced(ctx.client.clone(), &namespace);
    api.patch_status(
        &name,
        &PatchParams::default(),
        &Patch::Merge(&json!({ "status": status })),
    )
    .await?;

    crate::metrics::set_capital_markets_active(&namespace, &name, active);
    if transitioned {
        crate::metrics::record_capital_markets_transition(&namespace, &name);
    }

    info!(
        provider = %name,
        namespace = %namespace,
        active,
        next_transition = ?next,
        "CapitalMarketsSchedule reconciled"
    );

    // Single timed requeue at the next calendar boundary (or a bounded fallback
    // when none is found within the horizon). Not a polling interval.
    let requeue = match next {
        Some(next) => (next - now)
            .to_std()
            .unwrap_or(Duration::from_secs(PROVIDER_FALLBACK_REQUEUE_SECS)),
        None => Duration::from_secs(PROVIDER_FALLBACK_REQUEUE_SECS),
    };
    Ok(Action::requeue(requeue))
}

/// Controller error policy — log and requeue with a fixed back-off.
#[must_use]
pub fn error_policy(
    cms: Arc<CapitalMarketsSchedule>,
    error: &ProviderError,
    _ctx: Arc<CapitalMarketsContext>,
) -> Action {
    warn!(
        provider = %cms.name_any(),
        error = %error,
        "CapitalMarketsSchedule reconcile error; requeuing"
    );
    Action::requeue(Duration::from_secs(PROVIDER_ERROR_REQUEUE_SECS))
}

/// Run the provider controller until shutdown signal. Builds a `Controller`
/// over all `CapitalMarketsSchedule` objects.
///
/// # Errors
/// Returns an error only if the controller stream itself fails to start.
pub async fn run(client: Client) -> anyhow::Result<()> {
    let api = Api::<CapitalMarketsSchedule>::all(client.clone());
    let ctx = Arc::new(CapitalMarketsContext { client });

    info!("Starting CapitalMarketsSchedule provider controller");
    Controller::new(api, watcher::Config::default())
        .shutdown_on_signal()
        .run(reconcile, error_policy, ctx)
        .for_each(|res| async move {
            if let Err(e) = res {
                warn!(error = %e, "CapitalMarketsSchedule reconcile loop error");
            }
        })
        .await;
    info!("CapitalMarketsSchedule provider controller shut down");
    Ok(())
}

/// Build a `DateTime<Utc>` from y/m/d h:m in a named timezone. Test helper kept
/// here so both the module and its tests share one construction path.
#[cfg(test)]
#[must_use]
pub(crate) fn local_instant(tz: &str, y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
    use chrono::TimeZone;
    let zone: Tz = tz.parse().expect("valid tz");
    zone.with_ymd_and_hms(y, mo, d, h, mi, 0)
        .single()
        .expect("valid local time")
        .with_timezone(&Utc)
}

#[cfg(test)]
#[path = "capital_markets_tests.rs"]
mod tests;
