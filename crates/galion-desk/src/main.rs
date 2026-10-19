// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio
//
//! Bind, the Ollama address, and state paths come from `openmind-config` when
//! the config file exists (`$OPENMIND_CONFIG`, default
//! `/etc/openmind/openmind.toml`). `OPENMIND_*` overrides win over keys in
//! that file. `DESK_*` wins over both. A missing config file keeps today's
//! loopback addresses and absolute paths, and `OPENMIND_*` is not consulted.
//!
//!   DESK_BIND        listen address (default `127.0.0.1:8646`; from a present
//!                    file: `127.0.0.1:{ports.desk}`)
//!   DESK_OLLAMA      Ollama `host:port`, or an `http(s)` URL (default
//!                    `127.0.0.1:11434`; from a present file:
//!                    `{hosts.ollama}:{ports.ollama}`)
//!   DESK_STATE_DIR   desk state directory (default
//!                    `/opt/galion/galion-light/state`; from a present file:
//!                    `paths.base`)
//!   DESK_ONE_DIR     voice and session directory (default
//!                    `/workspace/galion-light/one`; from a present file:
//!                    `paths.base/one`)
//!   DESK_HERMES_DIR  Hermes state directory (default `/opt/galion/hermes`;
//!                    from a present file: `paths.base/hermes`)
//!   DESK_HERMES_BIN  mindpack binary (default
//!                    `/opt/galion/openmind/dist/bin/desk-hermes`)
//!
// Clippy allows, added only so `clippy -D warnings` passes. Both fire on the
// accept loop exactly as it is written in the live galion-desk source. The
// suggested rewrites (`.flatten()` on `listener.incoming()`, dropping the
// `let _ =` in front of a unit-valued `handle`) touch the control flow of the
// running server, so this ticket prefers an explicit allow over a rewrite.
#![allow(clippy::manual_flatten)]
#![allow(clippy::let_unit_value)]

use openmind_config::ConfigError;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Today's addresses and paths. Used when the config file is missing.
struct Defaults {
    bind: &'static str,
    ollama: &'static str,
    state_dir: &'static str,
    one_dir: &'static str,
    hermes_dir: &'static str,
    hermes_bin: &'static str,
}

const DEFAULTS: Defaults = Defaults {
    bind: "127.0.0.1:8646",
    ollama: "127.0.0.1:11434",
    state_dir: "/opt/galion/galion-light/state",
    one_dir: "/workspace/galion-light/one",
    hermes_dir: "/opt/galion/hermes",
    hermes_bin: "/opt/galion/openmind/dist/bin/desk-hermes",
};

/// Resolved listen address, Ollama address, and state roots for one process.
#[derive(Debug)]
struct Settings {
    bind: String,
    ollama: String,
    state_dir: PathBuf,
    one_dir: PathBuf,
    hermes_dir: PathBuf,
    hermes_bin: PathBuf,
}

impl Settings {
    fn from_defaults() -> Self {
        Settings {
            bind: DEFAULTS.bind.to_string(),
            ollama: DEFAULTS.ollama.to_string(),
            state_dir: PathBuf::from(DEFAULTS.state_dir),
            one_dir: PathBuf::from(DEFAULTS.one_dir),
            hermes_dir: PathBuf::from(DEFAULTS.hermes_dir),
            hermes_bin: PathBuf::from(DEFAULTS.hermes_bin),
        }
    }
}

static SETTINGS: OnceLock<Settings> = OnceLock::new();

fn cfg() -> &'static Settings {
    SETTINGS.get().expect("settings")
}

fn state(name: &str) -> PathBuf {
    cfg().state_dir.join(name)
}

fn one_file(name: &str) -> PathBuf {
    cfg().one_dir.join(name)
}

fn hermes_file(name: &str) -> PathBuf {
    cfg().hermes_dir.join(name)
}

/// `$OPENMIND_CONFIG` when set and non-empty, otherwise the crate default path.
fn config_file_path(env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    match env("OPENMIND_CONFIG") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => PathBuf::from(openmind_config::DEFAULT_PATH),
    }
}

/// `Ok(false)` when the file is absent (today's defaults apply). A directory
/// is treated as absent. Any other metadata error is reported.
fn config_file_present(path: &Path) -> Result<bool, ConfigError> {
    match std::fs::metadata(path) {
        Ok(meta) => Ok(meta.is_file()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(ConfigError::Read {
            file: path.to_path_buf(),
            message: err.to_string(),
        }),
    }
}

/// Accept `host:port` or an `http(s)` URL and return `host:port`.
fn normalize_ollama(value: &str) -> String {
    let trimmed = value.trim();
    let without_scheme = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .unwrap_or(trimmed);
    let stripped = without_scheme.trim_end_matches('/');
    if stripped.is_empty() {
        trimmed.to_string()
    } else {
        stripped.to_string()
    }
}

fn trim_dir(value: &str) -> String {
    let trimmed = value.trim();
    let stripped = trimmed.trim_end_matches('/');
    if stripped.is_empty() {
        trimmed.to_string()
    } else {
        stripped.to_string()
    }
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn apply_desk_env(settings: &mut Settings, env: &dyn Fn(&str) -> Option<String>) {
    if let Some(value) = env("DESK_BIND").as_deref().and_then(non_empty) {
        settings.bind = value.to_string();
    }
    if let Some(value) = env("DESK_OLLAMA").as_deref().and_then(non_empty) {
        settings.ollama = normalize_ollama(value);
    }
    if let Some(value) = env("DESK_STATE_DIR").as_deref().and_then(non_empty) {
        settings.state_dir = PathBuf::from(trim_dir(value));
    }
    if let Some(value) = env("DESK_ONE_DIR").as_deref().and_then(non_empty) {
        settings.one_dir = PathBuf::from(trim_dir(value));
    }
    if let Some(value) = env("DESK_HERMES_DIR").as_deref().and_then(non_empty) {
        settings.hermes_dir = PathBuf::from(trim_dir(value));
    }
    // Unset keeps the default. A set value, including empty, is used as-is,
    // matching the previous `std::env::var("DESK_HERMES_BIN").unwrap_or_else`.
    if let Some(value) = env("DESK_HERMES_BIN") {
        settings.hermes_bin = PathBuf::from(value);
    }
}

/// Precedence: `DESK_*`, then a present config file (with `OPENMIND_*`
/// already applied by `openmind-config`), then [`Defaults`] when that file
/// is missing.
fn resolve(env: &dyn Fn(&str) -> Option<String>) -> Result<Settings, ConfigError> {
    let mut settings = Settings::from_defaults();
    let path = config_file_path(env);
    if config_file_present(&path)? {
        let loaded = openmind_config::load_from(Some(&path), env)?;
        settings.bind = format!("127.0.0.1:{}", loaded.ports.desk);
        settings.ollama = format!("{}:{}", loaded.hosts.ollama.trim(), loaded.ports.ollama);
        settings.state_dir = loaded.paths.base.clone();
        settings.one_dir = loaded.paths.state_file("one");
        settings.hermes_dir = loaded.paths.state_file("hermes");
    }
    apply_desk_env(&mut settings, env);
    Ok(settings)
}

fn ollama_open() -> bool {
    let Ok(addr) = cfg().ollama.parse() else {
        return false;
    };
    TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok()
}

fn main() {
    let settings = match resolve(&|key| std::env::var(key).ok()) {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("galion-desk: {err}");
            std::process::exit(1);
        }
    };
    let bind = settings.bind.clone();
    SETTINGS.set(settings).expect("settings once");
    let listener = TcpListener::bind(&bind).expect("bind");
    eprintln!("galion-desk {bind}");
    for conn in listener.incoming() {
        if let Ok(stream) = conn {
            thread::spawn(move || {
                let _ = handle(stream);
            });
        }
    }
}

fn handle(mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf).unwrap_or(0);
    if n == 0 {
        return;
    }
    let req = String::from_utf8_lossy(&buf[..n]);
    let mut lines = req.split("\r\n");
    let start = lines.next().unwrap_or("");
    let mut parts = start.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let path = parts.next().unwrap_or("/");
    let mut body = String::new();
    if let Some(idx) = req.find("\r\n\r\n") {
        body = req[idx + 4..].to_string();
    }
    let json = if method == "POST" && path.starts_with("/desk") {
        apply(body.trim());
        snapshot()
    } else if method == "GET" && path.starts_with("/desk") {
        snapshot()
    } else if path.starts_with("/health") {
        "{\"ok\":true}".to_string()
    } else {
        send(&mut stream, 404, "{\"error\":\"not found\"}");
        return;
    };
    send(&mut stream, 200, &json);
}

fn send(stream: &mut TcpStream, code: u16, body: &str) {
    let reason = if code == 200 { "OK" } else { "Not Found" };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

fn apply(body: &str) {
    let act = json_string(body, "act");
    if act == "note" {
        write_note(&json_string(body, "text"));
    } else if act == "voice" {
        let name = json_string(body, "name");
        if name == "samantha" || name == "jarvis" {
            let _ = fs::write(one_file("voice"), format!("{name}\n"));
        }
    } else if act == "drop" {
        drop_note();
    } else if act == "stamp" {
        let line = if ollama_open() { "up" } else { "down" };
        let hermes = if tcp_open(8642) { "up" } else { "down" };
        write_note(&format!("Stamp. Line {line}. Hermes {hermes}."));
    } else if act == "hold" {
        let path = state("desk-hold");
        if fs::metadata(&path).is_ok() {
            let _ = fs::remove_file(&path);
        } else {
            let _ = fs::write(&path, "held\n");
        }
    } else if act == "wake" {
        wake();
    } else if act == "rest" {
        rest();
    } else if act == "focus" {
        focus_set(&json_string(body, "text"));
    }
}

fn focus_set(raw: &str) {
    let id: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(80)
        .collect();
    let _ = fs::create_dir_all(&cfg().state_dir);
    if id.is_empty() {
        let _ = fs::remove_file(state("desk-focus"));
    } else {
        let _ = fs::write(state("desk-focus"), format!("{id}\n"));
    }
}

fn focus_id() -> String {
    fs::read_to_string(state("desk-focus"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn write_note(text: &str) {
    let text = text.replace(['\n', '\r'], " ");
    let text: String = text.chars().take(160).collect();
    if text.trim().is_empty() {
        return;
    }
    let _ = fs::create_dir_all(&cfg().state_dir);
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(state("desk-notes.jsonl"))
    {
        let line = format!("{{\"at\":{},\"text\":\"{}\"}}\n", now(), escape(&text));
        let _ = file.write_all(line.as_bytes());
    }
}

fn drop_note() {
    let raw = fs::read_to_string(state("desk-notes.jsonl")).unwrap_or_default();
    let mut rows: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
    if rows.is_empty() {
        return;
    }
    rows.pop();
    let mut out = rows.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    let _ = fs::write(state("desk-notes.jsonl"), out);
}

static WAKING: AtomicBool = AtomicBool::new(false);

fn wake() {
    if WAKING.swap(true, Ordering::SeqCst) {
        return;
    }
    let host = cfg().ollama.clone();
    thread::spawn(move || {
        let body = "{\"model\":\"galion:latest\",\"prompt\":\"ok\",\"stream\":false,\"options\":{\"num_predict\":1}}";
        let _ = http_post(&host, "/api/generate", body);
        WAKING.store(false, Ordering::SeqCst);
    });
}

static RESTING: AtomicBool = AtomicBool::new(false);

fn rest() {
    let name = ollama_ps().0;
    if name.is_empty() || RESTING.swap(true, Ordering::SeqCst) {
        return;
    }
    let host = cfg().ollama.clone();
    thread::spawn(move || {
        let body = format!(
            "{{\"model\":\"{}\",\"keep_alive\":0,\"prompt\":\"\",\"stream\":false}}",
            escape(&name)
        );
        let _ = http_post(&host, "/api/generate", &body);
        RESTING.store(false, Ordering::SeqCst);
    });
}

fn snapshot() -> String {
    let link = fs::read_to_string(state("link.json")).unwrap_or_default();
    let model_ps = ollama_ps();
    let (model, vram, ctx) = if model_ps.0.is_empty() {
        let raw = json_string(&link, "model");
        (clean_model(&raw), 0, last_ctx(&link))
    } else {
        (clean_model(&model_ps.0), model_ps.1, model_ps.2)
    };
    let voice = fs::read_to_string(one_file("voice"))
        .unwrap_or_else(|_| "samantha".into())
        .trim()
        .to_string();
    let voice = if voice == "jarvis" {
        "jarvis"
    } else {
        "samantha"
    };
    let (heard, said, turns) = one_last();
    let notes = notes();
    format!(
        "{{\"foundation\":\"rust\",\"at\":{},\"line\":{},\"model\":\"{}\",\"vram\":{},\"ctx\":{},\"hermes\":{},\"one\":{},\"tunnel\":{},\"voice\":\"{}\",\"heard\":\"{}\",\"said\":\"{}\",\"turns\":{},\"waking\":{},\"resting\":{},\"hold\":{},\"services\":[{}],\"board\":{},\"events\":[{}],\"mindpack\":{},\"focus\":\"{}\",\"notes\":[{}]}}",
        now(),
        ollama_open(),
        escape(&model),
        vram,
        ctx,
        tcp_open(8642),
        tcp_open(8002),
        tcp_open(20241),
        voice,
        escape(&heard),
        escape(&said),
        turns,
        WAKING.load(Ordering::SeqCst),
        RESTING.load(Ordering::SeqCst),
        fs::metadata(state("desk-hold")).is_ok(),
        services(),
        board(),
        events(&link),
        mindpack(),
        escape(&focus_id()),
        notes,
    )
}

fn mindpack() -> String {
    // Replaces the old python script desk-hermes.py with the Rust binary.
    let bin = cfg().hermes_bin.clone();
    let Ok(out) = Command::new(bin).output() else {
        return "{}".to_string();
    };
    if !out.status.success() {
        return "{}".to_string();
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.starts_with('{') {
        text
    } else {
        "{}".to_string()
    }
}

fn clean_model(raw: &str) -> String {
    let cut = raw.split(" ctx ").next().unwrap_or(raw);
    cut.replace(" (idle)", "").trim().to_string()
}

fn last_ctx(link: &str) -> u64 {
    let mut found = 0u64;
    let mut rest = link;
    while let Some(i) = rest.find("ctx ") {
        let tail = &rest[i + 4..];
        let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<u64>() {
            if n > 0 {
                found = n;
            }
        }
        rest = &rest[i + 4..];
    }
    found
}

fn services() -> String {
    let rows = [
        ("Console", Some(8080u16)),
        ("Origin", Some(80u16)),
        ("Hermes", Some(8642u16)),
        ("Laptop", None),
        ("One", Some(8002u16)),
        ("Room", Some(4020u16)),
        ("Tulu", Some(8093u16)),
        ("Tunnel", Some(20241u16)),
    ];
    rows.iter()
        .map(|(name, port)| {
            let up = match port {
                Some(port) => tcp_open(*port),
                None => ollama_open(),
            };
            format!("{{\"name\":\"{name}\",\"up\":{up}}}")
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn board() -> String {
    let link = fs::read_to_string(state("link.json")).unwrap_or_default();
    let survival = fs::read_to_string(state("survival.json")).unwrap_or_default();
    let gw = fs::read_to_string(hermes_file("gateway_state.json")).unwrap_or_default();
    let machine = fs::read_to_string(state("machine.json")).unwrap_or_default();
    let studio = {
        let name = json_string(&link, "name");
        if name.is_empty() {
            "Galion Studio".to_string()
        } else {
            name
        }
    };
    let laptop_ram = format!(
        "{} / {} GB",
        json_amount(&link, "free_gb"),
        json_amount(&link, "ram_gb")
    );
    format!(
        "{{\"studio\":\"{}\",\"gpu\":\"{}\",\"cpu\":\"{}\",\"laptop_ram\":\"{}\",\"pod_ram\":\"{}\",\"hermes_ms\":{},\"model_ms\":{},\"agents\":{},\"version\":\"{}\",\"disk\":\"{}%\",\"load\":\"{}\",\"pod\":\"{}\",\"france\":{}}}",
        escape(&studio),
        escape(&short_gpu(&json_string(&link, "gpu"))),
        escape(&short_cpu(&json_string(&link, "cpu"))),
        escape(&laptop_ram),
        escape(&pod_ram()),
        json_number(&link, "hermes_ms"),
        json_number(&link, "model_ms"),
        json_number(&gw, "active_agents"),
        escape(&json_string(&gw, "code_version")),
        json_amount(&survival, "volume_used_pct"),
        escape(&load_line()),
        escape(&json_string(&machine, "publicIp")),
        flag(&link, "france"),
    )
}

fn events(link: &str) -> String {
    let Some(i) = link.find("\"events\"") else {
        return String::new();
    };
    let rest = &link[i..];
    let Some(start) = rest.find('[') else {
        return String::new();
    };
    let Some(end) = rest.find(']') else {
        return String::new();
    };
    let mut rows = Vec::new();
    let mut cur = String::new();
    let mut inside = false;
    let mut esc = false;
    for c in rest[start + 1..end].chars() {
        if !inside {
            if c == '"' {
                inside = true;
            }
            continue;
        }
        if esc {
            cur.push(c);
            esc = false;
            continue;
        }
        if c == '\\' {
            esc = true;
            continue;
        }
        if c == '"' {
            rows.push(cur.clone());
            cur.clear();
            inside = false;
            continue;
        }
        cur.push(c);
    }
    let take = if rows.len() > 4 {
        &rows[rows.len() - 4..]
    } else {
        &rows[..]
    };
    take.iter()
        .map(|row| format!("\"{}\"", escape(row)))
        .collect::<Vec<_>>()
        .join(",")
}

fn short_gpu(raw: &str) -> String {
    if raw.contains("4060") {
        let mib = raw.split(',').nth(1).unwrap_or("").trim();
        if mib.is_empty() {
            "RTX 4060".to_string()
        } else {
            format!("RTX 4060, {mib}")
        }
    } else if raw.is_empty() {
        "—".to_string()
    } else {
        raw.chars().take(36).collect()
    }
}

fn short_cpu(raw: &str) -> String {
    for mark in ["i7-", "i5-", "i9-"] {
        if let Some(i) = raw.find(mark) {
            return raw[i..].chars().take(16).collect();
        }
    }
    if raw.is_empty() {
        "—".to_string()
    } else {
        raw.chars().take(28).collect()
    }
}

fn json_amount(src: &str, key: &str) -> String {
    let pat = format!("\"{key}\"");
    let Some(i) = src.find(&pat) else {
        return "—".to_string();
    };
    let rest = &src[i + pat.len()..];
    let Some(colon) = rest.find(':') else {
        return "—".to_string();
    };
    let text: String = rest[colon + 1..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if text.is_empty() {
        "—".to_string()
    } else {
        text
    }
}

fn flag(src: &str, key: &str) -> bool {
    let pat = format!("\"{key}\"");
    let Some(i) = src.find(&pat) else {
        return false;
    };
    let rest = &src[i + pat.len()..];
    let Some(colon) = rest.find(':') else {
        return false;
    };
    rest[colon + 1..].trim_start().starts_with("true")
}

fn pod_ram() -> String {
    let raw = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let total = kb(&raw, "MemTotal:");
    let avail = kb(&raw, "MemAvailable:");
    if total == 0 {
        return "—".to_string();
    }
    format!("{} / {} GB", avail / 1024 / 1024, total / 1024 / 1024)
}

fn kb(src: &str, key: &str) -> u64 {
    for line in src.lines() {
        if let Some(rest) = line.strip_prefix(key) {
            let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
            return digits.parse().unwrap_or(0);
        }
    }
    0
}

fn load_line() -> String {
    let raw = fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let one = raw.split_whitespace().next().unwrap_or("0");
    let n = fs::read_to_string("/proc/cpuinfo")
        .unwrap_or_default()
        .matches("processor")
        .count();
    let n = if n == 0 { 1 } else { n };
    format!("{one} / {n}")
}

fn notes() -> String {
    let raw = fs::read_to_string(state("desk-notes.jsonl")).unwrap_or_default();
    let mut rows: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
    if rows.len() > 12 {
        rows = rows[rows.len() - 12..].to_vec();
    }
    rows.join(",")
}

fn one_last() -> (String, String, usize) {
    let raw = fs::read_to_string(one_file("session.jsonl")).unwrap_or_default();
    let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
    let turns = lines.len();
    let last = lines.last().copied().unwrap_or("");
    (json_string(last, "heard"), json_string(last, "said"), turns)
}

fn ollama_ps() -> (String, u64, u64) {
    let raw = http_get(&cfg().ollama, "/api/ps");
    if raw.is_empty() {
        return (String::new(), 0, 0);
    }
    let name = json_string(&raw, "name");
    let vram = json_number(&raw, "size_vram");
    let ctx = json_number(&raw, "context_length");
    (name, vram, ctx)
}

fn http_get(host: &str, path: &str) -> String {
    let Ok(mut stream) = TcpStream::connect_timeout(
        &host
            .parse()
            .unwrap_or_else(|_| "127.0.0.1:1".parse().unwrap()),
        Duration::from_millis(400),
    ) else {
        return String::new();
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(700)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(400)));
    let req = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    if stream.write_all(req.as_bytes()).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    text.split("\r\n\r\n").nth(1).unwrap_or("").to_string()
}

fn tcp_open(port: u16) -> bool {
    TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_millis(200),
    )
    .is_ok()
}

fn http_post(host: &str, path: &str, body: &str) -> String {
    let Ok(mut stream) = TcpStream::connect_timeout(
        &host
            .parse()
            .unwrap_or_else(|_| "127.0.0.1:1".parse().unwrap()),
        Duration::from_secs(2),
    ) else {
        return String::new();
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(90)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    if stream.write_all(req.as_bytes()).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn json_string(src: &str, key: &str) -> String {
    let pat = format!("\"{key}\"");
    let Some(i) = src.find(&pat) else {
        return String::new();
    };
    let rest = &src[i + pat.len()..];
    let Some(colon) = rest.find(':') else {
        return String::new();
    };
    let rest = rest[colon + 1..].trim_start();
    if !rest.starts_with('"') {
        return String::new();
    }
    let mut out = String::new();
    let mut chars = rest[1..].chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                out.push(n);
            }
            continue;
        }
        if c == '"' {
            break;
        }
        out.push(c);
    }
    out
}

fn json_number(src: &str, key: &str) -> u64 {
    let pat = format!("\"{key}\"");
    let Some(i) = src.find(&pat) else {
        return 0;
    };
    let rest = &src[i + pat.len()..];
    let Some(colon) = rest.find(':') else {
        return 0;
    };
    let rest = rest[colon + 1..].trim_start();
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().unwrap_or(0)
}

fn escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' | '\r' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out_dir() -> PathBuf {
        let dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .join("test-out/galion-desk");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn env_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        move |key| {
            owned
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        }
    }

    fn write_fixture(name: &str, body: &str) -> PathBuf {
        let path = out_dir().join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn missing_config_file_keeps_defaults_and_ignores_openmind_env() {
        let missing = out_dir().join("does-not-exist.toml");
        let _ = std::fs::remove_file(&missing);
        let settings = resolve(&env_from(&[
            ("OPENMIND_CONFIG", missing.to_str().unwrap()),
            ("OPENMIND_HOSTS_OLLAMA", "evil.example.invalid"),
            ("OPENMIND_PORTS_DESK", "1"),
            ("OPENMIND_PATHS_BASE", "/tmp/not-today"),
            ("DESK_BIND", "   "),
        ]))
        .unwrap();
        assert_eq!(settings.bind, DEFAULTS.bind);
        assert_eq!(settings.ollama, DEFAULTS.ollama);
        assert_eq!(settings.state_dir, PathBuf::from(DEFAULTS.state_dir));
        assert_eq!(settings.one_dir, PathBuf::from(DEFAULTS.one_dir));
        assert_eq!(settings.hermes_dir, PathBuf::from(DEFAULTS.hermes_dir));
        assert_eq!(settings.hermes_bin, PathBuf::from(DEFAULTS.hermes_bin));
        assert_eq!(
            settings.state_dir.join("desk-notes.jsonl"),
            PathBuf::from("/opt/galion/galion-light/state/desk-notes.jsonl")
        );
        assert_eq!(
            settings.state_dir.join("desk-focus"),
            PathBuf::from("/opt/galion/galion-light/state/desk-focus")
        );
        assert_eq!(
            settings.state_dir.join("link.json"),
            PathBuf::from("/opt/galion/galion-light/state/link.json")
        );
        assert_eq!(
            settings.state_dir.join("desk-hold"),
            PathBuf::from("/opt/galion/galion-light/state/desk-hold")
        );
        assert_eq!(
            settings.state_dir.join("survival.json"),
            PathBuf::from("/opt/galion/galion-light/state/survival.json")
        );
        assert_eq!(
            settings.state_dir.join("machine.json"),
            PathBuf::from("/opt/galion/galion-light/state/machine.json")
        );
        assert_eq!(
            settings.one_dir.join("voice"),
            PathBuf::from("/workspace/galion-light/one/voice")
        );
        assert_eq!(
            settings.one_dir.join("session.jsonl"),
            PathBuf::from("/workspace/galion-light/one/session.jsonl")
        );
        assert_eq!(
            settings.hermes_dir.join("gateway_state.json"),
            PathBuf::from("/opt/galion/hermes/gateway_state.json")
        );
    }

    #[test]
    fn config_fixture_supplies_bind_ollama_and_state_paths() {
        let base = out_dir().join("unit-base");
        let path = write_fixture(
            "unit-fixture.toml",
            &format!(
                "\
[hosts]
ollama = \"ollama.example.invalid\"
[ports]
ollama = 14001
desk = 14002
[paths]
base = '{base}'
",
                base = base.display()
            ),
        );
        let settings = resolve(&env_from(&[("OPENMIND_CONFIG", path.to_str().unwrap())])).unwrap();
        assert_eq!(settings.bind, "127.0.0.1:14002");
        assert_eq!(settings.ollama, "ollama.example.invalid:14001");
        assert_eq!(settings.state_dir, base);
        assert_eq!(settings.one_dir, base.join("one"));
        assert_eq!(settings.hermes_dir, base.join("hermes"));
        assert_eq!(settings.state_dir.join("link.json"), base.join("link.json"));
        assert_eq!(
            settings.state_dir.join("desk-notes.jsonl"),
            base.join("desk-notes.jsonl")
        );
        assert_eq!(settings.one_dir.join("voice"), base.join("one/voice"));
        assert_eq!(
            settings.one_dir.join("session.jsonl"),
            base.join("one/session.jsonl")
        );
        assert_eq!(
            settings.hermes_dir.join("gateway_state.json"),
            base.join("hermes/gateway_state.json")
        );
        assert_eq!(settings.hermes_bin, PathBuf::from(DEFAULTS.hermes_bin));
    }

    #[test]
    fn openmind_env_wins_over_config_file() {
        let base = out_dir().join("unit-file-base");
        let path = write_fixture(
            "unit-env-file.toml",
            &format!(
                "\
[hosts]
ollama = \"file.example.invalid\"
[ports]
ollama = 1
desk = 2
[paths]
base = '{base}'
",
                base = base.display()
            ),
        );
        let overridden = out_dir().join("unit-env-base");
        let settings = resolve(&env_from(&[
            ("OPENMIND_CONFIG", path.to_str().unwrap()),
            ("OPENMIND_HOSTS_OLLAMA", "env.example.invalid"),
            ("OPENMIND_PORTS_OLLAMA", "9"),
            ("OPENMIND_PORTS_DESK", "8"),
            ("OPENMIND_PATHS_BASE", overridden.to_str().unwrap()),
        ]))
        .unwrap();
        assert_eq!(settings.bind, "127.0.0.1:8");
        assert_eq!(settings.ollama, "env.example.invalid:9");
        assert_eq!(settings.state_dir, overridden);
        assert_eq!(settings.one_dir, overridden.join("one"));
        assert_eq!(settings.hermes_dir, overridden.join("hermes"));
    }

    #[test]
    fn desk_env_wins_over_file_and_openmind_env() {
        let base = out_dir().join("unit-desk-base");
        let path = write_fixture(
            "unit-desk-file.toml",
            &format!(
                "\
[hosts]
ollama = \"file.example.invalid\"
[ports]
ollama = 1
desk = 2
[paths]
base = '{base}'
",
                base = base.display()
            ),
        );
        let state_dir = out_dir().join("env-state");
        let one_dir = out_dir().join("env-one");
        let hermes_dir = out_dir().join("env-hermes");
        let settings = resolve(&env_from(&[
            ("OPENMIND_CONFIG", path.to_str().unwrap()),
            ("OPENMIND_HOSTS_OLLAMA", "openmind.example.invalid"),
            ("OPENMIND_PORTS_OLLAMA", "9"),
            ("OPENMIND_PORTS_DESK", "8"),
            ("OPENMIND_PATHS_BASE", "/tmp/openmind-base"),
            ("DESK_BIND", "127.0.0.1:18646"),
            ("DESK_OLLAMA", "http://127.0.0.1:1/"),
            ("DESK_STATE_DIR", state_dir.to_str().unwrap()),
            ("DESK_ONE_DIR", one_dir.to_str().unwrap()),
            ("DESK_HERMES_DIR", hermes_dir.to_str().unwrap()),
            ("DESK_HERMES_BIN", "/bin/false"),
        ]))
        .unwrap();
        assert_eq!(settings.bind, "127.0.0.1:18646");
        assert_eq!(settings.ollama, "127.0.0.1:1");
        assert_eq!(settings.state_dir, state_dir);
        assert_eq!(settings.one_dir, one_dir);
        assert_eq!(settings.hermes_dir, hermes_dir);
        assert_eq!(settings.hermes_bin, PathBuf::from("/bin/false"));

        let missing = out_dir().join("also-missing.toml");
        let _ = std::fs::remove_file(&missing);
        let from_env = resolve(&env_from(&[
            ("OPENMIND_CONFIG", missing.to_str().unwrap()),
            ("OPENMIND_PATHS_BASE", "/tmp/ignored"),
            ("DESK_BIND", "127.0.0.1:18646"),
            ("DESK_OLLAMA", "https://10.0.0.8:11434"),
            ("DESK_STATE_DIR", "/tmp/desk-state/"),
        ]))
        .unwrap();
        assert_eq!(from_env.bind, "127.0.0.1:18646");
        assert_eq!(from_env.ollama, "10.0.0.8:11434");
        assert_eq!(from_env.state_dir, PathBuf::from("/tmp/desk-state"));
        assert_eq!(from_env.one_dir, PathBuf::from(DEFAULTS.one_dir));
        assert_eq!(from_env.hermes_dir, PathBuf::from(DEFAULTS.hermes_dir));
    }

    #[test]
    fn bad_config_file_is_an_error() {
        let path = write_fixture("unit-bad.toml", "[ports]\nollama = \"nope\"\n");
        let err = resolve(&env_from(&[("OPENMIND_CONFIG", path.to_str().unwrap())])).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unit-bad.toml"), "{msg}");
    }
}
