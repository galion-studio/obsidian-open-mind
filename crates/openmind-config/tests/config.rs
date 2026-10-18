// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
use openmind_config::{load_from, ConfigError};
use std::collections::HashMap;
use std::path::PathBuf;

fn out(name: &str, body: &str) -> PathBuf {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-out/openmind-config");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let m: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |k| m.get(k).cloned()
}

#[test]
fn defaults_no_file() {
    let c = load_from(None, &|_| None).unwrap();
    assert_eq!(c.hosts.ollama, "127.0.0.1");
    assert_eq!(c.hosts.hermes, "127.0.0.1");
    assert_eq!(c.hosts.remote_probe, None);
    assert_eq!(c.ports.ollama, 11434);
    assert_eq!(c.ports.hermes, 8642);
    assert_eq!(c.ports.whisper, 8643);
    assert_eq!(c.ports.docs, 8645);
    assert_eq!(c.ports.one, 8091);
    assert_eq!(c.ports.desk, 8646);
    assert_eq!(c.ports.console, 8080);
}

#[test]
fn missing_file_is_defaults() {
    let p = PathBuf::from("target/test-out/openmind-config/does-not-exist.toml");
    let c = load_from(Some(&p), &|_| None).unwrap();
    assert_eq!(c, Default::default());
}

#[test]
fn full_file() {
    let p = out(
        "full.toml",
        r#"
[hosts]
ollama = "ollama.example.invalid"
hermes = "hermes.example.invalid"
remote_probe = "probe.example.invalid"
[ports]
ollama = 1
hermes = 2
whisper = 3
docs = 4
one = 5
desk = 6
console = 7
[paths]
base = "target/test-out/state"
[limits]
max_body_bytes = 10
request_timeout_secs = 11
max_connections = 12
"#,
    );
    let c = load_from(Some(&p), &|_| None).unwrap();
    assert_eq!(c.hosts.ollama, "ollama.example.invalid");
    assert_eq!(
        c.hosts.remote_probe.as_deref(),
        Some("probe.example.invalid")
    );
    assert_eq!(c.ports.console, 7);
    assert_eq!(
        c.paths.state_file("a.json"),
        PathBuf::from("target/test-out/state/a.json")
    );
    assert_eq!(c.limits.max_connections, 12);
}

#[test]
fn partial_file() {
    let p = out("partial.toml", "[ports]\none = 18091\n");
    let c = load_from(Some(&p), &|_| None).unwrap();
    assert_eq!(c.ports.one, 18091);
    assert_eq!(c.ports.desk, 8646);
    assert_eq!(c.hosts.ollama, "127.0.0.1");
}

#[test]
fn env_beats_file() {
    let p = out(
        "env.toml",
        "[ports]\none = 18091\n[hosts]\nollama = \"a.example.invalid\"\n",
    );
    let env = env_of(&[
        ("OPENMIND_PORTS_ONE", "18002"),
        ("OPENMIND_HOSTS_OLLAMA", "b.example.invalid"),
        ("OPENMIND_HOSTS_REMOTE_PROBE", "probe.example.invalid"),
    ]);
    let c = load_from(Some(&p), &env).unwrap();
    assert_eq!(c.ports.one, 18002);
    assert_eq!(c.hosts.ollama, "b.example.invalid");
    assert_eq!(
        c.hosts.remote_probe.as_deref(),
        Some("probe.example.invalid")
    );
}

#[test]
fn bad_toml_names_file() {
    let p = out("bad.toml", "[ports]\none = \"not a number\"\n");
    let e = load_from(Some(&p), &|_| None).unwrap_err();
    assert!(matches!(e, ConfigError::Parse { .. }));
    let msg = e.to_string();
    assert!(msg.contains("bad.toml"), "{msg}");
    assert!(msg.contains("one"), "{msg}");
    assert!(msg.contains("line 2"), "{msg}");
}

#[test]
fn bad_toml_syntax_names_file() {
    let p = out("syntax.toml", "[ports\n");
    let msg = load_from(Some(&p), &|_| None).unwrap_err().to_string();
    assert!(msg.contains("syntax.toml"), "{msg}");
}

#[test]
fn bad_env_value() {
    let env = env_of(&[("OPENMIND_PORTS_ONE", "eighty")]);
    let e = load_from(None, &env).unwrap_err();
    assert!(matches!(e, ConfigError::Env { .. }));
    let msg = e.to_string();
    assert!(msg.contains("OPENMIND_PORTS_ONE"), "{msg}");
    assert!(msg.contains("eighty"), "{msg}");
    let env = env_of(&[("OPENMIND_PORTS_ONE", "70000")]);
    assert!(load_from(None, &env).is_err());
}
