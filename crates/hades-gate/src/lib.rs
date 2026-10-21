// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Pure approval gate for destructive plans.
//!
//! [`decide`] classifies one action. It does not spawn a process, talk to a
//! service manager, or use the network. Default [`Mode`] is dry-run.
//!
//! [`Decision::WouldApply`] is only a note that an allow-listed action carried
//! a valid admin approval. The command is still not run. `kill` and `rotate`
//! are always [`Decision::Refused`].
//!
//! Real approvals (2026-10-20) are signed, expiring, single-use Ed25519 tokens
//! scoped to one action kind, one target service and one plan hash; see
//! [`approval::Approver`], [`token`], [`ledger`] and [`safety`]. The old public
//! test token `"approved-test"` no longer approves anything. [`decide`] has no
//! admin key, so it can only return dry-run or refused.

#![forbid(unsafe_code)]

/// One JSON record of a gate decision (was the hades-journal crate until 2026-10-20).
pub mod journal;

/// Adapter for GAIA HADES (`gaia-hades`).
pub mod adapter;
/// Config, admin key, ledger and safety rule put together.
pub mod approval;
/// Gate config file.
pub mod config;
/// Admin key files.
pub mod keys;
/// Append-only, fsynced replay ledger.
pub mod ledger;
/// The hard admin-safety rule.
pub mod safety;
/// Signed approval tokens.
pub mod token;
/// Option B prototype: typed, signed plan steps.
pub mod typed;

pub use approval::{Approver, Refusal, Request, Verdict};

/// The only operating mode. There is no apply mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// Classify only. Nothing is executed.
    #[default]
    DryRun,
}

/// Names the gate understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Healer apply plan. Allow-listed.
    HealApply,
    /// Switch plan. Allow-listed.
    Switch,
    /// Rollback plan. Allow-listed.
    Rollback,
    /// Kill switch. Always refused.
    Kill,
    /// Rotation plan. Always refused.
    Rotate,
}

impl Action {
    /// Parse a wire name. Unknown names are `None`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "heal-apply" => Self::HealApply,
            "switch" => Self::Switch,
            "rollback" => Self::Rollback,
            "kill" => Self::Kill,
            "rotate" => Self::Rotate,
            _ => return None,
        })
    }

    /// The wire name accepted by [`decide`].
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HealApply => "heal-apply",
            Self::Switch => "switch",
            Self::Rollback => "rollback",
            Self::Kill => "kill",
            Self::Rotate => "rotate",
        }
    }

    /// `heal-apply`, `switch`, and `rollback`.
    #[must_use]
    pub const fn is_allow_listed(self) -> bool {
        matches!(self, Self::HealApply | Self::Switch | Self::Rollback)
    }
}

/// Result of [`decide`]. No variant runs a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Default. No valid admin approval for an allow-listed action.
    DryRun,
    /// Allow-listed action plus a valid admin approval. Still not executed.
    WouldApply,
    /// `kill` or `rotate`. Never approved.
    Refused,
}

/// Actions that can return [`Decision::WouldApply`].
pub const ALLOW_LIST: [&str; 3] = ["heal-apply", "switch", "rollback"];

/// Actions that always return [`Decision::Refused`].
pub const ALWAYS_REFUSED: [&str; 2] = ["kill", "rotate"];

/// The old fixed test token. It is public, so it approves **nothing** now.
/// Kept only so old code still compiles; use [`approval::Approver`].
#[deprecated(
    note = "public token; it no longer approves anything. Use approval::Approver with a signed token."
)]
pub const APPROVED_TEST_TOKEN: &str = "approved-test";

/// The gate. Constructed in dry-run and cannot leave it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gate {
    mode: Mode,
}

impl Default for Gate {
    fn default() -> Self {
        Self::new()
    }
}

impl Gate {
    /// A dry-run gate.
    #[must_use]
    pub const fn new() -> Self {
        Self { mode: Mode::DryRun }
    }

    /// Always [`Mode::DryRun`].
    #[must_use]
    pub const fn mode(self) -> Mode {
        self.mode
    }

    /// Same rules as [`decide`]: without an admin key nothing is approved.
    #[must_use]
    pub fn decide(self, action: &str, token: &str) -> Decision {
        match self.mode {
            Mode::DryRun => classify(action, token),
        }
    }
}

/// Classify `action` with `token`.
///
/// This entry point has no admin key, so no token can approve here: it
/// returns [`Decision::DryRun`] for every action except `kill` and `rotate`,
/// which return [`Decision::Refused`]. The `token` argument is ignored and kept
/// for API stability. Use [`approval::Approver::approve`] for real approvals.
#[must_use]
pub fn decide(action: &str, token: &str) -> Decision {
    Gate::default().decide(action, token)
}

fn classify(action: &str, _token: &str) -> Decision {
    if ALWAYS_REFUSED.contains(&action) {
        Decision::Refused
    } else {
        Decision::DryRun
    }
}
