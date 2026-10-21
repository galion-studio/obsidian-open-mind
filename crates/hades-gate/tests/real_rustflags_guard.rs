// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life proof of the Gaia S5.4 SIMD guard: runs the real `cargo` on this
//! workspace with the rustflags overrides that used to drop
//! `--cfg curve25519_dalek_backend="serial"` silently, and checks that the
//! hades-gate build now FAILS with the guard message, while normal builds (and
//! overrides that keep the cfg) still PASS. Also reads curve25519-dalek's own
//! build-script decision (`-vv`) to show which backend it would have used.
//!
//! Runs only with `GALION_REAL=1` (slow: a fresh target dir per case).
//! `HADES_GUARD_TARGETS` (comma list) picks targets; default x86_64 + aarch64
//! musl. No network (`--offline --locked`); flag values are never printed.

use std::path::{Path, PathBuf};
use std::process::Command;

fn enabled() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn base_dir() -> PathBuf {
    let root = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace().join("target"));
    root.join("real-rustflags-guard")
}

fn tvar(target: &str) -> String {
    let t: String = target
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("CARGO_TARGET_{t}_RUSTFLAGS")
}

const SERIAL: &str = "--cfg curve25519_dalek_backend=\"serial\"";

#[derive(Clone, Copy, PartialEq, Debug)]
enum Want {
    Pass,
    GuardFail,
}

struct Case {
    name: &'static str,
    /// Run cargo from outside the repo with --manifest-path (no .cargo/config.toml found).
    outside: bool,
    env: Vec<(String, String)>,
    extra: Vec<String>,
    want: Want,
}

fn cases(target: &str) -> Vec<Case> {
    let e = |k: &str, v: &str| vec![(k.to_string(), v.to_string())];
    let mut v = vec![
        Case {
            outside: false,
            name: "normal",
            env: vec![],
            extra: vec![],
            want: Want::Pass,
        },
        Case {
            outside: false,
            name: "rustflags-empty",
            env: e("RUSTFLAGS", ""),
            extra: vec![],
            want: Want::GuardFail,
        },
        Case {
            outside: false,
            name: "rustflags-debuginfo",
            env: e("RUSTFLAGS", "-C debuginfo=0"),
            extra: vec![],
            want: Want::GuardFail,
        },
        Case {
            outside: false,
            name: "rustflags-keeps-cfg",
            env: e("RUSTFLAGS", SERIAL),
            extra: vec![],
            want: Want::Pass,
        },
        Case {
            outside: false,
            name: "rustflags-debuginfo-keeps-cfg",
            env: e("RUSTFLAGS", &format!("-C debuginfo=0 {SERIAL}")),
            extra: vec![],
            want: Want::Pass,
        },
        Case {
            outside: false,
            name: "encoded-rustflags-empty",
            env: e("CARGO_ENCODED_RUSTFLAGS", ""),
            extra: vec![],
            want: Want::GuardFail,
        },
        Case {
            outside: false,
            name: "target-rustflags-env",
            env: e(&tvar(target), "-C debuginfo=0"),
            extra: vec![],
            want: Want::GuardFail,
        },
        Case {
            outside: false,
            name: "target-rustflags-config",
            env: vec![],
            extra: vec![
                "--config".into(),
                format!("target.{target}.rustflags=[\"-Cdebuginfo=0\"]"),
            ],
            want: Want::GuardFail,
        },
    ];
    // Built from outside the repo: cargo never finds the workspace
    // .cargo/config.toml (config is looked up from the cwd, not the manifest).
    v.push(Case {
        outside: true,
        name: "outside-cwd-manifest-path",
        env: vec![],
        extra: vec![],
        want: Want::GuardFail,
    });
    if target.starts_with("x86_64") {
        // Explicit SIMD request: dalek accepts it on x86_64; the guard must not.
        v.push(Case {
            outside: false,
            name: "rustflags-simd",
            env: e("RUSTFLAGS", "--cfg curve25519_dalek_backend=\"simd\""),
            extra: vec![],
            want: Want::GuardFail,
        });
    }
    v
}

/// The backend curve25519-dalek's build script picked, from `-vv` output.
fn dalek_backend(out: &str) -> Option<String> {
    out.lines()
        .filter(|l| {
            l.contains("[curve25519-dalek") && l.contains("rustc-cfg=curve25519_dalek_backend=")
        })
        .filter_map(|l| l.split("curve25519_dalek_backend=").nth(1))
        .map(|s| s.trim().trim_matches('"').to_string())
        .next_back()
}

fn run_case(target: &str, c: &Case, n: &mut (u32, u32)) {
    let dir = base_dir().join(target).join(c.name);
    let _ = std::fs::remove_dir_all(&dir);
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new(cargo);
    // temp_dir is outside the repo, so no .cargo/config.toml above it.
    let outside_dir =
        std::env::temp_dir().join(format!("hades-guard-outside-{}", std::process::id()));
    std::fs::create_dir_all(&outside_dir).unwrap();
    if c.outside {
        cmd.current_dir(&outside_dir)
            .arg("check")
            .arg("--manifest-path")
            .arg(workspace().join("Cargo.toml"));
    } else {
        cmd.current_dir(workspace()).arg("check");
    }
    cmd.args([
        "-vv",
        "--offline",
        "--locked",
        "-j",
        "2",
        "-p",
        "hades-gate",
        "--lib",
    ])
    .args(["--target", target])
    .args(&c.extra)
    .env("CARGO_TARGET_DIR", &dir)
    .env("CARGO_INCREMENTAL", "0");
    for k in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_RUSTFLAGS",
        "CARGO_BUILD_TARGET",
    ] {
        cmd.env_remove(k);
    }
    cmd.env_remove(tvar(target));
    for (k, v) in &c.env {
        cmd.env(k, v);
    }
    let t0 = std::time::Instant::now();
    let o = cmd.output().expect("spawn cargo");
    let secs = t0.elapsed().as_secs_f32();
    let err = String::from_utf8_lossy(&o.stderr).to_string() + &String::from_utf8_lossy(&o.stdout);
    let backend = dalek_backend(&err).unwrap_or_else(|| "?".into());
    let guard_hit = err.contains("hades-gate: SIMD guard (project rule 1, no SIMD)");
    let got = if o.status.success() {
        Want::Pass
    } else {
        Want::GuardFail
    };
    let env_names: Vec<&str> = c.env.iter().map(|(k, _)| k.as_str()).collect();
    println!(
        "case target={target} name={} env={:?} config_override={} exit={:?} want={:?} guard_msg={} dalek_backend={} secs={secs:.1}",
        c.name,
        env_names,
        !c.extra.is_empty(),
        o.status.code(),
        c.want,
        guard_hit,
        backend,
    );
    let ok = match c.want {
        Want::Pass => got == Want::Pass && !guard_hit && backend == "serial",
        Want::GuardFail => got == Want::GuardFail && guard_hit,
    };
    if ok {
        n.0 += 1;
    } else {
        n.1 += 1;
        let tail: Vec<&str> = err.lines().rev().take(25).collect();
        println!("  FAIL detail (last lines, reversed):");
        for l in tail {
            println!("    {l}");
        }
    }
    // Which overriding env var the guard could name (cargo hides some of
    // them from build scripts; then the message lists every source).
    if guard_hit {
        let named = err
            .split("REPLACE the workspace `build.rustflags`: ")
            .nth(1)
            .and_then(|s| s.split('.').next())
            .unwrap_or("-");
        println!("  guard_named_vars={named}");
    }
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&outside_dir);
}

#[test]
fn rustflags_guard_real_cargo() {
    if !enabled() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let targets = std::env::var("HADES_GUARD_TARGETS")
        .unwrap_or_else(|_| "x86_64-unknown-linux-musl,aarch64-unknown-linux-musl".into());
    let mut n = (0u32, 0u32);
    for t in targets.split(',').filter(|t| !t.is_empty()) {
        for c in cases(t) {
            run_case(t, &c, &mut n);
        }
    }
    let _ = std::fs::remove_dir_all(base_dir());
    println!("summary pass={} fail={}", n.0, n.1);
    assert_eq!(n.1, 0, "guard cases failed");
}

/// The `compile_error!` backstop in src/lib.rs, without cargo or build.rs:
/// plain rustc on lib.rs must print the guard error without the cfg and must
/// not with it (other errors, e.g. unresolved crates, are expected there).
#[test]
fn compile_error_backstop_plain_rustc() {
    if !enabled() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let lib = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    let out = base_dir().join("backstop");
    std::fs::create_dir_all(&out).unwrap();
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let run = |cfg: bool| {
        let mut cmd = Command::new(&rustc);
        cmd.args([
            "--edition=2021",
            "--crate-type=lib",
            "--crate-name=hades_gate",
            "--emit=metadata",
        ])
        .arg("--out-dir")
        .arg(&out)
        .arg(&lib);
        if cfg {
            cmd.args(["--cfg", "curve25519_dalek_backend=\"serial\""]);
        }
        let o = cmd.output().expect("spawn rustc");
        let e = String::from_utf8_lossy(&o.stderr).to_string();
        (
            o.status.code(),
            e.contains("hades-gate: SIMD guard (project rule 1, no SIMD)"),
        )
    };
    let (c1, g1) = run(false);
    let (c2, g2) = run(true);
    println!("backstop no-cfg exit={c1:?} guard_msg={g1}; with-cfg exit={c2:?} guard_msg={g2}");
    let _ = std::fs::remove_dir_all(&out);
    assert!(g1, "compile_error! backstop did not fire without the cfg");
    assert!(!g2, "compile_error! backstop fired with the cfg set");
}

/// Normal release builds still work on both arches: build the real binary
/// for each target, run `keygen` (Ed25519 via the serial backend) and
/// `plan-hash` (sha2 force-soft); aarch64 runs under qemu-aarch64 when the
/// box has it. Plan hashes must match across arches. Throwaway keys only;
/// the seed never leaves the temp dir and is never printed.
#[test]
fn normal_release_binaries_run() {
    if !enabled() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let targets = std::env::var("HADES_GUARD_TARGETS")
        .unwrap_or_else(|_| "x86_64-unknown-linux-musl,aarch64-unknown-linux-musl".into());
    let tmp = base_dir().join("run");
    std::fs::create_dir_all(&tmp).unwrap();
    let plan = tmp.join("plan.txt");
    std::fs::write(&plan, b"restart openmind-nerve.service\n").unwrap();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut hashes = Vec::new();
    for t in targets.split(',').filter(|t| !t.is_empty()) {
        let dir = base_dir().join("bin").join(t);
        let mut cmd = Command::new(&cargo);
        cmd.current_dir(workspace())
            .args([
                "build",
                "--offline",
                "--locked",
                "--release",
                "-j",
                "2",
                "-p",
                "hades-gate",
            ])
            .args(["--bin", "hades-gate", "--target", t])
            .env("CARGO_TARGET_DIR", &dir)
            .env("CARGO_INCREMENTAL", "0");
        for k in [
            "RUSTFLAGS",
            "CARGO_ENCODED_RUSTFLAGS",
            "CARGO_BUILD_RUSTFLAGS",
            "CARGO_BUILD_TARGET",
        ] {
            cmd.env_remove(k);
        }
        cmd.env_remove(tvar(t));
        if t.starts_with("aarch64") {
            cmd.env("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER", "rust-lld");
        }
        let o = cmd.output().expect("spawn cargo build");
        assert!(
            o.status.success(),
            "{t}: normal release build failed:\n{}",
            String::from_utf8_lossy(&o.stderr)
        );
        let bin = dir.join(t).join("release/hades-gate");
        let size = std::fs::metadata(&bin).unwrap().len();
        let host_arch = std::env::consts::ARCH;
        let runner: Option<&str> = if t.starts_with(host_arch) {
            None
        } else if t.starts_with("aarch64") && Path::new("/usr/bin/qemu-aarch64").exists() {
            Some("/usr/bin/qemu-aarch64")
        } else {
            println!("bin target={t} size={size} run=skipped (no runner)");
            continue;
        };
        let exec = |args: &[&str]| {
            let mut c = match runner {
                Some(r) => {
                    let mut c = Command::new(r);
                    c.arg(&bin);
                    c
                }
                None => Command::new(&bin),
            };
            c.args(args).output().expect("run hades-gate")
        };
        let kd = tmp.join(t);
        std::fs::create_dir_all(&kd).unwrap();
        let seed = kd.join("admin.seed");
        let public = kd.join("admin.pub");
        let k = exec(&[
            "keygen",
            "--seed",
            seed.to_str().unwrap(),
            "--pub",
            public.to_str().unwrap(),
        ]);
        let kout = String::from_utf8_lossy(&k.stdout).to_string();
        assert!(
            k.status.success() && kout.contains("\"ok\":true"),
            "{t}: keygen failed"
        );
        let h = exec(&["plan-hash", "--plan-file", plan.to_str().unwrap()]);
        assert!(h.status.success(), "{t}: plan-hash failed");
        let hash = String::from_utf8_lossy(&h.stdout).trim().to_string();
        println!(
            "bin target={t} size={size} runner={} keygen=ok plan_hash={hash}",
            runner.unwrap_or("native")
        );
        hashes.push(hash);
    }
    let _ = std::fs::remove_dir_all(base_dir());
    assert!(!hashes.is_empty());
    assert!(
        hashes.windows(2).all(|w| w[0] == w[1]),
        "plan hash differs across arches"
    );
}
