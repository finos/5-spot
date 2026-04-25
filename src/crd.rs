// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
//! # CRD type definitions
//!
//! This module is the **source of truth** for the `ScheduledMachine` custom resource.
//! The YAML files under `deploy/crds/` are **auto-generated** from these types by
//! `cargo run --bin crdgen` — never edit the YAML directly.
//!
//! ## Key types
//! - [`ScheduledMachineSpec`] / [`ScheduledMachine`] — the top-level CR
//! - [`ScheduleSpec`] — time-based schedule (days of week, hours, timezone)
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
    group = "5spot.finos.org",
    version = "v1alpha1",
    kind = "ScheduledMachine",
    namespaced,
    shortname = "sm",
    status = "ScheduledMachineStatus",
    printcolumn = r#"{"name":"Phase","type":"string","jsonPath":".status.phase"}"#,
    printcolumn = r#"{"name":"InSchedule","type":"boolean","jsonPath":".status.inSchedule"}"#,
    printcolumn = r#"{"name":"Enabled","type":"boolean","jsonPath":".spec.schedule.enabled"}"#,
    printcolumn = r#"{"name":"Schedule Days","type":"string","jsonPath":".spec.schedule.daysOfWeek"}"#,
    printcolumn = r#"{"name":"Schedule Hours","type":"string","jsonPath":".spec.schedule.hoursOfDay"}"#,
    printcolumn = r#"{"name":"KillSwitch","type":"boolean","jsonPath":".spec.killSwitch"}"#,
    printcolumn = r#"{"name":"Age","type":"date","jsonPath":".metadata.creationTimestamp"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledMachineSpec {
    /// Machine scheduling configuration
    pub schedule: ScheduleSpec,

    /// Name of the CAPI cluster this machine belongs to.
    ///
    /// Bounded to 63 characters — the RFC-1123 DNS label limit and the
    /// effective CAPI cluster-name cap, since the value flows downstream
    /// into the `cluster.x-k8s.io/cluster-name` label and into generated
    /// DNS labels. The schema also restricts the charset to ASCII
    /// alphanumerics, `-`, `.`, and `_` to block log-injection via
    /// embedded control characters and to bound Prometheus label
    /// cardinality.
    #[schemars(schema_with = "cluster_name_schema")]
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

    /// User-defined taints applied to the Kubernetes Node once it is Ready.
    ///
    /// The controller owns and reconciles only the taints it applied (tracked
    /// in `status.appliedNodeTaints` plus the `5spot.finos.org/applied-taints`
    /// annotation on the Node). Admin-added taints on the same Node are left
    /// untouched. A taint is identified by the tuple `(key, effect)`; `value`
    /// is mutable. Keys prefixed with `5spot.finos.org/`, `kubernetes.io/`,
    /// `node.kubernetes.io/`, or `node-role.kubernetes.io/` are rejected.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub node_taints: Vec<NodeTaint>,

    /// Optional list of process patterns that trigger an emergency node
    /// reclaim. When non-empty, the 5-Spot controller installs the
    /// `5spot-reclaim-agent` `DaemonSet` on every Node backing this
    /// `ScheduledMachine`; the agent watches `/proc` for any process whose
    /// basename or argv matches one of these patterns and, on first match,
    /// annotates the Node to request immediate (non-graceful) removal from
    /// the cluster. When absent or empty, no agent is installed and
    /// behaviour is time-based scheduling only.
    ///
    /// Patterns are evaluated against both `/proc/<pid>/comm` (exact
    /// basename) and `/proc/<pid>/cmdline` (substring). See the
    /// `5spot-emergency-reclaim-by-process-match.md` roadmap for full
    /// semantics.
    ///
    /// Bounded to 100 entries × 256 characters each. The caps guard the
    /// per-node agent's CPU (every pattern is evaluated against every
    /// `/proc/<pid>`) and cap the size of the per-node `ConfigMap`
    /// projection — an unbounded list is both an operator foot-gun and a
    /// denial-of-service vector when driven from a malicious CR.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[schemars(schema_with = "kill_if_commands_schema")]
    pub kill_if_commands: Option<Vec<String>>,
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
    /// Days of the week when machine should be active
    /// Supports: individual days (mon), ranges (mon-fri), combinations (mon-wed,fri-sun)
    #[serde(default)]
    pub days_of_week: Vec<String>,

    /// Hours when machine should be active (0-23)
    /// Supports: individual hours (9), ranges (9-17), combinations (0-9,18-23)
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
    /// Get the set of active weekday numbers (0=Monday, 6=Sunday)
    ///
    /// # Errors
    /// Returns error if `days_of_week` parsing fails
    pub fn get_active_weekdays(&self) -> Result<Option<HashSet<u8>>, String> {
        parse_day_ranges(&self.days_of_week).map(Some)
    }

    /// Get the set of active hours (0-23)
    ///
    /// # Errors
    /// Returns error if `hours_of_day` parsing fails
    pub fn get_active_hours(&self) -> Result<Option<HashSet<u8>>, String> {
        parse_hour_ranges(&self.hours_of_day).map(Some)
    }
}

// ============================================================================
// EmbeddedResource - Inline resource specification for CAPI resources
// ============================================================================

/// An embedded Kubernetes resource specification.
///
/// Used for inline bootstrap and infrastructure specs. This is intentionally
/// unstructured to support any provider type (`K0sWorkerConfig`, `KubeadmConfig`,
/// `RemoteMachine`, `AWSMachine`, etc.) without requiring schema knowledge.
///
/// Must contain at minimum `apiVersion` and `kind` fields. The controller
/// will extract these to create the appropriate dynamic resource.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[schemars(schema_with = "embedded_resource_schema")]
pub struct EmbeddedResource(pub Value);

impl EmbeddedResource {
    /// Get the apiVersion field from the embedded resource
    #[must_use]
    pub fn api_version(&self) -> Option<&str> {
        self.0.get("apiVersion").and_then(Value::as_str)
    }

    /// Get the kind field from the embedded resource
    #[must_use]
    pub fn kind(&self) -> Option<&str> {
        self.0.get("kind").and_then(Value::as_str)
    }

    /// Get the spec field from the embedded resource
    #[must_use]
    pub fn spec(&self) -> Option<&Value> {
        self.0.get("spec")
    }

    /// Get the inner JSON value
    #[must_use]
    pub fn inner(&self) -> &Value {
        &self.0
    }
}

/// Schema for the timezone field — bounded string to prevent log injection
fn timezone_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "maxLength": 64,
        "pattern": "^[A-Za-z][A-Za-z0-9_+\\-/]*$"
    })
}

/// Schema for `spec.clusterName` — bounded to the effective CAPI cluster-name
/// cap (RFC-1123 DNS label, 63 chars) with an ASCII-safe charset. Mirrors the
/// runtime check in `validate_cluster_name()` (src/reconcilers/helpers.rs).
fn cluster_name_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "minLength": 1,
        "maxLength": 63,
        "pattern": "^[A-Za-z0-9][A-Za-z0-9._-]*$"
    })
}

/// Schema for `spec.killIfCommands` — bounded list of bounded strings.
/// Mirrors the runtime check in `validate_kill_if_commands()`
/// (src/reconcilers/helpers.rs). 100 patterns × 256 chars is well above any
/// realistic workload and caps both reclaim-agent CPU cost and the per-node
/// `ConfigMap` projection size.
fn kill_if_commands_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "array",
        "maxItems": 100,
        "items": {
            "type": "string",
            "minLength": 1,
            "maxLength": 256
        }
    })
}

/// Schema for `EmbeddedResource` — requires apiVersion, kind, and spec fields.
/// The `spec` field uses `x-kubernetes-preserve-unknown-fields` to allow any
/// provider-specific fields (`K0sWorkerConfig`, `RemoteMachine`, `AWSMachine`, etc.).
fn embedded_resource_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "object",
        "required": ["apiVersion", "kind", "spec"],
        "properties": {
            "apiVersion": {
                "type": "string",
                "description": "API version of the resource (e.g., 'bootstrap.cluster.x-k8s.io/v1beta1')"
            },
            "kind": {
                "type": "string",
                "description": "Kind of the resource (e.g., 'K0sWorkerConfig', 'RemoteMachine')"
            },
            "spec": {
                "type": "object",
                "x-kubernetes-preserve-unknown-fields": true,
                "description": "Provider-specific configuration"
            }
        },
        "additionalProperties": false
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
// NodeTaint / TaintEffect - User-declared taints on the provisioned Node
// ============================================================================

/// A taint the controller applies to the Kubernetes Node once it is Ready.
///
/// Mirrors the shape of core/v1 `Taint`. Identity is the tuple `(key, effect)`;
/// `value` is mutable. See `ScheduledMachineSpec.node_taints` for ownership
/// semantics.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct NodeTaint {
    /// Taint key. Must be a qualified name (`[prefix/]name`), 1–63 chars on the
    /// name portion and matching `[a-z0-9A-Z]([-a-zA-Z0-9.]*[a-zA-Z0-9])?`.
    /// Reserved prefixes (`5spot.finos.org/`, `kubernetes.io/`,
    /// `node.kubernetes.io/`, `node-role.kubernetes.io/`) are rejected.
    pub key: String,

    /// Optional taint value (max 63 chars). Matches the same qualified-name
    /// pattern as `key`'s name portion when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,

    /// Taint effect — one of `NoSchedule`, `PreferNoSchedule`, `NoExecute`.
    pub effect: TaintEffect,
}

/// Taint effect — matches the three values defined by core/v1.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub enum TaintEffect {
    NoSchedule,
    PreferNoSchedule,
    NoExecute,
}

// ============================================================================
// ObjectReference / NodeRef - References to Kubernetes objects
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

/// Reference to a Kubernetes Node with apiVersion/kind/name and optional UID.
///
/// Mirrors the shape of `Machine.status.nodeRef` in CAPI, giving operators
/// enough identity to correlate a `ScheduledMachine` with a specific Node
/// object (UID protects against node-name reuse).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeRef {
    /// API version of the Node resource (typically `"v1"`)
    pub api_version: String,

    /// Kind of the referenced object (typically `"Node"`)
    pub kind: String,

    /// Name of the Node
    pub name: String,

    /// UID of the Node, protecting against name reuse
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
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

    /// Reference to the Kubernetes Node (once provisioned), mirroring the
    /// shape of CAPI's `Machine.status.nodeRef`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_ref: Option<NodeRef>,

    /// Provider-assigned machine identifier, copied from the CAPI
    /// `Machine.spec.providerID`. Stable for the life of the machine and
    /// unique across the cluster.
    #[serde(
        rename = "providerID",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub provider_id: Option<String>,

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

    /// Taints the controller has applied to the Node, in the order they were
    /// applied. Maintained as the controller's record of truth so subsequent
    /// reconciles only mutate taints we own — admin-added taints on the same
    /// Node whose `(key, effect)` collides with an entry here are surfaced as
    /// a `TaintOwnershipConflict` condition rather than overwritten.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub applied_node_taints: Vec<NodeTaint>,
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

// ============================================================================
// NodeTaint validation
// ============================================================================

/// Maximum length of the name portion of a taint key, and of a taint value.
const TAINT_NAME_MAX_LEN: usize = 63;

/// Maximum length of the optional prefix portion of a taint key (DNS subdomain).
const TAINT_PREFIX_MAX_LEN: usize = 253;

/// Our own reserved taint-key prefix — operators may not apply taints under it.
const RESERVED_TAINT_PREFIX_OWN: &str = "5spot.finos.org/";

/// Kubernetes-reserved taint-key prefixes. Rejected at the CR boundary so that
/// control-plane / kubelet signalling is never spoofed via `spec.nodeTaints`.
const RESERVED_K8S_TAINT_PREFIXES: &[&str] = &[
    "node.kubernetes.io/",
    "node-role.kubernetes.io/",
    "kubernetes.io/",
];

/// Validate a list of user-declared `NodeTaint` entries.
///
/// Rules:
/// - Each key is a qualified name (`[prefix/]name`) where the name portion
///   matches `[a-z0-9A-Z]([-a-zA-Z0-9.]*[a-zA-Z0-9])?` and is 1..=63 chars.
/// - If present, the value obeys the same pattern and is <=63 chars.
/// - `(key, effect)` pairs are unique; same key with different effects is OK.
/// - Reserved prefixes are rejected with a pointed error message.
///
/// # Errors
/// Returns a human-readable string describing the first offending taint — the
/// reconciler bubbles this up as a condition on the CR.
pub fn validate_node_taints(taints: &[NodeTaint]) -> Result<(), String> {
    let mut seen: HashSet<(String, TaintEffect)> = HashSet::new();
    for t in taints {
        validate_taint_key(&t.key)?;
        if let Some(v) = &t.value {
            validate_taint_value(v)?;
        }
        if !seen.insert((t.key.clone(), t.effect.clone())) {
            return Err(format!(
                "duplicate (key, effect) in spec.nodeTaints: ({}, {:?})",
                t.key, t.effect
            ));
        }
    }
    Ok(())
}

fn validate_taint_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("taint key must be non-empty".to_string());
    }
    if key.starts_with(RESERVED_TAINT_PREFIX_OWN) {
        return Err(format!(
            "taint key must not use reserved prefix {RESERVED_TAINT_PREFIX_OWN}: {key}"
        ));
    }
    for prefix in RESERVED_K8S_TAINT_PREFIXES {
        if key.starts_with(prefix) {
            return Err(format!(
                "taint key prefix {prefix} is reserved by Kubernetes; use spec.machineTemplate for control-plane role signalling, got: {key}"
            ));
        }
    }
    let (prefix_opt, name) = match key.split_once('/') {
        Some((p, n)) => (Some(p), n),
        None => (None, key),
    };
    if let Some(prefix) = prefix_opt {
        if prefix.is_empty() || prefix.len() > TAINT_PREFIX_MAX_LEN {
            return Err(format!(
                "taint key prefix must be 1..={TAINT_PREFIX_MAX_LEN} chars: {key}"
            ));
        }
        if !is_dns_subdomain(prefix) {
            return Err(format!("taint key prefix is not a DNS subdomain: {key}"));
        }
    }
    if name.is_empty() || name.len() > TAINT_NAME_MAX_LEN {
        return Err(format!(
            "taint key name portion must be 1..={TAINT_NAME_MAX_LEN} chars: {key}"
        ));
    }
    if !is_qualified_name(name) {
        return Err(format!(
            "taint key does not match qualified-name pattern: {key}"
        ));
    }
    Ok(())
}

fn validate_taint_value(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    if value.len() > TAINT_NAME_MAX_LEN {
        return Err(format!(
            "taint value must be 0..={TAINT_NAME_MAX_LEN} chars: {value}"
        ));
    }
    if !is_qualified_name(value) {
        return Err(format!(
            "taint value does not match qualified-name pattern: {value}"
        ));
    }
    Ok(())
}

/// `[a-z0-9A-Z]([-a-zA-Z0-9.]*[a-zA-Z0-9])?`
fn is_qualified_name(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    if !bytes[0].is_ascii_alphanumeric() {
        return false;
    }
    if bytes.len() == 1 {
        return true;
    }
    if !bytes[bytes.len() - 1].is_ascii_alphanumeric() {
        return false;
    }
    bytes[1..bytes.len() - 1]
        .iter()
        .all(|&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

fn is_dns_subdomain(s: &str) -> bool {
    if s.is_empty() || s.len() > TAINT_PREFIX_MAX_LEN {
        return false;
    }
    s.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= TAINT_NAME_MAX_LEN
            && label
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            && label
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric)
            && label
                .as_bytes()
                .iter()
                .all(|&b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

#[cfg(test)]
#[path = "crd_tests.rs"]
mod tests;
