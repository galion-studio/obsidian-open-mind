// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The systemd backend, end to end through the real binary: `systemctl` and
//! `journalctl` are the `sm_shim` example on PATH, the health URL is a test server on
//! 127.0.0.1:19394 (slot 39 range). Replaces scripts/real/galion-deploy-equiv.sh:
//! the same ten scenarios, with the exact systemctl/journalctl call sequence
//! asserted, plus first install, signals, the journal and the lock. Every intended
//! difference from the bash script is spelled out next to its assertion.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PORT: u16 = 19394;

fn exe_dir() -> PathBuf {
    // target/<profile>/deps/systemd_backend-<hash> -> target/<profile>
    let me = std::env::current_exe().unwrap();
    me.parent().unwrap().parent().unwrap().to_path_buf()
}

fn shim() -> PathBuf {
    let p = exe_dir().join("examples/sm_shim");
    if !p.exists() {
        let st = Command::new(env!("CARGO"))
            .args(["build", "-q", "-p", "galion-deploy", "--example", "sm_shim"])
            .args(if exe_dir().ends_with("release") {
                vec!["--release"]
            } else {
                vec![]
            })
            .status()
            .unwrap();
        assert!(st.success());
    }
    p
}

struct World {
    d: PathBuf,
    target: PathBuf,
    bad: PathBuf,
    good: PathBuf,
    old: PathBuf,
    script: PathBuf,
    state: PathBuf,
    bdir: PathBuf,
}

/// Health server: 200 while the shim's unit is active and not forced bad.
#[derive(Default)]
struct Health {
    state: Option<PathBuf>,
    all_bad: bool,
}

fn serve(h: Arc<Mutex<Health>>) {
    let l = TcpListener::bind(("127.0.0.1", PORT)).expect("bind 19394");
    std::thread::spawn(move || {
        for c in l.incoming() {
            let Ok(mut c) = c else { continue };
            let mut b = [0u8; 1024];
            let _ = c.set_read_timeout(Some(Duration::from_secs(1)));
            let _ = c.read(&mut b);
            let ok = {
                let g = h.lock().unwrap();
                !g.all_bad && g.state.as_ref().map(|s| s.exists()).unwrap_or(false)
            };
            let line = if ok {
                "200 OK"
            } else {
                "500 Internal Server Error"
            };
            let _ = c.write_all(
                format!("HTTP/1.1 {line}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            );
        }
    });
}

fn world(tag: &str) -> World {
    let d = std::env::temp_dir().join(format!("gd-sd-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("opt")).unwrap();
    fs::create_dir_all(d.join("shim")).unwrap();
    for n in ["systemctl", "journalctl", "ctl", "data-backup"] {
        std::os::unix::fs::symlink(shim(), d.join("shim").join(n)).unwrap();
    }
    let w = World {
        target: d.join("opt/tuner"),
        bad: d.join("bad.bin"),
        good: d.join("good.bin"),
        old: d.join("old.bin"),
        script: d.join("script.bin"),
        state: d.join("active"),
        bdir: d.join("backups/galion/deploy"),
        d,
    };
    fs::write(&w.old, b"\x7fELFgood-old").unwrap();
    fs::write(&w.good, b"\x7fELFgood-new").unwrap();
    fs::write(&w.bad, b"\x7fELFbad-new").unwrap();
    fs::write(&w.script, b"#!/bin/sh\n").unwrap();
    for p in [&w.old, &w.good, &w.bad] {
        fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::copy(&w.old, &w.target).unwrap();
    fs::write(&w.state, b"").unwrap();
    w
}

impl World {
    fn cmd(&self, args: &[&str], extra: &[(&str, &str)]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_galion-deploy"));
        let path = format!(
            "{}:{}",
            self.d.join("shim").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        c.args(args)
            .env_clear()
            .env("PATH", path)
            .env("SHIM_CALLS", self.d.join("calls"))
            .env("SHIM_KNOWN", "tuner")
            .env("SHIM_TARGET", &self.target)
            .env("SHIM_STATE", &self.state)
            .env("SHIM_BAD", &self.bad)
            .env("GALION_DEPLOY_BACKUP_DIR", &self.bdir)
            .env("GALION_DEPLOY_HEALTH_INTERVAL_MS", "10")
            .env("SKIP_DATA_BACKUP", "1")
            .stdin(Stdio::null());
        for (k, v) in extra {
            c.env(k, v);
        }
        c
    }
    fn run(&self, args: &[&str], extra: &[(&str, &str)]) -> (i32, String, Vec<String>) {
        let _ = fs::remove_file(self.d.join("calls"));
        let o: Output = self.cmd(args, extra).output().unwrap();
        let out =
            String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
        (o.status.code().unwrap_or(-1), out, self.calls())
    }
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.d.join("calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
    fn target(&self) -> Vec<u8> {
        fs::read(&self.target).unwrap_or_default()
    }
    fn backups(&self) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(&self.bdir)
            .map(|r| {
                r.flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.d);
    }
}

const CAT: &str = "systemctl cat tuner.service";
const SHOW: &str = "systemctl show -p ExecStart --value tuner.service";
const RESET: &str = "systemctl reset-failed tuner.service";
const RESTART: &str = "systemctl restart tuner.service";
const ACTIVE: &str = "systemctl is-active --quiet tuner.service";
const JOURNAL: &str = "journalctl -u tuner.service -n 20 --no-pager -o cat";
const FFSHOW: &str = "systemctl show -p ActiveState -p SubState -p NRestarts tuner.service";

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn rep(x: &str, n: usize) -> Vec<String> {
    vec![x.to_string(); n]
}

fn wait_for(p: impl Fn() -> bool, ms: u64) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(ms) {
        if p() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn spawn(w: &World, args: &[&str], extra: &[(&str, &str)]) -> Child {
    w.cmd(args, extra)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// All scenarios in one test: they share the one health port.
#[test]
fn systemd_backend_scenarios() {
    let h = Arc::new(Mutex::new(Health::default()));
    serve(h.clone());
    let url = format!("http://127.0.0.1:{PORT}/health");
    let set = |w: &World, all_bad: bool| {
        let mut g = h.lock().unwrap();
        g.state = Some(w.state.clone());
        g.all_bad = all_bad;
    };
    let mut report = Vec::new();

    // 1 healthy (bash: cat, show, restart, sleep, is-active, curl)
    let w = world("healthy");
    set(&w, false);
    let good = w.good.display().to_string();
    let (rc, out, calls) = w.run(&["tuner", &good, &url], &[]);
    assert_eq!(rc, 0, "{out}");
    // intended: reset-failed before restart (F2); sleep/curl are in-process now
    assert_eq!(calls, s(&[CAT, SHOW, RESET, RESTART, ACTIVE]));
    assert_eq!(w.target(), b"\x7fELFgood-new");
    let b = w.backups();
    assert_eq!(b.len(), 2, "{b:?}"); // the .bin and tuner.log
    assert!(
        b[0].starts_with("tuner-")
            && b[0].ends_with(".bin")
            && b[0].len() == "tuner-20261020-225020+0200.bin".len()
    );
    assert_eq!(b[1], "tuner.log");
    assert!(out.contains("level=info msg=\"backup\" unit=tuner target="));
    assert!(out.contains(" sha_old=") && out.contains(" sha_new="));
    assert!(out.contains(&format!(
        "level=info msg=\"deployed\" unit=tuner health={url}"
    )));
    for d in [w.bdir.clone(), w.bdir.parent().unwrap().to_path_buf()] {
        assert_eq!(
            fs::metadata(&d).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    let bm = fs::metadata(w.bdir.join(&b[0])).unwrap();
    assert_eq!(
        bm.permissions().mode() & 0o777,
        0o755,
        "cp -p keeps the mode"
    );
    assert_eq!(
        fs::metadata(&w.target).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert!(!w.bdir.join("tuner.journal").exists());
    report.push(format!("healthy: exit {rc}, {} calls", calls.len()));

    // 2 unit suffix and table URL
    let w = world("suffix");
    set(&w, false);
    let (rc, _, calls) = w.run(&["tuner.service", &w.good.display().to_string(), &url], &[]);
    assert_eq!(rc, 0);
    assert_eq!(calls, s(&[CAT, SHOW, RESET, RESTART, ACTIVE]));
    report.push(format!(
        "healthy-unit-suffix: exit {rc}, {} calls",
        calls.len()
    ));

    // 3 broken rolls back. bash: exit 1; intended: exit 3 (F3)
    let w = world("broken");
    set(&w, false);
    let (rc, out, calls) = w.run(&["tuner", &w.bad.display().to_string(), &url], &[]);
    assert_eq!(rc, 3, "{out}");
    let mut exp = s(&[CAT, SHOW, RESET, RESTART]);
    exp.extend(rep(ACTIVE, 20));
    exp.extend(s(&[JOURNAL, RESET, RESTART, ACTIVE]));
    assert_eq!(calls, exp);
    assert_eq!(w.target(), b"\x7fELFgood-old");
    assert!(out.contains("level=error msg=\"health failed, rolling back\" unit=tuner"));
    assert!(out.contains("  | tuner: bind 127.0.0.1:8650: boom"));
    assert!(out.contains("level=warn msg=\"rolled back ok\" unit=tuner"));
    report.push(format!(
        "broken-rolls-back: exit {rc}, {} calls",
        calls.len()
    ));

    // 3b fail-fast (new, opt-in GALION_DEPLOY_FAILFAST=2): the crash-looping build is
    // given up after 2 crashes instead of 20 tries; one extra read per poll,
    // `systemctl show -p ActiveState -p SubState -p NRestarts`. The rollback is unchanged.
    let w = world("failfast");
    set(&w, false);
    let counter = w.d.join("nrestarts");
    let (rc, out, calls) = w.run(
        &["tuner", &w.bad.display().to_string(), &url],
        &[
            ("GALION_DEPLOY_FAILFAST", "2"),
            ("SHIM_NRESTARTS", counter.to_str().unwrap()),
        ],
    );
    assert_eq!(rc, 3, "{out}");
    assert_eq!(
        calls,
        s(&[CAT, SHOW, RESET, RESTART, FFSHOW, FFSHOW, JOURNAL, RESET, RESTART, ACTIVE])
    );
    assert_eq!(w.target(), b"\x7fELFgood-old");
    assert!(out.contains("level=info msg=\"fail-fast armed\" unit=tuner crash_limit=2 nrestarts_base=1 state=activating/auto-restart"));
    assert!(out.contains("msg=\"crash loop: 2 crash(es) of the new build since the deploy restart (limit 2); failing fast\" unit=tuner"));
    assert!(out.contains("level=error msg=\"health failed, rolling back\" unit=tuner"));
    assert!(out.contains("level=warn msg=\"rolled back ok\" unit=tuner"));
    report.push(format!(
        "failfast-broken: exit {rc}, {} calls (gave up on the first poll instead of after 20 tries)",
        calls.len()
    ));

    // 3c fail-fast asked for, but systemd gives no NRestarts (older than 235): warn,
    // then exactly the old 20-try window.
    let w = world("failfast-old");
    set(&w, false);
    let (rc, out, calls) = w.run(
        &["tuner", &w.bad.display().to_string(), &url],
        &[("GALION_DEPLOY_FAILFAST", "2")],
    );
    assert_eq!(rc, 3, "{out}");
    let mut exp = s(&[CAT, SHOW, RESET, RESTART, FFSHOW]);
    exp.extend(rep(ACTIVE, 20));
    exp.extend(s(&[JOURNAL, RESET, RESTART, ACTIVE]));
    assert_eq!(calls, exp);
    assert!(out.contains("fail-fast off: cannot read NRestarts; using the full health window"));
    report.push(format!(
        "failfast-no-nrestarts: exit {rc}, {} calls (full window)",
        calls.len()
    ));

    // 3d fail-fast on, healthy build: one arming read, otherwise the healthy sequence
    let w = world("failfast-ok");
    set(&w, false);
    let counter = w.d.join("nrestarts");
    let (rc, out, calls) = w.run(
        &["tuner", &w.good.display().to_string(), &url],
        &[
            ("GALION_DEPLOY_FAILFAST", "1"),
            ("SHIM_NRESTARTS", counter.to_str().unwrap()),
        ],
    );
    assert_eq!(rc, 0, "{out}");
    assert_eq!(
        calls,
        s(&[CAT, SHOW, RESET, RESTART, FFSHOW, FFSHOW, ACTIVE])
    );
    assert!(!out.contains("crash loop"));
    report.push(format!(
        "failfast-healthy: exit {rc}, {} calls",
        calls.len()
    ));

    // 4 rollback also bad. bash: exit 1; intended: exit 4
    let w = world("allbad");
    set(&w, true);
    let (rc, out, calls) = w.run(&["tuner", &w.good.display().to_string(), &url], &[]);
    assert_eq!(rc, 4, "{out}");
    assert!(out.contains("ROLLBACK ALSO UNHEALTHY for tuner; manual action needed (backup "));
    assert_eq!(calls.iter().filter(|c| *c == ACTIVE).count(), 40);
    assert_eq!(w.target(), b"\x7fELFgood-old");
    report.push(format!(
        "rollback-also-bad: exit {rc}, {} calls",
        calls.len()
    ));

    // 5 restart refused. bash: died under set -e, exit 1, NEW binary left installed
    // (no rollback). Intended: retry once after reset-failed, then roll back -> 4
    // (the rollback restart is refused too), OLD binary back on disk.
    let w = world("refused");
    set(&w, false);
    let (rc, out, calls) = w.run(
        &["tuner", &w.good.display().to_string(), &url],
        &[("SHIM_RESTART_RC", "1")],
    );
    assert!(
        out.contains("level=error msg=\"restart refused, rolling back\""),
        "{out}"
    );
    assert_eq!(rc, 4, "{out}");
    assert_eq!(w.target(), b"\x7fELFgood-old");
    assert_eq!(
        calls,
        s(&[CAT, SHOW, RESET, RESTART, RESET, RESTART, JOURNAL, RESET, RESTART, RESET, RESTART])
    );
    report.push(format!(
        "restart-refused: exit {rc}, {} calls (old binary restored)",
        calls.len()
    ));

    // 6 unit-only health (none): is-active 4x, then twice on try 5, like bash
    let w = world("none");
    set(&w, false);
    let (rc, _, calls) = w.run(&["tuner", &w.good.display().to_string(), "none"], &[]);
    assert_eq!(rc, 0);
    let mut exp = s(&[CAT, SHOW, RESET, RESTART]);
    exp.extend(rep(ACTIVE, 6));
    assert_eq!(calls, exp);
    report.push(format!(
        "unit-only-health: exit {rc}, {} calls",
        calls.len()
    ));

    // 7-10 refusals: same exit codes and calls as bash
    let w = world("refusals");
    let (rc, out, calls) = w.run(&["nosuch", &w.good.display().to_string()], &[]);
    assert_eq!((rc, calls), (1, s(&["systemctl cat nosuch.service"])));
    assert!(out.contains("level=error msg=\"unknown unit nosuch.service\""));
    report.push("unknown-unit: exit 1, 1 call".into());
    let (rc, out, calls) = w.run(&["tuner", &w.script.display().to_string()], &[]);
    assert_eq!((rc, calls), (1, s(&[CAT, SHOW])));
    assert!(out.contains("is not an ELF binary"));
    report.push("not-elf: exit 1, 2 calls".into());
    let (rc, out, calls) = w.run(&["tuner", &w.d.join("nope.bin").display().to_string()], &[]);
    assert_eq!((rc, calls), (1, s(&[CAT, SHOW])));
    assert!(out.contains("no such file"));
    report.push("missing-new: exit 1, 2 calls".into());
    let (rc, out, calls) = w.run(&["tuner"], &[]);
    assert_eq!((rc, calls.len()), (2, 0));
    assert_eq!(
        out.trim(),
        "usage: galion-deploy <service> <new-binary> [health-url]"
    );
    report.push("usage: exit 2, 0 calls".into());
    assert!(w.backups().is_empty(), "refusals touch nothing");

    // 11 first install (F5): bash died "cannot resolve installed binary"
    let w = world("first");
    set(&w, false);
    fs::remove_file(&w.target).unwrap();
    fs::remove_file(&w.state).unwrap();
    let (rc, out, calls) = w.run(&["tuner", &w.good.display().to_string(), &url], &[]);
    assert_eq!(rc, 0, "{out}");
    assert_eq!(calls, s(&[CAT, SHOW, ACTIVE, RESET, RESTART, ACTIVE]));
    assert!(out.contains("msg=\"first install\""));
    assert_eq!(w.target(), b"\x7fELFgood-new");
    let (rc2, _, _) = {
        // and a failed first install leaves nothing behind
        let w2 = world("firstbad");
        set(&w2, false);
        fs::remove_file(&w2.target).unwrap();
        fs::remove_file(&w2.state).unwrap();
        let r = w2.run(&["tuner", &w2.bad.display().to_string(), &url], &[]);
        assert!(!w2.target.exists());
        assert!(r.2.contains(&"systemctl stop tuner.service".to_string()));
        set(&w, false);
        r
    };
    assert_eq!(rc2, 3);
    report.push(
        "first-install: exit 0; broken first install: exit 3, binary removed, unit stopped".into(),
    );

    // 12 SIGTERM during the health window (F6): bash left the new binary, no rollback
    let w = world("sigterm");
    set(&w, false);
    let c = spawn(
        &w,
        &["tuner", &w.bad.display().to_string(), &url],
        &[("GALION_DEPLOY_HEALTH_INTERVAL_MS", "200")],
    );
    assert!(wait_for(|| w.target() == b"\x7fELFbad-new", 5000));
    std::thread::sleep(Duration::from_millis(300));
    let pid = c.id() as i32;
    assert!(Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .unwrap()
        .success());
    let o = c.wait_with_output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(5), "{out}");
    assert!(out.contains("interrupted by SIGTERM, rolling back"));
    assert_eq!(w.target(), b"\x7fELFgood-old");
    assert!(!w.bdir.join("tuner.journal").exists());
    report.push("sigterm-in-health-window: exit 5, old binary back".into());

    // 13 SIGKILL (uncatchable): the journal survives; --recover restores; lock
    let w = world("sigkill");
    set(&w, false);
    let mut c = spawn(
        &w,
        &["tuner", &w.bad.display().to_string(), &url],
        &[("GALION_DEPLOY_HEALTH_INTERVAL_MS", "500")],
    );
    assert!(wait_for(|| w.target() == b"\x7fELFbad-new", 5000));
    // while it runs, a second deploy of the same unit is refused (lock)
    let (rc, out, _) = w.run(&["tuner", &w.good.display().to_string(), &url], &[]);
    assert_eq!(rc, 1, "{out}");
    assert!(out.contains("another galion-deploy (pid "), "{out}");
    c.kill().unwrap();
    let _ = c.wait();
    let j = fs::read_to_string(w.bdir.join("tuner.journal")).unwrap();
    assert!(j.contains("phase=verify"), "{j}");
    assert_eq!(
        w.target(),
        b"\x7fELFbad-new",
        "killed mid-deploy: new binary still there"
    );
    let (rc, out, _) = w.run(&["--recover", "tuner"], &[]);
    assert_eq!(rc, 3, "{out}");
    assert!(out.contains("found the journal of an interrupted deploy"));
    assert_eq!(w.target(), b"\x7fELFgood-old");
    let (rc, _, _) = w.run(&["--recover", "tuner"], &[]);
    assert_eq!(rc, 0);
    report.push("sigkill: journal phase=verify kept; second deploy refused (lock, exit 1); --recover exit 3, old binary back; again exit 0".into());

    // 14 data backup runs unless SKIP_DATA_BACKUP=1 (as before)
    let w = world("data");
    set(&w, false);
    let db = w.d.join("shim/data-backup");
    let mark = w.d.join("ran");
    let mut c = w.cmd(
        &["tuner", &w.good.display().to_string(), &url],
        &[
            ("GALION_DEPLOY_DATA_BACKUP", db.to_str().unwrap()),
            ("SHIM_DATA_MARK", mark.to_str().unwrap()),
        ],
    );
    c.env_remove("SKIP_DATA_BACKUP");
    assert_eq!(c.output().unwrap().status.code(), Some(0));
    assert!(w.d.join("ran").exists());
    report.push("data-backup: runs when executable and not skipped".into());

    // 15 pluggable: direct backend with a ctl command
    let w = world("direct");
    set(&w, false);
    let ctl = w.d.join("shim/ctl");
    let (rc, out, _) = w.run(
        &["tuner", &w.bad.display().to_string(), &url],
        &[
            ("GALION_DEPLOY_SM", "direct"),
            ("GALION_DEPLOY_CTL", ctl.to_str().unwrap()),
        ],
    );
    assert_eq!(rc, 3, "{out}");
    let (rc, out, _) = w.run(&["tuner", &good], &[("GALION_DEPLOY_SM", "direct")]);
    assert_eq!(rc, 1);
    assert!(out.contains("GALION_DEPLOY_SM=direct needs GALION_DEPLOY_CTL"));
    let (rc, out, _) = w.run(&["tuner", &good], &[("GALION_DEPLOY_SM", "upstart")]);
    assert_eq!(rc, 1);
    assert!(out.contains("unknown GALION_DEPLOY_SM=upstart (systemd|direct)"));
    report.push("direct backend: broken -> exit 3; missing CTL / unknown SM -> exit 1".into());

    eprintln!("galion-deploy systemd backend scenarios:");
    for r in &report {
        eprintln!("  OK  {r}");
    }
    let _ = Path::new("");
}
