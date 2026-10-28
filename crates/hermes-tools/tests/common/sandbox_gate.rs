// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Skip / run / fail decision for the hermes-tools sandboxed suite (F-335-1).
//!
//! `cargo xtask test` passes its sandbox mode to the test processes as
//! `GALION_SANDBOX_TESTS=<auto|force|require|off>`.
//!
//! - `auto` (or unset, or any value we do not know): the old behaviour —
//!   SKIP when `CI` is set (RULES.md `sandbox_vps_only` belt-and-suspenders)
//!   or when the host has neither Landlock nor userns, or git/dd/install are
//!   missing.
//! - `force` (`1`/`on`): explicit opt-in; the `CI` skip is lifted, but a host
//!   without the features still SKIPs (force means "try", not "gate").
//! - `require`: the CI gate. Never skip: the `CI` skip is lifted and a host
//!   that lacks what the suite needs FAILS the test (panic) instead of
//!   printing SKIP. "Needs" mirrors `xtask::missing("hermes-tools")`:
//!   Landlock (or `HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1`) and userns, plus
//!   git/dd/install.
//!
//! Pure: no probing here, so the decision table can be unit-tested on any
//! host (tests/sandbox_gate.rs) without running a sandbox.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateMode {
    Auto,
    Force,
    Require,
}

impl GateMode {
    pub fn from_env(v: Option<&str>) -> GateMode {
        match v.map(str::trim) {
            Some("require") => GateMode::Require,
            Some("force") | Some("1") | Some("on") => GateMode::Force,
            _ => GateMode::Auto,
        }
    }
}

/// What the host offers, as far as the suite cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostCaps {
    pub landlock_abi: i32,
    pub userns: bool,
    pub bins_missing: bool,
    pub allow_no_landlock: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gate {
    Run,
    Skip(String),
    Fail(String),
}

/// Decide before probing: only the `CI` skip, which needs no probe.
pub fn pre_probe(mode: GateMode, ci: bool) -> Option<Gate> {
    if ci && mode == GateMode::Auto {
        return Some(Gate::Skip(
            "SKIP: hermes-tools sandboxed suite is VPS-only (CI=set, GALION_SANDBOX_TESTS auto)"
                .into(),
        ));
    }
    None
}

/// Decide after probing the host.
pub fn decide(mode: GateMode, ci: bool, h: HostCaps) -> Gate {
    if let Some(g) = pre_probe(mode, ci) {
        return g;
    }
    if mode == GateMode::Require {
        let mut lacks = Vec::new();
        if h.landlock_abi < 1 && !h.allow_no_landlock {
            lacks.push("landlock");
        }
        if !h.userns {
            lacks.push("userns");
        }
        if h.bins_missing {
            lacks.push("git/dd/install");
        }
        if lacks.is_empty() {
            return Gate::Run;
        }
        return Gate::Fail(format!(
            "GALION_SANDBOX_TESTS=require: hermes-tools sandboxed suite cannot run here, host lacks {} \
             (require never skips; use auto, or HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1 where that override is meant)",
            lacks.join("+")
        ));
    }
    if (h.landlock_abi < 1 && !h.userns) || h.bins_missing {
        return Gate::Skip("SKIP: no landlock/userns or no git/dd/install on this machine".into());
    }
    Gate::Run
}
