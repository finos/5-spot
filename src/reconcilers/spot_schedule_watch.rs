// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # Dynamic spot-schedule provider watch manager (ADR 0006, Phase 3)
//!
//! Makes a spot-schedule provider's `status.active` flip wake the referencing
//! `ScheduledMachine`s at **watch latency**, replacing the pull-on-reconcile
//! resolution of Phase 2 (which only re-checked on the controller's own timer).
//!
//! ## Architecture
//!
//! 1. A standalone `ScheduledMachine` reflector in `main.rs` feeds every SM
//!    apply/delete into [`SpotScheduleWatchManager::observe_scheduled_machine`]
//!    / [`SpotScheduleWatchManager::forget_scheduled_machine`]. On controller
//!    restart the reflector's initial list rebuilds the index from scratch —
//!    nothing is stored outside Kubernetes.
//! 2. The manager keeps a [`ReverseIndex`] mapping each referenced provider
//!    object `(GVK, namespace, name)` back to the set of
//!    `ObjectRef<ScheduledMachine>`s that reference it, plus a per-SM record so
//!    a changed/removed `spec.spotSchedule` updates the index precisely.
//! 3. After every index change the manager reconciles its **per-GVK** dynamic
//!    watcher set against the index's referenced GVKs: it lazily spawns a
//!    `watcher`/`Api<DynamicObject>` stream for a newly-referenced GVK and
//!    aborts the stream for a GVK no longer referenced by any SM.
//! 4. Each provider event is mapped through the reverse index and the resulting
//!    `ObjectRef<ScheduledMachine>`s are pushed onto an `mpsc::Sender` whose
//!    receiver is fed into `Controller::reconcile_on` by `main.rs`.
//!
//! ## Hard edges (ADR 0006 §5)
//!
//! - **Provider CRD installed *after* an SM references it** — discovery fails,
//!   so the per-GVK task retries `pinned_kind` with a fixed back-off
//!   ([`SPOT_SCHEDULE_DISCOVERY_RETRY_SECS`]); meanwhile the SM still resolves
//!   to `Unresolved` on its normal reconciles (Phase 2).
//! - **Provider CRD deleted while watched** — the watch stream ends; the task
//!   loops back to re-resolve after the same back-off.
//! - **Controller restart** — the reflector replays the full SM list, so the
//!   index and watchers are rebuilt from cluster state on boot.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use kube::core::{DynamicObject, GroupVersionKind};
use kube::discovery::pinned_kind;
use kube::runtime::reflector::ObjectRef;
use kube::runtime::watcher;
use kube::{Api, Client, ResourceExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::constants::SPOT_SCHEDULE_DISCOVERY_RETRY_SECS;
use crate::crd::ScheduledMachine;

/// Identity of a referenced provider object: its kind (GVK) plus the
/// namespace/name it lives at. The reverse-index key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProviderKey {
    /// Group/version/kind of the provider resource.
    pub gvk: GroupVersionKind,
    /// Namespace the provider object lives in (the SM's own namespace).
    pub namespace: String,
    /// Name of the provider object.
    pub name: String,
}

/// Maps referenced provider objects back to the `ScheduledMachine`s that
/// reference them. Pure data structure — no I/O — so it is exhaustively
/// unit-testable; the async watcher lifecycle lives in
/// [`SpotScheduleWatchManager`].
#[derive(Debug, Default)]
pub struct ReverseIndex {
    /// `(gvk, ns, name)` → SMs referencing it.
    refs: HashMap<ProviderKey, HashSet<ObjectRef<ScheduledMachine>>>,
    /// SM → the provider key it currently references, so a changed or removed
    /// `spec.spotSchedule` can be applied precisely (remove from the old key
    /// before inserting the new one).
    sm_key: HashMap<ObjectRef<ScheduledMachine>, ProviderKey>,
}

impl ReverseIndex {
    /// Register (or update) `sm`'s reference to provider `key`. If `sm`
    /// previously referenced a different key it is removed from that entry
    /// first, so an SM is never indexed under two providers at once.
    pub fn register(&mut self, sm: ObjectRef<ScheduledMachine>, key: ProviderKey) {
        if let Some(previous) = self.sm_key.get(&sm) {
            if previous != &key {
                self.remove_from_refs(&sm, &previous.clone());
            }
        }
        self.sm_key.insert(sm.clone(), key.clone());
        self.refs.entry(key).or_default().insert(sm);
    }

    /// Remove `sm` from the index entirely (its `spec.spotSchedule` was removed,
    /// or the SM was deleted). No-op if `sm` is not indexed.
    pub fn deregister(&mut self, sm: &ObjectRef<ScheduledMachine>) {
        if let Some(key) = self.sm_key.remove(sm) {
            self.remove_from_refs(sm, &key);
        }
    }

    /// SMs that reference the provider object at `key`.
    #[must_use]
    pub fn lookup(&self, key: &ProviderKey) -> Vec<ObjectRef<ScheduledMachine>> {
        self.refs
            .get(key)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// The distinct set of provider GVKs currently referenced by at least one
    /// SM — the desired watcher set.
    #[must_use]
    pub fn referenced_gvks(&self) -> HashSet<GroupVersionKind> {
        self.refs.keys().map(|key| key.gvk.clone()).collect()
    }

    /// Number of distinct provider objects currently indexed. Test/health use.
    #[must_use]
    pub fn key_count(&self) -> usize {
        self.refs.len()
    }

    fn remove_from_refs(&mut self, sm: &ObjectRef<ScheduledMachine>, key: &ProviderKey) {
        if let Some(set) = self.refs.get_mut(key) {
            set.remove(sm);
            if set.is_empty() {
                self.refs.remove(key);
            }
        }
    }
}

/// Build the [`ProviderKey`] a `ScheduledMachine` references, if any. Returns
/// `None` when the SM has no `spec.spotSchedule`, no namespace, or an
/// `apiVersion` without a `group/version` form (the latter is already rejected
/// at admission, so it is a defensive guard).
#[must_use]
pub fn provider_key_for(sm: &ScheduledMachine) -> Option<ProviderKey> {
    let reference = sm.spec.spot_schedule.as_ref()?;
    let namespace = sm.namespace()?;
    let (group, version) = reference.api_version.split_once('/')?;
    Some(ProviderKey {
        gvk: GroupVersionKind::gvk(group, version, &reference.kind),
        namespace,
        name: reference.name.clone(),
    })
}

/// Owns the reverse index and the lifecycle of one dynamic watcher per
/// referenced provider GVK. Cheaply cloneable via the shared `Arc`.
#[derive(Clone)]
pub struct SpotScheduleWatchManager {
    inner: Arc<Inner>,
}

struct Inner {
    client: Client,
    tx: mpsc::Sender<ObjectRef<ScheduledMachine>>,
    /// Shared with every per-GVK watcher task so provider events can be mapped
    /// back to SMs without a reference cycle to the manager.
    index: Arc<Mutex<ReverseIndex>>,
    watchers: Mutex<HashMap<GroupVersionKind, JoinHandle<()>>>,
}

impl SpotScheduleWatchManager {
    /// Construct a manager. `tx` is the channel into the
    /// `Controller::reconcile_on` stream.
    #[must_use]
    pub fn new(client: Client, tx: mpsc::Sender<ObjectRef<ScheduledMachine>>) -> Self {
        Self {
            inner: Arc::new(Inner {
                client,
                tx,
                index: Arc::new(Mutex::new(ReverseIndex::default())),
                watchers: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Apply one observed `ScheduledMachine` (an apply/init event from the SM
    /// reflector): register or update its provider reference, then reconcile the
    /// watcher set. An SM whose `spec.spotSchedule` is unset is removed from the
    /// index (equivalent to [`Self::forget_scheduled_machine`]).
    pub fn observe_scheduled_machine(&self, sm: &ScheduledMachine) {
        let sm_ref = ObjectRef::from_obj(sm);
        match provider_key_for(sm) {
            Some(key) => self.lock_index().register(sm_ref, key),
            None => self.lock_index().deregister(&sm_ref),
        }
        self.sync_watchers();
    }

    /// Drop one `ScheduledMachine` from the index (a delete event), then
    /// reconcile the watcher set.
    pub fn forget_scheduled_machine(&self, sm: &ScheduledMachine) {
        let sm_ref = ObjectRef::from_obj(sm);
        self.lock_index().deregister(&sm_ref);
        self.sync_watchers();
    }

    /// Number of distinct provider objects currently indexed.
    #[must_use]
    pub fn indexed_key_count(&self) -> usize {
        self.lock_index().key_count()
    }

    /// Number of per-GVK watcher tasks currently tracked.
    #[must_use]
    pub fn watcher_count(&self) -> usize {
        self.inner
            .watchers
            .lock()
            .expect("spot-schedule watcher lock poisoned")
            .len()
    }

    fn lock_index(&self) -> std::sync::MutexGuard<'_, ReverseIndex> {
        self.inner
            .index
            .lock()
            .expect("spot-schedule index lock poisoned")
    }

    /// Reconcile the running per-GVK watchers against the index's referenced
    /// GVKs: abort watchers whose GVK is no longer referenced, spawn watchers
    /// for newly-referenced GVKs. Idempotent.
    fn sync_watchers(&self) {
        let desired = self.lock_index().referenced_gvks();
        let mut watchers = self
            .inner
            .watchers
            .lock()
            .expect("spot-schedule watcher lock poisoned");

        watchers.retain(|gvk, handle| {
            let keep = desired.contains(gvk);
            if !keep {
                handle.abort();
                debug!(
                    ?gvk,
                    "aborted spot-schedule provider watcher (no SM references it)"
                );
            }
            keep
        });

        for gvk in desired {
            if let std::collections::hash_map::Entry::Vacant(slot) = watchers.entry(gvk.clone()) {
                let handle = tokio::spawn(run_provider_watcher(
                    self.inner.client.clone(),
                    gvk,
                    Arc::clone(&self.inner.index),
                    self.inner.tx.clone(),
                ));
                slot.insert(handle);
            }
        }
    }
}

/// Per-GVK watcher loop: resolve the `ApiResource` (retrying while the CRD is
/// absent), watch all objects of that kind, and map each event back to the
/// referencing SMs. Re-resolves after the watch stream ends (e.g. CRD deleted).
/// Exits only when the spawning task is `abort()`-ed.
async fn run_provider_watcher(
    client: Client,
    gvk: GroupVersionKind,
    index: Arc<Mutex<ReverseIndex>>,
    tx: mpsc::Sender<ObjectRef<ScheduledMachine>>,
) {
    let retry = Duration::from_secs(SPOT_SCHEDULE_DISCOVERY_RETRY_SECS);
    info!(?gvk, "spot-schedule provider watcher starting");

    loop {
        let api_resource = match pinned_kind(&client, &gvk).await {
            Ok((api_resource, _capabilities)) => api_resource,
            Err(error) => {
                warn!(
                    ?gvk,
                    %error,
                    "spot-schedule provider CRD not resolvable yet; retrying after back-off"
                );
                tokio::time::sleep(retry).await;
                continue;
            }
        };

        let api: Api<DynamicObject> = Api::all_with(client.clone(), &api_resource);
        let mut stream = watcher::watcher(api, watcher::Config::default()).boxed();
        while let Some(event) = stream.next().await {
            match event {
                Ok(
                    watcher::Event::Apply(object)
                    | watcher::Event::InitApply(object)
                    | watcher::Event::Delete(object),
                ) => emit_refs_for_provider(&gvk, &object, &index, &tx).await,
                Ok(watcher::Event::Init | watcher::Event::InitDone) => {}
                Err(error) => {
                    warn!(?gvk, %error, "spot-schedule provider watcher error; kube-runtime will reconnect");
                }
            }
        }

        warn!(
            ?gvk,
            "spot-schedule provider watch stream ended; re-resolving after back-off"
        );
        tokio::time::sleep(retry).await;
    }
}

/// Map one provider object event to its referencing SMs and enqueue each for
/// reconciliation. Objects with no namespace/name (malformed) are ignored.
async fn emit_refs_for_provider(
    gvk: &GroupVersionKind,
    object: &DynamicObject,
    index: &Arc<Mutex<ReverseIndex>>,
    tx: &mpsc::Sender<ObjectRef<ScheduledMachine>>,
) {
    let (Some(namespace), Some(name)) = (object.namespace(), object.metadata.name.clone()) else {
        return;
    };
    let key = ProviderKey {
        gvk: gvk.clone(),
        namespace,
        name,
    };
    let refs = index
        .lock()
        .expect("spot-schedule index lock poisoned")
        .lookup(&key);
    for sm_ref in refs {
        if tx.send(sm_ref).await.is_err() {
            debug!("controller reconcile_on receiver dropped; skipping provider→SM dispatch");
            return;
        }
    }
}

#[cfg(test)]
#[path = "spot_schedule_watch_tests.rs"]
mod tests;
