// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # Spot-schedule provider resolution (ADR 0006)
//!
//! Resolves a [`SpotScheduleRef`](crate::crd::SpotScheduleRef) on a
//! `ScheduledMachine` into a [`SpotScheduleVerdict`] by reading the referenced
//! provider object's **duck-typed `status`** on the management cluster. 5-Spot
//! reads only `status.active` (and a recommended `Ready` condition); it never
//! reads the provider `spec` and never writes the provider object.
//!
//! This is the **pull-on-reconcile** half (roadmap Phase 2): the resolver runs
//! during a normal reconcile. The event-driven dynamic watch that makes provider
//! transitions wake the `ScheduledMachine` lands in Phase 3.
//!
//! ## Resolution outcomes
//!
//! The resolver distinguishes *transient* failures (a Kubernetes API error mid
//! call → returned as `Err`, so the reconciler retries with back-off) from
//! *unresolved* states (the provider CRD is not installed, the object is absent,
//! it exposes no `status.active`, or its `Ready` condition is `False`). The
//! latter are **not** errors — they are [`SpotScheduleVerdict::Unresolved`]
//! verdicts that drive hold-last-state composition (ADR 0006 §4), never a panic.
//!
//! ## `Ready` semantics
//!
//! `status.conditions[type=Ready]` is **recommended, not required**. A provider
//! that omits it has its `status.active` taken as authoritative (resolved). A
//! provider that sets `Ready` to anything other than `True` is treated as
//! **unresolved** (`ProviderNotReady`) — that is how a provider explicitly says
//! "do not trust my `active` right now" without flapping the machine.

use kube::core::{DynamicObject, GroupVersionKind};
use kube::discovery::pinned_kind;
use kube::{Api, Client};
use serde_json::Value;

use crate::constants;
use crate::crd::SpotScheduleRef;
use crate::reconcilers::ReconcilerError;

/// The active/inactive verdict 5-Spot derives from a spot-schedule provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpotScheduleVerdict {
    /// Provider resolved and reports `status.active: true` (and is `Ready`).
    Active {
        /// The provider's `status.observedGeneration`, if present.
        provider_generation: Option<i64>,
    },
    /// Provider resolved and reports `status.active: false` (and is `Ready`).
    Inactive {
        /// The provider's `status.observedGeneration`, if present.
        provider_generation: Option<i64>,
    },
    /// The provider could not be authoritatively resolved. Drives
    /// hold-last-state composition rather than a machine teardown (ADR 0006 §4).
    Unresolved {
        /// Machine-readable reason (`constants::REASON_SPOT_SCHEDULE_*`).
        reason: &'static str,
        /// Human-readable detail for status/logs.
        message: String,
    },
}

impl SpotScheduleVerdict {
    /// `true` unless this is [`SpotScheduleVerdict::Unresolved`].
    #[must_use]
    pub fn is_resolved(&self) -> bool {
        !matches!(self, Self::Unresolved { .. })
    }

    /// The provider's active boolean when resolved; `None` when unresolved.
    #[must_use]
    pub fn active(&self) -> Option<bool> {
        match self {
            Self::Active { .. } => Some(true),
            Self::Inactive { .. } => Some(false),
            Self::Unresolved { .. } => None,
        }
    }

    /// The provider's `observedGeneration` when resolved; `None` otherwise.
    #[must_use]
    pub fn provider_generation(&self) -> Option<i64> {
        match self {
            Self::Active {
                provider_generation,
            }
            | Self::Inactive {
                provider_generation,
            } => *provider_generation,
            Self::Unresolved { .. } => None,
        }
    }

    /// The machine-readable reason for the current resolution state — a
    /// `constants::REASON_SPOT_SCHEDULE_*` value, including `Resolved` for the
    /// resolved variants.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Active { .. } | Self::Inactive { .. } => constants::REASON_SPOT_SCHEDULE_RESOLVED,
            Self::Unresolved { reason, .. } => reason,
        }
    }
}

/// Build an [`SpotScheduleVerdict::Unresolved`] with the given reason/message.
fn unresolved(reason: &'static str, message: impl Into<String>) -> SpotScheduleVerdict {
    SpotScheduleVerdict::Unresolved {
        reason,
        message: message.into(),
    }
}

/// Derive a verdict from a provider object's `status` value — **pure**, no I/O.
///
/// Object-level resolution (CRD installed, object present) is the caller's job
/// ([`resolve_spot_schedule`]); this covers the duck-typed `status` shape:
///
/// - `status` absent, or `status.active` absent / not a boolean →
///   [`SpotScheduleVerdict::Unresolved`] (`StatusActiveMissing`).
/// - a `Ready` condition present and not `"True"` → `Unresolved`
///   (`ProviderNotReady`).
/// - otherwise `Active` / `Inactive` per `status.active`, carrying
///   `status.observedGeneration`.
#[must_use]
pub fn verdict_from_status(status: Option<&Value>) -> SpotScheduleVerdict {
    let Some(status) = status else {
        return unresolved(
            constants::REASON_SPOT_SCHEDULE_STATUS_ACTIVE_MISSING,
            "provider status is absent",
        );
    };

    let Some(active) = status.get("active").and_then(Value::as_bool) else {
        return unresolved(
            constants::REASON_SPOT_SCHEDULE_STATUS_ACTIVE_MISSING,
            "provider status.active is absent or not a boolean",
        );
    };

    if ready_condition_blocks(status) {
        return unresolved(
            constants::REASON_SPOT_SCHEDULE_PROVIDER_NOT_READY,
            "provider Ready condition is not True",
        );
    }

    let provider_generation = status.get("observedGeneration").and_then(Value::as_i64);
    if active {
        SpotScheduleVerdict::Active {
            provider_generation,
        }
    } else {
        SpotScheduleVerdict::Inactive {
            provider_generation,
        }
    }
}

/// `true` if `status.conditions` contains a `Ready` entry whose `status` is not
/// `"True"`. An absent `Ready` condition returns `false` (the provider's
/// `active` is taken as authoritative — `Ready` is recommended, not required).
fn ready_condition_blocks(status: &Value) -> bool {
    let Some(conditions) = status.get("conditions").and_then(Value::as_array) else {
        return false;
    };
    conditions
        .iter()
        .find(|c| c.get("type").and_then(Value::as_str) == Some(constants::CONDITION_TYPE_READY))
        .is_some_and(|ready| {
            ready.get("status").and_then(Value::as_str) != Some(constants::CONDITION_STATUS_TRUE)
        })
}

/// Resolve a [`SpotScheduleRef`] into a [`SpotScheduleVerdict`] by reading the
/// provider object's status on the management cluster (in `namespace`).
///
/// # Errors
/// [`ReconcilerError::KubeError`] only for a *transient* API failure when
/// fetching the object (so the reconciler retries). A missing CRD, a missing
/// object, an absent `status.active`, or a non-`Ready` provider are returned as
/// `Ok(`[`SpotScheduleVerdict::Unresolved`]`)`, never an error.
pub async fn resolve_spot_schedule(
    client: &Client,
    namespace: &str,
    reference: &SpotScheduleRef,
) -> Result<SpotScheduleVerdict, ReconcilerError> {
    let (group, version) = reference
        .api_version
        .split_once('/')
        .unwrap_or(("", reference.api_version.as_str()));
    let gvk = GroupVersionKind::gvk(group, version, &reference.kind);

    // Discovery resolves the plural/namespaced capabilities for the kind. A
    // failure here means no CRD for this group/kind is installed.
    let api_resource = match pinned_kind(client, &gvk).await {
        Ok((api_resource, _capabilities)) => api_resource,
        Err(error) => {
            return Ok(unresolved(
                constants::REASON_SPOT_SCHEDULE_PROVIDER_CRD_NOT_INSTALLED,
                format!(
                    "no CRD for {}/{} kind {} is installed: {error}",
                    group, version, reference.kind
                ),
            ));
        }
    };

    let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), namespace, &api_resource);
    let object = match api.get_opt(&reference.name).await? {
        Some(object) => object,
        None => {
            return Ok(unresolved(
                constants::REASON_SPOT_SCHEDULE_PROVIDER_NOT_FOUND,
                format!(
                    "provider {} {}/{} not found",
                    reference.kind, namespace, reference.name
                ),
            ));
        }
    };

    Ok(verdict_from_status(object.data.get("status")))
}

#[cfg(test)]
#[path = "spot_schedule_tests.rs"]
mod tests;
