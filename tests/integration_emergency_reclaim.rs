// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: Apache-2.0
//! # Integration test: agent → controller annotation contract on a real cluster
//!
//! Scope cut, deliberately: full 7-step `EmergencyRemove` flow needs a
//! ScheduledMachine + CAPI Machine + drain target — out of scope for a
//! kind-cluster smoke test. What we *can* prove against a real API
//! server with no operator running is the **annotation contract** — the
//! single load-bearing surface between the node-side agent and the
//! controller. If the agent's `build_patch_body` writes annotations
//! that the controller's `node_reclaim_request` cannot parse, the whole
//! flow fails silently. This test catches that class of regression.
//!
//! Mirrors the pattern in `tests/integration_node_taints.rs` —
//! graceful skip when no cluster is reachable, picks an arbitrary
//! Ready Node, mutates only test-owned annotations, and scrubs on
//! exit.
//!
//! ## Running
//!
//! ```bash
//! kind create cluster                       # or any reachable cluster
//! cargo test --test integration_emergency_reclaim -- --ignored --test-threads=1
//! ```
//!
//! ## Safety — DO NOT run against a cluster with the 5-spot operator installed
//!
//! The annotation keys are the production keys. If the operator is
//! running, this test could trigger a real `EmergencyRemove` flow on
//! the chosen Node. The cleanup guard scrubs on exit, but the operator
//! may have already started draining. Use a kind cluster without the
//! operator deployed.

use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::Node;
use kube::api::{Patch, PatchParams};
use kube::{Api, Client};
use serde_json::json;

use five_spot::constants::{
    RECLAIM_REASON_ANNOTATION, RECLAIM_REQUESTED_ANNOTATION, RECLAIM_REQUESTED_AT_ANNOTATION,
    RECLAIM_REQUESTED_VALUE,
};
use five_spot::reclaim_agent::{build_patch_body, Match, MatchSource};
use five_spot::reconcilers::node_reclaim_request;

const TEST_FIELD_MANAGER: &str = "5spot-integration-test-emergency-reclaim";

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

async fn clear_test_annotations(nodes: &Api<Node>, name: &str) {
    let patch = json!({
        "metadata": {
            "annotations": {
                RECLAIM_REQUESTED_ANNOTATION: serde_json::Value::Null,
                RECLAIM_REASON_ANNOTATION: serde_json::Value::Null,
                RECLAIM_REQUESTED_AT_ANNOTATION: serde_json::Value::Null,
            }
        }
    });
    let _ = nodes
        .patch(
            name,
            &PatchParams::apply(TEST_FIELD_MANAGER).force(),
            &Patch::Merge(&patch),
        )
        .await;
}

/// End-to-end: agent's annotation patch lands on a real Node, controller's
/// parser reads it back as a typed `ReclaimRequest` whose fields match.
#[tokio::test]
#[ignore]
async fn agent_annotation_round_trips_through_controller_parser() {
    let Some(client) = client_or_skip().await else {
        return;
    };
    let Some(node_name) = pick_ready_node(&client).await else {
        eprintln!("SKIP: no Ready node found in cluster");
        return;
    };
    let nodes: Api<Node> = Api::all(client.clone());

    // Always scrub on entry so a previous failed run doesn't pollute.
    clear_test_annotations(&nodes, &node_name).await;

    // Build what the agent would PATCH on first match.
    let m = Match {
        pid: 99999,
        matched_pattern: "integration-test-java".to_string(),
        source: MatchSource::Comm,
    };
    let ts = "2026-05-02T23:30:00Z";
    let patch = build_patch_body(&m, ts);

    // Apply with a unique field manager so we own the annotations cleanly.
    nodes
        .patch(
            &node_name,
            &PatchParams::apply(TEST_FIELD_MANAGER).force(),
            &Patch::Merge(&patch),
        )
        .await
        .expect("PATCH reclaim annotations");

    // Re-fetch the Node and feed it through the controller's parser.
    let fetched = nodes.get(&node_name).await.expect("re-fetch node");
    let parsed = node_reclaim_request(&fetched);

    // Cleanup runs regardless of assertion outcome.
    let result = std::panic::catch_unwind(|| {
        let req = parsed.expect("controller must parse the annotations the agent wrote");
        // The reason format is "<source>: <pattern>" per build_patch_body.
        let reason = req.reason.expect("reason annotation present");
        assert!(
            reason.contains("integration-test-java"),
            "reason must include the matched pattern; got: {reason}"
        );
        assert_eq!(
            req.requested_at.as_deref(),
            Some(ts),
            "timestamp annotation must round-trip verbatim"
        );
    });

    clear_test_annotations(&nodes, &node_name).await;

    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

/// `node_reclaim_request` returns `None` when the requested annotation
/// is absent or set to a value other than the literal "true" sentinel.
/// Proves the parser refuses to fire on partial / stale state — a
/// missing requested annotation means "no request right now," even if
/// the reason or timestamp annotations linger from a prior cleanup.
#[tokio::test]
#[ignore]
async fn parser_returns_none_for_partial_annotations() {
    let Some(client) = client_or_skip().await else {
        return;
    };
    let Some(node_name) = pick_ready_node(&client).await else {
        eprintln!("SKIP: no Ready node found in cluster");
        return;
    };
    let nodes: Api<Node> = Api::all(client.clone());

    clear_test_annotations(&nodes, &node_name).await;

    // Set ONLY the reason — without the requested=true sentinel, the
    // controller must not interpret this as a live reclaim request.
    let partial = json!({
        "metadata": {
            "annotations": {
                RECLAIM_REASON_ANNOTATION: "process-match: stale",
            }
        }
    });
    nodes
        .patch(
            &node_name,
            &PatchParams::apply(TEST_FIELD_MANAGER).force(),
            &Patch::Merge(&partial),
        )
        .await
        .expect("PATCH partial annotations");

    let fetched = nodes.get(&node_name).await.expect("re-fetch node");

    let result = std::panic::catch_unwind(|| {
        let parsed = node_reclaim_request(&fetched);
        assert!(
            parsed.is_none(),
            "parser must return None when requested=true is absent; got {parsed:?}"
        );
    });

    clear_test_annotations(&nodes, &node_name).await;

    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

/// Read-only check that the production annotation keys we use here
/// match the constants the rest of the codebase imports. Pinned so a
/// rename in `src/constants.rs` is caught at the integration boundary
/// instead of silently breaking the agent/controller contract.
#[test]
fn annotation_keys_match_published_contract() {
    let _: BTreeMap<&str, &str> = [
        (
            RECLAIM_REQUESTED_ANNOTATION,
            "5spot.finos.org/reclaim-requested",
        ),
        (RECLAIM_REASON_ANNOTATION, "5spot.finos.org/reclaim-reason"),
        (
            RECLAIM_REQUESTED_AT_ANNOTATION,
            "5spot.finos.org/reclaim-requested-at",
        ),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        RECLAIM_REQUESTED_ANNOTATION,
        "5spot.finos.org/reclaim-requested"
    );
    assert_eq!(RECLAIM_REASON_ANNOTATION, "5spot.finos.org/reclaim-reason");
    assert_eq!(
        RECLAIM_REQUESTED_AT_ANNOTATION,
        "5spot.finos.org/reclaim-requested-at"
    );
    assert_eq!(RECLAIM_REQUESTED_VALUE, "true");
}
