// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life probe for wave 4 slot 408 (console-survival-card, Console V2
//! prep, NOT the live cutover): the Rust `/bank` page carries a survival
//! card fed by the staged `GET /api/v1/survival` (slot 389). It shows the
//! `survival.json` age and `asOf` on the Zagreb clock, a stale state and the
//! error states, and never the file content.
//!
//! Starts the real release `galion-console-static` on ports 22780-22789:
//! `--dist --ui rust --survival-file` (self-upstream) for the fresh / stale /
//! missing / writer-failed / real-writer states, a console without
//! `--survival-file` (404 -> "off"), and `--ui rust --upstream` pointing at an
//! in-test fake API (hostile/leaky 200, 502, 401) or at a dead port (down).
//! The real writer is the release `galion-survival` with an empty key file
//! (no network, nothing spent).
//!
//! Optional live leg (`CONSOLE_SURVIVAL_LIVE=1`, `CONSOLE_SURVIVAL_VPS=host`,
//! default `slavik-vps`): READ-ONLY `ssh stat` of the live `survival.json`
//! mtime (content never read) copied onto a local placeholder; the card must
//! show that instant on the Zagreb clock and call it stale.
//!
//! `GALION_REAL=1 cargo test --release -p console-static --test real_survival_card -- --nocapture --test-threads=1`

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

fn mtime(p: &Path) -> Option<i64> {
    let m = std::fs::metadata(p).ok()?.modified().ok()?;
    Some(m.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64)
}

fn set_mtime(p: &Path, unix: i64) {
    let f = std::fs::File::options().write(true).open(p).unwrap();
    f.set_modified(UNIX_EPOCH + Duration::from_secs(unix as u64))
        .unwrap();
}

fn get_host(addr: &str, path: &str, host: &str) -> (u16, String, u128) {
    let t = Instant::now();
    let mut s = TcpStream::connect(addr).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let code = head
        .split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (code, body.to_string(), t.elapsed().as_millis())
}

fn get(addr: &str, path: &str) -> (u16, String) {
    let (c, b, _) = get_host(addr, path, "127.0.0.1");
    (c, b)
}

/// The survival card out of a `/bank` page.
fn card(page: &str) -> String {
    let i = page
        .find("<section aria-label=\"Survival\"")
        .unwrap_or_else(|| panic!("no survival card in {page}"));
    let rest = &page[i..];
    let j = rest.find("</section>").expect("card end") + "</section>".len();
    rest[..j].to_string()
}

fn state(card: &str) -> String {
    card.split("data-state=\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .unwrap_or("")
        .to_string()
}

/// Visible text of the card (tags dropped), for the evidence log.
fn text(card: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for c in card.chars() {
        match c {
            '<' => {
                tag = true;
                out.push(' ');
            }
            '>' => tag = false,
            c if !tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What the survival.json placeholder holds: cash-like content and a decoy
/// key that must never reach the page.
const CONTENT: &str = r#"{"when":"2026-10-27T18:19:50+01:00","openrouter":{"usd":4321.98,"key":"sk-or-v1-decoy408"},"revolut":{"usd":8765.43},"cpu":{"load":17.77},"lane":"zai-org/GLM-5.3-Flash"}"#;
const LEAKS: &[&str] = &[
    "4321.98",
    "sk-or-v1",
    "decoy408",
    "8765.43",
    "17.77",
    "GLM-5.3",
    "revolut",
    "openrouter",
];

fn no_leak(page: &str) {
    for l in LEAKS {
        assert!(!page.contains(l), "content leaked ({l})");
    }
}

struct Root {
    base: PathBuf,
}

impl Root {
    fn new(tag: &str) -> Root {
        let base = std::env::temp_dir().join(format!("real-408-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("dist/static/assets")).unwrap();
        std::fs::create_dir_all(base.join("light/state")).unwrap();
        std::fs::create_dir_all(base.join("hermes")).unwrap();
        std::fs::write(base.join("dist/static/assets/app.css"), "body{}").unwrap();
        std::fs::write(
            base.join("light/state/hardware.json"),
            r#"{"processors":4,"gpu":{"name":null},"line":"CPU only."}"#,
        )
        .unwrap();
        std::fs::write(
            base.join("hermes/modes.json"),
            r#"{"modes":{"action":"zai-org/GLM-5.3-Flash"}}"#,
        )
        .unwrap();
        std::fs::write(base.join("empty.env"), "").unwrap();
        Root { base }
    }
    fn file(&self) -> PathBuf {
        self.base.join("light/state/survival.json")
    }
    fn put(&self, unix: i64) {
        std::fs::write(self.file(), CONTENT).unwrap();
        set_mtime(&self.file(), unix);
    }
    fn writer_env(&self) -> Vec<String> {
        [
            format!("GALION_LIGHT_ROOT={}", self.base.join("light").display()),
            format!("GALION_HERMES_ROOT={}", self.base.join("hermes").display()),
            format!(
                "OPENROUTER_KEY_FILE={}",
                self.base.join("empty.env").display()
            ),
            "GALION_SURVIVAL_DISK=/".to_string(),
        ]
        .into_iter()
        .flat_map(|e| ["--survival-env".to_string(), e])
        .collect()
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn spawn(bind: &str, args: &[String]) -> Kill {
    let child = Command::new(env!("CARGO_BIN_EXE_galion-console-static"))
        .args(["--bind", bind, "--quiet"])
        .args(args)
        .env("OPENROUTER_API_KEY", "decoy-408-not-a-key")
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

fn start(bind: &str, root: &Root, extra: &[String]) -> Kill {
    let mut args: Vec<String> = vec![
        "--ui".into(),
        "rust".into(),
        "--dist".into(),
        root.base.join("dist").display().to_string(),
        "--survival-file".into(),
        root.file().display().to_string(),
    ];
    args.extend_from_slice(extra);
    spawn(bind, &args)
}

/// GET /bank, check it, return the card.
fn bank_card(bind: &str, label: &str) -> String {
    let (c, page, ms) = get_host(bind, "/bank", "127.0.0.1");
    assert_eq!(c, 200, "{label}: {page}");
    no_leak(&page);
    assert!(
        !page.contains("decoy-408-not-a-key"),
        "{label}: env key on page"
    );
    let k = card(&page);
    println!(
        "  {label}: /bank 200 {ms} ms, card [{}] {}",
        state(&k),
        text(&k)
    );
    k
}

fn wait_idle(bind: &str) {
    let t0 = Instant::now();
    loop {
        let (_, b) = get(bind, "/api/v1/survival");
        if b.contains("\"running\":false") {
            return;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(60),
            "writer still running"
        );
        thread::sleep(Duration::from_millis(100));
    }
}

/// The real writer, built in release next to this test.
fn writer_bin() -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let st = Command::new(cargo)
        .args(["build", "--release", "-j", "2", "-p", "galion-survival"])
        .status()
        .expect("cargo build galion-survival");
    assert!(st.success());
    let exe = std::env::current_exe().unwrap();
    let target = exe.parent().unwrap().parent().unwrap().parent().unwrap();
    let w = target.join("release/galion-survival");
    assert!(w.is_file(), "{}", w.display());
    w
}

/// Fake console API: `/api/v1/survival` answers the swappable reply, all
/// else 404.
fn fake_api(bind: &str, reply: Arc<Mutex<(u16, String)>>) {
    let l = TcpListener::bind(bind).expect("bind fake");
    thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let reply = Arc::clone(&reply);
            thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = head.split(' ').nth(1).unwrap_or("").to_string();
                let (code, body) = if path == "/api/v1/survival" {
                    reply.lock().unwrap().clone()
                } else {
                    (404, "{\"error\":\"not found\"}".to_string())
                };
                let _ = write!(
                    s,
                    "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
}

#[test]
fn survival_card_states() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let t0 = Instant::now();

    // 1. Stale: the live one-shot instant 2026-10-27 18:19:50 CET.
    println!("1) status-only, stale file (mtime 2026-10-27 18:19:50 +01:00)");
    {
        let r = Root::new("stale");
        r.put(1_793_121_590);
        let _g = start("127.0.0.1:22780", &r, &[]);
        let k = bank_card("127.0.0.1:22780", "stale");
        assert_eq!(state(&k), "stale");
        assert!(k.contains(">2026-10-27 18:19 CET (+01:00)<"), "{k}");
        assert!(k.contains("status only (no writer)"), "{k}");
        assert!(k.contains("more than 3 refreshes of 15 min"), "{k}");
        assert!(k.contains("t-bad"), "{k}");
        assert_eq!(
            mtime(&r.file()),
            Some(1_793_121_590),
            "status only: no write"
        );
        // HEAD /bank and the status route agree with the card.
        let (c, s) = get("127.0.0.1:22780", "/api/v1/survival");
        assert_eq!(c, 200);
        assert!(
            s.contains("\"asOf\":\"2026-10-27T18:19:50+01:00\"") && s.contains("\"stale\":true"),
            "{s}"
        );
        no_leak(&s);
    }

    // 2. Fresh (written 2 min ago) and the summer-time side.
    println!("2) fresh file (2 min old) and a CEST instant");
    {
        let r = Root::new("fresh");
        r.put(now() - 120);
        let _g = start("127.0.0.1:22781", &r, &[]);
        let k = bank_card("127.0.0.1:22781", "fresh");
        assert_eq!(state(&k), "fresh");
        assert!(
            k.contains(">2 min<") || k.contains(">120 s<") || k.contains(">121 s<"),
            "{k}"
        );
        assert!(!k.contains("t-bad"), "{k}");
        r.put(1_792_352_150); // 2026-10-18 21:35:50 CEST, last Python cron run
        let k = bank_card("127.0.0.1:22781", "cest");
        assert!(k.contains(">2026-10-18 21:35 CEST (+02:00)<"), "{k}");
        assert_eq!(state(&k), "stale");
        // Future mtime: skew, not "fresh".
        r.put(now() + 7_200);
        let k = bank_card("127.0.0.1:22781", "skew");
        assert_eq!(state(&k), "skew");
        assert!(k.contains("ahead (clock skew)"), "{k}");
    }

    // 3. Missing file.
    println!("3) missing file");
    {
        let r = Root::new("missing");
        let _g = start("127.0.0.1:22782", &r, &[]);
        let k = bank_card("127.0.0.1:22782", "missing");
        assert_eq!(state(&k), "missing");
        assert!(k.contains("survival.json is missing"), "{k}");
    }

    // 4. Writer failures: exits 1, cannot start.
    println!("4) writer /bin/false (exit 1) and a missing writer binary");
    {
        let r = Root::new("fail");
        r.put(1_793_121_590);
        let _g = start(
            "127.0.0.1:22783",
            &r,
            &["--survival-writer".into(), "/bin/false".into()],
        );
        let _ = get("127.0.0.1:22783", "/bank"); // triggers the run
        wait_idle("127.0.0.1:22783");
        let k = bank_card("127.0.0.1:22783", "exit1");
        assert_eq!(state(&k), "stale");
        assert!(k.contains(": exit 1"), "{k}");
        assert!(
            k.contains("The last writer run failed (exit 1); the console waits 15 min"),
            "{k}"
        );
        assert!(k.contains("1 runs, 1 failed · on-request"), "{k}");
        // 20 more page loads inside the back-off: still one run.
        for _ in 0..20 {
            let _ = get("127.0.0.1:22783", "/bank");
        }
        let k = bank_card("127.0.0.1:22783", "backoff");
        assert!(k.contains("1 runs, 1 failed"), "{k}");
    }
    {
        let r = Root::new("nowriter");
        r.put(1_793_121_590);
        let w = r.base.join("no-such-writer").display().to_string();
        let _g = start("127.0.0.1:22784", &r, &["--survival-writer".into(), w]);
        let _ = get("127.0.0.1:22784", "/bank");
        wait_idle("127.0.0.1:22784");
        let k = bank_card("127.0.0.1:22784", "spawn-error");
        assert!(k.contains(": could not start"), "{k}");
    }

    // 5. Real writer: stale -> refreshed by galion-survival -> fresh card.
    println!("5) real galion-survival writer (empty key file, no network)");
    {
        let w = writer_bin();
        let r = Root::new("real");
        r.put(1_793_121_590);
        let mut extra = vec![
            "--survival-writer".to_string(),
            w.display().to_string(),
            "--survival-refresh-s".into(),
            "900".into(),
        ];
        extra.extend(r.writer_env());
        let _g = start("127.0.0.1:22785", &r, &extra);
        let first = bank_card("127.0.0.1:22785", "first-load");
        // The first load triggered the run; the card was rendered either
        // before it (stale) or while/after it.
        assert!(
            ["stale", "fresh"].contains(&state(&first).as_str()),
            "{first}"
        );
        wait_idle("127.0.0.1:22785");
        let k = bank_card("127.0.0.1:22785", "after-run");
        assert_eq!(state(&k), "fresh", "{k}");
        assert!(k.contains(": ok<"), "{k}");
        assert!(k.contains("1 runs, 0 failed · on-request"), "{k}");
        let m = mtime(&r.file()).unwrap();
        assert!(now() - m < 60, "file rewritten");
        // The real writer's file has cash/cpu content; none of it on the page.
        let real = std::fs::read_to_string(r.file()).unwrap();
        assert!(real.contains("\"cpu\""), "real writer content present");
        let (_, page) = get("127.0.0.1:22785", "/bank");
        assert!(
            !page.contains("\"cpu\"") && !page.contains("measured"),
            "content on page"
        );
    }

    // 6. Without --survival-file: the route is 404 -> card "off".
    println!("6) console without --survival-file");
    {
        let r = Root::new("off");
        let _g = spawn(
            "127.0.0.1:22786",
            &[
                "--ui".into(),
                "rust".into(),
                "--dist".into(),
                r.base.join("dist").display().to_string(),
            ],
        );
        let (c, _) = get("127.0.0.1:22786", "/api/v1/survival");
        println!("  GET /api/v1/survival -> {c}");
        let k = bank_card("127.0.0.1:22786", "off");
        assert_eq!(state(&k), "off");
        assert!(k.contains("--survival-file"), "{k}");
    }

    // 7. Fake upstream: leaky/hostile 200, 502, 401; then a dead upstream.
    println!("7) --upstream fake API (leaky 200, 502, 401) and a dead upstream");
    {
        let reply = Arc::new(Mutex::new((200u16, String::new())));
        fake_api("127.0.0.1:22787", Arc::clone(&reply));
        let up = [
            "--ui".to_string(),
            "rust".into(),
            "--upstream".into(),
            "127.0.0.1:22787".into(),
        ];
        let _g = spawn("127.0.0.1:22788", &up);
        *reply.lock().unwrap() = (
            200,
            format!(
                r#"{{"exists":true,"asOf":"<script>alert(408)</script>","ageS":60,"mode":"<b>x</b>","writer":"<img src=x>","content":{CONTENT},"file":"/opt/galion/galion-light/state/survival.json"}}"#
            ),
        );
        let k = bank_card("127.0.0.1:22788", "leaky-200");
        for bad in ["<script", "alert(408)", "<b>", "<img", "/opt/galion"] {
            assert!(!k.contains(bad), "{bad} in {k}");
        }
        assert_eq!(state(&k), "fresh");
        assert!(k.contains(">60 s<"), "{k}");
        *reply.lock().unwrap() = (502, "{\"error\":\"bad gateway\"}".into());
        let k = bank_card("127.0.0.1:22788", "502");
        assert_eq!(state(&k), "error");
        assert!(k.contains("HTTP 502"), "{k}");
        *reply.lock().unwrap() = (401, "{\"error\":\"phone required\"}".into());
        let k = bank_card("127.0.0.1:22788", "401");
        assert_eq!(state(&k), "gated");
        *reply.lock().unwrap() = (200, "not json".into());
        let k = bank_card("127.0.0.1:22788", "not-json");
        assert_eq!(state(&k), "down");
    }
    {
        // Nothing listens on 22789 except the console itself? No: point the
        // UI at a port nobody holds.
        let dead = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = dead.local_addr().unwrap().to_string();
        drop(dead);
        let _g = spawn(
            "127.0.0.1:22789",
            &["--ui".into(), "rust".into(), "--upstream".into(), addr],
        );
        let k = bank_card("127.0.0.1:22789", "down");
        assert_eq!(state(&k), "down");
        assert!(k.contains("did not answer"), "{k}");
    }

    // 8. Live leg: READ-ONLY mtime of the VPS file.
    if std::env::var("CONSOLE_SURVIVAL_LIVE").as_deref() == Ok("1") {
        let host = std::env::var("CONSOLE_SURVIVAL_VPS").unwrap_or_else(|_| "slavik-vps".into());
        let o = Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                &host,
                "stat -c %Y /opt/galion/galion-light/state/survival.json",
            ])
            .output()
            .expect("ssh");
        let live: i64 = String::from_utf8_lossy(&o.stdout)
            .trim()
            .parse()
            .expect("mtime");
        println!(
            "8) live survival.json mtime {live} ({:.1} h old)",
            (now() - live) as f64 / 3600.0
        );
        let r = Root::new("live");
        r.put(live);
        let _g = start("127.0.0.1:22780", &r, &[]);
        let k = bank_card("127.0.0.1:22780", "live-mtime");
        let expect = if now() - live > 2_700 {
            "stale"
        } else {
            "fresh"
        };
        assert_eq!(state(&k), expect, "{k}");
    }
    println!("all states ok in {} ms", t0.elapsed().as_millis());
}
