// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 425 (console-survival-fold, Console V2 prep, NOT the live
//! cutover, NO deploy): the staged drop-in
//! `deploy/console-rust-serve/survival-fold/console-rust-primary.service.d/70-survival.conf`
//! applied on top of the LIVE primary unit + its installed drop-in (as-built
//! mirrors under `deploy/console-rust-serve/live/`) gives exactly the live
//! 5aa31927 argv plus the in-console survival refresh (D-389-1, interval
//! 900 s per D-389-2), and that one process carries both halves:
//!
//! 1. no request at all: the interval tick runs the real `galion-survival`
//!    writer once, the stale file becomes fresh (Zagreb `when`, decoy key
//!    never reaches the writer, nothing spent);
//! 2. `/api/v1/survival` status: interval, 900 s, 1 run, not stale; POST 405,
//!    foreign host 401; 40 more GETs do not start another run;
//! 3. `/bank` card (D-408-1) `fresh`, never file content;
//! 4. the 5aa31927 half still works in the same process: `/api/arena` rows
//!    from `--arena-root`, `/arena`, `/api/v1/health`, `/api/v1/bank` 404;
//! 5. fallback (:8080 style, `--survival-file` only, no writer) reads the
//!    same file as status/card only and never writes it.
//!
//! The unit test (always on) checks the composition; the real leg is gated:
//! `GALION_REAL=1 cargo test --release -p console-static --test real_survival_fold -- --nocapture --test-threads=1`
//! Optional live leg (`CONSOLE_SURVIVAL_LIVE=1`, `CONSOLE_SURVIVAL_VPS`,
//! default `slavik-vps`): READ-ONLY drift check of the installed drop-in
//! against its mirror, the live survival.json mtime (never content), and live
//! `:18080 /api/v1/survival` (404 until the fold is installed).
//! Ports: 22950-22959 (box only).

mod units;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const BASE: &str =
    include_str!("../../../deploy/console-rust-serve/live/console-rust-primary.service");
const LIVE_DROP: &str = include_str!(
    "../../../deploy/console-rust-serve/live/console-rust-primary.service.d/60-admin-store.conf"
);
const FOLD: &str = include_str!(
    "../../../deploy/console-rust-serve/survival-fold/console-rust-primary.service.d/70-survival.conf"
);
const FALLBACK: &str = include_str!("../../../deploy/console-rust-serve/live/console.service");
const OLD_STAGED: &str =
    include_str!("../../../deploy/staged/survival-refresh/console-rust-primary-survival.conf");

const LIVE_BIN: &str = "/opt/galion/rust/bin/galion-console-serve-5aa31927";
const FOLD_BIN: &str = "/opt/galion/rust/bin/galion-console-serve-1767bafa";
const STATE: &str = "/opt/galion/galion-light/state/survival.json";
const WRITER: &str = "/opt/galion/rust/bin/galion-survival";
const DECOY: &str = "decoy-425-not-a-key";
const BOARD: &str = concat!(
    "{\"id\":\"f425-a\",\"generation\":1,\"score\":3.5,\"status\":\"done\"}\n",
    "{\"id\":\"f425-b\",\"generation\":2,\"score\":4.0,\"status\":\"done\"}\n",
);

/// Body below the as-built marker (what systemd actually reads).
fn live_body(mirror: &str) -> &str {
    let m = "# ---- live body ----\n";
    let i = mirror.find(m).expect("mirror marker") + m.len();
    &mirror[i..]
}

#[test]
fn fold_dropin_is_live_argv_plus_survival() {
    let base = live_body(BASE);
    let live = units::exec_argv(&[base, live_body(LIVE_DROP)]);
    let fold = units::exec_argv(&[base, live_body(LIVE_DROP), FOLD]);
    assert_eq!(live[0], LIVE_BIN, "installed drop-in runs 5aa31927");
    assert_eq!(fold[0], FOLD_BIN, "fold binary named after code 1767bafa");
    let extra: Vec<String> = [
        "--survival-file",
        STATE,
        "--survival-writer",
        WRITER,
        "--survival-refresh-s",
        "900",
        "--survival-mode",
        "interval",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mut want = live[1..].to_vec();
    want.extend(extra);
    assert_eq!(
        fold[1..],
        want[..],
        "fold = live argv + survival flags only"
    );
    assert_eq!(units::flag(&fold, "--bind"), Some("127.0.0.1:18080"));
    assert_eq!(units::flag(&fold, "--arena-root"), Some("/workspace/arena"));
    // Sandbox stays as live; the writer may only replace files under state/.
    let files = [base, live_body(LIVE_DROP), FOLD];
    assert_eq!(
        units::effective(&files, "Service", "ProtectSystem").as_deref(),
        Some("strict")
    );
    let live_rw = units::effective_paths(&[base, live_body(LIVE_DROP)], "ReadWritePaths");
    let rw = units::effective_paths(&files, "ReadWritePaths");
    let mut want_rw = live_rw.clone();
    want_rw.push("-/opt/galion/galion-light/state".to_string());
    assert_eq!(
        rw, want_rw,
        "fold only adds state/ to the live ReadWritePaths"
    );
    assert!(
        !units::covered(STATE, &live_rw),
        "live cannot write survival.json today"
    );
    assert!(units::covered(STATE, &rw));
    assert!(units::covered(
        "/opt/galion/galion-light/state/eval/survival.txt",
        &rw
    ));
    // D-389-3: the HOLD park write to modes.json keeps failing silently.
    assert!(!units::covered("/opt/galion/hermes/modes.json", &rw));
    assert!(!units::covered("/opt/galion/rust/bin/galion-survival", &rw));
    let ro = units::effective_paths(&files, "ReadOnlyPaths");
    assert!(units::covered(WRITER, &ro), "writer stays read-only");
    // Live today: no survival flags anywhere.
    assert!(!live.iter().any(|a| a.starts_with("--survival")));
    // Fallback (:8080) never gets a writer.
    let fb = units::exec_argv(&[live_body(FALLBACK)]);
    assert!(!fb.iter().any(|a| a.starts_with("--survival")));
    assert_eq!(units::flag(&fb, "--bind"), Some("127.0.0.1:8080"));
    // The slot 389 staged drop-in is marked superseded.
    assert!(OLD_STAGED.contains("SUPERSEDED (slot 425"));
    println!("fold argv: {}", fold.join(" "));
}

// ---------------------------------------------------------------- real leg

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

fn req(addr: &str, method: &str, path: &str, host: &str) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect(addr) else {
        return (0, String::new());
    };
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let cl = if method == "GET" || method == "HEAD" {
        String::new()
    } else {
        "Content-Length: 0\r\n".to_string()
    };
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\n{cl}Connection: close\r\n\r\n"
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

fn get(addr: &str, path: &str) -> (u16, String) {
    req(addr, "GET", path, "127.0.0.1")
}

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

fn status(addr: &str) -> String {
    let (c, b) = get(addr, "/api/v1/survival");
    assert_eq!(c, 200, "{b}");
    for leak in ["\"cpu\"", "\"rules\"", "openrouter", "cash", DECOY] {
        assert!(!b.contains(leak), "status echoes content ({leak}): {b}");
    }
    assert!(!b.contains("Z\""), "Zulu stamp in {b}");
    b
}

fn card(addr: &str) -> (String, String) {
    let (c, page) = get(addr, "/bank");
    assert_eq!(c, 200, "/bank");
    let i = page
        .find("<section aria-label=\"Survival\"")
        .unwrap_or_else(|| panic!("no survival card on /bank"));
    let rest = &page[i..];
    let j = rest.find("</section>").expect("card end") + "</section>".len();
    let card = rest[..j].to_string();
    for leak in ["\"cpu\"", "\"rules\"", "zai-org", DECOY] {
        assert!(!card.contains(leak), "card shows content ({leak})");
    }
    let st = card
        .split("data-state=\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .unwrap_or("")
        .to_string();
    let mut txt = String::new();
    let mut tag = false;
    for ch in card.chars() {
        match ch {
            '<' => {
                tag = true;
                txt.push(' ');
            }
            '>' => tag = false,
            ch if !tag => txt.push(ch),
            _ => {}
        }
    }
    (st, txt.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn writer_bin() -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let st = Command::new(cargo)
        .args(["build", "--release", "-j", "2", "-p", "galion-survival"])
        .status()
        .expect("cargo build galion-survival");
    assert!(st.success(), "build galion-survival");
    let exe = std::env::current_exe().unwrap();
    let target = exe.parent().unwrap().parent().unwrap().parent().unwrap();
    let w = target.join("release/galion-survival");
    assert!(w.is_file(), "{}", w.display());
    w
}

struct Root(PathBuf);
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Map the unit argv onto the temp root: binary, ports and host paths only.
fn remap(argv: &[String], root: &Path, writer: &Path, bind: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = argv[1..].iter();
    while let Some(a) = it.next() {
        out.push(a.clone());
        let v = match a.as_str() {
            "--bind" => bind.to_string(),
            "--dist" => root.join("dist").display().to_string(),
            "--pulse" => "127.0.0.1:22958".into(), // dead on purpose
            "--desk" => "127.0.0.1:22959".into(),  // dead on purpose
            "--admin-store" => root.join("admin/admin-device.json").display().to_string(),
            "--admin-mirror" => root.join("persist/admin-device.json").display().to_string(),
            "--arena-root" => root.join("arena").display().to_string(),
            "--survival-file" => root.join("light/state/survival.json").display().to_string(),
            "--survival-writer" => writer.display().to_string(),
            "--survival-refresh-s" | "--survival-mode" => it.next().unwrap().clone(),
            _ => continue,
        };
        if !matches!(a.as_str(), "--survival-refresh-s" | "--survival-mode") {
            it.next();
        }
        out.push(v);
    }
    out
}

fn start(argv: &[String], bind: &str) -> Kill {
    let child = Command::new(env!("CARGO_BIN_EXE_galion-console-serve"))
        .args(argv)
        .env("OPENROUTER_API_KEY", DECOY)
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

#[test]
fn real_survival_fold() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let w = writer_bin();
    let root = Root(std::env::temp_dir().join(format!("real-425-{}", std::process::id())));
    let r = &root.0;
    let _ = std::fs::remove_dir_all(r);
    for d in [
        "light/state",
        "hermes",
        "dist/static/assets",
        "arena",
        "admin",
        "persist",
    ] {
        std::fs::create_dir_all(r.join(d)).unwrap();
    }
    std::fs::write(r.join("dist/static/assets/app.css"), "body{}").unwrap();
    std::fs::write(
        r.join("light/state/hardware.json"),
        r#"{"processors":4,"gpu":{"name":null},"line":"CPU only."}"#,
    )
    .unwrap();
    std::fs::write(
        r.join("hermes/modes.json"),
        r#"{"modes":{"action":"zai-org/GLM-5.3-Flash"}}"#,
    )
    .unwrap();
    std::fs::write(r.join("empty.env"), "").unwrap();
    std::fs::write(r.join("arena/board.jsonl"), BOARD).unwrap();
    let file = r.join("light/state/survival.json");
    // Same stale instant as live: 2026-10-27 18:19:50 +01:00.
    std::fs::write(&file, "{\"when\":\"2026-10-27T18:19:50+01:00\"}\n").unwrap();
    set_mtime(&file, 1_793_121_590);

    let base = live_body(BASE);
    let unit = units::exec_argv(&[base, live_body(LIVE_DROP), FOLD]);
    let bind = "127.0.0.1:22950";
    let mut argv = remap(&unit, r, &w, bind);
    assert_eq!(argv.len(), unit.len() - 1, "every unit flag kept: {argv:?}");
    for (k, v) in [
        ("GALION_LIGHT_ROOT", r.join("light")),
        ("GALION_HERMES_ROOT", r.join("hermes")),
        ("OPENROUTER_KEY_FILE", r.join("empty.env")),
    ] {
        argv.push("--survival-env".into());
        argv.push(format!("{k}={}", v.display()));
    }
    argv.push("--survival-env".into());
    argv.push("GALION_SURVIVAL_DISK=/".into());
    println!("argv (remapped): {}", argv.join(" "));

    let t0 = Instant::now();
    let _g = start(&argv, bind);
    // 1) no request: the interval tick refreshes the stale file.
    loop {
        if mtime(&file).is_some_and(|t| now() - t < 30) {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(30), "not refreshed");
        thread::sleep(Duration::from_millis(100));
    }
    println!(
        "1) refreshed {} ms after start, no request",
        t0.elapsed().as_millis()
    );
    let text = std::fs::read_to_string(&file).unwrap();
    let when = field(&text.replace(['\n', ' '], ""), "when");
    assert!(
        when.ends_with("+01:00") || when.ends_with("+02:00"),
        "{when}"
    );
    for k in ["\"cpu\"", "\"disk\"", "\"lane\"", "\"rules\""] {
        assert!(text.contains(k), "{k} missing");
    }
    assert!(text.contains("no OPENROUTER_API_KEY"), "decoy key leaked");
    assert!(!text.contains(DECOY));
    println!("   file when={when} bytes={}", text.len());

    // 2) status route.
    let s = loop {
        let s = status(bind);
        if field(&s, "running") == "false" && field(&s, "lastExit") != "null" {
            break s;
        }
        assert!(t0.elapsed() < Duration::from_secs(60), "still running {s}");
        thread::sleep(Duration::from_millis(100));
    };
    println!("2) status {s}");
    assert_eq!(field(&s, "mode"), "interval");
    assert_eq!(field(&s, "refreshS"), "900");
    assert_eq!(field(&s, "runs"), "1");
    assert_eq!(field(&s, "lastExit"), "0");
    assert_eq!(field(&s, "stale"), "false");
    assert_eq!(req(bind, "POST", "/api/v1/survival", "127.0.0.1").0, 405);
    let (fc, _) = req(bind, "GET", "/api/v1/survival", "console.galion.app");
    assert_eq!(fc, 401, "foreign host gated");
    for _ in 0..20 {
        let _ = get(bind, "/api/v1/survival");
        let _ = get(bind, "/bank");
    }
    thread::sleep(Duration::from_millis(500));
    let s2 = status(bind);
    assert_eq!(
        field(&s2, "runs"),
        "1",
        "requests never start a run in interval mode"
    );
    println!(
        "   POST 405, foreign host {fc}, 40 GETs -> runs {}",
        field(&s2, "runs")
    );

    // 3) card.
    let (st, txt) = card(bind);
    println!("3) /bank card [{st}] {txt}");
    assert_eq!(st, "fresh");

    // 4) the 5aa31927 half in the same process.
    let (ac, ab) = get(bind, "/api/arena");
    assert_eq!(ac, 200, "{ab}");
    assert!(ab.contains("f425-a") && ab.contains("f425-b"), "{ab}");
    let (pc, _) = get(bind, "/arena");
    assert_eq!(pc, 200);
    assert_eq!(get(bind, "/api/v1/health").0, 200);
    let (bc, _) = get(bind, "/api/v1/bank");
    assert_eq!(bc, 404, "bank reader still undecided (D-336-2)");
    assert_eq!(
        std::fs::read_to_string(r.join("arena/board.jsonl")).unwrap(),
        BOARD
    );
    println!("4) /api/arena 200 ids 2/2, /arena {pc}, health 200, /api/v1/bank {bc}");

    // 5) fallback style: --survival-file only, same file, status/card only.
    let fb_unit = units::exec_argv(&[live_body(FALLBACK)]);
    let fb_bind = "127.0.0.1:22951";
    let mut fb = remap(&fb_unit, r, &w, fb_bind);
    fb.push("--survival-file".into());
    fb.push(file.display().to_string());
    let before = mtime(&file);
    {
        let _f = start(&fb, fb_bind);
        let s = status(fb_bind);
        assert_eq!(field(&s, "writer"), "null");
        assert_eq!(field(&s, "stale"), "false");
        let (st, _) = card(fb_bind);
        assert_eq!(st, "fresh");
        thread::sleep(Duration::from_secs(2));
        assert_eq!(mtime(&file), before, "fallback never writes");
        println!("5) fallback writer null, card [{st}], file untouched");
    }

    // Live leg: read-only.
    if std::env::var("CONSOLE_SURVIVAL_LIVE").as_deref() == Ok("1") {
        let host = std::env::var("CONSOLE_SURVIVAL_VPS").unwrap_or_else(|_| "slavik-vps".into());
        let o = Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                &host,
                "cat /etc/systemd/system/console-rust-primary.service.d/60-admin-store.conf; \
                 echo @@; ls /etc/systemd/system/console-rust-primary.service.d/; \
                 echo @@; stat -c %Y /opt/galion/galion-light/state/survival.json; \
                 echo @@; curl -s -o /dev/null -w %{http_code} http://127.0.0.1:18080/api/v1/survival",
            ])
            .output()
            .expect("ssh");
        let out = String::from_utf8_lossy(&o.stdout).to_string();
        let parts: Vec<&str> = out.split("@@\n").collect();
        assert_eq!(parts.len(), 4, "live probe");
        assert_eq!(
            parts[0],
            live_body(LIVE_DROP),
            "installed drop-in drifted from mirror"
        );
        assert_eq!(parts[1].trim(), "60-admin-store.conf", "fold not installed");
        let m: i64 = parts[2].trim().parse().expect("mtime");
        println!(
            "live: drop-in == mirror, dir [{}], survival.json age {:.1} h, :18080 /api/v1/survival {}",
            parts[1].trim(),
            (now() - m) as f64 / 3600.0,
            parts[3].trim()
        );
        assert_eq!(
            parts[3].trim(),
            "404",
            "live console has no survival route yet"
        );
    }
}
