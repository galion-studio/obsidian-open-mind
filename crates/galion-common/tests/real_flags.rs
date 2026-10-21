// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life check for ops S2.2 (GALION_REAL=1): every deployable binary
//! answers `--version`, `--help` and `--check` without binding.
//!
//! The test builds every deployable binary in release with a stamped
//! commit/time (or takes them from `GALION_REAL_BIN_DIR`), then for each
//! binary and flag:
//!
//! * runs it 3 times with a cleared environment and requires exit 0 (exit 0
//!   or 1 for `--check` with an empty environment) and a wall time under
//!   100 ms for every run;
//! * runs it once under `strace -f` and requires: exactly one `execve`, no
//!   network syscall at all (`socket`, `bind`, `listen`, `connect`, ...), no
//!   `clone`/`fork`, no file opened for writing or created, no mkdir, unlink
//!   or rename;
//! * compares `ss -Hltnup` before and after: no new listener owned by the
//!   binary and nothing on this slot's ports 20640-20649.
//!
//! hermes-gateway also gets a broken config: `--version`, `--help` still
//! exit 0 without opening config.yaml, while a plain start dies on the
//! config error (proof that the startup checks do not run on `--version`).
//!
//! Test helper mocks (gaia-dummy, galion-bench-mock, hermes-mock-model) are
//! not deployable and are not covered.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const STAMP_COMMIT: &str = "realflags0";
const STAMP_TIME: &str = "2026-10-21T15:30:00+02:00";
/// Fake key for hermes-gateway's `--check` (a test value, not a secret).
const FAKE_KEY: &str = "realtest-not-a-key-0000";

/// (cargo package, binary name).
const BINS: &[(&str, &str)] = &[
    ("bench-banking", "bench-banking"),
    ("bench-scoreboard", "bench-scoreboard"),
    ("desk-hermes", "desk-hermes"),
    ("gaia", "gaia"),
    ("galion-backup", "galion-backup"),
    ("galion-boot", "galion-boot"),
    ("galion-deploy", "galion-deploy"),
    ("galion-desk", "galion-desk"),
    ("galion-guard", "galion-guard"),
    ("galion-health", "galion-health"),
    ("galion-one", "galion-one"),
    ("galion-origin", "galion-origin"),
    ("galion-realtest", "galion-realtest"),
    ("galion-vending", "galion-vending"),
    ("galion-voice-cpu", "galion-voice-serve"),
    ("galion-wallstreet", "galion-wallstreet"),
    ("galleon-arena", "galleon-arena"),
    ("guardian-tick", "guardian-tick"),
    ("hades-gate", "hades-gate"),
    ("hermes-bench", "hermes-bench"),
    ("hermes-gateway", "hermes-gateway"),
    ("link-watch", "link-watch"),
    ("one-defence", "one-defence"),
    ("one-set-model", "one-set-model"),
    ("one-talk", "one-talk"),
    ("openmind-discovery", "openmind-discovery"),
    ("openmind-mcp-stack", "openmind-mcp-stack"),
    ("openmind-memory", "openmind-memory"),
    ("openmind-nerve", "openmind-nerve"),
    ("openmind-queue", "openmind-queue"),
    ("persona-plex-relay", "persona-plex-relay"),
    ("repo-check", "repo-check"),
    ("room", "room"),
    ("tuner", "tuner"),
];

/// (flag, env label, env, allowed exit codes).
type Case<'a> = (&'a str, &'a str, &'a [(String, String)], &'a [i32]);

const LIMIT: Duration = Duration::from_millis(100);

struct Run {
    code: Option<i32>,
    took: Duration,
    stdout: String,
}

fn run(bin: &Path, args: &[&str], env: &[(String, String)], cwd: &Path) -> Run {
    let start = Instant::now();
    let mut child = Command::new(bin)
        .args(args)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    // A binary that ignored the flag and started serving would never exit:
    // give it 5 s, then kill it and fail.
    loop {
        if child.try_wait().expect("wait").is_some() {
            break;
        }
        if start.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "{} {args:?} still running after 5 s (served instead of exiting?)",
                bin.display()
            );
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let out = child.wait_with_output().expect("output");
    Run {
        code: out.status.code(),
        took: start.elapsed(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
    }
}

/// Every TCP/UDP listener as "proto local users".
fn listeners() -> BTreeSet<String> {
    let out = Command::new("ss").args(["-Hltnup"]).output().expect("ss");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let users = f
                .iter()
                .find(|x| x.starts_with("users:"))
                .copied()
                .unwrap_or("");
            format!(
                "{} {} {}",
                f.first().copied().unwrap_or(""),
                f.get(4).copied().unwrap_or(""),
                users
            )
        })
        .collect()
}

fn slot_port(l: &str) -> bool {
    l.split_whitespace()
        .nth(1)
        .and_then(|a| a.rsplit_once(':'))
        .and_then(|(_, p)| p.parse::<u16>().ok())
        .is_some_and(|p| (20640..=20649).contains(&p))
}

/// Syscalls that would mean the flag did real work. Returns the offending lines.
fn strace_violations(trace: &str) -> Vec<String> {
    let mut bad = Vec::new();
    let mut execs = 0;
    for l in trace.lines() {
        // "PID name(args) = ret" (with -f); resumed/unfinished lines too.
        let body = l.split_once(' ').map_or(l, |(_, r)| r).trim_start();
        let name = body.split('(').next().unwrap_or("");
        let name = name
            .trim_start_matches("<... ")
            .split_whitespace()
            .next()
            .unwrap_or("");
        match name {
            "execve" => {
                execs += 1;
                if execs > 1 {
                    bad.push(l.to_string());
                }
            }
            "exit" | "exit_group" | "+++" | "---" => {}
            "openat" | "open" => {
                if ["O_WRONLY", "O_RDWR", "O_CREAT", "O_TRUNC"]
                    .iter()
                    .any(|f| body.contains(f))
                {
                    bad.push(l.to_string());
                }
            }
            "" => {}
            _ => bad.push(l.to_string()),
        }
    }
    bad
}

fn strace(bin: &Path, args: &[&str], env: &[(String, String)], cwd: &Path, out: &Path) -> String {
    let status = Command::new("strace")
        .args(["-f", "-qq", "-o"])
        .arg(out)
        .args([
            "-e",
            "trace=%network,%process,open,openat,creat,mkdir,mkdirat,unlink,unlinkat,rename,renameat,renameat2,truncate,ftruncate,link,linkat,symlink,symlinkat,chmod,fchmodat",
        ])
        .arg(bin)
        .args(args)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("strace");
    assert!(
        status.code().is_some(),
        "strace {}: {status:?}",
        bin.display()
    );
    std::fs::read_to_string(out).unwrap_or_default()
}

fn kv(pairs: &[(&str, String)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

/// Environment under which `--check` must pass (temp dirs, slot ports that
/// are never bound).
fn pass_env(bin: &str, t: &Path, root: &Path) -> Vec<(String, String)> {
    let p = |s: &str| t.join(s).display().to_string();
    match bin {
        "hermes-gateway" => kv(&[
            ("HERMES_HOME", p("hermes")),
            ("HERMES_RS_PORT", "20640".into()),
            ("API_SERVER_KEY", FAKE_KEY.into()),
        ]),
        "desk-hermes" => kv(&[
            ("HERMES_HOME", p("hermes")),
            ("HERMES_STATE_DB", p("hermes/state.db")),
        ]),
        "galion-deploy" => kv(&[
            ("GALION_DEPLOY_BACKUP_DIR", p("deploy-backups")),
            ("SKIP_DATA_BACKUP", "1".into()),
        ]),
        "galion-backup" => kv(&[("GALION_BACKUP_DEST", p("backup-dest"))]),
        "galion-health" => kv(&[("GALION_HEALTH_ADDR", "127.0.0.1:20641".into())]),
        "guardian-tick" => kv(&[("GUARDIAN_PROBES", p("probes.toml"))]),
        "one-defence" => kv(&[("ONE_DEFENCE_DB", p("defence.db"))]),
        "galion-desk" => kv(&[("DESK_BIND", "127.0.0.1:20642".into())]),
        "galion-origin" => kv(&[
            ("ORIGIN_ADDR", "127.0.0.1:20643".into()),
            ("ORIGIN_ROOT", p("")),
        ]),
        "room" => kv(&[
            ("ROOM_BIND", "127.0.0.1:20644".into()),
            ("ROOM_ROOT", p("")),
        ]),
        "openmind-memory" => kv(&[
            ("OPENMIND_MEMORY_HOST", "127.0.0.1".into()),
            ("OPENMIND_MEMORY_PORT", "20645".into()),
            ("OPENMIND_MEMORY_DATA_DIR", p("memory")),
        ]),
        "openmind-nerve" => kv(&[
            ("NERVE_ADDR", "127.0.0.1:20646".into()),
            ("NERVE_DATA_DIR", p("nerve")),
        ]),
        "galion-voice-serve" => kv(&[
            ("GALION_VOICE_BIND", "127.0.0.1:20647".into()),
            ("GALION_VOICE_MODELS", p("models")),
        ]),
        "galion-one" => kv(&[
            ("ONE_BIND", "127.0.0.1:20648".into()),
            ("ONE_MOUTH_BIND", "127.0.0.1:20649".into()),
        ]),
        "galion-wallstreet" => kv(&[("WALLSTREET_BIND", "127.0.0.1:20649".into())]),
        "tuner" => kv(&[
            ("TUNER_ADDR", "127.0.0.1:20640".into()),
            (
                "TUNER_ROOT",
                root.join("crates/tuner/www").display().to_string(),
            ),
        ]),
        _ => Vec::new(),
    }
}

fn fixture(t: &Path) {
    for d in [
        "hermes",
        "deploy-backups",
        "backup-dest",
        "memory",
        "nerve",
        "models",
        "badhermes",
    ] {
        std::fs::create_dir_all(t.join(d)).expect("mkdir");
    }
    std::fs::write(t.join("hermes/state.db"), b"").expect("state.db");
    std::fs::write(t.join("defence.db"), b"").expect("defence.db");
    std::fs::write(t.join("probes.toml"), "[[probe]]\nkind = \"mem\"\n").expect("probes");
    // Unreadable as YAML for hermes-rs, plus a non-numeric port.
    std::fs::write(
        t.join("badhermes/config.yaml"),
        "model: [unclosed\n  - : :\n",
    )
    .expect("bad yaml");
}

#[test]
fn every_binary_flags_are_instant_and_never_bind() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("root");
    let bin_dir = match std::env::var("GALION_REAL_BIN_DIR") {
        Ok(d) => PathBuf::from(d),
        Err(_) => {
            let target = std::env::var("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| root.join("target"));
            let mut build = Command::new(env!("CARGO"));
            build
                .current_dir(&root)
                .args(["build", "--release", "-j", "2"])
                .env("GALION_BUILD_COMMIT", STAMP_COMMIT)
                .env("GALION_BUILD_TIME", STAMP_TIME);
            for (pkg, bin) in BINS {
                build.args(["-p", pkg, "--bin", bin]);
            }
            assert!(build.status().expect("cargo build").success());
            target.join("release")
        }
    };
    let t = std::env::temp_dir().join(format!("galion-real-flags-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&t);
    fixture(&t);
    let traces = t.join("traces");
    std::fs::create_dir_all(&traces).expect("traces");
    let cwd = t.join("cwd");
    std::fs::create_dir_all(&cwd).expect("cwd");

    let before = listeners();
    let mut failures: Vec<String> = Vec::new();
    let mut rows = 0;
    let mut worst = Duration::ZERO;
    eprintln!(
        "binary | flag | env | exit | runs (ms) | strace syscalls flagged | first stdout line"
    );
    for (_, name) in BINS {
        let bin = bin_dir.join(name);
        assert!(bin.is_file(), "missing {}", bin.display());
        let pe = pass_env(name, &t, &root);
        let cases: [Case<'_>; 4] = [
            ("--version", "empty", &[], &[0]),
            ("--help", "empty", &[], &[0]),
            ("--check", "pass", &pe, &[0]),
            ("--check", "empty", &[], &[0, 1]),
        ];
        for (flag, env_name, env, allowed) in cases {
            let mut times = Vec::new();
            let mut first = String::new();
            let mut code = None;
            for _ in 0..3 {
                let r = run(&bin, &[flag], env, &cwd);
                times.push(r.took);
                worst = worst.max(r.took);
                code = r.code;
                if !r.code.is_some_and(|c| allowed.contains(&c)) {
                    failures.push(format!(
                        "{name} {flag} ({env_name} env): exit {:?}, stdout {:?}",
                        r.code, r.stdout
                    ));
                }
                if r.took >= LIMIT {
                    failures.push(format!(
                        "{name} {flag} ({env_name} env): {:?} >= 100 ms",
                        r.took
                    ));
                }
                if flag == "--version" {
                    let want = format!(
                        "{name} {} (commit {STAMP_COMMIT}, built {STAMP_TIME})",
                        env!("CARGO_PKG_VERSION")
                    );
                    if std::env::var("GALION_REAL_BIN_DIR").is_err() && r.stdout.trim_end() != want
                    {
                        failures.push(format!(
                            "{name} --version: {:?} != {want:?}",
                            r.stdout.trim_end()
                        ));
                    }
                }
                first = r.stdout.lines().next().unwrap_or("").to_string();
            }
            let tf = traces.join(format!("{name}{flag}-{env_name}.trace"));
            let bad = strace_violations(&strace(&bin, &[flag], env, &cwd, &tf));
            if !bad.is_empty() {
                failures.push(format!("{name} {flag} ({env_name} env) syscalls: {bad:?}"));
            }
            let ms: Vec<String> = times
                .iter()
                .map(|d| format!("{:.1}", d.as_secs_f64() * 1000.0))
                .collect();
            eprintln!(
                "{name} | {flag} | {env_name} | {code:?} | {} | {} | {first}",
                ms.join("/"),
                bad.len()
            );
            rows += 1;
        }
        // No listener owned by this binary appeared, nothing on the slot ports.
        let comm: String = name.chars().take(15).collect();
        for l in listeners().difference(&before) {
            if l.contains(&format!("((\"{comm}\"")) || slot_port(l) {
                failures.push(format!("{name}: new listener {l}"));
            }
        }
    }

    // hermes-gateway with a broken config: flags still answer, start refuses.
    let bad_env = kv(&[
        ("HERMES_HOME", t.join("badhermes").display().to_string()),
        ("HERMES_RS_PORT", "notaport".into()),
    ]);
    let hg = bin_dir.join("hermes-gateway");
    for flag in ["--version", "--help"] {
        let r = run(&hg, &[flag], &bad_env, &cwd);
        let trace = strace(
            &hg,
            &[flag],
            &bad_env,
            &cwd,
            &traces.join(format!("hermes-gateway{flag}-broken.trace")),
        );
        let opened = trace.lines().any(|l| l.contains("config.yaml"));
        eprintln!(
            "hermes-gateway | {flag} | broken config | {:?} | {:.1} | config.yaml opened={opened}",
            r.code,
            r.took.as_secs_f64() * 1000.0
        );
        if r.code != Some(0) || opened || r.took >= LIMIT {
            failures.push(format!(
                "hermes-gateway {flag} with broken config: exit {:?} opened={opened} {:?}",
                r.code, r.took
            ));
        }
    }
    let r = run(&hg, &["--check"], &bad_env, &cwd);
    eprintln!(
        "hermes-gateway | --check | broken config | {:?} | {}",
        r.code,
        r.stdout.trim_end()
    );
    if r.code != Some(1) {
        failures.push(format!(
            "hermes-gateway --check with broken config: exit {:?}",
            r.code
        ));
    }
    let r = run(&hg, &[], &bad_env, &cwd);
    eprintln!(
        "hermes-gateway | (start) | broken config | {:?} | {:.1} ms",
        r.code,
        r.took.as_secs_f64() * 1000.0
    );
    if r.code != Some(2) {
        failures.push(format!(
            "hermes-gateway start with broken config: exit {:?} (want 2)",
            r.code
        ));
    }

    // Positive controls: the harness really sees binds. (1) A real tuner start
    // on a non-local address fails in bind(), which strace must show. (2) A
    // real tuner start on 20640 must show up in ss as owned by "tuner".
    let tuner = bin_dir.join("tuner");
    let www = root.join("crates/tuner/www").display().to_string();
    let ctl_env = kv(&[
        ("TUNER_ADDR", "192.0.2.1:20640".into()),
        ("TUNER_ROOT", www.clone()),
    ]);
    let trace = strace(
        &tuner,
        &[],
        &ctl_env,
        &cwd,
        &traces.join("control-tuner-start.trace"),
    );
    let saw_bind = trace.lines().any(|l| l.contains("bind("));
    eprintln!("control | tuner start on 192.0.2.1:20640 under strace | bind( seen={saw_bind}");
    if !saw_bind || strace_violations(&trace).is_empty() {
        failures.push("control: strace did not see the bind of a real start".into());
    }
    let mut child = Command::new(&tuner)
        .env_clear()
        .env("TUNER_ADDR", "127.0.0.1:20640")
        .env("TUNER_ROOT", &www)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("tuner");
    let start = Instant::now();
    let mut seen = None;
    while start.elapsed() < Duration::from_secs(5) {
        if let Some(l) = listeners()
            .into_iter()
            .find(|l| slot_port(l) && l.contains("((\"tuner\""))
        {
            seen = Some(l);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
    eprintln!("control | tuner start on 127.0.0.1:20640 | ss saw {seen:?}");
    if seen.is_none() {
        failures.push("control: ss did not see a real tuner listener on 20640".into());
    }
    std::thread::sleep(Duration::from_millis(50));

    for l in listeners().difference(&before) {
        if slot_port(l) {
            failures.push(format!("slot port still bound at the end: {l}"));
        }
    }
    eprintln!(
        "{} binaries, {rows} flag runs x3 + strace, worst {:.1} ms, failures {}",
        BINS.len(),
        worst.as_secs_f64() * 1000.0,
        failures.len()
    );
    let _ = std::fs::remove_dir_all(&t);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
