// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 393 (F-377-1): the build stamp in `openmind-nerve --version` and
//! `/health`, proven on real release builds, with the reproducible-build check
//! kept (the stamp never reads the wall clock) and the size delta vs the
//! unstamped base measured (F-376-1: headroom is thin). Ported from the
//! cortex real test (swarm/cortex-version-commit b36dc358).
//!
//! Gated: `GALION_REAL=1`. x86_64 musl release, default features (like the
//! live nerve), five builds:
//!
//!   A  `git archive HEAD` in dir 1 + GALION_BUILD_COMMIT/SOURCE_DATE_EPOCH
//!   B  `git archive HEAD` in a different dir/depth + same env
//!   C  `git clone` checkout of HEAD, no env (stamp from git)
//!   D  `git archive HEAD`, no env (no git -> `unknown`)
//!   Z  `git archive $NERVE_STAMP_BASE` (default 9baa450c, no stamp code):
//!      size delta only; `NERVE_STAMP_BASE=` (empty) skips it.
//!
//! Asserts sha256(A) = sha256(B) = sha256(C), D differs, then serves A, C, D
//! on 127.0.0.1:22630-22632 (REAL_NERVE_PORT_BASE moves the block) and checks
//! the startup line and `/health` `commit` / `build`.

use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const TARGET: &str = "x86_64-unknown-linux-musl";

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn log(m: impl AsRef<str>) {
    eprintln!(
        "[real_build_stamp {}] {}",
        zagreb_time::now_rfc3339(),
        m.as_ref()
    );
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn git(args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(root())
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}");
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

fn archive_into(commit: &str, dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let mut a = Command::new("git")
        .arg("-C")
        .arg(root())
        .args(["archive", commit])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let t = Command::new("tar")
        .arg("-x")
        .arg("-C")
        .arg(dir)
        .stdin(a.stdout.take().unwrap())
        .status()
        .unwrap();
    assert!(
        a.wait().unwrap().success() && t.success(),
        "archive {commit}"
    );
}

/// Release build in `src` with its own target dir; returns the binary path.
fn build(src: &Path, target_dir: &Path, stamp: Option<(&str, &str)>) -> PathBuf {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or("cargo".into()));
    c.current_dir(src)
        .args([
            "build",
            "-j",
            "2",
            "--release",
            "--locked",
            "--offline",
            "-p",
            "openmind-nerve",
        ])
        .args(["--target", TARGET])
        .env("CARGO_TARGET_DIR", target_dir)
        .env("CARGO_INCREMENTAL", "0")
        .env_remove("GALION_BUILD_COMMIT")
        .env_remove("SOURCE_DATE_EPOCH")
        .env_remove("RUSTFLAGS");
    if let Some((commit, epoch)) = stamp {
        c.env("GALION_BUILD_COMMIT", commit)
            .env("SOURCE_DATE_EPOCH", epoch);
    }
    let t = Instant::now();
    let st = c.status().unwrap();
    assert!(st.success(), "cargo build in {}", src.display());
    log(format!(
        "built {} in {:.1} s",
        src.display(),
        t.elapsed().as_secs_f64()
    ));
    target_dir.join(TARGET).join("release/openmind-nerve")
}

fn sha(p: &Path) -> String {
    let o = Command::new("sha256sum").arg(p).output().unwrap();
    assert!(o.status.success());
    String::from_utf8(o.stdout).unwrap()[..64].to_string()
}

fn size(p: &Path) -> u64 {
    std::fs::metadata(p).unwrap().len()
}

fn version(p: &Path) -> String {
    let o = Command::new(p).arg("--version").output().unwrap();
    assert!(o.status.success());
    String::from_utf8(o.stdout).unwrap().trim_end().to_string()
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn health(port: u16) -> Option<Value> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        s,
        "GET /health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    serde_json::from_str(out.split_once("\r\n\r\n")?.1).ok()
}

/// Serve `bin` dry-run, no systemd/journal, one dead component; returns the
/// process guard, the startup line and the first `/health` body.
fn serve(bin: &Path, port: u16, dir: &Path) -> (Kill, String, Value) {
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::write(
        dir.join("components.json"),
        r#"[{"name":"stamp-real-dead","port":1,"path":"/health","gaia":"DEMETER","system":"tuner"}]"#,
    )
    .unwrap();
    let mut child = Command::new(bin)
        .args(["serve", "--listen", &format!("127.0.0.1:{port}")])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("NERVE_DATA_DIR", dir.join("data"))
        .env("NERVE_COMPONENTS", dir.join("components.json"))
        .env("NERVE_DRY_RUN", "1")
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_INTERVAL", "3600")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let k = Kill(child);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for l in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tx.send(l).is_err() {
                break;
            }
        }
    });
    let line = loop {
        let l = rx
            .recv_timeout(Duration::from_secs(30))
            .unwrap_or_else(|e| panic!("no startup line on {port}: {e}"));
        if l.contains(" listening on ") {
            break l;
        }
    };
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(15) {
        if let Some(h) = health(port) {
            log(format!(
                "{port} /health after {} ms",
                t.elapsed().as_millis()
            ));
            return (k, line, h);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("no /health on {port}");
}

#[test]
fn real_build_stamp_reproducible() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let base: u16 = std::env::var("REAL_NERVE_PORT_BASE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(22630);
    let head = git(&["rev-parse", "HEAD"]);
    let ct = git(&["show", "-s", "--format=%ct", "HEAD"]);
    let date = zagreb_time::from_unix(ct.parse().unwrap());
    let tmp = std::env::var("NERVE_STAMP_REAL_TMP")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::temp_dir().join(format!("nerve-stamp-{}", std::process::id()))
        });
    let _ = std::fs::remove_dir_all(&tmp);
    log(format!(
        "HEAD {head} committed {date}; tmp {}",
        tmp.display()
    ));

    let a_src = tmp.join("a/src");
    let b_src = tmp.join("b/elsewhere/deeper/src");
    let c_src = tmp.join("c/checkout");
    let d_src = tmp.join("d/src");
    archive_into(&head, &a_src);
    archive_into(&head, &b_src);
    archive_into(&head, &d_src);
    let st = Command::new("git")
        .args(["clone", "-q", "--no-hardlinks"])
        .arg(root())
        .arg(&c_src)
        .status()
        .unwrap();
    assert!(st.success());
    let st = Command::new("git")
        .arg("-C")
        .arg(&c_src)
        .args(["checkout", "-q", "--detach", &head])
        .status()
        .unwrap();
    assert!(st.success());

    let a = build(&a_src, &tmp.join("a/t"), Some((&head, &ct)));
    let b = build(&b_src, &tmp.join("b/t"), Some((&head, &ct)));
    let c = build(&c_src, &tmp.join("c/t"), None);
    let d = build(&d_src, &tmp.join("d/t"), None);
    let (sa, sb, sc, sd) = (sha(&a), sha(&b), sha(&c), sha(&d));
    for (n, p, s) in [
        ("A", &a, &sa),
        ("B", &b, &sb),
        ("C", &c, &sc),
        ("D", &d, &sd),
    ] {
        log(format!(
            "{n} {} B sha256 {s} --version {:?}",
            size(p),
            version(p)
        ));
    }
    assert_eq!(sa, sb, "archive builds in two dirs must be byte-identical");
    assert_eq!(sa, sc, "env-stamped archive build = git checkout build");
    assert_ne!(sa, sd, "unstamped build differs only by the stamp");
    assert_eq!(size(&a), size(&d), "the stamp should not change the size");

    // Size delta vs the unstamped base (F-376-1).
    let zbase = std::env::var("NERVE_STAMP_BASE").unwrap_or_else(|_| "9baa450c".into());
    if !zbase.is_empty() {
        let z_src = tmp.join("z/src");
        archive_into(&zbase, &z_src);
        let z = build(&z_src, &tmp.join("z/t"), None);
        let (za, zz) = (size(&a) as i64, size(&z) as i64);
        log(format!(
            "Z base {zbase} {zz} B sha256 {} --version {:?}; stamp delta A-Z = {:+} B",
            sha(&z),
            version(&z),
            za - zz
        ));
        assert_eq!(
            version(&z),
            format!("openmind-nerve {}", env!("CARGO_PKG_VERSION"))
        );
        // Budget guard: the stamp must cost at most one 4 KiB page.
        assert!(za - zz <= 4096, "stamp grew the nerve by {} B", za - zz);
    }

    let va = version(&a);
    assert!(
        va.contains(&format!("(commit {}, built {date},", &head[..8])),
        "{va}"
    );
    assert_eq!(va, version(&c));
    let vd = version(&d);
    assert!(vd.contains("(commit unknown, built unknown,"), "{vd}");

    let (_ka, la, ha) = serve(&a, base, &tmp.join("a/run"));
    let (_kc, lc, hc) = serve(&c, base + 1, &tmp.join("c/run"));
    let (_kd, ld, hd) = serve(&d, base + 2, &tmp.join("d/run"));
    for (n, l, h) in [("A", &la, &ha), ("C", &lc, &hc), ("D", &ld, &hd)] {
        log(format!("{n} startup {l:?}"));
        log(format!(
            "{n} /health commit {} build {}",
            h["commit"], h["build"]
        ));
    }
    assert!(la.ends_with(&format!(" commit={}", &head[..8])), "{la}");
    assert!(lc.ends_with(&format!(" commit={}", &head[..8])), "{lc}");
    assert!(ld.ends_with(" commit=unknown"), "{ld}");
    assert_eq!(ha["status"], "ok");
    assert_eq!(ha["commit"], head.as_str());
    assert_eq!(ha["build"]["date"], date.as_str());
    assert_eq!(ha["build"]["features"], serde_json::json!([]));
    assert_eq!(hc["commit"], head.as_str());
    assert_eq!(hc["build"]["date"], date.as_str());
    assert_eq!(hd["commit"], "unknown");
    assert_eq!(hd["build"]["date"], "unknown");
    drop((_ka, _kc, _kd));
    if std::env::var("NERVE_STAMP_REAL_KEEP").is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
