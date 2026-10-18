use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::thread;
use std::time::Duration;

// Listen address for the room socket. Reads ROOM_BIND (trimmed; empty counts as
// unset) and otherwise falls back to the loopback address room has always bound.
fn bind_addr(name: &str, port: u16) -> String {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => format!("127.0.0.1:{port}"),
    }
}

struct Obj {
    kind: &'static str,
    name: String,
    path: String,
    note: String,
}

fn main() {
    let objects = index();
    let json = to_json(&objects);
    let listener = TcpListener::bind(bind_addr("ROOM_BIND", 4020)).expect("4020");
    eprintln!("room: {} objects", objects.len());
    for stream in listener.incoming().flatten() {
        let json = json.clone();
        thread::spawn(move || {
            let _ = handle(stream, &json);
        });
    }
}

fn index() -> Vec<Obj> {
    let mut out = vec![
        live("one", "The mouth. Rust, 8091."),
        live("voice", "Chatterbox. Warm on this box."),
        live("whisper", "The ear."),
        live("hermes", "The agent."),
        live("vision", "The eyes."),
        live("console", "The desk."),
        live("room", "This browser. Rust, 4020."),
    ];
    for host in [
        "galion.app",
        "studio.galion.app",
        "developer.galion.app",
        "api.galion.app",
        "docs.galion.app",
        "reference.galion.app",
        "voice.galion.app",
        "knowledge.galion.app",
        "galion.studio",
        "machine.galion.app",
        "merlinelement.com",
        "sentialink.com",
    ] {
        out.push(Obj {
            kind: "held",
            name: host.to_string(),
            path: host.to_string(),
            note: "Named. The heavy face is not loaded.".to_string(),
        });
    }
    let mut budget = 1800usize;
    for root in [
        "/workspace/galion-light/docs",
        "/workspace/galion-src/galion-docs",
        "/workspace/galion-src/galion-workspace3d/src",
        "/workspace/galion-src/galion-app/docs",
        "/workspace/galion-src/galion-app/src",
    ] {
        walk(Path::new(root), &mut out, &mut budget);
    }
    for path in [
        "/workspace/galion-src/galion-workspace3d/package.json",
        "/workspace/galion-src/galion-app/package.json",
        "/workspace/galion-src/galion-docs/package.json",
        "/workspace/galion-src/galion-rs/Cargo.toml",
        "/workspace/galion-src/galion-console/package.json",
        "/workspace/galion-src/galion-api-docs/package.json",
        "/workspace/galion-src/galion-knowledge/package.json",
        "/workspace/galion-src/galion-studio-app/package.json",
        "/workspace/galion-src/merlin-element/package.json",
        "/workspace/galion-src/galion-shared/package.json",
    ] {
        if let Some(obj) = classify(Path::new(path)) {
            out.push(obj);
        }
    }
    out
}

fn live(name: &str, note: &str) -> Obj {
    Obj {
        kind: "live",
        name: name.to_string(),
        path: name.to_string(),
        note: note.to_string(),
    }
}

fn walk(dir: &Path, out: &mut Vec<Obj>, budget: &mut usize) {
    if *budget == 0 {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else { return };
    for ent in read.flatten() {
        if *budget == 0 {
            return;
        }
        let path = ent.path();
        let name = ent.file_name().to_string_lossy().to_string();
        if name.starts_with('.')
            || matches!(
                name.as_str(),
                "node_modules" | "target" | "dist" | "__pycache__" | "venv" | ".next"
            )
        {
            continue;
        }
        if path.is_dir() {
            walk(&path, out, budget);
            continue;
        }
        if let Some(obj) = classify(&path) {
            out.push(obj);
            *budget -= 1;
        }
    }
}

fn classify(path: &Path) -> Option<Obj> {
    let text = path.to_string_lossy().to_string();
    let low = text.to_ascii_lowercase();
    if low.contains("secret")
        || low.contains(".env")
        || low.contains("token")
        || low.contains("id_ed25519")
    {
        return None;
    }
    let name = path.file_name()?.to_string_lossy().to_string();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let stem = name.to_ascii_lowercase();
    let kind =
        if ext == "pdf" || (ext == "md" && (low.contains("/papers/") || stem.contains("paper"))) {
            "paper"
        } else if name == "package.json" || name == "Cargo.toml" {
            "library"
        } else if ext == "md" || ext == "txt" {
            "doc"
        } else if low.contains("/api/")
            && matches!(ext.as_str(), "ts" | "tsx" | "py" | "rs" | "yml" | "yaml")
        {
            "api"
        } else if matches!(ext.as_str(), "ts" | "tsx" | "js" | "jsx")
            && (name.contains("Orb")
                || name.contains("Scene")
                || name.contains("Shader")
                || name.contains("Particle")
                || name.contains("Canvas"))
        {
            "light"
        } else {
            return None;
        };
    let show = if name == "package.json" || name == "Cargo.toml" {
        lib_name(path).unwrap_or_else(|| name.clone())
    } else {
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or(name)
    };
    Some(Obj {
        kind,
        name: show,
        path: text,
        note: first_line(path),
    })
}

fn lib_name(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    for key in ["\"name\"", "name"] {
        if let Some(pos) = text.find(key) {
            let rest = &text[pos + key.len()..];
            let start = rest.find('"')? + 1;
            let end = rest[start..].find('"')?;
            let name = rest[start..start + end].trim();
            if !name.is_empty() && name != "name" {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn first_line(path: &Path) -> String {
    let Ok(text) = fs::read_to_string(path) else {
        return String::new();
    };
    for line in text.lines() {
        let line = line.trim().trim_start_matches('#').trim();
        if line.is_empty() || line.starts_with("---") || line.starts_with("import ") {
            continue;
        }
        let mut out = String::new();
        for ch in line.chars().take(160) {
            out.push(ch);
        }
        return out;
    }
    String::new()
}

fn esc(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' | '\r' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

fn to_json(rows: &[Obj]) -> String {
    let mut out = String::from("{\"objects\":[");
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"kind\":\"{}\",\"name\":\"{}\",\"path\":\"{}\",\"note\":\"{}\"}}",
            row.kind,
            esc(&row.name),
            esc(&row.path),
            esc(&row.note)
        ));
    }
    out.push_str("]}");
    out
}

fn handle(mut stream: TcpStream, json: &str) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(8)));
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;
    let head = String::from_utf8_lossy(&buf[..n]);
    let line = head.lines().next().unwrap_or("");
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let url = parts.next().unwrap_or("/");
    let path = url.split('?').next().unwrap_or("/");
    let query = url.split_once('?').map(|p| p.1).unwrap_or("");
    if method != "GET" {
        return write(&mut stream, 405, "text/plain", "no\n");
    }
    match path {
        "/health" => write(&mut stream, 200, "text/plain", "ok\n"),
        "/" | "/index.html" => {
            let bytes = fs::read("/workspace/galion-light/room/index.html")
                .unwrap_or_else(|_| b"missing".to_vec());
            write(
                &mut stream,
                200,
                "text/html; charset=utf-8",
                &String::from_utf8_lossy(&bytes),
            )
        }
        "/api/objects" => write(&mut stream, 200, "application/json", json),
        "/api/read" => {
            let raw = query.strip_prefix("path=").unwrap_or("");
            match read_doc(&decode(raw)) {
                Some(text) => write(&mut stream, 200, "text/plain; charset=utf-8", &text),
                None => write(&mut stream, 404, "text/plain", "held\n"),
            }
        }
        _ => write(&mut stream, 404, "text/plain", "no\n"),
    }
}

fn read_doc(path: &str) -> Option<String> {
    if path.contains("..") || path.contains('\0') {
        return None;
    }
    let ok = path.starts_with("/workspace/galion-src/")
        || path.starts_with("/workspace/galion-light/docs/");
    if !ok {
        return None;
    }
    let low = path.to_ascii_lowercase();
    if low.contains("secret")
        || low.contains(".env")
        || low.contains("token")
        || low.contains("id_ed25519")
    {
        return None;
    }
    if !(low.ends_with(".md") || low.ends_with(".txt")) {
        return None;
    }
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 80_000 {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn decode(raw: &str) -> String {
    let mut out = String::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &raw[i + 1..i + 3];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v as char);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(' ');
        } else {
            out.push(bytes[i] as char);
        }
        i += 1;
    }
    out
}

fn write(stream: &mut TcpStream, status: u16, kind: &str, body: &str) -> std::io::Result<()> {
    let reason = if status == 200 { "OK" } else { "No" };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())
}
