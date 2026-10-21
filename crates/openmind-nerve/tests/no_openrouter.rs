// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The nerve and the cortex never reach OpenRouter or a paid LLM endpoint
//! (Mate, 2026-10-21 18:10 +02:00: Hermes on the VPS is the only LLM path).
//!
//! 1. Source scan: no file under `src/` other than `egress.rs` names
//!    OpenRouter or an LLM endpoint path, and nothing reads an OPENROUTER env.
//! 2. Behaviour: the built-in HTTP client refuses LLM paths and non-loopback
//!    addresses before connecting (a local listener sees zero connections).

use openmind_nerve::egress;
use std::net::TcpListener;
use std::path::Path;
use std::time::Duration;

fn rs_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn source_has_no_llm_path() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![];
    rs_files(&src, &mut files);
    assert!(files.len() > 10, "found {} files", files.len());
    let mut bad = vec![];
    for f in &files {
        if f.file_name().is_some_and(|n| n == "egress.rs") {
            continue;
        }
        let text = std::fs::read_to_string(f).unwrap();
        let low = text.to_ascii_lowercase();
        for needle in egress::FORBIDDEN
            .iter()
            .chain(["chat/completions", "/v1/responses", "/v1/runs"].iter())
        {
            if low.contains(needle) {
                bad.push(format!("{}: {needle}", f.display()));
            }
        }
    }
    assert!(bad.is_empty(), "LLM/OpenRouter references: {bad:?}");
    const { assert!(!egress::LLM_ENABLED) };
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
