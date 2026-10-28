// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-binary test (slot 34, port 19342; `CONSOLE_STATIC_ADDR` overrides).
//! Replaces `scripts/real/console-static.sh` (Rust only, no shell). Gated: runs only with
//! `GALION_REAL=1`. Asset set (slot 349): `CONSOLE_STATIC_OUT`, else
//! /workspace/swarm/artifacts/console-static/out (client/ + routes.txt); a set that is
//! found must match deploy/console-static/asset-set.2169d1c.manifest (sha256 + size)
//! and every built file is then checked byte-for-byte over HTTP. With no set on the
//! machine the test generates a minimal fixture set and prints `asset set: FIXTURE`
//! (never a silent pass); `CONSOLE_STATIC_REQUIRE_ARTIFACT=1` turns that into a failure.
//! Scanner paths are also replayed from
//! the nerve capture journal when `/workspace/swarm/nerve-capture` exists.
//!
//! With `CONSOLE_STATIC_LIVE=1` it also opens a read-only SSH forward
//! (`CONSOLE_STATIC_UP`, default 127.0.0.1:19341 -> `CONSOLE_STATIC_VPS` (`user@host`)
//! 127.0.0.1:8080; `ssh -N`,
//! nothing written on the VPS), uses it as the API upstream and compares
//! each API GET through the front with the same GET sent directly. Only GET
//! is sent upstream; the markers stream is never opened (its handler bumps a
//! live counter).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn addr() -> String {
    std::env::var("CONSOLE_STATIC_ADDR").unwrap_or_else(|_| "127.0.0.1:19342".into())
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn status(path: &str) -> u16 {
    let mut s = TcpStream::connect(addr()).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out[9..12]).parse().unwrap()
}

/// Raw GET/POST: (status, lower-cased head, body). Status 0 if no answer.
fn fetch(at: &str, method: &str, path: &str, extra: &str) -> (u16, String, Vec<u8>) {
    let Ok(mut s) = TcpStream::connect(at) else {
        return (0, String::new(), Vec::new());
    };
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let body = if method == "POST" { "x" } else { "" };
    let len = if method == "POST" {
        "Content-Length: 1\r\n"
    } else {
        ""
    };
    let _ = write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{extra}{len}Connection: close\r\n\r\n{body}"
    );
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let Some(p) = out.windows(4).position(|w| w == b"\r\n\r\n") else {
        return (0, String::new(), Vec::new());
    };
    let head = String::from_utf8_lossy(&out[..p]).to_ascii_lowercase();
    let code = head.get(9..12).and_then(|c| c.parse().ok()).unwrap_or(0);
    let mut body = out[p + 4..].to_vec();
    if head.contains("transfer-encoding: chunked") {
        let mut plain = Vec::new();
        let mut rest = &body[..];
        while let Some(i) = rest.windows(2).position(|w| w == b"\r\n") {
            let n =
                usize::from_str_radix(String::from_utf8_lossy(&rest[..i]).trim(), 16).unwrap_or(0);
            if n == 0 || rest.len() < i + 2 + n {
                break;
            }
            plain.extend_from_slice(&rest[i + 2..i + 2 + n]);
            rest = &rest[(i + 4 + n).min(rest.len())..];
        }
        body = plain;
    }
    (code, head, body)
}

fn header<'a>(head: &'a str, name: &str) -> &'a str {
    head.lines()
        .find_map(|l| l.strip_prefix(name).and_then(|r| r.strip_prefix(':')))
        .map(str::trim)
        .unwrap_or("")
}

/// Paths the Vite dev server logged for scanner traffic in the capture.
fn captured_probes(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return found;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("jsonl") {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        for (marker, end) in [("Instead of ", ','), ("request id \\\"", '\\')] {
            let mut rest = text.as_str();
            while let Some(i) = rest.find(marker) {
                rest = &rest[i + marker.len()..];
                let path: String = rest
                    .chars()
                    .take_while(|c| *c != end && *c != ' ' && *c != '"')
                    .collect();
                if path.starts_with('/') && !found.contains(&path) {
                    found.push(path);
                }
            }
        }
    }
    found
}

/// Where the asset set came from.
#[derive(PartialEq)]
enum Mode {
    /// The real pre-built console set, verified against the committed manifest.
    Artifact,
    /// No real set on this machine: a minimal set generated by the test itself.
    Fixture,
}

const DEFAULT_OUT: &str = "/workspace/swarm/artifacts/console-static/out";

fn repo_file(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

/// (sha256 hex, bytes, path under out/) from the committed manifest.
fn manifest() -> Vec<(String, u64, String)> {
    let p = repo_file("deploy/console-static/asset-set.2169d1c.manifest");
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let mut it = l.splitn(3, ' ');
            let h = it.next().unwrap().to_string();
            let n = it.next().unwrap().parse().unwrap();
            let f = it.next().unwrap().to_string();
            (h, n, f)
        })
        .collect()
}

/// Checks every manifest entry (size + sha256) and that `client/` has no extra
/// files. Returns the list of problems (empty = identical set).
fn verify(out: &Path, want: &[(String, u64, String)]) -> Vec<String> {
    let mut bad = Vec::new();
    for (h, n, f) in want {
        match std::fs::read(out.join(f)) {
            Ok(b) if b.len() as u64 != *n => bad.push(format!("{f}: {} bytes, want {n}", b.len())),
            Ok(b) if console_static::sha256::hex(&b) != *h => {
                bad.push(format!("{f}: sha256 differs"))
            }
            Ok(_) => {}
            Err(e) => bad.push(format!("{f}: {e}")),
        }
    }
    fn walk(root: &Path, d: &Path, acc: &mut Vec<String>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(root, &p, acc);
            } else {
                acc.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
            }
        }
    }
    let mut have = Vec::new();
    walk(out, &out.join("client"), &mut have);
    for f in have {
        if !want.iter().any(|(_, _, w)| *w == f) {
            bad.push(format!("{f}: not in manifest"));
        }
    }
    bad
}

/// Removes the generated fixture directory.
struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Finds the asset set, never silently. Order:
/// 1. `CONSOLE_STATIC_OUT` (explicit: must exist and match the manifest, else FAIL);
/// 2. the default box path (if present it must match the manifest, else FAIL);
/// 3. none found: FAIL when `CONSOLE_STATIC_REQUIRE_ARTIFACT=1`, otherwise generate
///    a minimal fixture set (shell, icons, committed routes.2169d1c.txt) and say so.
fn asset_set() -> (PathBuf, Mode, Option<Tmp>) {
    let want = manifest();
    let explicit = std::env::var("CONSOLE_STATIC_OUT").ok();
    let cand = PathBuf::from(explicit.clone().unwrap_or_else(|| DEFAULT_OUT.into()));
    let present = cand.join("client/_shell.html").is_file();
    if present || explicit.is_some() {
        assert!(
            present,
            "CONSOLE_STATIC_OUT={} has no client/_shell.html (prerequisite: the asset set; \
             see docs/real-tests/console-static-artifact-test.md for the restore recipe)",
            cand.display()
        );
        let bad = verify(&cand, &want);
        assert!(
            bad.is_empty(),
            "asset set at {} does not match deploy/console-static/asset-set.2169d1c.manifest: {bad:?}",
            cand.display()
        );
        eprintln!(
            "asset set: ARTIFACT {} ({} files verified by sha256 against the manifest)",
            cand.display(),
            want.len()
        );
        return (cand, Mode::Artifact, None);
    }
    assert!(
        std::env::var("CONSOLE_STATIC_REQUIRE_ARTIFACT").as_deref() != Ok("1"),
        "CONSOLE_STATIC_REQUIRE_ARTIFACT=1 but no asset set at {DEFAULT_OUT} and \
         CONSOLE_STATIC_OUT unset. Prerequisite: restore the pre-built console set \
         (docs/real-tests/console-static-artifact-test.md) or set CONSOLE_STATIC_OUT"
    );
    let dir = std::env::temp_dir().join(format!(
        "real-console-static-fixture-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let client = dir.join("client");
    std::fs::create_dir_all(&client).unwrap();
    std::fs::write(
        client.join("_shell.html"),
        "<!doctype html><html><head><title>Galion console (fixture)</title></head><body></body></html>",
    )
    .unwrap();
    std::fs::write(
        client.join("favicon.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1 1\"/>",
    )
    .unwrap();
    std::fs::write(client.join("manifest.webmanifest"), "{\"name\":\"Galion\"}").unwrap();
    std::fs::write(client.join("og.jpg"), [0xFFu8, 0xD8, 0xFF, 0xD9]).unwrap();
    std::fs::copy(
        repo_file("deploy/console-static/routes.2169d1c.txt"),
        dir.join("routes.txt"),
    )
    .unwrap();
    eprintln!(
        "asset set: FIXTURE {} (no real set at {DEFAULT_OUT}; generated shell + 3 icons + \
         committed routes.2169d1c.txt). Real asset bytes are NOT checked in this mode; set \
         CONSOLE_STATIC_REQUIRE_ARTIFACT=1 to make a missing set a failure",
        dir.display()
    );
    (dir.clone(), Mode::Fixture, Some(Tmp(dir)))
}

#[test]
fn real_binary_serves_console_and_404s_probes() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let (out, mode, _fixture) = asset_set();
    let live = std::env::var("CONSOLE_STATIC_LIVE").as_deref() == Ok("1");
    let up = std::env::var("CONSOLE_STATIC_UP").unwrap_or_else(|_| "127.0.0.1:19341".into());
    let mut upstream: Vec<String> = Vec::new();
    let _tunnel = if live {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        let vps = std::env::var("CONSOLE_STATIC_VPS")
            .expect("set CONSOLE_STATIC_VPS=user@host (the VPS) with CONSOLE_STATIC_LIVE=1");
        let child = Command::new("ssh")
            .args(["-i", &format!("{home}/.ssh/id_ed25519_vps")])
            .args([
                "-o",
                "IdentitiesOnly=yes",
                "-o",
                "BatchMode=yes",
                "-o",
                "ExitOnForwardFailure=yes",
            ])
            .arg("-N")
            .arg("-L")
            .arg(format!("{up}:127.0.0.1:8080"))
            .arg(vps)
            .stdin(Stdio::null())
            .spawn()
            .expect("ssh");
        let k = Kill(child);
        let t0 = Instant::now();
        while fetch(&up, "GET", "/api/v1/health", "").0 != 200 {
            assert!(
                t0.elapsed() < Duration::from_secs(20),
                "ssh forward did not come up"
            );
            std::thread::sleep(Duration::from_millis(200));
        }
        upstream = vec!["--upstream".into(), up.clone()];
        Some(k)
    } else {
        None
    };
    let child = Command::new(env!("CARGO_BIN_EXE_galion-console-static"))
        .args(["--bind", &addr(), "--quiet", "--root"])
        .arg(out.join("client"))
        .arg("--routes")
        .arg(out.join("routes.txt"))
        .args(&upstream)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    let _guard = Kill(child);
    let t0 = Instant::now();
    while TcpStream::connect(addr()).is_err() {
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "server did not start"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let routes = std::fs::read_to_string(out.join("routes.txt")).unwrap();
    let pages: Vec<&str> = routes
        .lines()
        .filter_map(|l| l.strip_prefix("page "))
        .collect();
    assert!(pages.len() >= 10);
    for p in &pages {
        assert_eq!(status(p), 200, "{p}");
    }
    for p in ["/favicon.svg", "/manifest.webmanifest", "/og.jpg"] {
        assert_eq!(status(p), 200, "{p}");
    }
    let mut probes: Vec<String> = [
        "/.env",
        "/public/.env",
        "/phpinfo.php",
        "/public/phpinfo.php",
        "/.git/config",
        "/@fs/opt/galion/console-ide/.git/config",
        "/opt/galion/console-ide/.git/config",
        "/package.json",
        "/vite.config.ts",
        "/src/lib/host.ts",
        "/@vite/client",
        "/_shell.html",
        "/health",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let captured = captured_probes(Path::new("/workspace/swarm/nerve-capture"));
    eprintln!("captured scanner paths: {captured:?}");
    probes.extend(captured);
    for p in &probes {
        assert_eq!(status(p), 404, "{p}");
    }
    eprintln!(
        "real: {} pages 200, {} probes 404",
        pages.len(),
        probes.len()
    );

    // Methods and conditional GET (as the shell script did).
    let at = addr();
    if mode == Mode::Artifact {
        // Every built file is served byte-for-byte (sha256 of the body = manifest).
        let mut n = 0;
        for (h, len, f) in manifest() {
            let Some(rel) = f.strip_prefix("client/") else {
                continue;
            };
            if rel == "_shell.html" {
                continue; // served for page routes, never under its own name
            }
            let (c, _, body) = fetch(&at, "GET", &format!("/{rel}"), "");
            assert_eq!(c, 200, "/{rel}");
            assert_eq!(body.len() as u64, len, "/{rel} length");
            assert_eq!(console_static::sha256::hex(&body), h, "/{rel} sha256");
            n += 1;
        }
        eprintln!("real: {n} built assets served byte-identical (sha256)");
        assert!(n >= 30, "{n}");
    }
    let (c, _, _) = fetch(&at, "POST", "/agents", "");
    eprintln!("real: POST /agents -> {c}");
    assert_eq!(c, 405);
    let (c, _, _) = fetch(&at, "POST", "/.env", "");
    eprintln!("real: POST /.env -> {c}");
    assert!(c == 404 || c == 405, "{c}");
    let (_, head, _) = fetch(&at, "GET", "/favicon.svg", "");
    let etag = header(&head, "etag").to_string();
    assert!(!etag.is_empty());
    let (c, _, _) = fetch(
        &at,
        "GET",
        "/favicon.svg",
        &format!("If-None-Match: {etag}\r\n"),
    );
    eprintln!("real: If-None-Match favicon -> {c}");
    assert_eq!(c, 304);

    if live {
        // API GETs through the front vs the same GET sent directly (read-only).
        for p in [
            "/api/v1/health",
            "/api/v1/catalog",
            "/api/v1/services",
            "/api/v1/protocol",
            "/api/v1/markers",
            "/api/hermes/health",
            "/api/nope",
            "/api/.env",
        ] {
            let (ca, ha, ba) = fetch(&at, "GET", p, "");
            let (cb, hb, bb) = fetch(&up, "GET", p, "");
            let same = if ba == bb {
                "same-body".to_string()
            } else {
                format!("body-differs({}/{})", ba.len(), bb.len())
            };
            eprintln!(
                "real: {p:<22} ours {ca} {:<32} live {cb} {:<32} {same}",
                header(&ha, "content-type"),
                header(&hb, "content-type")
            );
            assert_eq!(ca, cb, "{p}: status differs from live");
            // The marker ring and Hermes health (uptime, clock) change as the
            // console runs; the /api/v1 catalog payloads
            // are a pure function of the catalog.
            if p.starts_with("/api/v1/") && !p.contains("markers") && ca == 200 {
                assert_eq!(ba, bb, "{p}: body differs from live");
            }
        }
    }
}
