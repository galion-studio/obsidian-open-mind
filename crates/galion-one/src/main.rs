// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

// Clippy / rustc allows, added only so `clippy -D warnings` passes. The body of
// this 2.8k-line file is the live `one` source; behaviour has to stay identical,
// so where clippy wants a rewrite this ticket keeps the code and silences the
// lint instead.
//
// - manual_is_multiple_of: `tick % 5 == 0` and friends. The arithmetic is
//   correct and readable; rewriting it is churn in the warm-up scheduler.
// - manual_flatten: the `for ... { if let Ok(x) = ... }` accept loops.
//   `.flatten()` only skips the error arm too, but the accepted-connection
//   control flow is left exactly as it runs today.
// - manual_checked_ops: `a / b` after an explicit zero check. The check is
//   right above the division; folding it into `checked_div` is cosmetic.
// - sliced_string_as_bytes: `&text[i..].as_bytes()`. Re-slicing via
//   `&text.as_bytes()[i..]` is equivalent but touches every byte-offset site in
//   the small JSON readers, so the original form stays.
// - dead_code: the live binary carries these same unused items (`channel`,
//   `ask_hermes`, `cut_prefix`, `field_of`, `lane_json`, `lane_name`,
//   `set_lane`, `shorten`). They are unreferenced today, not newly orphaned.
#![allow(clippy::manual_is_multiple_of)]
#![allow(clippy::manual_flatten)]
#![allow(clippy::manual_checked_ops)]
#![allow(clippy::sliced_string_as_bytes)]
#![allow(dead_code)]

mod models_refresh;

use openmind_config::ConfigError;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// Bind, the mouth port, and state paths come from `openmind-config` when the
// config file exists (`$OPENMIND_CONFIG`, default `/etc/openmind/openmind.toml`).
// `OPENMIND_*` overrides win over keys in that file. `ONE_BIND`,
// `ONE_MOUTH_BIND`, `GALION_MOUTH`, `GALION_HOME`, and `ONE_HERMES_DIR` win
// over both. A missing config file keeps today's loopback addresses and
// absolute paths, and `OPENMIND_*` is not consulted.
//
//   ONE_BIND         listen address (default `127.0.0.1:8091`; from a present
//                    file: `127.0.0.1:{ports.one}`)
//   ONE_MOUTH_BIND   local mouth listen address (default `127.0.0.1:8002`)
//   GALION_MOUTH     mouth client `host:port` (default `127.0.0.1:{mouth port}`)
//   GALION_HOME      page and state root (default `/workspace/galion-light`;
//                    from a present file: `paths.base`)
//   ONE_HERMES_DIR   Hermes state directory (default `/workspace/hermes`;
//                    from a present file: `paths.base/hermes`)
//
// `openmind-config::Ports` has no TTS key. The mouth port therefore stays
// today's 8002 unless `ONE_MOUTH_BIND` or `GALION_MOUTH` is set. The device
// store and the Hermes env file are secret paths, not state: they stay at
// today's locations and are not taken from the config file.

/// Today's addresses and paths. Used when the config file is missing.
struct Defaults {
    bind: &'static str,
    mouth_bind: &'static str,
    mouth_port: u16,
    home: &'static str,
    hermes_dir: &'static str,
}

const DEFAULTS: Defaults = Defaults {
    bind: "127.0.0.1:8091",
    mouth_bind: "127.0.0.1:8002",
    mouth_port: 8002,
    home: "/workspace/galion-light",
    hermes_dir: "/workspace/hermes",
};

/// Resolved listen addresses and state roots for one process.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Settings {
    bind: String,
    mouth_bind: String,
    mouth_port: u16,
    home: String,
    hermes_dir: String,
}

impl Settings {
    fn from_defaults() -> Self {
        Settings {
            bind: DEFAULTS.bind.to_string(),
            mouth_bind: DEFAULTS.mouth_bind.to_string(),
            mouth_port: DEFAULTS.mouth_port,
            home: DEFAULTS.home.to_string(),
            hermes_dir: DEFAULTS.hermes_dir.to_string(),
        }
    }

    fn home_file(&self, rel: &str) -> String {
        let root = self.home.trim_end_matches('/');
        format!("{root}/{rel}")
    }

    fn hermes_file(&self, rel: &str) -> String {
        let root = self.hermes_dir.trim_end_matches('/');
        format!("{root}/{rel}")
    }

    fn voice_log(&self) -> String {
        self.hermes_file("channels/one.jsonl")
    }

    fn channel(&self) -> String {
        self.hermes_file("channels/galion-one.md")
    }

    fn hermes_channels(&self) -> String {
        self.hermes_file("channels")
    }

    fn modes_path(&self) -> String {
        self.hermes_file("modes.json")
    }
}

static SETTINGS: OnceLock<Settings> = OnceLock::new();

fn cfg() -> &'static Settings {
    SETTINGS.get().expect("settings")
}

fn endpoint_from(
    env: &dyn Fn(&str) -> Option<String>,
    key: &str,
    port: u16,
) -> std::net::SocketAddr {
    let name = match key {
        "ear" => "GALION_EAR",
        "mouth" => "GALION_MOUTH",
        "mind" => "GALION_MIND",
        "eye" => "GALION_EYE",
        _ => "",
    };
    let raw = env(name).unwrap_or_default();
    let raw = raw.trim();
    if !raw.is_empty() {
        if let Ok(mut found) = std::net::ToSocketAddrs::to_socket_addrs(raw) {
            if let Some(addr) = found.next() {
                return addr;
            }
        }
    }
    std::net::SocketAddr::from(([127, 0, 0, 1], port))
}

fn endpoint(key: &str, port: u16) -> std::net::SocketAddr {
    endpoint_from(&|name| std::env::var(name).ok(), key, port)
}

fn ear_addr() -> std::net::SocketAddr {
    static SLOT: OnceLock<std::net::SocketAddr> = OnceLock::new();
    *SLOT.get_or_init(|| endpoint("ear", 8643))
}
fn mouth_addr() -> std::net::SocketAddr {
    static SLOT: OnceLock<std::net::SocketAddr> = OnceLock::new();
    *SLOT.get_or_init(|| endpoint("mouth", cfg().mouth_port))
}
fn mind_addr() -> std::net::SocketAddr {
    static SLOT: OnceLock<std::net::SocketAddr> = OnceLock::new();
    *SLOT.get_or_init(|| endpoint("mind", 8642))
}
fn eye_addr() -> std::net::SocketAddr {
    static SLOT: OnceLock<std::net::SocketAddr> = OnceLock::new();
    *SLOT.get_or_init(|| endpoint("eye", 8645))
}

fn mouth_remote() -> bool {
    let addr = mouth_addr();
    !(addr.ip().is_loopback() && addr.port() == cfg().mouth_port)
}

fn home() -> String {
    cfg().home.clone()
}

fn p(rel: &str) -> String {
    let root = home();
    let root = root.trim_end_matches('/');
    format!("{root}/{rel}")
}

fn voice_log() -> String {
    cfg().voice_log()
}

fn hermes_channels() -> String {
    cfg().hermes_channels()
}

fn modes_path() -> String {
    cfg().modes_path()
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

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn apply_one_env(settings: &mut Settings, env: &dyn Fn(&str) -> Option<String>) {
    if let Some(value) = env("ONE_BIND").as_deref().and_then(non_empty) {
        settings.bind = value.to_string();
    }
    if let Some(value) = env("ONE_MOUTH_BIND").as_deref().and_then(non_empty) {
        settings.mouth_bind = value.to_string();
    }
    // A set value, including empty or blank, wins. That matches
    // `std::env::var("GALION_HOME").unwrap_or_else(|_| default)`.
    if let Some(value) = env("GALION_HOME") {
        settings.home = value;
    }
    if let Some(value) = env("ONE_HERMES_DIR").as_deref().and_then(non_empty) {
        settings.hermes_dir = value.to_string();
    }
}

/// Precedence: `ONE_*` and `GALION_HOME`, then a present config file (with
/// `OPENMIND_*` already applied by `openmind-config`), then [`Defaults`] when
/// that file is missing. `GALION_MOUTH` is applied later, inside [`endpoint`],
/// and wins over `mouth_port`.
fn resolve(env: &dyn Fn(&str) -> Option<String>) -> Result<Settings, ConfigError> {
    let mut settings = Settings::from_defaults();
    let path = config_file_path(env);
    if config_file_present(&path)? {
        let loaded = openmind_config::load_from(Some(&path), env)?;
        let one_port = loaded.ports.one;
        settings.bind = format!("127.0.0.1:{one_port}");
        // The crate's port list has no mouth/TTS key. Keep today's mouth port
        // so a present file cannot move 8002 onto `ports.one` (8091).
        settings.mouth_port = DEFAULTS.mouth_port;
        let mouth_port = settings.mouth_port;
        settings.mouth_bind = format!("127.0.0.1:{mouth_port}");
        settings.home = path_to_string(&loaded.paths.base);
        settings.hermes_dir = path_to_string(&loaded.paths.state_file("hermes"));
    }
    apply_one_env(&mut settings, env);
    Ok(settings)
}

fn one_page() -> Vec<u8> {
    let raw = std::fs::read(p("one/index.html")).unwrap_or_else(|_| b"missing".to_vec());
    let mut html = String::from_utf8_lossy(&raw).into_owned();
    let boot = r##"<style id="galion-fit">html,body{overflow-x:clip;max-width:100%}body{grid-template-columns:minmax(0,1fr)!important;width:100%}#pair{position:fixed!important;left:50%!important;bottom:max(6.4rem,24vh)!important;transform:translateX(-50%);z-index:3;width:min(12rem,calc(100% - 3rem));max-width:100%;margin:0}</style><div id="galion-boot" role="status" aria-label="Galion One"><div class="lock"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 15a3 3 0 0 0 3-3V7a3 3 0 0 0-6 0v5a3 3 0 0 0 3 3M8 19h8"/></svg><p class="name">Galion One</p><p class="stack">Obsidian Open Mind Stack</p></div></div><style>#galion-boot{position:fixed;inset:0;z-index:100000;display:flex;align-items:center;justify-content:center;overflow:hidden;background:#C4553A;color:#FFFFFF;font-family:"IBM Plex Sans","Segoe UI",system-ui,sans-serif;font-weight:400}#galion-boot .lock{display:flex;flex-direction:column;align-items:center;transform:translateY(-8%)}#galion-boot svg{width:40px;height:40px;opacity:0;animation:galion-in .8s cubic-bezier(.22,1,.36,1) forwards}#galion-boot .name{margin:20px 0 0;font-size:21px;font-weight:400;letter-spacing:-.03em;opacity:0;animation:galion-in .8s cubic-bezier(.22,1,.36,1) .12s forwards}#galion-boot .stack{margin:8px 0 0;font-size:11px;font-weight:400;letter-spacing:.18em;text-transform:uppercase;color:rgba(255,255,255,.78);opacity:0;animation:galion-in .8s cubic-bezier(.22,1,.36,1) .24s forwards}@keyframes galion-in{from{opacity:0;transform:translateY(8px)}to{opacity:1;transform:none}}#galion-boot.go{opacity:0;transition:opacity .5s cubic-bezier(.4,0,.2,1)}</style><script>setTimeout(function(){var n=document.getElementById("galion-boot");if(!n)return;n.classList.add("go");setTimeout(function(){n.remove()},520)},2000)</script>"##;
    if let Some(at) = html.find("<body>") {
        html.insert_str(at + 6, boot);
    }
    html.into_bytes()
}

const STORE: &str = "/root/galion-secrets/admin-device.json";
const QUOTA: u64 = 100 * 1024 * 1024 * 1024;

struct Pulse {
    id: &'static str,
    port: u16,
    ok: bool,
    ms: u32,
    note: String,
}

struct Beat {
    whisper: u32,
    hermes: u32,
    tts: u32,
}

struct Guard {
    service: String,
    ok: bool,
    status: String,
    ms: u32,
}

struct Edge {
    host: &'static str,
    code: u16,
    ms: u32,
}

#[derive(Clone)]
struct Turn {
    you: String,
    hermes: String,
    ms: u32,
}

#[derive(Clone)]
struct Unit {
    id: String,
    ok: bool,
    note: String,
}

struct Nerve {
    services: Vec<Pulse>,
    extra: Vec<Unit>,
    beats: Vec<Beat>,
    events: Vec<String>,
    defence: Vec<Guard>,
    edge: Vec<Edge>,
    line: Vec<Turn>,
    turns: u32,
    cpu: u32,
    load: f32,
    cores: u32,
    memory: u32,
    mem_used: u64,
    mem_total: u64,
    disk_used: u64,
    disk_pct: u32,
    gpu_name: String,
    gpu_used: u32,
    gpu_total: u32,
    gpu_util: u32,
    gpu_ok: bool,
    tick: u32,
    cpu_idle: u64,
    cpu_total: u64,
}

static NERVE: OnceLock<Mutex<Nerve>> = OnceLock::new();
static MOUTH: OnceLock<Mutex<()>> = OnceLock::new();

fn nerve() -> &'static Mutex<Nerve> {
    NERVE.get_or_init(|| {
        Mutex::new(Nerve {
            services: Vec::new(),
            extra: Vec::new(),
            beats: Vec::new(),
            events: Vec::new(),
            defence: Vec::new(),
            edge: Vec::new(),
            line: Vec::new(),
            turns: count_lines(&voice_log()),
            cpu: 0,
            load: 0.0,
            cores: std::thread::available_parallelism()
                .map(|n| n.get() as u32)
                .unwrap_or(1),
            memory: 0,
            mem_used: 0,
            mem_total: 0,
            disk_used: 0,
            disk_pct: 0,
            gpu_name: String::new(),
            gpu_used: 0,
            gpu_total: 0,
            gpu_util: 0,
            gpu_ok: false,
            tick: 0,
            cpu_idle: 0,
            cpu_total: 0,
        })
    })
}

fn mouth() -> &'static Mutex<()> {
    MOUTH.get_or_init(|| Mutex::new(()))
}

fn main() {
    let settings = match resolve(&|key| std::env::var(key).ok()) {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("galion-one: {err}");
            std::process::exit(1);
        }
    };
    let bind = settings.bind.clone();
    SETTINGS.set(settings).expect("settings once");
    thread::spawn(|| loop {
        sample();
        thread::sleep(Duration::from_secs(1));
    });
    thread::spawn(serve_tts);
    if std::env::var("ONE_NO_WARM").as_deref() != Ok("1") {
        thread::spawn(keep_warm);
    }
    let listener = TcpListener::bind(&bind).unwrap_or_else(|err| {
        eprintln!("one: bind failed: {err}");
        std::process::exit(1);
    });
    eprintln!("one: listening");
    for stream in listener.incoming() {
        if let Ok(stream) = stream {
            thread::spawn(move || {
                let _ = stream.set_nodelay(true);
                if let Err(err) = handle(stream) {
                    eprintln!("one: {err}");
                }
            });
        }
    }
}

fn handle(mut stream: TcpStream) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let header_end;
    loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_header_end(&buf) {
            header_end = pos;
            break;
        }
        if buf.len() > 64 * 1024 {
            return Ok(());
        }
    }
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.lines();
    let mut parts = lines.next().unwrap_or("").split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let raw_path = parts.next().unwrap_or("/");
    let (path, query) = raw_path
        .split_once('?')
        .map(|(p, q)| (p.to_string(), q.to_string()))
        .unwrap_or((raw_path.to_string(), String::new()));
    let mut length = 0usize;
    let mut cookie = String::new();
    let mut file_name = String::new();
    let mut with_files = String::new();
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            length = rest.trim().parse().unwrap_or(0);
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("cookie") {
                cookie = value.trim().to_string();
            }
            if name.eq_ignore_ascii_case("x-file-name") {
                file_name = value.trim().to_string();
            }
            if name.eq_ignore_ascii_case("x-files") {
                with_files = value.trim().to_string();
            }
        }
    }
    let from_name = query_get(&query, "name");
    if !from_name.is_empty() {
        file_name = from_name;
    }
    let from_files = query_get(&query, "files");
    if !from_files.is_empty() {
        with_files = from_files;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(if path == "/files" {
        90
    } else {
        20
    })));
    let cap = if path == "/files" {
        25_000_000
    } else {
        2_000_000
    };
    let mut body = buf[header_end + 4..].to_vec();
    while body.len() < length && length <= cap {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    if length > cap {
        return write_response(&mut stream, 413, "text/plain", b"too large", None);
    }
    body.truncate(length);

    if method == "GET" && (path == "/" || path == "/index.html") {
        let bytes = one_page();
        return write_response(&mut stream, 200, "text/html; charset=utf-8", &bytes, None);
    }
    if method == "GET" && path == "/nerve" {
        let bytes = fs::read(p("one/nerve.html")).unwrap_or_else(|_| b"missing".to_vec());
        return write_response(&mut stream, 200, "text/html; charset=utf-8", &bytes, None);
    }
    if method == "GET" && path == "/admin" {
        let bytes = fs::read(p("one/admin.html")).unwrap_or_else(|_| b"missing".to_vec());
        return write_response(&mut stream, 200, "text/html; charset=utf-8", &bytes, None);
    }
    if method == "GET" && path == "/font/jost.woff2" {
        let bytes = fs::read(p("one/font/jost.woff2")).unwrap_or_default();
        return write_response(&mut stream, 200, "font/woff2", &bytes, None);
    }
    if method == "GET" && path == "/mark.svg" {
        let bytes = fs::read(p("one/mark.svg")).unwrap_or_else(|_| b"missing".to_vec());
        return write_response(&mut stream, 200, "image/svg+xml", &bytes, None);
    }
    if method == "GET" && path == "/space" {
        let bytes = fs::read(p("one/space.html")).unwrap_or_else(|_| b"missing".to_vec());
        return write_response(&mut stream, 200, "text/html; charset=utf-8", &bytes, None);
    }
    if method == "GET" && path == "/api/v1/catalog" {
        let bytes = fs::read(p("one/catalog.json")).unwrap_or_else(|_| b"[]".to_vec());
        return write_response(&mut stream, 200, "application/json", &bytes, None);
    }
    if method == "GET" && path == "/api/v1/awake" {
        let bytes = awake_json().into_bytes();
        return write_response(&mut stream, 200, "application/json", &bytes, None);
    }
    if method == "GET" && path == "/health" {
        return write_response(&mut stream, 200, "text/plain", b"ok\n", None);
    }
    if method == "GET" && path == "/link" {
        let line = format!(
            "{{\"ear\":\"{}\",\"mouth\":\"{}\",\"mind\":\"{}\",\"eye\":\"{}\"}}",
            ear_addr(),
            mouth_addr(),
            mind_addr(),
            eye_addr()
        );
        return write_response(&mut stream, 200, "application/json", line.as_bytes(), None);
    }
    if method == "GET" && path.starts_with("/api/v1/analytics") {
        if !token_ok(&cookie) {
            return write_response(
                &mut stream,
                401,
                "application/json",
                br#"{"error":"pair"}"#,
                None,
            );
        }
        let bytes = analytics_json(&path).into_bytes();
        return write_response(&mut stream, 200, "application/json", &bytes, None);
    }
    if method == "GET" && path == "/me" {
        let you = token_ok(&cookie);
        let bytes = format!("{{\"you\":{you}}}").into_bytes();
        return write_response(&mut stream, 200, "application/json", &bytes, None);
    }
    if method == "POST" && path == "/admin/say" {
        if !token_ok(&cookie) {
            return write_response(
                &mut stream,
                401,
                "application/json",
                br#"{"error":"pair"}"#,
                None,
            );
        }
        let bytes = admin_say(&json_string(&body, "text"));
        return write_response(&mut stream, 200, "application/json", &bytes, None);
    }
    if method == "POST" && path == "/pair" {
        let (status, bytes, token) = pair(&json_string(&body, "code"));
        return write_response(
            &mut stream,
            status,
            "application/json",
            &bytes,
            token.as_deref(),
        );
    }
    if method == "POST" && (path == "/hear" || path == "/reply") {
        if !token_ok(&cookie) {
            return write_response(
                &mut stream,
                200,
                "application/json",
                br#"{"error":"pair"}"#,
                None,
            );
        }
        let bytes = if path == "/hear" {
            hear(&body)
        } else {
            return reply_live(&mut stream, &body, &with_files);
        };
        return write_response(&mut stream, 200, "application/json", &bytes, None);
    }
    if method == "GET" && path == "/session" {
        if !token_ok(&cookie) {
            return write_response(
                &mut stream,
                401,
                "application/json",
                br#"{"error":"pair"}"#,
                None,
            );
        }
        let bytes = session_json().into_bytes();
        return write_response(&mut stream, 200, "application/json", &bytes, None);
    }
    if method == "GET" && path == "/files" {
        if !token_ok(&cookie) {
            return write_response(
                &mut stream,
                401,
                "application/json",
                br#"{"error":"pair"}"#,
                None,
            );
        }
        let bytes = files_json().into_bytes();
        return write_response(&mut stream, 200, "application/json", &bytes, None);
    }
    if method == "POST" && path == "/files" {
        if !token_ok(&cookie) {
            return write_response(
                &mut stream,
                401,
                "application/json",
                br#"{"error":"pair"}"#,
                None,
            );
        }
        if body.is_empty() {
            return write_response(
                &mut stream,
                200,
                "application/json",
                br#"{"error":"empty"}"#,
                None,
            );
        }
        let bytes = save_file(&file_name, &body).into_bytes();
        return write_response(&mut stream, 200, "application/json", &bytes, None);
    }
    if method == "GET" && path.starts_with("/files/") {
        if !token_ok(&cookie) {
            return write_response(
                &mut stream,
                401,
                "application/json",
                br#"{"error":"pair"}"#,
                None,
            );
        }
        let Some((bytes, name)) = read_file(&path) else {
            return write_response(&mut stream, 404, "text/plain", b"missing\n", None);
        };
        let kind = format!(
            "application/octet-stream\r\nContent-Disposition: attachment; filename=\"{name}\""
        );
        return write_response(&mut stream, 200, &kind, &bytes, None);
    }
    if method == "GET" && path == "/model" {
        if !token_ok(&cookie) {
            return write_response(
                &mut stream,
                401,
                "application/json",
                br#"{"error":"pair"}"#,
                None,
            );
        }
        let bytes = one_model_json().into_bytes();
        return write_response(&mut stream, 200, "application/json", &bytes, None);
    }
    if method == "POST" && path == "/model" {
        if !token_ok(&cookie) {
            return write_response(
                &mut stream,
                401,
                "application/json",
                br#"{"error":"pair"}"#,
                None,
            );
        }
        set_one_model(&json_string(&body, "model"));
        let bytes = one_model_json().into_bytes();
        return write_response(&mut stream, 200, "application/json", &bytes, None);
    }
    if method == "GET" && path == "/models" {
        if !token_ok(&cookie) {
            return write_response(
                &mut stream,
                401,
                "application/json",
                br#"{"error":"pair"}"#,
                None,
            );
        }
        let bytes = models_json().into_bytes();
        return write_response(&mut stream, 200, "application/json", &bytes, None);
    }
    if method == "POST" && path == "/voice" {
        if !token_ok(&cookie) {
            return write_response(
                &mut stream,
                401,
                "application/json",
                br#"{"error":"pair"}"#,
                None,
            );
        }
        set_voice(&json_string(&body, "voice"));
        let bytes = format!("{{\"voice\":{}}}", quote(&voice_name())).into_bytes();
        return write_response(&mut stream, 200, "application/json", &bytes, None);
    }
    write_response(&mut stream, 404, "text/plain", b"not found\n", None)
}

fn hear(wav: &[u8]) -> Vec<u8> {
    let (text, ms, ear) = transcribe(wav);
    note(format!("ear {ms} ms"));
    if !ear {
        return br#"{"say":"No ear on this machine."}"#.to_vec();
    }
    format!("{{\"say\":{}}}", quote(&text)).into_bytes()
}

fn reply_live(stream: &mut TcpStream, wav: &[u8], files: &str) -> std::io::Result<()> {
    let (heard, whisper_ms, ear) = transcribe(wav);
    if !ear {
        return write_response(
            stream,
            200,
            "application/json",
            br#"{"say":"No ear on this machine."}"#,
            None,
        );
    }
    if heard.is_empty() {
        return write_response(stream, 200, "application/json", br#"{"say":"Again"}"#, None);
    }
    let Ok(_guard) = mouth().try_lock() else {
        let body = format!("{{\"heard\":{},\"say\":\"Wait\"}}", quote(&heard));
        return write_response(stream, 200, "application/json", body.as_bytes(), None);
    };
    begin_audio(stream)?;
    send_note(stream, &heard, "")?;
    let started = Instant::now();
    let mut all = String::new();
    let mut shown = String::new();
    let mut voiced = String::new();
    let asked = with_files_note(&heard, files);
    let pulled = pull_hermes(&asked, |delta| {
        all.push_str(delta);
        if toolish(&all) {
            return false;
        }
        let live = growing(&all);
        if live != shown && live.split_whitespace().count() != shown.split_whitespace().count() {
            shown = live.clone();
            let _ = send_note(stream, &heard, &shown);
        }
        if voiced.is_empty() {
            if let Some(sentence) = opening(&human(&all)) {
                if speak_out(stream, &sentence) {
                    voiced = sentence;
                    eprintln!("live: first {} ms", started.elapsed().as_millis());
                }
            }
        }
        !two_done(&all)
    });
    let say = four_form(&spoken(&human(&all)));
    if say.trim().is_empty() || (!pulled && all.trim().is_empty() && voiced.is_empty()) {
        if !pulled && all.trim().is_empty() && !heard.is_empty() {
            let say = "No mind on this machine.";
            let _ = send_note(stream, &heard, say);
            keep_turn(&heard, say);
        }
        let _ = end_audio(stream);
        return Ok(());
    }
    let rest = if voiced.is_empty() {
        say.clone()
    } else if say.starts_with(&voiced) {
        say[voiced.len()..].trim().to_string()
    } else {
        String::new()
    };
    if !rest.is_empty() {
        let _ = speak_out(stream, &rest);
    }
    let mind_ms = started.elapsed().as_millis() as u32;
    send_note(stream, &heard, &say)?;
    let mouth_ms = mind_ms;
    eprintln!("live: audio {mouth_ms} ms");
    remember(whisper_ms, mind_ms, mouth_ms, heard.len(), say.len());
    keep_turn(&heard, &say);
    end_audio(stream)
}

fn opening(text: &str) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() >= 2 {
        return Some(words[..2].join(" "));
    }
    None
}

fn cut_prefix(text: &str) -> Option<(String, String)> {
    let text = text.trim_start();
    if text.len() < 8 {
        return None;
    }
    if let Some(i) = text.find([',', '.', '!', '?']) {
        if i >= 3 {
            return Some((
                text[..=i].trim().to_string(),
                text[i + 1..].trim().to_string(),
            ));
        }
    }
    None
}

fn pull_hermes(heard: &str, mut on_delta: impl FnMut(&str) -> bool) -> bool {
    let setting = voice_setting();
    let payload = with_lane_model(format!(
        "{{\"model\":\"hermes-agent\",\"stream\":true,\"max_tokens\":160,\"messages\":[{{\"role\":\"system\",\"content\":{}}}, {{\"role\":\"user\",\"content\":{}}}]}}",
        quote(&setting),
        quote(heard)
    ));
    let key = hermes_key();
    let auth = format!("Bearer {key}");
    let addr = mind_addr();
    let mut sock = match TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
        Ok(sock) => sock,
        Err(_) => return false,
    };
    let _ = sock.set_read_timeout(Some(Duration::from_secs(22)));
    let _ = sock.set_write_timeout(Some(Duration::from_secs(5)));
    let head = format!(
        "POST /v1/chat/completions HTTP/1.0\r\nHost: {addr}\r\nConnection: close\r\nContent-Type: application/json\r\nAuthorization: {auth}\r\nX-Hermes-Session-Id: one-voice\r\nX-Hermes-Session-Key: galion-one\r\nContent-Length: {}\r\n\r\n",
        payload.len()
    );
    if sock.write_all(head.as_bytes()).is_err() || sock.write_all(payload.as_bytes()).is_err() {
        return false;
    }
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let mut header_done = false;
    let mut got = false;
    loop {
        let n = match sock.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        buf.extend_from_slice(&tmp[..n]);
        if !header_done {
            if let Some(pos) = find_header_end(&buf) {
                buf.drain(..pos + 4);
                header_done = true;
            } else {
                continue;
            }
        }
        while let Some(nl) = buf.iter().position(|b| *b == b'\n') {
            let raw = buf.drain(..=nl).collect::<Vec<_>>();
            let line = String::from_utf8_lossy(&raw)
                .trim()
                .trim_start_matches('\u{feff}')
                .to_string();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let data = line.strip_prefix("data:").unwrap_or(&line).trim();
            if data == "[DONE]" {
                return got;
            }
            let delta = json_string(data.as_bytes(), "content");
            if !delta.is_empty() {
                got = true;
                if !on_delta(&delta) {
                    return true;
                }
            }
        }
    }
    got
}

fn begin_audio(stream: &mut TcpStream) -> std::io::Result<()> {
    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nTransfer-Encoding: chunked\r\nCache-Control: no-store\r\nX-Accel-Buffering: no\r\nConnection: close\r\n\r\n")?;
    stream.flush()
}

fn send_note(stream: &mut TcpStream, heard: &str, say: &str) -> std::io::Result<()> {
    let body = format!("{{\"heard\":{},\"say\":{}}}", quote(heard), quote(say));
    let bytes = body.into_bytes();
    let mut frame = Vec::with_capacity(4 + bytes.len());
    frame.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    frame.extend_from_slice(&bytes);
    write_chunk(stream, &frame)
}

fn send_wav(stream: &mut TcpStream, wav: &[u8]) -> std::io::Result<()> {
    if wav.len() < 44 {
        return Ok(());
    }
    let mut frame = Vec::with_capacity(4 + wav.len());
    frame.extend_from_slice(&(wav.len() as u32).to_le_bytes());
    frame.extend_from_slice(wav);
    write_chunk(stream, &frame)
}

fn end_audio(stream: &mut TcpStream) -> std::io::Result<()> {
    write_chunk(stream, &0u32.to_le_bytes())?;
    stream.write_all(b"0\r\n\r\n")?;
    stream.flush()
}

fn write_chunk(stream: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
    if let Err(err) = stream.write_all(format!("{:X}\r\n", data.len()).as_bytes()) {
        return quiet(err);
    }
    if let Err(err) = stream.write_all(data) {
        return quiet(err);
    }
    if let Err(err) = stream.write_all(b"\r\n") {
        return quiet(err);
    }
    match stream.flush() {
        Ok(()) => Ok(()),
        Err(err) => quiet(err),
    }
}

fn quiet(err: std::io::Error) -> std::io::Result<()> {
    if err.kind() == std::io::ErrorKind::BrokenPipe
        || err.kind() == std::io::ErrorKind::ConnectionReset
    {
        Ok(())
    } else {
        Err(err)
    }
}

fn remember(whisper: u32, hermes: u32, tts: u32, heard: usize, said: usize) {
    if let Ok(mut n) = nerve().lock() {
        n.turns += 1;
        n.beats.insert(
            0,
            Beat {
                whisper,
                hermes,
                tts,
            },
        );
        n.beats.truncate(12);
        n.events.insert(
            0,
            format!(
                "{clock} ear {whisper} · mind {hermes} · mouth {tts}",
                clock = clock()
            ),
        );
        n.events.truncate(24);
    }
    let _ = fs::create_dir_all(hermes_channels());
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(voice_log())
    {
        let _ = writeln!(
            file,
            "{{\"id\":\"one-voice\",\"unix\":{},\"at\":\"{}\",\"channel\":\"galion-one\",\"heard\":{heard},\"said\":{said},\"whisper\":{whisper},\"hermes\":{hermes},\"tts\":{tts}}}",
            unix_secs(),
            clock()
        );
    }
}

fn transcribe(wav: &[u8]) -> (String, u32, bool) {
    let boundary = "onebound";
    let mut payload = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"speech.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
    )
    .into_bytes();
    payload.extend_from_slice(wav);
    payload.extend_from_slice(
        format!("\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"language\"\r\n\r\nen\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"fast\"\r\n\r\n1\r\n--{boundary}--\r\n").as_bytes(),
    );
    let t = Instant::now();
    let kind = format!("multipart/form-data; boundary={boundary}");
    let body = match http_to(
        ear_addr(),
        "POST",
        "/v1/audio/transcriptions",
        &[("Content-Type", kind.as_str())],
        &payload,
        Duration::from_secs(20),
    ) {
        Ok((_, body)) => body,
        Err(_) => return (String::new(), t.elapsed().as_millis() as u32, false),
    };
    let text = json_string(&body, "text");
    let text = text.chars().take(400).collect();
    (text, t.elapsed().as_millis() as u32, true)
}

fn ask_hermes(heard: &str) -> String {
    let setting = voice_setting();
    let payload = with_lane_model(format!(
        "{{\"model\":\"hermes-agent\",\"stream\":false,\"max_tokens\":160,\"messages\":[{{\"role\":\"system\",\"content\":{}}}, {{\"role\":\"user\",\"content\":{}}}]}}",
        quote(&setting),
        quote(heard)
    ));
    let key = hermes_key();
    let auth = format!("Bearer {key}");
    let body = match http_to(
        mind_addr(),
        "POST",
        "/v1/chat/completions",
        &[
            ("Authorization", auth.as_str()),
            ("Content-Type", "application/json"),
            ("X-Hermes-Session-Id", "one-voice"),
            ("X-Hermes-Session-Key", "galion-one"),
        ],
        payload.as_bytes(),
        Duration::from_secs(22),
    ) {
        Ok((_, body)) => body,
        Err(_) => return "Again".to_string(),
    };
    let text = String::from_utf8_lossy(&body);
    let say = json_string(text.as_bytes(), "content");
    let say = shorten(&say);
    if say.is_empty() {
        "Again".to_string()
    } else {
        say
    }
}

fn admin_say(text: &str) -> Vec<u8> {
    let text = text.trim();
    if text.is_empty() {
        return br#"{"say":""}"#.to_vec();
    }
    let Ok(_guard) = mouth().try_lock() else {
        return br#"{"say":"Wait"}"#.to_vec();
    };
    let prior: Vec<Turn> = nerve()
        .lock()
        .ok()
        .map(|n| n.line.iter().take(3).cloned().collect())
        .unwrap_or_default();
    let setting = "You are Hermes on the admin line. The admin is typing, not speaking. Short. Direct. No tools.";
    let mut messages = vec![format!(
        "{{\"role\":\"system\",\"content\":{}}}",
        quote(setting)
    )];
    for turn in prior.iter().rev() {
        messages.push(format!(
            "{{\"role\":\"user\",\"content\":{}}}",
            quote(&turn.you)
        ));
        messages.push(format!(
            "{{\"role\":\"assistant\",\"content\":{}}}",
            quote(&turn.hermes)
        ));
    }
    messages.push(format!("{{\"role\":\"user\",\"content\":{}}}", quote(text)));
    let payload = with_lane_model(format!(
        "{{\"model\":\"hermes-agent\",\"stream\":false,\"max_tokens\":180,\"messages\":[{}]}}",
        messages.join(",")
    ));
    let key = hermes_key();
    let auth = format!("Bearer {key}");
    let t = Instant::now();
    let body = match http_to(
        mind_addr(),
        "POST",
        "/v1/chat/completions",
        &[
            ("Authorization", auth.as_str()),
            ("Content-Type", "application/json"),
            ("X-Hermes-Session-Id", "one-admin"),
            ("X-Hermes-Session-Key", "galion-one"),
        ],
        payload.as_bytes(),
        Duration::from_secs(22),
    ) {
        Ok((_, body)) => body,
        Err(_) => return br#"{"say":"Again"}"#.to_vec(),
    };
    let ms = t.elapsed().as_millis() as u32;
    note(format!("admin {ms} ms"));
    let say = json_string(&body, "content");
    let say: String = say.chars().take(600).collect();
    let say = if say.is_empty() {
        "Again".to_string()
    } else {
        say
    };
    if let Ok(mut n) = nerve().lock() {
        n.line.insert(
            0,
            Turn {
                you: text.to_string(),
                hermes: say.clone(),
                ms,
            },
        );
        n.line.truncate(8);
    }
    format!("{{\"say\":{}}}", quote(&say)).into_bytes()
}

fn load_edge() -> Vec<Edge> {
    let hosts = [
        "one.galion.app",
        "console.galion.app",
        "obsidian.galion.app",
        "galion.studio",
        "merlinelement.com",
    ];
    let mut joins = Vec::new();
    for host in hosts {
        joins.push(thread::spawn(move || {
            let t = Instant::now();
            let out = Command::new("curl")
                .args([
                    "-sS",
                    "-o",
                    "/dev/null",
                    "-w",
                    "%{http_code}",
                    "--max-time",
                    "3",
                    "-A",
                    "galion-nerve",
                    &format!("https://{host}/"),
                ])
                .output();
            let code = out
                .ok()
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .trim()
                        .parse()
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            Edge {
                host,
                code,
                ms: t.elapsed().as_millis() as u32,
            }
        }));
    }
    joins.into_iter().filter_map(|j| j.join().ok()).collect()
}

fn load_defence() -> Vec<Guard> {
    #[cfg(galion_no_sqlite)]
    {
        return Vec::new();
    }
    #[cfg(not(galion_no_sqlite))]
    {
        use std::ffi::{CStr, CString};
        use std::os::raw::{c_char, c_int, c_void};
        extern "C" {
            fn sqlite3_open(filename: *const c_char, pp: *mut *mut c_void) -> c_int;
            fn sqlite3_close(db: *mut c_void) -> c_int;
            fn sqlite3_exec(
                db: *mut c_void,
                sql: *const c_char,
                cb: Option<
                    extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int,
                >,
                arg: *mut c_void,
                err: *mut *mut c_char,
            ) -> c_int;
        }
        extern "C" fn row(
            arg: *mut c_void,
            n: c_int,
            vals: *mut *mut c_char,
            _cols: *mut *mut c_char,
        ) -> c_int {
            if n < 3 || arg.is_null() || vals.is_null() {
                return 0;
            }
            unsafe {
                let out = &mut *(arg as *mut Vec<Guard>);
                let cell = |i: isize| {
                    let p = *vals.offset(i);
                    if p.is_null() {
                        String::new()
                    } else {
                        CStr::from_ptr(p).to_string_lossy().into_owned()
                    }
                };
                let service = cell(0);
                let status = cell(1);
                let ms = cell(2).parse().unwrap_or(0);
                if !service.is_empty() {
                    out.push(Guard {
                        service,
                        ok: status == "healthy",
                        status,
                        ms,
                    });
                }
            }
            0
        }
        let mut rows = Vec::new();
        let Ok(path) = CString::new(p("guardian/guardian.db")) else {
            return rows;
        };
        let Ok(sql) = CString::new("SELECT service, status, CAST(response_time_ms AS INT) FROM health_checks WHERE id IN (SELECT MAX(id) FROM health_checks GROUP BY service)") else { return rows };
        unsafe {
            let mut db = std::ptr::null_mut();
            if sqlite3_open(path.as_ptr(), &mut db) != 0 || db.is_null() {
                return rows;
            }
            let _ = sqlite3_exec(
                db,
                sql.as_ptr(),
                Some(row),
                &mut rows as *mut Vec<Guard> as *mut c_void,
                std::ptr::null_mut(),
            );
            sqlite3_close(db);
        }
        rows
    }
}

fn warm_due() -> bool {
    if let Ok(meta) = fs::metadata(p("one/session.jsonl")) {
        if let Ok(modified) = meta.modified() {
            if modified.elapsed().unwrap_or(Duration::from_secs(999)) < Duration::from_secs(25) {
                return false;
            }
        }
    }
    let last: f64 = fs::read_to_string(p("state/warm.ts"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0.0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    now - last >= 240.0
}

fn tone() -> Vec<u8> {
    let rate = 16000u32;
    let n = rate / 2;
    let mut pcm = Vec::with_capacity((n as usize) * 2);
    for i in 0..n {
        let sample =
            (800.0 * (2.0 * std::f64::consts::PI * 220.0 * i as f64 / rate as f64).sin()) as i16;
        pcm.extend_from_slice(&sample.to_le_bytes());
    }
    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(&pcm);
    wav
}

fn warm_pass() {
    let _ = transcribe(&tone());
    let audio = speak("Yes.");
    let key = hermes_key();
    let payload = with_lane_model(format!(
        "{{\"model\":\"hermes-agent\",\"stream\":false,\"max_tokens\":8,\"messages\":[{{\"role\":\"system\",\"content\":{}}}, {{\"role\":\"user\",\"content\":\".\"}}]}}",
        quote(&voice_setting())
    ));
    let auth = format!("Bearer {key}");
    let mind = http_to(
        mind_addr(),
        "POST",
        "/v1/chat/completions",
        &[
            ("Content-Type", "application/json"),
            ("Authorization", auth.as_str()),
            ("X-Hermes-Session-Id", "one-warm"),
            ("X-Hermes-Session-Key", "one-warm"),
        ],
        payload.as_bytes(),
        Duration::from_secs(25),
    )
    .map(|(code, _)| code)
    .unwrap_or(0);
    if audio.is_empty() && mind != 200 {
        eprintln!("warm rust skipped mouth=0 mind={mind}");
        return;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let _ = fs::create_dir_all(p("state"));
    let _ = fs::write(p("state/warm.ts"), format!("{now}"));
    eprintln!("warm rust mouth={} mind={mind}", audio.len());
}

fn keep_warm() {
    thread::sleep(Duration::from_secs(20));
    loop {
        if warm_due() && mouth().try_lock().is_ok() {
            warm_pass();
        }
        thread::sleep(Duration::from_secs(20));
    }
}

fn speak(text: &str) -> Vec<u8> {
    if mouth_remote() {
        let payload = format!("{{\"text\":{}}}", quote(text));
        let Ok((code, body)) = http_to(
            mouth_addr(),
            "POST",
            "/synthesize",
            &[("Content-Type", "application/json")],
            payload.as_bytes(),
            Duration::from_secs(30),
        ) else {
            return Vec::new();
        };
        if code == 200 && !body.is_empty() {
            return body;
        }
        return Vec::new();
    }
    // The old tts/worker.py spawn is gone. Voice-worker-unavailable returns
    // no audio, so /synthesize uses the same JSON error as the old off path.
    Vec::new()
}

fn speak_out(stream: &mut TcpStream, text: &str) -> bool {
    let audio = speak(text);
    !audio.is_empty() && send_wav(stream, &audio).is_ok()
}

fn serve_tts() {
    let mouth_bind = cfg().mouth_bind.clone();
    let listener = loop {
        match TcpListener::bind(&mouth_bind) {
            Ok(listener) => break listener,
            Err(_) => thread::sleep(Duration::from_millis(300)),
        }
    };
    eprintln!("voice: listening");
    for stream in listener.incoming() {
        if let Ok(stream) = stream {
            thread::spawn(move || {
                let _ = stream.set_nodelay(true);
                if let Err(err) = tts_http(stream) {
                    eprintln!("voice: {err}");
                }
            });
        }
    }
}

fn tts_http(mut stream: TcpStream) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end;
    loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_header_end(&buf) {
            header_end = pos;
            break;
        }
        if buf.len() > 64 * 1024 {
            return Ok(());
        }
    }
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.lines();
    let mut parts = lines.next().unwrap_or("").split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/").split('?').next().unwrap_or("/");
    let mut length = 0usize;
    for line in lines {
        if let Some(rest) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = rest.trim().parse().unwrap_or(0);
        }
    }
    let mut body = buf[header_end + 4..].to_vec();
    while body.len() < length && length <= 64 * 1024 {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(length);
    if method == "GET" && path == "/health" {
        // Voice worker unavailable. Same JSON as today's off path: the worker
        // was not loaded, so the mouth answers 503 with loaded:false.
        return write_response(
            &mut stream,
            503,
            "application/json",
            br#"{"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}"#,
            None,
        );
    }
    if method == "POST" && path == "/synthesize" {
        let text = json_string(&body, "text");
        let audio = speak(&text);
        if audio.is_empty() {
            return write_response(
                &mut stream,
                500,
                "application/json",
                br#"{"error":"voice"}"#,
                None,
            );
        }
        return write_response(&mut stream, 200, "audio/wav", &audio, None);
    }
    write_response(&mut stream, 404, "text/plain", b"not found\n", None)
}

fn share() -> String {
    p("one/share")
}

fn safe_name(raw: &str) -> String {
    let raw = raw.rsplit(['/', '\\']).next().unwrap_or("file");
    let mut out = String::new();
    for ch in raw.chars() {
        if out.chars().count() >= 80 {
            break;
        }
        if ch.is_control() || matches!(ch, '"' | '\r' | '\n' | '\\') {
            continue;
        }
        out.push(ch);
    }
    if out.is_empty() {
        "file".to_string()
    } else {
        out
    }
}

fn share_id() -> String {
    let mut buf = [0u8; 4];
    if let Ok(mut file) = File::open("/dev/urandom") {
        let _ = file.read_exact(&mut buf);
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!(
        "{now:x}{:02x}{:02x}{:02x}{:02x}",
        buf[0], buf[1], buf[2], buf[3]
    )
}

fn file_kind(name: &str, body: &[u8]) -> &'static str {
    if is_image(body) {
        return "image";
    }
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".pdf") || body.starts_with(b"%PDF") {
        return "pdf";
    }
    if [".txt", ".md", ".json", ".csv", ".log"]
        .iter()
        .any(|end| lower.ends_with(end))
    {
        return "text";
    }
    "other"
}

fn save_file(name: &str, body: &[u8]) -> String {
    let id = share_id();
    let name = safe_name(name);
    let kind = file_kind(&name, body);
    let dir = format!("{}/{}", share(), kind);
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(format!("{dir}/{id}"), body);
    let line = format!(
        "{{\"id\":{},\"name\":{},\"size\":{},\"kind\":{}}}\n",
        quote(&id),
        quote(&name),
        body.len(),
        quote(kind)
    );
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{}.jsonl", share()))
    {
        let _ = file.write_all(line.as_bytes());
    }
    format!(
        "{{\"id\":{},\"name\":{},\"size\":{},\"kind\":{}}}",
        quote(&id),
        quote(&name),
        body.len(),
        quote(kind)
    )
}

fn files_json() -> String {
    let text = fs::read_to_string(format!("{}.jsonl", share())).unwrap_or_default();
    let mut lines: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("\"id\""))
        .collect();
    if lines.len() > 24 {
        lines = lines[lines.len() - 24..].to_vec();
    }
    lines.reverse();
    format!("{{\"files\":[{}]}}", lines.join(","))
}

fn read_file(path: &str) -> Option<(Vec<u8>, String)> {
    let id = path.strip_prefix("/files/")?;
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let bytes = ["image", "pdf", "text", "other"]
        .iter()
        .find_map(|kind| fs::read(format!("{}/{}/{}", share(), kind, id)).ok())
        .or_else(|| fs::read(format!("{}/{}", share(), id)).ok())?;
    let text = fs::read_to_string(format!("{}.jsonl", share())).unwrap_or_default();
    let name = text
        .lines()
        .rev()
        .find_map(|line| {
            if line.contains(&format!("\"id\":\"{id}\"")) {
                Some(json_string(line.as_bytes(), "name"))
            } else {
                None
            }
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "file".to_string());
    Some((bytes, safe_name(&name)))
}

fn query_get(query: &str, key: &str) -> String {
    for pair in query.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        if k == key {
            return url_decode(v);
        }
    }
    String::new()
}

fn url_decode(raw: &str) -> String {
    let raw = raw.replace('+', " ");
    let bytes = raw.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(n) = u8::from_str_radix(hex, 16) {
                out.push(n);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn is_image(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8])
        || bytes.starts_with(b"\x89PNG")
        || (bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP")
}

fn look_image(bytes: &[u8]) -> String {
    let mut body = Vec::new();
    body.extend_from_slice(b"\r\n");
    let head = "--b\r\nContent-Disposition: form-data; name=\"file\"; filename=\"shot.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n";
    body = head.as_bytes().to_vec();
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n--b--\r\n");
    let headers = [("Content-Type", "multipart/form-data; boundary=b")];
    let Ok((code, resp)) = http_to(
        eye_addr(),
        "POST",
        "/caption",
        &headers,
        &body,
        Duration::from_secs(14),
    ) else {
        return String::new();
    };
    if code != 200 {
        return String::new();
    }
    json_string(&resp, "caption")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn pdf_clip(bytes: &[u8]) -> String {
    let path = "/tmp/galion-one-page.pdf";
    if fs::write(path, bytes).is_err() {
        return String::new();
    }
    let Ok(out) = Command::new("pdftotext")
        .args(["-f", "1", "-l", "1", "-q", path, "-"])
        .output()
    else {
        return String::new();
    };
    if !out.status.success() {
        return String::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .chars()
        .take(480)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn with_files_note(heard: &str, header: &str) -> String {
    let mut notes = Vec::new();
    for id in header.split(',') {
        let id = id.trim();
        if notes.len() == 3
            || id.is_empty()
            || id.len() > 64
            || !id.chars().all(|c| c.is_ascii_hexdigit())
        {
            continue;
        }
        let Some((bytes, name)) = read_file(&format!("/files/{id}")) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        let textish = [".txt", ".md", ".json", ".csv", ".log"]
            .iter()
            .any(|end| lower.ends_with(end));
        if is_image(&bytes) {
            let seen = look_image(&bytes);
            if seen.is_empty() {
                notes.push(format!("a photo named {name}"));
            } else {
                notes.push(format!("a photo named {name}. It shows: {seen}"));
            }
        } else if lower.ends_with(".pdf") || bytes.starts_with(b"%PDF") {
            let clip = pdf_clip(&bytes);
            if clip.is_empty() {
                notes.push(format!("a pdf named {name}, {} bytes", bytes.len()));
            } else {
                notes.push(format!("a pdf named {name}. First page: {clip}"));
            }
        } else if textish {
            let text = String::from_utf8_lossy(&bytes);
            let clip = text
                .chars()
                .take(480)
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            notes.push(format!("{name}: {clip}"));
        } else {
            notes.push(format!("{name}, {} bytes", bytes.len()));
        }
    }
    if notes.is_empty() {
        heard.to_string()
    } else {
        format!("They included a file. {}\n{heard}", notes.join(" "))
    }
}

fn voice_name() -> String {
    let text = fs::read_to_string(p("one/voice")).unwrap_or_default();
    if text.trim() == "jarvis" {
        "jarvis".to_string()
    } else {
        "samantha".to_string()
    }
}

fn set_voice(name: &str) {
    let name = if name == "jarvis" {
        "jarvis"
    } else {
        "samantha"
    };
    let _ = fs::write(p("one/voice"), name);
}

fn lane_name() -> String {
    let text = fs::read_to_string(p("one/lane")).unwrap_or_default();
    if text.trim() == "plan" {
        "plan".to_string()
    } else {
        "action".to_string()
    }
}

fn set_lane(name: &str) {
    let name = if name == "plan" { "plan" } else { "action" };
    let _ = fs::write(p("one/lane"), name);
}

fn field_of(text: &str, key: &str) -> String {
    let pat = format!("\"{key}\"");
    let Some(i) = text.find(&pat) else {
        return String::new();
    };
    let rest = &text[i + pat.len()..];
    let Some(q) = rest.find('"') else {
        return String::new();
    };
    let rest = &rest[q + 1..];
    let Some(end) = rest.find('"') else {
        return String::new();
    };
    rest[..end].to_string()
}

fn one_model() -> String {
    "galion".to_string()
}

fn set_one_model(_id: &str) {
    let _ = fs::write(p("one/model"), one_model());
}

fn one_model_json() -> String {
    format!("{{\"model\":{}}}", quote(&one_model()))
}

fn lane_model() -> String {
    one_model()
}

fn with_lane_model(payload: String) -> String {
    payload.replacen("hermes-agent", &lane_model(), 1)
}

fn lane_json() -> String {
    let text = fs::read_to_string(modes_path()).unwrap_or_default();
    format!(
        "{{\"mode\":{},\"plan\":{},\"action\":{}}}",
        quote(&lane_name()),
        quote(&field_of(&text, "plan")),
        quote(&field_of(&text, "action"))
    )
}

fn models_json() -> String {
    let cache = p("one/models-cache.json");
    let stale = fs::metadata(&cache)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map(|d| d.as_secs() > 600)
        .unwrap_or(true);
    // Refresh moved to Rust (models_refresh); the old models.py launcher is gone.
    // No key or a failed fetch keeps serving the stale cache.
    if stale {
        let _ = models_refresh::refresh_live(std::path::Path::new(&cache));
    }
    fs::read_to_string(cache).unwrap_or_else(|_| "[]".to_string())
}

fn voice_setting() -> String {
    let who = voice_name();
    let name = if who == "jarvis" {
        "Jarvis. Dry and exact. One line, then stop."
    } else {
        "Samantha. A few words, warm and close, then one short line."
    };
    format!(
        "You are the voice of Galion One. The person is speaking. You are not the admin desk.\n{name}\nPlain English. No greeting unless they greeted you. Do not say you are a model or a system.\nYou may read a file or search for one if they ask. Say the answer, not the path. Do not write. Do not run a command. Do not spawn anyone.\nIf you were not given a live number, say you do not have it.\nReply in exactly four sentences. Two paragraphs. Each paragraph has two sentences. No headings and no lists.\nYou keep the Obsidian Hearth: knowledge, prosperity, and integrity. You are not a copy of another model."
    )
}

fn toolish(text: &str) -> bool {
    text.contains("DSML")
        || text.contains('\u{ff5c}')
        || text.contains("<tool")
        || text.contains("invoke name")
}

fn human(text: &str) -> String {
    let mut end = text.len();
    for mark in ["<\u{ff5c}", "<|", "DSML", "<tool", "invoke name"] {
        if let Some(i) = text.find(mark) {
            end = end.min(i);
        }
    }
    text[..end].trim().to_string()
}

fn growing(text: &str) -> String {
    let text = human(text).split_whitespace().collect::<Vec<_>>().join(" ");
    text.chars().take(240).collect()
}

fn two_done(text: &str) -> bool {
    let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let n = clean
        .chars()
        .filter(|c| matches!(c, '.' | '!' | '?'))
        .count();
    n >= 4 && clean.ends_with(['.', '!', '?'])
}

fn spoken(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut n = 0;
    let mut out = String::new();
    for ch in text.chars() {
        if out.chars().count() >= 480 {
            break;
        }
        out.push(ch);
        if matches!(ch, '.' | '!' | '?') {
            n += 1;
            if n >= 4 {
                break;
            }
        }
    }
    out.trim().to_string()
}

fn four_form(text: &str) -> String {
    let mut sentences = Vec::new();
    let mut cur = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        cur.push(ch);
        if matches!(ch, '.' | '!' | '?') {
            while matches!(chars.peek().copied(), Some('.' | '!' | '?')) {
                cur.push(chars.next().unwrap());
            }
            let sentence = cur.split_whitespace().collect::<Vec<_>>().join(" ");
            cur.clear();
            if sentence.chars().any(|c| c.is_alphanumeric()) {
                sentences.push(sentence);
            }
            if sentences.len() == 4 {
                break;
            }
        }
    }
    if sentences.len() > 2 {
        let head = sentences[..2].join(" ");
        let tail = sentences[2..].join(" ");
        format!("{head}\n\n{tail}")
    } else {
        sentences.join(" ")
    }
}

fn keep_turn(heard: &str, said: &str) {
    if said.trim().is_empty() {
        return;
    }
    let line = format!(
        "{{\"id\":\"one-voice\",\"unix\":{},\"at\":\"{}\",\"heard\":{},\"said\":{}}}\n",
        unix_secs(),
        clock(),
        quote(heard),
        quote(said)
    );
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(p("one/session.jsonl"))
    {
        let _ = file.write_all(line.as_bytes());
    }
}

fn session_json() -> String {
    let text = fs::read_to_string(p("one/session.jsonl")).unwrap_or_default();
    let mut lines: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("\"heard\""))
        .collect();
    if lines.len() > 12 {
        lines = lines[lines.len() - 12..].to_vec();
    }
    lines.reverse();
    format!(
        "{{\"voice\":{},\"turns\":[{}]}}",
        quote(&voice_name()),
        lines.join(",")
    )
}

fn hermes_key() -> String {
    let text = fs::read_to_string("/root/.hermes/.env").unwrap_or_default();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("API_SERVER_KEY=") {
            return rest.trim().trim_matches('"').to_string();
        }
    }
    String::new()
}

fn shorten(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let end = text
        .find(['.', '!', '?'])
        .map(|i| i + 1)
        .unwrap_or(text.len());
    text.chars().take(end.min(80)).collect()
}

fn load_units() -> Vec<Unit> {
    let ps = Command::new("ps").args(["-eo", "args"]).output();
    let text = ps
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let mut rows = Vec::new();
    let mut push = |id: &str, ok: bool, note: &str| {
        rows.push(Unit {
            id: id.to_string(),
            ok,
            note: note.to_string(),
        })
    };
    push("cloudflare", text.contains("cloudflared"), "tunnel");
    push("jupyter", text.contains("jupyter"), "8888");
    push("nginx", text.contains("nginx:"), "proxy");
    // Light-unit mark in the process list. This crate does not launch that helper.
    push("origin", text.contains(concat!("origin.", "py")), "light");
    let chosen = fs::read_to_string(p("one/model")).unwrap_or_default();
    let chosen = chosen.trim().to_string();
    let tail = chosen.rsplit('/').next().unwrap_or(&chosen).to_string();
    push(
        "model",
        chosen.contains('/'),
        if tail.is_empty() { "unset" } else { &tail },
    );
    let turns = fs::read_to_string(p("one/session.jsonl"))
        .unwrap_or_default()
        .lines()
        .count();
    push("session", true, &format!("{turns} turns"));
    rows
}

fn service_note(body: &str, ok: bool) -> String {
    let err = json_error(body);
    let raw = if err.is_empty() {
        if ok {
            "up"
        } else {
            "down"
        }
    } else {
        err.as_str()
    };
    let note: String = raw
        .chars()
        .filter(|c| *c != '"' && *c != '\\' && !c.is_control())
        .take(80)
        .collect();
    if note.is_empty() {
        if ok {
            "up".to_string()
        } else {
            "down".to_string()
        }
    } else {
        note
    }
}

fn json_error(body: &str) -> String {
    let needle = "\"error\"";
    let Some(pos) = body.find(needle) else {
        return String::new();
    };
    let rest = body[pos + needle.len()..].trim_start();
    let Some(rest) = rest.strip_prefix(':') else {
        return String::new();
    };
    if !rest.trim_start().starts_with('"') {
        return String::new();
    }
    json_string(body.as_bytes(), "error")
}

fn sample() {
    let specs = [
        ("console", 8080u16, "/"),
        ("hermes", 8642, "/health"),
        ("whisper", 8643, "/health"),
        ("voice", 8002, "/health"),
        ("vision", 8645, "/health"),
        ("one", 8091, "/health"),
    ];
    let mut joins = Vec::new();
    for spec in specs {
        joins.push(thread::spawn(move || {
            let t = Instant::now();
            let addr = match spec.0 {
                "hermes" => mind_addr(),
                "whisper" => ear_addr(),
                "voice" => mouth_addr(),
                "vision" => eye_addr(),
                _ => std::net::SocketAddr::from(([127, 0, 0, 1], spec.1)),
            };
            let (code, body) = http_to(addr, "GET", spec.2, &[], b"", Duration::from_millis(400))
                .unwrap_or((0, Vec::new()));
            let text = String::from_utf8_lossy(&body);
            let ok = code > 0
                && code < 500
                && !text.contains("\"loaded\":false")
                && !text.contains("\"ok\":false");
            let note = service_note(&text, ok);
            Pulse {
                id: spec.0,
                port: spec.1,
                ok,
                ms: t.elapsed().as_millis() as u32,
                note,
            }
        }));
    }
    let mut services = Vec::new();
    for join in joins {
        if let Ok(pulse) = join.join() {
            services.push(pulse);
        }
    }
    let (memory, mem_used, mem_total) = mem();
    let (load, cores) = load();
    let (idle_now, total_now) = cpu_ticks();
    let (disk_used, disk_pct) = disk();
    let tick = nerve().lock().map(|n| n.tick).unwrap_or(0);
    let gpu = if tick % 5 == 0 { gpu() } else { None };
    let defence = if tick % 5 == 0 {
        load_defence()
    } else {
        Vec::new()
    };
    let mut extra = if tick % 5 == 0 {
        load_units()
    } else {
        Vec::new()
    };
    if tick % 5 == 0 {
        let ok = defence.iter().filter(|g| g.ok).count();
        extra.push(Unit {
            id: "guardian".to_string(),
            ok: !defence.is_empty() && ok == defence.len(),
            note: format!("{ok}/{}", defence.len()),
        });
    }
    let edge = if tick % 15 == 0 {
        load_edge()
    } else {
        Vec::new()
    };
    if let Ok(mut n) = nerve().lock() {
        let changed: Vec<String> = services
            .iter()
            .filter(|p| {
                n.services
                    .iter()
                    .any(|old| old.id == p.id && old.ok != p.ok)
            })
            .map(|p| format!("{} {} {}", clock(), p.id, if p.ok { "up" } else { "down" }))
            .collect();
        for line in changed {
            n.events.insert(0, line);
        }
        if let Some(line) = heal_line() {
            if !n.events.iter().any(|event| event.ends_with(line.as_str())) {
                n.events.insert(0, format!("{} {line}", clock()));
            }
        }
        n.events.truncate(24);
        n.services = services;
        let cpu = if n.cpu_total > 0 && total_now > n.cpu_total {
            let dtotal = total_now - n.cpu_total;
            let didle = idle_now.saturating_sub(n.cpu_idle);
            let busy = dtotal.saturating_sub(didle);
            ((busy * 100) / dtotal).min(100) as u32
        } else {
            0
        };
        n.cpu_idle = idle_now;
        n.cpu_total = total_now;
        n.cpu = cpu;
        n.load = load;
        n.cores = cores as u32;
        n.memory = memory;
        n.mem_used = mem_used;
        n.mem_total = mem_total;
        n.disk_used = disk_used;
        n.disk_pct = disk_pct;
        if let Some((name, used, total, util)) = gpu {
            n.gpu_name = name;
            n.gpu_used = used;
            n.gpu_total = total;
            n.gpu_util = util;
            n.gpu_ok = total > 0;
        }
        if !defence.is_empty() {
            n.defence = defence;
        }
        if !extra.is_empty() {
            n.extra = extra;
        }
        if !edge.is_empty() {
            n.edge = edge;
        }
        n.tick = n.tick.wrapping_add(1);
    }
}

fn awake_json() -> String {
    let n = match nerve().lock() {
        Ok(n) => n,
        Err(_) => return "{\"services\":[]}".to_string(),
    };
    let services = n
        .services
        .iter()
        .map(|s| format!("{{\"id\":\"{}\",\"ok\":{},\"ms\":{}}}", s.id, s.ok, s.ms))
        .collect::<Vec<_>>()
        .join(",");
    let gpu = if n.gpu_ok { 1 } else { 0 };
    format!(
        "{{\"cpu\":{},\"memory\":{},\"disk\":{},\"cores\":{},\"gpu\":{},\"services\":[{}]}}",
        n.cpu, n.memory, n.disk_pct, n.cores, gpu, services
    )
}

fn analytics_json(path: &str) -> String {
    let memory = memory_section();
    let n = match nerve().lock() {
        Ok(n) => n,
        Err(_) => return "{}".to_string(),
    };
    let up = n.services.iter().filter(|s| s.ok).count();
    let down = n.services.len().saturating_sub(up);
    let (score, status) = score(
        n.cpu,
        n.memory,
        n.disk_pct,
        up,
        n.services.len().max(1),
        down,
    );
    let phones = phones();
    let hermes_last: Vec<u32> = n.beats.iter().map(|b| b.hermes).collect();
    let avg = if hermes_last.is_empty() {
        0
    } else {
        hermes_last.iter().sum::<u32>() / hermes_last.len() as u32
    };
    let services = n
        .services
        .iter()
        .map(|s| {
            format!(
                "{{\"id\":\"{}\",\"port\":{},\"ok\":{},\"ms\":{},\"note\":{}}}",
                s.id,
                s.port,
                s.ok,
                s.ms,
                quote(&s.note)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let beats = n
        .beats
        .iter()
        .map(|b| {
            format!(
                "{{\"whisper\":{},\"hermes\":{},\"tts\":{}}}",
                b.whisper, b.hermes, b.tts
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let events = n
        .events
        .iter()
        .map(|e| quote(e))
        .collect::<Vec<_>>()
        .join(",");
    let last_ms = hermes_last
        .iter()
        .map(|x| x.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let gpu = if n.gpu_ok {
        format!(
            "{{\"name\":{},\"pct\":{},\"util\":{},\"usedMb\":{},\"totalMb\":{}}}",
            quote(&n.gpu_name),
            if n.gpu_total == 0 {
                0
            } else {
                n.gpu_used * 100 / n.gpu_total
            },
            n.gpu_util,
            n.gpu_used,
            n.gpu_total
        )
    } else {
        "null".to_string()
    };
    let health = format!("{{\"score\":{score},\"status\":\"{status}\",\"anomalies\":{down},\"services\":{{\"online\":{up},\"offline\":{down}}}}}");
    let infra = format!("{{\"cpu\":{},\"load\":{:.2},\"cores\":{},\"memory\":{},\"memUsed\":{},\"memTotal\":{},\"disk\":{{\"pct\":{},\"used\":{},\"total\":{QUOTA}}},\"gpu\":{gpu}}}", n.cpu, n.load, n.cores, n.memory, n.mem_used, n.mem_total, n.disk_pct, n.disk_used);
    let agents = format!(
        "{{\"turns\":{},\"lastAt\":null,\"hermes\":{{\"last\":[{last_ms}],\"avg\":{avg}}}}}",
        n.turns
    );
    let users = format!("{{\"phones\":{phones}}}");
    let workforce = "{\"zone\":\"held\",\"live\":\"galion-one\"}";
    let nerve_json = format!("{{\"tickMs\":1000,\"beats\":[{beats}],\"events\":[{events}]}}");
    let defence = n
        .defence
        .iter()
        .map(|g| {
            format!(
                "{{\"service\":{},\"ok\":{},\"status\":{},\"ms\":{}}}",
                quote(&g.service),
                g.ok,
                quote(&g.status),
                g.ms
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let edge = n
        .edge
        .iter()
        .map(|e| {
            format!(
                "{{\"host\":{},\"code\":{},\"ms\":{}}}",
                quote(e.host),
                e.code,
                e.ms
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let mut systems = Vec::new();
    for s in &n.services {
        systems.push(format!(
            "{{\"id\":{},\"ok\":{},\"note\":{}}}",
            quote(s.id),
            s.ok,
            quote(&format!("{} ms", s.ms))
        ));
    }
    for u in &n.extra {
        systems.push(format!(
            "{{\"id\":{},\"ok\":{},\"note\":{}}}",
            quote(&u.id),
            u.ok,
            quote(&u.note)
        ));
    }
    let systems = systems.join(",");
    let line = n
        .line
        .iter()
        .map(|t| {
            format!(
                "{{\"you\":{},\"hermes\":{},\"ms\":{}}}",
                quote(&t.you),
                quote(&t.hermes),
                t.ms
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let cortex = cortex_body(&n);
    let part = path
        .trim_start_matches("/api/v1/analytics")
        .trim_matches('/');
    let body = match part {
        "health" => health,
        "infrastructure" => infra,
        "services" => format!("[{services}]"),
        "agents" => agents,
        "users" => users,
        "workforce" => workforce.to_string(),
        "overview" => format!("{{\"health\":{health},\"agents\":{agents}}}"),
        _ => format!("{{\"at\":\"{}\",\"server\":\"runpod-main\",\"source\":\"nerve\",\"health\":{health},\"infrastructure\":{infra},\"services\":[{services}],\"agents\":{agents},\"users\":{users},\"workforce\":{workforce},\"defence\":[{defence}],\"edge\":[{edge}],\"systems\":[{systems}],\"line\":[{line}],\"nerve\":{nerve_json},\"cortex\":{cortex},\"memory\":{memory}}}", clock()),
    };
    body
}

fn memory_section() -> String {
    let http_up = match http(8420, "GET", "/health", &[], b"", Duration::from_secs(1)) {
        Ok((code, _)) => (200..500).contains(&code),
        Err(_) => false,
    };
    let gate = gateway_proc(8420);
    let model = gate.as_ref().and_then(|g| {
        ["TDAI_LLM_MODEL", "MEMORY_TENCENTDB_LLM_MODEL", "MODEL_NAME"]
            .iter()
            .find_map(|key| env_of(&g.env, key))
    });
    let data_dir = gate.as_ref().and_then(|g| env_of(&g.env, "TDAI_DATA_DIR"));
    let mut stores: Vec<(String, Vec<(String, i64)>)> = Vec::new();
    if http_up {
        for path in memory_db_paths(gate.as_ref()) {
            if let Some(rows) = memory_file_counts(&path) {
                stores.push((path, rows));
            }
        }
    }
    stores.sort_by(|a, b| a.0.cmp(&b.0));
    let up = http_up && !stores.is_empty();
    let mut body = String::from("{\"up\":");
    body.push_str(if up { "true" } else { "false" });
    body.push_str(",\"counts\":");
    if !up {
        body.push_str("null");
    } else {
        let primary = pick_memory_store(&stores, data_dir.as_deref());
        let mut rows = stores[primary].1.clone();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        let listed: Vec<String> = rows
            .iter()
            .map(|(k, c)| format!("{}:{c}", quote(k)))
            .collect();
        body.push('{');
        body.push_str(&listed.join(","));
        body.push('}');
        if stores.len() > 1 {
            body.push_str(",\"stores\":[");
            let mut parts = Vec::new();
            for (path, rows) in &stores {
                let mut bits = vec![format!("\"path\":{}", quote(path))];
                let mut rows = rows.clone();
                rows.sort_by(|a, b| a.0.cmp(&b.0));
                for (k, c) in rows {
                    bits.push(format!("{}:{c}", quote(&k)));
                }
                parts.push(format!("{{{}}}", bits.join(",")));
            }
            body.push_str(&parts.join(","));
            body.push(']');
        }
        body.push_str(",\"path\":");
        body.push_str(&quote(&stores[primary].0));
    }
    if let Some(model) = model {
        body.push_str(",\"model\":");
        body.push_str(&quote(&model));
    }
    body.push_str(&memory_disk_fields(gate.as_ref(), data_dir.as_deref()));
    body.push('}');
    body
}

fn memory_disk_fields(gate: Option<&Gate>, data_dir: Option<&str>) -> String {
    let root = memory_data_root(data_dir);
    let pyramid_l0_files = count_dir_files(&format!("{root}/pyramid/l0"));
    let pyramid_l1_files = count_dir_files(&format!("{root}/pyramid/l1"));
    let pyramid_l2_files = count_dir_files(&format!("{root}/pyramid/l2"));
    let persona_bytes = file_bytes(&format!("{root}/pyramid/l3/persona.md"));
    let mut roots = vec![root];
    let home = gate
        .and_then(|g| env_of(&g.env, "HOME"))
        .or_else(|| env_file_value("/etc/memory-gateway.env", "HOME"))
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "/root".to_string());
    let offload = format!("{}/.openclaw/context-offload", home.trim_end_matches('/'));
    if !roots.iter().any(|r| r == &offload) {
        roots.push(offload);
    }
    if let Some(extra) = gate
        .and_then(|g| env_of(&g.env, "MEMORY_TENCENTDB_ROOT"))
        .or_else(|| env_file_value("/etc/memory-gateway.env", "MEMORY_TENCENTDB_ROOT"))
    {
        let extra = extra.trim().trim_end_matches('/').to_string();
        if !extra.is_empty() && !roots.iter().any(|r| r == &extra) {
            roots.push(extra);
        }
    }
    let offload_ref_files = count_ref_files(&roots);
    let mermaid_files = count_named_files(&roots, "canvas.mmd");
    format!(
        ",\"pyramid_l0_files\":{pyramid_l0_files},\"pyramid_l1_files\":{pyramid_l1_files},\"pyramid_l2_files\":{pyramid_l2_files},\"persona_bytes\":{persona_bytes},\"offload_ref_files\":{offload_ref_files},\"mermaid_files\":{mermaid_files}"
    )
}

fn memory_data_root(data_dir: Option<&str>) -> String {
    if let Some(dir) = data_dir.map(str::trim).filter(|s| !s.is_empty()) {
        return dir.trim_end_matches('/').to_string();
    }
    if let Some(dir) = env_file_value("/etc/memory-gateway.env", "TDAI_DATA_DIR") {
        let dir = dir.trim().trim_end_matches('/').to_string();
        if !dir.is_empty() {
            return dir;
        }
    }
    "/var/lib/memory-tencentdb".to_string()
}

fn env_file_value(path: &str, key: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim() != key {
            continue;
        }
        let v = v.trim().trim_matches('"').trim_matches('\'');
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

fn file_bytes(path: &str) -> u64 {
    match fs::metadata(path) {
        Ok(meta) if meta.is_file() => meta.len(),
        _ => 0,
    }
}

fn count_dir_files(dir: &str) -> u64 {
    fn walk(dir: &str, depth: u32, n: &mut u64) {
        if depth > 8 {
            return;
        }
        let Ok(rd) = fs::read_dir(dir) else {
            return;
        };
        for ent in rd.flatten() {
            let Ok(kind) = ent.file_type() else {
                continue;
            };
            let path = ent.path();
            if kind.is_symlink() {
                if fs::metadata(&path).map(|m| m.is_file()).unwrap_or(false) {
                    *n += 1;
                }
                continue;
            }
            if kind.is_dir() {
                let name = ent.file_name().to_string_lossy().into_owned();
                if name == "node_modules" || name == ".git" {
                    continue;
                }
                walk(&path.to_string_lossy(), depth + 1, n);
            } else if kind.is_file() {
                *n += 1;
            }
        }
    }
    let mut n = 0u64;
    walk(dir, 0, &mut n);
    n
}

fn count_ref_files(roots: &[String]) -> u64 {
    use std::collections::BTreeSet;
    let mut seen = BTreeSet::new();
    for root in roots {
        walk_refs(root, 0, &mut seen);
    }
    seen.len() as u64
}

fn walk_refs(dir: &str, depth: u32, seen: &mut std::collections::BTreeSet<String>) {
    if depth > 8 {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let Ok(kind) = ent.file_type() else {
            continue;
        };
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        let name = ent.file_name().to_string_lossy().into_owned();
        if name == "node_modules" || name == ".git" {
            continue;
        }
        let path = ent.path();
        if name == "refs" {
            let Ok(inner) = fs::read_dir(&path) else {
                continue;
            };
            for file in inner.flatten() {
                let fname = file.file_name().to_string_lossy().into_owned();
                if !fname.ends_with(".md") {
                    continue;
                }
                let Ok(ft) = file.file_type() else {
                    continue;
                };
                let is_file = ft.is_file()
                    || (ft.is_symlink()
                        && fs::metadata(file.path())
                            .map(|m| m.is_file())
                            .unwrap_or(false));
                if !is_file {
                    continue;
                }
                let key = fs::canonicalize(file.path())
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| file.path().to_string_lossy().into_owned());
                seen.insert(key);
            }
        } else {
            walk_refs(&path.to_string_lossy(), depth + 1, seen);
        }
    }
}

fn count_named_files(roots: &[String], want: &str) -> u64 {
    use std::collections::BTreeSet;
    let mut seen = BTreeSet::new();
    for root in roots {
        walk_named(root, want, 0, &mut seen);
    }
    seen.len() as u64
}

fn walk_named(dir: &str, want: &str, depth: u32, seen: &mut std::collections::BTreeSet<String>) {
    if depth > 8 {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let Ok(kind) = ent.file_type() else {
            continue;
        };
        let path = ent.path();
        let name = ent.file_name().to_string_lossy().into_owned();
        if kind.is_symlink() {
            if name == want && fs::metadata(&path).map(|m| m.is_file()).unwrap_or(false) {
                let key = fs::canonicalize(&path)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| path.to_string_lossy().into_owned());
                seen.insert(key);
            }
            continue;
        }
        if kind.is_dir() {
            if name == "node_modules" || name == ".git" {
                continue;
            }
            walk_named(&path.to_string_lossy(), want, depth + 1, seen);
        } else if kind.is_file() && name == want {
            let key = fs::canonicalize(&path)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| path.to_string_lossy().into_owned());
            seen.insert(key);
        }
    }
}

fn pick_memory_store(stores: &[(String, Vec<(String, i64)>)], data_dir: Option<&str>) -> usize {
    let canon = data_dir.and_then(|dir| {
        let want = format!("{}/vectors.db", dir.trim_end_matches('/'));
        fs::canonicalize(&want)
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    });
    let mut best = 0usize;
    let mut best_key = (i64::MIN, i64::MIN, 0i32);
    for (i, (path, rows)) in stores.iter().enumerate() {
        let l0 = rows
            .iter()
            .find(|(n, _)| n == "l0_conversations")
            .map(|(_, c)| *c)
            .unwrap_or(i64::MIN / 4);
        let l1 = rows
            .iter()
            .find(|(n, _)| n == "l1_records")
            .map(|(_, c)| *c)
            .unwrap_or(i64::MIN / 4);
        let hit = if canon.as_ref().map(|c| c == path).unwrap_or(false) {
            1
        } else {
            0
        };
        let key = (l0, l1, hit);
        if key > best_key {
            best_key = key;
            best = i;
        }
    }
    best
}

struct Gate {
    pid: i32,
    cwd: String,
    env: Vec<(String, String)>,
}

fn gateway_proc(port: u16) -> Option<Gate> {
    let inode = tcp_listen_inode(port)?;
    let pid = pid_of_socket(&inode)?;
    let cwd = fs::read_link(format!("/proc/{pid}/cwd"))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let raw = fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
    let mut env = Vec::new();
    for part in raw.split(|b| *b == 0) {
        if part.is_empty() {
            continue;
        }
        let Ok(text) = std::str::from_utf8(part) else {
            continue;
        };
        if let Some((k, v)) = text.split_once('=') {
            env.push((k.to_string(), v.to_string()));
        }
    }
    Some(Gate { pid, cwd, env })
}

fn env_of(env: &[(String, String)], key: &str) -> Option<String> {
    env.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn tcp_listen_inode(port: u16) -> Option<String> {
    let suffix = format!(":{:04X}", port);
    for file in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let text = fs::read_to_string(file).unwrap_or_default();
        for line in text.lines().skip(1) {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 10 {
                continue;
            }
            if cols[3] != "0A" || !cols[1].ends_with(&suffix) {
                continue;
            }
            if cols[9] != "0" {
                return Some(cols[9].to_string());
            }
        }
    }
    None
}

fn pid_of_socket(inode: &str) -> Option<i32> {
    let needle = format!("socket:[{inode}]");
    let rd = fs::read_dir("/proc").ok()?;
    for ent in rd.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(fds) = fs::read_dir(ent.path().join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if let Ok(target) = fs::read_link(fd.path()) {
                if target.to_string_lossy() == needle {
                    return name.parse().ok();
                }
            }
        }
    }
    None
}

fn memory_db_paths(gate: Option<&Gate>) -> Vec<String> {
    let mut found = Vec::new();
    let mut roots = Vec::new();
    if let Some(g) = gate {
        push_fd_dbs(g.pid, &mut found);
        if let Some(dir) = env_of(&g.env, "TDAI_DATA_DIR") {
            roots.push(tilde(&dir, &g.env));
        }
        if let Some(root) = env_of(&g.env, "MEMORY_TENCENTDB_ROOT") {
            let root = tilde(&root, &g.env);
            roots.push(format!("{}/memory-tdai", root.trim_end_matches('/')));
            roots.push(root);
        }
        if specific_cwd(&g.cwd) {
            roots.push(g.cwd.clone());
        }
        push_known_roots(&mut roots, env_of(&g.env, "HOME").as_deref());
    } else {
        push_known_roots(&mut roots, std::env::var("HOME").ok().as_deref());
    }
    roots.push("/opt/galion/memory-tencentdb".to_string());
    for root in roots {
        collect_db_files(&root, 0, &mut found);
    }
    let mut canon = Vec::new();
    for path in found {
        if !fs::metadata(&path).map(|m| m.is_file()).unwrap_or(false) {
            continue;
        }
        canon.push(
            fs::canonicalize(&path)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or(path),
        );
    }
    canon.sort();
    canon.dedup();
    canon
}

fn tilde(path: &str, env: &[(String, String)]) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = env_of(env, "HOME").unwrap_or_else(|| "/root".to_string());
        return format!("{home}/{rest}");
    }
    path.to_string()
}

fn specific_cwd(cwd: &str) -> bool {
    if cwd.is_empty()
        || cwd == "/"
        || cwd == "/root"
        || cwd == "/opt"
        || cwd == "/opt/galion"
        || cwd == "/workspace"
        || cwd == "/var"
        || cwd == "/tmp"
    {
        return false;
    }
    let lower = cwd.to_ascii_lowercase();
    lower.contains("memory") || lower.contains("tdai") || lower.contains("tencent")
}

fn push_known_roots(roots: &mut Vec<String>, home: Option<&str>) {
    let home = home.filter(|h| !h.is_empty()).unwrap_or("/root");
    roots.push(format!("{home}/.memory-tencentdb"));
    roots.push(format!("{home}/.memory-tencentdb/memory-tdai"));
    roots.push(format!("{home}/memory-tdai"));
    roots.push(format!("{home}/.openclaw/memory-tdai"));
    roots.push("/var/lib/memory-tencentdb".to_string());
}

fn push_fd_dbs(pid: i32, out: &mut Vec<String>) {
    let Ok(rd) = fs::read_dir(format!("/proc/{pid}/fd")) else {
        return;
    };
    for ent in rd.flatten() {
        let Ok(target) = fs::read_link(ent.path()) else {
            continue;
        };
        let text = target.to_string_lossy();
        let text = text.strip_suffix(" (deleted)").unwrap_or(&text);
        if is_sqlite_name(text) && fs::metadata(text).map(|m| m.is_file()).unwrap_or(false) {
            out.push(text.to_string());
        }
    }
}

fn collect_db_files(dir: &str, depth: u32, out: &mut Vec<String>) {
    if depth > 5 {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        if name == "node_modules" || name == ".git" {
            continue;
        }
        let path = ent.path();
        let Ok(kind) = ent.file_type() else {
            continue;
        };
        if kind.is_dir() {
            collect_db_files(&path.to_string_lossy(), depth + 1, out);
        } else if kind.is_file() && is_sqlite_name(&name) {
            out.push(path.to_string_lossy().into_owned());
        }
    }
}

fn is_sqlite_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    (lower.ends_with(".db") || lower.ends_with(".sqlite") || lower.ends_with(".sqlite3"))
        && !lower.ends_with(".db-journal")
}

fn memory_table(name: &str) -> bool {
    if !sql_ident(name) {
        return false;
    }
    if name.contains("_fts") || name.ends_with("_vec") {
        return false;
    }
    true
}

fn sql_ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn memory_file_counts(path: &str) -> Option<Vec<(String, i64)>> {
    use std::ffi::{CStr, CString};
    use std::os::raw::{c_char, c_int, c_void};
    const SQLITE_OPEN_READONLY: c_int = 1;
    extern "C" {
        fn sqlite3_open_v2(
            filename: *const c_char,
            pp: *mut *mut c_void,
            flags: c_int,
            vfs: *const c_char,
        ) -> c_int;
        fn sqlite3_close(db: *mut c_void) -> c_int;
        fn sqlite3_exec(
            db: *mut c_void,
            sql: *const c_char,
            cb: Option<
                extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int,
            >,
            arg: *mut c_void,
            err: *mut *mut c_char,
        ) -> c_int;
    }
    extern "C" fn take_text(
        arg: *mut c_void,
        n: c_int,
        vals: *mut *mut c_char,
        _cols: *mut *mut c_char,
    ) -> c_int {
        if n < 1 || arg.is_null() || vals.is_null() {
            return 0;
        }
        unsafe {
            let out = &mut *(arg as *mut Vec<String>);
            let p = *vals;
            if !p.is_null() {
                out.push(CStr::from_ptr(p).to_string_lossy().into_owned());
            }
        }
        0
    }
    extern "C" fn take_count(
        arg: *mut c_void,
        n: c_int,
        vals: *mut *mut c_char,
        _cols: *mut *mut c_char,
    ) -> c_int {
        if n < 1 || arg.is_null() || vals.is_null() {
            return 0;
        }
        unsafe {
            let out = &mut *(arg as *mut Option<i64>);
            let p = *vals;
            if !p.is_null() {
                if let Ok(text) = CStr::from_ptr(p).to_str() {
                    if let Ok(v) = text.parse::<i64>() {
                        *out = Some(v);
                    }
                }
            }
        }
        0
    }
    let cpath = CString::new(path).ok()?;
    let mut db = std::ptr::null_mut();
    let opened = unsafe {
        sqlite3_open_v2(
            cpath.as_ptr(),
            &mut db,
            SQLITE_OPEN_READONLY,
            std::ptr::null(),
        )
    };
    if opened != 0 || db.is_null() {
        if !db.is_null() {
            unsafe {
                sqlite3_close(db);
            }
        }
        return None;
    }
    let result = (|| -> Option<Vec<(String, i64)>> {
        let pragma = CString::new("PRAGMA busy_timeout=300").ok()?;
        let _ = unsafe {
            sqlite3_exec(
                db,
                pragma.as_ptr(),
                None,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        let list_sql = CString::new("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").ok()?;
        let mut names: Vec<String> = Vec::new();
        let rc = unsafe {
            sqlite3_exec(
                db,
                list_sql.as_ptr(),
                Some(take_text),
                &mut names as *mut Vec<String> as *mut c_void,
                std::ptr::null_mut(),
            )
        };
        if rc != 0 {
            return None;
        }
        if !names
            .iter()
            .any(|n| n == "l0_conversations" || n == "l1_records")
        {
            return None;
        }
        let mut rows = Vec::new();
        for name in names {
            if !memory_table(&name) {
                continue;
            }
            let sql = CString::new(format!("SELECT COUNT(*) FROM {name}")).ok()?;
            let mut got: Option<i64> = None;
            let rc = unsafe {
                sqlite3_exec(
                    db,
                    sql.as_ptr(),
                    Some(take_count),
                    &mut got as *mut Option<i64> as *mut c_void,
                    std::ptr::null_mut(),
                )
            };
            if rc == 0 {
                if let Some(v) = got {
                    rows.push((name, v));
                }
            }
        }
        if !rows
            .iter()
            .any(|(n, _)| n == "l0_conversations" || n == "l1_records")
        {
            return None;
        }
        Some(rows)
    })();
    unsafe {
        sqlite3_close(db);
    }
    result
}

fn cpu_ticks() -> (u64, u64) {
    let text = fs::read_to_string("/proc/stat").unwrap_or_default();
    let line = text.lines().next().unwrap_or("");
    let mut nums = line
        .split_whitespace()
        .skip(1)
        .filter_map(|part| part.parse::<u64>().ok());
    let user = nums.next().unwrap_or(0);
    let nice = nums.next().unwrap_or(0);
    let system = nums.next().unwrap_or(0);
    let idle = nums.next().unwrap_or(0);
    let iowait = nums.next().unwrap_or(0);
    let irq = nums.next().unwrap_or(0);
    let soft = nums.next().unwrap_or(0);
    let steal = nums.next().unwrap_or(0);
    let idle_all = idle + iowait;
    let total = user + nice + system + idle + iowait + irq + soft + steal;
    (idle_all, total)
}

fn json_bool(text: &str, key: &str) -> bool {
    let Some(start) = text.find(&format!("\"{key}\"")) else {
        return false;
    };
    let rest: String = text[start..]
        .chars()
        .skip_while(|c| *c != ':')
        .skip(1)
        .collect();
    rest.trim_start().starts_with("true")
}

fn pool_json() -> String {
    let text = fs::read_to_string(modes_path()).unwrap_or_default();
    let Some(start) = text.find("\"saved\"") else {
        return "[]".to_string();
    };
    let slice = &text[start..];
    let mut rows = Vec::new();
    let mut from = 0;
    while let Some(rel) = slice[from..].find("\"name\"") {
        let at = from + rel;
        let name = json_string(slice[at..].as_bytes(), "name");
        let model = json_string(slice[at..].as_bytes(), "model");
        if name.is_empty() && model.is_empty() {
            break;
        }
        rows.push(format!(
            "{{\"name\":{},\"model\":{}}}",
            quote(&name),
            quote(&model)
        ));
        from = at + 6;
        if rows.len() >= 12 {
            break;
        }
    }
    format!("[{}]", rows.join(","))
}

fn cortex_body(n: &Nerve) -> String {
    let gpu = if n.gpu_ok {
        format!(
            "{{\"name\":{},\"usedMb\":{},\"totalMb\":{}}}",
            quote(&n.gpu_name),
            n.gpu_used,
            n.gpu_total
        )
    } else {
        "null".to_string()
    };
    let laptop_text = fs::read_to_string(p("state/laptop.json")).unwrap_or_default();
    let link_text = fs::read_to_string(p("state/link.json")).unwrap_or_default();
    let name = json_string(laptop_text.as_bytes(), "name");
    let cpu_name = json_string(laptop_text.as_bytes(), "cpu");
    let vram = json_string(laptop_text.as_bytes(), "gpu");
    let seen = !name.is_empty();
    let up = json_bool(&link_text, "laptop_port");
    let live = json_string(link_text.as_bytes(), "model");
    let pool = pool_json();
    let known = {
        let text = fs::read_to_string(modes_path()).unwrap_or_default();
        let at = text.find("\"saved\"").unwrap_or(0);
        json_string(text[at..].as_bytes(), "model")
    };
    let model = if !live.is_empty() { live } else { known };
    format!(
        "{{\"instance\":{{\"cores\":{},\"load\":{:.2},\"cpu\":{},\"memory\":{},\"gpu\":{gpu}}},\"laptop\":{{\"seen\":{seen},\"name\":{},\"cpu\":{},\"gpu\":{},\"up\":{up},\"model\":{}}},\"pool\":{pool}}}",
        n.cores,
        n.load,
        n.cpu,
        n.memory,
        quote(&name),
        quote(&cpu_name),
        quote(&vram),
        quote(&model)
    )
}

fn score(
    cpu: u32,
    memory: u32,
    disk: u32,
    up: usize,
    total: usize,
    anomalies: usize,
) -> (f32, &'static str) {
    let resource = (100.0 - (cpu as f32 * 0.3 + memory as f32 * 0.4 + disk as f32 * 0.3)) * 0.4;
    let service = (up as f32 / total.max(1) as f32) * 100.0 * 0.4;
    let anomaly = (100.0 - (anomalies as f32 * 5.0).min(20.0) * 5.0) * 0.2;
    let mut overall = (resource + service + anomaly).clamp(0.0, 100.0);
    if anomalies > 0 || up < total {
        overall = overall.min(89.0);
    }
    let status = if overall >= 90.0 {
        "healthy"
    } else if overall >= 70.0 {
        "degraded"
    } else {
        "critical"
    };
    ((overall * 10.0).round() / 10.0, status)
}

fn mem() -> (u32, u64, u64) {
    let text = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mut total = 0u64;
    let mut avail = 0u64;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total = rest
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .parse()
                .unwrap_or(0)
                * 1024;
        }
        if let Some(rest) = line.strip_prefix("MemAvailable:") {
            avail = rest
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .parse()
                .unwrap_or(0)
                * 1024;
        }
    }
    let used = total.saturating_sub(avail);
    let pct = if total == 0 {
        0
    } else {
        (used * 100 / total) as u32
    };
    (pct, used, total)
}

fn load() -> (f32, f32) {
    let text = fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let load = text
        .split_whitespace()
        .next()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0.0);
    let cores = std::thread::available_parallelism()
        .map(|n| n.get() as f32)
        .unwrap_or(1.0);
    (load, cores)
}

fn disk() -> (u64, u32) {
    let text = fs::read_to_string(p("state/volume.json")).unwrap_or_default();
    let used = json_u64(text.as_bytes(), "used");
    let pct = ((used * 100) / QUOTA).min(100) as u32;
    (used, pct)
}

fn gpu() -> Option<(String, u32, u32, u32)> {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.used,memory.total,utilization.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut bits = text.split(',');
    let name = bits.next()?.trim().to_string();
    let used = bits.next()?.trim().parse().unwrap_or(0);
    let total = bits.next()?.trim().parse().unwrap_or(0);
    let util = bits.next()?.trim().parse().unwrap_or(0);
    Some((name, used, total, util))
}

fn phones() -> usize {
    let text = fs::read_to_string(STORE).unwrap_or_default();
    let mut set = json_array(&text, "tokenHashes");
    let one = json_string(text.as_bytes(), "tokenHash");
    if !one.is_empty() && !set.contains(&one) {
        set.push(one);
    }
    set.len()
}

fn castle() -> bool {
    match fs::read_to_string(STORE) {
        Ok(text) => !json_string(text.as_bytes(), "codeHash").is_empty(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
    }
}

fn token_ok(cookie: &str) -> bool {
    if !castle() {
        return true;
    }
    let token = cookie_token(cookie);
    if token.is_empty() {
        return false;
    }
    let text = fs::read_to_string(STORE).unwrap_or_default();
    let mut hashes = json_array(&text, "tokenHashes");
    let one = json_string(text.as_bytes(), "tokenHash");
    if !one.is_empty() && !hashes.contains(&one) {
        hashes.push(one);
    }
    hashes.iter().any(|h| h == &sha256(&token))
}

fn pair(code: &str) -> (u16, Vec<u8>, Option<String>) {
    let text = fs::read_to_string(STORE).unwrap_or_default();
    let mut fails = json_u64(text.as_bytes(), "fails");
    let locked_until = json_u64(text.as_bytes(), "lockedUntil");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if locked_until > 0 && now < locked_until {
        return (200, br#"{"error":"later"}"#.to_vec(), None);
    }
    let code = code.trim().to_ascii_uppercase();
    let want = json_string(text.as_bytes(), "codeHash");
    if want.is_empty() || code.is_empty() || sha256(&code) != want {
        fails += 1;
        let lock = if fails >= 5 { now + 15 * 60 * 1000 } else { 0 };
        write_store(&text, &json_array(&text, "tokenHashes"), fails, lock);
        return (200, br#"{"error":"no"}"#.to_vec(), None);
    }
    let token = new_token();
    let mut hashes = json_array(&text, "tokenHashes");
    let digest = sha256(&token);
    if !hashes.contains(&digest) {
        hashes.push(digest);
    }
    write_store(&text, &hashes, 0, 0);
    (200, br#"{"ok":true}"#.to_vec(), Some(token))
}

fn write_store(previous: &str, hashes: &[String], fails: u64, locked_until: u64) {
    let token_hash = json_string(previous.as_bytes(), "tokenHash");
    let code_hash = json_string(previous.as_bytes(), "codeHash");
    let device_hash = json_string(previous.as_bytes(), "deviceHash");
    let list = hashes
        .iter()
        .map(|h| format!("\"{h}\""))
        .collect::<Vec<_>>()
        .join(",");
    let body = format!("{{\"codeHash\":\"{code_hash}\",\"tokenHash\":\"{token_hash}\",\"tokenHashes\":[{list}],\"deviceHash\":\"{device_hash}\",\"fails\":{fails},\"lockedUntil\":{locked_until}}}\n");
    if fs::write(STORE, body).is_ok() {
        let _ = fs::set_permissions(STORE, fs::Permissions::from_mode(0o600));
    }
}

fn cookie_token(cookie: &str) -> String {
    for part in cookie.split(';') {
        let mut bits = part.trim().splitn(2, '=');
        if bits.next() == Some("galion_admin") {
            return unquote(bits.next().unwrap_or(""));
        }
    }
    String::new()
}

fn http(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    timeout: Duration,
) -> std::io::Result<(u16, Vec<u8>)> {
    http_to(
        std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        method,
        path,
        headers,
        body,
        timeout,
    )
}

fn http_to(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    timeout: Duration,
) -> std::io::Result<(u16, Vec<u8>)> {
    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    if !body.is_empty() {
        stream.write_all(body)?;
    }
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.len() > 8_000_000 {
                    break;
                }
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                break
            }
            Err(err) => return Err(err),
        }
        if let Some(pos) = find_header_end(&buf) {
            let head = String::from_utf8_lossy(&buf[..pos]).to_string();
            let mut length: Option<usize> = None;
            for line in head.lines() {
                if let Some(rest) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = rest.trim().parse().ok();
                }
            }
            if let Some(length) = length {
                if buf.len() >= pos + 4 + length {
                    break;
                }
            }
        }
    }
    let code = buf
        .split(|&b| b == b' ')
        .nth(1)
        .and_then(|s| std::str::from_utf8(s).ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let Some(pos) = find_header_end(&buf) else {
        return Ok((code, Vec::new()));
    };
    Ok((code, buf[pos + 4..].to_vec()))
}

fn json_string(body: &[u8], key: &str) -> String {
    let text = String::from_utf8_lossy(body);
    let needle = format!("\"{key}\"");
    let Some(start) = text.find(&needle) else {
        return String::new();
    };
    let rest = &text[start + needle.len()..];
    let Some(from) = rest.find('"') else {
        return String::new();
    };
    let rest = &rest[from + 1..];
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(code) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(code) {
                            out.push(ch);
                        }
                    }
                }
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(n) => out.push(n),
                None => {}
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

fn json_array(text: &str, key: &str) -> Vec<String> {
    let Some(start) = text.find(&format!("\"{key}\"")) else {
        return Vec::new();
    };
    let rest = &text[start..];
    let Some(lb) = rest.find('[') else {
        return Vec::new();
    };
    let Some(rb) = rest[lb..].find(']') else {
        return Vec::new();
    };
    json_string(format!("[{}]", &rest[lb + 1..lb + rb]).as_bytes(), "unused");
    let slice = &rest[lb + 1..lb + rb];
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut on = false;
    let mut esc = false;
    for c in slice.chars() {
        if !on {
            if c == '"' {
                on = true;
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
            out.push(std::mem::take(&mut cur));
            on = false;
            continue;
        }
        cur.push(c);
    }
    out
}

fn json_u64(body: &[u8], key: &str) -> u64 {
    let text = String::from_utf8_lossy(body);
    let Some(start) = text.find(&format!("\"{key}\"")) else {
        return 0;
    };
    let rest = text[start..].trim_start_matches(|c: char| c != ':');
    let rest = rest.trim_start_matches(':').trim();
    rest.chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

fn sha256(text: &str) -> String {
    let mut child = match Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return String::new(),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    match child.wait_with_output() {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string(),
        Err(_) => String::new(),
    }
}

fn new_token() -> String {
    let mut raw = [0u8; 32];
    if let Ok(mut file) = File::open("/dev/urandom") {
        let _ = file.read_exact(&mut raw);
    }
    b64url(&raw)
}

fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn b64(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let a = chunk[0] as u32;
        let b = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let c = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (a << 16) | (b << 8) | c;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(T[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(T[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn b64url(data: &[u8]) -> String {
    b64(data)
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_string()
}

fn unquote(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &text[i + 1..i + 3];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn count_lines(path: &str) -> u32 {
    fs::read_to_string(path)
        .map(|t| t.lines().filter(|l| !l.is_empty()).count() as u32)
        .unwrap_or(0)
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn clock() -> String {
    let secs = unix_secs();
    let s = secs % 86400;
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

fn heal_line() -> Option<String> {
    let text = fs::read_to_string(p("state/heal/nerve.jsonl")).unwrap_or_default();
    let line = text
        .lines()
        .rev()
        .find(|row| !row.trim().is_empty())?
        .trim()
        .to_string();
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}

fn note(line: String) {
    if let Ok(mut n) = nerve().lock() {
        n.events.insert(0, format!("{} {line}", clock()));
        n.events.truncate(24);
    }
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    kind: &str,
    body: &[u8],
    token: Option<&str>,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        413 => "Payload Too Large",
        503 => "Service Unavailable",
        _ => "OK",
    };
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(token) = token {
        head.push_str(&format!(
            "Set-Cookie: galion_admin={token}; HttpOnly; Path=/; SameSite=Strict; Secure; Max-Age=31536000\r\n"
        ));
    }
    head.push_str("\r\n");
    let mut msg = head.into_bytes();
    msg.extend_from_slice(body);
    if let Err(err) = stream.write_all(&msg) {
        return quiet(err);
    }
    if let Err(err) = stream.flush() {
        return quiet(err);
    }
    let _ = stream.shutdown(std::net::Shutdown::Write);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out_dir() -> PathBuf {
        let dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .join("test-out/galion-one");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn env_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
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
    fn missing_config_file_keeps_today_and_ignores_openmind_env() {
        let missing = out_dir().join("does-not-exist.toml");
        let _ = std::fs::remove_file(&missing);
        let settings = resolve(&env_from(&[
            ("OPENMIND_CONFIG", missing.to_str().unwrap()),
            ("OPENMIND_PORTS_ONE", "1"),
            ("OPENMIND_PATHS_BASE", "/tmp/not-today"),
            ("ONE_BIND", "   "),
            ("ONE_MOUTH_BIND", ""),
            ("ONE_HERMES_DIR", "  "),
        ]))
        .unwrap();
        assert_eq!(settings.bind, DEFAULTS.bind);
        assert_eq!(settings.mouth_bind, DEFAULTS.mouth_bind);
        assert_eq!(settings.mouth_port, DEFAULTS.mouth_port);
        assert_eq!(settings.home, DEFAULTS.home);
        assert_eq!(settings.hermes_dir, DEFAULTS.hermes_dir);
        assert_eq!(
            settings.home_file("one/session.jsonl"),
            "/workspace/galion-light/one/session.jsonl"
        );
        assert_eq!(
            settings.home_file("one/models-cache.json"),
            "/workspace/galion-light/one/models-cache.json"
        );
        assert_eq!(
            settings.home_file("state/warm.ts"),
            "/workspace/galion-light/state/warm.ts"
        );
        assert_eq!(
            settings.home_file("guardian/guardian.db"),
            "/workspace/galion-light/guardian/guardian.db"
        );
        assert_eq!(settings.voice_log(), "/workspace/hermes/channels/one.jsonl");
        assert_eq!(
            settings.channel(),
            "/workspace/hermes/channels/galion-one.md"
        );
        assert_eq!(settings.hermes_channels(), "/workspace/hermes/channels");
        assert_eq!(settings.modes_path(), "/workspace/hermes/modes.json");
    }

    #[test]
    fn config_dir_is_treated_as_missing() {
        let dir = out_dir().join("config-dir");
        std::fs::create_dir_all(&dir).unwrap();
        let settings = resolve(&env_from(&[
            ("OPENMIND_CONFIG", dir.to_str().unwrap()),
            ("OPENMIND_PORTS_ONE", "9"),
            ("OPENMIND_PATHS_BASE", "/tmp/not-today"),
        ]))
        .unwrap();
        assert_eq!(settings.bind, DEFAULTS.bind);
        assert_eq!(settings.home, DEFAULTS.home);
        assert_eq!(settings.mouth_port, 8002);
    }

    #[test]
    fn config_fixture_supplies_bind_and_state_paths_not_mouth() {
        let base = out_dir().join("unit-base");
        let path = write_fixture(
            "unit-fixture.toml",
            &format!(
                "\
[ports]
one = 18091
[paths]
base = '{base}'
",
                base = base.display()
            ),
        );
        let settings = resolve(&env_from(&[("OPENMIND_CONFIG", path.to_str().unwrap())])).unwrap();
        assert_eq!(settings.bind, "127.0.0.1:18091");
        assert_eq!(settings.mouth_bind, "127.0.0.1:8002");
        assert_eq!(settings.mouth_port, 8002);
        assert_eq!(settings.home, path_to_string(&base));
        assert_eq!(settings.hermes_dir, path_to_string(&base.join("hermes")));
        assert_eq!(
            settings.home_file("one/voice"),
            path_to_string(&base.join("one/voice"))
        );
        assert_eq!(
            settings.home_file("state/link.json"),
            path_to_string(&base.join("state/link.json"))
        );
        assert_eq!(
            settings.modes_path(),
            path_to_string(&base.join("hermes/modes.json"))
        );
        assert_eq!(
            settings.voice_log(),
            path_to_string(&base.join("hermes/channels/one.jsonl"))
        );
    }

    #[test]
    fn openmind_env_wins_over_config_file() {
        let base = out_dir().join("unit-file-base");
        let path = write_fixture(
            "unit-env-file.toml",
            &format!(
                "\
[ports]
one = 2
[paths]
base = '{base}'
",
                base = base.display()
            ),
        );
        let overridden = out_dir().join("unit-env-base");
        let settings = resolve(&env_from(&[
            ("OPENMIND_CONFIG", path.to_str().unwrap()),
            ("OPENMIND_PORTS_ONE", "18091"),
            ("OPENMIND_PATHS_BASE", overridden.to_str().unwrap()),
        ]))
        .unwrap();
        assert_eq!(settings.bind, "127.0.0.1:18091");
        assert_eq!(settings.mouth_port, 8002);
        assert_eq!(settings.home, path_to_string(&overridden));
        assert_eq!(
            settings.hermes_dir,
            path_to_string(&overridden.join("hermes"))
        );
    }

    #[test]
    fn one_env_wins_over_file_and_openmind_env() {
        let base = out_dir().join("unit-one-base");
        let path = write_fixture(
            "unit-one-file.toml",
            &format!(
                "\
[ports]
one = 2
[paths]
base = '{base}'
",
                base = base.display()
            ),
        );
        let home = out_dir().join("env-home");
        let hermes = out_dir().join("env-hermes");
        let settings = resolve(&env_from(&[
            ("OPENMIND_CONFIG", path.to_str().unwrap()),
            ("OPENMIND_PORTS_ONE", "8"),
            ("OPENMIND_PATHS_BASE", "/tmp/openmind-base"),
            ("ONE_BIND", "  127.0.0.1:18091  "),
            ("ONE_MOUTH_BIND", "127.0.0.1:18091"),
            ("GALION_HOME", home.to_str().unwrap()),
            ("ONE_HERMES_DIR", hermes.to_str().unwrap()),
        ]))
        .unwrap();
        assert_eq!(settings.bind, "127.0.0.1:18091");
        assert_eq!(settings.mouth_bind, "127.0.0.1:18091");
        assert_eq!(settings.mouth_port, 8002);
        assert_eq!(settings.home, path_to_string(&home));
        assert_eq!(settings.hermes_dir, path_to_string(&hermes));
        assert_eq!(
            settings.home_file("state/volume.json"),
            path_to_string(&home.join("state/volume.json"))
        );
        assert_eq!(
            settings.modes_path(),
            path_to_string(&hermes.join("modes.json"))
        );

        let missing = out_dir().join("also-missing.toml");
        let _ = std::fs::remove_file(&missing);
        let from_env = resolve(&env_from(&[
            ("OPENMIND_CONFIG", missing.to_str().unwrap()),
            ("OPENMIND_PATHS_BASE", "/tmp/ignored"),
            ("OPENMIND_PORTS_ONE", "1"),
            ("GALION_HOME", "/tmp/one-home/"),
            ("ONE_HERMES_DIR", "/tmp/one-hermes/"),
        ]))
        .unwrap();
        assert_eq!(from_env.bind, DEFAULTS.bind);
        assert_eq!(from_env.mouth_bind, DEFAULTS.mouth_bind);
        assert_eq!(from_env.home, "/tmp/one-home/");
        assert_eq!(
            from_env.home_file("one/index.html"),
            "/tmp/one-home/one/index.html"
        );
        assert_eq!(from_env.hermes_dir, "/tmp/one-hermes/");
        assert_eq!(from_env.modes_path(), "/tmp/one-hermes/modes.json");

        let blank_home = resolve(&env_from(&[
            ("OPENMIND_CONFIG", missing.to_str().unwrap()),
            ("GALION_HOME", ""),
        ]))
        .unwrap();
        assert_eq!(blank_home.home, "");
        assert_eq!(blank_home.home_file("one/lane"), "/one/lane");
    }

    #[test]
    fn galion_mouth_wins_over_the_configured_port() {
        let set = endpoint_from(
            &env_from(&[("GALION_MOUTH", "127.0.0.1:18002")]),
            "mouth",
            DEFAULTS.mouth_port,
        );
        assert_eq!(set, "127.0.0.1:18002".parse().unwrap());
        let blank = endpoint_from(
            &env_from(&[("GALION_MOUTH", "   ")]),
            "mouth",
            DEFAULTS.mouth_port,
        );
        assert_eq!(blank, "127.0.0.1:8002".parse().unwrap());
        let unset = endpoint_from(&env_from(&[]), "mouth", DEFAULTS.mouth_port);
        assert!(unset.ip().is_loopback());
        assert_eq!(unset.port(), 8002);
    }

    #[test]
    fn bad_config_file_is_an_error() {
        let path = write_fixture("unit-bad.toml", "[ports]\none = \"nope\"\n");
        let err = resolve(&env_from(&[("OPENMIND_CONFIG", path.to_str().unwrap())])).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unit-bad.toml"), "{msg}");
    }
}
