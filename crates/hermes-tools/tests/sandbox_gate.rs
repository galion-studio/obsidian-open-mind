// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Decision table for the sandboxed suite gate (F-335-1). Pure: probes
//! nothing and runs no sandbox, so it is safe on every host.

#[path = "common/sandbox_gate.rs"]
mod sandbox_gate;
use sandbox_gate::{decide, pre_probe, Gate, GateMode, HostCaps};

const FULL: HostCaps = HostCaps {
    landlock_abi: 4,
    userns: true,
    bins_missing: false,
    allow_no_landlock: false,
};
// The live VPS: userns yes, Landlock no (F-320-1).
const VPS: HostCaps = HostCaps {
    landlock_abi: 0,
    ..FULL
};
const BARE: HostCaps = HostCaps {
    landlock_abi: 0,
    userns: false,
    ..FULL
};

fn is_skip(g: &Gate) -> bool {
    matches!(g, Gate::Skip(_))
}
fn is_fail(g: &Gate) -> bool {
    matches!(g, Gate::Fail(_))
}

#[test]
fn mode_from_env() {
    assert_eq!(GateMode::from_env(None), GateMode::Auto);
    assert_eq!(GateMode::from_env(Some("")), GateMode::Auto);
    assert_eq!(GateMode::from_env(Some("auto")), GateMode::Auto);
    assert_eq!(GateMode::from_env(Some("off")), GateMode::Auto);
    assert_eq!(GateMode::from_env(Some("bogus")), GateMode::Auto);
    assert_eq!(GateMode::from_env(Some("1")), GateMode::Force);
    assert_eq!(GateMode::from_env(Some("force")), GateMode::Force);
    assert_eq!(GateMode::from_env(Some("require")), GateMode::Require);
}

#[test]
fn auto_keeps_ci_skip_and_feature_skip() {
    assert!(is_skip(&decide(GateMode::Auto, true, FULL)));
    assert!(pre_probe(GateMode::Auto, true).is_some());
    assert_eq!(decide(GateMode::Auto, false, FULL), Gate::Run);
    // Old rule: Landlock OR userns is enough to try in auto.
    assert_eq!(decide(GateMode::Auto, false, VPS), Gate::Run);
    assert!(is_skip(&decide(GateMode::Auto, false, BARE)));
    let nobin = HostCaps {
        bins_missing: true,
        ..FULL
    };
    assert!(is_skip(&decide(GateMode::Auto, false, nobin)));
}

#[test]
fn require_never_skips() {
    // CI set no longer skips under require.
    assert!(pre_probe(GateMode::Require, true).is_none());
    assert_eq!(decide(GateMode::Require, true, FULL), Gate::Run);
    assert_eq!(decide(GateMode::Require, false, FULL), Gate::Run);
    for ci in [false, true] {
        for h in [
            VPS,
            BARE,
            HostCaps {
                bins_missing: true,
                ..FULL
            },
            HostCaps {
                userns: false,
                ..FULL
            },
        ] {
            let g = decide(GateMode::Require, ci, h);
            assert!(is_fail(&g), "require must fail, got {g:?} for {h:?}");
        }
    }
    match decide(GateMode::Require, true, VPS) {
        Gate::Fail(m) => assert!(m.contains("landlock") && !m.contains("userns"), "{m}"),
        g => panic!("{g:?}"),
    }
    match decide(GateMode::Require, false, BARE) {
        Gate::Fail(m) => assert!(m.contains("landlock+userns"), "{m}"),
        g => panic!("{g:?}"),
    }
}

#[test]
fn require_honours_landlock_override() {
    let vps_override = HostCaps {
        allow_no_landlock: true,
        ..VPS
    };
    assert_eq!(decide(GateMode::Require, true, vps_override), Gate::Run);
    // The override lifts only Landlock, not userns.
    let bare_override = HostCaps {
        allow_no_landlock: true,
        ..BARE
    };
    assert!(is_fail(&decide(GateMode::Require, false, bare_override)));
}

#[test]
fn force_lifts_ci_skip_only() {
    assert!(pre_probe(GateMode::Force, true).is_none());
    assert_eq!(decide(GateMode::Force, true, FULL), Gate::Run);
    assert!(is_skip(&decide(GateMode::Force, true, BARE)));
}
