// Copyright (c) 2025 Erick Bourgeois, RBC Capital Markets
// SPDX-License-Identifier: Apache-2.0
// Health check endpoints for 5-Spot controller
//
// Provides /healthz (liveness) and /readyz (readiness) endpoints
// following Kubernetes conventions.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tracing::{debug, error, info};
use warp::Filter;

/// Health state shared across the application
#[derive(Debug, Clone)]
pub struct HealthState {
    /// Whether the controller has successfully connected to Kubernetes API
    k8s_connected: Arc<AtomicBool>,
    /// Whether the controller is ready to serve traffic
    ready: Arc<AtomicBool>,
}

impl Default for HealthState {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthState {
    /// Create a new health state (starts as not ready)
    #[must_use]
    pub fn new() -> Self {
        Self {
            k8s_connected: Arc::new(AtomicBool::new(false)),
            ready: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Mark Kubernetes API as connected
    pub fn set_k8s_connected(&self, connected: bool) {
        self.k8s_connected.store(connected, Ordering::SeqCst);
        debug!(
            connected = connected,
            "Kubernetes API connection status updated"
        );
    }

    /// Mark the controller as ready
    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::SeqCst);
        debug!(ready = ready, "Controller readiness status updated");
    }

    /// Check if the controller is healthy (liveness)
    /// Returns true if the process is running and not deadlocked
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        // For liveness, we just need to be able to respond
        // If we can execute this code, we're alive
        true
    }

    /// Check if the controller is ready (readiness)
    /// Returns true if the controller can serve traffic
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.k8s_connected.load(Ordering::SeqCst) && self.ready.load(Ordering::SeqCst)
    }

    /// Get detailed health status
    #[must_use]
    pub fn get_status(&self) -> HealthStatus {
        HealthStatus {
            healthy: self.is_healthy(),
            ready: self.is_ready(),
            k8s_connected: self.k8s_connected.load(Ordering::SeqCst),
        }
    }
}

/// Detailed health status for debugging
#[derive(Debug, serde::Serialize)]
pub struct HealthStatus {
    pub healthy: bool,
    pub ready: bool,
    pub k8s_connected: bool,
}

/// Start the health check server
pub async fn start_health_server(port: u16, health_state: HealthState) {
    info!(port = port, "Starting health check server");

    let health_state_healthz = health_state.clone();
    let health_state_readyz = health_state.clone();
    let health_state_status = health_state;

    // /healthz - Kubernetes liveness probe
    // Returns 200 if the process is alive.
    //
    // Body type is `String` (not `&str`) because `Reply` is only implemented
    // for `&'static str`; using owned `String` avoids inferring a non-'static
    // closure return type. The `.boxed()` on the final filter chain is the
    // companion fix — without it, the warp 0.4 filter type carries a higher-
    // ranked `AsRef<str>` bound that propagates through `tokio::spawn` and
    // fails `Send + 'static`.
    let healthz = warp::path("healthz").map(move || {
        if health_state_healthz.is_healthy() {
            warp::reply::with_status("OK".to_string(), warp::http::StatusCode::OK)
        } else {
            error!("Health check failed");
            warp::reply::with_status(
                "UNHEALTHY".to_string(),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )
        }
    });

    // /readyz - Kubernetes readiness probe
    // Returns 200 if the controller is ready to serve traffic.
    let readyz = warp::path("readyz").map(move || {
        if health_state_readyz.is_ready() {
            warp::reply::with_status("OK".to_string(), warp::http::StatusCode::OK)
        } else {
            debug!("Readiness check failed - controller not ready");
            warp::reply::with_status(
                "NOT READY".to_string(),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )
        }
    });

    // /health/status - Detailed health status (for debugging)
    let status = warp::path!("health" / "status").map(move || {
        let status = health_state_status.get_status();
        warp::reply::json(&status)
    });

    // Legacy endpoints for backward compatibility.
    let health_legacy = warp::path("health")
        .map(|| warp::reply::with_status("OK".to_string(), warp::http::StatusCode::OK));
    let ready_legacy = warp::path("ready")
        .map(|| warp::reply::with_status("OK".to_string(), warp::http::StatusCode::OK));

    let routes = healthz
        .or(readyz)
        .or(status)
        .or(health_legacy)
        .or(ready_legacy)
        .boxed();

    warp::serve(routes).run(([0, 0, 0, 0], port)).await;
}

#[cfg(test)]
#[path = "health_tests.rs"]
mod tests;
