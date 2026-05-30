// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
//! # Per-child-cluster Node watcher manager
//!
//! Closes the event-driven gap left by the Phase 1 resolver work: now
//! that `Node` and `Pod` operations are correctly routed to the child
//! cluster, this module owns the inverse direction — child-cluster
//! `Node` events being routed back to the management cluster's
//! `Controller` so an SM is reconciled when its bound Node changes
//! `Ready`/`Unschedulable`/etc.
//!
//! ## Architecture
//!
//! 1. The manager implements [`crate::reconcilers::child_client::ChildWatchHook`]
//!    and is installed on the cache via
//!    `ChildClientCache::set_hook` at startup.
//! 2. When the cache builds (or rebuilds) a child `kube::Client` for a
//!    given `CacheKey`, the manager spawns a `kube::runtime::watcher`
//!    against `Api::<Node>::all(child_client)` and runs it in a Tokio
//!    task. Each `Apply` / `InitApply` / `Delete` event is mapped via
//!    [`crate::reconcilers::node_to_scheduled_machines_via_machine`]
//!    using a snapshot of the management cluster's CAPI Machine
//!    reflector store — the canonical, tenant-unforgeable Node→SM
//!    mapping path established by the 2026-04-25 security audit.
//! 3. Mapped `ObjectRef<ScheduledMachine>`s are pushed onto an
//!    `mpsc::Sender` whose corresponding `Receiver` is fed into
//!    `Controller::reconcile_on` by `main.rs`.
//! 4. On cache eviction (LRU or RV-change rotation), the manager
//!    `JoinHandle::abort()`s the running task. The watcher is
//!    `Send + 'static`, abort-safe at every await point (kube-runtime
//!    streams + Tokio mpsc both are), so abort never strands resources.
//!
//! ## Why one watcher per Secret, not per SM
//!
//! Two `ScheduledMachine`s in the same namespace pointing at the same
//! `kubeconfigSecretRef` (or both relying on the same
//! `<clusterName>-kubeconfig` auto-discovery) share one cached client
//! AND one Node watcher. Per-SM watchers would multiply the watch
//! count linearly with SM count; per-Secret watchers cap it at the
//! number of distinct child clusters.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use k8s_openapi::api::core::v1::Node;
use kube::core::DynamicObject;
use kube::runtime::reflector;
use kube::runtime::reflector::ObjectRef;
use kube::runtime::watcher;
use kube::{Api, Client};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::crd::ScheduledMachine;
use crate::reconcilers::child_client::{CacheKey, ChildWatchHook};
use crate::reconcilers::node_to_scheduled_machines_via_machine;

/// Manages the lifecycle of one per-child-cluster `Node` watcher per
/// active `CacheKey`. Implements [`ChildWatchHook`] so the
/// [`crate::reconcilers::child_client::ChildClientCache`] drives start /
/// cancel events directly.
///
/// Cheaply cloneable via the shared `Arc`-wrapped state.
#[derive(Clone)]
pub struct ChildNodeWatchManager {
    inner: Arc<Inner>,
}

struct Inner {
    tasks: Mutex<HashMap<CacheKey, JoinHandle<()>>>,
    tx: mpsc::Sender<ObjectRef<ScheduledMachine>>,
    machine_store: reflector::Store<DynamicObject>,
}

impl ChildNodeWatchManager {
    /// Construct a fresh manager. `tx` is the channel into the
    /// `Controller::reconcile_on` stream; `machine_store` is the
    /// management cluster's CAPI Machine reflector that the canonical
    /// Node→SM mapper reads from.
    #[must_use]
    pub fn new(
        tx: mpsc::Sender<ObjectRef<ScheduledMachine>>,
        machine_store: reflector::Store<DynamicObject>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                tasks: Mutex::new(HashMap::new()),
                tx,
                machine_store,
            }),
        }
    }

    /// Number of currently-running watcher tasks. Exposed for tests +
    /// future Phase 2 health endpoint.
    #[must_use]
    pub fn task_count(&self) -> usize {
        self.inner
            .tasks
            .lock()
            .expect("child watch manager lock poisoned")
            .len()
    }

    /// `true` iff a watcher task is currently tracked (running or just
    /// recently aborted but not yet pruned) for the given `CacheKey`.
    /// Tests use this to assert lifecycle ordering.
    #[must_use]
    pub fn has_watcher(&self, key: &CacheKey) -> bool {
        self.inner
            .tasks
            .lock()
            .expect("child watch manager lock poisoned")
            .contains_key(key)
    }
}

impl ChildWatchHook for ChildNodeWatchManager {
    fn on_child_resolved(&self, key: &CacheKey, client: Client) {
        // Defensive cancel-then-start: if a stale task exists (shouldn't
        // happen because the cache fires `on_child_evicted` before
        // re-resolving the same key, but the invariant is cheap to
        // enforce), drop it. JoinHandle::drop does NOT abort, so call
        // abort() explicitly.
        let mut guard = self
            .inner
            .tasks
            .lock()
            .expect("child watch manager lock poisoned");
        if let Some(existing) = guard.remove(key) {
            existing.abort();
        }

        let tx = self.inner.tx.clone();
        let machine_store = self.inner.machine_store.clone();
        let key_owned = key.clone();
        let api: Api<Node> = Api::all(client);
        let join = tokio::spawn(run_node_watcher(api, tx, machine_store, key_owned));
        guard.insert(key.clone(), join);

        debug!(
            namespace = %key.namespace,
            secret = %key.secret_name,
            "spawned child-cluster Node watcher"
        );
    }

    fn on_child_evicted(&self, key: &CacheKey) {
        let mut guard = self
            .inner
            .tasks
            .lock()
            .expect("child watch manager lock poisoned");
        if let Some(join) = guard.remove(key) {
            join.abort();
            debug!(
                namespace = %key.namespace,
                secret = %key.secret_name,
                "aborted child-cluster Node watcher (cache eviction)"
            );
        }
    }
}

/// The watcher loop body. Pure async fn so it's easy to spawn — see
/// [`ChildNodeWatchManager::on_child_resolved`].
///
/// Exits when:
/// - the spawning task is `abort()`-ed (Tokio cancels at the next
///   await point), or
/// - the underlying `kube::runtime::watcher` stream terminates
///   (rare — `watcher::watcher` reconnects internally on transient
///   errors, so this typically only happens on terminal auth failure).
async fn run_node_watcher(
    api: Api<Node>,
    tx: mpsc::Sender<ObjectRef<ScheduledMachine>>,
    machine_store: reflector::Store<DynamicObject>,
    key: CacheKey,
) {
    info!(
        namespace = %key.namespace,
        secret = %key.secret_name,
        "child-cluster Node watcher starting"
    );
    let mut stream = watcher::watcher(api, watcher::Config::default()).boxed();
    while let Some(evt) = stream.next().await {
        match evt {
            Ok(watcher::Event::Apply(node) | watcher::Event::InitApply(node)) => {
                emit_refs_for_node(&node, &machine_store, &tx).await;
            }
            Ok(watcher::Event::Delete(node)) => {
                emit_refs_for_node(&node, &machine_store, &tx).await;
            }
            // `Init` and `InitDone` are bookkeeping for the initial
            // list — no Node payload to map. The first concrete event
            // is `InitApply`, which we handle above.
            Ok(watcher::Event::Init | watcher::Event::InitDone) => {}
            Err(e) => {
                warn!(
                    namespace = %key.namespace,
                    secret = %key.secret_name,
                    error = %e,
                    "child-cluster Node watcher error; kube-runtime will reconnect"
                );
            }
        }
    }
    info!(
        namespace = %key.namespace,
        secret = %key.secret_name,
        "child-cluster Node watcher stream ended"
    );
}

/// Map one Node event into zero-or-more `ObjectRef<ScheduledMachine>`s
/// via the canonical CAPI Machine ownership chain and send each ref
/// on the manager's mpsc. Failure to send (receiver dropped) is logged
/// at debug — that means the controller is shutting down; the next
/// shutdown signal will tear the watcher down anyway.
async fn emit_refs_for_node(
    node: &Node,
    machine_store: &reflector::Store<DynamicObject>,
    tx: &mpsc::Sender<ObjectRef<ScheduledMachine>>,
) {
    let snapshot = machine_store.state();
    let refs = node_to_scheduled_machines_via_machine(node, snapshot.iter().map(AsRef::as_ref));
    for r in refs {
        if tx.send(r).await.is_err() {
            debug!("controller reconcile_on receiver dropped; skipping Node→SM ref dispatch");
            return;
        }
    }
}

#[cfg(test)]
#[path = "child_watch_tests.rs"]
mod child_watch_tests;
