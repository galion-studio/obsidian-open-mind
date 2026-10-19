// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

use openmind_survival::{decide, Escalation, ProbeResult};

fn sample(name: &str, ok: bool, detail: &str, ts: &str) -> ProbeResult {
    ProbeResult {
        name: name.to_owned(),
        ok,
        detail: detail.to_owned(),
        ts: ts.to_owned(),
    }
}

#[test]
fn decide_table() {
    // name, ok, detail, level, token, text
    let rows: &[(&str, bool, &str, Escalation, &str, &str)] = &[
        (
            "hermes",
            true,
            "status 200",
            Escalation::Observe,
            "observe",
            "hermes ok: keep watching",
        ),
        (
            "unit:demo.service",
            true,
            "active",
            Escalation::Observe,
            "observe",
            "demo.service ok: keep watching",
        ),
        (
            "disk:/",
            true,
            "used 42.0% pass",
            Escalation::Observe,
            "observe",
            "disk ok: keep watching",
        ),
        (
            "fresh:/var/lib/beat",
            true,
            "age 1s max 60s",
            Escalation::Observe,
            "observe",
            "freshness ok: keep watching",
        ),
        (
            "mem",
            true,
            "used 50.0% pass",
            Escalation::Observe,
            "observe",
            "memory ok: keep watching",
        ),
        (
            "load",
            true,
            "load1/core 12.0% pass",
            Escalation::Observe,
            "observe",
            "load ok: keep watching",
        ),
        (
            "cloudflared",
            true,
            "pid 200",
            Escalation::Observe,
            "observe",
            "cloudflared ok: keep watching",
        ),
        (
            "mem",
            false,
            "used 90.0% warn",
            Escalation::Warn,
            "warn",
            "memory high: operator may stop an idle heavy service",
        ),
        (
            "mem",
            false,
            "used 97.0% crit",
            Escalation::Warn,
            "warn",
            "memory critical: operator may stop an idle heavy service",
        ),
        (
            "mem",
            false,
            "unparseable meminfo",
            Escalation::Warn,
            "warn",
            "memory unreadable: operator may check the source",
        ),
        (
            "load",
            false,
            "load1/core 90.0% warn (load1 3.60, cores 4)",
            Escalation::Warn,
            "warn",
            "load high: operator may pause a batch job",
        ),
        (
            "load",
            false,
            "load1/core 120.0% crit (load1 4.80, cores 4)",
            Escalation::Warn,
            "warn",
            "load critical: operator may pause a batch job",
        ),
        (
            "load",
            false,
            "unparseable loadavg",
            Escalation::Warn,
            "warn",
            "load unreadable: operator may check the source",
        ),
        (
            "disk:/",
            false,
            "used 90.0% warn",
            Escalation::Warn,
            "warn",
            "disk high: operator may free space or rotate logs",
        ),
        (
            "disk:/var",
            false,
            "used 96.0% crit",
            Escalation::Warn,
            "warn",
            "disk critical: operator may free space or rotate logs",
        ),
        (
            "disk:/",
            false,
            "unparseable df output",
            Escalation::Warn,
            "warn",
            "disk unreadable: operator may check the source",
        ),
        (
            "fresh:/tmp/x",
            false,
            "age 100s max 60s",
            Escalation::Warn,
            "warn",
            "freshness failed: operator may check the writer service",
        ),
        (
            "mystery",
            false,
            "unparseable sample",
            Escalation::Warn,
            "warn",
            "probe unreadable: operator may check the source",
        ),
        (
            "unit:demo.service",
            false,
            "inactive",
            Escalation::SuggestRestart,
            "suggest-restart",
            "demo.service not active: operator may restart demo.service",
        ),
        (
            "unit:bad name",
            false,
            "failed",
            Escalation::SuggestRestart,
            "suggest-restart",
            "service not active: operator may restart service",
        ),
        (
            "unit:",
            false,
            "inactive",
            Escalation::SuggestRestart,
            "suggest-restart",
            "service not active: operator may restart service",
        ),
        (
            "cloudflared",
            false,
            "not running",
            Escalation::SuggestRestart,
            "suggest-restart",
            "cloudflared absent: operator may restart the tunnel",
        ),
        (
            "whisper",
            false,
            "status 503",
            Escalation::SuggestRestart,
            "suggest-restart",
            "whisper down: operator may restart whisper",
        ),
        (
            "console",
            false,
            "connect: refused",
            Escalation::SuggestRestart,
            "suggest-restart",
            "console down: operator may restart console",
        ),
        (
            "odd/name",
            false,
            "status 0",
            Escalation::SuggestRestart,
            "suggest-restart",
            "service down: operator may restart service",
        ),
    ];

    let results: Vec<ProbeResult> = rows
        .iter()
        .map(|(name, ok, detail, _, _, _)| sample(name, *ok, detail, "2026-10-19T16:00:00+02:00"))
        .collect();
    let got = decide(&results);
    assert_eq!(got.len(), rows.len());
    for (g, (name, _, _, level, token, text)) in got.iter().zip(rows.iter().copied()) {
        assert_eq!(g.name, name);
        assert_eq!(g.level, level);
        assert_eq!(g.level.as_str(), token);
        assert_eq!(g.text, text);
        assert!(!g.text.contains('/'), "{}", g.text);
        assert!(!g.text.contains("systemctl"), "{}", g.text);
    }
}

#[test]
fn decide_empty() {
    assert!(decide(&[]).is_empty());
}

#[test]
fn decide_ignores_timestamp_and_keeps_order() {
    let early = sample("whisper", false, "status 503", "2026-03-29T01:00:00+01:00");
    let late = sample("whisper", false, "status 503", "2026-10-19T16:00:00+02:00");
    assert_eq!(decide(std::slice::from_ref(&early)), decide(&[late]));

    let mixed = [
        sample("hermes", true, "status 200", "2026-10-19T16:00:00+02:00"),
        sample("voice", false, "status 503", "2026-10-19T16:00:01+02:00"),
        sample("mem", false, "used 90.0% warn", "2026-10-19T16:00:02+02:00"),
    ];
    let got = decide(&mixed);
    assert_eq!(got[0].level, Escalation::Observe);
    assert_eq!(got[1].level, Escalation::SuggestRestart);
    assert_eq!(got[1].name, "voice");
    assert_eq!(got[2].level, Escalation::Warn);
}

#[test]
fn decide_module_does_not_touch_the_machine() {
    let src = include_str!("../src/decide.rs");
    for banned in [
        "systemctl",
        "Command::",
        "TcpStream",
        "std::process",
        "std::net",
        "std::fs",
    ] {
        assert!(!src.contains(banned), "{banned} in decide.rs");
    }
}
