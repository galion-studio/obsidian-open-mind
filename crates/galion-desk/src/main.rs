// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Clippy allows, added only so `clippy -D warnings` passes. Both fire on the
// accept loop exactly as it is written in the live galion-desk source. The
// suggested rewrites (`.flatten()` on `listener.incoming()`, dropping the
// `let _ =` in front of a unit-valued `handle`) touch the control flow of the
// running server, so this ticket prefers an explicit allow over a rewrite.
#![allow(clippy::manual_flatten)]
#![allow(clippy::let_unit_value)]

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const NOTES: &str = "/opt/galion/galion-light/state/desk-notes.jsonl";
const FOCUS: &str = "/opt/galion/galion-light/state/desk-focus";
const LINK: &str = "/opt/galion/galion-light/state/link.json";
const VOICE: &str = "/workspace/galion-light/one/voice";
const ONE: &str = "/workspace/galion-light/one/session.jsonl";

fn main() {
    let bind = match std::env::var("DESK_BIND") {
        Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => "127.0.0.1:8646".to_string(),
    };
    let listener = TcpListener::bind(&bind).expect("bind");
    eprintln!("galion-desk 127.0.0.1:8646");
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
            let _ = fs::write(VOICE, format!("{name}\n"));
        }
    } else if act == "drop" {
        drop_note();
    } else if act == "stamp" {
        let line = if tcp_open(11434) { "up" } else { "down" };
        let hermes = if tcp_open(8642) { "up" } else { "down" };
        write_note(&format!("Stamp. Line {line}. Hermes {hermes}."));
    } else if act == "hold" {
        let path = "/opt/galion/galion-light/state/desk-hold";
        if fs::metadata(path).is_ok() {
            let _ = fs::remove_file(path);
        } else {
            let _ = fs::write(path, "held\n");
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
    let _ = fs::create_dir_all("/opt/galion/galion-light/state");
    if id.is_empty() {
        let _ = fs::remove_file(FOCUS);
    } else {
        let _ = fs::write(FOCUS, format!("{id}\n"));
    }
}

fn focus_id() -> String {
    fs::read_to_string(FOCUS)
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
    let _ = fs::create_dir_all("/opt/galion/galion-light/state");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(NOTES) {
        let line = format!("{{\"at\":{},\"text\":\"{}\"}}\n", now(), escape(&text));
        let _ = file.write_all(line.as_bytes());
    }
}

fn drop_note() {
    let raw = fs::read_to_string(NOTES).unwrap_or_default();
    let mut rows: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
    if rows.is_empty() {
        return;
    }
    rows.pop();
    let mut out = rows.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    let _ = fs::write(NOTES, out);
}

static WAKING: AtomicBool = AtomicBool::new(false);

fn wake() {
    if WAKING.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::spawn(|| {
        let body = "{\"model\":\"galion:latest\",\"prompt\":\"ok\",\"stream\":false,\"options\":{\"num_predict\":1}}";
        let _ = http_post("127.0.0.1:11434", "/api/generate", body);
        WAKING.store(false, Ordering::SeqCst);
    });
}

static RESTING: AtomicBool = AtomicBool::new(false);

fn rest() {
    let name = ollama_ps().0;
    if name.is_empty() || RESTING.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::spawn(move || {
        let body = format!(
            "{{\"model\":\"{}\",\"keep_alive\":0,\"prompt\":\"\",\"stream\":false}}",
            escape(&name)
        );
        let _ = http_post("127.0.0.1:11434", "/api/generate", &body);
        RESTING.store(false, Ordering::SeqCst);
    });
}

fn snapshot() -> String {
    let link = fs::read_to_string(LINK).unwrap_or_default();
    let model_ps = ollama_ps();
    let (model, vram, ctx) = if model_ps.0.is_empty() {
        let raw = json_string(&link, "model");
        (clean_model(&raw), 0, last_ctx(&link))
    } else {
        (clean_model(&model_ps.0), model_ps.1, model_ps.2)
    };
    let voice = fs::read_to_string(VOICE)
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
        tcp_open(11434),
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
        fs::metadata("/opt/galion/galion-light/state/desk-hold").is_ok(),
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
    let bin = std::env::var("DESK_HERMES_BIN")
        .unwrap_or_else(|_| "/opt/galion/openmind/dist/bin/desk-hermes".to_string());
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
        ("Console", 8080u16),
        ("Origin", 80u16),
        ("Hermes", 8642u16),
        ("Laptop", 11434u16),
        ("One", 8002u16),
        ("Room", 4020u16),
        ("Tulu", 8093u16),
        ("Tunnel", 20241u16),
    ];
    rows.iter()
        .map(|(name, port)| format!("{{\"name\":\"{}\",\"up\":{}}}", name, tcp_open(*port)))
        .collect::<Vec<_>>()
        .join(",")
}

fn board() -> String {
    let link = fs::read_to_string(LINK).unwrap_or_default();
    let survival =
        fs::read_to_string("/opt/galion/galion-light/state/survival.json").unwrap_or_default();
    let gw = fs::read_to_string("/opt/galion/hermes/gateway_state.json").unwrap_or_default();
    let machine =
        fs::read_to_string("/opt/galion/galion-light/state/machine.json").unwrap_or_default();
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
    let raw = fs::read_to_string(NOTES).unwrap_or_default();
    let mut rows: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
    if rows.len() > 12 {
        rows = rows[rows.len() - 12..].to_vec();
    }
    rows.join(",")
}

fn one_last() -> (String, String, usize) {
    let raw = fs::read_to_string(ONE).unwrap_or_default();
    let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
    let turns = lines.len();
    let last = lines.last().copied().unwrap_or("");
    (json_string(last, "heard"), json_string(last, "said"), turns)
}

fn ollama_ps() -> (String, u64, u64) {
    let raw = http_get("127.0.0.1:11434", "/api/ps");
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
