// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: MIT
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
pub const API_GROUP: &str = "5spot.io";

/// API version for 5-Spot CRDs
pub const API_VERSION: &str = "v1alpha1";

/// Full API version string
pub const API_VERSION_FULL: &str = "5spot.io/v1alpha1";

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
pub const FINALIZER_SCHEDULED_MACHINE: &str = "5spot.io/scheduledmachine";

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

/// Timeout for finalizer cleanup operations (10 minutes in seconds)
pub const FINALIZER_CLEANUP_TIMEOUT_SECS: u64 = 600;

/// Reserved label/annotation key prefixes that users cannot inject into system resources
pub const RESERVED_LABEL_PREFIXES: &[&str] = &[
    "kubernetes.io/",
    "k8s.io/",
    "cluster.x-k8s.io/",
    "5spot.io/",
];

/// Allowed API groups for bootstrap embedded resources
pub const ALLOWED_BOOTSTRAP_API_GROUPS: &[&str] = &["bootstrap.cluster.x-k8s.io", "k0smotron.io"];

/// Allowed API groups for infrastructure embedded resources
pub const ALLOWED_INFRASTRUCTURE_API_GROUPS: &[&str] =
    &["infrastructure.cluster.x-k8s.io", "k0smotron.io"];
