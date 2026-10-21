// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Cortex T4.2: HADES plan templates per fix family.
//!
//! - every family yields a well-formed plan: allow-listed action, a target
//!   and plan text that pass the **real** hades-gate admin-safety review,
//!   preconditions, verify and rollback steps, signed bytes =
//!   `gaia_plan_bytes(steps)`, stable across cycles;
//! - unknown and excluded findings yield none (listed in `skipped`);
//! - protected names (ssh hostnames, port 22, HADES) never get a plan;
//! - a template goes through a real single-use approval with a throwaway
//!   key: approved once (still `WouldApply`, nothing runs), refused on
//!   replay, refused when one byte of the plan changes;
//! - the golden replay fixture yields its six plans through the engine.

use ed25519_dalek::SigningKey;
use hades_gate::adapter::gaia_plan_bytes;
use hades_gate::token::{plan_hash, Claims};
use hades_gate::{safety, Action, Approver, Decision, Request, Verdict};
use openmind_nerve::config::Config;
use openmind_nerve::plans::{from_state, Family, Plan, Plans, ACTION};
use openmind_nerve::replay::replay_dir;
use serde_json::{json, Value};
use std::path::PathBuf;

fn comp(name: &str, unit: Option<&str>, port: Option<u16>) -> Value {
    json!({"name": name, "unit": unit, "port": port, "tunnel": null, "host": null})
}

fn finding(id: &str, sev: &str, comp: &str, cause: Option<&str>) -> Value {
    json!({"id": id, "severity": sev, "component": comp, "title": "", "evidence": [],
           "fix": "", "incident_cause": cause, "source": "test"})
}

fn components() -> Value {
    json!([
        comp("openmind-memory", Some("openmind-memory.service"), Some(8420)),
        comp("galion-one", Some("galion-one.service"), Some(8091)),
        comp("galion-one-mouth", None, Some(8002)),
        comp("tuner", Some("tuner.service"), Some(8650)),
        comp("hades-gate", Some("hades-gate.service"), Some(8700)),
        {"name": "cloudflared", "unit": "cloudflared.service", "port": null, "tunnel": {"ha_connections": 4}, "host": null},
        {"name": "lxc-host", "unit": null, "port": null, "tunnel": null, "host": {"disk_total_kb": 1}},
    ])
}

/// A state with one trigger per family.
fn all_families() -> Value {
    let mut unmanaged = finding("unmanaged-listeners", "warn", "host", None);
    unmanaged["title"] = json!("2 loopback listener(s) no monitored unit owns: :8652 :8653");
    let mut stray = finding("stray-owner", "warn", "host", None);
    stray["title"] = json!("1 listener(s) run as an unexpected user: :8653 (uid 1000)");
    let mut edge = finding("edge-dead-origins", "error", "cloudflared", Some("edge"));
    edge["evidence"] = json!([
        "origin galion-app (port 4000) refused 12 dial(s), 6 failed request(s); hosts: galion.app, www.galion.app",
        "origin ? (port 3011) refused 2 dial(s), 1 failed request(s); hosts: -",
        "origin console (port 8080) refused 1 dial(s), 1 failed request(s); hosts: console.galion.app",
        "ports with a listener now: [8080] (they may be flapping)",
        "cloudflared counters since its start: 9 of 10 requests failed (90%)"
    ]);
    json!({
        "components": components(),
        "incidents": {"open": [
            {"id": "inc-mouth", "cause": "galion-one", "correlation": {"findings": [{"finding": "bleeding:stuck-loading-galion-one-mouth"}]}},
            {"id": "inc-edge", "cause": "edge", "correlation": {"findings": [{"finding": "bleeding:edge-dead-origins"}]}},
            {"id": "inc-flood", "cause": "ddos:tuner", "correlation": {"findings": []}},
        ]},
        "cortex": {
            "vulnerabilities": [finding("public-listener-8420", "error", "host", None), unmanaged, stray],
            "bleeding": [
                finding("stuck-loading-galion-one-mouth", "error", "galion-one-mouth", Some("galion-one")),
                finding("memory-trend-openmind-memory", "warn", "openmind-memory", Some("openmind-memory")),
                edge,
            ],
            "ddos": {"firing": true, "streams": [
                {"stream": "tuner", "component": "tuner", "level": "flood", "threshold_rps": 10.0, "baseline_rps": 0.5,
                 "shape": "single-source", "top_ips": [{"ip": "203.0.113.7", "requests": 2900}]},
                {"stream": "openmind-memory", "component": "openmind-memory", "level": "normal", "threshold_rps": 10.0, "baseline_rps": 0.1}
            ]},
            "exposure": {"unmanaged_ports": [8652, 8653]},
        }
    })
}

fn rq<'a>(target: &'a str, plan: &'a [u8], token: &'a str) -> Request<'a> {
    Request {
        action: ACTION,
        target,
        plan,
        token,
    }
}

fn well_formed(p: &Plan) {
    let text = p.plan_text();
    assert_eq!(
        text.as_bytes(),
        gaia_plan_bytes(&p.steps).as_slice(),
        "{}",
        p.id
    );
    let action = Action::parse(ACTION).expect("known action");
    assert!(action.is_allow_listed(), "heal-apply is allow-listed");
    assert!(
        hades_gate::token::valid_target(&p.target),
        "{}: target {}",
        p.id,
        p.target
    );
    assert_eq!(
        safety::review(&p.target, text.as_bytes()),
        Ok(()),
        "{}: hades-gate safety refused:\n{text}",
        p.id
    );
    assert!(
        p.id.starts_with(&format!("plan-{}-", p.family.as_str())),
        "{}",
        p.id
    );
    assert!(!p.findings.is_empty(), "{}", p.id);
    assert!(
        p.steps[0].starts_with(&format!("plan {}", p.family.as_str())),
        "{}",
        p.id
    );
    for must in ["precondition:", "verify:", "rollback:"] {
        assert!(
            p.steps.iter().any(|s| s.starts_with(must)),
            "{}: no {must}",
            p.id
        );
    }
    let lower = text.to_ascii_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .collect();
    for banned in [
        "iptables", "nft", "ufw", "ssh", "sshd", "kill", "pkill", "killall", "rm", "chmod",
        "chown", "passwd", "dig", "nsupdate",
    ] {
        assert!(
            !words.contains(&banned),
            "{}: word {banned:?} in\n{text}",
            p.id
        );
    }
    for banned in [
        "authorized_keys",
        "dns record change",
        "/etc/ssh",
        "sigkill",
    ] {
        assert!(!lower.contains(banned), "{}: {banned:?} in\n{text}", p.id);
    }
    // No timestamps in the signed text: the hash is stable cycle to cycle.
    assert!(
        !text.contains("+02:00") && !text.contains("+01:00"),
        "{}",
        p.id
    );
    let j = p.to_json();
    assert_eq!(j["executed"], false);
    assert_eq!(j["action"], "heal-apply");
    assert_eq!(j["plan_bytes"], text.len());
}

fn by(ps: &Plans, f: Family) -> Vec<&Plan> {
    ps.plans.iter().filter(|p| p.family == f).collect()
}

#[test]
fn every_family_yields_a_well_formed_plan() {
    let ps = from_state(&all_families());
    for p in &ps.plans {
        well_formed(p);
    }
    for f in Family::ALL {
        assert!(
            !by(&ps, f).is_empty(),
            "{} missing: {:#?}",
            f.as_str(),
            ps.plans
        );
    }
    let ids: Vec<&str> = ps.plans.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "plan-edge-rate-limit-cloudflare-edge-tuner",
            "plan-unit-restart-galion-one.service",
            "plan-ingress-rule-removal-cloudflared",
            "plan-rebind-loopback-openmind-memory.service",
            "plan-unit-restart-openmind-memory.service",
            "plan-stray-stop-stray-listener-8652",
            "plan-stray-stop-stray-listener-8653",
        ]
    );
    // unit-restart: the mouth has no unit, its group's unit is restarted.
    let mouth = &by(&ps, Family::UnitRestart)[0];
    assert_eq!(
        (mouth.target.as_str(), mouth.incident.as_deref()),
        ("galion-one.service", Some("inc-mouth"))
    );
    assert!(mouth.notes.iter().any(|n| n.contains("no unit of its own")));
    // stray-stop: :8653 is both unmanaged and stray-owned -> one plan, two findings.
    let s8653 = ps
        .plans
        .iter()
        .find(|p| p.target == "stray-listener-8653")
        .unwrap();
    assert_eq!(
        s8653.findings,
        [
            "vulnerability:unmanaged-listeners",
            "vulnerability:stray-owner"
        ]
    );
    assert!(s8653.incident.is_none(), "finding-only problem");
    // ingress: 4000 (2 hosts) and 3011 (no host); 8080 has a listener now -> skipped.
    let ing = &by(&ps, Family::IngressRuleRemoval)[0];
    let t = ing.plan_text();
    assert!(
        t.contains("for galion.app, www.galion.app whose service is http://localhost:4000"),
        "{t}"
    );
    assert!(t.contains("whose service is http://localhost:3011"), "{t}");
    assert!(
        !t.contains("8080") && !t.contains("console.galion.app"),
        "{t}"
    );
    assert!(t.contains("DNS records of these hostnames are not changed"));
    assert_eq!(ing.incident.as_deref(), Some("inc-edge"));
    assert!(ps
        .skipped
        .iter()
        .any(|s| s.finding == "bleeding:edge-dead-origins:8080"));
    // edge rate limit: flood stream only; single source -> one ban line.
    let rl = by(&ps, Family::EdgeRateLimit);
    assert_eq!(rl.len(), 1);
    assert_eq!(
        (rl[0].target.as_str(), rl[0].severity.as_str()),
        ("cloudflare-edge", "critical")
    );
    assert!(rl[0]
        .plan_text()
        .contains("more than 100 requests per 10 s"));
    assert!(rl[0]
        .plan_text()
        .contains("block client address 203.0.113.7"));
    assert_eq!(rl[0].incident.as_deref(), Some("inc-flood"));
    // rebind: the port's owning unit.
    let rb = &by(&ps, Family::RebindLoopback)[0];
    assert_eq!(rb.target, "openmind-memory.service");
    assert!(rb
        .plan_text()
        .contains("set the bind address of openmind-memory.service to 127.0.0.1:8420"));
    // Same state twice: byte-identical plans (same approval hash).
    assert_eq!(from_state(&all_families()), ps);
    assert_eq!(ps.to_json()["by_family"]["stray-stop"], 2);
    assert!(
        !ps.skipped
            .iter()
            .any(|s| s.finding.starts_with("incident:")),
        "every open incident has a plan"
    );
}

#[test]
fn unknown_and_excluded_findings_yield_no_plan() {
    let others = [
        (
            "vulnerabilities",
            "rustsec-RUSTSEC-2026-0001-zip",
            "error",
            "host",
            None,
        ),
        (
            "vulnerabilities",
            "drift-hermes-rs",
            "warn",
            "hermes-rs",
            None,
        ),
        (
            "vulnerabilities",
            "hades-token-abuse",
            "critical",
            "hades-gate",
            Some("hades"),
        ),
        (
            "vulnerabilities",
            "hades-safety-refusals",
            "error",
            "hades-gate",
            Some("hades"),
        ),
        (
            "vulnerabilities",
            "sandbox-escape-attempts",
            "error",
            "hermes-rs",
            Some("sandbox"),
        ),
        (
            "vulnerabilities",
            "scanner-probes-console",
            "error",
            "console",
            Some("scan:console"),
        ),
        (
            "vulnerabilities",
            "dev-server-console",
            "warn",
            "console",
            None,
        ),
        ("vulnerabilities", "deleted-binary", "error", "host", None),
        (
            "vulnerabilities",
            "surface-dns",
            "warn",
            "cloudflared",
            Some("edge"),
        ),
        (
            "vulnerabilities",
            "brand-new-sensor-finding",
            "critical",
            "host",
            None,
        ),
        (
            "bleeding",
            "restarts-galion-desk",
            "error",
            "galion-desk",
            Some("galion-desk"),
        ),
        (
            "bleeding",
            "error-rate-cloudflared",
            "warn",
            "cloudflared",
            Some("edge"),
        ),
        (
            "bleeding",
            "host-pressure-lxc-host",
            "warn",
            "lxc-host",
            Some("lxc-host"),
        ),
        (
            "bleeding",
            "memory-heavy-console",
            "info",
            "console",
            Some("console"),
        ),
        (
            "bleeding",
            "sshd-auth-log-growth",
            "warn",
            "host",
            Some("ssh-flood"),
        ),
        (
            "bleeding",
            "hades-ledger-failing",
            "error",
            "hades-gate",
            Some("hades"),
        ),
        (
            "ddos",
            "sshd-credential-flood",
            "critical",
            "host",
            Some("ssh-flood"),
        ),
        ("ddos", "conn-rate-8642", "warn", "hermes-rs", None),
        (
            "ddos",
            "edge-host-rate",
            "warn",
            "cloudflared",
            Some("ddos:edge-cloudflared"),
        ),
    ];
    let mut cortex =
        json!({"vulnerabilities": [], "bleeding": [], "ddos": {"findings": [], "streams": []}});
    for (sec, id, sev, c, cause) in others {
        let arr = if sec == "ddos" {
            &mut cortex["ddos"]["findings"]
        } else {
            &mut cortex[sec]
        };
        arr.as_array_mut().unwrap().push(finding(id, sev, c, cause));
    }
    // An ssh flood incident and a flood stream no incident stands behind.
    cortex["ddos"]["streams"] = json!([{"stream": "galion-desk", "component": "galion-desk", "level": "flood", "threshold_rps": 10.0}]);
    let state = json!({"components": components(), "cortex": cortex,
        "incidents": {"open": [{"id": "inc-ssh", "cause": "ssh-flood", "correlation": {"findings": [{"finding": "ddos:sshd-credential-flood"}]}}]}});
    let ps = from_state(&state);
    assert!(ps.plans.is_empty(), "{:#?}", ps.plans);
    // Every warn+ finding is accounted for in `skipped`, info ones silently.
    let skipped: Vec<&str> = ps.skipped.iter().map(|s| s.finding.as_str()).collect();
    for (sec, id, sev, ..) in others {
        let kind = match sec {
            "vulnerabilities" => "vulnerability",
            "bleeding" => "bleeding",
            _ => "ddos",
        };
        let key = format!("{kind}:{id}");
        if sev == "info" || id == "edge-host-rate" {
            assert!(!skipped.contains(&key.as_str()), "{key}");
        } else {
            assert!(skipped.contains(&key.as_str()), "{key} not in {skipped:?}");
        }
    }
    let reason = |k: &str| {
        ps.skipped
            .iter()
            .find(|s| s.finding == k)
            .unwrap()
            .reason
            .clone()
    };
    assert!(reason("ddos:sshd-credential-flood").contains("no template touches ssh"));
    assert!(reason("bleeding:restarts-galion-desk").contains("restart loop"));
    assert!(reason("ddos:galion-desk").contains("not behind an open incident"));
    assert!(reason("incident:ssh-flood").contains("manual"));
    // Empty / garbage input: nothing, no panic.
    for v in [
        json!(null),
        json!({}),
        json!({"cortex": {"bleeding": "x"}, "incidents": 3}),
        json!([1, 2]),
    ] {
        assert!(from_state(&v).plans.is_empty());
    }
}

#[test]
fn protected_names_never_get_a_plan() {
    let mut edge = finding("edge-dead-origins", "error", "cloudflared", Some("edge"));
    edge["evidence"] = json!(["origin ? (port 2222) refused 5 dial(s), 5 failed request(s); hosts: ssh.galion.app, admin.galion.app"]);
    let state = json!({
        "components": components(),
        "cortex": {
            "vulnerabilities": [finding("public-listener-22", "error", "host", None), finding("unmanaged-listeners", "warn", "host", None)],
            "bleeding": [finding("stuck-loading-hades-gate", "error", "hades-gate", Some("hades-gate")), edge,
                         finding("stuck-loading-cloudflared", "error", "cloudflared", Some("edge")),
                         finding("memory-trend-lxc-host", "warn", "lxc-host", Some("lxc-host"))],
            "exposure": {"unmanaged_ports": [22]},
        }
    });
    let ps = from_state(&state);
    // The only thing left: an ingress removal for :2222 with no hostname.
    let ids: Vec<&str> = ps.plans.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(
        ids,
        ["plan-ingress-rule-removal-cloudflared"],
        "{:#?}",
        ps.skipped
    );
    let t = ps.plans[0].plan_text();
    assert!(!t.contains("ssh") && !t.contains("admin"), "{t}");
    well_formed(&ps.plans[0]);
    for k in [
        "vulnerability:public-listener-22",
        "bleeding:stuck-loading-hades-gate",
        "bleeding:stuck-loading-cloudflared",
        "bleeding:memory-trend-lxc-host",
        "bleeding:edge-dead-origins:ssh.galion.app",
        "bleeding:edge-dead-origins:admin.galion.app",
        "vulnerability:unmanaged-listeners:22",
    ] {
        assert!(
            ps.skipped.iter().any(|s| s.finding == k),
            "{k} not skipped: {:?}",
            ps.skipped
        );
    }
    // Parity: every protected label is refused by the real gate as a target.
    for l in openmind_nerve::plans::PROTECTED_LABELS {
        assert!(
            safety::review_target(l).is_err(),
            "{l} not protected by hades-gate"
        );
    }
}

#[test]
fn a_template_goes_through_a_real_single_use_approval() {
    let ps = from_state(&all_families());
    let dir = std::env::temp_dir().join(format!("t42-approval-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Throwaway key from a fixed test seed (never a real admin key).
    let admin = SigningKey::from_bytes(&[0x42; 32]);
    let gate = Approver::new(
        admin.verifying_key(),
        dir.join("ledger.jsonl"),
        dir.join("DISABLED"),
        900,
    )
    .unwrap();
    let now = 1_792_000_000;
    for (n, p) in ps.plans.iter().enumerate() {
        let plan = gaia_plan_bytes(&p.steps);
        // No token: dry-run (the default), never applied.
        assert_eq!(
            gate.approve(rq(&p.target, &plan, ""), now),
            Verdict::DryRun,
            "{}",
            p.id
        );
        let tok = Claims {
            action: ACTION.into(),
            target: p.target.clone(),
            plan_hash: plan_hash(&plan),
            nonce: [n as u8 + 1; 16],
            issued_at: now,
            expires_at: now + 300,
        }
        .sign(&admin);
        let v = gate.approve(rq(&p.target, &plan, &tok), now + 1);
        assert_eq!(v.decision(), Decision::WouldApply, "{}: {v:?}", p.id);
        // Single use.
        let again = gate.approve(rq(&p.target, &plan, &tok), now + 2);
        assert_eq!(again.decision(), Decision::Refused, "{}", p.id);
        // One changed byte (next cycle with a different port) -> wrong scope.
        let mut other = plan.clone();
        other.push(b'x');
        let v = gate.approve(
            Request {
                action: ACTION,
                target: &p.target,
                plan: &other,
                token: &tok,
            },
            now + 3,
        );
        assert_eq!(v.decision(), Decision::Refused, "{}", p.id);
    }
    assert_eq!(ps.plans.len(), 7);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn golden_replay_yields_its_plans_through_the_engine() {
    let fx = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cortex-replay");
    let data = std::env::temp_dir().join(format!("t42-replay-{}", std::process::id()));
    let d = data.display().to_string();
    let cfg = Config::from_env(&|k| (k == "NERVE_DATA_DIR").then(|| d.clone())).unwrap();
    let out = replay_dir(&fx, cfg).unwrap();
    let sh = out.engine.shared.lock().unwrap();
    let p = &sh.state["cortex_plans"];
    assert_eq!(p["executed"], false);
    let ids: Vec<&str> = p["plans"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "plan-edge-rate-limit-cloudflare-edge-tuner",
            "plan-unit-restart-galion-one.service",
            "plan-ingress-rule-removal-cloudflared",
            "plan-rebind-loopback-listener-9999",
            "plan-stray-stop-stray-listener-8652",
            "plan-stray-stop-stray-listener-8653",
        ]
    );
    // Same function on the served state JSON gives the same answer.
    assert_eq!(from_state(&sh.state).to_json(), *p);
    for x in p["plans"].as_array().unwrap() {
        let steps: Vec<String> = serde_json::from_value(x["steps"].clone()).unwrap();
        let text = steps.join("\n");
        assert_eq!(
            safety::review(x["target"].as_str().unwrap(), text.as_bytes()),
            Ok(()),
            "{}",
            x["id"]
        );
        assert!(
            x["incident"].is_null()
                || sh.state["incidents"]["open"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|i| i["id"] == x["incident"])
        );
    }
    // The report (self-eval golden) is untouched: plans live in the state only.
    assert!(sh.report.get("cortex_plans").is_none());
    let _ = std::fs::remove_dir_all(&data);
}
