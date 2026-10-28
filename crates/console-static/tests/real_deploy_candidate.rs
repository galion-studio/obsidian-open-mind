// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real test (wave-4 slot 343, console-deploy-candidate): ONE release
//! `galion-console-serve` started with the exact argv of the staged
//! candidate drop-ins `deploy/console-rust-serve/candidate/*/60-admin-store.conf`
//! (live base unit + drop-in; only host paths/ports remapped into a temp root)
//! carries BOTH halves of the deploy candidate at the same time:
//!
//! A. F-313-1 lockout (console-admin-lockout-fix): 5 wrong codes 401 then
//!    429 for wrong AND right, `lockedUntil` persisted; RO store answers
//!    byte-identical 503 for right/wrong codes, then 429.
//! B. D-336-1 arena (console-dist-arena-bank): `/api/arena` 200 with rows
//!    from `--arena-root`, `/arena` page renders, non-GET 405 and the board
//!    file is unchanged; remote console host without cookie 401 (gate first),
//!    with the already-paired cookie 200; `/api/v1/bank` honest 404.
//! C. Board routes (`/api/v1/health`, `/`) 200 in the same process.
//! D. Missing arena root (fresh VPS without /workspace/arena): server still
//!    starts, `/api/arena` fails soft, health 200.
//!
//! Optional live leg (`CONSOLE_CANDIDATE_LIVE=1`, `CONSOLE_CANDIDATE_VPS`,
//! default `slavik-vps`): READ-ONLY `ssh cat /workspace/arena/board.jsonl`
//! into the temp arena root (no secrets in that file), then B again.
//!
//! Gated: `GALION_REAL=1 cargo test --release -p console-static --test real_deploy_candidate -- --nocapture`
//! Ports: 22130-22139 (box only). No live change, NO deploy.

mod units;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const LIVE: &str = include_str!("../../../deploy/console-rust-serve/live/console.service");
const DROP: &str = include_str!(
    "../../../deploy/console-rust-serve/candidate/console.service.d/60-admin-store.conf"
);
const PRIMARY: &str =
    include_str!("../../../deploy/console-rust-serve/live/console-rust-primary.service");
const PRIMARY_DROP: &str = include_str!(
    "../../../deploy/console-rust-serve/candidate/console-rust-primary.service.d/60-admin-store.conf"
);
const OLD_DROP: &str =
    include_str!("../../../deploy/console-rust-serve/console.service.d/60-admin-store.conf");
const BOARD_ROUTES: &[&str] = &[
    "/api/v1/services",
    "/api/v1/domains",
    "/api/v1/repos",
    "/api/v1/catalog",
    "/api/v1/ledger",
    "/api/v1/docs",
    "/api/v1/protocol",
    "/api/v1/protocol/snapshot",
    "/api/v1/markers",
    "/api/v1/health",
];
const REMOTE: &str = "console.galion.app";
const CODE: &str = "SLOT343-CANDIDATE-CODE";
const OLD_TOKEN: &str = "slot343-already-paired-phone-token";
const DEVICE: &str = "slot343-paired-device-secret-0123456789";
const ARENA_BOARD: &str = concat!(
    "{\"id\":\"c343-a\",\"generation\":1,\"score\":3.5,\"status\":\"done\"}\n",
    "{\"id\":\"c343-b\",\"generation\":2,\"score\":4.25,\"status\":\"done\"}\n",
    "{\"id\":\"c343-c\",\"generation\":3,\"score\":5.0,\"status\":\"done\"}\n",
);

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Debug)]
struct Resp {
    code: u16,
    head: String,
    body: String,
}

fn req(at: &str, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Resp {
    let Ok(mut s) = TcpStream::connect(at) else {
        return Resp {
            code: 0,
            head: String::new(),
            body: String::new(),
        };
    };
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
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
    Resp { code, head, body }
}

fn local(at: &str, method: &str, path: &str) -> Resp {
    let body: &[u8] = if method == "GET" {
        b""
    } else {
        br#"{"action":"generation"}"#
    };
    req(
        at,
        method,
        path,
        &[("Host", "127.0.0.1"), ("Content-Type", "application/json")],
        body,
    )
}

fn code(at: &str, c: &str) -> Resp {
    let body = format!("{{\"code\":\"{c}\"}}");
    req(
        at,
        "POST",
        "/api/v1/admin",
        &[("Host", REMOTE), ("Content-Type", "application/json")],
        body.as_bytes(),
    )
}

fn has_cookie(r: &Resp) -> bool {
    r.head.lines().any(|l| {
        l.to_ascii_lowercase()
            .starts_with("set-cookie: galion_admin=")
    })
}

fn wait_up(at: &str) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
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
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
}

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
                .replace("127.0.0.1:8091", "127.0.0.1:22138")
                .replace("127.0.0.1:8646", "127.0.0.1:22139")
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
    let k = Kill(child);
    assert!(wait_up(at), "server did not bind {at}");
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

/// Arena checks against a running candidate (`rows` = expected board ids).
fn arena_checks(at: &str, board: &Path, ids: &[String], tag: &str) {
    assert!(!ids.is_empty(), "{tag}: no board ids parsed");
    let t = Instant::now();
    let a = local(at, "GET", "/api/arena");
    let ms = t.elapsed().as_millis();
    assert_eq!(a.code, 200, "{tag}: /api/arena {}", a.body);
    for id in ids {
        assert!(a.body.contains(id.as_str()), "{tag}: id {id} missing");
    }
    let page = local(at, "GET", "/arena");
    assert_eq!(page.code, 200, "{tag}: /arena");
    assert!(
        !page.body.contains("<script") && !page.body.contains("<form"),
        "{tag}: /arena script/form"
    );
    let first = ids.first().cloned().unwrap_or_default();
    assert!(page.body.contains(&first), "{tag}: /arena renders rows");
    let before = std::fs::read(board).unwrap();
    for m in ["POST", "PUT", "DELETE"] {
        assert_eq!(
            local(at, m, "/api/arena").code,
            405,
            "{tag}: {m} /api/arena"
        );
    }
    assert_eq!(
        std::fs::read(board).unwrap(),
        before,
        "{tag}: board unchanged"
    );
    let bank = local(at, "GET", "/api/v1/bank");
    assert_eq!(bank.code, 404, "{tag}: /api/v1/bank stays 404 (D-336-2)");
    eprintln!(
        "{tag}: /api/arena 200 {} B {ms} ms ids {}/{}, /arena 200 {} B, POST/PUT/DELETE 405, board unchanged, /api/v1/bank 404",
        a.body.len(),
        ids.len(),
        ids.len(),
        page.body.len()
    );
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

#[test]
fn candidate_dropins_are_the_admin_dropins_plus_arena() {
    // Pure (no GALION_REAL): the candidate argv = slot 313 argv with the new
    // binary name and exactly one extra flag pair; ReadWritePaths unchanged.
    let old = units::exec_argv(&[LIVE, OLD_DROP]);
    let new = units::exec_argv(&[LIVE, DROP]);
    assert!(
        new[0].ends_with("/galion-console-serve-5aa31927"),
        "{}",
        new[0]
    );
    let strip = |v: &[String]| -> Vec<String> {
        let mut out = Vec::new();
        let mut it = v.iter().skip(1);
        while let Some(a) = it.next() {
            if a == "--arena-root" {
                it.next();
                continue;
            }
            out.push(a.clone());
        }
        out
    };
    assert_eq!(strip(&new), strip(&old));
    assert_eq!(units::flag(&new, "--arena-root"), Some("/workspace/arena"));
    assert_eq!(
        units::effective_paths(&[LIVE, DROP], "ReadWritePaths"),
        units::effective_paths(&[LIVE, OLD_DROP], "ReadWritePaths")
    );
    let p = units::exec_argv(&[PRIMARY, PRIMARY_DROP]);
    assert_eq!(p[0], new[0], "primary + fallback run the same binary");
    assert_eq!(units::flag(&p, "--bind"), Some("127.0.0.1:18080"));
    assert_eq!(units::flag(&new, "--bind"), Some("127.0.0.1:8080"));
    assert_eq!(units::flag(&p, "--arena-root"), Some("/workspace/arena"));
    // /workspace/arena must stay readable under ProtectSystem=strict (not
    // in ReadOnlyPaths' exclusions, not in InaccessiblePaths).
    assert!(units::effective_paths(&[PRIMARY, PRIMARY_DROP], "InaccessiblePaths").is_empty());
}

#[test]
fn real_deploy_candidate() {
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
    let tmp = std::env::temp_dir().join(format!("real-343-{}", std::process::id()));
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
    let reset = || {
        std::fs::write(&store, store_json(0)).unwrap();
        chmod(&store, 0o600);
    };
    let arena = tmp.join("arena");
    std::fs::create_dir_all(arena.join("champions")).unwrap();
    let board = arena.join("board.jsonl");
    std::fs::write(&board, ARENA_BOARD).unwrap();
    let staged = units::exec_argv(&[LIVE, DROP]);
    let cookie = format!("galion_admin={OLD_TOKEN}");
    let t0 = Instant::now();

    // ---- B + C on a fresh process ----
    let at = "127.0.0.1:22130";
    reset();
    {
        let _k = spawn(&remap(&staged, &exe, &tmp, at), at);
        let h = local(at, "GET", "/api/v1/health");
        assert_eq!(h.code, 200, "C: health {}", h.body);
        for p in BOARD_ROUTES {
            let r = local(at, "GET", p);
            assert_eq!(r.code, 200, "C {p}: {}", r.body);
        }
        let root = local(at, "GET", "/").code;
        assert!(root == 200 || root == 302, "C: / {root}");
        arena_checks(at, &board, &ids_of(ARENA_BOARD), "B");
        let r = req(at, "GET", "/api/arena", &[("Host", REMOTE)], b"");
        assert_eq!(r.code, 401, "B: remote no cookie gated first");
        let r = req(
            at,
            "GET",
            "/api/arena",
            &[("Host", REMOTE), ("Cookie", &cookie)],
            b"",
        );
        assert_eq!(r.code, 200, "B: remote + paired cookie {}", r.body);
        let r = req(at, "GET", "/api/arena", &[("Host", "evil.example")], b"");
        assert_eq!(r.code, 401, "B: foreign host");
        eprintln!("B: remote /api/arena no cookie 401, paired cookie 200, foreign host 401; C: {} board routes 200, / ok", BOARD_ROUTES.len());

        // ---- A: lockout in the SAME process (RW store) ----
        let wrong: Vec<u16> = (0..5)
            .map(|i| code(at, &format!("WRONG{i}")).code)
            .collect();
        assert_eq!(wrong, vec![401; 5], "A: 5 wrong codes");
        let w6 = code(at, "WRONG5");
        let right = code(at, CODE);
        assert_eq!((w6.code, right.code), (429, 429), "A: locked");
        assert_eq!(w6.body, right.body, "A: identical 429");
        assert!(!has_cookie(&right));
        let raw = std::fs::read_to_string(&store).unwrap();
        assert!(
            raw.contains("\"lockedUntil\":") && raw.contains("\"fails\":5"),
            "{raw}"
        );
        assert!(!raw.contains(CODE) && !raw.contains(DEVICE), "hashes only");
        // Arena + paired cookie still serve during the admin lock.
        let r = req(
            at,
            "GET",
            "/api/arena",
            &[("Host", REMOTE), ("Cookie", &cookie)],
            b"",
        );
        assert_eq!(r.code, 200, "A: paired cookie works during lock");
        assert_eq!(local(at, "GET", "/api/arena").code, 200);
        eprintln!("A: 5x401 -> 429 wrong+right identical, lockedUntil persisted, hashes only; arena + paired cookie still 200 during lock");
    }

    // ---- A2: RO store dir (F-313-1 shape) ----
    let at2 = "127.0.0.1:22131";
    reset();
    let before = std::fs::read(&store).unwrap();
    chmod(&sdir, 0o500);
    chmod(&mdir, 0o500);
    {
        let _k = spawn(&remap(&staged, &exe, &tmp, at2), at2);
        let r1 = code(at2, CODE);
        let w1 = code(at2, "WRONGRO");
        // Root ignores dir modes; only assert the F-313-1 contract when the
        // store is really unwritable for us.
        let ro = std::fs::File::create(sdir.join(".probe")).is_err();
        let _ = std::fs::remove_file(sdir.join(".probe"));
        if ro {
            assert_eq!((r1.code, w1.code), (503, 503), "A2: no oracle");
            assert_eq!(r1.body, w1.body, "A2: identical 503");
            let more: Vec<u16> = (0..4).map(|i| code(at2, &format!("W{i}")).code).collect();
            assert_eq!(*more.last().unwrap(), 429, "A2: locks {more:?}");
            assert_eq!(
                std::fs::read(&store).unwrap(),
                before,
                "A2: store unchanged"
            );
            eprintln!("A2: RO store right/wrong 503 identical, then {more:?}, store unchanged");
        } else {
            eprintln!("A2: running as root, RO dir not enforced; covered by real_admin_lockout B");
        }
        assert_eq!(
            local(at2, "GET", "/api/arena").code,
            200,
            "A2: arena unaffected"
        );
    }
    chmod(&sdir, 0o700);
    chmod(&mdir, 0o700);

    // ---- D: missing arena root ----
    let at3 = "127.0.0.1:22132";
    reset();
    std::fs::rename(&arena, tmp.join("arena.off")).unwrap();
    {
        let _k = spawn(&remap(&staged, &exe, &tmp, at3), at3);
        assert_eq!(local(at3, "GET", "/api/v1/health").code, 200, "D: health");
        let a = local(at3, "GET", "/api/arena");
        assert!(
            a.code == 200 || a.code == 500,
            "D: soft {} {}",
            a.code,
            a.body
        );
        assert!(!a.body.contains(tmp.to_str().unwrap()), "D: no path leak");
        assert_eq!(
            local(at3, "GET", "/arena").code,
            200,
            "D: page still renders"
        );
        eprintln!(
            "D: no arena root -> health 200, /api/arena {} {:?}, /arena 200",
            a.code,
            &a.body[..a.body.len().min(120)]
        );
    }
    std::fs::rename(tmp.join("arena.off"), &arena).unwrap();

    // ---- live leg (read-only) ----
    if std::env::var("CONSOLE_CANDIDATE_LIVE").as_deref() == Ok("1") {
        let vps = std::env::var("CONSOLE_CANDIDATE_VPS").unwrap_or_else(|_| "slavik-vps".into());
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
        let live = String::from_utf8_lossy(&out.stdout).to_string();
        std::fs::write(&board, &live).unwrap();
        let ids = ids_of(&live);
        let tail: Vec<String> = ids.iter().rev().take(16).rev().cloned().collect();
        let at4 = "127.0.0.1:22133";
        reset();
        let _k = spawn(&remap(&staged, &exe, &tmp, at4), at4);
        eprintln!(
            "live: board.jsonl {} lines, {} ids",
            live.lines().count(),
            ids.len()
        );
        arena_checks(at4, &board, &tail, "live");
    }

    let _ = std::fs::remove_dir_all(&tmp);
    eprintln!(
        "real_deploy_candidate total {} ms",
        t0.elapsed().as_millis()
    );
}
