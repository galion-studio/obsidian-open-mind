// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Port of `scripts/real/sandbox.sh` (deleted). Orchestrates the
//! openmind-sandbox adversarial suite (`openmind-sandbox/tests/real_escape.rs`)
//! the way the script did:
//!  * builds the `escape` payload and the `real_escape` test binary (release);
//!  * with `sudo -n` (or as root): makes a setuid-root copy of `/bin/id` so the
//!    setuid check is meaningful, and a delegated cgroup v2 subtree
//!    (`$CGROUP_ROOT/om-sandbox-real-<pid>`, `+cpu +memory +pids`, chowned to
//!    the run user) so the pids.max / memory.max checks run too;
//!  * runs the suite UNPRIVILEGED (this test refuses to run the suite as root),
//!    forked from inside `<cg>/runner` (root moves this test process in, and
//!    back out at teardown, as the script did with its shell);
//!  * tears everything down: cgroup.kill, rmdir depth-first, helper removed.
//!
//! Box only; never touches the VPS.
//!
//!   GALION_REAL=1 cargo test -p galion-realtest --test real_sh_sandbox -- --nocapture
//!
//! Env: `CGROUP_ROOT` (default /sys/fs/cgroup), `GALION_REAL_BIN_DIR`
//! (then the newest `deps/real_escape-*` and `examples/escape` there are used).

mod sh3;

use sh3::*;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn sudo(args: &[&str]) -> bool {
    Command::new("sudo")
        .arg("-n")
        .args(args)
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn sudo_write(file: &Path, text: &str) -> bool {
    let Ok(mut c) = Command::new("sudo")
        .args(["-n", "tee"])
        .arg(file)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let _ = c.stdin.take().unwrap().write_all(text.as_bytes());
    c.wait().is_ok_and(|s| s.success())
}

/// Newest executable `deps/real_escape-*` (not `.d`) in `rel`.
fn newest_test_bin(rel: &Path) -> Option<PathBuf> {
    std::fs::read_dir(rel.join("deps"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy();
            n.starts_with("real_escape-") && !n.ends_with(".d") && !n.contains('.')
        })
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

fn uid_gid() -> (String, String) {
    let id = |f: &str| out(&run(Command::new("id").arg(f))).trim().to_string();
    (id("-u"), id("-g"))
}

/// All dirs under `d` (deepest first), then `d`.
fn dirs_depth_first(d: &Path, acc: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(d) {
        for e in rd.flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                dirs_depth_first(&e.path(), acc);
            }
        }
    }
    acc.push(d.to_path_buf());
}

struct Teardown {
    cg: Option<PathBuf>,
    /// Where this process lived before it moved into the runner cgroup.
    orig: Option<PathBuf>,
    helper_dir: Option<PathBuf>,
}

impl Drop for Teardown {
    fn drop(&mut self) {
        let back = self
            .orig
            .as_ref()
            .is_none_or(|o| sudo_write(&o.join("cgroup.procs"), &std::process::id().to_string()));
        if let Some(cg) = &self.cg {
            let k = cg.join("runner/cgroup.kill");
            // Never cgroup.kill the runner while this process may still be in it.
            if back && k.exists() {
                sudo_write(&k, "1");
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
            let mut ds = Vec::new();
            dirs_depth_first(cg, &mut ds);
            for d in ds {
                sudo(&["rmdir", &d.to_string_lossy()]);
            }
            say(format!(
                "teardown: cgroup {} removed: {}",
                cg.display(),
                !cg.exists()
            ));
        }
        if let Some(h) = &self.helper_dir {
            sudo(&["rm", "-rf", &h.to_string_lossy()]);
        }
        let _ = std::fs::remove_dir_all(repo_root().join("target/test-out/escape"));
    }
}

#[test]
fn real_sh_sandbox() {
    if !enabled("real_sh_sandbox") {
        return;
    }
    let (uid, gid) = uid_gid();
    assert_ne!(
        uid, "0",
        "run the suite as an unprivileged user (the setuid check needs it)"
    );
    let (rel, test_bin) = if std::env::var_os("GALION_REAL_BIN_DIR").is_some() {
        let rel = release_bins(None, &[], &[]);
        let t = newest_test_bin(&rel).expect("no deps/real_escape-* in GALION_REAL_BIN_DIR");
        (rel, t)
    } else {
        let td = cargo_build(
            &[
                "build",
                "--release",
                "--locked",
                "-p",
                "openmind-sandbox",
                "--example",
                "escape",
            ],
            &[],
        );
        cargo_build(
            &[
                "test",
                "--release",
                "--locked",
                "-p",
                "openmind-sandbox",
                "--test",
                "real_escape",
                "--no-run",
            ],
            &[],
        );
        let rel = td.join("release");
        let t = newest_test_bin(&rel).expect("real_escape test binary");
        (rel, t)
    };
    need(&rel.join("examples/escape"));
    say(format!("suite binary: {}", test_bin.display()));

    let mut td = Teardown {
        cg: None,
        orig: None,
        helper_dir: None,
    };
    let can_root = sudo(&["true"]);
    let mut env: Vec<(String, String)> = vec![("GALION_REAL".into(), "1".into())];
    if can_root {
        let s = Scratch::new("om-sandbox-suid");
        let dir = s.0.clone();
        std::mem::forget(s); // removed by Teardown via sudo (root-owned file inside)
        let helper = dir.join("suid-id");
        let h = helper.to_string_lossy().to_string();
        let ok = sudo(&["cp", "/bin/id", &h])
            && sudo(&["chown", "root:root", &h])
            && sudo(&["chmod", "4755", &h]);
        td.helper_dir = Some(dir);
        assert!(ok, "setuid helper setup failed");
        env.push(("GALION_SETUID_HELPER".into(), h));

        let cg_root = std::env::var("CGROUP_ROOT").unwrap_or_else(|_| "/sys/fs/cgroup".into());
        let cg = PathBuf::from(cg_root).join(format!("om-sandbox-real-{}", std::process::id()));
        let cgs = cg.to_string_lossy().to_string();
        let mut ok = sudo(&["mkdir", "-p", &format!("{cgs}/runner")]);
        td.cg = Some(cg.clone());
        ok &= sudo_write(&cg.join("cgroup.subtree_control"), "+cpu +memory +pids");
        for f in [
            "",
            "cgroup.procs",
            "cgroup.subtree_control",
            "cgroup.threads",
            "runner",
            "runner/cgroup.procs",
            "runner/cgroup.subtree_control",
            "runner/cgroup.threads",
        ] {
            ok &= sudo(&["chown", &format!("{uid}:{gid}"), &format!("{cgs}/{f}")]);
        }
        assert!(ok, "delegated cgroup setup failed");
        env.push(("GALION_REAL_CGROUP_PARENT".into(), cgs));
        say(format!(
            "delegated cgroup {} (+cpu +memory +pids), setuid helper ready",
            cg.display()
        ));
    } else {
        say("no sudo -n: setuid and cgroup checks will be skipped by the suite");
    }

    say(format!(
        "run as uid {uid}{}",
        td.cg
            .as_ref()
            .map(|c| format!(" inside {}/runner", c.display()))
            .unwrap_or_default()
    ));
    // Like the script: root moves THIS process into <cg>/runner (an
    // unprivileged process may not move itself across the hierarchy), the
    // suite is forked from inside it, and Teardown moves us back out so the
    // subtree is empty and can be removed.
    if let Some(cg) = &td.cg {
        let orig = std::fs::read_to_string("/proc/self/cgroup")
            .unwrap_or_default()
            .lines()
            .find_map(|l| l.strip_prefix("0::").map(str::to_string))
            .expect("cgroup v2 path of this process");
        td.orig = Some(
            PathBuf::from(std::env::var("CGROUP_ROOT").unwrap_or_else(|_| "/sys/fs/cgroup".into()))
                .join(orig.trim_start_matches('/')),
        );
        assert!(
            sudo_write(
                &cg.join("runner/cgroup.procs"),
                &std::process::id().to_string()
            ),
            "could not move into the runner cgroup"
        );
    }
    let st = Command::new(&test_bin)
        .args(["--ignored", "--nocapture"])
        .envs(env.iter().map(|(k, v)| (k, v)))
        .status()
        .expect("run real_escape");
    say(format!("real_escape exit: {st}"));
    drop(td);
    assert!(st.success(), "real_escape suite failed");
}
