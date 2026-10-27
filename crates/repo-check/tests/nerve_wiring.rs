// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Unit tests of the nerve wiring-depth check (wave-4 slot 327), in memory.

use repo_check::nerve::*;
use repo_check::Level;
use std::collections::BTreeMap;

const ROOT: &str = "[workspace]\nmembers = [\"crates/*\"]\n";
const DEP: &str = "[dependencies]\nnerve-client = { path = \"../nerve-client\" }\n";

fn pkg(name: &str, deps: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n{deps}")
}

fn run(files: &[(&str, &str)]) -> (Vec<repo_check::Finding>, Vec<WiringRow>) {
    let mut m: BTreeMap<String, String> = BTreeMap::new();
    m.insert("Cargo.toml".into(), ROOT.into());
    for (p, t) in files {
        m.insert(p.to_string(), t.to_string());
    }
    let names: Vec<String> = m.keys().cloned().collect();
    let read = |p: &str| m.get(p).cloned();
    check_wiring(m.get("Cargo.toml").map(String::as_str), &names, &read)
}

const STARTED: &str = "nerve_client::event(\n    \"svc\",\n    \"started\",\n    &msg,\n);\n";

#[test]
fn non_test_cuts_at_cfg_test() {
    let t = "fn a() {}\n#[cfg(test)]\nmod t { fn b() { TcpListener::bind(\"x\"); } }\n";
    assert_eq!(non_test(t), "fn a() {}\n");
    assert_eq!(non_test("fn a() {}"), "fn a() {}");
}

#[test]
fn facts_find_multiline_started_only_in_event_call() {
    assert!(wire_facts(STARTED).started);
    // "started" outside an event call does not count.
    assert!(!wire_facts("let s = \"started\"; nerve_client::event(\"x\", \"up\", s);").started);
    // a later event call can carry it
    let two =
        "nerve_client::event(\"x\", \"up\", \"\");\nnerve_client::event(\"x\", \"started\", \"\");";
    assert!(wire_facts(two).started);
}

#[test]
fn missing_rules() {
    let f = wire_facts("TcpListener::bind(a); nerve_client::start_heartbeat(\"x\", 60);");
    assert_eq!(wire_missing(&f), vec![Wire::Started, Wire::ObserveRequest]);
    let f = wire_facts(&format!("{STARTED} nerve_client::flush(d);"));
    assert!(wire_missing(&f).is_empty());
    let f = wire_facts(STARTED);
    assert_eq!(wire_missing(&f), vec![Wire::HeartbeatOrFlush]);
}

#[test]
fn exemption_needs_known_rule_and_reason() {
    let f = wire_facts(
        "// nerve-wiring: exempt observe_request mock server for benches\n// nerve-wiring: exempt started\n// nerve-wiring: exempt bogus why\n",
    );
    assert_eq!(f.exempt.len(), 1);
    assert_eq!(f.exempt[0].0, Wire::ObserveRequest);
    assert_eq!(f.exempt[0].1, "mock server for benches");
}

#[test]
fn check_wiring_scopes_to_direct_bin_packages() {
    let (fs, rows) = run(&[
        // direct dep + bin, serves HTTP, no observe -> one gap
        ("crates/web/Cargo.toml", &pkg("web", DEP)),
        (
            "crates/web/src/main.rs",
            &format!("fn main() {{ {STARTED} nerve_client::start_heartbeat(\"web\", 60); let l = TcpListener::bind(a); }}\n#[cfg(test)]\nmod t {{}}\n"),
        ),
        // one-shot that flushes: wired
        ("crates/cli/Cargo.toml", &pkg("cli", DEP)),
        (
            "crates/cli/src/main.rs",
            &format!("fn main() {{ {STARTED} nerve_client::flush(d); }}"),
        ),
        // listener only in tests: not "http"
        ("crates/quiet/Cargo.toml", &pkg("quiet", DEP)),
        (
            "crates/quiet/src/main.rs",
            &format!("fn main() {{ {STARTED} nerve_client::flush(d); }}\n#[cfg(test)]\nmod t {{ fn f() {{ TcpListener::bind(a); }} }}\n"),
        ),
        // library only (no bin): skipped
        ("crates/libonly/Cargo.toml", &pkg("libonly", DEP)),
        ("crates/libonly/src/lib.rs", "pub fn f() {}"),
        // no nerve dep: skipped (the coverage gate handles it)
        ("crates/plain/Cargo.toml", &pkg("plain", "")),
        ("crates/plain/src/main.rs", "fn main() {}"),
        // optional dep only: skipped
        (
            "crates/opt/Cargo.toml",
            &pkg("opt", "[dependencies]\nnerve-client = { path = \"../nerve-client\", optional = true }\n"),
        ),
        ("crates/opt/src/main.rs", "fn main() {}"),
        // exempted miss
        ("crates/mock/Cargo.toml", &pkg("mock", DEP)),
        (
            "crates/mock/src/main.rs",
            &format!("// nerve-wiring: exempt observe_request fixture server only\nfn main() {{ {STARTED} nerve_client::flush(d); TcpListener::bind(a); }}"),
        ),
    ]);
    let names: Vec<&str> = rows.iter().map(|r| r.package.as_str()).collect();
    assert_eq!(names, vec!["cli", "mock", "quiet", "web"]);
    let by = |n: &str| rows.iter().find(|r| r.package == n).unwrap();
    assert_eq!(by("web").missing, vec![Wire::ObserveRequest]);
    assert!(by("web").serves_http);
    assert!(by("cli").missing.is_empty());
    assert!(!by("quiet").serves_http);
    assert!(by("mock").missing.is_empty());
    assert_eq!(by("mock").exempt.len(), 1);
    assert_eq!(fs.len(), 1);
    assert_eq!(fs[0].level, Level::Warn);
    assert!(fs[0].msg.contains("web: observe_request"), "{}", fs[0].msg);
    let table = render_wiring(&rows);
    assert!(
        table.contains("web                    http  GAP observe_request"),
        "{table}"
    );
    assert!(
        table.ends_with("nerve wiring: 4 direct packages, 1 with gaps, 1 wiring gap(s)\n"),
        "{table}"
    );
}

#[test]
fn no_workspace_no_rows() {
    let (fs, rows) = check_wiring(None, &[], &|_| None);
    assert!(fs.is_empty() && rows.is_empty());
}
