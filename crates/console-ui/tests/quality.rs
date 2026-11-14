// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Snapshot and accessibility checks for the main admin and console screens.
//!
//! `GALION_SNAP_UPDATE=1` rewrites `tests/snaps/*.html`. Otherwise the
//! rendered HTML must match the committed snapshot.

use console_ui::a11y::{self, focusables, unlabeled_fields};
use console_ui::json::{self, Value};
use console_ui::layout::{self, Frame, Origin};
use console_ui::pages::{self, hermes};
use console_ui::sound::{self, Ratio, TONIC_HZ};
use std::path::PathBuf;

fn frame<'a>(path: &'a str) -> Frame<'a> {
    Frame {
        title: "Galion Console",
        css_href: "/_ui/console.test.css",
        extra_css_href: None,
        light: Some(false),
        path,
        silence: false,
    }
}

fn sample() -> (Vec<pages::Service>, Value) {
    let services = json::parse(br#"{"items":[{"id":"a","name":"Nerve","kind":"api","weight":"light","summary":"loop","hosts":["h"],"port":1,"defaultOn":true},{"id":"whisper","name":"Whisper","kind":"held","weight":"heavy","summary":"voice","hosts":[],"port":2,"defaultOn":false}]}"#).unwrap();
    let domains =
        json::parse(br#"{"items":[{"host":"console.galion.app","role":"desk","serviceId":"a"}]}"#)
            .unwrap();
    (pages::services_of(&services), domains)
}

fn screens() -> Vec<(&'static str, String)> {
    let (svcs, domains) = sample();
    let health =
        json::parse(br#"{"status":"healthy","version":"1","target":{"pod":"ip50"}}"#).unwrap();
    let f = frame("/console");
    vec![
        ("shell", layout::console(&f, Origin::Up, "<p>Desk</p>")),
        ("pair", pages::gate::pair("/console", None, false)),
        (
            "pair-error",
            pages::gate::pair("/console", Some("That code was refused."), false),
        ),
        ("unavailable", pages::gate::unavailable(false)),
        (
            "status",
            layout::document(
                &frame("/status"),
                "",
                &pages::status::body(Some(true), &svcs, &domains, &[]),
            ),
        ),
        (
            "services",
            layout::console(
                &frame("/services"),
                Origin::Up,
                &pages::services::body(
                    &svcs,
                    &[pages::Pulse {
                        id: "whisper".into(),
                        ok: false,
                    }],
                    true,
                ),
            ),
        ),
        (
            "agents",
            layout::console(
                &frame("/agents"),
                Origin::Up,
                &pages::agents::body(Some(&health), ""),
            ),
        ),
        (
            "hermes",
            layout::console(
                &frame("/hermes"),
                Origin::Down,
                &hermes::body(&hermes::View::default(), "/hermes/attach", &[], None),
            ),
        ),
        (
            "overview",
            pages::overview::body(console_ui::PORTED.len() + 1),
        ),
    ]
}

fn snap_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snaps")
}

#[test]
fn main_screens_match_snapshots_and_stay_labelled() {
    let update = std::env::var("GALION_SNAP_UPDATE").ok().as_deref() == Some("1");
    let dir = snap_dir();
    if update {
        std::fs::create_dir_all(&dir).unwrap();
    }
    for (name, html) in screens() {
        assert!(!html.contains("<script"), "{name} picked up a script");
        assert!(
            !a11y::positive_tabindex(&html),
            "{name} has a positive tabindex"
        );
        let missing = unlabeled_fields(&html);
        assert!(missing.is_empty(), "{name} unlabeled fields: {missing:?}");
        let file = dir.join(format!("{name}.html"));
        if update {
            std::fs::write(&file, &html).unwrap();
            continue;
        }
        let prev = std::fs::read_to_string(&file).unwrap_or_else(|_| {
            panic!(
                "missing snapshot {} (set GALION_SNAP_UPDATE=1)",
                file.display()
            )
        });
        assert_eq!(prev, html, "snapshot drifted: {name}");
    }
}

#[test]
fn shell_focus_order_starts_at_the_skip_link() {
    let html = layout::console(
        &frame("/services"),
        Origin::Up,
        "<a href=\"/status\">Status</a>",
    );
    let order = focusables(&html);
    assert_eq!(order.first().map(String::as_str), Some("a #main"));
    assert!(order.iter().any(|s| s.contains("Turn on silence")));
    assert!(order.iter().any(|s| s.contains("Menu")));
    let skip = html.find("href=\"#main\"").unwrap();
    let silence = html.find("Turn on silence").unwrap();
    let main = html.find("id=\"main\"").unwrap();
    let dock = html.find("<nav class=\"dock\"").unwrap();
    assert!(skip < silence && silence < main && main < dock);
    assert!(html[main..dock].contains("/status"));
    let pair = pages::gate::pair("/console", Some("no"), false);
    assert!(pair.contains("aria-describedby=\"pair-err\""));
    assert!(pair.contains("/_ui/sound/error.wav"));
    assert!(!pair.contains("bell-"));
    let ask = pages::gate::pair("/console", None, false);
    assert!(ask.contains("/_ui/sound/approval.wav"));
    assert!(pages::gate::pair("/console", None, true).contains("Turn sound on"));
    assert!(!pages::gate::pair("/console", None, true).contains("<audio"));
}

#[test]
fn typescript_sound_set_matches_the_rust_ratios() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../stack/research-agent/console-ide/src/lib/galion-sound.ts");
    let ts = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(ts.contains("export const TONIC_HZ = 432"));
    for (name, ratio) in [
        ("unison: [1, 1]", Ratio::new(1, 1)),
        ("octaveBelow: [1, 2]", Ratio::new(1, 2)),
        ("minorSecond: [16, 15]", Ratio::new(16, 15)),
        ("majorThird: [5, 4]", Ratio::new(5, 4)),
        ("fourth: [4, 3]", Ratio::new(4, 3)),
        ("fifth: [3, 2]", Ratio::new(3, 2)),
        ("octave: [2, 1]", Ratio::new(2, 1)),
    ] {
        assert!(ts.contains(name), "missing {name}");
        assert!((ratio.hz() - TONIC_HZ * f64::from(ratio.num) / f64::from(ratio.den)).abs() < 1e-9);
    }
    assert!(ts.contains("[5, 2]"));
    assert!(ts.contains("[3, 1]"));
    assert!(ts.contains("[4, 1]"));
    assert!(ts.contains("approval: [\"fourth\", \"fifth\"]"));
    assert!(ts.contains("done: [\"fifth\", \"octave\"]"));
    assert!(ts.contains("error: [\"minorSecond\", \"minorSecond\"]"));
    assert!(ts.contains("minimise: \"octave\""));
    let cue_at = ts.find("export function cue").unwrap();
    let bell_at = ts.find("export function bell").unwrap();
    assert!(cue_at < bell_at);
    assert!(!ts[cue_at..bell_at].contains("bell("));
    assert!(sound::bell(sound::Window::Minimise).is_bell());
    assert!(!sound::admin(sound::Admin::TaskDone).is_bell());
}
