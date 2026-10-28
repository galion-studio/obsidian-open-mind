// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Never-unroute hostnames (owner decision D-T42-4, Mate 2026-10-21 17:04 +02:00).
//!
//! The route-removal plan (`ingress-rule-removal`) never targets the VPS
//! itself, DNS, the mail server or the backup server; every other hostname
//! may still be proposed. The list is config
//! (`deploy/nerve/plans-protected-hosts.conf`, `NERVE_PLANS_PROTECTED_HOSTS_FILE`,
//! `NERVE_PLANS_PROTECTED_HOSTS`), never addresses.

use hades_gate::adapter::gaia_plan_bytes;
use hades_gate::safety;
use openmind_nerve::config::Config;
use openmind_nerve::plans::{from_state, from_state_with, Family, Plan, Plans};
use openmind_nerve::protected_hosts::{ProtectedHosts, BUILTIN, REQUIRED};
use serde_json::{json, Value};

/// A state whose only finding is `edge-dead-origins` with these origin lines.
fn edge_state(lines: &[&str]) -> Value {
    let ev: Vec<Value> = lines.iter().map(|l| json!(l)).collect();
    json!({
        "components": [
            {"name": "cloudflared", "unit": "cloudflared.service", "port": null, "tunnel": {"ha_connections": 4}, "host": null}
        ],
        "incidents": {"open": [
            {"id": "inc-edge", "cause": "edge", "correlation": {"findings": [{"finding": "bleeding:edge-dead-origins"}]}}
        ]},
        "cortex": {"vulnerabilities": [], "bleeding": [
            {"id": "edge-dead-origins", "severity": "error", "component": "cloudflared", "title": "",
             "evidence": ev, "fix": "", "incident_cause": "edge", "source": "test"}
        ]}
    })
}

fn origin(port: u16, hosts: &str) -> String {
    format!("origin x (port {port}) refused 3 dial(s), 1 failed request(s); hosts: {hosts}")
}

fn ingress(ps: &Plans) -> Option<&Plan> {
    ps.plans
        .iter()
        .find(|p| p.family == Family::IngressRuleRemoval)
}

fn remove_lines(p: &Plan) -> Vec<&str> {
    p.steps
        .iter()
        .filter(|s| s.starts_with("remove only"))
        .map(String::as_str)
        .collect()
}

fn gate_ok(p: &Plan) {
    let text = p.plan_text();
    assert_eq!(text.as_bytes(), gaia_plan_bytes(&p.steps).as_slice());
    assert_eq!(
        safety::review(&p.target, text.as_bytes()),
        Ok(()),
        "hades-gate refused:\n{text}"
    );
    assert!(p
        .steps
        .iter()
        .any(|s| s.starts_with("precondition: no rule to remove names a never-unroute hostname")));
}

fn skipped(ps: &Plans, k: &str) -> bool {
    ps.skipped.iter().any(|s| s.finding == k)
}

#[test]
fn builtin_config_covers_the_four_categories_without_addresses() {
    let p = ProtectedHosts::builtin();
    assert_eq!(p.warning, None, "{:?}", p.warning);
    for c in REQUIRED {
        assert!(!p.in_category(c).is_empty(), "category {c} empty");
    }
    // No IPv4 dotted quad or IPv6 literal anywhere in the committed file.
    for tok in BUILTIN.split(|c: char| c.is_whitespace() || c == ',' || c == '=') {
        assert!(tok.parse::<std::net::IpAddr>().is_err(), "address {tok:?}");
    }
    assert!(BUILTIN.contains("SPDX-License-Identifier: MIT"));
}

#[test]
fn protected_hostnames_are_never_unrouted_everything_else_is() {
    // Live shape (VPS 2026-10-21): galion.studio on closed :4001, plus routes to
    // protected names on other closed ports.
    let lines = [
        origin(4001, "galion.studio, www.galion.studio"),
        origin(4101, "ip50.vp3.titanaxe.com"),
        origin(4102, "mx.zoho.eu, docs.galion.app"),
        origin(4103, "PEYTON.NS.CLOUDFLARE.COM."),
        origin(4104, "route2.mx.cloudflare.net, ip50"),
    ];
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let ps = from_state(&edge_state(&refs));
    let p = ingress(&ps).expect("an ingress plan for the unprotected names");
    gate_ok(p);
    let rm = remove_lines(p);
    assert_eq!(
        rm,
        [
            "remove only the ingress rule(s) for galion.studio, www.galion.studio whose service is http://localhost:4001",
            "remove only the ingress rule(s) for docs.galion.app whose service is http://localhost:4102",
        ],
        "{:#?}",
        ps.skipped
    );
    for port in [4101, 4103, 4104] {
        assert!(
            skipped(&ps, &format!("bleeding:edge-dead-origins:{port}")),
            "{port}: {:#?}",
            ps.skipped
        );
    }
    for h in [
        "ip50.vp3.titanaxe.com",
        "mx.zoho.eu",
        "peyton.ns.cloudflare.com.",
        "route2.mx.cloudflare.net",
        "ip50",
    ] {
        assert!(
            ps.skipped.iter().any(|s| s.finding.to_ascii_lowercase()
                == format!("bleeding:edge-dead-origins:{h}")
                && s.reason.contains("never-unroute")),
            "{h}: {:#?}",
            ps.skipped
        );
    }
    let j = ps.to_json();
    assert_eq!(j["protected_hosts"]["source"], "builtin");
    assert!(j["protected_hosts"]["categories"]["mail"]
        .as_array()
        .unwrap()
        .contains(&json!("mx.zoho.eu")));
}

#[test]
fn every_builtin_name_is_refused_on_its_own_port() {
    let keep = ProtectedHosts::builtin();
    for (i, h) in keep.names().iter().enumerate() {
        let port = 30000 + i as u16;
        let l = origin(port, h);
        let ps = from_state(&edge_state(&[&l]));
        assert!(ingress(&ps).is_none(), "{h} got a plan: {:#?}", ps.plans);
        assert!(skipped(&ps, &format!("bleeding:edge-dead-origins:{port}")));
    }
}

#[test]
fn config_list_from_env_is_what_the_plan_obeys() {
    // Adding galion.studio through config (one line) keeps its rule.
    let cfg = Config::from_env(&|k| match k {
        "NERVE_PLANS_PROTECTED_HOSTS" => Some("galion.studio".into()),
        _ => None,
    })
    .unwrap();
    assert_eq!(cfg.plans_protected.category("galion.studio"), Some("extra"));
    let lines = [
        origin(4001, "galion.studio"),
        origin(4042, "docs.galion.app"),
    ];
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let state = edge_state(&refs);
    let ps = from_state_with(&state, &cfg.plans_protected);
    let p = ingress(&ps).unwrap();
    gate_ok(p);
    assert_eq!(
        remove_lines(p),
        ["remove only the ingress rule(s) for docs.galion.app whose service is http://localhost:4042"]
    );
    assert!(
        p.plan_text()
            .contains("galion.studio; if one does, keep that rule")
            || p.plan_text().contains("galion.studio, ")
    );
    // Default config: galion.studio is not on the D-T42-4 list, so it may be proposed.
    let def = Config::from_env(&|_| None).unwrap();
    assert!(!def.plans_protected.is_protected("galion.studio"));
    let ps = from_state_with(&state, &def.plans_protected);
    assert_eq!(remove_lines(ingress(&ps).unwrap()).len(), 2);
}

#[test]
fn a_bad_config_file_never_weakens_the_list() {
    let d = std::env::temp_dir().join(format!("nerve-ph-it-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let bad = d.join("bad.conf");
    std::fs::write(&bad, "vps = 192.0.2.10\n").unwrap();
    let bp = bad.display().to_string();
    let cfg = Config::from_env(&|k| (k == "NERVE_PLANS_PROTECTED_HOSTS_FILE").then(|| bp.clone()))
        .unwrap();
    assert_eq!(cfg.plans_protected.source, "builtin");
    assert!(cfg.plans_protected.is_protected("ip50.vp3.titanaxe.com"));
    let l = origin(4101, "ip50.vp3.titanaxe.com");
    let ps = from_state_with(&edge_state(&[&l]), &cfg.plans_protected);
    assert!(ingress(&ps).is_none());
    assert!(ps.to_json()["protected_hosts"]["warning"]
        .as_str()
        .unwrap()
        .contains("built-in list kept"));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn plan_text_is_stable_and_hostless_ports_still_carry_the_guard() {
    // An origin with no hostname in the evidence is removed by port; the
    // precondition names every never-unroute hostname so the operator keeps
    // such a rule if the config shows one.
    let l = origin(3011, "-");
    let s = edge_state(&[&l]);
    let a = from_state(&s);
    let b = from_state(&s);
    let p = ingress(&a).unwrap();
    gate_ok(p);
    assert_eq!(p.plan_text(), ingress(&b).unwrap().plan_text());
    let guard = p
        .steps
        .iter()
        .find(|x| x.starts_with("precondition: no rule to remove"))
        .unwrap();
    for h in ProtectedHosts::builtin().names() {
        assert!(guard.contains(h), "{h} missing from {guard}");
    }
}
