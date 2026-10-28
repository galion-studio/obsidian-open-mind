// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 333 (nerve-merge-6, D-316-1 option A): the staged nerve component
//! table `deploy/staged/nerve/components-live.json` is the live VPS table
//! `/etc/galion-nerve/components-live.json` (read-only copy 2026-10-28 01:17
//! +01:00, fixture `tests/fixtures/own-services/components-live-vps-20261028-0117.json`)
//! plus exactly two changes:
//!
//! 1. a `console-rust-primary` component for `console-rust-primary.service`
//!    on 127.0.0.1:18080 (HEPHAESTUS, group `console`, not critical, never
//!    restarted), so the live nerve stops flagging :18080 as unmanaged;
//! 2. `deep` routes on `console` and `console-rust-primary` (N-307-1): the
//!    live nerve reads this file, not the built-in table, so without them the
//!    deep console probe of slot 315 would stay off after a deploy.
//!
//! Hermetic (no network, no binary): parses both files with the same parser
//! the nerve uses at start-up.

use openmind_nerve::config::{parse_specs, Spec, CONSOLE_DEEP};
use std::path::Path;

fn read(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel))
        .unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn staged() -> Vec<Spec> {
    parse_specs(&read("../../deploy/staged/nerve/components-live.json")).expect("staged parses")
}

fn live() -> Vec<Spec> {
    parse_specs(&read(
        "tests/fixtures/own-services/components-live-vps-20261028-0117.json",
    ))
    .expect("live parses")
}

/// Everything the nerve uses from a spec, as one comparable string.
fn key(s: &Spec, with_deep: bool) -> String {
    format!(
        "{}|{:?}|{:?}|{}|{:?}|{:?}|{}|{}|{}|{}|{}|{}|{}|{:?}",
        s.name,
        s.unit,
        s.port,
        s.path,
        s.metrics_port,
        s.kind,
        s.gaia,
        s.gaia_system,
        s.restarts_per_hour,
        s.critical,
        s.expected,
        s.note,
        s.group,
        if with_deep { s.deep } else { &[] },
    )
}

#[test]
fn staged_is_live_plus_console_rust_primary() {
    let (st, lv) = (staged(), live());
    let names = |v: &[Spec]| v.iter().map(|s| s.name).collect::<Vec<_>>();
    let mut expect = names(&lv);
    let at = expect.iter().position(|n| *n == "console").unwrap() + 1;
    expect.insert(at, "console-rust-primary");
    assert_eq!(names(&st), expect);
    for l in &lv {
        let s = st.iter().find(|s| s.name == l.name).unwrap();
        assert_eq!(key(s, false), key(l, false), "{} changed", l.name);
        let want: &[&str] = if l.name == "console" {
            CONSOLE_DEEP
        } else {
            &[]
        };
        assert_eq!(s.deep, want, "{} deep", l.name);
        assert!(l.deep.is_empty(), "live {} has deep already", l.name);
    }
}

#[test]
fn console_rust_primary_spec() {
    let st = staged();
    let c = st
        .iter()
        .find(|s| s.name == "console-rust-primary")
        .unwrap();
    assert_eq!(c.unit, Some("console-rust-primary.service"));
    assert_eq!((c.port, c.path, c.metrics_port), (Some(18080), "/", None));
    assert_eq!(
        (c.gaia, c.gaia_system, c.group),
        ("HEPHAESTUS", "console", "console")
    );
    assert_eq!(
        c.restarts_per_hour, 0,
        "alert-only: the nerve never restarts it"
    );
    assert!(
        !c.critical,
        "a down primary must only degrade, never page as hard down"
    );
    assert!(c.expected);
    assert_eq!(c.deep, CONSOLE_DEEP);
    // No other component claims 18080 and every port stays unique.
    let mut ports: Vec<u16> = st.iter().filter_map(|s| s.port).collect();
    let n = ports.len();
    ports.sort();
    ports.dedup();
    assert_eq!(ports.len(), n, "duplicate component port");
    // Every staged component is never restarted, like the live table.
    assert!(st.iter().all(|s| s.restarts_per_hour == 0));
}
