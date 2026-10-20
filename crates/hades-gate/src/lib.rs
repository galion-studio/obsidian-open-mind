// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Pure approval gate for destructive plans.
//!
//! [`decide`] classifies one action. It does not spawn a process, talk to a
//! service manager, or use the network. Default [`Mode`] is dry-run.
//!
//! [`Decision::WouldApply`] is only a note that an allow-listed action had the
//! fixed test token. The command is still not run. `kill` and `rotate` are
//! always [`Decision::Refused`].

#![forbid(unsafe_code)]

/// One JSON record of a gate decision (was the hades-journal crate until 2026-10-20).
pub mod journal;

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
    /// Default. The action was not both allow-listed and test-approved.
    DryRun,
    /// Allow-listed action plus the test token. Still not executed.
    WouldApply,
    /// `kill` or `rotate`. Never approved.
    Refused,
}

/// Actions that can return [`Decision::WouldApply`].
pub const ALLOW_LIST: [&str; 3] = ["heal-apply", "switch", "rollback"];

/// Actions that always return [`Decision::Refused`].
pub const ALWAYS_REFUSED: [&str; 2] = ["kill", "rotate"];

/// Fixed test token. Not a credential.
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

    /// Same rules as [`decide`]. The stored mode is dry-run, so this never applies.
    #[must_use]
    pub fn decide(self, action: &str, token: &str) -> Decision {
        match self.mode {
            Mode::DryRun => classify(action, token),
        }
    }
}

/// Classify `action` with `token`.
///
/// Returns [`Decision::DryRun`] unless `token` is exactly [`APPROVED_TEST_TOKEN`]
/// and `action` is in [`ALLOW_LIST`]. Those cases return [`Decision::WouldApply`]
/// and still do not run a command. `kill` and `rotate` always return
/// [`Decision::Refused`], including when the test token is present.
#[must_use]
pub fn decide(action: &str, token: &str) -> Decision {
    Gate::default().decide(action, token)
}

fn classify(action: &str, token: &str) -> Decision {
    if ALWAYS_REFUSED.contains(&action) {
        Decision::Refused
    } else if ALLOW_LIST.contains(&action) && token == APPROVED_TEST_TOKEN {
        Decision::WouldApply
    } else {
        Decision::DryRun
    }
}
