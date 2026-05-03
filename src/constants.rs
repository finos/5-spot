// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
//! # Global constants
//!
//! All named constants used across the controller.  No magic numbers are
//! permitted in source files — every literal that carries semantic meaning must
//! be declared here.
//!
//! Constants are grouped by concern:
//! - Kubernetes API strings (`API_GROUP`, `API_VERSION`, CAPI constants)
//! - Resource names and kinds
//! - Finalizer names
//! - Timing values (requeue intervals, timeouts)
//! - Status condition types, statuses, and reasons
//! - Machine phase names
//! - Metrics and health endpoint paths
//! - Operator instance environment variables
//! - Security constraints (`MAX_DURATION_SECS`, `RESERVED_LABEL_PREFIXES`, …)

// ============================================================================
// Kubernetes API Constants
// ============================================================================

/// API group for 5-Spot CRDs
pub const API_GROUP: &str = "5spot.finos.org";

/// API version for 5-Spot CRDs
pub const API_VERSION: &str = "v1alpha1";

/// Full API version string
pub const API_VERSION_FULL: &str = "5spot.finos.org/v1alpha1";

// ============================================================================
// Resource Names
// ============================================================================

/// `ScheduledMachine` resource kind
pub const KIND_SCHEDULED_MACHINE: &str = "ScheduledMachine";

/// `ScheduledMachine` resource plural name
pub const RESOURCE_SCHEDULED_MACHINES: &str = "scheduledmachines";

/// CAPI Machine resource kind
pub const KIND_MACHINE: &str = "Machine";

/// `ConfigMap` resource kind
pub const KIND_CONFIG_MAP: &str = "ConfigMap";

/// Secret resource kind
pub const KIND_SECRET: &str = "Secret";

// ============================================================================
// Finalizer Names
// ============================================================================

/// Finalizer for `ScheduledMachine` resources
pub const FINALIZER_SCHEDULED_MACHINE: &str = "5spot.finos.org/scheduledmachine";

// ============================================================================
// Emergency Reclaim — Node annotations and labels
// ============================================================================
//
// Contract between the node-side `5spot-reclaim-agent` (rung 1: /proc poll;
// rung 2: netlink proc connector) and the 5-Spot controller. The agent
// writes these annotations on its own `Node` object via a PATCH scoped to
// the kubelet's node-scoped token — no broad RBAC is required.
//
// When the controller observes [`RECLAIM_REQUESTED_ANNOTATION`] set to
// `"true"` on a Node that maps to a `ScheduledMachine`, it enters
// `Phase::EmergencyRemove`, skipping the graceful-drain timeouts.

/// Annotation key on a `Node` requesting emergency removal. Value must be
/// the literal string `"true"` to trigger the reclaim path — any other
/// value (including presence with a different value) is ignored so that
/// partial writes cannot accidentally reclaim a node.
pub const RECLAIM_REQUESTED_ANNOTATION: &str = "5spot.finos.org/reclaim-requested";

/// Annotation key on a `Node` carrying the human-readable reason the
/// agent fired. Format: `"<source>: <detail>"` (e.g.
/// `"process-match: java"`). Surfaced on the `ScheduledMachine`
/// `EmergencyReclaim` event so operators can correlate without reading
/// node-level logs.
pub const RECLAIM_REASON_ANNOTATION: &str = "5spot.finos.org/reclaim-reason";

/// Annotation key on a `Node` recording the RFC-3339 UTC timestamp at
/// which the reclaim was requested. Used for audit and for detecting
/// stale annotations left behind by a node that rejoined the cluster
/// before the controller observed the request.
pub const RECLAIM_REQUESTED_AT_ANNOTATION: &str = "5spot.finos.org/reclaim-requested-at";

/// Value written to [`RECLAIM_REQUESTED_ANNOTATION`]. Any other value is
/// treated as "not requested" by the controller; this avoids foot-guns
/// like `"false"`, `"1"`, or empty strings accidentally triggering
/// nuclear removal.
pub const RECLAIM_REQUESTED_VALUE: &str = "true";

/// Node label key used by the reclaim-agent `DaemonSet`'s `nodeSelector`.
/// The controller stamps this onto every Node backing a
/// `ScheduledMachine` whose `spec.killIfCommands` is non-empty;
/// clearing the list removes the label and tears the agent off the node.
/// Mirrors the `katacontainers.io/kata-runtime` opt-in pattern used by
/// `kata-deploy`.
pub const RECLAIM_AGENT_LABEL: &str = "5spot.finos.org/reclaim-agent";

/// Value for [`RECLAIM_AGENT_LABEL`] indicating the agent is enabled
/// for this node.
pub const RECLAIM_AGENT_LABEL_ENABLED: &str = "enabled";

/// Namespace on the **child** cluster where the 5-Spot controller
/// projects per-node `ConfigMap`s for the reclaim agent and where the
/// `DaemonSet` itself runs. Mirrors the controller's management-cluster
/// [`DEFAULT_LEASE_NAMESPACE`] for symmetry.
pub const RECLAIM_AGENT_NAMESPACE: &str = "5spot-system";

/// Prefix for the per-node `ConfigMap` projected from
/// `spec.killIfCommands`. Full name is
/// `reclaim-agent-<sanitised-node-name>`.
pub const RECLAIM_AGENT_CONFIGMAP_PREFIX: &str = "reclaim-agent-";

/// Key inside the per-node `ConfigMap.data` that carries the agent's TOML
/// body. The controller writes under this key in `build_reclaim_agent_configmap`
/// and the agent reads from this key when it observes a `ConfigMap` event.
/// Both sides MUST agree on the literal — centralising it here is the only
/// thing that keeps the projection/consumption contract from silently
/// drifting if one side is renamed in isolation.
pub const RECLAIM_CONFIG_DATA_KEY: &str = "reclaim.toml";

/// Kubernetes Event reason emitted on the `ScheduledMachine` when the
/// emergency reclaim path fires. Operators see this in
/// `kubectl describe scheduledmachine`.
pub const REASON_EMERGENCY_RECLAIM: &str = "EmergencyReclaim";

/// Kubernetes Event reason emitted on the `ScheduledMachine` **after** the
/// controller has flipped `spec.schedule.enabled = false` as part of the
/// emergency reclaim. Split from [`REASON_EMERGENCY_RECLAIM`] so operators
/// can tell from `kubectl describe` that the schedule was the explicit
/// cause of the node's continued absence (vs. the ejection itself).
pub const REASON_EMERGENCY_RECLAIM_DISABLED_SCHEDULE: &str = "EmergencyReclaimDisabledSchedule";

/// Drain timeout for the emergency reclaim path, in seconds.
///
/// Deliberately shorter than [`DEFAULT_NODE_DRAIN_TIMEOUT_SECS`]: by the
/// time the node-side agent has annotated the Node, the user-space
/// process match is the authoritative signal that the workload cannot
/// remain on this node — we do not want to stall for the full five-minute
/// drain window while a misbehaving process keeps running. Pods that
/// cannot evict within this bound are left to be forcibly terminated
/// when the Machine is deleted.
pub const EMERGENCY_DRAIN_TIMEOUT_SECS: u64 = 60;

/// Number of recent reclaim events that constitute a "rapid" loop.
///
/// When `≥ RAPID_RE_RECLAIM_THRESHOLD` reclaims fire on the same SM
/// within [`RAPID_RE_RECLAIM_WINDOW_SECS`], the controller emits a
/// `RapidReReclaim` Warning Event recommending the operator stop the
/// conflicting process before re-enabling.
///
/// Threshold of 3 catches the canonical "user re-enables a SM whose
/// JVM is still running, gets ejected, re-enables, gets ejected again"
/// trio without flagging legitimate re-enables that just happen to
/// land near a prior reclaim.
pub const RAPID_RE_RECLAIM_THRESHOLD: usize = 3;

/// Sliding-window length (10 minutes) for rapid-re-reclaim detection.
/// Reclaims older than this drop off the tracked list and no longer
/// count toward the threshold.
pub const RAPID_RE_RECLAIM_WINDOW_SECS: i64 = 600;

/// Cap on how many reclaim timestamps we hold in memory per SM, to
/// bound the worst-case allocation. Larger than
/// [`RAPID_RE_RECLAIM_THRESHOLD`] so a brief storm above the threshold
/// is still observable in logs without resizing.
pub const RAPID_RE_RECLAIM_MAX_TRACKED: usize = 10;

/// Kubernetes Event reason emitted on the `ScheduledMachine` when the
/// rapid-re-reclaim threshold is crossed. Operators see this in
/// `kubectl describe scheduledmachine` and on the `RapidReReclaim`
/// metric.
pub const REASON_RAPID_RE_RECLAIM: &str = "RapidReReclaim";

// ============================================================================
// Timing Constants (in seconds)
// ============================================================================

/// Default requeue interval for successful reconciliation (5 minutes)
pub const DEFAULT_REQUEUE_SECS: u64 = 300;

/// Error requeue interval (30 seconds)
pub const ERROR_REQUEUE_SECS: u64 = 30;

/// Timer reconciliation interval (60 seconds)
pub const TIMER_REQUEUE_SECS: u64 = 60;

/// Grace period for machine shutdown (5 minutes)
pub const DEFAULT_GRACE_PERIOD_SECS: u64 = 300;

/// Node drain timeout (5 minutes)
pub const DEFAULT_NODE_DRAIN_TIMEOUT_SECS: u64 = 300;

/// Timeout for Kubernetes API operations (30 seconds)
pub const K8S_API_TIMEOUT_SECS: u64 = 30;

/// Maximum backoff time for exponential retry (5 minutes)
pub const MAX_BACKOFF_SECS: u64 = 300;

/// Maximum number of per-resource reconciliation retries before capping at [`MAX_BACKOFF_SECS`]
pub const MAX_RECONCILE_RETRIES: u32 = 10;

// ============================================================================
// Leader Election Constants
// ============================================================================

/// Default Kubernetes Lease name used for leader election
pub const DEFAULT_LEASE_NAME: &str = "5spot-leader";

/// Default Lease duration in seconds — how long the lease is considered valid
pub const DEFAULT_LEASE_DURATION_SECS: u64 = 15;

/// Default renew deadline in seconds — the leader must renew before this deadline
pub const DEFAULT_LEASE_RENEW_DEADLINE_SECS: u64 = 10;

/// Default retry period in seconds — documented for ops; not a direct `kube-lease-manager` parameter
pub const DEFAULT_LEASE_RETRY_PERIOD_SECS: u64 = 2;

/// Computed default grace period (duration − `renew_deadline`) in seconds
pub const DEFAULT_LEASE_GRACE_SECS: u64 =
    DEFAULT_LEASE_DURATION_SECS - DEFAULT_LEASE_RENEW_DEADLINE_SECS;

/// Default namespace for the leader election Lease resource
pub const DEFAULT_LEASE_NAMESPACE: &str = "5spot-system";

// ============================================================================
// Condition Types
// ============================================================================

/// Ready condition type
pub const CONDITION_TYPE_READY: &str = "Ready";

/// Scheduled condition type
pub const CONDITION_TYPE_SCHEDULED: &str = "Scheduled";

/// `MachineReady` condition type
pub const CONDITION_TYPE_MACHINE_READY: &str = "MachineReady";

/// `ReferencesValid` condition type
pub const CONDITION_TYPE_REFERENCES_VALID: &str = "ReferencesValid";

/// `NodeTainted` condition type — tracks user-declared Node taint application.
/// See `~/dev/roadmaps/completed-5spot-user-defined-node-taints.md` Phase 2.
pub const CONDITION_TYPE_NODE_TAINTED: &str = "NodeTainted";

// ============================================================================
// Condition Statuses
// ============================================================================

/// Condition status: True
pub const CONDITION_STATUS_TRUE: &str = "True";

/// Condition status: False
pub const CONDITION_STATUS_FALSE: &str = "False";

/// Condition status: Unknown
pub const CONDITION_STATUS_UNKNOWN: &str = "Unknown";

// ============================================================================
// Condition Reasons
// ============================================================================

/// Reason: Reconcile succeeded
pub const REASON_RECONCILE_SUCCEEDED: &str = "ReconcileSucceeded";

/// Reason: Reconcile failed
pub const REASON_RECONCILE_FAILED: &str = "ReconcileFailed";

/// Reason: Schedule active
pub const REASON_SCHEDULE_ACTIVE: &str = "ScheduleActive";

/// Reason: Schedule inactive
pub const REASON_SCHEDULE_INACTIVE: &str = "ScheduleInactive";

/// Reason: Machine created
pub const REASON_MACHINE_CREATED: &str = "MachineCreated";

/// Reason: Machine deleted
pub const REASON_MACHINE_DELETED: &str = "MachineDeleted";

/// Reason: Machine ready
pub const REASON_MACHINE_READY: &str = "MachineReady";

/// Reason: Kill switch activated
pub const REASON_KILL_SWITCH: &str = "KillSwitchActivated";

/// Reason: Awaiting schedule
pub const REASON_AWAITING_SCHEDULE: &str = "AwaitingSchedule";

/// Reason: Grace period active
pub const REASON_GRACE_PERIOD: &str = "GracePeriodActive";

/// Reason: References invalid
pub const REASON_REFERENCES_INVALID: &str = "ReferencesInvalid";

/// Reason: File resolution failed
pub const REASON_FILE_RESOLUTION_FAILED: &str = "FileResolutionFailed";

/// Reason: Schedule disabled
pub const REASON_SCHEDULE_DISABLED: &str = "ScheduleDisabled";

/// Reason: Node draining
pub const REASON_NODE_DRAINING: &str = "NodeDraining";

/// Reason: Node drained
pub const REASON_NODE_DRAINED: &str = "NodeDrained";

/// Reason: Node drain failed
pub const REASON_NODE_DRAIN_FAILED: &str = "NodeDrainFailed";

/// Reason: all declared `spec.nodeTaints` are present on the Node.
pub const REASON_NODE_TAINTS_APPLIED: &str = "Applied";

/// Reason: Node exists but its `Ready` condition is not `True` yet, so we
/// deliberately defer applying taints to avoid racing with kubelet init.
pub const REASON_NODE_NOT_READY: &str = "NodeNotReady";

/// Reason: the most recent `PATCH` against the Node's `spec.taints` returned
/// an API error. Next reconcile will retry with backoff.
pub const REASON_NODE_TAINT_PATCH_FAILED: &str = "PatchFailed";

/// Reason: `status.nodeRef` is not populated yet (CAPI has not claimed a Node
/// for this machine). Condition status is `Unknown` in this case.
pub const REASON_NO_NODE_YET: &str = "NoNodeYet";

/// Reason: an admin-added taint on the Node collides on `(key, effect)` with
/// an entry in `spec.nodeTaints`. The controller refuses to overwrite and
/// surfaces this condition so the operator can reconcile ownership.
pub const REASON_TAINT_OWNERSHIP_CONFLICT: &str = "TaintOwnershipConflict";

/// Server-side-apply field manager used when the controller patches Node
/// `spec.taints`. Distinct from other field managers so a `kubectl describe
/// node` shows ownership clearly.
pub const NODE_TAINT_FIELD_MANAGER: &str = "5spot-controller-node-taints";

/// Annotation written on every Node whose taints we manage, carrying a JSON
/// array of `(key, effect)` tuples the controller owns. Lets a second
/// controller (or a human) see our ownership at a glance without reading the
/// `ScheduledMachine` CR.
pub const APPLIED_TAINTS_ANNOTATION: &str = "5spot.finos.org/applied-taints";

// ============================================================================
// SSH and Machine Defaults
// ============================================================================

/// Default SSH port
pub const DEFAULT_SSH_PORT: u16 = 22;

/// Default machine priority
pub const DEFAULT_PRIORITY: u8 = 50;

/// Minimum priority value
pub const MIN_PRIORITY: u8 = 0;

/// Maximum priority value
pub const MAX_PRIORITY: u8 = 100;

// ============================================================================
// Time and Schedule Constants
// ============================================================================

/// Default timezone
pub const DEFAULT_TIMEZONE: &str = "UTC";

/// Number of days in a week
pub const DAYS_IN_WEEK: u8 = 7;

/// Hours in a day
pub const HOURS_IN_DAY: u8 = 24;

/// Maximum hour value (23 for 0-23 range)
pub const MAX_HOUR: u8 = 23;

// ============================================================================
// CAPI Machine Phase Constants
// ============================================================================

/// Machine phase: Pending (initial state)
pub const PHASE_PENDING: &str = "Pending";

/// Machine phase: Active (machine is running)
pub const PHASE_ACTIVE: &str = "Active";

/// Machine phase: `ShuttingDown` (graceful shutdown in progress)
pub const PHASE_SHUTTING_DOWN: &str = "ShuttingDown";

/// Machine phase: Inactive (machine removed)
pub const PHASE_INACTIVE: &str = "Inactive";

/// Machine phase: Disabled (schedule disabled)
pub const PHASE_DISABLED: &str = "Disabled";

/// Machine phase: Terminated (kill switch activated)
pub const PHASE_TERMINATED: &str = "Terminated";

/// Machine phase: Error (error occurred)
pub const PHASE_ERROR: &str = "Error";

/// Machine phase: `EmergencyRemove` (node-driven non-graceful reclaim).
///
/// Entered when the controller observes
/// [`RECLAIM_REQUESTED_ANNOTATION`] = `"true"` on the Node backing a
/// `ScheduledMachine`. Differs from [`PHASE_TERMINATED`] (operator-driven,
/// terminal) in three ways:
/// 1. Trigger is node-local (`5spot-reclaim-agent` process match) rather
///    than an operator flipping `spec.killSwitch`.
/// 2. Exit is non-terminal: the handler transitions to [`PHASE_DISABLED`]
///    after clearing the annotations and flipping
///    `spec.schedule.enabled = false` to break the
///    eject→re-add→re-eject loop.
/// 3. Drain uses a short emergency timeout
///    ([`EMERGENCY_DRAIN_TIMEOUT_SECS`]) rather than
///    `spec.nodeDrainTimeout`, since the user-space process match is the
///    authoritative signal that the node must leave the cluster now.
pub const PHASE_EMERGENCY_REMOVE: &str = "EmergencyRemove";

// ============================================================================
// CAPI API Constants
// ============================================================================

/// Cluster API Machine API group
pub const CAPI_MACHINE_API_GROUP: &str = "cluster.x-k8s.io";
// ============================================================================
// CAPI (Cluster API) Constants
// ============================================================================

/// CAPI API group
pub const CAPI_GROUP: &str = "cluster.x-k8s.io";

/// CAPI Machine API version
pub const CAPI_MACHINE_API_VERSION: &str = "v1beta1";

/// Full CAPI Machine API version string
pub const CAPI_MACHINE_API_VERSION_FULL: &str = "cluster.x-k8s.io/v1beta1";

/// CAPI cluster name label
pub const CAPI_CLUSTER_NAME_LABEL: &str = "cluster.x-k8s.io/cluster-name";

/// CAPI Machine resource plural name
pub const CAPI_RESOURCE_MACHINES: &str = "machines";

// ============================================================================
// Kubernetes Core API Constants
// ============================================================================

/// Kubernetes Node resource kind
pub const KIND_NODE: &str = "Node";

/// Kubernetes Node resource plural name
pub const RESOURCE_NODES: &str = "nodes";

/// Kubernetes Pod resource kind
pub const KIND_POD: &str = "Pod";

/// Kubernetes Pod resource plural name
pub const RESOURCE_PODS: &str = "pods";

/// Pod eviction grace period (seconds)
pub const POD_EVICTION_GRACE_PERIOD_SECS: i64 = 30;

// ============================================================================
// Metrics Constants
// ============================================================================

/// Metrics port
pub const METRICS_PORT: u16 = 8080;

/// Health check port
pub const HEALTH_PORT: u16 = 8081;

/// Metrics endpoint path
pub const METRICS_PATH: &str = "/metrics";

/// Health endpoint path
pub const HEALTH_PATH: &str = "/health";

/// Readiness endpoint path
pub const READINESS_PATH: &str = "/ready";

// ============================================================================
// Operator Instance Constants
// ============================================================================

/// Environment variable for operator instance ID
pub const ENV_OPERATOR_INSTANCE_ID: &str = "OPERATOR_INSTANCE_ID";

/// Environment variable for total instance count
pub const ENV_OPERATOR_INSTANCE_COUNT: &str = "OPERATOR_INSTANCE_COUNT";

/// Default instance ID if not set
pub const DEFAULT_INSTANCE_ID: u32 = 0;

/// Default instance count if not set
pub const DEFAULT_INSTANCE_COUNT: u32 = 1;

// ============================================================================
// Field Manager
// ============================================================================

/// Field manager name for server-side apply
pub const FIELD_MANAGER: &str = "5spot-controller";

// ============================================================================
// Security Constants
// ============================================================================

/// Maximum allowed duration for timeout fields (24 hours in seconds)
pub const MAX_DURATION_SECS: u64 = 86_400;

/// Maximum allowed timezone string length
pub const MAX_TIMEZONE_LEN: usize = 64;

/// Maximum allowed length of `spec.clusterName`.
///
/// CAPI's `Cluster.metadata.name` inherits the Kubernetes DNS-1123 subdomain
/// cap (253 chars), but cluster names are used downstream as DNS labels and
/// as label values (`cluster.x-k8s.io/cluster-name`), both of which are
/// bounded by RFC-1123's 63-character DNS label limit. 63 is therefore the
/// effective CAPI constraint. Rejecting longer values at the CR boundary
/// also bounds Prometheus label cardinality (metrics emit the cluster name
/// via `CAPI_CLUSTER_NAME_LABEL`) and caps log-line width, closing a
/// cheap log-injection / cardinality-DoS vector.
pub const MAX_CLUSTER_NAME_LEN: usize = 63;

/// Maximum number of `spec.killIfCommands` patterns accepted on a single
/// `ScheduledMachine`. The reclaim agent evaluates every pattern against
/// every PID in `/proc`; an unbounded list lets a malicious (or
/// misconfigured) CR pin agent CPU. 100 is well above any realistic
/// workload — a single node rarely runs more than a dozen distinct
/// kill-switchable processes — and keeps the worst-case match cost bounded.
pub const MAX_KILL_IF_COMMANDS_COUNT: usize = 100;

/// Maximum length of a single `spec.killIfCommands` entry. Patterns are
/// matched against `/proc/<pid>/comm` (15 chars max per the kernel) and
/// `/proc/<pid>/cmdline` (longer, but arguments beyond 256 bytes are a red
/// flag for the intended use case of process-basename matching). This bound
/// also caps Prometheus label widths if a future metric tags reclaim
/// outcomes by matched pattern.
pub const MAX_KILL_IF_COMMAND_LEN: usize = 256;

/// Timeout for finalizer cleanup operations (10 minutes in seconds)
pub const FINALIZER_CLEANUP_TIMEOUT_SECS: u64 = 600;

/// Reserved label/annotation key prefixes that users cannot inject into system resources
pub const RESERVED_LABEL_PREFIXES: &[&str] = &[
    "kubernetes.io/",
    "k8s.io/",
    "cluster.x-k8s.io/",
    "5spot.finos.org/",
];

/// Allowed API groups for bootstrap embedded resources
pub const ALLOWED_BOOTSTRAP_API_GROUPS: &[&str] = &["bootstrap.cluster.x-k8s.io", "k0smotron.io"];

/// Allowed API groups for infrastructure embedded resources
pub const ALLOWED_INFRASTRUCTURE_API_GROUPS: &[&str] =
    &["infrastructure.cluster.x-k8s.io", "k0smotron.io"];
