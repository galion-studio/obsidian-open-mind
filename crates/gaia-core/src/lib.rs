// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Gaia core: the central orchestrator of the Gaia universal creator, modelled
//! on the GAIA system of Horizon Zero Dawn (inspiration only; see
//! `gaia/docs/gaia-design.md`).
//!
//! * [`SubordinateFunction`]: one domain, one set of owned [`System`]s.
//! * [`Gaia`]: registry, routing, tick loop, containment, master override.
//! * [`bus`]: signal towers (broadcast) and direct links.
//! * [`lineage`]: every action recorded with a Europe/Zagreb timestamp.
//!
//! std only (plus `zagreb-time`), no network, no processes, no file I/O.

pub mod bus;
pub mod gaia;
pub mod json;
pub mod lineage;
pub mod model;
pub mod registry;
pub mod system;

pub use bus::{Bus, Endpoint, Message, Route};
pub use gaia::{Gaia, Override, TickReport};
pub use json::Json;
pub use lineage::{Lineage, LineageRecord, SimClock};
pub use model::{
    ApprovalToken, Context, Directive, Environment, Health, Lifecycle, Outcome, Report, Verb,
};
pub use registry::AlphaRegistry;
pub use system::{FunctionId, System};

/// One of GAIA's subordinate functions.
pub trait SubordinateFunction {
    fn id(&self) -> FunctionId;

    fn name(&self) -> &'static str {
        self.id().name()
    }

    fn domain(&self) -> &'static str {
        self.id().domain()
    }

    /// The systems this function declares it owns. GAIA refuses to register a
    /// function whose declaration differs from the ownership table.
    fn systems(&self) -> Vec<System>;

    fn health(&self) -> Health;

    /// Handle one directive aimed at one of this function's systems.
    fn handle(&mut self, directive: &Directive, ctx: &mut Context) -> Report;

    /// A message arrived over a tower or a direct link.
    fn on_message(&mut self, _msg: &Message, _ctx: &mut Context) {}

    /// Autonomous work for one tick.
    fn tick(&mut self, _ctx: &mut Context) -> Option<Report> {
        None
    }

    /// Back to a clean state (reboot).
    fn reset(&mut self) {}

    /// Internal state for `gaia status`.
    fn state_json(&self) -> Json {
        Json::obj()
    }
}

/// Shared helper: scale a base amount by directive intensity (0..=100),
/// at least 1.
pub fn scaled(base: u64, intensity: u8) -> u64 {
    (base.saturating_mul(u64::from(intensity.min(100))) / 100).max(1)
}

/// Shared helper: FNV-1a 64-bit, for deterministic ids.
pub fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests;
