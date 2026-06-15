// Copyright (c) 2026 Erick Bourgeois, 5-Spot
// SPDX-License-Identifier: Apache-2.0
//! # Spot-schedule providers
//!
//! Reference implementations of the spot-schedule provider contract (ADR 0006)
//! that publish the duck-typed `status.active` a
//! `ScheduledMachine.spec.spotSchedule` consumes. Each provider is a standalone
//! controller (its own binary), separate from the main 5-Spot controller.
//!
//! - [`capital_markets`] — `CapitalMarketsSchedule`: an exchange calendar
//!   (sessions / holidays / early closes) evaluated in a configured timezone.

pub mod capital_markets;
