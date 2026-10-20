// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Golden test: the key set of link.json must not change.

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

const GOLDEN: &str = r#"["at","at_local","laptop_port","model","model_ms","hermes","hermes_ms","france","laptop","events"]"#;

fn run_once(tag: &str, france: Option<&str>) -> Value {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test-out/link-watch");
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join(format!("{tag}-link.json"));
    let _ = std::fs::remove_file(&out);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_link-watch"));
    cmd.env("LINK_WATCH_ONCE", "1")
        .env("LINK_WATCH_OUT", &out)
        .env("LINK_WATCH_LAPTOP", dir.join(format!("{tag}-laptop.json")))
        .env("LINK_WATCH_LOG", dir.join(format!("{tag}-log.json")))
        .env("LINK_WATCH_OLLAMA_URL", "http://127.0.0.1:1")
        .env("LINK_WATCH_HERMES_URL", "http://127.0.0.1:1")
        // No config file: today's Defaults, then the LINK_WATCH_* values above.
        .env("OPENMIND_CONFIG", dir.join("missing-openmind.toml"))
        .env_remove("LINK_WATCH_FRANCE_URL");
    if let Some(u) = france {
        cmd.env("LINK_WATCH_FRANCE_URL", u);
    }
    assert!(cmd.status().unwrap().success());
    serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap()
}

fn keys(v: &Value) -> Vec<String> {
    v.as_object().unwrap().keys().cloned().collect()
}

#[test]
fn key_set_matches_golden_and_france_unconfigured() {
    let golden: Vec<String> = serde_json::from_str(GOLDEN).unwrap();
    let v = run_once("unset", None);
    assert_eq!(keys(&v), golden);
    assert_eq!(v["france"], Value::String("unconfigured".into()));
}

#[test]
fn key_set_matches_golden_when_configured() {
    let golden: Vec<String> = serde_json::from_str(GOLDEN).unwrap();
    let v = run_once("set", Some("http://127.0.0.1:1"));
    assert_eq!(keys(&v), golden);
    assert_eq!(v["france"], Value::Bool(false));
}
