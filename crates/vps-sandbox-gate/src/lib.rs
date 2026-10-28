// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Gate for real tests that RUN the sandbox (openmind-sandbox suites, the
//! sandboxed hermes-tools heavy test, the `real_sh_sandbox` orchestrator).
//!
//! Standing rule `sandbox_vps_only` (Slavik via Mate, 2026-10-21 04:29 CEST
//! +02:00): sandbox runs happen only on the VPS. Finding F-383-10
//! (2026-10-28, slot 383): `GALION_REAL` was set in the shared box shell env,
//! so a plain `cargo test --workspace` ran `real_sh_sandbox` on the box.
//! `GALION_REAL=1` alone is therefore NOT enough for these tests. They need
//! all three:
//!
//! 1. `GALION_REAL=1` (the usual real-test switch);
//! 2. `GALION_REAL_SANDBOX=1`, an explicit per-run opt-in that no shared
//!    profile may set (it names the one thing being allowed);
//! 3. a host check: the machine looks like the VPS (`/opt/galion` and
//!    `/etc/galion` are directories) and NOT like the swarm box
//!    (`/workspace/swarm` is absent).
//!
//! If (1) is missing the test is skipped quietly, as before. If (1) is set
//! but (2) is missing, the test is skipped with a loud line naming the guard,
//! so an ambient `GALION_REAL` can never start a sandbox run. If (1) and (2)
//! are set but the host check fails, the test is refused: skipped with a
//! `REFUSED` line, never run. There is no override for the host check.

use std::path::Path;

/// Opt-in variable, in addition to `GALION_REAL=1`.
pub const OPT_IN: &str = "GALION_REAL_SANDBOX";
/// Directories that must exist on the VPS.
pub const VPS_MARKERS: [&str; 2] = ["/opt/galion", "/etc/galion"];
/// Directory that exists only on the swarm box.
pub const BOX_MARKER: &str = "/workspace/swarm";

/// What the host looks like, read from the filesystem (or faked in tests).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Host {
    /// Every path in [`VPS_MARKERS`] is a directory.
    pub vps_markers: bool,
    /// [`BOX_MARKER`] exists.
    pub box_marker: bool,
}

impl Host {
    /// Probe the real filesystem (metadata only, nothing is read or written).
    pub fn probe() -> Self {
        Host {
            vps_markers: VPS_MARKERS.iter().all(|p| Path::new(p).is_dir()),
            box_marker: Path::new(BOX_MARKER).exists(),
        }
    }

    /// The host passes the VPS check.
    pub fn is_vps(&self) -> bool {
        self.vps_markers && !self.box_marker
    }
}

/// Outcome of the gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Run the sandbox test.
    Run,
    /// `GALION_REAL` is not `1`: ordinary skip.
    SkipNotReal,
    /// `GALION_REAL=1` but no `GALION_REAL_SANDBOX=1`: the ambient case.
    SkipNoOptIn,
    /// Both set, but this is not the VPS.
    RefuseOffVps,
}

/// Pure decision from the two variables and the host facts.
pub fn decide(galion_real: Option<&str>, opt_in: Option<&str>, host: Host) -> Decision {
    if galion_real != Some("1") {
        return Decision::SkipNotReal;
    }
    if opt_in != Some("1") {
        return Decision::SkipNoOptIn;
    }
    if !host.is_vps() {
        return Decision::RefuseOffVps;
    }
    Decision::Run
}

/// The message printed for a decision (None for [`Decision::Run`]).
pub fn message(test: &str, d: Decision, host: Host) -> Option<String> {
    match d {
        Decision::Run => None,
        Decision::SkipNotReal => Some(format!(
            "SKIP {test}: set GALION_REAL=1 {OPT_IN}=1 on the VPS (sandbox_vps_only)"
        )),
        Decision::SkipNoOptIn => Some(format!(
            "SKIP {test}: GALION_REAL=1 is set but {OPT_IN}=1 is not; sandbox tests need the \
             explicit opt-in and run only on the VPS (sandbox_vps_only, F-383-10)"
        )),
        Decision::RefuseOffVps => Some(format!(
            "REFUSED {test}: not the VPS (vps_markers={} box_marker={}); sandbox runs only on \
             the VPS (sandbox_vps_only). Nothing was run.",
            host.vps_markers, host.box_marker
        )),
    }
}

/// Gate a sandbox real test. Returns true only when it may run. Prints the
/// reason to stderr otherwise.
pub fn sandbox_real(test: &str) -> bool {
    let real = std::env::var("GALION_REAL").ok();
    let opt = std::env::var(OPT_IN).ok();
    let host = Host::probe();
    let d = decide(real.as_deref(), opt.as_deref(), host);
    if let Some(m) = message(test, d, host) {
        eprintln!("{m}");
    }
    d == Decision::Run
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: Host = Host {
        vps_markers: false,
        box_marker: true,
    };
    const VPS: Host = Host {
        vps_markers: true,
        box_marker: false,
    };
    const BOTH: Host = Host {
        vps_markers: true,
        box_marker: true,
    };
    const NEITHER: Host = Host {
        vps_markers: false,
        box_marker: false,
    };

    #[test]
    fn not_real_is_plain_skip_everywhere() {
        for h in [BOX, VPS, BOTH, NEITHER] {
            for o in [None, Some("1"), Some("0")] {
                assert_eq!(decide(None, o, h), Decision::SkipNotReal);
                assert_eq!(decide(Some("0"), o, h), Decision::SkipNotReal);
                assert_eq!(decide(Some(""), o, h), Decision::SkipNotReal);
            }
        }
    }

    #[test]
    fn ambient_galion_real_never_runs() {
        for h in [BOX, VPS, BOTH, NEITHER] {
            for o in [None, Some("0"), Some(""), Some("yes"), Some("true")] {
                assert_eq!(
                    decide(Some("1"), o, h),
                    Decision::SkipNoOptIn,
                    "{h:?} {o:?}"
                );
            }
        }
    }

    #[test]
    fn opt_in_off_vps_is_refused() {
        for h in [BOX, BOTH, NEITHER] {
            assert_eq!(decide(Some("1"), Some("1"), h), Decision::RefuseOffVps);
        }
    }

    #[test]
    fn opt_in_on_vps_runs() {
        assert_eq!(decide(Some("1"), Some("1"), VPS), Decision::Run);
    }

    #[test]
    fn messages_name_the_rule() {
        assert!(message("t", Decision::Run, VPS).is_none());
        let m = message("t", Decision::SkipNoOptIn, BOX).unwrap();
        assert!(m.contains("GALION_REAL_SANDBOX=1") && m.contains("sandbox_vps_only"));
        let m = message("t", Decision::RefuseOffVps, BOX).unwrap();
        assert!(m.starts_with("REFUSED t") && m.contains("box_marker=true"));
    }

    #[test]
    fn this_box_probe_is_not_vps() {
        // On the swarm box /workspace/swarm exists, so the probe must fail.
        // On the VPS / CI the assertion is skipped (no box marker).
        let h = Host::probe();
        if Path::new(BOX_MARKER).exists() {
            assert!(!h.is_vps());
        }
    }
}
