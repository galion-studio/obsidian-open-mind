// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio

//! Real-life test for swarm/sec-runbooks (wave 2, slot 26), in Rust (it
//! replaces the former `scripts/real/sec-runbooks.sh`). Runs ONLY on the box,
//! on scratch copies; never pushes, never contacts GitHub or the VPS.
//!
//! ```text
//! GALION_REAL=1 CARGO_TARGET_DIR=/workspace/swarm/target/sec-runbooks \
//!   nice -n 10 cargo test --release -j 3 -p history-purge --test real_sec_runbooks \
//!   -- --ignored --nocapture
//! ```
//! Env: GALION_REAL_SRC (source repo, default /workspace/swarm/base.git),
//! GITLEAKS (gitleaks binary; default: PATH, then the launch-gates tool dir).
//!
//! 1. `git clone --mirror` of the source into a scratch dir
//! 2. refusals, scan, canary (random fake key on a scratch-only branch),
//!    gitleaks before, rewrite, commit map, verify, plain-git cross-checks,
//!    fsck, gitleaks after, source mirror unchanged
//! 3. clone the rewritten mirror, check out main, `cargo check --workspace --locked`
//! 4. `galion-purge sec3-perms` on a fixture Hermes home: dry run changes
//!    nothing, `--apply` fixes modes, `--fix-world-writable`, `--rollback`
//!    restores, symlink refused (exit 3)

use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_galion-purge");

fn step(s: &str) {
    println!("\n== {} {s}", history_purge::zagreb::now_rfc3339());
}

fn run(c: &mut Command) -> Output {
    c.stdin(Stdio::null()).output().expect("spawn")
}

fn ok(c: &mut Command) -> String {
    let o = run(c);
    assert!(
        o.status.success(),
        "{c:?} failed: {}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn git(dir: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C")
        .arg(dir)
        .env("GIT_PAGER", "cat")
        .env("LC_ALL", "C");
    c
}

fn purge() -> Command {
    Command::new(BIN)
}

/// Independent matcher for the purge list (not the crate's PathSet).
fn listed(p: &str) -> bool {
    let p = p.trim_matches('"');
    ["contrib/", "stack/kms/", "dist/bin/", "edge/dist/"]
        .iter()
        .any(|d| p.starts_with(d))
        || p == "stack/INDEX.md"
        || p.ends_with(".redacted")
}

fn gitleaks() -> Option<PathBuf> {
    if let Some(g) = std::env::var_os("GITLEAKS") {
        return Some(PathBuf::from(g));
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|d| d.join("gitleaks"))
        .chain([PathBuf::from(
            "/workspace/swarm/target/launch-gates/tools/bin/gitleaks",
        )])
        .find(|p| p.is_file())
}

fn findings(report: &Path) -> (usize, Vec<String>) {
    let t = fs::read_to_string(report).unwrap_or_default();
    let n = t.matches("\"RuleID\"").count();
    let mut files: Vec<String> = t
        .lines()
        .filter_map(|l| l.trim().strip_prefix("\"File\": "))
        .map(|s| s.trim_end_matches(',').to_string())
        .collect();
    files.sort();
    files.dedup();
    (n, files)
}

/// FNV-1a 64, for a short display digest of the ref list.
fn fnv(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn canary_key() -> String {
    let mut b = [0u8; 16];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .expect("urandom");
    let abc = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let tail: String = b.iter().map(|x| abc[(*x % 32) as usize] as char).collect();
    format!("AKIA{tail}")
}

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "real-life test: GALION_REAL=1 ... -- --ignored"]
fn real_sec_runbooks() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        println!("SKIP: set GALION_REAL=1");
        return;
    }
    let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let here = fs::canonicalize(here).unwrap();
    let list = here.join("docs/launch/purge-paths.obsidian-open-mind.txt");
    let src = PathBuf::from(
        std::env::var_os("GALION_REAL_SRC").unwrap_or("/workspace/swarm/base.git".into()),
    );
    let tgt = PathBuf::from(
        std::env::var_os("CARGO_TARGET_DIR")
            .unwrap_or("/workspace/swarm/target/sec-runbooks".into()),
    );
    let work = tgt.join("real-scratch");
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).unwrap();
    let _clean = Cleanup(work.clone());
    let scratch = work.join("scratch.git");

    step("binary under test");
    let size = fs::metadata(BIN).unwrap().len();
    println!("{size} {BIN}");

    let refs = |d: &Path| ok(git(d).args(["for-each-ref", "--format=%(objectname) %(refname)"]));
    let src_main = ok(git(&src).args(["rev-parse", "refs/heads/main"]))
        .trim()
        .to_string();
    let src_refs = refs(&src);

    step(&format!(
        "scratch mirror clone of {} (main {src_main})",
        src.display()
    ));
    ok(Command::new("git")
        .args(["clone", "-q", "--mirror"])
        .arg(&src)
        .arg(&scratch));
    let count = |d: &Path| {
        ok(git(d).args(["rev-list", "--all", "--count"]))
            .trim()
            .to_string()
    };
    println!(
        "refs: {}, commits: {}, remotes: {}",
        refs(&scratch).lines().count(),
        count(&scratch),
        ok(git(&scratch).arg("remote"))
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    );

    step("refusals");
    let o = run(purge()
        .arg("rewrite")
        .arg("--repo")
        .arg(&scratch)
        .arg("--paths")
        .arg(&list));
    assert!(!o.status.success(), "ran without --confirm-scratch-copy");
    let o = run(purge()
        .arg("rewrite")
        .arg("--repo")
        .arg(&scratch)
        .arg("--paths")
        .arg(&list)
        .arg("--confirm-scratch-copy"));
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("--strip-remotes"));
    println!(
        "without --confirm-scratch-copy: refused; with a remote and no --strip-remotes: refused"
    );

    step("scan before (paths only)");
    print!(
        "{}",
        ok(purge()
            .arg("scan")
            .arg("--repo")
            .arg(&scratch)
            .arg("--paths")
            .arg(&list)
            .arg("--quiet"))
    );

    step("plant a canary secret in listed paths on a scratch-only branch (generated now, never printed)");
    let cw = work.join("canary-wt");
    ok(Command::new("git")
        .args(["clone", "-q"])
        .arg(&scratch)
        .arg(&cw));
    fs::create_dir_all(cw.join("contrib")).unwrap();
    fs::create_dir_all(cw.join("stack/kms")).unwrap();
    {
        let key = canary_key();
        fs::write(
            cw.join("contrib/canary.env"),
            format!("aws_access_key_id = {key}\n"),
        )
        .unwrap();
        fs::write(
            cw.join("stack/kms/canary.env.redacted"),
            format!("AWS_ACCESS_KEY_ID={key}\n"),
        )
        .unwrap();
    }
    ok(git(&cw).args(["checkout", "-q", "-b", "canary"]));
    ok(git(&cw).args(["add", "-A"]));
    ok(git(&cw).args([
        "-c",
        "user.name=canary",
        "-c",
        "user.email=canary@example.invalid",
        "commit",
        "-q",
        "-m",
        "canary: fake key in listed paths",
    ]));
    ok(git(&scratch)
        .arg("fetch")
        .arg("-q")
        .arg(&cw)
        .arg("canary:refs/heads/canary"));
    fs::remove_dir_all(&cw).unwrap();
    println!("canary branch added to the scratch mirror only");

    let gl = gitleaks();
    let leaks = |report: &Path| -> Option<bool> {
        let g = gl.as_ref()?;
        let o = run(Command::new(g)
            .arg("git")
            .arg(&scratch)
            .args([
                "--redact",
                "--no-banner",
                "--log-level",
                "warn",
                "--report-path",
            ])
            .arg(report));
        Some(!o.status.success())
    };
    step("gitleaks before (all history, redacted)");
    match leaks(&work.join("gl-before.json")) {
        Some(found) => {
            let (n, files) = findings(&work.join("gl-before.json"));
            println!("findings before: {n} (files: {})", files.join(" "));
            assert!(found && n >= 1, "gitleaks missed the canary");
        }
        None => println!("SKIP: gitleaks not found (set GITLEAKS)"),
    }

    step("rewrite");
    print!(
        "{}",
        ok(purge()
            .arg("rewrite")
            .arg("--repo")
            .arg(&scratch)
            .arg("--paths")
            .arg(&list)
            .args(["--confirm-scratch-copy", "--strip-remotes", "--quiet"]))
    );
    let map = fs::read_to_string(scratch.join("galion-purge-commit-map.tsv")).unwrap();
    let new_main = map
        .lines()
        .filter(|l| !l.starts_with('#'))
        .find_map(|l| {
            l.split_once('\t')
                .filter(|(o, _)| *o == src_main)
                .map(|(_, n)| n.to_string())
        })
        .expect("main in commit map");
    println!(
        "commit map: {} lines; main {} -> {}",
        map.lines().filter(|l| !l.starts_with('#')).count(),
        &src_main[..7],
        &new_main[..7]
    );
    assert_eq!(
        new_main,
        ok(git(&scratch).args(["rev-parse", "refs/heads/main"])).trim()
    );

    step("verify (galion-purge)");
    print!(
        "{}",
        ok(purge()
            .arg("verify")
            .arg("--repo")
            .arg(&scratch)
            .arg("--paths")
            .arg(&list)
            .arg("--quiet"))
    );

    step("cross-check with plain git (every commit diff vs every parent, every reachable object path)");
    let n1 = ok(git(&scratch).args([
        "log",
        "--all",
        "-m",
        "--no-renames",
        "--format=",
        "--name-only",
    ]))
    .lines()
    .filter(|l| listed(l))
    .count();
    let n2 = ok(git(&scratch).args(["rev-list", "--all", "--objects"]))
        .lines()
        .filter_map(|l| l.split_once(' ').map(|(_, p)| p.to_string()))
        .filter(|p| listed(p))
        .count();
    println!("log -m --name-only hits: {n1}; rev-list --objects path hits: {n2}");
    assert_eq!((n1, n2), (0, 0), "plain git still sees listed paths");
    assert!(
        ok(git(&scratch).arg("remote")).trim().is_empty(),
        "remote still configured"
    );
    ok(git(&scratch).args(["fsck", "--full", "--no-dangling"]));
    println!("fsck: clean");
    println!(
        "refs after: {}, commits after: {}",
        refs(&scratch).lines().count(),
        count(&scratch)
    );

    step("gitleaks after");
    match leaks(&work.join("gl-after.json")) {
        Some(found) => {
            let (n, _) = findings(&work.join("gl-after.json"));
            println!("findings after: {n}");
            assert!(!found && n == 0, "gitleaks found something after the purge");
        }
        None => println!("SKIP: gitleaks not found"),
    }

    step("source mirror unchanged");
    assert_eq!(
        ok(git(&src).args(["rev-parse", "refs/heads/main"])).trim(),
        src_main
    );
    assert_eq!(refs(&src), src_refs, "source refs changed");
    println!("source refs digest {} unchanged", fnv(&src_refs));

    step("rewritten tree still builds (main, cargo check --workspace --locked)");
    let tree = work.join("tree");
    ok(Command::new("git")
        .args(["clone", "-q"])
        .arg(&scratch)
        .arg(&tree));
    print!("{}", ok(git(&tree).args(["log", "--oneline", "-1"])));
    for p in [
        "contrib",
        "stack/kms",
        "stack/INDEX.md",
        "dist/bin",
        "edge/dist",
    ] {
        assert!(!tree.join(p).exists(), "{p} present in checkout");
    }
    let tree_tgt = tgt.join("real-tree");
    ok(Command::new("nice")
        .args([
            "-n",
            "10",
            "cargo",
            "check",
            "-q",
            "--workspace",
            "--locked",
            "-j",
            "3",
        ])
        .current_dir(&tree)
        .env("CARGO_TARGET_DIR", &tree_tgt)
        .env("CARGO_INCREMENTAL", "0")
        .env_remove("RUSTFLAGS"));
    let _ = fs::remove_dir_all(&tree_tgt);
    println!("cargo check: ok");

    step("SEC-3 (galion-purge sec3-perms) on a fixture HERMES_HOME");
    let fx = work.join("hermes-fixture");
    fs::create_dir_all(fx.join("vault")).unwrap();
    fs::create_dir_all(fx.join("sessions")).unwrap();
    let names = [
        "config.yaml",
        "SOUL.md",
        "vault",
        "vault/.vault.lock",
        "USER.md",
        "notes.md",
    ];
    for f in [
        "config.yaml",
        "SOUL.md",
        "vault/.vault.lock",
        "USER.md",
        "notes.md",
    ] {
        fs::write(fx.join(f), b"").unwrap();
    }
    let set =
        |p: &str, m: u32| fs::set_permissions(fx.join(p), fs::Permissions::from_mode(m)).unwrap();
    set("", 0o700);
    for (n, m) in names.iter().zip([0o666, 0o666, 0o777, 0o666, 0o664, 0o666]) {
        set(n, m);
    }
    let modes = || {
        names
            .iter()
            .map(|n| {
                format!(
                    "{:o} {n}",
                    fs::symlink_metadata(fx.join(n))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o7777
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    let state = work.join("sec3-state");
    let s3 = |args: &[&str]| -> Output {
        run(purge()
            .arg("sec3-perms")
            .arg("--hermes-home")
            .arg(&fx)
            .arg("--state-dir")
            .arg(&state)
            .arg("--no-chown")
            .args(args))
    };
    let tail = |o: &Output, n: usize| {
        let t = String::from_utf8_lossy(&o.stdout).into_owned();
        let v: Vec<&str> = t.lines().collect();
        v[v.len().saturating_sub(n)..].join("\n")
    };
    let orig =
        "666 config.yaml 666 SOUL.md 777 vault 666 vault/.vault.lock 664 USER.md 666 notes.md";
    println!("before:   {}", modes());
    let o = s3(&[]);
    assert!(o.status.success());
    print!("{}", String::from_utf8_lossy(&o.stdout));
    println!("dry-run:  {}", modes());
    assert_eq!(modes(), orig, "dry-run changed modes");
    assert!(!state.exists(), "dry run wrote a state dir");
    let o = s3(&["--apply"]);
    assert!(o.status.success());
    println!("{}", tail(&o, 3));
    println!("applied:  {}", modes());
    assert_eq!(
        modes(),
        "600 config.yaml 644 SOUL.md 700 vault 600 vault/.vault.lock 600 USER.md 666 notes.md"
    );
    let o = s3(&["--apply", "--fix-world-writable"]);
    assert!(o.status.success());
    println!("{}", tail(&o, 1));
    println!("o-w:      {}", modes());
    assert_eq!(
        modes(),
        "600 config.yaml 644 SOUL.md 700 vault 600 vault/.vault.lock 600 USER.md 664 notes.md"
    );
    let mut rbs: Vec<PathBuf> = fs::read_dir(&state)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    rbs.sort();
    assert_eq!(rbs.len(), 2);
    let o = s3(&["--rollback", rbs[0].to_str().unwrap()]);
    assert!(o.status.success());
    println!("{}", tail(&o, 1));
    println!("rollback: {}", modes());
    assert_eq!(modes(), orig, "rollback result");
    fs::remove_file(fx.join("SOUL.md")).unwrap();
    std::os::unix::fs::symlink("/etc/hostname", fx.join("SOUL.md")).unwrap();
    let o = s3(&["--apply"]);
    assert_eq!(o.status.code(), Some(3), "symlinked target not refused");
    println!(
        "symlinked SOUL.md: refused (exit 3): {}",
        String::from_utf8_lossy(&o.stderr).trim()
    );

    step("PASS");
}
