// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of `galion-deploy dist-install` (slot 379, D-356-3).
//! Gated by `GALION_REAL=1`. Ports: `GALION_PORT_BASE` (default 22490).
//!
//! * `GALION_DIST_SRC=DIR`: a copy of the live console-dist (taken with a
//!   read-only `tar -cf -` over ssh, modes kept: 52 files 666, 6 entries 777);
//!   without it a synthetic tree of the same shape is used.
//! * `GALION_SERVE_BIN=PATH`: a release `galion-console-static`; the installed
//!   `static/` is served on 127.0.0.1:PORT_BASE and every file is fetched over
//!   real HTTP, also while a second install swaps the tree underneath.
//! * `GALION_REAL_VPS=HOST`: read-only `curl` GETs of the same paths from the
//!   live console on the VPS (127.0.0.1:8080); bytes must match the install.

use galion_deploy::sha256;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_galion-deploy");

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn run(args: &[&str]) -> (i32, String) {
    let o = Command::new(BIN).args(args).output().unwrap();
    let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&o.stderr));
    (o.status.code().unwrap_or(-1), s)
}

fn walk(root: &Path, out: &mut Vec<PathBuf>) {
    let mut v: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    v.sort();
    for p in v {
        out.push(p.clone());
        if std::fs::symlink_metadata(&p).unwrap().is_dir() {
            walk(&p, out);
        }
    }
}

fn mode(p: &Path) -> u32 {
    std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o7777
}

fn synthetic(root: &Path) {
    for d in ["static/__grok/install", "node_modules/srvx/dist/adapters"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    for (f, m) in [
        ("static/favicon.svg", 0o666),
        ("static/manifest.webmanifest", 0o666),
        ("static/__grok/install/styles.css", 0o666),
        ("static/app.js", 0o644),
        ("node_modules/srvx/dist/adapters/node.mjs", 0o666),
    ] {
        std::fs::write(root.join(f), format!("/* {f} */")).unwrap();
        std::fs::set_permissions(root.join(f), std::fs::Permissions::from_mode(m)).unwrap();
    }
    for d in [
        "node_modules/srvx",
        "node_modules/srvx/dist",
        "node_modules/srvx/dist/adapters",
    ] {
        std::fs::set_permissions(root.join(d), std::fs::Permissions::from_mode(0o777)).unwrap();
    }
}

fn get(port: u16, path: &str) -> Option<(u16, Vec<u8>)> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        s,
        "GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).ok()?;
    let end = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&buf[..end]);
    let code = head.split_whitespace().nth(1)?.parse().ok()?;
    Some((code, buf[end + 4..].to_vec()))
}

fn url_path(rel: &Path) -> String {
    let mut s = String::new();
    for c in rel.components() {
        s.push('/');
        for b in c.as_os_str().to_string_lossy().bytes() {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                s.push(b as char);
            } else {
                s.push_str(&format!("%{b:02X}"));
            }
        }
    }
    s
}

#[test]
fn real_dist_install() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let port: u16 = std::env::var("GALION_PORT_BASE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(22490);
    let work = std::env::temp_dir().join(format!("gd-real-dist-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let src = match std::env::var("GALION_DIST_SRC") {
        Ok(s) => PathBuf::from(s),
        Err(_) => {
            let s = work.join("src");
            synthetic(&s);
            s
        }
    };
    let dest = work.join("console-dist");

    // 1. the source as unpacked: dist-check flags it
    let (c, out) = run(&["dist-check", src.to_str().unwrap()]);
    let flagged = out.lines().filter(|l| !l.contains("dist-check:")).count();
    eprintln!(
        "source dist-check exit {c}, {flagged} writable entries; last line: {}",
        out.lines().last().unwrap_or("")
    );
    assert_eq!(c, 1);
    assert!(flagged > 0);

    // 2. install (umask 077 inside the binary must not matter)
    let t0 = Instant::now();
    let (c, out) = run(&[
        "dist-install",
        src.to_str().unwrap(),
        dest.to_str().unwrap(),
    ]);
    eprintln!("install exit {c} in {:?}: {}", t0.elapsed(), out.trim());
    assert_eq!(c, 0, "{out}");
    let (c, out) = run(&["dist-check", dest.to_str().unwrap()]);
    eprintln!("installed dist-check exit {c}: {}", out.trim());
    assert_eq!(c, 0);
    let (mut src_list, mut dst_list) = (Vec::new(), Vec::new());
    walk(&src, &mut src_list);
    walk(&dest, &mut dst_list);
    assert_eq!(src_list.len(), dst_list.len());
    let (mut n644, mut n755d, mut n755f) = (0, 0, 0);
    for (s, d) in src_list.iter().zip(&dst_list) {
        assert_eq!(
            s.strip_prefix(&src).unwrap(),
            d.strip_prefix(&dest).unwrap()
        );
        let md = std::fs::symlink_metadata(d).unwrap();
        let m = mode(d);
        if md.is_dir() {
            assert_eq!(m, 0o755, "{}", d.display());
            n755d += 1;
        } else if md.is_file() {
            assert_eq!(std::fs::read(s).unwrap(), std::fs::read(d).unwrap());
            let want = if mode(s) & 0o111 != 0 { 0o755 } else { 0o644 };
            assert_eq!(m, want, "{}", d.display());
            if want == 0o644 {
                n644 += 1
            } else {
                n755f += 1
            }
        }
    }
    eprintln!("installed modes: {n755d} dirs 755, {n644} files 644, {n755f} files 755; contents identical");

    // 3. serve the installed static/ with the real console binary
    let static_rel = if dest.join("static").is_dir() {
        "static"
    } else {
        "."
    };
    let serve_root = dest.join(static_rel);
    let files: Vec<PathBuf> = dst_list
        .iter()
        .filter(|p| p.starts_with(&serve_root) && p.is_file())
        .map(|p| p.strip_prefix(&serve_root).unwrap().to_path_buf())
        .collect();
    if let Ok(serve) = std::env::var("GALION_SERVE_BIN") {
        let shell = serve_root.join("_shell.html");
        // the live tree has no HTML shell (the Rust console renders pages);
        // console-static needs one to start, so the test adds a 644 stub
        let made_shell = !shell.exists();
        if made_shell {
            std::fs::write(&shell, "<!doctype html><title>t</title>").unwrap();
            std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        let mut child = Command::new(&serve)
            .args([
                "--bind",
                &format!("127.0.0.1:{port}"),
                "--root",
                serve_root.to_str().unwrap(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let up = (0..100).any(|_| {
            std::thread::sleep(Duration::from_millis(50));
            TcpStream::connect(("127.0.0.1", port)).is_ok()
        });
        if !up {
            let _ = child.kill();
            let mut e = String::new();
            let _ = child.stderr.take().unwrap().read_to_string(&mut e);
            panic!("console-static did not start: {e}");
        }
        let mut ok = 0;
        for rel in &files {
            let (code, body) = get(port, &url_path(rel)).expect("http");
            if code == 200 {
                assert_eq!(
                    body,
                    std::fs::read(serve_root.join(rel)).unwrap(),
                    "{}",
                    rel.display()
                );
                ok += 1;
            } else {
                eprintln!("  {code} {}", url_path(rel));
            }
        }
        eprintln!(
            "served {ok}/{} installed static files over HTTP on :{port}, bytes identical",
            files.len()
        );
        assert!(
            ok * 10 >= files.len() * 9,
            "most installed files must be served"
        );

        // 4. swap under load: second install while a client keeps fetching
        let probe = files
            .iter()
            .find(|p| p.ends_with("favicon.svg"))
            .cloned()
            .unwrap_or_else(|| files[0].clone());
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let st = stop.clone();
        let pp = url_path(&probe);
        let h = std::thread::spawn(move || {
            let (mut n, mut other) = (0u32, Vec::new());
            while !st.load(std::sync::atomic::Ordering::Relaxed) {
                match get(port, &pp) {
                    Some((200, _)) => n += 1,
                    Some((c, _)) => other.push(c.to_string()),
                    None => other.push("io".to_string()),
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            (n, other)
        });
        std::thread::sleep(Duration::from_millis(300));
        let (c, out) = run(&[
            "dist-install",
            src.to_str().unwrap(),
            dest.to_str().unwrap(),
        ]);
        std::thread::sleep(Duration::from_millis(300));
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let (n, other) = h.join().unwrap();
        eprintln!(
            "swap under load: install exit {c} ({}); client {n} x 200, other {other:?}",
            out.trim()
        );
        assert_eq!(c, 0);
        assert!(out.contains(".old-"), "old tree kept: {out}");
        assert!(
            other.len() <= 3,
            "at most a few misses in the rename gap: {other:?}"
        );
        std::thread::sleep(Duration::from_millis(200));
        let (code, _) = (0..5)
            .find_map(|_| {
                let r = get(port, &url_path(&probe));
                if r.is_none() {
                    std::thread::sleep(Duration::from_millis(200));
                }
                r
            })
            .expect("server answers after the swap");
        assert_eq!(code, 200, "served after the swap");
        let _ = child.kill();
        let _ = child.wait();
        if made_shell {
            let _ = std::fs::remove_file(&shell);
        }
    } else {
        eprintln!("GALION_SERVE_BIN unset: HTTP part skipped");
        let (c, _) = run(&[
            "dist-install",
            src.to_str().unwrap(),
            dest.to_str().unwrap(),
        ]);
        assert_eq!(c, 0);
    }

    // 5. rollback to the first install
    let old: PathBuf = std::fs::read_dir(&work)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("console-dist.old-")
        })
        .expect("old tree");
    let (c, out) = run(&[
        "dist-rollback",
        dest.to_str().unwrap(),
        old.to_str().unwrap(),
    ]);
    eprintln!("rollback exit {c}: {}", out.trim());
    assert_eq!(c, 0);
    assert!(!old.exists());
    assert_eq!(run(&["dist-check", dest.to_str().unwrap()]).0, 0);

    // 6. compare with the live console (read-only GETs)
    if let Ok(host) = std::env::var("GALION_REAL_VPS") {
        let picks: Vec<&PathBuf> = files
            .iter()
            .filter(|p| {
                let n = p.to_string_lossy();
                n.ends_with("favicon.svg")
                    || n.ends_with("manifest.webmanifest")
                    || n.ends_with("styles.css")
                    || n.ends_with("og.jpg")
            })
            .collect();
        let mut cmd = String::new();
        for p in &picks {
            cmd.push_str(&format!(
                "printf '%s ' {p}; curl -s -o /dev/null -w '%{{http_code}} ' 'http://127.0.0.1:8080{p}'; curl -s 'http://127.0.0.1:8080{p}' | sha256sum | cut -c1-64; ",
                p = url_path(p)
            ));
        }
        // two GETs per path, output to stdout only; nothing is written on the host
        let o = Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=15",
                &host,
                &cmd,
            ])
            .output()
            .unwrap();
        let live = String::from_utf8_lossy(&o.stdout);
        let mut same = 0;
        for (p, line) in picks.iter().zip(live.lines()) {
            let mine = sha256::file_hex(&serve_root.join(p)).unwrap();
            let f: Vec<&str> = line.split_whitespace().collect();
            eprintln!("  live {line}  install {}", &mine[..16]);
            if f.len() == 3 && f[1] == "200" && f[2] == mine {
                same += 1;
            }
        }
        eprintln!(
            "live console :8080 vs install: {same}/{} identical",
            picks.len()
        );
        assert_eq!(same, picks.len());
    }
    let _ = std::fs::remove_dir_all(&work);
}
