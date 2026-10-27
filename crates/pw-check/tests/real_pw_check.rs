// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real test (GALION_REAL=1): the release pw-check binary drives a real headless
//! Chromium (PW_CHROME, else Playwright's headless shell / chromium / google-chrome)
//! against a local console stand-in on 127.0.0.1:21406 (slot 270 ports 21400-21409).
//! Checks probe fields, cookie delivery (value never printed), page errors,
//! networkidle waiting for a late fetch, overflow, ASCII escaping, 390x844 PNGs,
//! report.json == stdout, nav failure exit, and that Chromium and its profile dir
//! are gone afterwards.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

const PORT: u16 = 21406;
const COOKIE: &str = "slot270-test-cookie-value";

fn page(path: &str, cookie_ok: bool) -> Option<(&'static str, String)> {
    let html = match path {
        "/console" => r#"<!doctype html><html><head><title>Galion Console</title></head><body>
<h1>Console</h1><h1>  Second  </h1>
<svg></svg><svg></svg><svg></svg>
<button aria-label="Use dark">t</button>
<p>Desk    Nerve
Defence</p><p id="late">waiting</p>
<script>
fetch("/slow").then(r => r.text()).then(t => { document.getElementById("late").textContent = t; });
throw new Error("boom-21406");
</script></body></html>"#
            .to_string(),
        "/auth" => format!(
            "<!doctype html><title>auth</title><h1>{}</h1>",
            if cookie_ok { "authed" } else { "anon" }
        ),
        "/wide" => "<!doctype html><title>wide</title><div style=\"width:2000px\">Instances Research</div><script>setTimeout(()=>{throw new TypeError(\"late-type\")},10)</script>".to_string(),
        "/uni" => "<!doctype html><meta charset=utf-8><title>Galion \u{b7} \u{17e}</title><h1>\u{17d}ivjo</h1>".to_string(),
        "/slow" => {
            std::thread::sleep(Duration::from_millis(700));
            return Some(("text/plain", "LATE-LOADED".into()));
        }
        _ => return None,
    };
    Some(("text/html; charset=utf-8", html))
}

fn serve(stop: Arc<AtomicBool>, hits: Arc<AtomicU32>) -> std::thread::JoinHandle<()> {
    let l = TcpListener::bind(("127.0.0.1", PORT)).expect("bind 21406");
    l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            match l.accept() {
                Ok((s, _)) => {
                    let hits = hits.clone();
                    std::thread::spawn(move || handle(s, hits));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    })
}

fn handle(s: TcpStream, hits: Arc<AtomicU32>) {
    s.set_nonblocking(false).unwrap();
    let mut r = BufReader::new(s.try_clone().unwrap());
    let mut line = String::new();
    if r.read_line(&mut line).is_err() {
        return;
    }
    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
    let mut cookie_ok = false;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
            break;
        }
        if h.to_ascii_lowercase().starts_with("cookie:")
            && h.contains(&format!("galion_admin={COOKIE}"))
        {
            cookie_ok = true;
        }
    }
    hits.fetch_add(1, Ordering::SeqCst);
    let (code, ct, body) = match page(&path, cookie_ok) {
        Some((ct, b)) => (200, ct, b),
        None => (404, "text/plain", "nope".to_string()),
    };
    let mut s = s;
    let _ = write!(
        s,
        "HTTP/1.1 {code} X\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn png_size(b: &[u8]) -> (u32, u32) {
    assert_eq!(&b[..8], b"\x89PNG\r\n\x1a\n");
    let w = u32::from_be_bytes(b[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(b[20..24].try_into().unwrap());
    (w, h)
}

fn chrome_children_of(pid: u32) -> usize {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            std::fs::read_to_string(e.path().join("stat"))
                .ok()
                .and_then(|s| {
                    s.rsplit_once(") ")
                        .map(|x| x.1.split(' ').nth(1).unwrap_or("").to_string())
                })
                .is_some_and(|ppid| ppid == pid.to_string())
        })
        .count()
}

#[test]
fn real_pw_check_against_local_console() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let dir = std::env::temp_dir().join(format!("real-pw-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cookie = dir.join("pw-cookie");
    std::fs::write(&cookie, format!("  {COOKIE}\n")).unwrap();
    let out = dir.join("out");
    let stop = Arc::new(AtomicBool::new(false));
    let hits = Arc::new(AtomicU32::new(0));
    let h = serve(stop.clone(), hits.clone());
    let base = format!("http://127.0.0.1:{PORT}");

    let t0 = std::time::Instant::now();
    let child = Command::new(env!("CARGO_BIN_EXE_pw-check"))
        .args(["/console", "auth", "/wide", "/uni", "--base", &base])
        .arg("--out-dir")
        .arg(&out)
        .arg("--cookie-file")
        .arg(&cookie)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    let res = child.wait_with_output().unwrap();
    let took = t0.elapsed();
    let stdout = String::from_utf8_lossy(&res.stdout).to_string();
    let stderr = String::from_utf8_lossy(&res.stderr).to_string();
    assert!(res.status.success(), "stderr: {stderr}");
    assert!(!stdout.contains(COOKIE) && !stderr.contains(COOKIE));
    assert!(stdout.is_ascii());
    let report_file = std::fs::read_to_string(out.join("report.json")).unwrap();
    assert_eq!(stdout.trim_end(), report_file);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let rows = v.as_array().unwrap();
    assert_eq!(rows.len(), 4);
    let keys: Vec<&String> = rows[0].as_object().unwrap().keys().collect();
    let mut want = [
        "title", "h1", "svgs", "theme", "dock", "wide", "text", "path", "errors", "shot",
    ];
    want.sort();
    let mut got: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
    got.sort();
    assert_eq!(got, want);
    // key order as printed (Python dict order)
    let order: Vec<usize> = [
        "\"title\"",
        "\"h1\"",
        "\"svgs\"",
        "\"theme\"",
        "\"dock\"",
        "\"wide\"",
        "\"text\"",
        "\"path\"",
        "\"errors\"",
        "\"shot\"",
    ]
    .iter()
    .map(|k| stdout.find(k).unwrap())
    .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");

    let c = &rows[0];
    assert_eq!(c["title"], "Galion Console");
    assert_eq!(c["h1"], serde_json::json!(["Console", "Second"]));
    assert_eq!(c["svgs"], 3);
    assert_eq!(c["theme"], "Use dark");
    assert_eq!(c["dock"], serde_json::json!(["Desk", "Nerve", "Defence"]));
    assert_eq!(c["wide"], false);
    assert!(
        c["text"].as_str().unwrap().contains("Desk Nerve Defence"),
        "{}",
        c["text"]
    );
    assert!(
        c["text"].as_str().unwrap().contains("LATE-LOADED"),
        "networkidle must wait for /slow: {}",
        c["text"]
    );
    assert_eq!(c["errors"], serde_json::json!(["boom-21406"]));
    assert_eq!(c["path"], "/console");
    assert_eq!(c["shot"], out.join("console.png").display().to_string());

    assert_eq!(
        rows[1]["h1"],
        serde_json::json!(["authed"]),
        "cookie must reach the server"
    );
    assert_eq!(rows[1]["path"], "auth");
    assert_eq!(rows[1]["theme"], serde_json::Value::Null);
    assert_eq!(rows[2]["wide"], true);
    assert_eq!(
        rows[2]["dock"],
        serde_json::json!(["Instances", "Research"])
    );
    assert_eq!(rows[2]["errors"], serde_json::json!(["late-type"]));
    assert_eq!(rows[3]["title"], "Galion \u{b7} \u{17e}");
    assert!(stdout.contains("\"Galion \\u00b7 \\u017e\""));
    for n in ["console", "auth", "wide", "uni"] {
        let b = std::fs::read(out.join(format!("{n}.png"))).unwrap();
        assert_eq!(png_size(&b), (390, 844), "{n}.png");
    }
    assert_eq!(chrome_children_of(pid), 0);
    let leftover = std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .flatten()
        .any(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(&format!("pw-check-{pid}-"))
        });
    assert!(!leftover, "chromium profile dir left behind");
    eprintln!(
        "S1 ok: 4 pages in {took:?}, {} requests served",
        hits.load(Ordering::SeqCst)
    );

    // S2: no cookie file -> page sees anon.
    let res = Command::new(env!("CARGO_BIN_EXE_pw-check"))
        .args(["/auth", "--base", &base])
        .arg("--out-dir")
        .arg(&out)
        .arg("--cookie-file")
        .arg(dir.join("absent"))
        .output()
        .unwrap();
    assert!(res.status.success());
    let v: serde_json::Value = serde_json::from_slice(&res.stdout).unwrap();
    assert_eq!(v[0]["h1"], serde_json::json!(["anon"]));
    eprintln!("S2 ok: no cookie -> anon");

    // S3: nothing listening -> exit 1 with the net error, like goto() raising.
    let res = Command::new(env!("CARGO_BIN_EXE_pw-check"))
        .args(["/console", "--base", "http://127.0.0.1:21407"])
        .arg("--out-dir")
        .arg(dir.join("out3"))
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&res.stderr);
    assert_eq!(res.status.code(), Some(1));
    assert!(err.contains("ERR_CONNECTION_REFUSED"), "{err}");
    assert!(!dir.join("out3/report.json").exists());
    eprintln!("S3 ok: {}", err.trim());

    stop.store(true, Ordering::SeqCst);
    h.join().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
