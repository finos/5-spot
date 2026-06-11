// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # Integration test: kata-config delivery contract + host-file lifecycle
//!
//! Scope cut, deliberately (mirrors `tests/integration_emergency_reclaim.rs`):
//! a full delivery — controller labels the Node, the DaemonSet schedules, the
//! agent writes the host file and bounces the k0s service — needs a real k0s
//! node and is exercised manually per the operator guide. What we *can* prove
//! here is:
//!
//! 1. **The annotation contract** (always runs, no cluster needed): the
//!    controller's `build_kata_config_ref_annotation_patch` writes a value the
//!    agent's `parse_kata_ref` parses field-for-field. This is the single
//!    load-bearing surface between the two processes (ADR 0002) — a drift here
//!    fails the whole feature silently.
//! 2. **The host-file lifecycle** (always runs, tempdir-backed): write →
//!    restart-once → drift-heal-without-restart → new-content-restart →
//!    tear-down, end-to-end through `sync_content` + the restart-loop guard
//!    (ADR 0003), with a counting `RestartExecutor` in place of `nsenter`.
//! 3. **The Node round-trip** (`--ignored`, needs a cluster): the controller's
//!    ref-annotation + opt-in-label patches land on a real Node and read back
//!    through the agent's parser.
//!
//! ## Running the cluster-backed test
//!
//! ```bash
//! kind create cluster                       # or any reachable cluster
//! cargo test --test integration_kata_config -- --ignored --test-threads=1
//! ```
//!
//! ## Safety — DO NOT run the ignored test against a cluster with the 5-spot
//! operator + kata-config DaemonSet installed: the label/annotation keys are
//! the production keys, so a live agent would treat the fixture as a real
//! delivery request. The cleanup guard scrubs on exit, but use a kind cluster
//! without the operator deployed.

use k8s_openapi::api::core::v1::Node;
use kube::api::{Patch, PatchParams};
use kube::{Api, Client};

use five_spot::constants::{KATA_CONFIG_LABEL, KATA_CONFIG_REF_ANNOTATION};
use five_spot::crd::{KataConfig, KataConfigSourceKind};
use five_spot::kata_config_agent::{
    intended_hash_for, is_drift_correction, needs_restart, parse_kata_ref, restart_if_needed,
    sync_content, RestartExecutor, SyncOutcome, ABSENT_HASH_MARKER,
};
use five_spot::reconcilers::{
    build_kata_config_label_patch, build_kata_config_ref_annotation_patch,
};

const TEST_FIELD_MANAGER: &str = "5spot-integration-test-kata-config";

/// The KataConfig fixture used across all three layers. No destPath — the
/// host location is the fixed `KATA_CONFIG_DEST_PATH` constant (ADR 0005).
fn fixture_kata_config() -> KataConfig {
    KataConfig {
        kind: KataConfigSourceKind::ConfigMap,
        name: "kata-drop-in".to_string(),
        namespace: "5spot-system".to_string(),
        key: "kata-containers.toml".to_string(),
        restart_service: "k0sworker.service".to_string(),
    }
}

/// Extract the ref-annotation string value out of the controller's merge-patch
/// body, exactly as the API server would store it on the Node.
fn ref_annotation_value(patch: &serde_json::Value) -> String {
    patch["metadata"]["annotations"][KATA_CONFIG_REF_ANNOTATION]
        .as_str()
        .expect("ref annotation must be a JSON string value")
        .to_string()
}

/// Counting fake standing in for the `nsenter` executor.
struct CountingExecutor {
    calls: std::cell::RefCell<Vec<String>>,
}

impl CountingExecutor {
    fn new() -> Self {
        Self {
            calls: std::cell::RefCell::new(Vec::new()),
        }
    }

    fn count(&self) -> usize {
        self.calls.borrow().len()
    }
}

impl RestartExecutor for CountingExecutor {
    fn restart(&self, service: &str) -> std::io::Result<()> {
        self.calls.borrow_mut().push(service.to_string());
        Ok(())
    }
}

/// Layer 1 — the controller's annotation patch parses through the agent's
/// parser with every field intact (the ADR 0002 contract).
#[test]
fn controller_ref_annotation_parses_with_agent_parser() {
    let kata = fixture_kata_config();
    let patch = build_kata_config_ref_annotation_patch(Some(&kata));
    let raw = ref_annotation_value(&patch);

    let parsed = parse_kata_ref(&raw).expect("agent must parse the controller's annotation value");
    assert_eq!(parsed.namespace, kata.namespace);
    assert_eq!(parsed.kind, "ConfigMap");
    assert_eq!(parsed.name, kata.name);
    assert_eq!(parsed.key, kata.key);
    assert_eq!(parsed.restart_service, kata.restart_service);
    assert!(
        !raw.contains("destPath"),
        "the ref annotation must carry NO host path (ADR 0005): {raw}"
    );
}

/// Layer 1 — clearing: the tear-down patch sets the annotation to JSON null so
/// merge-patch removes the key on the Node.
#[test]
fn controller_clear_patch_nulls_the_ref_annotation() {
    let patch = build_kata_config_ref_annotation_patch(None);
    assert!(
        patch["metadata"]["annotations"][KATA_CONFIG_REF_ANNOTATION].is_null(),
        "clear patch must use JSON null so merge-patch deletes the key"
    );
}

/// Layer 2 — full host-file lifecycle in a tempdir: first provision restarts
/// exactly once, an in-sync tick is a no-op, an out-of-band edit is rewritten
/// WITHOUT a second restart, new content restarts again, and source removal
/// tears the file down with a final restart for the present → absent
/// transition (ADR 0003).
#[test]
fn host_file_lifecycle_smoke() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("containerd.d/kata.toml");
    let exec = CountingExecutor::new();
    let service = "k0sworker.service";

    // --- First provision: write + restart exactly once.
    let v1 = b"[plugins]\nv = 1\n";
    let outcome = sync_content(Some(v1), &dest).unwrap();
    assert!(matches!(outcome, SyncOutcome::Wrote(_)));
    let mut applied = intended_hash_for(&outcome);
    assert!(restart_if_needed(&exec, None, &applied, service).unwrap());
    assert_eq!(exec.count(), 1, "first provision restarts exactly once");

    // --- Steady state: in-sync tick, no write, no restart.
    let outcome = sync_content(Some(v1), &dest).unwrap();
    assert!(matches!(outcome, SyncOutcome::Unchanged(Some(_))));
    let intended = intended_hash_for(&outcome);
    assert!(!needs_restart(Some(&applied), &intended));
    assert!(!restart_if_needed(&exec, Some(&applied), &intended, service).unwrap());
    assert_eq!(exec.count(), 1, "in-sync tick must not restart");

    // --- Drift: out-of-band edit is rewritten but NOT re-restarted.
    std::fs::write(&dest, b"tampered out-of-band").unwrap();
    let outcome = sync_content(Some(v1), &dest).unwrap();
    assert!(matches!(outcome, SyncOutcome::Wrote(_)));
    assert!(
        is_drift_correction(&outcome, Some(&applied)),
        "rewriting already-applied content is a drift correction"
    );
    let intended = intended_hash_for(&outcome);
    assert!(!restart_if_needed(&exec, Some(&applied), &intended, service).unwrap());
    assert_eq!(
        exec.count(),
        1,
        "drift correction must not bounce the service"
    );
    assert_eq!(std::fs::read(&dest).unwrap(), v1, "content healed");

    // --- New content: write + second restart.
    let v2 = b"[plugins]\nv = 2\n";
    let outcome = sync_content(Some(v2), &dest).unwrap();
    assert!(matches!(outcome, SyncOutcome::Wrote(_)));
    assert!(
        !is_drift_correction(&outcome, Some(&applied)),
        "a new hash is a rollout, not drift"
    );
    let intended = intended_hash_for(&outcome);
    assert!(restart_if_needed(&exec, Some(&applied), &intended, service).unwrap());
    assert_eq!(exec.count(), 2, "new content restarts again");
    applied = intended;

    // --- Tear-down: source gone ⇒ file unlinked, restart for present → absent.
    let outcome = sync_content(None, &dest).unwrap();
    assert_eq!(outcome, SyncOutcome::Deleted);
    let intended = intended_hash_for(&outcome);
    assert_eq!(intended, ABSENT_HASH_MARKER);
    assert!(restart_if_needed(&exec, Some(&applied), &intended, service).unwrap());
    assert_eq!(
        exec.count(),
        3,
        "tear-down restarts so containerd drops the config"
    );
    assert!(!dest.exists(), "GitOps: absent in source ⇒ absent on host");

    // --- Idempotent tear-down: already absent, no restart.
    let outcome = sync_content(None, &dest).unwrap();
    assert_eq!(outcome, SyncOutcome::Unchanged(None));
    let final_intended = intended_hash_for(&outcome);
    assert!(!restart_if_needed(&exec, Some(&intended), &final_intended, service).unwrap());
    assert_eq!(exec.count(), 3, "repeat tear-down tick is a no-op");
}

// ============================================================================
// Cluster-backed round-trip (ignored — needs a reachable cluster)
// ============================================================================

async fn client_or_skip() -> Option<Client> {
    match Client::try_default().await {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("SKIP: no reachable cluster ({e}); run against kind to exercise this test");
            None
        }
    }
}

async fn pick_ready_node(client: &Client) -> Option<String> {
    let nodes: Api<Node> = Api::all(client.clone());
    let list = match nodes.list(&Default::default()).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("SKIP: failed to list nodes ({e})");
            return None;
        }
    };
    for n in list.items {
        let name = n.metadata.name.clone()?;
        let ready = n
            .status
            .as_ref()
            .and_then(|s| s.conditions.as_ref())
            .map(|cs| cs.iter().any(|c| c.type_ == "Ready" && c.status == "True"))
            .unwrap_or(false);
        if ready {
            return Some(name);
        }
    }
    None
}

/// Scrub the kata ref annotation + opt-in label using the controller's own
/// tear-down patches.
async fn clear_kata_optin(nodes: &Api<Node>, name: &str) {
    for patch in [
        build_kata_config_ref_annotation_patch(None),
        build_kata_config_label_patch(false),
    ] {
        let _ = nodes
            .patch(
                name,
                &PatchParams::apply(TEST_FIELD_MANAGER).force(),
                &Patch::Merge(&patch),
            )
            .await;
    }
}

/// Layer 3 — the controller's delivery patches land on a real Node and the ref
/// annotation reads back through the agent's parser; the opt-in label is set
/// to the production value the DaemonSet nodeSelector matches.
#[tokio::test]
#[ignore]
async fn ref_annotation_round_trips_through_real_node() {
    let Some(client) = client_or_skip().await else {
        return;
    };
    let Some(node_name) = pick_ready_node(&client).await else {
        eprintln!("SKIP: no Ready node found in cluster");
        return;
    };
    let nodes: Api<Node> = Api::all(client.clone());

    // Always scrub on entry so a previous failed run doesn't pollute.
    clear_kata_optin(&nodes, &node_name).await;

    let kata = fixture_kata_config();
    for patch in [
        build_kata_config_ref_annotation_patch(Some(&kata)),
        build_kata_config_label_patch(true),
    ] {
        nodes
            .patch(
                &node_name,
                &PatchParams::apply(TEST_FIELD_MANAGER).force(),
                &Patch::Merge(&patch),
            )
            .await
            .expect("PATCH kata delivery state onto Node");
    }

    let fetched = nodes.get(&node_name).await.expect("re-fetch node");

    // Cleanup runs regardless of assertion outcome.
    let result = std::panic::catch_unwind(|| {
        let anns = fetched.metadata.annotations.clone().unwrap_or_default();
        let raw = anns
            .get(KATA_CONFIG_REF_ANNOTATION)
            .expect("ref annotation must be present on the Node");
        let parsed = parse_kata_ref(raw).expect("agent must parse the stored annotation");
        assert_eq!(parsed.name, kata.name);
        assert_eq!(parsed.restart_service, kata.restart_service);

        let labels = fetched.metadata.labels.clone().unwrap_or_default();
        assert!(
            labels.contains_key(KATA_CONFIG_LABEL),
            "opt-in label must be set so the DaemonSet schedules here"
        );
    });

    clear_kata_optin(&nodes, &node_name).await;

    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}
