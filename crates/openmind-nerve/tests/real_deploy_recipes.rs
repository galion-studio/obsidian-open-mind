// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 430 (nerve-merge-9, D-424-1 / D-404-2): build the three candidate
//! nerve deploy recipes from the committed rust-lld tree (`.cargo/config.toml`
//! pins `linker = "rust-lld"` for x86_64 musl, D-409-2), each TWICE from two
//! independent `git archive HEAD` trees with `GALION_BUILD_COMMIT` +
//! `SOURCE_DATE_EPOCH`, and prove:
//!
//!   * both builds of a recipe are byte-identical (reproducible from git);
//!   * every linker / rustflags env var is stripped, so the committed config
//!     alone picks the linker, and the ELF `.comment` names LLD;
//!   * each binary is under the 2 MiB ceiling (F-376-1) and prints a size
//!     table row (bytes, headroom);
//!   * each binary serves `/health` with the stamped commit on
//!     127.0.0.1:23000-23002 (`NERVE_RECIPE_PORT_BASE` moves the block).
//!
//! Recipes (owner still picks, D-424-1):
//!   default      `--release --locked -p openmind-nerve`
//!   lean-edge    `... --no-default-features --features sensor-edgerate`
//!   d-lean       `... --no-default-features` (404 option (d): lean plus
//!                only the features whose env the unit sets; the live unit
//!                sets no S2/memfresh/rustsec/drift/ingress env, so (d) = lean)
//!
//! Gated: `GALION_REAL=1`. `NERVE_RECIPE_ONLY=default,lean-edge` limits the
//! set; `NERVE_RECIPE_TMP` / `NERVE_RECIPE_KEEP` as in real_build_stamp.

use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const TARGET: &str = "x86_64-unknown-linux-musl";
const BUDGET: u64 = 2 * 1024 * 1024;

/// (name, extra cargo args)
const RECIPES: [(&str, &[&str]); 3] = [
    ("default", &[]),
    (
        "lean-edge",
        &["--no-default-features", "--features", "sensor-edgerate"],
    ),
    ("d-lean", &["--no-default-features"]),
];

fn log(m: impl AsRef<str>) {
    eprintln!(
        "[real_deploy_recipes {}] {}",
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

/// Env vars that could pick a linker or change codegen behind the committed
/// config's back.
fn strip_env(c: &mut Command) {
    for (k, _) in std::env::vars() {
        let linker = k.starts_with("CARGO_TARGET_") && k.ends_with("_LINKER");
        let flags = k.starts_with("CARGO_TARGET_") && k.ends_with("_RUSTFLAGS");
        if linker
            || flags
            || matches!(
                k.as_str(),
                "RUSTFLAGS"
                    | "CARGO_ENCODED_RUSTFLAGS"
                    | "CARGO_BUILD_RUSTFLAGS"
                    | "RUSTC_LINKER"
                    | "GALION_BUILD_COMMIT"
                    | "SOURCE_DATE_EPOCH"
            )
        {
            c.env_remove(&k);
        }
    }
}

fn build(src: &Path, target_dir: &Path, extra: &[&str], commit: &str, epoch: &str) -> PathBuf {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or("cargo".into()));
    c.current_dir(src);
    strip_env(&mut c);
    c.args([
        "build",
        "-j",
        "2",
        "--release",
        "--locked",
        "--offline",
        "-p",
        "openmind-nerve",
        "--target",
        TARGET,
    ])
    .args(extra)
    .env("CARGO_TARGET_DIR", target_dir)
    .env("CARGO_INCREMENTAL", "0")
    .env("GALION_BUILD_COMMIT", commit)
    .env("SOURCE_DATE_EPOCH", epoch);
    let t = Instant::now();
    assert!(
        c.status().unwrap().success(),
        "cargo build in {}",
        src.display()
    );
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

/// rust-lld writes `Linker: LLD <ver>` into `.comment`; GNU ld writes nothing.
fn linked_by_lld(p: &Path) -> bool {
    let b = std::fs::read(p).unwrap();
    b.windows(11).any(|w| w == b"Linker: LLD")
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

/// Serve dry-run with the live unit's env shape (no S2 env, NERVE_EDGE_HOSTS
/// unset); returns guard, startup lines up to `listening on`, `/health`.
fn serve(bin: &Path, port: u16, dir: &Path) -> (Kill, Vec<String>, Value) {
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::write(
        dir.join("components.json"),
        r#"[{"name":"recipe-real-dead","port":1,"path":"/health","gaia":"DEMETER","system":"tuner"}]"#,
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
    let mut lines = Vec::new();
    loop {
        let l = rx
            .recv_timeout(Duration::from_secs(30))
            .unwrap_or_else(|e| panic!("no startup line on {port}: {e}"));
        let done = l.contains(" listening on ");
        lines.push(l);
        if done {
            break;
        }
    }
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(15) {
        if let Some(h) = health(port) {
            return (k, lines, h);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("no /health on {port}");
}

#[test]
fn real_deploy_recipes_reproducible_under_budget() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let base: u16 = galion_realtest::ports::base_from_env("NERVE_RECIPE_PORT_BASE", 23000, 3);
    let only = std::env::var("NERVE_RECIPE_ONLY").ok();
    let head = git(&["rev-parse", "HEAD"]);
    let ct = git(&["show", "-s", "--format=%ct", "HEAD"]);
    let tmp = std::env::var("NERVE_RECIPE_TMP")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::temp_dir().join(format!("nerve-recipes-{}", std::process::id()))
        });
    let _ = std::fs::remove_dir_all(&tmp);
    log(format!(
        "HEAD {head} SOURCE_DATE_EPOCH {ct}; tmp {}",
        tmp.display()
    ));
    let mut rows = Vec::new();
    for (i, (name, extra)) in RECIPES.iter().enumerate() {
        if only
            .as_deref()
            .is_some_and(|o| !o.split(',').any(|x| x == *name))
        {
            continue;
        }
        log(format!(
            "recipe {name}: GALION_BUILD_COMMIT={head} SOURCE_DATE_EPOCH={ct} cargo build -j 2 --release --locked --offline -p openmind-nerve --target {TARGET} {}",
            extra.join(" ")
        ));
        let s1 = tmp.join(format!("{name}/one/src"));
        let s2 = tmp.join(format!("{name}/two/other/deeper/src"));
        archive_into(&head, &s1);
        archive_into(&head, &s2);
        let b1 = build(&s1, &tmp.join(format!("{name}/one/t")), extra, &head, &ct);
        let b2 = build(&s2, &tmp.join(format!("{name}/two/t")), extra, &head, &ct);
        let (h1, h2) = (sha(&b1), sha(&b2));
        let sz = size(&b1);
        log(format!(
            "recipe {name}: run1 {sz} B sha256 {h1}; run2 {} B sha256 {h2}",
            size(&b2)
        ));
        assert_eq!(h1, h2, "recipe {name}: two builds must be byte-identical");
        assert!(linked_by_lld(&b1), "recipe {name}: .comment has no LLD");
        assert!(sz <= BUDGET, "recipe {name}: {sz} B over {BUDGET} B");
        let port = base + i as u16;
        let (k, lines, h) = serve(&b1, port, &tmp.join(format!("{name}/run")));
        let warns = lines
            .iter()
            .filter(|l| l.starts_with("<4>") || l.to_ascii_lowercase().contains("warn"))
            .count();
        for l in &lines {
            log(format!("{name} :{port} startup {l}"));
        }
        log(format!(
            "{name} :{port} /health status {} commit {} build {}",
            h["status"], h["commit"], h["build"]
        ));
        assert_eq!(h["status"], "ok");
        assert_eq!(h["commit"], head.as_str());
        // The live unit sets no NERVE_EDGE_HOSTS: only a build without
        // `sensor-edgerate` warns (one line, slot 404 D-404-3).
        let want = usize::from(*name == "d-lean");
        assert_eq!(warns, want, "recipe {name}: startup warning lines");
        drop(k);
        rows.push(format!(
            "| {name} | {sz} | {} | {:.1} % | {} | {warns} |",
            BUDGET - sz,
            (BUDGET - sz) as f64 * 100.0 / BUDGET as f64,
            &h1[..16]
        ));
    }
    log("| recipe | bytes | headroom to 2 MiB | headroom % | sha256 (16) | startup warn lines |");
    log("|---|---:|---:|---:|---|---:|");
    for r in &rows {
        log(r);
    }
    if std::env::var("NERVE_RECIPE_KEEP").is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
