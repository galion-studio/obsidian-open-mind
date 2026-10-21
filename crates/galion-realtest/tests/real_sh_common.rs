// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Rust port of `scripts/real/common.sh` (slot 16; deleted on
//! swarm/realscript-port-1). Real-life test for galion-common: build the
//! migrated `tuner` and `galion-origin` (release), start them, hammer them
//! over real HTTP with `crates/galion-common/tests/real_hammer.rs` (latency,
//! malformed, oversized, slowloris), sample memory/threads once a second,
//! tear down. With `GALION_REAL_OLD=1` the pre-migration binaries from
//! 13a296ce are built in a temporary git worktree and run too, for
//! comparison. Box only; never touches a live service.
//!
//! Ports: block 19160 (`GALION_REAL_PORT_BASE`): +0 tuner, +1 origin,
//! +2 old origin, +3 old tuner.
//! Run: `GALION_REAL=1 cargo test -p galion-realtest --test real_sh_common -- --nocapture`

use galion_realtest::script::*;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const OLD_REV: &str = "13a296ce15b5bbd66adc26f6bfae787b2c1b5190";

fn start(
    procs: &mut Procs,
    name: &str,
    bin: &Path,
    port: u16,
    envs: &[(&str, String)],
    logs: &Path,
) -> u32 {
    let mut c = Command::new(bin);
    c.current_dir(repo_root())
        .envs(envs.iter().map(|(k, v)| (*k, v.as_str())))
        .stdout(fs::File::create(logs.join(format!("{name}.log"))).unwrap())
        .stderr(Stdio::null());
    let pid = procs.spawn(name, c);
    println!("  {name} pid {pid} port {port}");
    assert!(
        wait_port(port, Duration::from_secs(5)),
        "{name} did not start: {}",
        tail(&logs.join(format!("{name}.log")), 20)
    );
    pid
}

fn mem(label: &str, watch: &[(u32, String)]) {
    for (pid, name) in watch {
        match (
            proc_field(*pid, "VmRSS"),
            proc_field(*pid, "VmHWM"),
            proc_field(*pid, "Threads"),
        ) {
            (Some(r), Some(h), Some(t)) => {
                println!("{label} {name}: VmRSS:{r} VmHWM:{h} Threads:{t}")
            }
            _ => println!("{label} {name}: DEAD"),
        }
    }
}

#[test]
fn real_sh_common() {
    if !enabled("common") {
        return;
    }
    let base = port_base(19160);
    let (tport, oport, old_oport, old_tport) = (base, base + 1, base + 2, base + 3);
    let old = std::env::var("GALION_REAL_OLD").as_deref() == Ok("1");
    require_free(&[tport, oport]);
    build(&["-p", "tuner", "-p", "galion-origin"]);
    let tuner = need_bin(&release_bin("tuner"));
    let origin = need_bin(&release_bin("galion-origin"));
    let scratch_dir = TempDir(scratch("galion-common-real"));
    let sd = scratch_dir.0.clone();
    fs::create_dir_all(sd.join("root/docs/build-session")).unwrap();
    fs::create_dir_all(sd.join("root/state")).unwrap();
    fs::create_dir_all(sd.join("grinder")).unwrap();
    fs::write(sd.join("root/docs/build-session/a.md"), "# note\n").unwrap();
    fs::write(sd.join("grinder/index.html"), "<h1>grinder</h1>\n").unwrap();

    let mut procs = Procs::new();
    let tp = start(
        &mut procs,
        "tuner",
        &tuner,
        tport,
        &[
            ("TUNER_ADDR", format!("127.0.0.1:{tport}")),
            ("TUNER_ROOT", "crates/tuner/www".into()),
        ],
        &sd,
    );
    let origin_env = |port: u16| {
        vec![
            ("ORIGIN_ADDR", format!("127.0.0.1:{port}")),
            ("ORIGIN_ROOT", sd.join("root").display().to_string()),
            (
                "ORIGIN_GRINDER_ROOT",
                sd.join("grinder").display().to_string(),
            ),
        ]
    };
    let op = start(
        &mut procs,
        "origin",
        &origin,
        oport,
        &origin_env(oport),
        &sd,
    );
    let mut targets = format!("tuner=127.0.0.1:{tport},origin=127.0.0.1:{oport}");
    let mut watch = vec![(tp, "tuner".to_string()), (op, "origin".to_string())];

    if old {
        require_free(&[old_oport, old_tport]);
        let tree = sd.join("old");
        let o = run({
            let mut c = Command::new("git");
            c.current_dir(repo_root())
                .args(["worktree", "add", "-q", "--detach"])
                .arg(&tree)
                .arg(OLD_REV);
            c
        });
        assert!(o.status.success(), "git worktree add: {}", err(&o));
        let old_target = target_dir().join("old");
        build_in(
            &tree,
            &["-p", "tuner", "-p", "galion-origin"],
            &[("CARGO_TARGET_DIR", old_target.to_str().unwrap())],
        );
        let _ = run({
            let mut c = Command::new("git");
            c.current_dir(repo_root())
                .args(["worktree", "remove", "--force"])
                .arg(&tree);
            c
        });
        let ot: PathBuf = old_target.join("release/tuner");
        let oo: PathBuf = old_target.join("release/galion-origin");
        let p = start(
            &mut procs,
            "old-tuner",
            &ot,
            old_tport,
            &[
                ("TUNER_ADDR", format!("127.0.0.1:{old_tport}")),
                ("TUNER_ROOT", "crates/tuner/www".into()),
            ],
            &sd,
        );
        watch.push((p, "old-tuner".into()));
        let p = start(
            &mut procs,
            "old-origin",
            &oo,
            old_oport,
            &origin_env(old_oport),
            &sd,
        );
        watch.push((p, "old-origin".into()));
        targets.push_str(&format!(
            ",old-tuner=127.0.0.1:{old_tport},old-origin=127.0.0.1:{old_oport}"
        ));
    }

    say("start");
    mem("before", &watch);
    // 1 s sampler (sleeping thread, no busy loop): peak threads per process.
    let stop = Arc::new(AtomicBool::new(false));
    let peaks = Arc::new(Mutex::new(BTreeMap::<String, u64>::new()));
    let sampler = {
        let (stop, peaks, watch) = (stop.clone(), peaks.clone(), watch.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(1));
                for (pid, name) in &watch {
                    let t = proc_field(*pid, "Threads")
                        .and_then(|t| t.parse().ok())
                        .unwrap_or(0);
                    let mut m = peaks.lock().unwrap();
                    let e = m.entry(name.clone()).or_insert(0);
                    *e = (*e).max(t);
                }
            }
        })
    };
    let jobs = std::env::var("GALION_REAL_JOBS").unwrap_or_else(|_| "2".into());
    let o = run({
        let mut c = Command::new("nice");
        c.args(["-n", "10"])
            .arg(cargo())
            .args([
                "test",
                "-q",
                "-j",
                &jobs,
                "--release",
                "-p",
                "galion-common",
                "--test",
                "real_hammer",
                "--",
                "--nocapture",
            ])
            .current_dir(repo_root())
            .env("CARGO_INCREMENTAL", "0")
            .env("GALION_REAL", "1")
            .env("GALION_REAL_TARGETS", &targets);
        c
    });
    stop.store(true, Ordering::Relaxed);
    let _ = sampler.join();
    for l in format!("{}{}", out(&o), err(&o)).lines() {
        if !(l.starts_with("running") || l.starts_with("test result") || l.trim().is_empty()) {
            println!("{l}");
        }
    }
    let mut ck = Checks::new();
    ck.ok(
        "galion-common real_hammer against the release services",
        o.status.success(),
        &format!("exit {}", code(&o)),
    );
    std::thread::sleep(Duration::from_secs(1));
    say("done");
    mem("after", &watch);
    println!("peak threads seen by the 1 s sampler:");
    for (k, v) in peaks.lock().unwrap().iter() {
        println!("  {k} {v}");
    }
    for (pid, name) in &watch {
        ck.ok(&format!("{name}: alive after the hammer"), alive(*pid), "");
    }
    drop(procs);
    ck.finish("common");
}
