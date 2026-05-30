// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
//! # Child-cluster client resolver
//!
//! Resolves the right [`kube::Client`] for a `ScheduledMachine`'s Node and
//! Pod operations. In CAPI + k0smotron / k0rdent topologies, the `Node`
//! objects live inside a *workload* (child) cluster while the
//! `ScheduledMachine`, CAPI `Machine`, bootstrap, and infrastructure CRs
//! live on the *management* cluster. This module is the boundary: it reads
//! the kubeconfig out of a same-namespace Secret, builds a child-cluster
//! `kube::Client`, caches it by `(namespace, secret_name)`, and invalidates
//! the cache on the Secret's `resourceVersion` change.
//!
//! ## Resolution order
//! 1. Explicit `spec.kubeconfigSecretRef` — fail-closed on missing / bad.
//! 2. Auto-discover `<spec.clusterName>-kubeconfig` Secret in the same
//!    namespace — 404 falls through silently (preserves the degenerate
//!    single-cluster posture).
//! 3. Management client.
//!
//! ## Why no cross-namespace refs
//! The Secret MUST live in the `ScheduledMachine`'s own namespace. Allowing
//! cross-namespace refs would let a tenant in one namespace read a privileged
//! kubeconfig in another — a privilege-escalation surface. The CRD's
//! [`KubeconfigSecretRef`](crate::crd::KubeconfigSecretRef) intentionally has
//! no `namespace` field.
//!
//! ## Cache invalidation
//! Every `resolve()` GETs the Secret to compare `metadata.resourceVersion`
//! against the cached entry; a mismatch rebuilds the client. The GET targets
//! the *management* cluster (cheap, sub-millisecond) — the child cluster is
//! only contacted lazily when downstream code actually uses the returned
//! client. Token / cert rotations performed by CAPI's control-plane provider
//! flip the Secret's `resourceVersion`, so the next reconcile picks up the
//! new credentials with no additional refresh logic.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use k8s_openapi::api::core::v1::Secret;
use kube::{config::Kubeconfig, Api, Client, ResourceExt};
use tracing::{debug, info, warn};

use crate::constants::{CHILD_CLIENT_CACHE_CAP, K8S_API_TIMEOUT_SECS};
use crate::crd::ScheduledMachine;
use crate::metrics::{record_child_kubeconfig_error, record_child_kubeconfig_resolution};
use crate::reconcilers::ReconcilerError;

/// Default key inside the kubeconfig Secret's `data` map — CAPI's
/// `<clusterName>-kubeconfig` Secret writes the kubeconfig under `value`.
/// Used for auto-discovery and as the serde default on
/// [`KubeconfigSecretRef::key`](crate::crd::KubeconfigSecretRef).
pub const DEFAULT_KUBECONFIG_SECRET_KEY: &str = "value";

/// Identifies one cached child-cluster `kube::Client` by the Secret backing
/// it. Two `ScheduledMachine`s pointing at the same Secret share one cache
/// entry (and Phase 3 will share one Node watcher).
#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub struct CacheKey {
    pub namespace: String,
    pub secret_name: String,
}

impl CacheKey {
    /// Construct a `CacheKey`. Provided as a constructor so tests and future
    /// callers don't have to remember the field order.
    #[must_use]
    pub fn new(namespace: impl Into<String>, secret_name: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            secret_name: secret_name.into(),
        }
    }
}

struct CachedEntry {
    client: Client,
    resource_version: String,
    last_used: Instant,
}

/// Outcome of resolving the right client for a `ScheduledMachine`'s
/// Node/Pod operations. `Management` and `Child` are kept distinct so the
/// caller can distinguish "no kubeconfig configured" from "kubeconfig
/// resolved successfully" for logging, metrics, and Phase 2 status
/// conditions.
#[derive(Clone)]
pub enum ResolvedClient {
    /// No `kubeconfigSecretRef` was set, and no `<clusterName>-kubeconfig`
    /// Secret was found in the namespace. The controller falls back to the
    /// management client — the degenerate single-cluster posture where
    /// management ≡ workload.
    Management(Client),
    /// A child-cluster client was built from a kubeconfig Secret.
    Child {
        client: Client,
        key: CacheKey,
        resource_version: String,
    },
}

// `kube::Client` does not implement `Debug`, so we provide a hand-rolled
// impl that elides the client and only surfaces the cache key + RV. This
// is enough for `expect_err` diagnostics and ad-hoc tracing.
impl std::fmt::Debug for ResolvedClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Management(_) => f
                .debug_tuple("Management")
                .field(&"<kube::Client>")
                .finish(),
            Self::Child {
                key,
                resource_version,
                ..
            } => f
                .debug_struct("Child")
                .field("key", key)
                .field("resource_version", resource_version)
                .field("client", &"<kube::Client>")
                .finish(),
        }
    }
}

impl ResolvedClient {
    /// Borrow the underlying `kube::Client` regardless of which variant.
    /// Use this when the call site doesn't care whether the client targets
    /// the management or the child cluster (e.g. `Api::all(client.clone())`).
    #[must_use]
    pub fn client(&self) -> &Client {
        match self {
            Self::Management(c) | Self::Child { client: c, .. } => c,
        }
    }

    /// Consume and return the underlying `kube::Client`.
    #[must_use]
    pub fn into_client(self) -> Client {
        match self {
            Self::Management(c) | Self::Child { client: c, .. } => c,
        }
    }

    /// `true` iff the resolved client targets a child cluster (not management).
    /// Used by tests and by Phase 2 metrics labels.
    #[must_use]
    pub fn is_child(&self) -> bool {
        matches!(self, Self::Child { .. })
    }
}

/// Hook called by the cache when it builds a new child `kube::Client` or
/// evicts a cached one. Implemented by
/// [`crate::reconcilers::child_watch::ChildNodeWatchManager`] to start /
/// cancel per-child Node watchers in lock-step with the client cache.
///
/// Default behaviour is a no-op so unit tests of `ChildClientCache` —
/// and any deployment that doesn't wire a manager — work unchanged.
pub trait ChildWatchHook: Send + Sync {
    /// Called once when a child `Client` is first built for a given
    /// `CacheKey`, AND on every rebuild (Secret `resourceVersion`
    /// change). Implementations are expected to be idempotent: the
    /// manager checks for an existing watcher before starting one.
    fn on_child_resolved(&self, key: &CacheKey, client: Client);

    /// Called when a `CacheKey`'s entry is removed from the cache
    /// (LRU eviction). The manager should cancel and join any
    /// watcher running for this key.
    fn on_child_evicted(&self, key: &CacheKey);
}

/// No-op hook used when no `ChildWatchHook` has been installed. Keeps
/// the cache usable in unit tests + degenerate deployments without
/// requiring an `Option<Arc<dyn ChildWatchHook>>` everywhere.
struct NoopWatchHook;

impl ChildWatchHook for NoopWatchHook {
    fn on_child_resolved(&self, _: &CacheKey, _: Client) {}
    fn on_child_evicted(&self, _: &CacheKey) {}
}

/// In-memory cache of child-cluster `kube::Client`s keyed by the Secret
/// backing each one. Cheaply cloneable (the inner state is `Arc`-wrapped),
/// so the cache can live on [`crate::reconcilers::Context`] and be shared
/// across every reconcile.
#[derive(Clone)]
pub struct ChildClientCache {
    inner: Arc<RwLock<HashMap<CacheKey, CachedEntry>>>,
    /// Watch-lifecycle hook. Defaults to a no-op until `set_hook()` is
    /// called from `main.rs` once the per-child Node watcher manager
    /// has its reflector store wired up.
    hook: Arc<RwLock<Arc<dyn ChildWatchHook>>>,
}

impl Default for ChildClientCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ChildClientCache {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            hook: Arc::new(RwLock::new(Arc::new(NoopWatchHook))),
        }
    }

    /// Install a watch-lifecycle hook. Idempotent: the most recent hook
    /// wins (`main.rs` calls this exactly once at startup). When unset
    /// (default), the cache behaves as if no per-child Node watcher
    /// exists — useful for unit tests and for the degenerate
    /// single-cluster posture.
    pub fn set_hook(&self, hook: Arc<dyn ChildWatchHook>) {
        *self
            .hook
            .write()
            .expect("ChildClientCache hook lock poisoned") = hook;
    }

    /// Get a cheap clone of the current hook for hot-path use.
    fn current_hook(&self) -> Arc<dyn ChildWatchHook> {
        self.hook
            .read()
            .expect("ChildClientCache hook lock poisoned")
            .clone()
    }

    /// Current number of cached child-cluster clients. Exposed for tests +
    /// Phase 2 health endpoint.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner
            .read()
            .expect("ChildClientCache lock poisoned")
            .len()
    }

    /// `true` iff the cache currently holds no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Resolve the right client for this SM's Node/Pod operations.
    ///
    /// See module docs for the resolution order and cache semantics.
    ///
    /// # Errors
    /// - [`ReconcilerError::InvalidConfig`] if the SM is not namespaced
    /// - [`ReconcilerError::ChildClusterUnreachable`] if an explicit
    ///   `spec.kubeconfigSecretRef` cannot be resolved (404, network error)
    /// - [`ReconcilerError::KubeconfigSecretMissingKey`] if the Secret exists
    ///   but lacks the requested `data[key]`
    /// - [`ReconcilerError::KubeconfigInvalid`] if the kubeconfig YAML is
    ///   malformed or a `kube::Client` cannot be built from it
    /// - [`ReconcilerError::KubeError`] for non-404 Secret GET failures
    pub async fn resolve(
        &self,
        mgmt_client: &Client,
        sm: &ScheduledMachine,
    ) -> Result<ResolvedClient, ReconcilerError> {
        let namespace = sm.namespace().ok_or_else(|| {
            ReconcilerError::InvalidConfig(
                "ScheduledMachine must be namespaced for kubeconfig resolution".to_string(),
            )
        })?;

        // 1. Explicit ref — fail-closed.
        if let Some(r) = &sm.spec.kubeconfig_secret_ref {
            let result = self
                .build_or_get_cached(mgmt_client, &namespace, &r.name, &r.key, true)
                .await;
            // Metric label: distinguish the explicit-ref path from auto +
            // surface error/cache-hit/rebuild via the result discriminant.
            record_for_outcome(&result, "child_explicit");
            return result;
        }

        // 2. Auto-discover `<clusterName>-kubeconfig` Secret.
        let auto_name = format!("{}-kubeconfig", sm.spec.cluster_name);
        match self
            .build_or_get_cached(
                mgmt_client,
                &namespace,
                &auto_name,
                DEFAULT_KUBECONFIG_SECRET_KEY,
                false,
            )
            .await
        {
            Ok(r) => {
                record_for_resolved(&r, "child_auto");
                return Ok(r);
            }
            // Auto-discovery 404 falls through to management.
            Err(ReconcilerError::NotFound(_)) => {
                debug!(
                    namespace = %namespace,
                    secret = %auto_name,
                    sm = %sm.name_any(),
                    "no auto-discovered kubeconfig Secret; using management client"
                );
            }
            Err(e) => {
                record_child_kubeconfig_resolution("error");
                record_error_label(&e);
                return Err(e);
            }
        }

        // 3. Management fallback (degenerate co-located case).
        record_child_kubeconfig_resolution("management");
        Ok(ResolvedClient::Management(mgmt_client.clone()))
    }

    /// Get a cached child client by `CacheKey`, if one exists. Bumps
    /// `last_used`. Returned as a clone (the `kube::Client` is cheaply
    /// cloneable). Exposed for tests + the Phase 1.9 watcher module which
    /// needs to share the same cached `Client` instance across resolver +
    /// watcher.
    #[must_use]
    pub fn peek(&self, key: &CacheKey) -> Option<Client> {
        let mut g = self.inner.write().expect("ChildClientCache lock poisoned");
        g.get_mut(key).map(|e| {
            e.last_used = Instant::now();
            e.client.clone()
        })
    }

    /// Manually evict a cache entry. Used by the watcher module when a
    /// per-child Node watch fails terminally (e.g. unauthorized) so the next
    /// reconcile rebuilds from scratch. Fires `on_child_evicted` if an
    /// entry was actually removed.
    pub fn evict(&self, key: &CacheKey) {
        let removed = {
            let mut g = self.inner.write().expect("ChildClientCache lock poisoned");
            g.remove(key).is_some()
        };
        if removed {
            self.current_hook().on_child_evicted(key);
        }
    }

    /// GET the Secret, compare RV against the cache, rebuild the client on
    /// miss. `explicit = true` means a `spec.kubeconfigSecretRef` was set;
    /// 404 in that case is fail-closed. `explicit = false` is auto-discovery;
    /// 404 returns [`ReconcilerError::NotFound`] for the caller to treat as
    /// fall-through.
    async fn build_or_get_cached(
        &self,
        mgmt_client: &Client,
        namespace: &str,
        secret_name: &str,
        key: &str,
        explicit: bool,
    ) -> Result<ResolvedClient, ReconcilerError> {
        let secrets: Api<Secret> = Api::namespaced(mgmt_client.clone(), namespace);
        let secret = match secrets.get(secret_name).await {
            Ok(s) => s,
            Err(kube::Error::Api(e)) if e.code == 404 => {
                if explicit {
                    return Err(ReconcilerError::ChildClusterUnreachable {
                        namespace: namespace.to_string(),
                        name: secret_name.to_string(),
                        reason: format!(
                            "explicit kubeconfigSecretRef points at Secret {namespace}/{secret_name} which does not exist"
                        ),
                    });
                }
                // Auto-discovery: caller treats this as fall-through.
                return Err(ReconcilerError::NotFound(format!(
                    "{namespace}/{secret_name}"
                )));
            }
            Err(e) => {
                if explicit {
                    return Err(ReconcilerError::ChildClusterUnreachable {
                        namespace: namespace.to_string(),
                        name: secret_name.to_string(),
                        reason: format!("GET Secret failed: {e}"),
                    });
                }
                return Err(e.into());
            }
        };

        let rv = secret.metadata.resource_version.clone().unwrap_or_default();
        let cache_key = CacheKey::new(namespace, secret_name);

        // Cache hit on identical RV: return cached client and bump last_used.
        {
            let mut g = self.inner.write().expect("ChildClientCache lock poisoned");
            if let Some(entry) = g.get_mut(&cache_key) {
                if entry.resource_version == rv {
                    entry.last_used = Instant::now();
                    return Ok(ResolvedClient::Child {
                        client: entry.client.clone(),
                        key: cache_key,
                        resource_version: rv,
                    });
                }
                // RV changed — fall through to rebuild. Logged for ops.
                info!(
                    namespace = %namespace,
                    secret = %secret_name,
                    cached_rv = %entry.resource_version,
                    new_rv = %rv,
                    "kubeconfig Secret resourceVersion changed; rebuilding child client"
                );
            }
        }

        // Cache miss / stale: build a new child Client from the Secret data.
        let raw = secret
            .data
            .as_ref()
            .and_then(|d| d.get(key))
            .ok_or_else(|| ReconcilerError::KubeconfigSecretMissingKey {
                namespace: namespace.to_string(),
                name: secret_name.to_string(),
                key: key.to_string(),
            })?;

        let yaml = std::str::from_utf8(&raw.0).map_err(|e| ReconcilerError::KubeconfigInvalid {
            namespace: namespace.to_string(),
            name: secret_name.to_string(),
            reason: format!("kubeconfig data is not valid UTF-8: {e}"),
        })?;

        let kubeconfig =
            Kubeconfig::from_yaml(yaml).map_err(|e| ReconcilerError::KubeconfigInvalid {
                namespace: namespace.to_string(),
                name: secret_name.to_string(),
                reason: format!("YAML parse failed: {e}"),
            })?;

        let mut config = kube::Config::from_custom_kubeconfig(
            kubeconfig,
            &kube::config::KubeConfigOptions::default(),
        )
        .await
        .map_err(|e| ReconcilerError::KubeconfigInvalid {
            namespace: namespace.to_string(),
            name: secret_name.to_string(),
            reason: format!("kubeconfig is not usable to build a Config: {e}"),
        })?;
        // Apply the same wire timeouts the management client uses (see
        // src/main.rs Client construction). Without these a hung child API
        // would stall reconciliation indefinitely.
        config.read_timeout = Some(std::time::Duration::from_secs(K8S_API_TIMEOUT_SECS));
        config.write_timeout = Some(std::time::Duration::from_secs(K8S_API_TIMEOUT_SECS));

        let child = Client::try_from(config).map_err(|e| ReconcilerError::KubeconfigInvalid {
            namespace: namespace.to_string(),
            name: secret_name.to_string(),
            reason: format!("could not build kube::Client: {e}"),
        })?;

        // Insert (with LRU eviction if at cap) and return. Track whether
        // we're replacing an existing entry for the same key (RV change
        // / rotation) AND which (if any) different key was LRU-evicted —
        // both situations fire `on_child_evicted` so the watcher gets
        // cancelled and restarted with the fresh Client. The hook is
        // invoked AFTER the inner-map write lock is released so a
        // manager re-entering the cache (e.g. via `peek`) doesn't
        // deadlock.
        let (replaced_same_key, evicted_other): (bool, Option<CacheKey>) = {
            let mut g = self.inner.write().expect("ChildClientCache lock poisoned");
            let evicted_other = if g.len() >= CHILD_CLIENT_CACHE_CAP && !g.contains_key(&cache_key)
            {
                let lru = g
                    .iter()
                    .min_by_key(|(_, e)| e.last_used)
                    .map(|(k, _)| k.clone());
                if let Some(ref k) = lru {
                    warn!(
                        evicted_namespace = %k.namespace,
                        evicted_secret = %k.secret_name,
                        cap = CHILD_CLIENT_CACHE_CAP,
                        "ChildClientCache at capacity; evicting LRU entry"
                    );
                    g.remove(k);
                }
                lru
            } else {
                None
            };
            let previous = g.insert(
                cache_key.clone(),
                CachedEntry {
                    client: child.clone(),
                    resource_version: rv.clone(),
                    last_used: Instant::now(),
                },
            );
            (previous.is_some(), evicted_other)
        };

        let hook = self.current_hook();
        if let Some(ev) = evicted_other {
            hook.on_child_evicted(&ev);
        }
        if replaced_same_key {
            // RV change: cancel the watcher running with stale credentials
            // before the manager starts a new one with the rebuilt Client.
            hook.on_child_evicted(&cache_key);
        }
        hook.on_child_resolved(&cache_key, child.clone());

        Ok(ResolvedClient::Child {
            client: child,
            key: cache_key,
            resource_version: rv,
        })
    }
}

/// Translate a `Result<ResolvedClient, _>` from `build_or_get_cached`
/// into the right Prometheus result label. `path` is `"child_explicit"`
/// or `"child_auto"` depending on which arm of `resolve()` is calling.
/// On success, we use `cache_hit` vs `rebuild` semantics — but
/// `build_or_get_cached` doesn't currently distinguish them at the
/// return type level. For now we record only the `path` on success;
/// when Phase 2 cache-stats land, this can split into hit/rebuild.
fn record_for_outcome(result: &Result<ResolvedClient, ReconcilerError>, path: &str) {
    match result {
        Ok(r) => record_for_resolved(r, path),
        Err(e) => {
            record_child_kubeconfig_resolution("error");
            record_error_label(e);
        }
    }
}

/// Record one resolution success against the appropriate result label.
/// `Management` overrides `path` because the path argument only
/// describes which arm tried (`child_explicit` / `child_auto`), and
/// the resolver's design lets `build_or_get_cached` only return
/// `Child` — but defensive code in case the type evolves.
fn record_for_resolved(resolved: &ResolvedClient, path: &str) {
    let label = match resolved {
        ResolvedClient::Management(_) => "management",
        ResolvedClient::Child { .. } => path,
    };
    record_child_kubeconfig_resolution(label);
}

/// Map a `ReconcilerError` to a `fivespot_child_kubeconfig_errors_total`
/// reason label. Variants that can't originate from the resolver are
/// folded into `unreachable` as a safe default — they'd indicate a
/// future refactor missed updating this match.
fn record_error_label(err: &ReconcilerError) {
    let reason = match err {
        ReconcilerError::KubeconfigSecretMissingKey { .. } => "secret_missing_key",
        ReconcilerError::KubeconfigInvalid { .. } => "invalid_yaml",
        ReconcilerError::ChildClusterUnreachable { .. } => "unreachable",
        ReconcilerError::KubeError(_) => "non_404_kube_error",
        _ => "unreachable",
    };
    record_child_kubeconfig_error(reason);
}

#[cfg(test)]
#[path = "child_client_tests.rs"]
mod child_client_tests;
