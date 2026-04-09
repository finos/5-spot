// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: MIT
//! # CRD type definitions
//!
//! This module is the **source of truth** for the `ScheduledMachine` custom resource.
//! The YAML files under `deploy/crds/` are **auto-generated** from these types by
//! `cargo run --bin crdgen` — never edit the YAML directly.
//!
//! ## Key types
//! - [`ScheduledMachineSpec`] / [`ScheduledMachine`] — the top-level CR
//! - [`ScheduleSpec`] — time-based schedule (days of week, hours, timezone, or cron)
//! - [`EmbeddedResource`] — inline bootstrap or infrastructure provider spec
//! - [`ScheduledMachineStatus`] — runtime phase and condition tracking
//! - [`Condition`] — standard Kubernetes status condition

use chrono::Utc;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

// ============================================================================
// ScheduledMachine CRD
// ============================================================================

#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "5spot.io",
    version = "v1alpha1",
    kind = "ScheduledMachine",
    namespaced,
    shortname = "sm",
    status = "ScheduledMachineStatus",
    printcolumn = r#"{"name":"Phase","type":"string","jsonPath":".status.phase"}"#,
    printcolumn = r#"{"name":"InSchedule","type":"boolean","jsonPath":".status.inSchedule"}"#,
    printcolumn = r#"{"name":"Enabled","type":"boolean","jsonPath":".spec.schedule.enabled"}"#,
    printcolumn = r#"{"name":"KillSwitch","type":"boolean","jsonPath":".spec.killSwitch"}"#,
    printcolumn = r#"{"name":"Age","type":"date","jsonPath":".metadata.creationTimestamp"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledMachineSpec {
    /// Machine scheduling configuration
    pub schedule: ScheduleSpec,

    /// Name of the CAPI cluster this machine belongs to
    pub cluster_name: String,

    /// Inline bootstrap configuration spec (e.g., `K0sWorkerConfig`)
    /// This resource will be created when the schedule is active
    pub bootstrap_spec: EmbeddedResource,

    /// Inline infrastructure configuration spec (e.g., `RemoteMachine`)
    /// This resource will be created when the schedule is active
    pub infrastructure_spec: EmbeddedResource,

    /// Optional configuration for the created CAPI Machine
    /// If not specified, creates a Machine with default labels/annotations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_template: Option<MachineTemplateSpec>,

    /// Priority for machine scheduling (higher values = higher priority)
    #[serde(default = "default_priority")]
    pub priority: u8,

    /// Timeout for graceful machine shutdown (e.g., "5m", "10s")
    #[serde(default = "default_graceful_shutdown_timeout")]
    pub graceful_shutdown_timeout: String,

    /// Timeout for draining the node before deletion (e.g., "5m", "10m")
    #[serde(default = "default_node_drain_timeout")]
    pub node_drain_timeout: String,

    /// When true, immediately removes the machine from cluster
    #[serde(default)]
    pub kill_switch: bool,
}

fn default_priority() -> u8 {
    50
}

fn default_graceful_shutdown_timeout() -> String {
    "5m".to_string()
}

fn default_node_drain_timeout() -> String {
    "5m".to_string()
}

// ============================================================================
// ScheduleSpec - Time-based scheduling
// ============================================================================

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleSpec {
    /// Cron expression for the schedule (e.g., "0 9-17 * * 1-5" for Mon-Fri 9am-5pm)
    /// If specified, takes precedence over daysOfWeek/hoursOfDay
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,

    /// Days of the week when machine should be active
    /// Supports: individual days (mon), ranges (mon-fri), combinations (mon-wed,fri-sun)
    /// Ignored if cron is specified
    #[serde(default)]
    pub days_of_week: Vec<String>,

    /// Hours when machine should be active (0-23)
    /// Supports: individual hours (9), ranges (9-17), combinations (0-9,18-23)
    /// Ignored if cron is specified
    #[serde(default)]
    pub hours_of_day: Vec<String>,

    /// Timezone for the schedule (e.g., "UTC", "America/New\_York")
    /// Maximum length of 64 characters.
    #[serde(default = "default_timezone")]
    #[schemars(schema_with = "timezone_schema")]
    pub timezone: String,

    /// Whether the schedule is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_timezone() -> String {
    "UTC".to_string()
}

fn default_enabled() -> bool {
    true
}

impl ScheduleSpec {
    /// Check if this schedule uses cron expression
    #[must_use]
    pub fn uses_cron(&self) -> bool {
        self.cron.is_some()
    }

    /// Get the set of active weekday numbers (0=Monday, 6=Sunday)
    /// Returns None if using cron expression
    ///
    /// # Errors
    /// Returns error if `days_of_week` parsing fails
    pub fn get_active_weekdays(&self) -> Result<Option<HashSet<u8>>, String> {
        if self.uses_cron() {
            return Ok(None);
        }
        parse_day_ranges(&self.days_of_week).map(Some)
    }

    /// Get the set of active hours (0-23)
    /// Returns None if using cron expression
    ///
    /// # Errors
    /// Returns error if `hours_of_day` parsing fails
    pub fn get_active_hours(&self) -> Result<Option<HashSet<u8>>, String> {
        if self.uses_cron() {
            return Ok(None);
        }
        parse_hour_ranges(&self.hours_of_day).map(Some)
    }
}

// ============================================================================
// EmbeddedResource - Inline resource specification for CAPI resources
// ============================================================================

/// An embedded Kubernetes resource specification
/// Used for inline bootstrap and infrastructure specs
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedResource {
    /// API version of the resource (e.g., "bootstrap.cluster.x-k8s.io/v1beta1")
    pub api_version: String,

    /// Kind of the resource (e.g., `K0sWorkerConfig`, `RemoteMachine`)
    pub kind: String,

    /// The spec of the resource (provider-specific)
    /// This is an arbitrary JSON object whose schema depends on the kind
    #[schemars(schema_with = "arbitrary_object_schema")]
    pub spec: Value,
}

/// Schema for the timezone field — bounded string to prevent log injection
fn timezone_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "maxLength": 64,
        "pattern": "^[A-Za-z][A-Za-z0-9_+\\-/]*$"
    })
}

/// Schema for arbitrary JSON object
fn arbitrary_object_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "object",
        "additionalProperties": true,
        "x-kubernetes-preserve-unknown-fields": true
    })
}

/// Schema for `Condition.status` — enforces the Kubernetes condition status enum.
///
/// Only `"True"`, `"False"`, and `"Unknown"` are valid values per the
/// Kubernetes API conventions and NIST CM-5 configuration change control
/// requirements.
fn condition_status_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "enum": ["True", "False", "Unknown"]
    })
}

// ============================================================================
// MachineTemplateSpec - Optional configuration for created CAPI Machine
// ============================================================================

/// Optional configuration applied to the created CAPI Machine
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MachineTemplateSpec {
    /// Labels to apply to the created Machine
    #[serde(default)]
    pub labels: BTreeMap<String, String>,

    /// Annotations to apply to the created Machine
    #[serde(default)]
    pub annotations: BTreeMap<String, String>,
}

// ============================================================================
// LocalObjectReference - Reference to a resource in a namespace
// ============================================================================

/// Reference to a Kubernetes object with apiVersion, kind, name, namespace
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ObjectReference {
    /// API version of the referenced object
    pub api_version: String,

    /// Kind of the referenced object
    pub kind: String,

    /// Name of the referenced object
    pub name: String,

    /// Namespace of the referenced object
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

/// Simple reference to a resource by name only (same namespace assumed)
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalObjectReference {
    /// Name of the referenced object
    pub name: String,
}

// ============================================================================
// ScheduledMachineStatus - Runtime status
// ============================================================================

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledMachineStatus {
    /// Current phase of the machine lifecycle
    /// Values: Pending, Active, `ShuttingDown`, Inactive, Disabled, Terminated, Error
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,

    /// Human-readable status message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Last time a machine was created (RFC3339 format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_scheduled_time: Option<String>,

    /// Reference to the created CAPI Machine
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_ref: Option<ObjectReference>,

    /// Reference to the created bootstrap resource
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bootstrap_ref: Option<ObjectReference>,

    /// Reference to the created infrastructure resource
    #[serde(skip_serializing_if = "Option::is_none")]
    pub infrastructure_ref: Option<ObjectReference>,

    /// Reference to the Kubernetes Node (once provisioned)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_ref: Option<LocalObjectReference>,

    /// Standard Kubernetes conditions
    #[serde(default)]
    pub conditions: Vec<Condition>,

    /// Observed generation for change detection
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,

    /// Whether machine is currently in scheduled window
    #[serde(default)]
    pub in_schedule: bool,

    /// Next scheduled activation time (RFC3339 format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_activation: Option<String>,

    /// Time when machine will be cleaned up (RFC3339 format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cleanup: Option<String>,
}

// ============================================================================
// Condition - Status condition information
// ============================================================================

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    /// Type of condition (e.g., "Ready", "`MachineReady`", "`ReferencesValid`")
    pub r#type: String,

    /// Status: "True", "False", or "Unknown"
    #[schemars(schema_with = "condition_status_schema")]
    pub status: String,

    /// Last transition time (RFC3339 format)
    pub last_transition_time: String,

    /// One-word reason in CamelCase
    pub reason: String,

    /// Human-readable message
    pub message: String,
}

impl Condition {
    /// Create a new condition with current timestamp
    #[must_use]
    pub fn new(condition_type: &str, status: &str, reason: &str, message: &str) -> Self {
        Self {
            r#type: condition_type.to_string(),
            status: status.to_string(),
            last_transition_time: Utc::now().to_rfc3339(),
            reason: reason.to_string(),
            message: message.to_string(),
        }
    }
}

// ============================================================================
// Helper functions for parsing day and hour ranges
// ============================================================================

/// Parse day range specifications into a set of weekday numbers
/// Examples: `["mon-fri"]` -> `{0,1,2,3,4}`, `["mon-wed,fri-sun"]` -> `{0,1,2,4,5,6}`
///
/// # Errors
/// Returns error if day name is invalid or range specification is malformed
pub fn parse_day_ranges(day_specs: &[String]) -> Result<HashSet<u8>, String> {
    const DAY_MAPPING: &[(&str, u8)] = &[
        ("mon", 0),
        ("tue", 1),
        ("wed", 2),
        ("thu", 3),
        ("fri", 4),
        ("sat", 5),
        ("sun", 6),
    ];

    let mut result = HashSet::new();

    for spec in day_specs {
        for part in spec.split(',') {
            let part = part.trim();

            if part.contains('-') {
                // Handle range (e.g., "mon-fri")
                let parts: Vec<&str> = part.split('-').collect();
                if parts.len() != 2 {
                    return Err(format!("Invalid day range: {part}"));
                }

                let start_day = parts[0].trim();
                let end_day = parts[1].trim();

                let start_num = DAY_MAPPING
                    .iter()
                    .find(|(name, _)| *name == start_day)
                    .map(|(_, num)| *num)
                    .ok_or_else(|| format!("Invalid day: {start_day}"))?;

                let end_num = DAY_MAPPING
                    .iter()
                    .find(|(name, _)| *name == end_day)
                    .map(|(_, num)| *num)
                    .ok_or_else(|| format!("Invalid day: {end_day}"))?;

                // Handle wrapping (e.g., fri-mon)
                if start_num <= end_num {
                    for day in start_num..=end_num {
                        result.insert(day);
                    }
                } else {
                    // Wrap around the week
                    for day in start_num..=6 {
                        result.insert(day);
                    }
                    for day in 0..=end_num {
                        result.insert(day);
                    }
                }
            } else {
                // Handle single day
                let day_num = DAY_MAPPING
                    .iter()
                    .find(|(name, _)| *name == part)
                    .map(|(_, num)| *num)
                    .ok_or_else(|| format!("Invalid day: {part}"))?;
                result.insert(day_num);
            }
        }
    }

    Ok(result)
}

/// Parse hour range specifications into a set of hour numbers (0-23)
/// Examples: `["0-9"]` -> `{0..9}`, `["9-12,15-23"]` -> `{9,10,11,12,15..23}`
///
/// # Errors
/// Returns error if hour is out of range (0-23) or format is invalid
pub fn parse_hour_ranges(hour_specs: &[String]) -> Result<HashSet<u8>, String> {
    const MAX_HOUR: u8 = 23;
    let mut result = HashSet::new();

    for spec in hour_specs {
        for part in spec.split(',') {
            let part = part.trim();

            if part.contains('-') && !part.starts_with('-') {
                // Handle hour range (e.g., "9-17")
                let parts: Vec<&str> = part.split('-').collect();
                if parts.len() != 2 {
                    return Err(format!("Invalid hour range: {part}"));
                }

                let start_hour: u8 = parts[0]
                    .trim()
                    .parse()
                    .map_err(|_| format!("Invalid hour: {}", parts[0]))?;

                let end_hour: u8 = parts[1]
                    .trim()
                    .parse()
                    .map_err(|_| format!("Invalid hour: {}", parts[1]))?;

                if start_hour > MAX_HOUR || end_hour > MAX_HOUR {
                    return Err(format!("Hours must be 0-23, got: {part}"));
                }

                // Handle wrapping (e.g., 22-6 for overnight)
                if start_hour <= end_hour {
                    for hour in start_hour..=end_hour {
                        result.insert(hour);
                    }
                } else {
                    // Wrap around the day
                    for hour in start_hour..=MAX_HOUR {
                        result.insert(hour);
                    }
                    for hour in 0..=end_hour {
                        result.insert(hour);
                    }
                }
            } else {
                // Handle single hour
                let hour: u8 = part.parse().map_err(|_| format!("Invalid hour: {part}"))?;

                if hour > MAX_HOUR {
                    return Err(format!("Hour must be 0-23, got: {hour}"));
                }
                result.insert(hour);
            }
        }
    }

    Ok(result)
}

#[cfg(test)]
#[path = "crd_tests.rs"]
mod tests;
