// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Slot 380 (edge-closed-ports-map): replay of the 2026-10-28 10:06-10:10 +01:00
//! read-only VPS capture (secrets-free config copy + `ss -ltnp`). Pins the
//! hostname -> ingress rule -> origin port -> owner map and its retired/broken
//! classification in `docs/ops/edge-closed-ports-map.md`, so the doc cannot
//! silently drift from the config it describes.

use tunnel_lint::*;

const CFG: &str = include_str!("fixtures/vps-config-20261028.yml");
const SS: &str = include_str!("fixtures/vps-ss-20261028.txt");
const OLD_CFG: &str = include_str!("fixtures/vps-config-20261020.yml");
const OLD_SS: &str = include_str!("fixtures/vps-ss-20261020.txt");

/// Classification used in the doc (one entry per closed rule).
/// N = no public DNS record (rule unreachable), L = legacy Node/Python site
/// (source tree only, last commit 2026-01-20, no build output, no unit),
/// O = orphan (no source tree at all), B = broken (has a real owner, owner down).
const MAP: &[(usize, &str, u16, char)] = &[
    (5, "jupyter.adriaticpadel.com", 8889, 'N'),
    (6, "adriaticpadel.com", 3010, 'L'),
    (7, "www.adriaticpadel.com", 3010, 'L'),
    (10, "studio.galion.app", 4001, 'L'),
    (11, "developer.galion.app", 3333, 'L'),
    (12, "api.galion.app", 8000, 'L'),
    (13, "docs.galion.app", 4042, 'L'),
    (14, "reference.galion.app", 3080, 'L'),
    (15, "voice.galion.app", 4003, 'L'),
    (16, "downloader.galion.app", 4444, 'O'),
    (17, "knowledge.galion.app", 3015, 'L'),
    (18, "galion.studio", 4001, 'L'),
    (19, "www.galion.studio", 4001, 'L'),
    (20, "ws3d.app", 4020, 'L'),
    (21, "www.ws3d.app", 4020, 'L'),
    (22, "workspace3d.app", 4020, 'L'),
    (23, "www.workspace3d.app", 4020, 'L'),
    (24, "slaviknomad.com", 4060, 'L'),
    (25, "www.slaviknomad.com", 4060, 'L'),
    (26, "villapenava.com", 4050, 'N'),
    (27, "www.villapenava.com", 4050, 'N'),
    (28, "machine.galion.app", 3004, 'O'),
    (29, "torchbearer.galion.app", 3005, 'O'),
    (30, "tactical-belts.galion.app", 4004, 'L'),
    (31, "www.tactical-belts.galion.app", 4004, 'N'),
    (32, "maivirtue.com", 3020, 'L'),
    (33, "www.maivirtue.com", 3020, 'L'),
    (34, "merlinelement.com", 3011, 'L'),
    (35, "www.merlinelement.com", 3011, 'L'),
    (36, "tulu.galion.app", 5530, 'L'),
    (38, "forge.galion.studio", 18800, 'B'),
];

const CLOSED_PORTS: [u16; 20] = [
    3004, 3005, 3010, 3011, 3015, 3020, 3080, 3333, 4001, 4003, 4004, 4020, 4042, 4050, 4060, 4444,
    5530, 8000, 8889, 18800,
];

fn report() -> Report {
    lint(&parse(CFG).unwrap(), &SsSnapshot::parse(SS))
}

fn port_of(r: &RuleReport) -> u16 {
    match &r.origin {
        Origin::Local { port, .. } => *port,
        o => panic!("rule {} not local: {o:?}", r.rule.index),
    }
}

#[test]
fn map_matches_capture_exactly() {
    let rep = report();
    assert_eq!(rep.rules.len(), 39);
    assert_eq!(rep.closed_ports(), CLOSED_PORTS.to_vec());
    let closed: Vec<(usize, String, u16)> = rep
        .closed()
        .map(|r| (r.rule.index, r.rule.hostname.clone().unwrap(), port_of(r)))
        .collect();
    let want: Vec<(usize, String, u16)> = MAP
        .iter()
        .map(|(i, h, p, _)| (*i, h.to_string(), *p))
        .collect();
    assert_eq!(closed, want, "doc map drifted from the capture");
    let classes = |c: char| MAP.iter().filter(|m| m.3 == c).count();
    assert_eq!(
        (classes('N'), classes('L'), classes('O'), classes('B')),
        (4, 23, 3, 1)
    );
}

#[test]
fn open_rules_and_owners() {
    let rep = report();
    let open: Vec<(usize, String, u16, String)> = rep
        .rules
        .iter()
        .filter_map(|r| match &r.status {
            Status::Open(o) => Some((
                r.rule.index,
                r.rule.hostname.clone().unwrap(),
                port_of(r),
                o.clone(),
            )),
            _ => None,
        })
        .collect();
    let want = [
        (1, "one.galion.app", 8671, "tanga-serve[311]"),
        (2, "one.galion.app", 8091, "galion-one[304]"),
        (3, "console.galion.app", 8080, "galion-console-[240630]"),
        (4, "obsidian.galion.app", 8080, "galion-console-[240630]"),
        (8, "galion.app", 8091, "galion-one[304]"),
        (9, "www.galion.app", 8091, "galion-one[304]"),
        (37, "tuner.galion.app", 8650, "tuner[313]"),
    ];
    assert_eq!(open.len(), want.len());
    for (w, o) in want.iter().zip(&open) {
        assert_eq!((w.0, w.1, w.2, w.3), (o.0, o.1.as_str(), o.2, o.3.as_str()));
    }
}

#[test]
fn every_nerve_reported_port_is_in_the_static_map() {
    // Port sets the live nerve incident inc-06ffb32f reported in its rolling
    // 30-min window (2026-10-28 08:45 / 09:15 / 09:45 / 10:00 +01:00). The nerve
    // only names ports that got refused traffic in the window; the static map
    // is the superset.
    let windows: [&[u16]; 4] = [
        &[3010, 3011, 3020, 4001, 4020, 4060],
        &[3005, 3010, 3011, 3020, 4001, 4020, 4060, 18800],
        &[
            3004, 3010, 3011, 3020, 3333, 4001, 4020, 4060, 4444, 5530, 8000,
        ],
        &[3004, 3010, 3011, 3333, 4001, 4020, 4060, 5530, 8000],
    ];
    for w in windows {
        for p in w {
            assert!(CLOSED_PORTS.contains(p), "nerve port {p} missing");
        }
    }
}

#[test]
fn drift_since_20261020_audit() {
    let old = lint(&parse(OLD_CFG).unwrap(), &SsSnapshot::parse(OLD_SS));
    let new = report();
    let gone: Vec<u16> = old
        .closed_ports()
        .into_iter()
        .filter(|p| !new.closed_ports().contains(p))
        .collect();
    let added: Vec<u16> = new
        .closed_ports()
        .into_iter()
        .filter(|p| !old.closed_ports().contains(p))
        .collect();
    // 4000: galion.app repointed to galion-one 8091; 7681: code.galion.app rule removed.
    assert_eq!(gone, vec![4000, 7681]);
    // 18800: forge.galion.studio added 2026-10-21 17:08 +02:00, box link down since 2026-10-22.
    assert_eq!(added, vec![18800]);
    assert_eq!(old.closed().count(), 33);
    assert_eq!(new.closed().count(), 31);
}

#[test]
fn staged_cleanup_keeps_only_live_routes_and_catch_all() {
    let rep = report();
    let staged = stage(CFG, &rep);
    let again = parse(&staged).unwrap();
    assert_eq!(again.len(), 8, "7 live routes + catch-all");
    let rep2 = lint(&again, &SsSnapshot::parse(SS));
    assert!(!rep2.has_errors(), "{:?}", rep2.findings);
    assert_eq!(again.last().unwrap().service, "http_status:404");
    for (_, h, p, _) in MAP {
        assert!(
            staged.contains(&format!("# tunnel-lint: closed port {p}")),
            "{h}"
        );
    }
    // Committed doc copy is exactly what the tool stages.
    let doc = include_str!("../../../docs/ops/edge-closed-ports-map.config.staged.yml");
    let body = doc
        .split_once("# --- staged by tunnel-lint below ---\n")
        .expect("marker")
        .1;
    assert_eq!(body, staged);
}
