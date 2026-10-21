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
    env: Vec<(String, String)>,
    extra: Vec<String>,
    want: Want,
}

fn cases(target: &str) -> Vec<Case> {
    let e = |k: &str, v: &str| vec![(k.to_string(), v.to_string())];
    let mut v = vec![
        Case {
            name: "normal",
            env: vec![],
            extra: vec![],
            want: Want::Pass,
        },
        Case {
            name: "rustflags-empty",
            env: e("RUSTFLAGS", ""),
            extra: vec![],
            want: Want::GuardFail,
        },
        Case {
            name: "rustflags-debuginfo",
            env: e("RUSTFLAGS", "-C debuginfo=0"),
            extra: vec![],
            want: Want::GuardFail,
        },
        Case {
            name: "rustflags-keeps-cfg",
            env: e("RUSTFLAGS", SERIAL),
            extra: vec![],
            want: Want::Pass,
        },
        Case {
            name: "rustflags-debuginfo-keeps-cfg",
            env: e("RUSTFLAGS", &format!("-C debuginfo=0 {SERIAL}")),
            extra: vec![],
            want: Want::Pass,
        },
        Case {
            name: "encoded-rustflags-empty",
            env: e("CARGO_ENCODED_RUSTFLAGS", ""),
            extra: vec![],
            want: Want::GuardFail,
        },
        Case {
            name: "target-rustflags-env",
            env: e(&tvar(target), "-C debuginfo=0"),
            extra: vec![],
            want: Want::GuardFail,
        },
        Case {
            name: "target-rustflags-config",
            env: vec![],
            extra: vec![
                "--config".into(),
                format!("target.{target}.rustflags=[\"-Cdebuginfo=0\"]"),
            ],
            want: Want::GuardFail,
        },
    ];
    if target.starts_with("x86_64") {
        // Explicit SIMD request: dalek accepts it on x86_64; the guard must not.
        v.push(Case {
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
        .last()
}

fn run_case(target: &str, c: &Case, n: &mut (u32, u32)) {
    let dir = base_dir().join(target).join(c.name);
    let _ = std::fs::remove_dir_all(&dir);
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(workspace())
        .args([
            "check",
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
    // Name the overriding env var when cargo lets the build script see it.
    if c.name == "target-rustflags-env" && guard_hit {
        println!("  names_target_var={}", err.contains(&tvar(target)));
    }
    let _ = std::fs::remove_dir_all(&dir);
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
