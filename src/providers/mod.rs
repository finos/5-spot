// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # Spot-schedule providers
//!
//! First-party implementations of the spot-schedule provider contract (ADR
//! 0006) that publish the duck-typed `status.active` a
//! `ScheduledMachine.spec.schedule` consumes (ADR 0009). Each provider is a
//! standalone controller (its own binary), separate from the main 5-Spot
//! controller.
//!
//! - [`time_based`] — `TimeBasedSpotSchedule`: the **core/default** provider —
//!   a day-of-week / hour-of-day window in a configured timezone (the reified
//!   former inline `spec.schedule`, ADR 0009).
//! - [`capital_markets`] — `CapitalMarketsSchedule`: an exchange calendar
//!   (sessions / holidays / early closes) evaluated in a configured timezone.

pub mod capital_markets;
pub mod time_based;
