// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! The runtime components of the single `5spot` binary, one module per
//! subcommand (ADR 0020).
//!
//! Each module owns its component's `Args` (every flag and environment variable
//! is unchanged from when the component was its own binary) and a `run`
//! function. They live in the binary crate, not the library, so they keep
//! `anyhow` at the edge and stay out of the library's API.

pub(crate) mod capacity_controller;
pub(crate) mod controller;
pub(crate) mod kata_config_agent;
pub(crate) mod nsenter;
pub(crate) mod provider_capital_markets;
pub(crate) mod provider_time_based;
pub(crate) mod reclaim_agent;
