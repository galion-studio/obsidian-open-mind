// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The HADES gate with a real hades-gate Approver, a throwaway admin key and
//! signed tokens: deploy held without a token, approved once with a correct
//! one, replay refused; merge refused even with a signed merge token; a
//! token for another task refused; HADES disable wins.

use hades_gate::keys;
use hades_gate::token::{plan_hash, Claims};
use hermes_core::action::ActionClass;
use hermes_core::gate::{hades_target, plan_bytes, Gate, GateDecision};
use hermes_core::readiness::synthetic;
use std::path::{Path, PathBuf};

fn setup(name: &str) -> (PathBuf, Gate) {
    let d = std::env::temp_dir().join(format!("hermes-core-gate-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    keys::generate(&d.join("admin.seed"), &d.join("admin.pub")).unwrap();
    let cfg = format!(
        "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\nmax_ttl_secs = 600\n",
        d.join("admin.pub").display(),
        d.join("hades-ledger.jsonl").display(),
        d.join("DISABLED").display()
    );
    std::fs::write(d.join("hades.toml"), cfg).unwrap();
    let gate = Gate::from_config(&d.join("hades.toml").display().to_string());
    assert!(gate.configured(), "{}", gate.why_closed());
    (d, gate)
}

fn mint(dir: &Path, action: &str, target: &str, plan: &[u8]) -> String {
    let seed = keys::load_seed(&dir.join("admin.seed")).unwrap();
    let now = hades_gate::approval::unix_now();
    Claims {
        action: action.into(),
        target: target.into(),
        plan_hash: plan_hash(plan),
        nonce: keys::os_random::<16>().unwrap(),
        issued_at: now,
        expires_at: now + 300,
    }
    .sign(&seed)
}

#[test]
fn deploy_needs_a_signed_single_use_token() {
    let (d, gate) = setup("deploy");
    let t = synthetic(
        "vps-ops:S4.1",
        "tuner 8650 binary swap via galion-deploy",
        ActionClass::Deploy,
    );
    assert!(matches!(gate.check(&t, ""), GateDecision::Held { .. }));
    assert!(matches!(gate.preview(&t), GateDecision::Held { .. }));
    let tok = mint(&d, "switch", &hades_target(&t), &plan_bytes(&t));
    assert!(matches!(
        gate.check(&t, &tok),
        GateDecision::Approved { .. }
    ));
    match gate.check(&t, &tok) {
        GateDecision::Refused { code, .. } => assert_eq!(code, "replayed"),
        other => panic!("replay must be refused: {other:?}"),
    }
}

#[test]
fn merge_is_refused_even_with_a_signed_merge_token() {
    let (d, gate) = setup("merge");
    let t = synthetic(
        "integration-ci:S1.T9",
        "Land integration-1 on main",
        ActionClass::Merge,
    );
    let tok = mint(&d, "merge", &hades_target(&t), &plan_bytes(&t));
    for token in ["", tok.as_str()] {
        match gate.check(&t, token) {
            GateDecision::Refused { code, action, .. } => {
                assert_eq!(code, "action");
                assert_eq!(action, "merge");
            }
            other => panic!("merge must be refused: {other:?}"),
        }
    }
}

#[test]
fn token_for_another_task_or_forged_is_refused() {
    let (d, gate) = setup("scope");
    let a = synthetic("vps-ops:S4.1", "tuner binary swap", ActionClass::Deploy);
    let b = synthetic(
        "vps-ops:S4.2",
        "room: new unit on loopback",
        ActionClass::Deploy,
    );
    let tok_a = mint(&d, "switch", &hades_target(&a), &plan_bytes(&a));
    match gate.check(&b, &tok_a) {
        GateDecision::Refused { code, .. } => assert_eq!(code, "wrong-scope"),
        other => panic!("{other:?}"),
    }
    let mut forged = tok_a.clone();
    forged.pop();
    forged.push(if tok_a.ends_with('0') { '1' } else { '0' });
    assert!(matches!(
        gate.check(&a, &forged),
        GateDecision::Refused { .. }
    ));
}

#[test]
fn disable_file_and_missing_config_fail_closed() {
    let (d, gate) = setup("disable");
    let t = synthetic("vps-ops:S4.1", "tuner binary swap", ActionClass::Deploy);
    let tok = mint(&d, "switch", &hades_target(&t), &plan_bytes(&t));
    std::fs::write(d.join("DISABLED"), b"off").unwrap();
    match gate.check(&t, &tok) {
        GateDecision::Refused { code, .. } => assert_eq!(code, "disabled"),
        other => panic!("{other:?}"),
    }
    let closed = Gate::from_config("");
    match closed.check(&t, &tok) {
        GateDecision::Refused { code, .. } => assert_eq!(code, "no-hades"),
        other => panic!("{other:?}"),
    }
    let code_task = synthetic("x:S1.1", "write a crate", ActionClass::Code);
    assert_eq!(closed.check(&code_task, ""), GateDecision::NotNeeded);
}

#[test]
fn admin_safety_refuses_a_hazardous_plan() {
    let (_d, gate) = setup("safety");
    let t = synthetic(
        "vps-ops:S9.9",
        "cutover then rm -rf / on the box root",
        ActionClass::Deploy,
    );
    match gate.check(&t, "") {
        GateDecision::Refused { code, .. } => assert_eq!(code, "admin-safety"),
        other => panic!("{other:?}"),
    }
}
