//! Rust port of `/opt/galion/galion-light/scripts/link-watch.py`.
//!
//! Same probes, same JSON keys, same CPython `json.dumps` byte layout. One new
//! key is added: `at_local`, an Europe/Zagreb RFC 3339 timestamp with an
//! explicit offset (via the shared `zagreb-time` crate). The legacy `at` key is
//! kept verbatim for wire compatibility with readers that already parse it, and
//! the legacy event stamps are kept verbatim too; both are UTC and both are
//! documented as such in `docs/` and in the crate README.
//!
//! Every path and every probe target can be overridden with an environment
//! variable so the binary can be tested without touching live state:
//!
//!   LINK_WATCH_OUT         output file (default: live `state/link.json`)
//!   LINK_WATCH_LAPTOP      laptop card   (default: live `state/laptop.json`)
//!   LINK_WATCH_LOG         event log     (default: live `state/link-log.json`)
//!   LINK_WATCH_OLLAMA_URL  base URL for `/api/ps` and `/api/tags`
//!   LINK_WATCH_HERMES_URL  base URL for `/health`
//!   LINK_WATCH_FRANCE_URL  base URL for `/console`
//!   LINK_WATCH_ONCE        if set, run exactly one iteration and exit
//!
//! The defaults are byte-identical to the constants in the Python original, so
//! running the binary with no environment at all behaves exactly like the
//! script did.

mod jsonout;

use jsonout::{json_str, to_json_compat};
use serde_json::{Map, Value};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_OUT: &str = "/opt/galion/galion-light/state/link.json";
const DEFAULT_LAPTOP: &str = "/opt/galion/galion-light/state/laptop.json";
const DEFAULT_LOG: &str = "/opt/galion/galion-light/state/link-log.json";
const DEFAULT_OLLAMA: &str = "http://127.0.0.1:11434";
const DEFAULT_HERMES: &str = "http://127.0.0.1:8642";
const DEFAULT_FRANCE: &str = "http://54.37.161.82";

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
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
    let out = PathBuf::from(env_or("LINK_WATCH_OUT", DEFAULT_OUT));
    let laptop = PathBuf::from(env_or("LINK_WATCH_LAPTOP", DEFAULT_LAPTOP));
    let log = PathBuf::from(env_or("LINK_WATCH_LOG", DEFAULT_LOG));
    let ollama = env_or("LINK_WATCH_OLLAMA_URL", DEFAULT_OLLAMA);
    let hermes = env_or("LINK_WATCH_HERMES_URL", DEFAULT_HERMES);
    let france = env_or("LINK_WATCH_FRANCE_URL", DEFAULT_FRANCE);
    let once = env_flag("LINK_WATCH_ONCE");

    let ps_url = format!("{}/api/ps", trim_base(&ollama));
    let tags_url = format!("{}/api/tags", trim_base(&ollama));
    let health_url = format!("{}/health", trim_base(&hermes));
    let console_url = format!("{}/console", trim_base(&france));

    let mut france_ok = false;
    let mut france_at = 0.0_f64;
    let mut previous: Option<(bool, String, bool, bool, String)> = None;
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
        if now - france_at > 30.0 {
            let (f_ok, _, _) = probe(&console_url, Duration::from_secs(4));
            france_ok = f_ok;
            france_at = now;
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
        let stamp = utc_stamp_clock(now_secs);
        let card_name = match card.get("name") {
            Some(v) => json_str(v),
            None => String::new(),
        };
        let signature = (ok, model.clone(), hermes_ok, france_ok, card_name);
        if Some(&signature) != previous.as_ref() {
            if previous.is_some() {
                let bit = if ok { "up" } else { "down" };
                let model_display = if model.is_empty() {
                    "none".to_string()
                } else {
                    model.clone()
                };
                let hermes_word = if hermes_ok { "up" } else { "down" };
                events.push(Value::String(format!(
                    "{stamp}  laptop {bit}  model {model_display}  hermes {hermes_word}"
                )));
                let len = events.len();
                if len > 12 {
                    events.drain(0..len - 12);
                }
                write_json(&log, &Value::Array(events.clone()));
            }
            previous = Some(signature);
        }
        let mut row = Map::new();
        row.insert("at".to_string(), Value::String(utc_stamp_full(now_secs)));
        row.insert(
            "at_local".to_string(),
            Value::String(zagreb_time::now_rfc3339()),
        );
        row.insert("laptop_port".to_string(), Value::Bool(ok));
        row.insert("model".to_string(), Value::String(model));
        row.insert("model_ms".to_string(), Value::Number(ms.into()));
        row.insert("hermes".to_string(), Value::Bool(hermes_ok));
        row.insert("hermes_ms".to_string(), Value::Number(hermes_ms.into()));
        row.insert("france".to_string(), Value::Bool(france_ok));
        row.insert("laptop".to_string(), card);
        row.insert("events".to_string(), Value::Array(events.clone()));
        write_json(&out, &Value::Object(row));

        if once {
            break;
        }
        std::thread::sleep(Duration::from_secs(3));
    }
}
