// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real test (wave-4 slot 413, console-deploy-candidate-2): ONE release
//! `galion-console-serve` started with the exact argv of the staged
//! `deploy/console-rust-serve/candidate-2/*/60-admin-store.conf` drop-ins
//! (live base unit + drop-in; host paths and ports remapped into a temp root
//! and box ports 22830-22839). Relative to what live runs since 2026-10-28
//! 17:30/17:34 CET (+01:00) (`candidate/` drop-in, binary -5aa31927) it adds
//! `--owner-routes --hermes 127.0.0.1:8642` (console-rust-routes-a 820dfaea)
//! and the F-395-2/3/4 fixes.
//!
//! A. Same process: health + board routes 200; owner rows forwarded to the
//!    pulse / hermes owners with Cookie/Authorization dropped, POST 405;
//!    the 12 retired rows (incl. /api/v1/bank) 410 JSON; hermes models /
//!    history 404 (D-384-3); arena rows 200, non-GET 405, board unchanged;
//!    remote console host gated first (401, owner not reached), paired
//!    cookie 200; 5 wrong admin codes 401 then 429 (F-313-1 kept).
//! B. F-395-2: the same argv without `--arena-root` exits 2 at startup.
//! C. Live leg (`CONSOLE_CANDIDATE2_LIVE=1`, `CONSOLE_CANDIDATE2_VPS`,
//!    default `slavik-vps`): READ-ONLY `ssh -N -L` forwards of the live
//!    galion-one :8091, hermes-rs :8642, galion-desk :8646 and nerve :8670
//!    plus `ssh cat /workspace/arena/board.jsonl`; the candidate answers the
//!    owner rows with the live owners' status and the live arena rows; nerve
//!    /state read before and after. GET only, nothing changes on the VPS.
//!
//! Gated: `GALION_REAL=1 cargo test --release -p console-static --test real_deploy_candidate_2 -- --nocapture`
//! No live change, NO deploy.

mod units;

use console_static::owners;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LIVE: &str = include_str!("../../../deploy/console-rust-serve/live/console.service");
const PRIMARY: &str =
    include_str!("../../../deploy/console-rust-serve/live/console-rust-primary.service");
/// What live runs (as-built since 17:30/17:34 CET).
const NOW: &str = include_str!(
    "../../../deploy/console-rust-serve/candidate/console.service.d/60-admin-store.conf"
);
const NOW_PRIMARY: &str = include_str!(
    "../../../deploy/console-rust-serve/candidate/console-rust-primary.service.d/60-admin-store.conf"
);
const DROP: &str = include_str!(
    "../../../deploy/console-rust-serve/candidate-2/console.service.d/60-admin-store.conf"
);
const PRIMARY_DROP: &str = include_str!(
    "../../../deploy/console-rust-serve/candidate-2/console-rust-primary.service.d/60-admin-store.conf"
);
/// Code commit the staged binary name points at.
const CODE_SHA: &str = "4e9e9e50";

const AT: &str = "127.0.0.1:22830";
const AT_B: &str = "127.0.0.1:22831";
const AT_LIVE: &str = "127.0.0.1:22832";
const NERVE_AT: &str = "127.0.0.1:22835";
const HERMES_AT: &str = "127.0.0.1:22837";
const PULSE_AT: &str = "127.0.0.1:22838";
const DESK_AT: &str = "127.0.0.1:22839";

const BOARD_ROUTES: &[&str] = &[
    "/api/v1/services",
    "/api/v1/domains",
    "/api/v1/repos",
    "/api/v1/catalog",
    "/api/v1/ledger",
    "/api/v1/docs",
    "/api/v1/protocol",
    "/api/v1/markers",
    "/api/v1/health",
];
const REMOTE: &str = "console.galion.app";
const CODE: &str = "SLOT413-CANDIDATE-CODE";
const OLD_TOKEN: &str = "slot413-already-paired-phone-token";
const DEVICE: &str = "slot413-paired-device-secret-0123456789";
const ARENA_BOARD: &str = concat!(
    "{\"id\":\"c413-a\",\"generation\":1,\"score\":3.5,\"status\":\"done\"}\n",
    "{\"id\":\"c413-b\",\"generation\":2,\"score\":4.25,\"status\":\"done\"}\n",
);

struct Kill(Child, &'static str);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        eprintln!("real: stopped {}", self.1);
    }
}

#[derive(Debug)]
struct Resp {
    code: u16,
    body: String,
}

fn req(at: &str, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Resp {
    let Ok(mut s) = TcpStream::connect(at) else {
        return Resp {
            code: 0,
            body: String::new(),
        };
    };
    s.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    let mut out = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\n");
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    if method != "GET" && method != "HEAD" {
        out.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    out.push_str("\r\n");
    let _ = s.write_all(out.as_bytes());
    let _ = s.write_all(body);
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let p = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(raw.len());
    let head = String::from_utf8_lossy(&raw[..p]).to_string();
    let code = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = String::from_utf8_lossy(raw.get(p + 4..).unwrap_or(&[])).to_string();
    Resp { code, body }
}

fn local(at: &str, method: &str, path: &str) -> Resp {
    let body: &[u8] = if method == "GET" || method == "HEAD" {
        b""
    } else {
        b"{}"
    };
    req(
        at,
        method,
        path,
        &[("Host", "127.0.0.1"), ("Content-Type", "application/json")],
        body,
    )
}

fn admin_code(at: &str, c: &str) -> Resp {
    let body = format!("{{\"code\":\"{c}\"}}");
    req(
        at,
        "POST",
        "/api/v1/admin",
        &[("Host", REMOTE), ("Content-Type", "application/json")],
        body.as_bytes(),
    )
}

fn wait_up(at: &str, secs: u64) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        if TcpStream::connect(at).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn sha_hex(s: &str) -> String {
    console_static::sha256::hex(s.as_bytes())
}

fn chmod(p: &Path, m: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
}

/// Live argv -> box argv (paths into `tmp`, owner ports onto the box ports).
fn remap(argv: &[String], exe: &Path, tmp: &Path, bind: &str) -> Vec<String> {
    let t = tmp.to_str().unwrap();
    argv.iter()
        .enumerate()
        .map(|(i, a)| {
            if i == 0 {
                return exe.to_str().unwrap().to_string();
            }
            a.replace("127.0.0.1:18080", bind)
                .replace("127.0.0.1:8080", bind)
                .replace("127.0.0.1:8091", PULSE_AT)
                .replace("127.0.0.1:8642", HERMES_AT)
                .replace("127.0.0.1:8646", DESK_AT)
                .replace("/opt/galion/console-dist", &format!("{t}/opt/console-dist"))
                .replace("/root/galion-secrets", &format!("{t}/root/galion-secrets"))
                .replace(
                    "/workspace/galion-light/secrets/persist",
                    &format!("{t}/persist"),
                )
                .replace("/workspace/arena", &format!("{t}/arena"))
        })
        .collect()
}

fn spawn(argv: &[String], at: &str) -> Kill {
    let child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn galion-console-serve");
    let k = Kill(child, "candidate-2 console");
    assert!(wait_up(at, 5), "server did not bind {at}");
    k
}

fn store_json(fails: u32) -> String {
    format!(
        "{{\"codeHash\":\"{}\",\"tokenHash\":\"{t}\",\"tokenHashes\":[\"{t}\"],\"deviceHash\":\"{}\",\"fails\":{fails}}}\n",
        sha_hex(CODE),
        sha_hex(DEVICE),
        t = sha_hex(OLD_TOKEN)
    )
}

fn ids_of(board: &str) -> Vec<String> {
    board
        .lines()
        .filter_map(|l| {
            let i = l.find("\"id\":")? + 5;
            let rest = l[i..].trim_start().strip_prefix('"')?;
            Some(rest[..rest.find('"')?].to_string())
        })
        .collect()
}

/// Strip the binary (argv[0]) and the given flags (with their value when
/// `valued`) from an argv.
fn strip(v: &[String], flags: &[(&str, bool)]) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = v.iter().skip(1);
    while let Some(a) = it.next() {
        if let Some((_, valued)) = flags.iter().find(|(f, _)| f == a) {
            if *valued {
                it.next();
            }
            continue;
        }
        out.push(a.clone());
    }
    out
}

type Seen = Arc<Mutex<Vec<String>>>;

/// Fake owner on a fixed box port: records request heads, answers `status`
/// with JSON naming `who` and the path it got.
fn fake_owner(at: &str, who: &'static str, status: u16, seen: Seen) {
    let l = TcpListener::bind(at).unwrap_or_else(|e| panic!("bind fake {who} {at}: {e}"));
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let seen = Arc::clone(&seen);
            std::thread::spawn(move || {
                let mut s = s;
                let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = s.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let head = String::from_utf8_lossy(&buf).to_string();
                let path = head.split(' ').nth(1).unwrap_or("").to_string();
                seen.lock().unwrap().push(head);
                let body = format!(r#"{{"who":"{who}","path":"{path}"}}"#);
                let _ = write!(
                    s,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
}

#[test]
fn candidate2_dropins_are_live_plus_owner_routes() {
    // Pure (no GALION_REAL): candidate-2 argv = the live (candidate/) argv
    // with the new binary name and exactly `--owner-routes --hermes
    // 127.0.0.1:8642` added; ReadWritePaths unchanged; both units agree.
    let now = units::exec_argv(&[LIVE, NOW]);
    let new = units::exec_argv(&[LIVE, DROP]);
    assert!(
        now[0].ends_with("/galion-console-serve-5aa31927"),
        "{}",
        now[0]
    );
    assert!(
        new[0].ends_with(&format!("/galion-console-serve-{CODE_SHA}")),
        "{}",
        new[0]
    );
    let added = [("--owner-routes", false), ("--hermes", true)];
    assert_eq!(strip(&new, &added), strip(&now, &[]));
    assert_eq!(new.len(), now.len() + 3, "exactly 3 new argv words");
    assert_eq!(units::flag(&new, "--hermes"), Some("127.0.0.1:8642"));
    assert_eq!(units::flag(&new, "--pulse"), Some("127.0.0.1:8091"));
    assert_eq!(units::flag(&new, "--arena-root"), Some("/workspace/arena"));
    assert!(new.iter().any(|a| a == "--owner-routes"));
    assert!(
        units::flag(&new, "--data-upstream").is_none(),
        "bank stays 410"
    );
    assert_eq!(
        units::effective_paths(&[LIVE, DROP], "ReadWritePaths"),
        units::effective_paths(&[LIVE, NOW], "ReadWritePaths")
    );
    let p = units::exec_argv(&[PRIMARY, PRIMARY_DROP]);
    let pnow = units::exec_argv(&[PRIMARY, NOW_PRIMARY]);
    assert_eq!(p[0], new[0], "primary + fallback run the same binary");
    assert_eq!(strip(&p, &added), strip(&pnow, &[]));
    assert_eq!(units::flag(&p, "--bind"), Some("127.0.0.1:18080"));
    assert_eq!(units::flag(&new, "--bind"), Some("127.0.0.1:8080"));
    // The staged argv parses; without --arena-root it is refused (F-395-2).
    let cfg = console_static::config::Config::from_args(new[1..].to_vec()).unwrap();
    assert!(cfg.owner_routes && cfg.hermes.is_some() && cfg.data_paths.is_empty());
    let no_arena = strip(&new, &[("--arena-root", true)]);
    let e = console_static::config::Config::from_args(no_arena)
        .unwrap_err()
        .to_string();
    assert!(e.contains("--owner-routes needs --arena-root"), "{e}");
}

#[test]
fn real_deploy_candidate_2() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let fixture = std::env::var("CONSOLE_DIST_STATIC")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from("/workspace/swarm/artifacts/console-rust-serve-impl/console-dist-static")
        });
    assert!(
        fixture.join("favicon.svg").is_file(),
        "fixture {}",
        fixture.display()
    );
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_galion-console-serve"));
    let tmp = std::env::temp_dir().join(format!("real-413-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let dist = tmp.join("opt/console-dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::os::unix::fs::symlink(&fixture, dist.join("static")).unwrap();
    let sdir = tmp.join("root/galion-secrets");
    let mdir = tmp.join("persist");
    for d in [&sdir, &mdir] {
        std::fs::create_dir_all(d).unwrap();
        chmod(d, 0o700);
    }
    let store = sdir.join("admin-device.json");
    std::fs::write(&store, store_json(0)).unwrap();
    chmod(&store, 0o600);
    let arena = tmp.join("arena");
    std::fs::create_dir_all(&arena).unwrap();
    let board = arena.join("board.jsonl");
    std::fs::write(&board, ARENA_BOARD).unwrap();
    let staged = units::exec_argv(&[LIVE, DROP]);
    let cookie = format!("galion_admin={OLD_TOKEN}");
    let t0 = Instant::now();
    let live = std::env::var("CONSOLE_CANDIDATE2_LIVE").as_deref() == Ok("1");

    if !live {
        // ---- A with fake owners ----
        let seen_p: Seen = Arc::default();
        let seen_h: Seen = Arc::default();
        fake_owner(PULSE_AT, "one", 200, Arc::clone(&seen_p));
        fake_owner(HERMES_AT, "hermes", 200, Arc::clone(&seen_h));
        let _k = spawn(&remap(&staged, &exe, &tmp, AT), AT);
        for p in BOARD_ROUTES {
            let r = local(AT, "GET", p);
            assert_eq!(r.code, 200, "A {p}: {}", r.body);
        }
        let root = local(AT, "GET", "/").code;
        assert!(root == 200 || root == 302, "A: / {root}");
        // Owner rows.
        let r = req(
            AT,
            "GET",
            "/api/v1/analytics?board=x",
            &[
                ("Host", "127.0.0.1"),
                ("Cookie", "galion_admin=x"),
                ("Authorization", "Bearer y"),
            ],
            b"",
        );
        assert_eq!(r.code, 200, "{}", r.body);
        assert!(
            r.body.contains(r#""path":"/api/v1/analytics?board=x""#),
            "{}",
            r.body
        );
        assert!(local(AT, "GET", "/api/v1/awake")
            .body
            .contains(r#""who":"one""#));
        let h = local(AT, "GET", "/api/hermes/health");
        assert!(
            h.body.contains(r#""who":"hermes","path":"/health""#),
            "{}",
            h.body
        );
        for head in seen_p.lock().unwrap().iter() {
            let l = head.to_ascii_lowercase();
            assert!(
                !l.contains("\r\ncookie:") && !l.contains("\r\nauthorization:"),
                "{head}"
            );
        }
        assert_eq!(local(AT, "POST", "/api/v1/analytics").code, 405);
        assert_eq!(local(AT, "GET", "/api/hermes/models").code, 404, "D-384-3");
        assert_eq!(local(AT, "GET", "/api/hermes/history").code, 404, "D-384-3");
        let (np, nh) = (seen_p.lock().unwrap().len(), seen_h.lock().unwrap().len());
        assert_eq!((np, nh), (2, 1), "owner hits");
        // Gate first: remote host without cookie never reaches the owner.
        let r = req(AT, "GET", "/api/v1/awake", &[("Host", REMOTE)], b"");
        assert_eq!(r.code, 401, "remote no cookie");
        let r = req(AT, "GET", "/api/v1/awake", &[("Host", "evil.example")], b"");
        assert_eq!(r.code, 401, "foreign host");
        assert_eq!(seen_p.lock().unwrap().len(), 2, "gate ran before the owner");
        let r = req(
            AT,
            "GET",
            "/api/v1/awake",
            &[("Host", REMOTE), ("Cookie", &cookie)],
            b"",
        );
        assert_eq!(r.code, 200, "paired cookie {}", r.body);
        let last = seen_p.lock().unwrap().last().cloned().unwrap();
        assert!(
            !last.to_ascii_lowercase().contains("galion_admin"),
            "cookie leaked: {last}"
        );
        eprintln!("A: {} board routes 200; owner rows -> pulse 3 / hermes 1, no Cookie/Authorization sent, POST 405, models/history 404; remote/foreign 401 before owner, paired cookie 200", BOARD_ROUTES.len());
        // Retired rows, bank included (F-395-3: no data proxy, 410 wins).
        for p in owners::RETIRED {
            let r = local(AT, "GET", p);
            assert_eq!(r.code, 410, "{p}: {}", r.body);
            assert!(r.body.contains(r#""error":"retired""#), "{p}");
        }
        eprintln!(
            "A: {} retired rows 410 JSON incl. /api/v1/bank",
            owners::RETIRED.len()
        );
        // Arena.
        let a = local(AT, "GET", "/api/arena");
        assert_eq!(a.code, 200, "{}", a.body);
        for id in ids_of(ARENA_BOARD) {
            assert!(a.body.contains(&id), "arena id {id}");
        }
        let before = std::fs::read(&board).unwrap();
        for m in ["POST", "PUT", "DELETE"] {
            assert_eq!(local(AT, m, "/api/arena").code, 405, "{m} /api/arena");
        }
        assert_eq!(std::fs::read(&board).unwrap(), before, "board unchanged");
        assert_eq!(local(AT, "GET", "/arena").code, 200, "/arena page");
        eprintln!(
            "A: /api/arena 200 {} B rows ok, POST/PUT/DELETE 405, board unchanged, /arena 200",
            a.body.len()
        );
        // Lockout kept (F-313-1).
        let wrong: Vec<u16> = (0..5)
            .map(|i| admin_code(AT, &format!("WRONG{i}")).code)
            .collect();
        assert_eq!(wrong, vec![401; 5]);
        let (w6, right) = (admin_code(AT, "WRONG5"), admin_code(AT, CODE));
        assert_eq!((w6.code, right.code), (429, 429));
        assert_eq!(w6.body, right.body);
        let raw = std::fs::read_to_string(&store).unwrap();
        assert!(!raw.contains(CODE) && !raw.contains(DEVICE), "hashes only");
        eprintln!("A: admin 5x401 -> 429 wrong+right identical, store hashes only");
    }

    // ---- B: F-395-2 refuse at startup ----
    {
        let argv = remap(&staged, &exe, &tmp, AT_B);
        let mut no_arena = vec![argv[0].clone()];
        no_arena.extend(strip(&argv, &[("--arena-root", true)]));
        let out = Command::new(&no_arena[0])
            .args(&no_arena[1..])
            .stdin(Stdio::null())
            .output()
            .expect("run without --arena-root");
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "F-395-2 exit: {err}");
        assert!(err.contains("--owner-routes needs --arena-root"), "{err}");
        assert!(TcpStream::connect(AT_B).is_err(), "nothing bound");
        eprintln!(
            "B: without --arena-root exit 2: {:?}",
            err.lines().next().unwrap_or("")
        );
    }

    // ---- C: live leg (read-only) ----
    if live {
        let vps = std::env::var("CONSOLE_CANDIDATE2_VPS").unwrap_or_else(|_| "slavik-vps".into());
        let out = Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                &vps,
                "cat /workspace/arena/board.jsonl",
            ])
            .output()
            .expect("ssh");
        assert!(out.status.success(), "live board read failed");
        let live_board = String::from_utf8_lossy(&out.stdout).to_string();
        std::fs::write(&board, &live_board).unwrap();
        let ids = ids_of(&live_board);
        let tail: Vec<String> = ids.iter().rev().take(16).rev().cloned().collect();
        let ssh = Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ExitOnForwardFailure=yes",
                "-N",
            ])
            .args(["-L", &format!("{PULSE_AT}:127.0.0.1:8091")])
            .args(["-L", &format!("{HERMES_AT}:127.0.0.1:8642")])
            .args(["-L", &format!("{DESK_AT}:127.0.0.1:8646")])
            .args(["-L", &format!("{NERVE_AT}:127.0.0.1:8670")])
            .arg(&vps)
            .stdin(Stdio::null())
            .spawn()
            .expect("ssh -N");
        let _ssh = Kill(ssh, "ssh forward");
        assert!(wait_up(NERVE_AT, 25), "ssh forward did not come up");
        eprintln!("real: forward {PULSE_AT}->8091 {HERMES_AT}->8642 {DESK_AT}->8646 {NERVE_AT}->8670 (ssh -N, GET only); candidate API {AT_LIVE}=local galion-console-serve");
        let nerve = |tag: &str| {
            let r = local(NERVE_AT, "GET", "/state");
            assert_eq!(r.code, 200, "nerve /state {tag}");
            let cyc = r
                .body
                .split("\"cycle\":")
                .nth(1)
                .map(|s| {
                    s.chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect::<String>()
                })
                .unwrap_or_default();
            let console_ok = r.body.contains("\"console\"");
            eprintln!(
                "nerve {tag}: /state 200 {} B cycle {cyc} console listed {console_ok}",
                r.body.len()
            );
        };
        nerve("before");
        let _k = spawn(&remap(&staged, &exe, &tmp, AT_LIVE), AT_LIVE);
        for (row, owner_at, owner_path) in [
            ("/api/v1/awake", PULSE_AT, "/api/v1/awake"),
            ("/api/v1/analytics", PULSE_AT, "/api/v1/analytics"),
            ("/api/hermes/health", HERMES_AT, "/health"),
        ] {
            let direct = local(owner_at, "GET", owner_path);
            let via = local(AT_LIVE, "GET", row);
            assert_ne!(via.code, 0, "{row}: no answer");
            assert_eq!(via.code, direct.code, "{row}: candidate vs live owner");
            eprintln!(
                "live: {row} candidate {} ({} B) = live owner {owner_path} {} ({} B)",
                via.code,
                via.body.len(),
                direct.code,
                direct.body.len()
            );
        }
        for p in owners::RETIRED {
            assert_eq!(local(AT_LIVE, "GET", p).code, 410, "live {p}");
        }
        let a = local(AT_LIVE, "GET", "/api/arena");
        assert_eq!(a.code, 200, "live arena {}", a.body);
        for id in &tail {
            assert!(a.body.contains(id.as_str()), "live arena id {id}");
        }
        for p in ["/console", "/services", "/hermes", "/arena", "/status"] {
            let r = local(AT_LIVE, "GET", p);
            assert_eq!(r.code, 200, "live page {p}");
            assert!(!r.body.contains("<script"), "{p}: no JS");
        }
        eprintln!(
            "live: board.jsonl {} lines, /api/arena 200 {} B ids {}/{}; {} retired 410; pages /console /services /hermes /arena /status 200 no JS",
            live_board.lines().count(),
            a.body.len(),
            tail.len(),
            tail.len(),
            owners::RETIRED.len()
        );
        nerve("after");
    }

    let _ = std::fs::remove_dir_all(&tmp);
    eprintln!(
        "real_deploy_candidate_2 total {} ms",
        t0.elapsed().as_millis()
    );
}
