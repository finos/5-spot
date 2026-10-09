// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # Dynamic reference watch manager (ADR 0006 Phase 3, generalized by ADR 0011)
//!
//! Makes a *referenced* object's change wake the objects that reference it, at
//! watch latency rather than on a timer. Originally built so a spot-schedule
//! provider's `status.active` flip would wake the referencing
//! `ScheduledMachine`s; generalized when ADR 0011 added a second consumer with
//! **two** reference edges of its own.
//!
//! ## Generic over the referencing kind
//!
//! [`DynamicRefWatchManager<K>`] is parameterised by the kind that *holds* the
//! reference, and takes a `key_for` extractor saying which object a given `K`
//! points at. That yields three uses from one implementation:
//!
//! | Manager | References | Woken by |
//! | --- | --- | --- |
//! | `DynamicRefWatchManager<ScheduledMachine>` | `spec.schedule` | provider `status.active` |
//! | `DynamicRefWatchManager<ScheduledCapacity>` | `spec.schedule` | provider `status.active` |
//! | `DynamicRefWatchManager<ScheduledCapacity>` | `spec.targetRef` | the target's `drainedPath` |
//!
//! The third is why this is generic at all: a capacity handback waits for a
//! consumer's in-use counter to reach zero, and polling for that would violate
//! the event-driven rule the same way polling a schedule would.
//!
//! ## Architecture
//!
//! 1. A standalone reflector in the owning binary feeds every apply/delete of
//!    `K` into [`DynamicRefWatchManager::observe`] /
//!    [`DynamicRefWatchManager::forget`]. On restart the reflector's initial
//!    list rebuilds the index from scratch: nothing is stored outside
//!    Kubernetes.
//! 2. The manager keeps a [`ReverseIndex<K>`] mapping each referenced object
//!    `(GVK, namespace, name)` back to the set of `ObjectRef<K>`s that
//!    reference it, plus a per-`K` record so a changed or removed reference
//!    updates the index precisely.
//! 3. After every index change the manager reconciles its **per-GVK** dynamic
//!    watcher set against the index's referenced GVKs: it lazily spawns a
//!    `watcher`/`Api<DynamicObject>` stream for a newly-referenced GVK and
//!    aborts the stream for a GVK no longer referenced by anything.
//! 4. Each event is mapped through the reverse index and the resulting
//!    `ObjectRef<K>`s are pushed onto an `mpsc::Sender` whose receiver is fed
//!    into `Controller::reconcile_on`.
//!
//! ## Hard edges (ADR 0006 §5)
//!
//! - **Referenced CRD installed *after* something references it** - discovery
//!   fails, so the per-GVK task retries `pinned_kind` with a fixed back-off
//!   ([`SPOT_SCHEDULE_DISCOVERY_RETRY_SECS`]); meanwhile the referencing object
//!   still resolves to `Unresolved` on its normal reconciles.
//! - **Referenced CRD deleted while watched** - the watch stream ends; the task
//!   loops back to re-resolve after the same back-off.
//! - **Restart** - the reflector replays the full list, so the index and
//!   watchers are rebuilt from cluster state on boot.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use kube::core::{DynamicObject, GroupVersionKind};
use kube::discovery::pinned_kind;
use kube::runtime::reflector::{Lookup, ObjectRef};
use kube::runtime::watcher;
use kube::{Api, Client, ResourceExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use std::hash::Hash;

use crate::constants::SPOT_SCHEDULE_DISCOVERY_RETRY_SECS;
use crate::crd::{ScheduledCapacity, ScheduledMachine};

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

/// Maps referenced objects back to the `K`s that reference them. Pure data
/// structure, no I/O, so it is exhaustively unit-testable; the async watcher
/// lifecycle lives in [`DynamicRefWatchManager`].
pub struct ReverseIndex<K>
where
    K: Lookup + ?Sized,
    K::DynamicType: Hash + Eq,
{
    /// `(gvk, ns, name)` -> the objects referencing it.
    refs: HashMap<ProviderKey, HashSet<ObjectRef<K>>>,
    /// Referencing object -> the key it currently references, so a changed or
    /// removed reference can be applied precisely (remove from the old key
    /// before inserting the new one).
    owner_key: HashMap<ObjectRef<K>, ProviderKey>,
}

// Both impls below are hand-written rather than derived. `derive(Default)` would
// demand `K: Default`, which no Kubernetes object satisfies and none needs to,
// and `derive(Debug)` would demand `K::DynamicType: Debug` from every caller to
// print an index whose contents are not the interesting part anyway: the counts
// are what a log line or a test wants.
impl<K> std::fmt::Debug for ReverseIndex<K>
where
    K: Lookup + ?Sized,
    K::DynamicType: Hash + Eq,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReverseIndex")
            .field("referenced_objects", &self.refs.len())
            .field("referencing_objects", &self.owner_key.len())
            .finish()
    }
}

impl<K> Default for ReverseIndex<K>
where
    K: Lookup + ?Sized,
    K::DynamicType: Hash + Eq,
{
    fn default() -> Self {
        Self {
            refs: HashMap::new(),
            owner_key: HashMap::new(),
        }
    }
}

impl<K> ReverseIndex<K>
where
    K: Lookup + ?Sized,
    K::DynamicType: Hash + Eq + Clone,
{
    /// Register (or update) `owner`'s reference to `key`. If `owner`
    /// previously referenced a different key it is removed from that entry
    /// first, so one object is never indexed under two keys at once.
    pub fn register(&mut self, owner: ObjectRef<K>, key: ProviderKey) {
        if let Some(previous) = self.owner_key.get(&owner) {
            if previous != &key {
                self.remove_from_refs(&owner, &previous.clone());
            }
        }
        self.owner_key.insert(owner.clone(), key.clone());
        self.refs.entry(key).or_default().insert(owner);
    }

    /// Remove `owner` from the index entirely (its reference was removed, or
    /// the object was deleted). No-op if `owner` is not indexed.
    pub fn deregister(&mut self, owner: &ObjectRef<K>) {
        if let Some(key) = self.owner_key.remove(owner) {
            self.remove_from_refs(owner, &key);
        }
    }

    /// The objects that reference the object at `key`.
    #[must_use]
    pub fn lookup(&self, key: &ProviderKey) -> Vec<ObjectRef<K>> {
        self.refs
            .get(key)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// The distinct set of GVKs currently referenced by at least one object:
    /// the desired watcher set.
    #[must_use]
    pub fn referenced_gvks(&self) -> HashSet<GroupVersionKind> {
        self.refs.keys().map(|key| key.gvk.clone()).collect()
    }

    /// Number of distinct referenced objects currently indexed. Test/health use.
    #[must_use]
    pub fn key_count(&self) -> usize {
        self.refs.len()
    }

    fn remove_from_refs(&mut self, owner: &ObjectRef<K>, key: &ProviderKey) {
        if let Some(set) = self.refs.get_mut(key) {
            set.remove(owner);
            if set.is_empty() {
                self.refs.remove(key);
            }
        }
    }
}

/// Build the [`ProviderKey`] a `ScheduledMachine` references, if any. Returns
/// `None` when the SM has no namespace or an `apiVersion` without a
/// `group/version` form (the latter is already rejected at admission, so it is a
/// defensive guard). `spec.schedule` itself is required since ADR 0009.
#[must_use]
pub fn provider_key_for(sm: &ScheduledMachine) -> Option<ProviderKey> {
    let reference = &sm.spec.schedule;
    let namespace = ResourceExt::namespace(sm)?;
    let (group, version) = reference.api_version.split_once('/')?;
    Some(ProviderKey {
        gvk: GroupVersionKind::gvk(group, version, &reference.kind),
        namespace,
        name: reference.name.clone(),
    })
}

/// The [`ProviderKey`] a `ScheduledCapacity`'s `spec.schedule` references.
/// Same shape and same guards as [`provider_key_for`], against the same
/// required [`SpotScheduleRef`](crate::crd::SpotScheduleRef) field.
#[must_use]
pub fn capacity_schedule_key_for(capacity: &ScheduledCapacity) -> Option<ProviderKey> {
    let reference = &capacity.spec.schedule;
    let namespace = ResourceExt::namespace(capacity)?;
    let (group, version) = reference.api_version.split_once('/')?;
    Some(ProviderKey {
        gvk: GroupVersionKind::gvk(group, version, &reference.kind),
        namespace,
        name: reference.name.clone(),
    })
}

/// The [`ProviderKey`] a `ScheduledCapacity`'s `spec.targetRef` references:
/// the **object whose capacity field 5-Spot owns and scales** (ADR 0011,
/// reshaped by ADR 0016).
///
/// Watching it is what makes handback event-driven. The controller writes zero
/// and then waits for the consumer's `spec.handback.drainedPath` to reach zero;
/// without this watch it would have to poll to notice, which the event-driven
/// rule forbids. The periodic requeue that remains
/// ([`CAPACITY_HANDBACK_DEADLINE_CHECK_SECS`](crate::constants::CAPACITY_HANDBACK_DEADLINE_CHECK_SECS))
/// exists only to notice an expired deadline when no further event arrives at
/// all.
///
/// The object's name is **derived**, not referenced: it is the
/// `ScheduledCapacity`'s own name in its own namespace, which is what makes the
/// two impossible to point at different things (ADR 0016 decision 1).
#[must_use]
pub fn capacity_target_key_for(capacity: &ScheduledCapacity) -> Option<ProviderKey> {
    let target = &capacity.spec.target;
    let namespace = ResourceExt::namespace(capacity)?;
    let (group, version) = target.api_version.split_once('/')?;
    Some(ProviderKey {
        gvk: GroupVersionKind::gvk(group, version, &target.kind),
        namespace,
        name: ResourceExt::name_any(capacity),
    })
}

/// Extracts the referenced [`ProviderKey`] from a referencing object, or `None`
/// if it currently references nothing resolvable. One `K` can have several
/// (a `ScheduledCapacity` has two), hence a function rather than a trait
/// method: the manager is instantiated once per reference *edge*, not per kind.
pub type KeyExtractor<K> = fn(&K) -> Option<ProviderKey>;

/// Log label distinguishing one manager's messages from another's, since a
/// binary can run several.
pub const WATCH_LABEL_SPOT_SCHEDULE: &str = "spot-schedule provider";

/// Log label for the capacity-target watch set.
pub const WATCH_LABEL_CAPACITY_TARGET: &str = "capacity target";

/// Owns the reverse index and the lifecycle of one dynamic watcher per
/// referenced GVK, for one reference edge of one kind `K`. Cheaply cloneable
/// via the shared `Arc`.
pub struct DynamicRefWatchManager<K>
where
    K: Lookup + ?Sized,
    K::DynamicType: Hash + Eq,
{
    inner: Arc<Inner<K>>,
}

// Hand-written so cloning never demands `K: Clone`: the manager is a handle to
// a shared `Arc`, and `derive(Clone)` would propagate a bound it does not need.
impl<K> Clone for DynamicRefWatchManager<K>
where
    K: Lookup + ?Sized,
    K::DynamicType: Hash + Eq,
{
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

struct Inner<K>
where
    K: Lookup + ?Sized,
    K::DynamicType: Hash + Eq,
{
    client: Client,
    tx: mpsc::Sender<ObjectRef<K>>,
    /// Shared with every per-GVK watcher task so events can be mapped back to
    /// referencing objects without a reference cycle to the manager.
    index: Arc<Mutex<ReverseIndex<K>>>,
    watchers: Mutex<HashMap<GroupVersionKind, JoinHandle<()>>>,
    key_for: KeyExtractor<K>,
    label: &'static str,
}

impl<K> DynamicRefWatchManager<K>
where
    K: Lookup + Clone + 'static,
    K::DynamicType: Hash + Eq + Clone + Default + Send + Sync + 'static,
{
    /// Construct a manager for one reference edge. `tx` is the channel into the
    /// `Controller::reconcile_on` stream, `key_for` says which object a given
    /// `K` references, and `label` distinguishes this manager's logs.
    #[must_use]
    pub fn new(
        client: Client,
        tx: mpsc::Sender<ObjectRef<K>>,
        key_for: KeyExtractor<K>,
        label: &'static str,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                client,
                tx,
                index: Arc::new(Mutex::new(ReverseIndex::default())),
                watchers: Mutex::new(HashMap::new()),
                key_for,
                label,
            }),
        }
    }

    /// Apply one observed object (an apply/init event from its reflector):
    /// register or update its reference, then reconcile the watcher set. An
    /// object that currently references nothing resolvable is removed from the
    /// index (equivalent to [`Self::forget`]).
    pub fn observe(&self, owner: &K) {
        let owner_ref = ObjectRef::from_obj(owner);
        match (self.inner.key_for)(owner) {
            Some(key) => self.lock_index().register(owner_ref, key),
            None => self.lock_index().deregister(&owner_ref),
        }
        self.sync_watchers();
    }

    /// Drop one object from the index (a delete event), then reconcile the
    /// watcher set.
    pub fn forget(&self, owner: &K) {
        let owner_ref = ObjectRef::from_obj(owner);
        self.lock_index().deregister(&owner_ref);
        self.sync_watchers();
    }

    /// Number of distinct referenced objects currently indexed.
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
            .expect("dynamic-ref watcher lock poisoned")
            .len()
    }

    fn lock_index(&self) -> std::sync::MutexGuard<'_, ReverseIndex<K>> {
        self.inner
            .index
            .lock()
            .expect("dynamic-ref index lock poisoned")
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
            .expect("dynamic-ref watcher lock poisoned");

        watchers.retain(|gvk, handle| {
            let keep = desired.contains(gvk);
            if !keep {
                handle.abort();
                debug!(
                    ?gvk,
                    label = self.inner.label,
                    "aborted dynamic watcher (nothing references it)"
                );
            }
            keep
        });

        for gvk in desired {
            if let std::collections::hash_map::Entry::Vacant(slot) = watchers.entry(gvk.clone()) {
                let handle = tokio::spawn(run_referenced_watcher(
                    self.inner.client.clone(),
                    gvk,
                    Arc::clone(&self.inner.index),
                    self.inner.tx.clone(),
                    self.inner.label,
                ));
                slot.insert(handle);
            }
        }
    }
}

/// The spot-schedule watch set for `ScheduledMachine` (ADR 0006 Phase 3).
pub type SpotScheduleWatchManager = DynamicRefWatchManager<ScheduledMachine>;

/// A `ScheduledCapacity` watch set: one instance per reference edge
/// (`spec.schedule` and `spec.targetRef`).
pub type CapacityRefWatchManager = DynamicRefWatchManager<ScheduledCapacity>;

/// Per-GVK watcher loop: resolve the `ApiResource` (retrying while the CRD is
/// absent), watch all objects of that kind, and map each event back to the
/// referencing objects. Re-resolves after the watch stream ends (e.g. CRD
/// deleted). Exits only when the spawning task is `abort()`-ed.
async fn run_referenced_watcher<K>(
    client: Client,
    gvk: GroupVersionKind,
    index: Arc<Mutex<ReverseIndex<K>>>,
    tx: mpsc::Sender<ObjectRef<K>>,
    label: &'static str,
) where
    K: Lookup + ?Sized,
    K::DynamicType: Hash + Eq + Clone,
{
    let retry = Duration::from_secs(SPOT_SCHEDULE_DISCOVERY_RETRY_SECS);
    info!(?gvk, label, "dynamic reference watcher starting");

    loop {
        let api_resource = match pinned_kind(&client, &gvk).await {
            Ok((api_resource, _capabilities)) => api_resource,
            Err(error) => {
                warn!(
                    ?gvk,
                    label,
                    %error,
                    "referenced CRD not resolvable yet; retrying after back-off"
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
                ) => emit_refs_for_object(&gvk, &object, &index, &tx, label).await,
                Ok(watcher::Event::Init | watcher::Event::InitDone) => {}
                Err(error) => {
                    warn!(?gvk, label, %error, "dynamic reference watcher error; kube-runtime will reconnect");
                }
            }
        }

        warn!(
            ?gvk,
            label, "dynamic reference watch stream ended; re-resolving after back-off"
        );
        tokio::time::sleep(retry).await;
    }
}

/// Map one referenced-object event to the objects referencing it and enqueue
/// each for reconciliation. Objects with no namespace/name (malformed) are
/// ignored.
async fn emit_refs_for_object<K>(
    gvk: &GroupVersionKind,
    object: &DynamicObject,
    index: &Arc<Mutex<ReverseIndex<K>>>,
    tx: &mpsc::Sender<ObjectRef<K>>,
    label: &'static str,
) where
    K: Lookup + ?Sized,
    K::DynamicType: Hash + Eq + Clone,
{
    let (Some(namespace), Some(name)) =
        (ResourceExt::namespace(object), object.metadata.name.clone())
    else {
        return;
    };
    let key = ProviderKey {
        gvk: gvk.clone(),
        namespace,
        name,
    };
    let refs = index
        .lock()
        .expect("dynamic-ref index lock poisoned")
        .lookup(&key);
    for owner_ref in refs {
        if tx.send(owner_ref).await.is_err() {
            debug!(
                label,
                "controller reconcile_on receiver dropped; skipping dispatch"
            );
            return;
        }
    }
}

#[cfg(test)]
#[path = "dynamic_ref_watch_tests.rs"]
mod tests;
