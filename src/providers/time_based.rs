// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # TimeBasedSpotSchedule provider controller (ADR 0009)
//!
//! The **core, first-party** spot-schedule provider: it reconciles
//! [`TimeBasedSpotSchedule`](crate::crd::TimeBasedSpotSchedule) objects and
//! publishes the duck-typed `status.active` that a
//! `ScheduledMachine.spec.schedule` consumes. It is the reified former inline
//! `spec.schedule`: a day-of-week / hour-of-day window evaluated in a configured
//! timezone, plus a provider-level `enabled` switch.
//!
//! ## Event-driven, single timed requeue (not a poll loop)
//!
//! Each reconcile computes the current `active` value *and* the **next window
//! boundary** ([`next_transition`]), then requeues exactly once at that boundary.
//! Spec edits arrive as watch events. There is no fixed polling interval —
//! mirroring [`crate::providers::capital_markets`] and the main controller.
//!
//! ## No network calls
//!
//! The window lives entirely in `spec` (declarative `daysOfWeek` / `hoursOfDay`
//! / `timezone`). Transition detection is hour-granular, so the requeue lands
//! within an hour of the true boundary for whole-hour-offset timezones; the
//! `active` value always self-corrects on the next reconcile.

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
    REASON_TIME_BASED_WINDOW_CLOSED, REASON_TIME_BASED_WINDOW_OPEN,
};
use crate::crd::{TimeBasedSpotSchedule, TimeBasedSpotScheduleSpec};

/// Errors raised while reconciling a `TimeBasedSpotSchedule`.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// `spec.timezone` is not a valid IANA timezone.
    #[error("invalid timezone: {0}")]
    InvalidTimezone(String),

    /// A `daysOfWeek` / `hoursOfDay` entry failed to parse.
    #[error("invalid schedule window: {0}")]
    Window(String),

    /// The object has no namespace (should be impossible for a namespaced CRD).
    #[error("TimeBasedSpotSchedule must be namespaced")]
    Unnamespaced,

    /// A Kubernetes API call failed.
    #[error("Kubernetes API error: {0}")]
    Kube(#[from] kube::Error),
}

/// Controller context — just the management-cluster client.
pub struct TimeBasedContext {
    /// Kubernetes client authenticated as the provider's ServiceAccount.
    pub client: Client,
}

/// Whether the schedule is **active** at `instant`, per its window (pure — no
/// I/O). Mirrors the former `evaluate_schedule`: a disabled provider is never
/// active; otherwise the instant's weekday **and** hour (in the configured
/// timezone) must both fall inside the declared sets. An empty `daysOfWeek` or
/// `hoursOfDay` set therefore yields inactive.
///
/// # Errors
/// [`ProviderError::InvalidTimezone`] or [`ProviderError::Window`].
pub fn is_active_at(
    spec: &TimeBasedSpotScheduleSpec,
    instant: DateTime<Utc>,
) -> Result<bool, ProviderError> {
    if !spec.enabled {
        return Ok(false);
    }

    let tz: Tz = spec
        .timezone
        .parse()
        .map_err(|_| ProviderError::InvalidTimezone(spec.timezone.clone()))?;
    let local = instant.with_timezone(&tz);

    let days = spec.get_active_weekdays().map_err(ProviderError::Window)?;
    let hours = spec.get_active_hours().map_err(ProviderError::Window)?;

    #[allow(clippy::cast_possible_truncation)]
    let weekday = local.weekday().num_days_from_monday() as u8;
    #[allow(clippy::cast_possible_truncation)]
    let hour = local.hour() as u8;

    Ok(days.is_some_and(|d| d.contains(&weekday)) && hours.is_some_and(|h| h.contains(&hour)))
}

/// The next UTC instant at which [`is_active_at`] flips, scanning hour-by-hour
/// from the hour after `from` up to [`PROVIDER_TRANSITION_HORIZON_HOURS`].
/// Returns `None` if no transition occurs within the horizon (e.g. an empty or
/// disabled schedule) — the caller then uses a fallback requeue. Pure — no I/O.
///
/// # Errors
/// Propagates [`is_active_at`] errors.
pub fn next_transition(
    spec: &TimeBasedSpotScheduleSpec,
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

/// Reconcile one `TimeBasedSpotSchedule`: compute `active` + the next
/// transition, patch `status`, and requeue at the boundary.
///
/// # Errors
/// Returns [`ProviderError`] on window/timezone parse failure or a status patch
/// API error; the [`error_policy`] requeues with back-off.
pub async fn reconcile(
    tbss: Arc<TimeBasedSpotSchedule>,
    ctx: Arc<TimeBasedContext>,
) -> Result<Action, ProviderError> {
    let now = Utc::now();
    let namespace = tbss.namespace().ok_or(ProviderError::Unnamespaced)?;
    let name = tbss.name_any();

    let active = is_active_at(&tbss.spec, now)?;
    let next = next_transition(&tbss.spec, now)?;

    let previous_active = tbss.status.as_ref().map(|status| status.active);
    let transitioned = previous_active != Some(active);
    let last_transition_time = if transitioned {
        Some(now.to_rfc3339())
    } else {
        tbss.status
            .as_ref()
            .and_then(|status| status.last_transition_time.clone())
    };

    let (reason, message) = if active {
        (REASON_TIME_BASED_WINDOW_OPEN, "schedule window open")
    } else {
        (REASON_TIME_BASED_WINDOW_CLOSED, "schedule window closed")
    };

    let mut status = json!({
        "active": active,
        "observedGeneration": tbss.metadata.generation,
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

    let api: Api<TimeBasedSpotSchedule> = Api::namespaced(ctx.client.clone(), &namespace);
    api.patch_status(
        &name,
        &PatchParams::default(),
        &Patch::Merge(&json!({ "status": status })),
    )
    .await?;

    crate::metrics::set_time_based_active(&namespace, &name, active);
    if transitioned {
        crate::metrics::record_time_based_transition(&namespace, &name);
    }

    info!(
        provider = %name,
        namespace = %namespace,
        active,
        next_transition = ?next,
        "TimeBasedSpotSchedule reconciled"
    );

    // Single timed requeue at the next window boundary (or a bounded fallback
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
    tbss: Arc<TimeBasedSpotSchedule>,
    error: &ProviderError,
    _ctx: Arc<TimeBasedContext>,
) -> Action {
    warn!(
        provider = %tbss.name_any(),
        error = %error,
        "TimeBasedSpotSchedule reconcile error; requeuing"
    );
    Action::requeue(Duration::from_secs(PROVIDER_ERROR_REQUEUE_SECS))
}

/// Run the provider controller until shutdown signal. Builds a `Controller`
/// over all `TimeBasedSpotSchedule` objects.
///
/// # Errors
/// Returns an error only if the controller stream itself fails to start.
pub async fn run(client: Client) -> anyhow::Result<()> {
    let api = Api::<TimeBasedSpotSchedule>::all(client.clone());
    let ctx = Arc::new(TimeBasedContext { client });

    info!("Starting TimeBasedSpotSchedule provider controller");
    Controller::new(api, watcher::Config::default())
        .shutdown_on_signal()
        .run(reconcile, error_policy, ctx)
        .for_each(|res| async move {
            if let Err(e) = res {
                warn!(error = %e, "TimeBasedSpotSchedule reconcile loop error");
            }
        })
        .await;
    info!("TimeBasedSpotSchedule provider controller shut down");
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
#[path = "time_based_tests.rs"]
mod tests;
