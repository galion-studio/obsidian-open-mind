// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The nerve and the cortex never reach OpenRouter directly (Mate,
//! 2026-10-21 18:10 +02:00), while the cortex may still hand incidents to
//! Hermes, which owns the only OpenRouter path (Mate t217, 22:50 +02:00).
//!
//! 1. Source scan: outside [`egress::GUARD_FILES`] no file under `src/` names
//!    OpenRouter or an LLM endpoint path; inside them, OpenRouter only appears
//!    to be refused, and the event-mode escalator has no LLM path baked in.
//! 2. Every external helper (curl) and every raw TCP connect sits behind a
//!    guard (`egress::check_url`, `escalate::endpoint_allowed`,
//!    `egress::check_addr`, `egress::mentions_forbidden`).
//! 3. Behaviour: the built-in HTTP client refuses LLM paths and non-loopback
//!    addresses before connecting; the escalator refuses OpenRouter and the
//!    Hermes gateway; the default escalation is a zero-spend handoff
//!    (`delegate`) and the handoff paths stay open.

use openmind_nerve::egress;
use openmind_nerve::evt::coalesce::Batch;
use openmind_nerve::evt::escalate::{self, EscalateConfig, Mode, Outcome};
use openmind_nerve::evt::{NerveEvent, Severity};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// `(path relative to src/, text)` for every source file.
fn sources() -> Vec<(String, String)> {
    let src = src();
    let mut files = vec![];
    rs_files(&src, &mut files);
    assert!(files.len() > 10, "found {} files", files.len());
    files
        .iter()
        .map(|f| {
            let rel = f
                .strip_prefix(&src)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (rel, std::fs::read_to_string(f).unwrap())
        })
        .collect()
}

/// Production code only: drop the `#[cfg(test)]` tail and `//` comments.
fn code_only(text: &str) -> String {
    let prod = text.split("#[cfg(test)]").next().unwrap_or("");
    prod.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

const LLM_NEEDLES: &[&str] = &["chat/completions", "/v1/responses", "/v1/runs"];

#[test]
fn source_has_no_llm_path() {
    let mut bad = vec![];
    let mut guards_seen = vec![];
    for (rel, text) in sources() {
        if egress::GUARD_FILES.contains(&rel.as_str()) {
            guards_seen.push(rel.clone());
            if rel == "egress.rs" {
                continue; // the lists themselves
            }
            // A guard file: no LLM path in production code, and every
            // non-comment OpenRouter mention is inside the refusal guard.
            let code = code_only(&text).to_ascii_lowercase();
            for n in LLM_NEEDLES {
                if code.contains(n) {
                    bad.push(format!("{rel}: {n} in production code"));
                }
            }
            for (i, l) in code.lines().enumerate() {
                if l.contains("openrouter")
                    && !(l.contains("refused")
                        || l.contains("must not")
                        || l.contains("contains_ci")
                        || l.contains("mentions_forbidden"))
                {
                    bad.push(format!(
                        "{rel}:{}: openrouter outside a refusal: {l}",
                        i + 1
                    ));
                }
            }
            continue;
        }
        let low = text.to_ascii_lowercase();
        for needle in egress::FORBIDDEN.iter().chain(LLM_NEEDLES.iter()) {
            if low.contains(needle) {
                bad.push(format!("{rel}: {needle}"));
            }
        }
        if code_only(&text).contains("OPENROUTER") {
            bad.push(format!("{rel}: reads an OPENROUTER env"));
        }
    }
    assert!(bad.is_empty(), "LLM/OpenRouter references: {bad:?}");
    assert!(
        guards_seen.len() == egress::GUARD_FILES.len(),
        "guard files missing: {guards_seen:?}"
    );
    const { assert!(!egress::LLM_ENABLED) };
}

#[test]
fn every_egress_site_is_guarded() {
    let mut bad = vec![];
    for (rel, text) in sources() {
        let code = code_only(&text);
        if code.contains("Command::new(\"curl\")") {
            let guarded = match rel.as_str() {
                "alerts.rs" => code.contains("egress::check_url"),
                "evt/escalate.rs" => code.contains("endpoint_allowed(&req.endpoint)?"),
                _ => false,
            };
            if !guarded {
                bad.push(format!("{rel}: curl without an egress guard"));
            }
        }
        if code.contains("TcpStream::connect") {
            let guarded = match rel.as_str() {
                "util.rs" => {
                    code.contains("egress::check_addr") && code.contains("egress::check_path")
                }
                "sensors/surface.rs" => code.contains("egress::mentions_forbidden"),
                _ => false,
            };
            if !guarded {
                bad.push(format!("{rel}: raw TCP connect without an egress guard"));
            }
        }
        for c in ["reqwest", "ureq", "hyper::", "UdpSocket"] {
            if code.contains(c) {
                bad.push(format!("{rel}: unexpected network client {c}"));
            }
        }
    }
    assert!(bad.is_empty(), "{bad:?}");
    let esc = std::fs::read_to_string(src().join("evt/escalate.rs")).unwrap();
    assert!(
        code_only(&esc).contains("crate::egress::mentions_forbidden"),
        "escalator shares the egress FORBIDDEN list"
    );
}

#[test]
fn client_refuses_llm_paths_without_connecting() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.set_nonblocking(true).unwrap();
    let addr = l.local_addr().unwrap();
    for p in [
        "/v1/chat/completions",
        "/p/default/v1/chat/completions",
        "/v1/runs",
        "/api/chat",
    ] {
        let r = openmind_nerve::util::http(addr, p, Some("{}"), Duration::from_millis(300));
        assert!(r.is_err(), "{p}");
        assert!(r.unwrap_err().contains("egress guard"), "{p}");
    }
    let r = openmind_nerve::util::http(
        "1.1.1.1:443".parse().unwrap(),
        "/health",
        None,
        Duration::from_millis(300),
    );
    assert!(r.unwrap_err().contains("not loopback"));
    // Nothing reached the listener.
    assert!(l.accept().is_err());
}

#[test]
fn escalator_refuses_openrouter_and_hermes_gateway() {
    for u in [
        "https://openrouter.ai/api/v1/chat/completions",
        "https://OPENROUTER.ai/api/v1",
        "https://openrouter%2eai/api",
        "https://user@openrouter.ai:443/x",
        "https://example.test/relay?u=openrouter.ai",
        "http://127.0.0.1:8642/v1/chat/completions",
        "http://localhost:8642/v1/runs",
        "https://hermes.galion.studio/v1/chat/completions",
    ] {
        let e = escalate::endpoint_allowed(u).unwrap_err();
        assert!(e.contains("refused"), "{u}: {e}");
        let env = |k: &str| match k {
            "NERVE_EVT_ESCALATE" => Some("openai".to_string()),
            "NERVE_EVT_LLM_ENDPOINT" => Some(u.to_string()),
            _ => None,
        };
        assert!(
            EscalateConfig::from_env(&env).is_err(),
            "config accepts {u}"
        );
    }
    assert!(escalate::endpoint_allowed("https://api.example.test/v1/chat/completions").is_ok());
}

#[test]
fn default_escalation_is_a_zero_spend_handoff() {
    let cfg = EscalateConfig::from_env(&|_| None).unwrap();
    assert_eq!(cfg.mode, Mode::Delegate);
    assert!(!cfg.enabled());
    assert_eq!(cfg.worst_case_usd(), 0.0);
    let b = Batch {
        id: 7,
        opened: 0,
        closed: 30,
        max_severity: Severity::High,
        events: vec![
            NerveEvent::new("journal", "unit.failed", Severity::High, "x")
                .unit("hermes-rs.service"),
        ],
        folded: 1,
    };
    let mut e = escalate::build(&cfg, None);
    assert_eq!(e.name(), "delegate");
    assert_eq!(e.escalate(&b, 31), Outcome::Delegated);
    // The handoff channels (event in, stream/feed out) stay allowed.
    for p in egress::HANDOFF_PATHS {
        assert!(egress::check_path(p).is_ok(), "{p}");
    }
}
