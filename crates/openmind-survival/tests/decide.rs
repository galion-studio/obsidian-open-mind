// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

use openmind_survival::{decide, DegradationRule, Escalation, ProbeResult, DEGRADATION_RULES};

fn sample(name: &str, ok: bool, detail: &str, ts: &str) -> ProbeResult {
    ProbeResult {
        name: name.to_owned(),
        ok,
        detail: detail.to_owned(),
        ts: ts.to_owned(),
    }
}

fn assert_suggestion_only(text: &str) {
    let lower = text.to_ascii_lowercase();
    for banned in ["systemctl", "restart", "kill"] {
        assert!(!lower.contains(banned), "{banned} in {text}");
    }
}

#[test]
fn decide_table() {
    // name, ok, detail, level, token, text
    // Each row is decided alone so the degradation clause has no siblings.
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
            "demo.service not active: operator may bring demo.service back; still works: none; paused: demo.service",
        ),
        (
            "unit:bad name",
            false,
            "failed",
            Escalation::SuggestRestart,
            "suggest-restart",
            "service not active: operator may bring service back; still works: none; paused: service",
        ),
        (
            "unit:",
            false,
            "inactive",
            Escalation::SuggestRestart,
            "suggest-restart",
            "service not active: operator may bring service back; still works: none; paused: service",
        ),
        (
            "cloudflared",
            false,
            "not running",
            Escalation::SuggestRestart,
            "suggest-restart",
            "cloudflared absent: operator may bring the tunnel back; still works: none; paused: cloudflared; cpu-light fallback",
        ),
        (
            "whisper",
            false,
            "status 503",
            Escalation::SuggestRestart,
            "suggest-restart",
            "whisper down: operator may bring whisper back; still works: none; paused: whisper; cpu-light fallback",
        ),
        (
            "console",
            false,
            "connect: refused",
            Escalation::SuggestRestart,
            "suggest-restart",
            "console down: operator may bring console back; still works: none; paused: console",
        ),
        (
            "odd/name",
            false,
            "status 0",
            Escalation::SuggestRestart,
            "suggest-restart",
            "service down: operator may bring service back; still works: none; paused: service",
        ),
    ];

    let results: Vec<ProbeResult> = rows
        .iter()
        .map(|(name, ok, detail, _, _, _)| sample(name, *ok, detail, "2026-10-19T16:00:00+02:00"))
        .collect();
    let batch = decide(&results);
    assert_eq!(batch.len(), rows.len());
    for (i, (name, _, _, level, token, text)) in rows.iter().copied().enumerate() {
        let one = decide(std::slice::from_ref(&results[i]));
        assert_eq!(one.len(), 1);
        let g = &one[0];
        assert_eq!(g.name, name);
        assert_eq!(g.level, level);
        assert_eq!(g.level.as_str(), token);
        assert_eq!(g.text, text);
        assert!(!g.text.contains('/'), "{}", g.text);
        assert_suggestion_only(&g.text);
        let grouped = &batch[i];
        assert_eq!(grouped.name, name);
        assert_eq!(grouped.level, level);
        assert_eq!(grouped.level.as_str(), token);
        assert!(!grouped.text.contains('/'), "{}", grouped.text);
        assert_suggestion_only(&grouped.text);
        if level == Escalation::SuggestRestart {
            assert!(grouped.text.contains("still works:"), "{}", grouped.text);
            assert!(grouped.text.contains("paused:"), "{}", grouped.text);
        } else {
            assert!(!grouped.text.contains("still works:"), "{}", grouped.text);
            assert!(!grouped.text.contains("paused:"), "{}", grouped.text);
        }
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
    assert_eq!(
        got[1].text,
        "voice down: operator may bring voice back; still works: hermes; paused: voice; cpu-light fallback"
    );
    assert_eq!(got[2].level, Escalation::Warn);
    assert!(!got[2].text.contains("paused:"));
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

#[test]
fn degradation_rule_table() {
    let losses: Vec<&str> = DEGRADATION_RULES.iter().map(|rule| rule.loss).collect();
    assert_eq!(losses, ["gpu", "lan", "network"]);
    for rule in DEGRADATION_RULES {
        assert!(!rule.paused.is_empty(), "{}", rule.loss);
        assert!(!rule.still_works.is_empty(), "{}", rule.loss);
        for name in rule.paused.iter().chain(rule.still_works.iter()) {
            assert!(!name.contains('/'), "{name}");
            assert!(!name.chars().any(char::is_whitespace), "{name}");
            assert_ne!(*name, "service");
        }
        for paused in rule.paused {
            assert!(
                !rule.still_works.contains(paused),
                "{} lists {paused} as both paused and still working",
                rule.loss
            );
        }
    }
    let gpu = rule("gpu");
    assert_eq!(gpu.paused, ["vision", "voice", "whisper"]);
    assert_eq!(gpu.still_works, ["hermes", "one", "console"]);
    assert_eq!(rule("lan").paused, ["lan"]);
    assert_eq!(rule("network").paused, ["cloudflared", "origin"]);
    assert!(rule("network").still_works.contains(&"hermes"));
    assert!(rule("lan").still_works.contains(&"whisper"));
}

fn rule(loss: &str) -> &'static DegradationRule {
    DEGRADATION_RULES
        .iter()
        .find(|rule| rule.loss == loss)
        .unwrap_or_else(|| panic!("missing rule {loss}"))
}

type ProbeCase = (&'static str, bool, &'static str, Escalation, &'static str);

#[test]

fn degrade_table() {
    // name, ok, detail, level, text. Each group is one decide() call.
    let groups: &[&[ProbeCase]] = &[
        &[
            (
                "gpu",
                false,
                "absent",
                Escalation::SuggestRestart,
                "gpu down: operator may bring gpu back; still works: hermes, one, console; paused: gpu, vision, voice, whisper; cpu-light fallback",
            ),
            (
                "hermes",
                true,
                "status 200",
                Escalation::Observe,
                "hermes ok: keep watching",
            ),
            (
                "one",
                true,
                "status 200",
                Escalation::Observe,
                "one ok: keep watching",
            ),
            (
                "console",
                true,
                "status 200",
                Escalation::Observe,
                "console ok: keep watching",
            ),
            (
                "vision",
                false,
                "status 503",
                Escalation::SuggestRestart,
                "vision down: operator may bring vision back; still works: hermes, one, console; paused: gpu, vision, voice, whisper; cpu-light fallback",
            ),
            (
                "voice",
                false,
                "connect: refused",
                Escalation::SuggestRestart,
                "voice down: operator may bring voice back; still works: hermes, one, console; paused: gpu, vision, voice, whisper; cpu-light fallback",
            ),
            (
                "whisper",
                false,
                "status 503",
                Escalation::SuggestRestart,
                "whisper down: operator may bring whisper back; still works: hermes, one, console; paused: gpu, vision, voice, whisper; cpu-light fallback",
            ),
            (
                "mem",
                true,
                "used 40.0% pass",
                Escalation::Observe,
                "memory ok: keep watching",
            ),
            (
                "disk:/",
                false,
                "used 96.0% crit",
                Escalation::Warn,
                "disk critical: operator may free space or rotate logs",
            ),
        ],
        &[
            (
                "lan",
                false,
                "absent",
                Escalation::SuggestRestart,
                "lan down: operator may bring lan back; still works: none; paused: lan; cpu-light fallback",
            ),
        ],
        &[
            (
                "hermes",
                true,
                "status 200",
                Escalation::Observe,
                "hermes ok: keep watching",
            ),
            (
                "cloudflared",
                false,
                "not running",
                Escalation::SuggestRestart,
                "cloudflared absent: operator may bring the tunnel back; still works: hermes; paused: cloudflared, origin; cpu-light fallback",
            ),
            (
                "origin",
                false,
                "status 0",
                Escalation::SuggestRestart,
                "origin down: operator may bring origin back; still works: hermes; paused: cloudflared, origin; cpu-light fallback",
            ),
            (
                "load",
                false,
                "load1/core 90.0% warn (load1 3.60, cores 4)",
                Escalation::Warn,
                "load high: operator may pause a batch job",
            ),
            (
                "fresh:/var/lib/beat",
                false,
                "age 100s max 60s",
                Escalation::Warn,
                "freshness failed: operator may check the writer service",
            ),
        ],
        &[
            (
                "network",
                false,
                "absent",
                Escalation::SuggestRestart,
                "network down: operator may bring network back; still works: one; paused: network; cpu-light fallback",
            ),
            (
                "one",
                true,
                "status 200",
                Escalation::Observe,
                "one ok: keep watching",
            ),
        ],
        &[
            (
                "console",
                true,
                "status 200",
                Escalation::Observe,
                "console ok: keep watching",
            ),
            (
                "unit:demo.service",
                false,
                "inactive",
                Escalation::SuggestRestart,
                "demo.service not active: operator may bring demo.service back; still works: console; paused: demo.service",
            ),
            (
                "mystery",
                false,
                "unparseable sample",
                Escalation::Warn,
                "probe unreadable: operator may check the source",
            ),
        ],
        &[
            (
                "unit:bad/name",
                false,
                "failed",
                Escalation::SuggestRestart,
                "service not active: operator may bring service back; still works: none; paused: service",
            ),
            (
                "odd/name",
                false,
                "status 0",
                Escalation::SuggestRestart,
                "service down: operator may bring service back; still works: none; paused: service",
            ),
        ],
    ];

    for rows in groups {
        let results: Vec<ProbeResult> = rows
            .iter()
            .map(|(name, ok, detail, _, _)| sample(name, *ok, detail, "2026-10-19T16:05:00+02:00"))
            .collect();
        let got = decide(&results);
        assert_eq!(got.len(), rows.len());
        for (g, (name, _, _, level, text)) in got.iter().zip(rows.iter().copied()) {
            assert_eq!(g.name, name);
            assert_eq!(g.level, level);
            assert_eq!(g.level.as_str(), level.as_str());
            assert_eq!(g.text, text, "{name}");
            assert!(!g.text.contains('/'), "{}", g.text);
            assert_suggestion_only(&g.text);
        }
    }
}

#[test]
fn decide_text_is_suggestion_only() {
    // Suggestion text is not a command. A down probe names a service that
    // still works when that service is in the same slice.
    let slice = [
        sample("hermes", true, "status 200", "2026-10-19T16:10:00+02:00"),
        sample("voice", false, "status 503", "2026-10-19T16:10:00+02:00"),
        sample("mem", false, "used 90.0% warn", "2026-10-19T16:10:00+02:00"),
    ];
    let got = decide(&slice);
    assert_eq!(got.len(), 3);
    for g in &got {
        assert_suggestion_only(&g.text);
    }
    let down = got.iter().find(|g| g.name == "voice").expect("voice");
    assert_eq!(down.level, Escalation::SuggestRestart);
    let still = down
        .text
        .split("still works:")
        .nth(1)
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("");
    assert!(
        still.split(',').any(|part| part.trim() == "hermes"),
        "down probe must name a service that still works: {still}"
    );
}
