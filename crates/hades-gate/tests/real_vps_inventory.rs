// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Option B against REAL VPS data. Runs only with `GALION_REAL=1`.
//!
//! Data source (read-only, never written on the VPS):
//! * `HADES_VPS_SSH=<ssh host>`: runs `systemctl list-units` and a loopback
//!   `curl` GET of the nerve `/state` (127.0.0.1:8670) over ssh; or
//! * `HADES_VPS_UNITS_FILE` + `HADES_NERVE_STATE_FILE`: box copies of the same.
//!
//! It checks the typed plan grammar against the services that really run:
//! every nerve-managed unit is a valid typed [`Service`], an [`Inventory`] built
//! from them accepts `restart <unit>` plans and refuses security units the
//! deny-list alone lets through, and it reports which corpus plans name units
//! that do not exist on the VPS. No secrets are read; no state is changed.

#[path = "support/typed_legit.rs"]
mod typed_legit;

use std::process::Command;

use hades_gate::typed::{Inventory, Plan, Service, MAGIC};

fn ssh(host: &str, cmd: &str) -> String {
    let o = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", host, cmd])
        .output()
        .expect("run ssh");
    assert!(o.status.success(), "ssh {cmd:?} failed");
    String::from_utf8(o.stdout).expect("utf-8")
}

fn load() -> (Vec<String>, serde_json::Value) {
    let (units, state) = if let Ok(host) = std::env::var("HADES_VPS_SSH") {
        (
            ssh(
                &host,
                "systemctl list-units --type=service --all --no-legend --plain",
            ),
            ssh(&host, "curl -s -m 5 http://127.0.0.1:8670/state"),
        )
    } else {
        let u =
            std::env::var("HADES_VPS_UNITS_FILE").expect("HADES_VPS_SSH or HADES_VPS_UNITS_FILE");
        let s = std::env::var("HADES_NERVE_STATE_FILE").expect("HADES_NERVE_STATE_FILE");
        (
            std::fs::read_to_string(u).unwrap(),
            std::fs::read_to_string(s).unwrap(),
        )
    };
    let mut names: Vec<String> = units
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .map(|u| u.strip_suffix(".service").unwrap_or(u).to_owned())
        .collect();
    names.sort();
    names.dedup();
    (
        names,
        serde_json::from_str(&state).expect("nerve /state JSON"),
    )
}

#[test]
fn real_typed_plans_against_vps_inventory() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let (units, state) = load();
    assert!(units.len() > 20, "too few units: {}", units.len());
    eprintln!(
        "REAL vps nerve ts={} mode={} cycle={}",
        state["ts"], state["mode"], state["cycle"]
    );

    // 1. The deny-list alone on the real unit list.
    let (ok, refused): (Vec<&String>, Vec<&String>) =
        units.iter().partition(|u| Service::new(u).is_ok());
    eprintln!(
        "REAL vps units={} deny-list accepts={} refuses={}",
        units.len(),
        ok.len(),
        refused.len()
    );
    let refused_names: Vec<&str> = refused.iter().map(|s| s.as_str()).collect();
    eprintln!("REAL vps refused: {}", refused_names.join(" "));
    let security = [
        "firewalld",
        "fail2ban",
        "auditd",
        "apparmor",
        "nftables",
        "iptables",
        "ufw",
        "ssh",
        "cron",
    ];
    let present: Vec<&str> = security
        .iter()
        .copied()
        .filter(|s| units.iter().any(|u| u == s))
        .collect();
    let leaked: Vec<&str> = present
        .iter()
        .copied()
        .filter(|s| Service::new(s).is_ok())
        .collect();
    eprintln!("REAL vps security units present={present:?} deny-list lets through={leaked:?}");
    for must in ["ssh", "nftables", "iptables", "ufw"] {
        if units.iter().any(|u| u == must) {
            assert!(Service::new(must).is_err(), "{must} must stay refused");
        }
    }

    // 2. Inventory from the nerve-managed units (what HADES is for).
    let comps = state["components"].as_array().expect("components");
    let managed: Vec<(String, Option<u64>)> = comps
        .iter()
        .filter_map(|c| {
            let unit = c["unit"].as_str()?;
            let name = unit.strip_suffix(".service").unwrap_or(unit).to_owned();
            Some((name, c["port"].as_u64()))
        })
        .collect();
    // A nerve unit that systemd does not list (not installed) stays out.
    let (managed, missing): (Vec<_>, Vec<_>) = managed
        .into_iter()
        .partition(|(m, _)| units.iter().any(|u| u == m));
    let missing: Vec<&str> = missing.iter().map(|(m, _)| m.as_str()).collect();
    eprintln!("REAL nerve units not installed on the VPS: {missing:?}");
    assert!(!managed.is_empty());
    let inv = Inventory::new(managed.iter().map(|(n, _)| n.as_str())).expect("inventory");
    eprintln!(
        "REAL inventory ({}): {}",
        inv.names().len(),
        inv.names().join(" ")
    );

    let mut plans_ok = 0;
    for (name, port) in &managed {
        let text = match port {
            Some(p) => format!("{MAGIC}\nrestart {name}\nverify 127.0.0.1:{p} /health"),
            None => format!("{MAGIC}\nrestart {name}"),
        };
        let p = Plan::parse(text.as_bytes()).expect("real unit typed plan parses");
        inv.check(&p).expect("real unit is in inventory");
        assert_eq!(p.canonical(), text);
        plans_ok += 1;
    }
    eprintln!(
        "REAL typed restart plans for real units: {plans_ok}/{}",
        managed.len()
    );

    // 3. Security units the deny-list lets through are refused by inventory.
    let mut inv_refused = 0;
    for s in &leaked {
        let p = Plan::parse(format!("{MAGIC}\nrestart {s}").as_bytes()).unwrap();
        assert!(inv.check(&p).is_err(), "{s} must not be in inventory");
        inv_refused += 1;
    }
    eprintln!(
        "REAL inventory refuses deny-list leaks: {inv_refused}/{}",
        leaked.len()
    );

    // 4. Corpus legit typed plans vs the real inventory.
    let legit = typed_legit::legit_typed();
    let mut stale = Vec::new();
    for (target, plan) in &legit {
        if let Err(e) = inv.check(plan) {
            stale.push(format!("{target}: {}", e.reason()));
        }
    }
    eprintln!(
        "REAL corpus legit plans naming a unit not on the VPS: {}/{}",
        stale.len(),
        legit.len()
    );
    for s in &stale {
        eprintln!("REAL   stale: {s}");
    }
}
