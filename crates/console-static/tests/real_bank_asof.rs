// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life probe for wave 4 slot 371 (console-bank-asof, F-351-5 +
//! F-351-3): the Rust `/bank` page now honours the reader's `asOf` /
//! `ageS` / `stale` / `refreshS`, ages purses that come from the one-shot
//! `survival.json` on their own 15-min clock, and reloads itself on the
//! refresh interval (`<meta http-equiv="refresh">`, no JavaScript).
//!
//! Starts the real release `galion-console-static` (`--dist`, `--ui rust`,
//! `--data-upstream` = an in-test fake bank reader whose answer the test
//! swaps between requests), ports 22410-22419.
//!
//! Optional live leg (`CONSOLE_BANK_LIVE=1`, `CONSOLE_BANK_VPS=host`, default
//! `slavik-vps`): READ-ONLY. `ssh stat` of the live `survival.json` mtime
//! (no content read) fed to the fake reader as the survival purse's `asOf`,
//! and an `ssh -L 22419 -> VPS 127.0.0.1:18080` tunnel (live Rust console
//! primary) as `--data-upstream`; GETs only, nothing on the VPS changes.
//!
//! `GALION_REAL=1 cargo test --release -p console-static --test real_bank_asof -- --nocapture --test-threads=1`

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// ISO with a fixed `+01:00` offset (a valid instant whatever the season;
/// the page converts it to the Zagreb clock itself).
fn iso_cet(unix: i64) -> String {
    let t = unix + 3_600;
    let (y, m, d) = civil(t.div_euclid(86_400));
    let s = t.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+01:00",
        s / 3600,
        (s % 3600) / 60,
        s % 60
    )
}

fn get(addr: &str, path: &str) -> (u16, String, String, u128) {
    let t = Instant::now();
    let mut s = TcpStream::connect(addr).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let ms = t.elapsed().as_millis();
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let code = head
        .split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (code, head.to_string(), body.to_string(), ms)
}

type Body = Arc<Mutex<Option<String>>>;

/// Fake bank reader: `GET /api/v1/bank` -> 200 with the current body, or
/// 503 `warming` when the body is `None` (the reader's pre-first-refresh
/// answer from the slot 351 design).
fn fake_reader(addr: &'static str, body: Body, hits: Arc<Mutex<u32>>) {
    let l = TcpListener::bind(addr).expect("bind fake reader");
    thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let mut buf = [0u8; 8192];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = req.lines().next().unwrap_or("").to_string();
            let path = line.split(' ').nth(1).unwrap_or("");
            let (code, out) = if line.starts_with("GET ") && path == "/api/v1/bank" {
                *hits.lock().unwrap() += 1;
                match body.lock().unwrap().clone() {
                    Some(b) => (200, b),
                    None => (503, r#"{"ok":false,"warming":true}"#.to_string()),
                }
            } else {
                (404, r#"{"error":"not found"}"#.to_string())
            };
            let _ = write!(
                s,
                "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
                out.len()
            );
        }
    });
}

fn dist_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("real-371-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("static/assets")).unwrap();
    std::fs::write(d.join("static/assets/app.css"), "body{}").unwrap();
    d
}

fn start(bind: &str, dist: &Path, up: &str) -> Kill {
    let child = Command::new(env!("CARGO_BIN_EXE_galion-console-static"))
        .args(["--ui", "rust", "--bind", bind, "--quiet", "--dist"])
        .arg(dist)
        .args(["--data-upstream", up, "--data-path", "/api/v1/bank"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn console");
    let g = Kill(child);
    let t0 = Instant::now();
    while TcpStream::connect(bind).is_err() {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "console did not start"
        );
        thread::sleep(Duration::from_millis(50));
    }
    g
}

fn bank_page(bind: &str) -> String {
    let r = get(bind, "/bank");
    assert_eq!(r.0, 200, "{}", r.1);
    assert!(r.1.to_ascii_lowercase().contains("content-type: text/html"));
    assert!(!r.2.contains("<script"), "script on /bank");
    println!("  /bank {} bytes {} ms", r.2.len(), r.3);
    r.2
}

fn meta(html: &str) -> Option<String> {
    let i = html.find("<meta http-equiv=\"refresh\" content=\"")? + 36;
    Some(html[i..].split('"').next()?.to_string())
}

fn snap(as_of: Option<i64>, extra: &str, purses: &str) -> String {
    let at = as_of.map_or(String::new(), |t| format!(",\"asOf\":\"{}\"", iso_cet(t)));
    format!(
        r#"{{"purses":[{purses}],"totalUsd":9.5,"weekTokensIn":1000,"weekTokensOut":20,"weekCostUsd":1.4,"dailyUsd":0.2,"daysLeft":47.5,"riverIfMovedDailyUsd":0.1,"note":"slot371 fake reader"{at}{extra}}}"#
    )
}

const OR: &str = r#"{"name":"OpenRouter","usd":9.5,"measured":true,"note":"key limit left"}"#;

#[test]
fn real_bank_page_honours_as_of() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    const BIND: &str = "127.0.0.1:22410";
    const UP: &str = "127.0.0.1:22411";
    let body: Body = Arc::new(Mutex::new(None));
    let hits = Arc::new(Mutex::new(0u32));
    fake_reader(UP, Arc::clone(&body), Arc::clone(&hits));
    let dist = dist_dir("asof");
    let _g = start(BIND, &dist, UP);

    // 1) Warming reader (503): explained, page still reloads every 5 min.
    println!("1) reader warming (503)");
    let h = bank_page(BIND);
    assert!(h.contains("/api/v1/bank answered HTTP 503."));
    assert!(h.contains("This page reloads every 5 min."));
    assert_eq!(meta(&h).as_deref(), Some("300"));
    assert!(!h.contains("Snapshot"));

    // 2) Fresh snapshot, 2 min old, reader every 120 s.
    println!("2) fresh snapshot, asOf now-120 s, refreshS 120");
    let t = now();
    *body.lock().unwrap() = Some(snap(Some(t - 120), r#","refreshS":120"#, OR));
    let h = bank_page(BIND);
    assert_eq!(
        meta(&h).as_deref(),
        Some("120"),
        "meta refresh follows refreshS"
    );
    assert!(h.contains("$9.50</span><span>OpenRouter"));
    let line = h
        .split("<p class=\"note\">Snapshot as of ")
        .nth(1)
        .and_then(|s| s.split("</p>").next())
        .expect("snapshot line");
    println!("  line: Snapshot as of {line}");
    assert!(line.contains(" CET (+01:00) · ") || line.contains(" CEST (+02:00) · "));
    assert!(
        line.contains("2 min ago.") || line.contains("120 s ago."),
        "{line}"
    );
    assert!(line.contains("The reader refreshes every 2 min; this page reloads every 2 min."));
    assert!(!h.contains("Stale"));

    // 3) Same reader, snapshot now 20 min old (reader stuck): warning,
    //    figures still shown.
    println!("3) stuck reader, asOf now-1200 s, refreshS 300");
    *body.lock().unwrap() = Some(snap(Some(now() - 1_200), r#","refreshS":300"#, OR));
    let h = bank_page(BIND);
    assert_eq!(meta(&h).as_deref(), Some("300"));
    assert!(
        h.contains("Stale: the last snapshot is 20 min old, more than 3 refreshes of 5 min."),
        "{h}"
    );
    assert!(h.contains("$9.50</span><span>total"));

    // 4) Reader flag only (ageS + stale:true), no asOf.
    println!("4) ageS 30 + stale:true");
    *body.lock().unwrap() = Some(snap(None, r#","ageS":30,"stale":true"#, OR));
    let h = bank_page(BIND);
    assert!(h.contains("Snapshot 30 s old."));
    assert!(h.contains("the reader marks this snapshot as stale"));

    // 5) Old reader shape (no time keys): says so.
    println!("5) no time keys (pre-F-351-5 reader)");
    *body.lock().unwrap() = Some(snap(None, "", OR));
    let h = bank_page(BIND);
    assert!(h.contains("sent no snapshot time (asOf)"));
    assert_eq!(meta(&h).as_deref(), Some("300"));

    // 6) Survival purse (F-351-3): fresh snapshot, but the survival figure
    //    is 10 h old -> that purse alone is marked stale.
    println!("6) survival purse 10 h old in a fresh snapshot");
    let t = now();
    let purses = format!(
        r#"{OR},{{"name":"Runpod","usd":3,"measured":true,"note":"from survival probe","source":"survival","asOf":"{}"}},{{"name":"Spare","usd":1,"source":"survival","asOf":"{}"}}"#,
        iso_cet(t - 36_000),
        iso_cet(t - 600)
    );
    *body.lock().unwrap() = Some(snap(Some(t - 10), "", &purses));
    let h = bank_page(BIND);
    let runpod = h
        .split("<span>Runpod</span><span class=\"muted\">")
        .nth(1)
        .and_then(|s| s.split("</span>").next())
        .expect("runpod note");
    println!("  Runpod note: {runpod}");
    assert!(runpod.starts_with("from survival probe · stale · as of "));
    assert!(runpod.ends_with(", 10.0 h old"));
    let spare = h
        .split("<span>Spare</span><span class=\"muted\">")
        .nth(1)
        .and_then(|s| s.split("</span>").next())
        .unwrap();
    println!("  Spare note: {spare}");
    assert!(!spare.contains("stale") && spare.ends_with(", 10 min old"));
    assert!(!h.contains("Stale:"), "snapshot itself is fresh");

    // 7) Each page load re-reads the reader (no console-side cache), and
    //    other rooms carry no meta refresh.
    let before = *hits.lock().unwrap();
    for _ in 0..5 {
        bank_page(BIND);
    }
    assert_eq!(*hits.lock().unwrap(), before + 5, "one reader GET per load");
    let a = get(BIND, "/arena");
    assert_eq!(a.0, 200);
    assert!(meta(&a.2).is_none(), "/arena must not auto-reload");
    println!("7) 5 loads -> 5 reader GETs; /arena has no meta refresh");
    let _ = std::fs::remove_dir_all(&dist);
}

#[test]
fn real_bank_asof_live_readonly() {
    if !real() || std::env::var("CONSOLE_BANK_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 CONSOLE_BANK_LIVE=1 [CONSOLE_BANK_VPS=host]");
        return;
    }
    const BIND: &str = "127.0.0.1:22416";
    const FAKE: &str = "127.0.0.1:22417";
    const BIND2: &str = "127.0.0.1:22418";
    const UP: &str = "127.0.0.1:22419";
    let vps = std::env::var("CONSOLE_BANK_VPS").unwrap_or_else(|_| "slavik-vps".into());
    let ssh = |args: &[&str]| {
        Command::new("ssh")
            .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=15"])
            .args(args)
            .stdin(Stdio::null())
            .output()
            .expect("ssh")
    };
    // 1) Live survival.json age: mtime only, the content is not read.
    let out = ssh(&[
        &vps,
        "stat -c %Y /workspace/galion-light/state/survival.json",
    ]);
    assert!(out.status.success(), "ssh stat failed");
    let mtime: i64 = String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("mtime");
    let t = now();
    println!(
        "live survival.json mtime {} = {} h ago",
        iso_cet(mtime),
        (t - mtime) / 3600
    );
    let body: Body = Arc::new(Mutex::new(Some(snap(
        Some(t),
        "",
        &format!(
            r#"{OR},{{"name":"Runpod","usd":null,"measured":false,"note":"from survival probe","source":"survival","asOf":"{}"}}"#,
            iso_cet(mtime)
        ),
    ))));
    fake_reader(FAKE, body, Arc::new(Mutex::new(0)));
    let dist = dist_dir("live");
    {
        let _g = start(BIND, &dist, FAKE);
        let h = bank_page(BIND);
        let runpod = h
            .split("<span>Runpod</span><span class=\"muted\">")
            .nth(1)
            .and_then(|s| s.split("</span>").next())
            .unwrap();
        println!("  Runpod note with live survival age: {runpod}");
        if t - mtime > 2_700 {
            assert!(runpod.contains("stale · as of "), "{runpod}");
        }
    }

    // 2) Live Rust console primary as the data upstream (GET only).
    let tunnel = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ExitOnForwardFailure=yes"])
        .args(["-N", "-L", &format!("{UP}:127.0.0.1:18080"), &vps])
        .stdin(Stdio::null())
        .spawn()
        .expect("ssh -L");
    let _t = Kill(tunnel);
    let t0 = Instant::now();
    while TcpStream::connect(UP).is_err() || get(UP, "/api/v1/health").0 != 200 {
        assert!(
            t0.elapsed() < Duration::from_secs(25),
            "tunnel did not come up"
        );
        thread::sleep(Duration::from_millis(200));
    }
    let (lb, _, lbb, _) = get(UP, "/api/v1/bank");
    println!("live :18080 /api/v1/bank -> {lb} {:?}", lbb.trim());
    let _g = start(BIND2, &dist, UP);
    let h = bank_page(BIND2);
    assert_eq!(meta(&h).as_deref(), Some("300"));
    if lb == 404 {
        assert!(h.contains("no /api/v1/bank route yet"));
        assert!(h.contains("This page reloads every 5 min."));
    } else if lb == 200 {
        assert!(h.contains("Snapshot") || h.contains("sent no snapshot time"));
    }
    let _ = std::fs::remove_dir_all(&dist);
}
