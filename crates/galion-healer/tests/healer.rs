// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

use galion_healer::actions::{clear_stale_gateway_lock, json_brace_repair, restart_unit};
use galion_healer::{config, run_cycle, Deps};
use openmind_survival::Runner;
use std::cell::RefCell;
use std::path::PathBuf;

struct Rec(RefCell<Vec<String>>);
impl Runner for Rec {
    fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
        self.0
            .borrow_mut()
            .push(format!("{program} {}", args.join(" ")));
        Ok(String::new())
    }
}

fn out(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-out/galion-healer")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn staged_config_and_refusals() {
    let f = config::parse(include_str!("../../../deploy/staged/healer.toml")).unwrap();
    assert_eq!(f.probe.len(), 5);
    assert!(!f.healer.apply);
    assert!(f.restart.is_empty());
    let base = "[[probe]]\nname=\"one\"\nport=1\npath=\"/\"\n";
    for unit in [
        "hermes",
        "cloudflared",
        "memory-gateway",
        "whisper-x",
        "sshd",
    ] {
        let t = format!("{base}[[restart]]\nprobe=\"one\"\nunit=\"{unit}\"\n");
        assert!(config::parse(&t).is_err(), "{unit}");
    }
    assert!(config::parse(&format!("{base}[[restart]]\nprobe=\"one\"\nunit=\"a;b\"\n")).is_err());
    assert!(config::parse(&format!(
        "{base}[[restart]]\nprobe=\"zz\"\nunit=\"galion-one\"\n"
    ))
    .is_err());
    assert!(config::parse(&format!(
        "{base}[[restart]]\nprobe=\"one\"\nunit=\"galion-one\"\n"
    ))
    .is_ok());
    assert!(config::parse(&format!("[healer]\nrestart_backend=\"kill\"\n{base}")).is_err());
}

#[test]
fn gateway_lock_repair() {
    let d = out("lock");
    let proc_root = d.join("proc");
    std::fs::create_dir_all(proc_root.join("77")).unwrap();
    let home = d.join("hermes");
    std::fs::create_dir_all(home.join("gateway.sock")).unwrap(); // a dir: never removed
    std::fs::write(home.join("gateway.lock"), "").unwrap();
    std::fs::write(home.join("gateway.pid"), "{\"pid\": 77}").unwrap();
    assert_eq!(
        clear_stale_gateway_lock(&home, true, &proc_root, true),
        "skip: gateway up"
    );
    assert_eq!(
        clear_stale_gateway_lock(&home, false, &proc_root, true),
        "skip: pid 77 alive"
    );
    std::fs::write(home.join("gateway.pid"), "{\"pid\": 99}").unwrap();
    let r = clear_stale_gateway_lock(&home, false, &proc_root, false);
    assert_eq!(r, "would remove gateway.pid,gateway.lock (pid 99 dead)");
    assert!(home.join("gateway.pid").exists());
    let r = clear_stale_gateway_lock(&home, false, &proc_root, true);
    assert_eq!(r, "removed gateway.pid,gateway.lock (pid 99 dead)");
    assert!(!home.join("gateway.pid").exists() && home.join("gateway.sock").is_dir());
}

#[test]
fn restart_backends() {
    let d = out("restart");
    let rec = Rec(RefCell::new(vec![]));
    assert_eq!(
        restart_unit("galion-one", "systemd", &d, &rec, false),
        "would restart via systemd"
    );
    assert!(rec.0.borrow().is_empty());
    assert_eq!(
        restart_unit("galion-one", "systemd", &d, &rec, true),
        "restarted via systemd"
    );
    assert_eq!(rec.0.borrow()[0], "systemctl restart galion-one");
    assert_eq!(
        restart_unit("hermes", "systemd", &d, &rec, true),
        "refuse: protected"
    );
    assert_eq!(
        restart_unit("galion-one", "supervisor", &d, &rec, true),
        "requested from supervisor"
    );
    assert!(d.join("restart/galion-one").exists());
}

#[test]
fn json_repair_fixture_only() {
    let d = out("json");
    let fx = d.join("heal-fixture");
    let root = fx.join("case1");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.json"), "{\"a\": 1\n").unwrap();
    let (v, c) = json_brace_repair(&fx, &root, "a.json", "JSONDecodeError", false);
    assert_eq!(
        (v["ok"].as_bool(), v["dry_run"].as_bool(), c),
        (Some(true), Some(true), 0)
    );
    assert_eq!(
        std::fs::read_to_string(root.join("a.json")).unwrap(),
        "{\"a\": 1\n"
    );
    let (v, c) = json_brace_repair(&fx, &root, "a.json", "JSONDecodeError", true);
    assert_eq!(
        (v["ok"].as_bool(), v["edit"].as_bool(), c),
        (Some(true), Some(true), 0)
    );
    assert_eq!(
        std::fs::read_to_string(root.join("a.json")).unwrap(),
        "{\"a\": 1}"
    );
    assert!(root.join("a.json.bak").exists());
    std::fs::write(root.join("b.json"), "[1,").unwrap();
    let (v, c) = json_brace_repair(&fx, &root, "b.json", "json error", true);
    assert_eq!((v["rolled"].as_bool(), c), (Some(true), 1));
    assert_eq!(std::fs::read_to_string(root.join("b.json")).unwrap(), "[1,");
    assert_eq!(
        json_brace_repair(&fx, &root, "a.json", "KeyError", true).0["refuse"],
        "no-fix"
    );
    assert_eq!(
        json_brace_repair(&fx, &root, "a.json", "json gateway", true).0["refuse"],
        "protected"
    );
    assert_eq!(
        json_brace_repair(&fx, &root, "../../x", "json", true).0["refuse"],
        "path"
    );
    assert_eq!(
        json_brace_repair(&fx, &d, "x", "json", true).0["refuse"],
        "root"
    );
}

#[test]
fn cycle_writes_files_and_logs_actions() {
    let d = out("cycle");
    let home = d.join("hermes");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("gateway.pid"), "{\"pid\": 999999}").unwrap();
    let t = format!(
        "[healer]\nstate_dir=\"{0}/heal\"\nhermes_home=\"{1}\"\nrestart_backend=\"systemd\"\n[[probe]]\nname=\"hermes\"\nport=1\npath=\"/\"\n[[probe]]\nname=\"one\"\nport=2\npath=\"/\"\n[[restart]]\nprobe=\"one\"\nunit=\"galion-one\"\nafter_cycles=2\n",
        d.display(), home.display()
    );
    let f = config::parse(&t).unwrap();
    let rec = Rec(RefCell::new(vec![]));
    let proc_root = d.join("proc");
    std::fs::create_dir_all(&proc_root).unwrap();
    let mut down = |_: &config::Probe| (false, "connect: refused".to_string());
    for i in 0..3 {
        let mut deps = Deps {
            runner: &rec,
            proc_root: &proc_root,
            prober: &mut down,
            now_unix: 1000.0 + i as f64 * 30.0,
            stamp: format!("2026-10-18T21:0{i}:00+02:00"),
        };
        let p = run_cycle(&f, &mut deps).unwrap();
        assert_eq!(p["mode"], "dry-run");
    }
    // dry-run: nothing executed, nothing removed, but logged
    assert!(rec.0.borrow().is_empty());
    assert!(home.join("gateway.pid").exists());
    let log = std::fs::read_to_string(d.join("heal/actions.jsonl")).unwrap();
    assert!(
        log.contains("\"action\":\"clear-stale-gateway-lock\"")
            && log.contains("\"mode\":\"dry-run\"")
    );
    assert_eq!(
        log.matches("restart-unit").count(),
        1,
        "rate limited to one per gap"
    );
    let nerve = std::fs::read_to_string(d.join("heal/nerve.jsonl")).unwrap();
    assert_eq!(
        nerve.lines().filter(|l| l.starts_with("heal sent")).count(),
        2
    );
    assert_eq!(
        nerve
            .lines()
            .filter(|l| l.starts_with("heal returned"))
            .count(),
        2
    );
    assert!(d.join("heal/inbox").read_dir().unwrap().count() == 2);
    let pulse: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.join("heal/pulse.json")).unwrap()).unwrap();
    assert_eq!(pulse["down"].as_array().unwrap().len(), 2);
}
