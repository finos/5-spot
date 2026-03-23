// Helper functions for ScheduledMachine reconciliation
// This file contains utility functions separated from the main reconciler logic

use super::{Context, ReconcilerError};
use crate::constants::{
    API_VERSION_FULL, CAPI_CLUSTER_NAME_LABEL, CAPI_GROUP, CAPI_MACHINE_API_VERSION,
    CAPI_MACHINE_API_VERSION_FULL, CAPI_RESOURCE_MACHINES, CONDITION_STATUS_TRUE,
    CONDITION_TYPE_READY, DEFAULT_INSTANCE_ID, ENV_OPERATOR_INSTANCE_ID, ERROR_REQUEUE_SECS,
    FINALIZER_SCHEDULED_MACHINE, PHASE_ACTIVE, PHASE_INACTIVE, PHASE_SHUTTING_DOWN,
    PHASE_TERMINATED, REASON_GRACE_PERIOD, REASON_KILL_SWITCH, REASON_RECONCILE_SUCCEEDED,
    TIMER_REQUEUE_SECS,
};
use crate::crd::{Condition, ScheduledMachine, ScheduledMachineStatus};
use chrono::{DateTime, Datelike, Timelike, Utc};
use chrono_tz::Tz;
use kube::{
    api::{Api, Patch, PatchParams},
    runtime::controller::Action,
    Client, Resource, ResourceExt,
};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info};

// ============================================================================
// Resource processing and consistent hashing
// ============================================================================

/// Determine if this operator instance should process a specific resource
/// Uses consistent hashing to distribute resources across instances
pub fn should_process_resource(
    name: &str,
    namespace: &str,
    priority: u8,
    instance_count: u32,
) -> bool {
    if instance_count <= 1 {
        return true;
    }

    // Create consistent hash of resource identifier with priority influence
    let resource_id = format!("{namespace}/{name}");
    let priority_modifier = u64::from(priority) * 1000;

    // Simple hash function (in production, consider using a proper hash)
    let mut hash: u64 = 0;
    for byte in resource_id.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(u64::from(byte));
    }
    hash = hash.wrapping_add(priority_modifier);

    #[allow(clippy::cast_possible_truncation)]
    let assigned_instance = (hash % u64::from(instance_count)) as u32;
    let current_instance = std::env::var(ENV_OPERATOR_INSTANCE_ID)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_INSTANCE_ID);

    debug!(
        resource = %resource_id,
        priority = priority,
        assigned_instance = assigned_instance,
        current_instance = current_instance,
        "Resource assignment check"
    );

    assigned_instance == current_instance
}

// ============================================================================
// Schedule evaluation
// ============================================================================

/// Evaluate if a machine should be active based on schedule
///
/// # Errors
/// Returns error if timezone is invalid or weekday/hour parsing fails
pub fn evaluate_schedule(
    schedule: &crate::crd::ScheduleSpec,
    check_time: Option<DateTime<Utc>>,
) -> Result<bool, ReconcilerError> {
    if !schedule.enabled {
        return Ok(false);
    }

    // If using cron expression, evaluate it
    if let Some(cron_expr) = &schedule.cron {
        // TODO: Implement cron expression evaluation
        // For now, return an error indicating cron is not yet supported
        return Err(ReconcilerError::ScheduleError(format!(
            "Cron expression evaluation not yet implemented: {cron_expr}"
        )));
    }

    let now = check_time.unwrap_or_else(Utc::now);

    // Parse timezone
    let tz: Tz = schedule.timezone.parse().map_err(|_| {
        ReconcilerError::ScheduleError(format!("Invalid timezone: {}", schedule.timezone))
    })?;

    let current_time = now.with_timezone(&tz);

    // Check weekday (Monday = 0, Sunday = 6)
    #[allow(clippy::cast_possible_truncation)]
    let current_weekday = current_time.weekday().num_days_from_monday() as u8;
    let allowed_weekdays = schedule
        .get_active_weekdays()
        .map_err(|e| ReconcilerError::ScheduleError(format!("Failed to parse weekdays: {e}")))?
        .ok_or_else(|| ReconcilerError::ScheduleError("No weekday schedule defined".to_string()))?;

    debug!(
        current_weekday = current_weekday,
        allowed_weekdays = ?allowed_weekdays,
        "Weekday check"
    );

    if !allowed_weekdays.contains(&current_weekday) {
        return Ok(false);
    }

    // Check hour
    #[allow(clippy::cast_possible_truncation)]
    let current_hour = current_time.hour() as u8;
    let allowed_hours = schedule
        .get_active_hours()
        .map_err(|e| ReconcilerError::ScheduleError(format!("Failed to parse hours: {e}")))?
        .ok_or_else(|| ReconcilerError::ScheduleError("No hour schedule defined".to_string()))?;

    debug!(
        current_hour = current_hour,
        allowed_hours = ?allowed_hours,
        "Hour check"
    );

    Ok(allowed_hours.contains(&current_hour))
}

// ============================================================================
// ============================================================================
// Finalizer management
// ============================================================================

/// Check if resource has finalizer
pub fn has_finalizer(resource: &ScheduledMachine) -> bool {
    resource
        .meta()
        .finalizers
        .as_ref()
        .is_some_and(|f| f.contains(&FINALIZER_SCHEDULED_MACHINE.to_string()))
}

/// Add finalizer to resource
pub async fn add_finalizer(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().unwrap();
    let name = resource.name_any();

    info!(
        resource = %name,
        namespace = %namespace,
        "Adding finalizer"
    );

    let api: Api<ScheduledMachine> = Api::namespaced(ctx.client.clone(), &namespace);

    let mut finalizers = resource.meta().finalizers.clone().unwrap_or_default();
    finalizers.push(FINALIZER_SCHEDULED_MACHINE.to_string());

    let patch = json!({
        "metadata": {
            "finalizers": finalizers
        }
    });

    api.patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;

    Ok(Action::requeue(Duration::from_secs(0)))
}

/// Handle resource deletion (finalizer cleanup)
pub async fn handle_deletion(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().unwrap();
    let name = resource.name_any();

    info!(
        resource = %name,
        namespace = %namespace,
        "Handling deletion"
    );

    // Check if machine is still in cluster
    let current_phase = resource.status.as_ref().and_then(|s| s.phase.as_deref());

    if let Some(phase) = current_phase {
        if matches!(phase, PHASE_ACTIVE | PHASE_SHUTTING_DOWN) {
            info!(
                resource = %name,
                namespace = %namespace,
                "Removing machine from cluster before deletion"
            );

            remove_machine_from_cluster(&resource, &ctx.client, &namespace).await?;
        }
    }

    // Remove finalizer
    let api: Api<ScheduledMachine> = Api::namespaced(ctx.client.clone(), &namespace);

    let mut finalizers = resource.meta().finalizers.clone().unwrap_or_default();
    finalizers.retain(|f| f != FINALIZER_SCHEDULED_MACHINE);

    let patch = json!({
        "metadata": {
            "finalizers": finalizers
        }
    });

    api.patch(&name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;

    info!(
        resource = %name,
        namespace = %namespace,
        "Finalizer removed, resource will be deleted"
    );

    Ok(Action::await_change())
}

// ============================================================================
// Kill switch handling
// ============================================================================

/// Handle kill switch activation - immediate removal
pub async fn handle_kill_switch(
    resource: Arc<ScheduledMachine>,
    ctx: Arc<Context>,
) -> Result<Action, ReconcilerError> {
    let namespace = resource.namespace().unwrap();
    let name = resource.name_any();

    let current_phase = resource.status.as_ref().and_then(|s| s.phase.as_deref());

    // Only remove if not already inactive or terminated
    if let Some(phase) = current_phase {
        if phase != PHASE_INACTIVE
            && phase != PHASE_TERMINATED
            && matches!(phase, PHASE_ACTIVE | PHASE_SHUTTING_DOWN)
        {
            info!(
                resource = %name,
                namespace = %namespace,
                "Kill switch active - removing machine immediately"
            );

            remove_machine_from_cluster(&resource, &ctx.client, &namespace).await?;
        }
    }

    update_phase(
        &ctx.client,
        &namespace,
        &name,
        PHASE_TERMINATED,
        Some(REASON_KILL_SWITCH),
        Some("Machine terminated due to kill switch"),
    )
    .await?;

    Ok(Action::requeue(Duration::from_secs(TIMER_REQUEUE_SECS)))
}

// ============================================================================
// Grace period management
// ============================================================================

/// Check if grace period has elapsed
pub fn check_grace_period_elapsed(resource: &ScheduledMachine) -> Result<bool, ReconcilerError> {
    let status = resource
        .status
        .as_ref()
        .ok_or_else(|| ReconcilerError::InvalidConfig("Resource has no status".to_string()))?;

    // Check for grace period start time in conditions
    let grace_start_str = status
        .conditions
        .iter()
        .find(|c| c.reason == REASON_GRACE_PERIOD)
        .map(|c| c.last_transition_time.as_str());

    if let Some(start_time_str) = grace_start_str {
        // Parse RFC3339 timestamp
        let start_time = DateTime::parse_from_rfc3339(start_time_str)
            .map_err(|e| ReconcilerError::InvalidConfig(format!("Invalid timestamp: {e}")))?
            .with_timezone(&Utc);

        let timeout = parse_duration(&resource.spec.graceful_shutdown_timeout)?;
        let now = Utc::now();
        let elapsed = now.signed_duration_since(start_time);

        debug!(
            grace_start = %start_time,
            elapsed_secs = elapsed.num_seconds(),
            timeout_secs = timeout.as_secs(),
            "Grace period check"
        );

        #[allow(clippy::cast_possible_wrap)]
        Ok(elapsed.num_seconds() >= timeout.as_secs() as i64)
    } else {
        // No grace period started yet
        Ok(true)
    }
}

/// Parse duration string (e.g., "5m", "10s", "1h")
pub fn parse_duration(duration_str: &str) -> Result<Duration, ReconcilerError> {
    let duration_str = duration_str.trim();

    if duration_str.is_empty() {
        return Err(ReconcilerError::InvalidConfig(
            "Empty duration string".to_string(),
        ));
    }

    let (value_str, unit) = duration_str.split_at(duration_str.len() - 1);
    let value: u64 = value_str.parse().map_err(|_| {
        ReconcilerError::InvalidConfig(format!("Invalid duration value: {duration_str}"))
    })?;

    let seconds = match unit {
        "s" => value,
        "m" => value * 60,
        "h" => value * 3600,
        _ => {
            return Err(ReconcilerError::InvalidConfig(format!(
                "Invalid duration unit: {unit}. Use 's', 'm', or 'h'"
            )))
        }
    };

    Ok(Duration::from_secs(seconds))
}

// ============================================================================
// Status update helpers
// ============================================================================

/// Update phase and status condition
pub async fn update_phase(
    client: &Client,
    namespace: &str,
    name: &str,
    phase: &str,
    reason: Option<&str>,
    message: Option<&str>,
) -> Result<(), ReconcilerError> {
    let api: Api<ScheduledMachine> = Api::namespaced(client.clone(), namespace);

    let condition = Condition::new(
        CONDITION_TYPE_READY,
        CONDITION_STATUS_TRUE,
        reason.unwrap_or(REASON_RECONCILE_SUCCEEDED),
        message.unwrap_or("Phase transition completed"),
    );

    let status = ScheduledMachineStatus {
        phase: Some(phase.to_string()),
        message: Some(message.unwrap_or("Phase transition completed").to_string()),
        conditions: vec![condition],
        ..Default::default()
    };

    let patch = json!({
        "status": status
    });

    api.patch_status(name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;

    Ok(())
}

/// Update phase with last schedule time
#[allow(dead_code)] // TODO: Use this when machine creation is implemented
pub async fn update_phase_with_last_schedule(
    client: &Client,
    namespace: &str,
    name: &str,
    phase: &str,
    reason: Option<&str>,
    message: Option<&str>,
) -> Result<(), ReconcilerError> {
    let api: Api<ScheduledMachine> = Api::namespaced(client.clone(), namespace);

    let condition = Condition::new(
        CONDITION_TYPE_READY,
        CONDITION_STATUS_TRUE,
        reason.unwrap_or(REASON_RECONCILE_SUCCEEDED),
        message.unwrap_or("Phase transition completed"),
    );

    let status = ScheduledMachineStatus {
        phase: Some(phase.to_string()),
        message: Some(message.unwrap_or("Phase transition completed").to_string()),
        conditions: vec![condition],
        last_scheduled_time: Some(Utc::now().to_rfc3339()),
        ..Default::default()
    };

    let patch = json!({
        "status": status
    });

    api.patch_status(name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;

    Ok(())
}

/// Update phase with grace period start time
pub async fn update_phase_with_grace_period(
    client: &Client,
    namespace: &str,
    name: &str,
    phase: &str,
    reason: Option<&str>,
    message: Option<&str>,
) -> Result<(), ReconcilerError> {
    let api: Api<ScheduledMachine> = Api::namespaced(client.clone(), namespace);

    let condition = Condition::new(
        CONDITION_TYPE_READY,
        CONDITION_STATUS_TRUE,
        reason.unwrap_or(REASON_GRACE_PERIOD),
        message.unwrap_or("Grace period started"),
    );

    let status = ScheduledMachineStatus {
        phase: Some(phase.to_string()),
        message: Some(message.unwrap_or("Grace period started").to_string()),
        conditions: vec![condition],
        ..Default::default()
    };

    let patch = json!({
        "status": status
    });

    api.patch_status(name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;

    Ok(())
}

// ============================================================================
// CAPI Resource Creation
// ============================================================================

/// Generate the name for a created bootstrap resource
fn bootstrap_resource_name(scheduled_machine_name: &str) -> String {
    format!("{scheduled_machine_name}-bootstrap")
}

/// Generate the name for a created infrastructure resource
fn infrastructure_resource_name(scheduled_machine_name: &str) -> String {
    format!("{scheduled_machine_name}-infra")
}

/// Generate the name for the created CAPI Machine
fn machine_resource_name(scheduled_machine_name: &str) -> String {
    format!("{scheduled_machine_name}-machine")
}

/// Add machine to cluster by creating bootstrap, infrastructure, and Machine resources
///
/// This function:
/// 1. Creates the bootstrap resource from bootstrapSpec
/// 2. Creates the infrastructure resource from infrastructureSpec
/// 3. Creates the CAPI Machine referencing both
#[allow(clippy::too_many_lines)]
pub async fn add_machine_to_cluster(
    resource: &ScheduledMachine,
    client: &Client,
    namespace: &str,
) -> Result<(), ReconcilerError> {
    let name = resource.name_any();
    let cluster_name = &resource.spec.cluster_name;

    let bootstrap_name = bootstrap_resource_name(&name);
    let infra_name = infrastructure_resource_name(&name);
    let machine_name = machine_resource_name(&name);

    info!(
        resource = %name,
        namespace = %namespace,
        cluster = %cluster_name,
        bootstrap = %bootstrap_name,
        infrastructure = %infra_name,
        machine = %machine_name,
        "Creating CAPI resources from inline specs"
    );

    let owner_ref = json!({
        "apiVersion": API_VERSION_FULL,
        "kind": "ScheduledMachine",
        "name": name,
        "uid": resource.metadata.uid.as_ref().ok_or_else(|| {
            ReconcilerError::InvalidConfig("Resource UID not set".to_string())
        })?,
        "controller": true,
        "blockOwnerDeletion": true,
    });

    // Determine namespaces for resources
    let bootstrap_ns = resource
        .spec
        .bootstrap_spec
        .namespace
        .as_deref()
        .unwrap_or(namespace);
    let infra_ns = resource
        .spec
        .infrastructure_spec
        .namespace
        .as_deref()
        .unwrap_or(namespace);

    // 1. Create bootstrap resource
    let bootstrap_spec = &resource.spec.bootstrap_spec;
    let bootstrap_obj = json!({
        "apiVersion": bootstrap_spec.api_version,
        "kind": bootstrap_spec.kind,
        "metadata": {
            "name": bootstrap_name,
            "namespace": bootstrap_ns,
            "ownerReferences": [owner_ref],
        },
        "spec": bootstrap_spec.spec,
    });

    create_dynamic_resource(
        client,
        bootstrap_ns,
        &bootstrap_spec.api_version,
        &bootstrap_spec.kind,
        bootstrap_obj,
    )
    .await
    .map_err(|e| ReconcilerError::CapiError(format!("Failed to create bootstrap resource: {e}")))?;

    info!(bootstrap = %bootstrap_name, "Bootstrap resource created");

    // 2. Create infrastructure resource
    let infra_spec = &resource.spec.infrastructure_spec;
    let infra_obj = json!({
        "apiVersion": infra_spec.api_version,
        "kind": infra_spec.kind,
        "metadata": {
            "name": infra_name,
            "namespace": infra_ns,
            "ownerReferences": [owner_ref],
        },
        "spec": infra_spec.spec,
    });

    create_dynamic_resource(
        client,
        infra_ns,
        &infra_spec.api_version,
        &infra_spec.kind,
        infra_obj,
    )
    .await
    .map_err(|e| {
        ReconcilerError::CapiError(format!("Failed to create infrastructure resource: {e}"))
    })?;

    info!(infrastructure = %infra_name, "Infrastructure resource created");

    // 3. Create CAPI Machine referencing both
    let mut machine_labels = std::collections::BTreeMap::new();
    machine_labels.insert(CAPI_CLUSTER_NAME_LABEL.to_string(), cluster_name.clone());
    machine_labels.insert("5spot.io/scheduled-machine".to_string(), name.clone());

    // Merge in user-provided labels
    if let Some(template) = &resource.spec.machine_template {
        for (k, v) in &template.labels {
            machine_labels.insert(k.clone(), v.clone());
        }
    }

    let mut machine_annotations = std::collections::BTreeMap::new();
    // Merge in user-provided annotations
    if let Some(template) = &resource.spec.machine_template {
        for (k, v) in &template.annotations {
            machine_annotations.insert(k.clone(), v.clone());
        }
    }

    let machine_obj = json!({
        "apiVersion": CAPI_MACHINE_API_VERSION_FULL,
        "kind": "Machine",
        "metadata": {
            "name": machine_name,
            "namespace": namespace,
            "labels": machine_labels,
            "annotations": machine_annotations,
            "ownerReferences": [owner_ref],
        },
        "spec": {
            "clusterName": cluster_name,
            "bootstrap": {
                "configRef": {
                    "apiVersion": bootstrap_spec.api_version,
                    "kind": bootstrap_spec.kind,
                    "name": bootstrap_name,
                    "namespace": bootstrap_ns,
                }
            },
            "infrastructureRef": {
                "apiVersion": infra_spec.api_version,
                "kind": infra_spec.kind,
                "name": infra_name,
                "namespace": infra_ns,
            },
        }
    });

    create_dynamic_resource(
        client,
        namespace,
        CAPI_MACHINE_API_VERSION_FULL,
        "Machine",
        machine_obj,
    )
    .await
    .map_err(|e| ReconcilerError::CapiError(format!("Failed to create Machine: {e}")))?;

    info!(
        resource = %name,
        machine_name = %machine_name,
        "CAPI Machine created successfully"
    );

    Ok(())
}

/// Helper to create a dynamic Kubernetes resource
async fn create_dynamic_resource(
    client: &Client,
    namespace: &str,
    api_version: &str,
    kind: &str,
    obj: serde_json::Value,
) -> Result<(), kube::Error> {
    let (group, version) = parse_api_version(api_version);

    let api: Api<kube::core::DynamicObject> = Api::namespaced_with(
        client.clone(),
        namespace,
        &kube::discovery::ApiResource {
            group,
            version,
            kind: kind.to_string(),
            plural: format!("{}s", kind.to_lowercase()),
            api_version: api_version.to_string(),
        },
    );

    let dyn_obj: kube::core::DynamicObject =
        serde_json::from_value(obj).map_err(kube::Error::SerdeError)?;

    api.create(&kube::api::PostParams::default(), &dyn_obj)
        .await?;
    Ok(())
}

/// Parse apiVersion into (group, version)
fn parse_api_version(api_version: &str) -> (String, String) {
    if let Some(idx) = api_version.rfind('/') {
        (
            api_version[..idx].to_string(),
            api_version[idx + 1..].to_string(),
        )
    } else {
        // Core API (e.g., "v1")
        (String::new(), api_version.to_string())
    }
}

/// Remove machine from cluster (delete CAPI Machine resource)
pub async fn remove_machine_from_cluster(
    resource: &ScheduledMachine,
    client: &Client,
    namespace: &str,
) -> Result<(), ReconcilerError> {
    let name = resource.name_any();
    let cluster_name = &resource.spec.cluster_name;
    let machine_name = format!("{name}-machine");

    info!(
        resource = %name,
        namespace = %namespace,
        cluster = %cluster_name,
        machine_name = %machine_name,
        "Deleting CAPI Machine resource"
    );

    // Get the Machine API
    let machines: Api<kube::core::DynamicObject> = Api::namespaced_with(
        client.clone(),
        namespace,
        &kube::discovery::ApiResource {
            group: CAPI_GROUP.to_string(),
            version: CAPI_MACHINE_API_VERSION.to_string(),
            kind: "Machine".to_string(),
            plural: CAPI_RESOURCE_MACHINES.to_string(),
            api_version: CAPI_MACHINE_API_VERSION_FULL.to_string(),
        },
    );

    // Delete the Machine resource if it exists
    match machines
        .delete(&machine_name, &kube::api::DeleteParams::default())
        .await
    {
        Ok(_) => {
            info!(
                machine_name = %machine_name,
                "CAPI Machine deletion initiated"
            );
            Ok(())
        }
        Err(kube::Error::Api(e)) if e.code == 404 => {
            // Machine already deleted or doesn't exist
            debug!(
                machine_name = %machine_name,
                "CAPI Machine already deleted or does not exist"
            );
            Ok(())
        }
        Err(e) => Err(ReconcilerError::CapiError(format!(
            "Failed to delete Machine {machine_name}: {e}"
        ))),
    }
}

// ============================================================================
// Error policy for controller
// ============================================================================

/// Determine requeue action on error
pub fn error_policy(
    _resource: Arc<ScheduledMachine>,
    error: &ReconcilerError,
    _ctx: Arc<Context>,
) -> Action {
    error!(error = %error, "Reconciliation error");
    Action::requeue(Duration::from_secs(ERROR_REQUEUE_SECS))
}
