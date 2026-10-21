// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `cargo xtask` entry point; see the library docs (xtask/src/lib.rs) and
//! docs/toolchain-aarch64.md.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use xtask::*;

const USAGE: &str = "\
usage:
  cargo xtask cross <triple> [cargo args]   run cargo for <triple> with zig cc for C code
                                             (default args: build --release --workspace)
  cargo xtask cross-env <triple>             print the KEY=VALUE env it would set
targets: aarch64-unknown-linux-musl armv7-unknown-linux-musleabihf
         x86_64-unknown-linux-musl aarch64-linux-android (check only)
zig: $GALION_ZIG, $ZIG, zig on PATH, <repo>/.tools/zig/zig (scripts/install-zig.sh .tools/zig),
     ~/.local/zig/zig, /opt/zig/zig, /workspace/tools/zig-x86_64-linux-0.15.2/zig";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the repo root")
        .to_path_buf()
}

fn find_zig() -> Option<PathBuf> {
    first_executable(&zig_candidates(
        std::env::var_os("GALION_ZIG"),
        std::env::var_os("ZIG"),
        std::env::var_os("PATH"),
        std::env::var_os("HOME"),
        &repo_root(),
    ))
}

fn exec_zig(args: Vec<String>) -> ExitCode {
    let Some(zig) = find_zig() else {
        eprintln!("galion-cc: zig not found (set GALION_ZIG or run via `cargo xtask cross`)");
        return ExitCode::from(127);
    };
    let e = Command::new(&zig).args(args).exec();
    eprintln!("galion-cc: exec {}: {e}", zig.display());
    ExitCode::from(127)
}

fn cc_mode(args: Vec<String>) -> ExitCode {
    let target = std::env::var("TARGET").ok();
    let Some(rust) = cc_rust_target(&args, target.as_deref()) else {
        eprintln!("galion-cc: no --target= argument and no $TARGET");
        return ExitCode::from(2);
    };
    let Some(zt) = zig_triple(&rust) else {
        eprintln!("galion-cc: unsupported target {rust:?}");
        return ExitCode::from(2);
    };
    exec_zig(zig_cc_args(zt, &args))
}

struct Setup {
    env: Vec<(String, String)>,
    zig: PathBuf,
}

fn setup(target: &str) -> Result<Setup, String> {
    if zig_triple(target).is_none() {
        return Err(format!("unknown target {target:?}\n{USAGE}"));
    }
    let zig = find_zig().ok_or_else(|| {
        format!("zig not found. Install the pinned {ZIG_PINNED} with `scripts/install-zig.sh .tools/zig` or set GALION_ZIG.\n{USAGE}")
    })?;
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let dir = exe
        .parent()
        .ok_or("current_exe has no parent")?
        .join("galion-cross");
    let cc = install_alias(&exe, &dir, CC_NAME).map_err(|e| format!("install {CC_NAME}: {e}"))?;
    let ar = install_alias(&exe, &dir, AR_NAME).map_err(|e| format!("install {AR_NAME}: {e}"))?;
    let td = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target"));
    let td = if td.is_relative() {
        repo_root().join(td)
    } else {
        td
    };
    let cache = td.join("zig-cache");
    let env = cross_env(target, &cc, &ar, &zig, &cache, &|k| {
        std::env::var_os(k).is_some()
    });
    Ok(Setup { env, zig })
}

fn zig_version(zig: &Path) -> String {
    Command::new(zig)
        .arg("version")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn task(args: Vec<String>) -> ExitCode {
    let (cmd, target) = (args.first().map(String::as_str), args.get(1));
    match (cmd, target) {
        (Some("cross-env"), Some(t)) => match setup(t) {
            Ok(s) => {
                for (k, v) in s.env {
                    println!("{k}={v}");
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("xtask: {e}");
                ExitCode::from(2)
            }
        },
        (Some("cross"), Some(t)) => {
            let s = match setup(t) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("xtask: {e}");
                    return ExitCode::from(2);
                }
            };
            let ver = zig_version(&s.zig);
            if ver != ZIG_PINNED {
                eprintln!(
                    "xtask: warning: zig {ver:?} at {}, pinned is {ZIG_PINNED}",
                    s.zig.display()
                );
            }
            let cargo_args = cross_cargo_args(t, &args[2..]);
            if t == "aarch64-linux-android"
                && cargo_args.first().map(String::as_str) != Some("check")
            {
                eprintln!(
                    "xtask: warning: android is check-only (C compiled against musl headers)"
                );
            }
            let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            eprintln!(
                "xtask: zig {ver} at {}; cargo {}",
                s.zig.display(),
                cargo_args.join(" ")
            );
            let st = Command::new(cargo)
                .args(&cargo_args)
                .envs(s.env)
                .current_dir(repo_root())
                .status();
            match st {
                Ok(st) if st.success() => ExitCode::SUCCESS,
                Ok(st) => ExitCode::from(st.code().unwrap_or(1).clamp(1, 255) as u8),
                Err(e) => {
                    eprintln!("xtask: run cargo: {e}");
                    ExitCode::from(127)
                }
            }
        }
        (Some("help" | "-h" | "--help"), _) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn main() -> ExitCode {
    let mut it = std::env::args();
    let argv0 = it.next().unwrap_or_default();
    let args: Vec<String> = it.collect();
    match mode_of(&argv0) {
        Mode::Cc => cc_mode(args),
        Mode::Ar => {
            let mut a = vec!["ar".to_string()];
            a.extend(args);
            exec_zig(a)
        }
        Mode::Task => task(args),
    }
}
