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
    version = "v1beta1",
    kind = "ScheduledMachine",
    namespaced,
    shortname = "sm",
    status = "ScheduledMachineStatus",
    printcolumn = r#"{"name":"Phase","type":"string","jsonPath":".status.phase"}"#,
    printcolumn = r#"{"name":"Ready","type":"boolean","jsonPath":".status.ready"}"#,
    printcolumn = r#"{"name":"InSchedule","type":"boolean","jsonPath":".status.inSchedule"}"#,
    printcolumn = r#"{"name":"Enabled","type":"boolean","jsonPath":".spec.schedule.enabled"}"#,
    printcolumn = r#"{"name":"Schedule Days","type":"string","jsonPath":".spec.schedule.daysOfWeek"}"#,
    printcolumn = r#"{"name":"Schedule Hours","type":"string","jsonPath":".spec.schedule.hoursOfDay"}"#,
    printcolumn = r#"{"name":"SpotSchedule","type":"string","jsonPath":".spec.spotSchedule.kind"}"#,
    printcolumn = r#"{"name":"KillSwitch","type":"boolean","jsonPath":".spec.killSwitch"}"#,
    printcolumn = r#"{"name":"Age","type":"date","jsonPath":".metadata.creationTimestamp"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledMachineSpec {
    /// Inline time-based scheduling configuration (days of week, hours,
    /// timezone, enabled). **Optional since `v1beta1`** (ADR 0006): a machine
    /// may instead delegate its active/inactive decision to an external
    /// provider via [`spot_schedule`](Self::spot_schedule). At least one of
    /// `schedule` / `spotSchedule` must be set (CEL-enforced). When both are
    /// set the machine is active only when the time window **and** the provider
    /// both agree (logical AND); `killSwitch` always overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<ScheduleSpec>,

    /// Reference to an external spot-schedule provider resource that owns the
    /// active/inactive decision for this machine (ADR 0006). The referenced
    /// object must live in **this `ScheduledMachine`'s namespace**, and its
    /// `apiVersion` group must be `spotschedules.5spot.finos.org`
    /// (CEL-enforced). 5-Spot reads only the provider's duck-typed
    /// `status.active` (and `Ready` condition); it never reads the provider
    /// `spec` and never writes the provider object. Composed with `schedule`
    /// via logical AND when both are present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spot_schedule: Option<SpotScheduleRef>,

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

    /// Reference to a Secret in this ScheduledMachine's namespace containing
    /// a kubeconfig for the workload (child) cluster whose Node(s) this
    /// resource manages.
    ///
    /// When set, every Node and Pod API call the controller makes on behalf
    /// of this resource — cordon, taint, drain (pod list + delete),
    /// reclaim-agent annotations / labels / ConfigMaps, status enrichment —
    /// is routed through that kubeconfig. CAPI / bootstrap / infrastructure
    /// / Machine objects continue to use the management cluster's in-cluster
    /// client.
    ///
    /// When unset (default), the controller first tries to auto-discover a
    /// Secret named `<spec.clusterName>-kubeconfig` in this same namespace
    /// (CAPI convention). If that Secret does not exist, the management
    /// client is used for Node/Pod operations as well — the degenerate
    /// single-cluster dev/test posture where management ≡ workload cluster.
    ///
    /// Cross-namespace Secret references are NOT supported: the Secret MUST
    /// live in this resource's own namespace. This is a security boundary
    /// (cross-namespace would let a tenant in one namespace read a kubeconfig
    /// in another).
    ///
    /// The supplied kubeconfig MUST grant: `nodes` get/list/watch/patch and
    /// `pods` get/list/delete in all namespaces of the child cluster, plus
    /// `configmaps` get/create/patch/delete in the reclaim-agent namespace
    /// if `killIfCommands` is also set. See
    /// `docs/src/concepts/child-cluster-kubeconfig.md` for the full RBAC
    /// requirements and threat model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kubeconfig_secret_ref: Option<KubeconfigSecretRef>,

    /// Optional reference to a `Secret` or `ConfigMap` on the **workload
    /// cluster** holding a Kata containerd drop-in to deliver to the node(s) this
    /// resource owns.
    ///
    /// When set, the controller resolves the referenced object on the workload
    /// cluster (via the `kubeconfig-<clusterName>` Secret) in `kata.namespace`
    /// (default `5spot-system`). If it is present, the controller stamps the
    /// `5spot.finos.org/kata-config=enabled` opt-in label **and** a reference
    /// annotation on the backing Node; the `5spot-kata-config-agent` DaemonSet —
    /// scheduled onto labelled nodes — reads the object from the workload API,
    /// writes the drop-in to the fixed host path
    /// `/etc/k0s/containerd.d/kata.toml` (not configurable — ADR 0005), and
    /// restarts `restartService` so
    /// containerd reloads it. If the object (or its namespace) is absent, the
    /// controller does **not** label the Node and reports a fail-fast status
    /// condition — 5-Spot never creates the object (it must pre-exist,
    /// Flux-delivered).
    ///
    /// This is config *delivery*, not a Kata install: the `/opt/kata` binaries
    /// remain `kata-deploy`'s responsibility, and the existing
    /// `katacontainers.io/kata-runtime` opt-in label is unaffected. See ADR 0002
    /// and ADR 0003.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kata: Option<KataConfig>,
}

impl ScheduledMachineSpec {
    /// Whether this machine's activation is enabled at all.
    ///
    /// A machine with an inline `spec.schedule` honours `schedule.enabled` (the
    /// operator's master on/off). A `spotSchedule`-only machine has no such
    /// flag and is always "enabled" — its active/inactive decision is governed
    /// entirely by the provider, folded into the composed should-be-active
    /// verdict (ADR 0006 §3). This is the gate the lifecycle phase handlers use
    /// to decide whether a machine is administratively disabled.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        match &self.schedule {
            Some(schedule) => schedule.enabled,
            None => true,
        }
    }
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
    /// An inactive placeholder schedule (`enabled: false`, no windows) used as
    /// the effective schedule for a `spotSchedule`-only `ScheduledMachine`
    /// (one with no inline `spec.schedule`). `enabled: false` makes the
    /// time-based evaluator return "not active", so a provider-only machine is
    /// inert until the spot-schedule resolver composes the provider verdict.
    #[must_use]
    pub fn inactive_placeholder() -> Self {
        Self {
            days_of_week: Vec::new(),
            hours_of_day: Vec::new(),
            timezone: default_timezone(),
            enabled: false,
        }
    }

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
// SpotScheduleRef - Reference to an external spot-schedule provider resource
// ============================================================================

/// Reference to an external spot-schedule provider resource (ADR 0006) that
/// owns the active/inactive decision for a [`ScheduledMachine`].
///
/// The referenced object MUST live in the **same namespace** as the
/// `ScheduledMachine` (there is no `namespace` field by design — cross-namespace
/// references are a deliberate non-goal). Its `apiVersion` group MUST be
/// `spotschedules.5spot.finos.org`; any served version is accepted (resolution
/// keys off group + kind, never a pinned version — ADR 0007). The group pin is
/// enforced both by the schema's `x-kubernetes-validations` CEL rule (see
/// [`spot_schedule_api_version_schema`]) and at runtime.
///
/// 5-Spot reads only the provider's duck-typed `status.active` (and `Ready`
/// condition); it never reads the provider `spec` and never writes the provider
/// object. `deny_unknown_fields` mirrors [`KubeconfigSecretRef`]: a typo must be
/// a hard error, not a silent miss.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpotScheduleRef {
    /// API version of the provider resource, `group/version`. The group MUST be
    /// `spotschedules.5spot.finos.org` (CEL-pinned); e.g.
    /// `spotschedules.5spot.finos.org/v1alpha1`.
    #[schemars(schema_with = "spot_schedule_api_version_schema")]
    pub api_version: String,

    /// Kind of the provider resource, e.g. `CapitalMarketsSchedule`.
    #[schemars(schema_with = "spot_schedule_kind_schema")]
    pub kind: String,

    /// Name of the provider object in **this `ScheduledMachine`'s namespace**.
    /// RFC-1123 DNS subdomain (max 253 chars).
    #[schemars(schema_with = "spot_schedule_name_schema")]
    pub name: String,
}

impl SpotScheduleRef {
    /// Extract the API group (the portion of `api_version` before the `/`).
    ///
    /// Returns the whole string when there is no `/` (a degenerate value the
    /// schema's CEL rule already rejects at admission).
    #[must_use]
    pub fn group(&self) -> &str {
        self.api_version
            .split_once('/')
            .map_or(self.api_version.as_str(), |(group, _)| group)
    }

    /// `true` if the reference targets the spot-schedule provider group.
    #[must_use]
    pub fn is_spot_schedule_group(&self) -> bool {
        self.group() == crate::constants::SPOT_SCHEDULE_API_GROUP
    }
}

/// Schema for `SpotScheduleRef.apiVersion` — a bounded `group/version` string
/// whose group is pinned to `spotschedules.5spot.finos.org` via a CEL
/// `x-kubernetes-validations` rule (ADR 0006). Any version segment is accepted
/// (ADR 0007 — resolution is version-agnostic).
fn spot_schedule_api_version_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "minLength": 1,
        "maxLength": 253,
        "pattern": "^spotschedules\\.5spot\\.finos\\.org/[A-Za-z0-9][A-Za-z0-9.-]*$",
        "x-kubernetes-validations": [
            {
                "rule": "self.startsWith('spotschedules.5spot.finos.org/')",
                "message": "spec.spotSchedule.apiVersion group must be spotschedules.5spot.finos.org"
            }
        ]
    })
}

/// Schema for `SpotScheduleRef.kind` — a bounded Kubernetes Kind (PascalCase
/// identifier), e.g. `CapitalMarketsSchedule`.
fn spot_schedule_kind_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "minLength": 1,
        "maxLength": 63,
        "pattern": "^[A-Z][A-Za-z0-9]*$"
    })
}

/// Schema for `SpotScheduleRef.name` — RFC-1123 DNS subdomain (max 253 chars),
/// the object name on the management cluster in the SM's own namespace.
fn spot_schedule_name_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "minLength": 1,
        "maxLength": 253,
        "pattern": "^[a-z0-9]([-a-z0-9]*[a-z0-9])?(\\.[a-z0-9]([-a-z0-9]*[a-z0-9])?)*$"
    })
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
///
/// # Security — pass-through trust boundary
///
/// **The `spec` field of an `EmbeddedResource` is forwarded unchanged
/// to the named provider.** The 5-Spot reconciler validates the
/// envelope (`apiVersion` group allowlist, presence of `kind`, etc.)
/// but **does not inspect the inner spec**. That is by design —
/// 5-Spot is provider-agnostic and cannot ship a schema for every
/// possible CAPI provider.
///
/// This means the trust boundary is the provider, not 5-Spot:
///
/// - `k0smotron.io/K0sWorkerConfig.spec.cloudInit` is interpreted as
///   cloud-init YAML and executed verbatim on the provisioned VM.
/// - `k0smotron.io/RemoteMachine.spec.address` is an SSH endpoint
///   reached by the infrastructure controller.
/// - Other providers carry their own code-execution / network-reach
///   surfaces in their inline specs.
///
/// In multi-tenant clusters where different teams can `create
/// scheduledmachines` in their own namespaces, operators **MUST**
/// either pre-stage approved provider specs (out of scope for
/// v1alpha1) or layer a complementary `ValidatingAdmissionPolicy`
/// that inspects the provider payload — the 5-Spot VAP only
/// validates structure. See `docs/src/concepts/scheduled-machine.md`
/// (section "Security: Provider payload pass-through") for the full
/// trade-off discussion.
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

    /// Get `metadata.namespace` if the user set one.
    ///
    /// This is **controller-owned** and must never be honoured — the controller
    /// always creates the resource in the `ScheduledMachine`'s own namespace.
    /// Used by `validate_embedded_metadata` to reject the field loudly.
    #[must_use]
    pub fn metadata_namespace(&self) -> Option<&str> {
        self.0
            .get("metadata")
            .and_then(|m| m.get("namespace"))
            .and_then(Value::as_str)
    }

    /// Get `metadata.name` if the user set one.
    ///
    /// Controller-owned: the created resource is always named after the
    /// `ScheduledMachine` (deletion relies on this), so a user-supplied name is
    /// rejected rather than silently overridden.
    #[must_use]
    pub fn metadata_name(&self) -> Option<&str> {
        self.0
            .get("metadata")
            .and_then(|m| m.get("name"))
            .and_then(Value::as_str)
    }

    /// Get `metadata.labels` as a string map (empty if absent or malformed).
    ///
    /// Non-string label values are skipped — the CRD schema already constrains
    /// values to strings, so this is a defensive narrowing for raw input.
    #[must_use]
    pub fn metadata_labels(&self) -> BTreeMap<String, String> {
        embedded_string_map(self.0.get("metadata").and_then(|m| m.get("labels")))
    }

    /// Get `metadata.annotations` as a string map (empty if absent or malformed).
    #[must_use]
    pub fn metadata_annotations(&self) -> BTreeMap<String, String> {
        embedded_string_map(self.0.get("metadata").and_then(|m| m.get("annotations")))
    }

    /// Get the inner JSON value
    #[must_use]
    pub fn inner(&self) -> &Value {
        &self.0
    }
}

/// Narrow an optional JSON value to a `BTreeMap<String, String>`, keeping only
/// string-valued entries. Returns an empty map for `None` or non-object values.
fn embedded_string_map(value: Option<&Value>) -> BTreeMap<String, String> {
    value
        .and_then(Value::as_object)
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
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

/// Schema for `KubeconfigSecretRef.name` — bounded to RFC-1123 DNS subdomain
/// length (253 chars) with the Kubernetes-standard charset. Matches the bound
/// the Kubernetes API server itself enforces on Secret names, so a value that
/// fits this schema is guaranteed to be a syntactically valid Secret name.
fn kubeconfig_secret_name_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "minLength": 1,
        "maxLength": 253,
        "pattern": "^[a-z0-9]([-a-z0-9]*[a-z0-9])?(\\.[a-z0-9]([-a-z0-9]*[a-z0-9])?)*$"
    })
}

/// Schema for `KubeconfigSecretRef.key` — bounded to the same 253-char cap as
/// the name, with the Secret-data key charset (alphanumerics, `.`, `-`, `_`).
/// The bound prevents pathologically long keys from inflating the Secret GET
/// path; the charset matches what the Kubernetes API server enforces on Secret
/// `data` keys.
fn kubeconfig_secret_key_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "minLength": 1,
        "maxLength": 253,
        "pattern": "^[A-Za-z0-9._-]+$"
    })
}

/// Default key for `KubeconfigSecretRef.key` — CAPI convention is to store the
/// kubeconfig YAML under `data.value` in a Secret named `<clusterName>-kubeconfig`.
fn default_kubeconfig_secret_key() -> String {
    "value".to_string()
}

/// Schema for `KataConfig.name` — bounded to RFC-1123 DNS subdomain length
/// (253 chars) with the Kubernetes-standard charset. The same rule governs both
/// `ConfigMap` and `Secret` names, so one schema covers both source kinds; a
/// value that fits is guaranteed to be a syntactically valid object name.
fn kata_config_name_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "minLength": 1,
        "maxLength": 253,
        "pattern": "^[a-z0-9]([-a-z0-9]*[a-z0-9])?(\\.[a-z0-9]([-a-z0-9]*[a-z0-9])?)*$"
    })
}

/// Schema for `KataConfig.key` — the ConfigMap/Secret `data` key holding the
/// drop-in content. Bounded to the 253-char data-key cap with the charset the
/// Kubernetes API server enforces on `data` keys.
fn kata_config_key_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "minLength": 1,
        "maxLength": 253,
        "pattern": "^[A-Za-z0-9._-]+$"
    })
}

/// Schema for `KataConfig.restartService` — the systemd unit the agent
/// restarts via `nsenter`. Constrained to a `*.service` unit name with the
/// systemd unit charset and bounded to systemd's 255-char unit-name cap.
fn kata_config_restart_service_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "minLength": 1,
        "maxLength": 255,
        "pattern": "^[A-Za-z0-9@._-]+\\.service$"
    })
}

/// Schema for `KataConfig.namespace` — the workload-cluster namespace the agent
/// reads the source object from. RFC-1123 label (max 63 chars) with the
/// Kubernetes namespace charset.
fn kata_config_namespace_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "minLength": 1,
        "maxLength": 63,
        "pattern": "^[a-z0-9]([-a-z0-9]*[a-z0-9])?$"
    })
}

/// Default workload-cluster namespace for `KataConfig.namespace` — the agent's
/// own namespace, so it reads from its namespace with no cross-namespace RBAC.
fn default_kata_config_namespace() -> String {
    crate::constants::KATA_CONFIG_NAMESPACE.to_string()
}

/// Default `data` key for `KataConfig.key` — the containerd drop-in filename.
fn default_kata_config_key() -> String {
    "kata-containers.toml".to_string()
}

/// Default systemd unit for `KataConfig.restartService` — the k0s worker
/// service. Single-node / controller-also-runs-workloads layouts override this
/// with `k0scontroller.service`.
fn default_kata_config_restart_service() -> String {
    "k0sworker.service".to_string()
}

/// Schema for `EmbeddedResource` — requires apiVersion, kind, and spec fields.
/// The `spec` field uses `x-kubernetes-preserve-unknown-fields` to allow any
/// provider-specific fields (`K0sWorkerConfig`, `RemoteMachine`, `AWSMachine`, etc.).
///
/// # `metadata` — labels/annotations only; name/namespace are controller-owned
///
/// `metadata` accepts **only** `labels` and `annotations` (string maps), which
/// the controller merges onto the created resource (after applying the
/// reserved-prefix allowlist, so users cannot forge `cluster.x-k8s.io/*` or
/// `5spot.finos.org/*` keys). `metadata.name` and `metadata.namespace` are
/// **not** valid — the controller owns the resource identity, always naming the
/// resource after the `ScheduledMachine` and creating it in the SM's own
/// namespace.
///
/// `metadata` is marked `x-kubernetes-preserve-unknown-fields: true` **on
/// purpose**: without it the API server would silently *prune* an unknown
/// `metadata.namespace`/`metadata.name` before any admission policy runs,
/// making a loud rejection impossible. Preserving the subtree lets the
/// `ValidatingAdmissionPolicy` (and the runtime `validate_embedded_metadata`
/// check) see and explicitly reject those fields. The pruned-vs-rejected
/// distinction is a documented CRD behaviour — see
/// <https://kubernetes.io/docs/tasks/extend-kubernetes/custom-resources/custom-resource-definitions/#field-pruning>.
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
            "metadata": {
                "type": "object",
                "x-kubernetes-preserve-unknown-fields": true,
                "description": "Optional labels/annotations to stamp on the created resource. Only 'labels' and 'annotations' are honoured; 'name' and 'namespace' are controller-owned and rejected at admission.",
                "properties": {
                    "labels": {
                        "type": "object",
                        "additionalProperties": { "type": "string" },
                        "description": "Labels merged onto the created resource (reserved prefixes are rejected)"
                    },
                    "annotations": {
                        "type": "object",
                        "additionalProperties": { "type": "string" },
                        "description": "Annotations merged onto the created resource (reserved prefixes are rejected)"
                    }
                }
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
// KubeconfigSecretRef - Reference to a child-cluster kubeconfig Secret
// ============================================================================

/// Pointer to a Secret in the ScheduledMachine's own namespace whose data
/// contains a kubeconfig for the workload (child) cluster.
///
/// See [`ScheduledMachineSpec::kubeconfig_secret_ref`] for the full semantics,
/// including resolution order (explicit → auto-discover `<clusterName>-kubeconfig`
/// → management fallback) and the RBAC requirements on the supplied kubeconfig.
///
/// `deny_unknown_fields` is intentional: a typo like `nameSpace` or an attempt
/// to add a cross-namespace `namespace` field must be a hard error, not a
/// silent miss. Cross-namespace Secret refs are forbidden by design.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KubeconfigSecretRef {
    /// Name of the Secret. RFC-1123 DNS subdomain (max 253 chars).
    /// The Secret MUST live in the same namespace as the ScheduledMachine —
    /// there is no `namespace` field by design.
    #[schemars(schema_with = "kubeconfig_secret_name_schema")]
    pub name: String,

    /// Key within the Secret's `data` map whose value is the kubeconfig YAML
    /// document. Defaults to `value` — CAPI's convention for
    /// `<clusterName>-kubeconfig` Secrets generated by the control-plane
    /// provider. Common overrides: `kubeconfig` (some k0smotron flows),
    /// `admin.conf` (kubeadm-style).
    #[serde(default = "default_kubeconfig_secret_key")]
    #[schemars(schema_with = "kubeconfig_secret_key_schema")]
    pub key: String,
}

// ============================================================================
// KataConfig - Reference to a Kata containerd drop-in source on the workload cluster
// ============================================================================

/// Source kind for a [`KataConfig`] — the drop-in content lives in either a
/// `ConfigMap` or a `Secret`. Variants serialize verbatim (`ConfigMap`,
/// `Secret`) so they line up with the Kubernetes object kinds and the
/// `KIND_CONFIG_MAP` / `KIND_SECRET` constants.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub enum KataConfigSourceKind {
    /// Drop-in content is stored in a `ConfigMap` on the workload cluster.
    ConfigMap,
    /// Drop-in content is stored in a `Secret` on the workload cluster.
    Secret,
}

/// Pointer to a `Secret` or `ConfigMap` on the **workload cluster** whose data
/// holds a Kata containerd drop-in to deliver to the node(s) this resource owns.
///
/// See [`ScheduledMachineSpec::kata`] for the full semantics (workload-cluster
/// resolution, opt-in label + reference annotation, host write, and k0s
/// restart). Decisions: ADR 0002 (contract + resolution) and ADR 0003 (host
/// write + `nsenter` restart).
///
/// `deny_unknown_fields` is intentional and mirrors [`KubeconfigSecretRef`]: a
/// typo must be a hard error, not a silent miss. The object is resolved on the
/// workload cluster in `namespace` (default `5spot-system`); 5-Spot **never
/// creates it** — it must pre-exist (Flux-delivered), or delivery fails fast
/// with a status condition.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KataConfig {
    /// Whether the source is a `ConfigMap` or a `Secret`. Required (no default).
    pub kind: KataConfigSourceKind,

    /// Name of the source object on the workload cluster. RFC-1123 DNS subdomain
    /// (max 253 chars).
    #[schemars(schema_with = "kata_config_name_schema")]
    pub name: String,

    /// Workload-cluster namespace the agent reads the object from. Defaults to
    /// `5spot-system` (the agent's own namespace, so no cross-namespace agent
    /// RBAC is needed). Override to place config in a per-tenant namespace.
    #[serde(default = "default_kata_config_namespace")]
    #[schemars(schema_with = "kata_config_namespace_schema")]
    pub namespace: String,

    /// Key within the source's `data` map whose value is the drop-in content.
    /// Defaults to `kata-containers.toml`.
    #[serde(default = "default_kata_config_key")]
    #[schemars(schema_with = "kata_config_key_schema")]
    pub key: String,

    /// systemd unit the node agent restarts (via `nsenter`) so containerd
    /// reloads the drop-in. Defaults to `k0sworker.service`; override with
    /// `k0scontroller.service` on single-node / controller-runs-workloads
    /// layouts.
    #[serde(default = "default_kata_config_restart_service")]
    #[schemars(schema_with = "kata_config_restart_service_schema")]
    pub restart_service: String,
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

    /// True only when the machine has reached the `Active` phase. Surfaced as
    /// the `Ready` printer column for fast operator triage; any other phase
    /// (Pending, ShuttingDown, Inactive, Disabled, Terminated, Error) is
    /// reported as `False`.
    #[serde(default)]
    pub ready: bool,

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

    /// Spot-schedule provider resolution state (ADR 0006), present only when
    /// `spec.spotSchedule` is set. Carries the last resolved/held provider
    /// `active` value (the input to hold-last-state composition), the
    /// resolution reason/message, and the provider's observed generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spot_schedule: Option<SpotScheduleStatus>,
}

/// Resolution state of a `ScheduledMachine`'s `spec.spotSchedule` provider
/// reference (ADR 0006). This is the durable surface the controller reads for
/// hold-last-state composition and that operators inspect to see *why* a
/// provider-driven machine is (in)active.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SpotScheduleStatus {
    /// Whether the referenced provider resolved into an authoritative verdict
    /// this reconcile. `false` mirrors a `SpotScheduleResolved=False`
    /// condition — the provider CRD is absent, the object is missing, it
    /// exposes no `status.active`, or it is not `Ready`.
    pub resolved: bool,

    /// Last known provider `status.active`. Held across an unresolved reconcile
    /// (hold-last-state); `None` until the provider first resolves.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,

    /// Machine-readable resolution reason (a `REASON_SPOT_SCHEDULE_*` value).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// Human-readable resolution detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// The provider's `status.observedGeneration` at the last resolution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_generation: Option<i64>,

    /// When `active` last transitioned (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_transition_time: Option<String>,
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

// ============================================================================
// CapitalMarketsSchedule - reference spot-schedule provider CRD (ADR 0006)
// ============================================================================

/// A single trading session window, expressed with the same day/hour range
/// syntax as [`ScheduleSpec`] (`mon-fri`, `9-17`, …). The market is "in
/// session" when the current time falls inside any session.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TradingSession {
    /// Days of the week this session runs (e.g. `["mon-fri"]`).
    #[serde(default)]
    pub days_of_week: Vec<String>,

    /// Hours of the day this session is open (e.g. `["9-16"]`).
    #[serde(default)]
    pub hours_of_day: Vec<String>,
}

/// An early-close override: on `date` the market closes at the end of
/// `closeHour` instead of its normal session end.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EarlyClose {
    /// Calendar date in `YYYY-MM-DD` (ISO-8601) form.
    #[schemars(schema_with = "iso_date_schema")]
    pub date: String,

    /// Last active hour (0–23) on that date.
    #[schemars(schema_with = "hour_of_day_schema")]
    pub close_hour: u8,
}

/// Reference spot-schedule provider (ADR 0006) modelling a capital-markets
/// **exchange calendar**: trading sessions, statutory holidays, and early
/// closes evaluated in a configured timezone. The provider controller (roadmap
/// Phase 5) reconciles this `spec` into the duck-typed
/// [`status.active`](CapitalMarketsScheduleStatus::active) boolean that a
/// `ScheduledMachine.spec.spotSchedule` consumes; 5-Spot's controller reads
/// only that status, never this spec.
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "spotschedules.5spot.finos.org",
    version = "v1alpha1",
    kind = "CapitalMarketsSchedule",
    namespaced,
    shortname = "cms",
    status = "CapitalMarketsScheduleStatus",
    printcolumn = r#"{"name":"Active","type":"boolean","jsonPath":".status.active"}"#,
    printcolumn = r#"{"name":"Timezone","type":"string","jsonPath":".spec.timezone"}"#,
    printcolumn = r#"{"name":"NextTransition","type":"string","jsonPath":".status.nextTransitionTime"}"#,
    printcolumn = r#"{"name":"Age","type":"date","jsonPath":".metadata.creationTimestamp"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct CapitalMarketsScheduleSpec {
    /// IANA timezone the sessions, holidays, and early closes are evaluated in
    /// (e.g. `America/New_York`). Defaults to `UTC`.
    #[serde(default = "default_timezone")]
    #[schemars(schema_with = "timezone_schema")]
    pub timezone: String,

    /// Trading sessions during which the market is open. Empty means the
    /// provider is never active on the session axis (still subject to
    /// holidays / early closes).
    #[serde(default)]
    pub sessions: Vec<TradingSession>,

    /// Calendar dates (`YYYY-MM-DD`) on which the market is fully closed,
    /// overriding any session.
    #[serde(default)]
    #[schemars(schema_with = "iso_date_list_schema")]
    pub holidays: Vec<String>,

    /// Early-close overrides for specific dates.
    #[serde(default)]
    pub early_closes: Vec<EarlyClose>,
}

/// Runtime status of a [`CapitalMarketsSchedule`]. Satisfies the spot-schedule
/// provider contract (ADR 0006): the authoritative signal is
/// [`active`](Self::active); `conditions[type=Ready]`, `observedGeneration`,
/// and `lastTransitionTime` are the recommended observability surface.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CapitalMarketsScheduleStatus {
    /// **The contract field.** `true` ⇒ a `ScheduledMachine` referencing this
    /// object should be active; `false` ⇒ it should be inactive.
    #[serde(default)]
    pub active: bool,

    /// Standard Kubernetes conditions. A `Ready` condition is recommended so
    /// consumers can distinguish "computed and current" from "stale/unhealthy"
    /// (the latter is treated as *unresolved* by 5-Spot, not inactive).
    #[serde(default)]
    pub conditions: Vec<Condition>,

    /// The `metadata.generation` this status reflects, for staleness detection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,

    /// When `active` last transitioned (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_transition_time: Option<String>,

    /// When `active` is next expected to transition (RFC3339) — the next
    /// session/holiday boundary. Lets the provider requeue exactly at the
    /// boundary instead of polling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_transition_time: Option<String>,
}

/// Schema for an `hoursOfDay` single value / `closeHour` — an integer hour
/// 0–23.
fn hour_of_day_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "integer",
        "minimum": 0,
        "maximum": 23
    })
}

/// Schema for an ISO-8601 calendar date string (`YYYY-MM-DD`).
fn iso_date_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}$"
    })
}

/// Schema for a bounded list of ISO-8601 calendar dates.
fn iso_date_list_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "array",
        "maxItems": 366,
        "items": {
            "type": "string",
            "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}$"
        }
    })
}

// ============================================================================
// v1alpha1 - frozen, deprecated ScheduledMachine version (ADR 0007)
// ============================================================================

/// The frozen, **deprecated** `v1alpha1` version of `ScheduledMachine`.
///
/// Kept served for backward compatibility (existing
/// `5spot.finos.org/v1alpha1` manifests continue to apply) but carries neither
/// `spec.spotSchedule` nor an optional `spec.schedule`. The storage version is
/// `v1beta1` (the top-level [`crate::crd::ScheduledMachineSpec`]); under
/// `conversion.strategy: None` the API server relabels stored objects between
/// versions losslessly because `v1beta1` is a superset. The controller operates
/// exclusively on `v1beta1`; this module exists solely so `crdgen` can merge
/// both served versions into one CRD. See ADR 0007.
pub mod v1alpha1 {
    use super::{
        cluster_name_schema, default_graceful_shutdown_timeout, default_node_drain_timeout,
        default_priority, kill_if_commands_schema, EmbeddedResource, KataConfig,
        KubeconfigSecretRef, MachineTemplateSpec, NodeTaint, ScheduleSpec, ScheduledMachineStatus,
    };
    use kube::CustomResource;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    /// Frozen `v1alpha1` spec — identical to the original `ScheduledMachine`
    /// shape before `spec.spotSchedule` was introduced. `schedule` is
    /// **required** here (it predates the optional-schedule relaxation).
    #[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
    #[kube(
        group = "5spot.finos.org",
        version = "v1alpha1",
        kind = "ScheduledMachine",
        namespaced,
        shortname = "sm",
        status = "ScheduledMachineStatus",
        deprecated = "5spot.finos.org/v1alpha1 ScheduledMachine is deprecated; migrate to v1beta1 (adds spec.spotSchedule)",
        printcolumn = r#"{"name":"Phase","type":"string","jsonPath":".status.phase"}"#,
        printcolumn = r#"{"name":"Ready","type":"boolean","jsonPath":".status.ready"}"#,
        printcolumn = r#"{"name":"InSchedule","type":"boolean","jsonPath":".status.inSchedule"}"#,
        printcolumn = r#"{"name":"Enabled","type":"boolean","jsonPath":".spec.schedule.enabled"}"#,
        printcolumn = r#"{"name":"Schedule Days","type":"string","jsonPath":".spec.schedule.daysOfWeek"}"#,
        printcolumn = r#"{"name":"Schedule Hours","type":"string","jsonPath":".spec.schedule.hoursOfDay"}"#,
        printcolumn = r#"{"name":"KillSwitch","type":"boolean","jsonPath":".spec.killSwitch"}"#,
        printcolumn = r#"{"name":"Age","type":"date","jsonPath":".metadata.creationTimestamp"}"#
    )]
    #[serde(rename_all = "camelCase")]
    pub struct ScheduledMachineSpec {
        /// Machine scheduling configuration (required in `v1alpha1`).
        pub schedule: ScheduleSpec,

        /// Name of the CAPI cluster this machine belongs to.
        #[schemars(schema_with = "cluster_name_schema")]
        pub cluster_name: String,

        /// Inline bootstrap configuration spec (e.g., `K0sWorkerConfig`).
        pub bootstrap_spec: EmbeddedResource,

        /// Inline infrastructure configuration spec (e.g., `RemoteMachine`).
        pub infrastructure_spec: EmbeddedResource,

        /// Optional configuration for the created CAPI Machine.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub machine_template: Option<MachineTemplateSpec>,

        /// Priority for machine scheduling (higher values = higher priority).
        #[serde(default = "default_priority")]
        pub priority: u8,

        /// Timeout for graceful machine shutdown (e.g., "5m", "10s").
        #[serde(default = "default_graceful_shutdown_timeout")]
        pub graceful_shutdown_timeout: String,

        /// Timeout for draining the node before deletion (e.g., "5m", "10m").
        #[serde(default = "default_node_drain_timeout")]
        pub node_drain_timeout: String,

        /// When true, immediately removes the machine from cluster.
        #[serde(default)]
        pub kill_switch: bool,

        /// User-defined taints applied to the Kubernetes Node once it is Ready.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub node_taints: Vec<NodeTaint>,

        /// Optional list of process patterns that trigger an emergency node
        /// reclaim.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        #[schemars(schema_with = "kill_if_commands_schema")]
        pub kill_if_commands: Option<Vec<String>>,

        /// Reference to a Secret holding a workload-cluster kubeconfig.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub kubeconfig_secret_ref: Option<KubeconfigSecretRef>,

        /// Optional reference to a Kata containerd drop-in source on the
        /// workload cluster.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub kata: Option<KataConfig>,
    }
}

#[cfg(test)]
#[path = "crd_tests.rs"]
mod tests;
