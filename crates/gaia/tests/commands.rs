// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The plan, dry-run, ownership and view commands, on fixtures.

use gaia_core::Json;

fn num(j: &Json, path: &[&str]) -> i64 {
    let mut cur = j;
    for p in path {
        cur = cur.get(p).unwrap_or_else(|| panic!("missing {p}"));
    }
    cur.as_i64().expect("number")
}

#[test]
fn dry_run_plans_everything_and_acts_on_nothing() {
    let j = gaia::dry_run(None).expect("dry-run");
    assert_eq!(num(&j, &["totals", "systems"]), 52);
    assert_eq!(num(&j, &["totals", "destructive_refused"]), 5);
    assert_eq!(num(&j, &["totals", "destructive_acted"]), 0);
    let h = gaia::dry_run(Some("hades")).expect("hades");
    assert_eq!(num(&h, &["totals", "systems"]), 5);
    assert!(
        gaia::dry_run(Some("ares")).is_err(),
        "ARES is not a function"
    );
}

#[test]
fn plan_is_dry_run_for_hades() {
    let j = gaia::plan("rollback", "").expect("plan");
    let out = j.compact();
    assert!(out.contains("\"function\":\"HADES\""));
    assert!(out.contains("\"outcome\":\"dry-run\""), "{out}");
    assert!(gaia::plan("no-such-system", "").is_err());
}

#[test]
fn ownership_counts() {
    let j = gaia::ownership_json();
    assert_eq!(num(&j, &["crates_total"]), 74); // main-staging: +tunnel-lint, +console-static, +console-ui, +galion-tar; cortex-staging-fold: +gaia-cortex, +nerve-cortex-contract, +xtask; slot 325: +galion-feature-pin, +galion-jsonout; galion-stt: +galion-stt
    assert_eq!(num(&j, &["system_crates"]), 62); // main-staging: +tunnel-lint, +console-static, +console-ui, +galion-tar; cortex-staging-fold: +gaia-cortex, +nerve-cortex-contract, +xtask; slot 325: +galion-feature-pin, +galion-jsonout; galion-stt: +galion-stt
    assert_eq!(num(&j, &["systems_total"]), 52);
}

#[test]
fn view_of_the_vps_fixture() {
    let units = include_str!("fixtures/vps-units-20261020.txt");
    let ss = include_str!("fixtures/vps-ss-20261020.txt");
    let j = gaia::view::view(units, Some(ss));
    assert_eq!(num(&j, &["summary", "units_seen"]), 16);
    assert_eq!(num(&j, &["summary", "units_running"]), 12);
    assert_eq!(num(&j, &["summary", "units_unmapped"]), 0);
    assert_eq!(num(&j, &["summary", "listeners"]), 22);
    let out = j.compact();
    for live in [
        "galion-one",
        "memory-gateway",
        "nerve",
        "tuner",
        "tunnels",
        "console",
    ] {
        let needle = format!("\"system\":\"{live}\",\"observed\":\"live\"");
        assert!(out.contains(&needle), "{live} should be live");
    }
    // The stray manual listeners from the review stay unmapped.
    assert!(
        out.contains("\"unmapped_listeners\":[8431,8432,8433,8652,8653]"),
        "{out}"
    );
}
