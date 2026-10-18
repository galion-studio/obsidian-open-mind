// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
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
//!   LINK_WATCH_FRANCE_URL  base URL for `/console` (no default; unset means
//!                          no network call and `"france": "unconfigured"`)
//!   LINK_WATCH_ONCE        if set, run exactly one iteration and exit
//!
//! Loopback and path defaults match the Python original. The remote probe has
//! no built-in host.

mod jsonout;

use jsonout::{json_str, to_json_compat};
use serde_json::{Map, Value};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Every built-in default in one place. Only loopback and local state paths
/// live here; the remote probe has no default and comes from the environment.
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

/// Value of the `france` key when `LINK_WATCH_FRANCE_URL` is unset or empty.
const UNCONFIGURED: &str = "unconfigured";

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
// Tiny blocking HTTP/1.1 client (no TLS; follows http redirects like urllib;
// stops at Content-Length so keep-alive servers do not stall. The probes are plain
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

/// One GET. Reads until EOF or until the Content-Length body is complete, so
/// a server that keeps the connection open does not cost a full timeout.
/// Returns `(status, raw headers, body)`.
fn http_get_once(url: &str, timeout: Duration) -> Option<(u16, String, Vec<u8>)> {
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
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if let Some(sep) = find(&buf, b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..sep]).to_ascii_lowercase();
                    let want = head.lines().find_map(|l| {
                        l.strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    });
                    if want.is_some_and(|w| buf.len() - sep - 4 >= w) {
                        break;
                    }
                }
                if buf.len() > 8 * 1024 * 1024 {
                    return None;
                }
            }
            Err(_) => return None,
        }
    }
    let sep = find(&buf, b"\r\n\r\n")?;
    let head = &buf[..sep];
    let raw_body = &buf[sep + 4..];
    let first_line_end = find(head, b"\r\n").unwrap_or(head.len());
    let status_line = String::from_utf8_lossy(&head[..first_line_end]);
    let status: u16 = status_line.split_whitespace().nth(1)?.parse().ok()?;
    let headers = String::from_utf8_lossy(head).to_string();
    let body = if headers
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        dechunk(raw_body)?
    } else {
        raw_body.to_vec()
    };
    Some((status, headers, body))
}

/// Resolve a redirect `Location` against the current http URL.
fn redirect_target(current: &str, location: &str) -> Option<String> {
    let location = location.trim();
    if location.starts_with("http://") {
        return Some(location.to_string());
    }
    if location.starts_with("https://") || location.is_empty() {
        // No TLS here; urllib would follow it. Treat as a failed probe.
        return None;
    }
    let rest = current.strip_prefix("http://")?;
    let host_end = rest.find('/').unwrap_or(rest.len());
    let origin = &current[..7 + host_end];
    if location.starts_with('/') {
        return Some(format!("{origin}{location}"));
    }
    let path = &rest[host_end..];
    let dir = &path[..path.rfind('/').map(|i| i + 1).unwrap_or(0)];
    let dir = if dir.is_empty() { "/" } else { dir };
    Some(format!("{origin}{dir}{location}"))
}

/// GET following up to 5 redirects (301/302/303/307/308), like urllib.
fn http_get(url: &str, timeout: Duration) -> Option<(u16, Vec<u8>)> {
    let mut url = url.to_string();
    for _ in 0..=5 {
        let (status, headers, body) = http_get_once(&url, timeout)?;
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            let loc = headers.lines().skip(1).find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.trim()
                    .eq_ignore_ascii_case("location")
                    .then(|| v.trim().to_string())
            });
            url = redirect_target(&url, &loc?)?;
            continue;
        }
        return Some((status, body));
    }
    None
}

/// CPython `probe(url, timeout)`: `(ok, milliseconds, body)`. urllib raises on
/// any final status outside 2xx, so only 2xx counts as up.
fn probe(url: &str, timeout: Duration) -> (bool, i64, Vec<u8>) {
    let started = Instant::now();
    let got = http_get(url, timeout);
    let ms = started.elapsed().as_millis() as i64;
    match got {
        Some((status, body)) if (200..300).contains(&status) => (true, ms, body),
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
    let out = PathBuf::from(env_or("LINK_WATCH_OUT", DEFAULTS.out));
    let laptop = PathBuf::from(env_or("LINK_WATCH_LAPTOP", DEFAULTS.laptop));
    let log = PathBuf::from(env_or("LINK_WATCH_LOG", DEFAULTS.log));
    let ollama = env_or("LINK_WATCH_OLLAMA_URL", DEFAULTS.ollama);
    let hermes = env_or("LINK_WATCH_HERMES_URL", DEFAULTS.hermes);
    let france = std::env::var("LINK_WATCH_FRANCE_URL")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let once = env_flag("LINK_WATCH_ONCE");

    let ps_url = format!("{}/api/ps", trim_base(&ollama));
    let tags_url = format!("{}/api/tags", trim_base(&ollama));
    let health_url = format!("{}/health", trim_base(&hermes));
    let console_url = france
        .as_deref()
        .map(|base| format!("{}/console", trim_base(base)));

    let mut france_state = Value::String(UNCONFIGURED.to_string());
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
        let stamp = utc_stamp_clock(now_secs);
        let card_name = match card.get("name") {
            Some(v) => json_str(v),
            None => String::new(),
        };
        let france_sig = france_state.as_bool().unwrap_or(false);
        let signature = (ok, model.clone(), hermes_ok, france_sig, card_name);
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
        row.insert("france".to_string(), france_state.clone());
        row.insert("laptop".to_string(), card);
        row.insert("events".to_string(), Value::Array(events.clone()));
        write_json(&out, &Value::Object(row));

        if once {
            break;
        }
        std::thread::sleep(Duration::from_secs(3));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn redirect_targets() {
        let u = "http://10.0.0.1/console";
        assert_eq!(
            redirect_target(u, "/console/").as_deref(),
            Some("http://10.0.0.1/console/")
        );
        assert_eq!(
            redirect_target(u, "Login").as_deref(),
            Some("http://10.0.0.1/Login")
        );
        assert_eq!(
            redirect_target("http://h:8/a/b", "c").as_deref(),
            Some("http://h:8/a/c")
        );
        assert_eq!(
            redirect_target(u, "http://x/y").as_deref(),
            Some("http://x/y")
        );
        assert_eq!(redirect_target(u, "https://x/y"), None);
    }

    /// Fake server: `/r` redirects to `/Ok`, `/Ok` answers 200 and keeps the
    /// connection open (Content-Length only), `/e` answers 503.
    fn serve() -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for mut c in l.incoming().flatten() {
                let mut b = [0u8; 2048];
                let n = c.read(&mut b).unwrap_or(0);
                let req = String::from_utf8_lossy(&b[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
                let resp = match path.as_str() {
                    "/r" => "HTTP/1.1 302 Found\r\nLocation: /Ok\r\nContent-Length: 0\r\n\r\n"
                        .to_string(),
                    "/Ok" => {
                        "HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}".to_string()
                    }
                    _ => "HTTP/1.1 503 Busy\r\nContent-Length: 0\r\n\r\n".to_string(),
                };
                let _ = c.write_all(resp.as_bytes());
                held.push(c); // never close: a keep-alive server
            }
        });
        base
    }

    #[test]
    fn follows_redirect_and_does_not_wait_for_close() {
        let base = serve();
        let t0 = Instant::now();
        let (ok, _ms, body) = probe(&format!("{base}/r"), Duration::from_secs(3));
        assert!(ok);
        assert_eq!(body, b"{\"ok\":true}");
        assert!(t0.elapsed() < Duration::from_secs(2), "{:?}", t0.elapsed());
        let (ok, _, body) = probe(&format!("{base}/e"), Duration::from_secs(3));
        assert!(!ok && body.is_empty());
    }

    #[test]
    fn model_names() {
        assert_eq!(
            model_from_ps(br#"{"models":[{"name":"galion:latest","context_length":65536}]}"#),
            "galion:latest ctx 65536"
        );
        assert_eq!(model_from_ps(br#"{"models":[]}"#), "");
        assert_eq!(
            model_from_tags(br#"{"models":[{"name":"qwen"},{"name":"galion:latest"}]}"#),
            "galion:latest (idle)"
        );
        assert_eq!(model_from_tags(b"{}"), " (idle)");
    }
}
