// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life probe for wave 4 slot 389 (survival-refresh-writer, F-371-1):
//! the console keeps `survival.json` fresh itself, without a systemd timer,
//! by running the real Rust writer `galion-survival` when the file is due.
//!
//! The test builds the release `galion-survival`, starts the real release
//! `galion-console-static` (`--dist --ui rust`) on ports 22590-22597 and
//! points both at throwaway roots under the temp dir. The OpenRouter key
//! file is an empty temp file and the console's own environment carries a
//! decoy `OPENROUTER_API_KEY`; the writer must not see it (cleared env), so
//! no network call is made and nothing is spent.
//!
//! Optional live leg (`CONSOLE_SURVIVAL_LIVE=1`, `CONSOLE_SURVIVAL_VPS=host`,
//! default `slavik-vps`): READ-ONLY `ssh stat` of the live `survival.json`
//! mtime (content never read), copied onto a local placeholder, and the
//! status route must call it stale with the same age.
//!
//! `GALION_REAL=1 cargo test --release -p console-static --test real_survival_refresh -- --nocapture --test-threads=1`

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
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

fn get(addr: &str, path: &str) -> (u16, String) {
    let mut s = TcpStream::connect(addr).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
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
    (code, body.to_string())
}

/// Tiny field reader for the flat status JSON (no serde in this crate).
fn field(json: &str, key: &str) -> String {
    let pat = format!("\"{key}\":");
    let i = json.find(&pat).unwrap_or_else(|| panic!("{key} in {json}")) + pat.len();
    let rest = &json[i..];
    if let Some(r) = rest.strip_prefix('"') {
        r.split('"').next().unwrap().to_string()
    } else {
        rest.split([',', '}']).next().unwrap().to_string()
    }
}

fn status(bind: &str) -> String {
    let (c, b) = get(bind, "/api/v1/survival");
    assert_eq!(c, 200, "{b}");
    for leak in ["\"cpu\"", "\"rules\"", "openrouter", "cash"] {
        assert!(!b.contains(leak), "status echoes content ({leak}): {b}");
    }
    assert!(!b.contains("Z\""), "Zulu stamp in {b}");
    b
}

/// Build the real writer (release) next to this test's target dir.
fn writer_bin() -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let st = Command::new(cargo)
        .args(["build", "--release", "-j", "2", "-p", "galion-survival"])
        .status()
        .expect("cargo build galion-survival");
    assert!(st.success(), "build galion-survival");
    // .../target/<profile>/deps/real_survival_refresh-xxxx -> .../target/release
    let exe = std::env::current_exe().unwrap();
    let target = exe.parent().unwrap().parent().unwrap().parent().unwrap();
    let w = target.join("release/galion-survival");
    assert!(w.is_file(), "{}", w.display());
    w
}

struct Roots {
    base: PathBuf,
    light: PathBuf,
    hermes: PathBuf,
    key: PathBuf,
}

impl Roots {
    fn new(tag: &str) -> Roots {
        let base = std::env::temp_dir().join(format!("real-389-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let light = base.join("light");
        let hermes = base.join("hermes");
        std::fs::create_dir_all(light.join("state")).unwrap();
        std::fs::create_dir_all(&hermes).unwrap();
        std::fs::create_dir_all(base.join("dist/static/assets")).unwrap();
        std::fs::write(base.join("dist/static/assets/app.css"), "body{}").unwrap();
        std::fs::write(
            light.join("state/hardware.json"),
            r#"{"processors":4,"gpu":{"name":null},"line":"CPU only."}"#,
        )
        .unwrap();
        std::fs::write(
            hermes.join("modes.json"),
            r#"{"modes":{"action":"zai-org/GLM-5.3-Flash"}}"#,
        )
        .unwrap();
        let key = base.join("empty.env");
        std::fs::write(&key, "").unwrap();
        // The stale file the console has to refresh: Oct 27 18:19:50 CET.
        let f = light.join("state/survival.json");
        std::fs::write(&f, "{\"when\":\"2026-10-27T18:19:50+01:00\"}\n").unwrap();
        set_mtime(&f, 1_793_121_590);
        Roots {
            base,
            light,
            hermes,
            key,
        }
    }
    fn file(&self) -> PathBuf {
        self.light.join("state/survival.json")
    }
    fn writer_env(&self) -> Vec<String> {
        [
            format!("GALION_LIGHT_ROOT={}", self.light.display()),
            format!("GALION_HERMES_ROOT={}", self.hermes.display()),
            format!("OPENROUTER_KEY_FILE={}", self.key.display()),
            "GALION_SURVIVAL_DISK=/".to_string(),
        ]
        .into_iter()
        .flat_map(|e| ["--survival-env".to_string(), e])
        .collect()
    }
}

impl Drop for Roots {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn start(bind: &str, roots: &Roots, extra: &[String]) -> Kill {
    let child = Command::new(env!("CARGO_BIN_EXE_galion-console-static"))
        .args(["--ui", "rust", "--bind", bind, "--quiet", "--dist"])
        .arg(roots.base.join("dist"))
        .arg("--survival-file")
        .arg(roots.file())
        .args(extra)
        .env("OPENROUTER_API_KEY", "sk-decoy-must-not-reach-writer")
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

fn wait_fresh(file: &Path, secs: u64) -> Duration {
    let t0 = Instant::now();
    loop {
        if mtime(file).is_some_and(|t| now() - t < 30) {
            return t0.elapsed();
        }
        assert!(t0.elapsed() < Duration::from_secs(secs), "not refreshed");
        thread::sleep(Duration::from_millis(100));
    }
}

fn wait_idle(bind: &str) -> String {
    let t0 = Instant::now();
    loop {
        let s = status(bind);
        if field(&s, "running") == "false" && field(&s, "lastExit") != "null" {
            return s;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "writer still running: {s}"
        );
        thread::sleep(Duration::from_millis(100));
    }
}

/// The refreshed file is the real writer's output, on the Zagreb clock,
/// without the decoy key.
fn check_written(file: &Path) {
    let text = std::fs::read_to_string(file).unwrap();
    let when = field(&text.replace(['\n', ' '], ""), "when");
    assert!(
        when.ends_with("+01:00") || when.ends_with("+02:00"),
        "when {when}"
    );
    assert!(!when.ends_with('Z'));
    for k in ["\"cpu\"", "\"disk\"", "\"gpu\"", "\"lane\"", "\"rules\""] {
        assert!(text.contains(k), "{k} missing");
    }
    assert!(
        text.contains("\"measured\": false"),
        "openrouter must be unmeasured"
    );
    assert!(
        text.contains("no OPENROUTER_API_KEY"),
        "decoy key leaked to writer"
    );
    assert!(!text.contains("sk-decoy"));
    assert!(
        text.contains("zai-org/GLM-5.3-Flash"),
        "lane from modes.json"
    );
    println!("  file when={when} bytes={}", text.len());
}

#[test]
fn real_survival_refresh() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let w = writer_bin();
    let ws = w.display().to_string();

    // 1) on-request: the first /bank GET starts the real writer.
    println!("1) on-request, refresh 60 s, stale file (2026-10-27 18:19:50 CET)");
    let r = Roots::new("onreq");
    let mut a = vec![
        "--survival-writer".into(),
        ws.clone(),
        "--survival-refresh-s".into(),
        "60".into(),
    ];
    a.extend(r.writer_env());
    {
        let _g = start("127.0.0.1:22590", &r, &a);
        // Before any trigger: status itself is a trigger, so read it via HEAD-free
        // ordering: first /bank, then status.
        let (c, _) = get("127.0.0.1:22590", "/bank");
        assert_eq!(c, 200);
        let took = wait_fresh(&r.file(), 20);
        println!("  refreshed {} ms after GET /bank", took.as_millis());
        let s = wait_idle("127.0.0.1:22590");
        println!("  status {s}");
        assert_eq!(field(&s, "lastExit"), "0");
        assert_eq!(field(&s, "runs"), "1");
        assert_eq!(field(&s, "ok"), "true");
        assert_eq!(field(&s, "stale"), "false");
        assert_eq!(field(&s, "mode"), "on-request");
        assert!(field(&s, "asOf").ends_with("+01:00"));
        check_written(&r.file());

        // 2) flood: 60 trigger GETs while fresh -> no second run.
        println!("2) 60 trigger GETs while fresh");
        let t0 = Instant::now();
        for i in 0..60 {
            let p = ["/bank", "/api/v1/bank", "/api/v1/survival"][i % 3];
            let _ = get("127.0.0.1:22590", p);
        }
        let s = status("127.0.0.1:22590");
        println!(
            "  60 GETs {} ms, runs {}",
            t0.elapsed().as_millis(),
            field(&s, "runs")
        );
        assert_eq!(field(&s, "runs"), "1");
        // Non-trigger paths never start one either, even when due.
        set_mtime(&r.file(), now() - 3_600);
        thread::sleep(Duration::from_secs(31)); // past MIN_GAP_S
        for p in ["/console", "/api/v1/health", "/assets/app.css"] {
            let _ = get("127.0.0.1:22590", p);
        }
        thread::sleep(Duration::from_millis(500));
        assert_eq!(mtime(&r.file()).map(|t| now() - t > 3_000), Some(true));
        // 3) due again (aged 1 h) -> one more run on the next trigger.
        println!("3) aged 1 h -> next trigger refreshes again");
        let _ = get("127.0.0.1:22590", "/api/v1/bank");
        wait_fresh(&r.file(), 20);
        let s = wait_idle("127.0.0.1:22590");
        assert_eq!(field(&s, "runs"), "2");
        assert_eq!(field(&s, "lastExit"), "0");
    }

    // 4) failing writer: exit 1, file untouched, backed off a full interval.
    println!("4) failing writer (unwritable root) -> back-off");
    let r = Roots::new("fail");
    let a = vec![
        "--survival-writer".into(),
        ws.clone(),
        "--survival-refresh-s".into(),
        "60".into(),
        "--survival-env".into(),
        "GALION_LIGHT_ROOT=/proc/389-no-such-root".into(),
    ];
    {
        let _g = start("127.0.0.1:22591", &r, &a);
        let _ = get("127.0.0.1:22591", "/bank");
        let s = wait_idle("127.0.0.1:22591");
        println!("  status {s}");
        assert_eq!(field(&s, "lastExit"), "1");
        assert_eq!(field(&s, "failures"), "1");
        assert_eq!(field(&s, "stale"), "true");
        for _ in 0..20 {
            let _ = get("127.0.0.1:22591", "/bank");
        }
        let s = status("127.0.0.1:22591");
        assert_eq!(field(&s, "runs"), "1", "no retry inside back-off");
        assert_eq!(mtime(&r.file()), Some(1_793_121_590), "file untouched");
    }

    // 5) missing writer binary: spawn error -1, console keeps serving.
    println!("5) missing writer binary");
    let r = Roots::new("missing");
    let a = vec![
        "--survival-writer".into(),
        r.base.join("no-writer").display().to_string(),
    ];
    {
        let _g = start("127.0.0.1:22592", &r, &a);
        let _ = get("127.0.0.1:22592", "/bank");
        let s = wait_idle("127.0.0.1:22592");
        assert_eq!(field(&s, "lastExit"), "-1");
        assert_eq!(get("127.0.0.1:22592", "/api/v1/health").0, 200);
    }

    // 6) interval mode: no request at all, the tick refreshes it.
    println!("6) interval mode, no requests");
    let r = Roots::new("interval");
    let mut a = vec![
        "--survival-writer".into(),
        ws.clone(),
        "--survival-refresh-s".into(),
        "60".into(),
        "--survival-mode".into(),
        "interval".into(),
    ];
    a.extend(r.writer_env());
    {
        let _g = start("127.0.0.1:22593", &r, &a);
        let took = wait_fresh(&r.file(), 20);
        println!("  refreshed {} ms after start", took.as_millis());
        check_written(&r.file());
        let s = wait_idle("127.0.0.1:22593");
        assert_eq!(field(&s, "mode"), "interval");
        assert_eq!(field(&s, "runs"), "1");
    }

    // 7) status-only (no writer): reports age, never writes.
    println!("7) status only");
    let r = Roots::new("statusonly");
    {
        let _g = start("127.0.0.1:22594", &r, &[]);
        let s = status("127.0.0.1:22594");
        println!("  status {s}");
        assert_eq!(field(&s, "writer"), "null");
        assert_eq!(field(&s, "stale"), "true");
        assert_eq!(field(&s, "asOf"), "2026-10-27T18:19:50+01:00");
        assert_eq!(mtime(&r.file()), Some(1_793_121_590));
    }

    // Live leg: read-only mtime of the VPS file.
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
            "8) live survival.json mtime {live} ({} h old)",
            (now() - live) / 3600
        );
        let r = Roots::new("live");
        set_mtime(&r.file(), live);
        let _g = start("127.0.0.1:22595", &r, &[]);
        let s = status("127.0.0.1:22595");
        println!("  status {s}");
        assert_eq!(field(&s, "stale"), (now() - live > 2_700).to_string());
        let age: i64 = field(&s, "ageS").parse().unwrap();
        assert!((age - (now() - live)).abs() <= 5);
    }
}
