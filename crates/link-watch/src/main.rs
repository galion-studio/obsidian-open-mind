// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Rust port of `/opt/galion/galion-light/scripts/link-watch.py`.
//!
//! Same probes, same JSON keys, same CPython `json.dumps` byte layout. One new
//! key is added: `at_local`, an Europe/Zagreb RFC 3339 timestamp with an
//! explicit offset (via the shared `zagreb-time` crate). The legacy `at` key is
//! kept verbatim for wire compatibility with readers that already parse it, and
//! the legacy event stamps are kept verbatim too; both are UTC and both are
//! documented as such in `docs/` and in the crate README.
//!
//! Probe URLs and the output paths come from `openmind-config` when the config
//! file exists (`$OPENMIND_CONFIG`, default `/etc/openmind/openmind.toml`).
//! `LINK_WATCH_*` environment variables still win over that file, and over
//! `OPENMIND_*` overrides applied by the loader. If the config file is missing,
//! the `Defaults` below are kept (today's paths and loopback probe URLs) and
//! `OPENMIND_*` is not consulted.
//!
//!   LINK_WATCH_OUT         output file (default: live `state/link.json`)
//!   LINK_WATCH_LAPTOP      laptop card   (default: live `state/laptop.json`)
//!   LINK_WATCH_LOG         event log     (default: live `state/link-log.json`)
//!   LINK_WATCH_OLLAMA_URL  base URL for `/api/ps` and `/api/tags`
//!   LINK_WATCH_HERMES_URL  base URL for `/health`
//!   LINK_WATCH_FRANCE_URL  base URL for `/console` (no built-in host; unset
//!                          means no network call and `"france": "unconfigured"`,
//!                          unless the config file sets `hosts.remote_probe`)
//!   LINK_WATCH_ONCE        if set, run exactly one iteration and exit
//!
//! A present file supplies `http://{hosts.ollama}:{ports.ollama}`,
//! `http://{hosts.hermes}:{ports.hermes}`, and, when `hosts.remote_probe` is
//! set, `http://{hosts.remote_probe}:{ports.console}` as the France base.
//! `link.json`, `laptop.json` and `link-log.json` are `paths.base` joined with
//! those names. Loopback defaults match the Python original. The remote probe
//! has no built-in host.

mod jsonout;

use jsonout::{json_str, to_json_compat};
use link_watch::{watch_decision, WatchFacts};
use openmind_config::ConfigError;
use serde_json::{Map, Value};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Built-in defaults used when the OpenMind config file is missing.
/// Only loopback and local state paths live here; the remote probe has
/// no default.
struct Defaults {
    out: &'static str,
    laptop: &'static str,
    log: &'static str,
    ollama: &'static str,
    hermes: &'static str,
}

const DEFAULTS: Defaults = Defaults {
    out: "/opt/galion/galion-light/state/link.json",
    laptop: "/opt/galion/galion-light/state/laptop.json",
    log: "/opt/galion/galion-light/state/link-log.json",
    ollama: "http://127.0.0.1:11434",
    hermes: "http://127.0.0.1:8642",
};

/// Value of the `france` key when no France probe base is configured.
const UNCONFIGURED: &str = "unconfigured";

/// Resolved paths and probe bases for one process.
#[derive(Debug)]
struct Settings {
    out: PathBuf,
    laptop: PathBuf,
    log: PathBuf,
    ollama: String,
    hermes: String,
    france: Option<String>,
}

impl Settings {
    fn from_defaults() -> Self {
        Settings {
            out: PathBuf::from(DEFAULTS.out),
            laptop: PathBuf::from(DEFAULTS.laptop),
            log: PathBuf::from(DEFAULTS.log),
            ollama: DEFAULTS.ollama.to_string(),
            hermes: DEFAULTS.hermes.to_string(),
            france: None,
        }
    }
}

fn probe_base(host: &str, port: u16) -> String {
    format!("http://{host}:{port}")
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

fn apply_link_watch_env(settings: &mut Settings, env: &dyn Fn(&str) -> Option<String>) {
    if let Some(value) = env("LINK_WATCH_OUT") {
        settings.out = PathBuf::from(value);
    }
    if let Some(value) = env("LINK_WATCH_LAPTOP") {
        settings.laptop = PathBuf::from(value);
    }
    if let Some(value) = env("LINK_WATCH_LOG") {
        settings.log = PathBuf::from(value);
    }
    if let Some(value) = env("LINK_WATCH_OLLAMA_URL") {
        settings.ollama = value;
    }
    if let Some(value) = env("LINK_WATCH_HERMES_URL") {
        settings.hermes = value;
    }
    if let Some(value) = env("LINK_WATCH_FRANCE_URL") {
        let trimmed = value.trim();
        settings.france = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
    }
}

/// Precedence: `LINK_WATCH_*`, then a present config file (with `OPENMIND_*`
/// already applied by `openmind-config`), then `Defaults` when that file is
/// missing.
fn resolve(env: &dyn Fn(&str) -> Option<String>) -> Result<Settings, ConfigError> {
    let mut settings = Settings::from_defaults();
    let path = config_file_path(env);
    if config_file_present(&path)? {
        let cfg = openmind_config::load_from(Some(&path), env)?;
        settings.out = cfg.paths.state_file("link.json");
        settings.laptop = cfg.paths.state_file("laptop.json");
        settings.log = cfg.paths.state_file("link-log.json");
        settings.ollama = probe_base(cfg.hosts.ollama.trim(), cfg.ports.ollama);
        settings.hermes = probe_base(cfg.hosts.hermes.trim(), cfg.ports.hermes);
        if let Some(host) = cfg
            .hosts
            .remote_probe
            .as_deref()
            .map(str::trim)
            .filter(|host| !host.is_empty())
        {
            settings.france = Some(probe_base(host, cfg.ports.console));
        }
    }
    apply_link_watch_env(&mut settings, env);
    Ok(settings)
}

fn env_flag(key: &str) -> bool {
    match std::env::var(key) {
        Ok(v) => !v.is_empty() && v != "0",
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Legacy UTC formatting (kept only for the legacy `at` key and event stamps).
// ---------------------------------------------------------------------------

/// Days since the unix epoch -> (year, month, day). Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn split_unix(secs: i64) -> (i64, i64, i64, i64, i64, i64) {
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    let (y, mo, d) = civil_from_days(days);
    (y, mo, d, sod / 3600, (sod % 3600) / 60, sod % 60)
}

/// CPython `time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())`.
fn utc_stamp_full(secs: i64) -> String {
    let (y, mo, d, h, mi, s) = split_unix(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// CPython `time.strftime("%H:%M:%S", time.gmtime())`.
fn utc_stamp_clock(secs: i64) -> String {
    let (_, _, _, h, mi, s) = split_unix(secs);
    format!("{h:02}:{mi:02}:{s:02}")
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn unix_now_f64() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

// ---------------------------------------------------------------------------
// Tiny blocking HTTP/1.1 client (no TLS, no redirects - the probes are plain
// loopback HTTP, exactly like the Python urllib calls).
// ---------------------------------------------------------------------------

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn dechunk(mut data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let nl = find(data, b"\r\n")?;
        let line = String::from_utf8_lossy(&data[..nl]);
        let size_str = line.split(';').next()?.trim();
        let size = usize::from_str_radix(size_str, 16).ok()?;
        data = &data[nl + 2..];
        if size == 0 {
            break;
        }
        if data.len() < size {
            return None;
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size..];
        if data.len() >= 2 {
            data = &data[2..];
        }
    }
    Some(out)
}

fn http_get(url: &str, timeout: Duration) -> Option<(u16, Vec<u8>)> {
    let rest = url.strip_prefix("http://")?;
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if hostport.is_empty() {
        return None;
    }
    let addr = if hostport.contains(':') {
        hostport.to_string()
    } else {
        format!("{hostport}:80")
    };
    let sock = addr.to_socket_addrs().ok()?.next()?;
    let mut stream = TcpStream::connect_timeout(&sock, timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {hostport}\r\nUser-Agent: link-watch\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).ok()?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).ok()?;
    let sep = find(&buf, b"\r\n\r\n")?;
    let head = &buf[..sep];
    let raw_body = &buf[sep + 4..];
    let first_line_end = find(head, b"\r\n").unwrap_or(head.len());
    let status_line = String::from_utf8_lossy(&head[..first_line_end]);
    let status: u16 = status_line.split_whitespace().nth(1)?.parse().ok()?;
    let headers = String::from_utf8_lossy(head).to_ascii_lowercase();
    let body = if headers.contains("transfer-encoding: chunked") {
        dechunk(raw_body)?
    } else {
        raw_body.to_vec()
    };
    Some((status, body))
}

/// CPython `probe(url, timeout)`: `(ok, milliseconds, body)`.
fn probe(url: &str, timeout: Duration) -> (bool, i64, Vec<u8>) {
    let started = Instant::now();
    let got = http_get(url, timeout);
    let ms = started.elapsed().as_millis() as i64;
    match got {
        Some((status, body)) if (200..400).contains(&status) => (true, ms, body),
        _ => (false, ms, Vec::new()),
    }
}

// ---------------------------------------------------------------------------

fn trim_base(base: &str) -> &str {
    base.trim_end_matches('/')
}

fn load_events(path: &Path) -> Vec<Value> {
    if !path.exists() {
        return Vec::new();
    }
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return Vec::new();
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Array(items)) => items,
        _ => Vec::new(),
    }
}

fn write_json(path: &Path, value: &Value) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(err) = std::fs::write(path, to_json_compat(value, true)) {
        eprintln!("link-watch: cannot write {}: {err}", path.display());
    }
}

/// The model name derived from `/api/ps`, or the idle fallback from `/api/tags`.
fn model_from_ps(body: &[u8]) -> String {
    let Ok(parsed) = serde_json::from_slice::<Value>(body) else {
        return String::new();
    };
    let Some(loaded) = parsed.get("models").and_then(|m| m.as_array()) else {
        return String::new();
    };
    let Some(first) = loaded.first() else {
        return String::new();
    };
    let name = first.get("name").map(json_str).unwrap_or_default();
    match first.get("context_length") {
        Some(ctx) if jsonout::truthy(ctx) => format!("{name} ctx {}", json_str(ctx)),
        _ => name,
    }
}

fn model_from_tags(body: &[u8]) -> String {
    let Ok(parsed) = serde_json::from_slice::<Value>(body) else {
        return String::new();
    };
    let names: Vec<String> = parsed
        .get("models")
        .and_then(|m| m.as_array())
        .map(|items| {
            items
                .iter()
                .map(|m| match m.get("name") {
                    Some(Value::String(s)) => s.clone(),
                    _ => String::new(),
                })
                .collect()
        })
        .unwrap_or_default();
    let prefer = names
        .iter()
        .find(|n| n.starts_with("galion"))
        .cloned()
        .unwrap_or_default();
    let base = if !prefer.is_empty() {
        prefer
    } else {
        names.first().cloned().unwrap_or_default()
    };
    format!("{base} (idle)")
}

fn main() {
    let settings = match resolve(&|key| std::env::var(key).ok()) {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("link-watch: {err}");
            std::process::exit(1);
        }
    };
    let Settings {
        out,
        laptop,
        log,
        ollama,
        hermes,
        france,
    } = settings;
    let once = env_flag("LINK_WATCH_ONCE");

    let ps_url = format!("{}/api/ps", trim_base(&ollama));
    let tags_url = format!("{}/api/tags", trim_base(&ollama));
    let health_url = format!("{}/health", trim_base(&hermes));
    let console_url = france
        .as_deref()
        .map(|base| format!("{}/console", trim_base(base)));

    let mut france_state = Value::String(UNCONFIGURED.to_string());
    let mut france_at = 0.0_f64;
    let mut previous: Option<link_watch::WatchSignature> = None;
    let mut events = load_events(&log);

    loop {
        let (ok, ms, body) = probe(&ps_url, Duration::from_secs(3));
        let mut model = String::new();
        if ok {
            model = model_from_ps(&body);
        }
        if ok && model.is_empty() {
            let (_, _, tags_body) = probe(&tags_url, Duration::from_secs(3));
            model = model_from_tags(&tags_body);
        }
        let (hermes_ok, hermes_ms, _) = probe(&health_url, Duration::from_secs(3));
        let now = unix_now_f64();
        if let Some(url) = &console_url {
            if now - france_at > 30.0 {
                let (f_ok, _, _) = probe(url, Duration::from_secs(4));
                france_state = Value::Bool(f_ok);
                france_at = now;
            }
        }
        let mut card = Value::Object(Map::new());
        if laptop.exists() {
            if let Ok(bytes) = std::fs::read(&laptop) {
                if let Ok(parsed) = serde_json::from_slice::<Value>(&bytes) {
                    card = parsed;
                }
            }
        }
        let now_secs = unix_now();
        let facts = WatchFacts {
            at: utc_stamp_full(now_secs),
            at_local: zagreb_time::now_rfc3339(),
            stamp: utc_stamp_clock(now_secs),
            laptop_port: ok,
            model,
            model_ms: ms,
            hermes: hermes_ok,
            hermes_ms,
            france: france_state.clone(),
            laptop: card,
        };
        let decision = watch_decision(&facts, previous.as_ref(), &events);
        if decision.events_changed {
            write_json(&log, &Value::Array(decision.events.clone()));
        }
        events = decision.events;
        previous = Some(decision.signature);
        write_json(&out, &decision.report);

        if once {
            break;
        }
        std::thread::sleep(Duration::from_secs(3));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out_dir() -> PathBuf {
        let dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-out/link-watch");
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
            ("OPENMIND_PATHS_BASE", "/tmp/not-today"),
        ]))
        .unwrap();
        assert_eq!(settings.out, PathBuf::from(DEFAULTS.out));
        assert_eq!(settings.laptop, PathBuf::from(DEFAULTS.laptop));
        assert_eq!(settings.log, PathBuf::from(DEFAULTS.log));
        assert_eq!(settings.ollama, DEFAULTS.ollama);
        assert_eq!(settings.hermes, DEFAULTS.hermes);
        assert!(settings.france.is_none());
    }

    #[test]
    fn config_fixture_supplies_urls_and_paths() {
        let base = out_dir().join("unit-base");
        let path = write_fixture(
            "unit-fixture.toml",
            &format!(
                "\
[hosts]
ollama = \"ollama.example.invalid\"
hermes = \"hermes.example.invalid\"
remote_probe = \"probe.example.invalid\"
[ports]
ollama = 14001
hermes = 14002
console = 14003
[paths]
base = '{base}'
",
                base = base.display()
            ),
        );
        let settings = resolve(&env_from(&[("OPENMIND_CONFIG", path.to_str().unwrap())])).unwrap();
        assert_eq!(settings.out, base.join("link.json"));
        assert_eq!(settings.laptop, base.join("laptop.json"));
        assert_eq!(settings.log, base.join("link-log.json"));
        assert_eq!(settings.ollama, "http://ollama.example.invalid:14001");
        assert_eq!(settings.hermes, "http://hermes.example.invalid:14002");
        assert_eq!(
            settings.france.as_deref(),
            Some("http://probe.example.invalid:14003")
        );
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
hermes = \"hermes.example.invalid\"
[ports]
ollama = 1
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
            ("OPENMIND_PATHS_BASE", overridden.to_str().unwrap()),
            ("OPENMIND_HOSTS_REMOTE_PROBE", "probe.example.invalid"),
        ]))
        .unwrap();
        assert_eq!(settings.ollama, "http://env.example.invalid:9");
        assert_eq!(settings.out, overridden.join("link.json"));
        assert_eq!(
            settings.france.as_deref(),
            Some("http://probe.example.invalid:8080")
        );
    }

    #[test]
    fn link_watch_env_wins_over_config_file() {
        let base = out_dir().join("unit-link-base");
        let path = write_fixture(
            "unit-link-file.toml",
            &format!(
                "\
[hosts]
ollama = \"file.example.invalid\"
hermes = \"file-hermes.example.invalid\"
remote_probe = \"probe.example.invalid\"
[paths]
base = '{base}'
",
                base = base.display()
            ),
        );
        let settings = resolve(&env_from(&[
            ("OPENMIND_CONFIG", path.to_str().unwrap()),
            ("OPENMIND_HOSTS_OLLAMA", "openmind.example.invalid"),
            ("LINK_WATCH_OUT", "/tmp/link-watch-env-out.json"),
            ("LINK_WATCH_LAPTOP", "/tmp/link-watch-env-laptop.json"),
            ("LINK_WATCH_LOG", "/tmp/link-watch-env-log.json"),
            ("LINK_WATCH_OLLAMA_URL", "http://127.0.0.1:1"),
            ("LINK_WATCH_HERMES_URL", "http://127.0.0.1:1"),
            ("LINK_WATCH_FRANCE_URL", "  "),
        ]))
        .unwrap();
        assert_eq!(settings.out, PathBuf::from("/tmp/link-watch-env-out.json"));
        assert_eq!(
            settings.laptop,
            PathBuf::from("/tmp/link-watch-env-laptop.json")
        );
        assert_eq!(settings.log, PathBuf::from("/tmp/link-watch-env-log.json"));
        assert_eq!(settings.ollama, "http://127.0.0.1:1");
        assert_eq!(settings.hermes, "http://127.0.0.1:1");
        assert!(settings.france.is_none());
    }

    #[test]
    fn bad_config_file_is_an_error() {
        let path = write_fixture("unit-bad.toml", "[ports]\nollama = \"nope\"\n");
        let err = resolve(&env_from(&[("OPENMIND_CONFIG", path.to_str().unwrap())])).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unit-bad.toml"), "{msg}");
    }
}
