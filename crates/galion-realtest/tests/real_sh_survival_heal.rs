// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Port of `scripts/real/survival-heal.sh` (deleted). Release
//! `guardian-tick heal` against two REAL dummy processes
//! (`guardian-tick --example dummy_service`): 19842 stays healthy, 19843
//! dies after `DIE_S` seconds. Ticks: both up (dry-run), death detected and
//! planned but not acted on without `--allow`, three applied restarts that
//! really start a new dummy process (new pid each time, verified healthy),
//! then the reflex cap (no fourth restart). The restart backend is a plain
//! argv (`setsid -f dummy ...`), no shell. Then `classify` over the captured
//! live status.json in docs/real-tests/captures/ (read on the box only).
//! Never touches the VPS. Kills everything it started.
//!
//!   GALION_REAL=1 cargo test -p galion-realtest --test real_sh_survival_heal -- --nocapture

mod sh3;

use sh3::*;
use std::process::{Command, Stdio};
use std::time::Duration;

const HEALTHY: u16 = PORT_BASE + 2;
const DIES: u16 = PORT_BASE + 3;
const DIE_S: u64 = 3;

fn pid_in(f: &std::path::Path) -> Option<u32> {
    std::fs::read_to_string(f).ok()?.trim().parse().ok()
}

fn kill_pidfile(f: &std::path::Path) {
    if let Some(p) = pid_in(f) {
        let _ = Command::new("kill")
            .arg(p.to_string())
            .stderr(Stdio::null())
            .status();
    }
}

struct KillOnDrop(Vec<std::path::PathBuf>);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        for f in &self.0 {
            kill_pidfile(f);
        }
    }
}

#[test]
fn real_sh_survival_heal() {
    if !enabled("real_sh_survival_heal") {
        return;
    }
    let rel = release_bins(
        None,
        &[
            "-p",
            "guardian-tick",
            "--bins",
            "--example",
            "dummy_service",
        ],
        &[],
    );
    let gt = need(&rel.join("guardian-tick"));
    let dummy = need(&rel.join("examples/dummy_service"));
    require_free(&[HEALTHY, DIES]);
    let run_dir = Scratch::new("survival-heal-real");
    let hp = run_dir.path("healthy.pid");
    let dp = run_dir.path("dies.pid");
    let _guard = KillOnDrop(vec![hp.clone(), dp.clone()]);
    let mut ck = Checks::default();

    let dummy_argv = |port: u16, die: u64, pidfile: &std::path::Path| -> Vec<String> {
        let mut v = vec![
            "/usr/bin/setsid".to_string(),
            "-f".into(),
            dummy.display().to_string(),
            "--port".into(),
            port.to_string(),
            "--pidfile".into(),
            pidfile.display().to_string(),
        ];
        if die > 0 {
            v.extend(["--die-after-s".into(), die.to_string()]);
        }
        v
    };
    let restart = dummy_argv(DIES, DIE_S, &dp);
    let toml_argv = restart
        .iter()
        .map(|a| format!("{a:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let probes = run_dir.put(
        "probes.toml",
        &format!(
            "[[probe]]\nkind = \"http\"\nname = \"dummy-healthy\"\nport = {HEALTHY}\npath = \"/health\"\n\n\
             [[probe]]\nkind = \"http\"\nname = \"dummy-dies\"\nport = {DIES}\npath = \"/health\"\n\n\
             [[repair]]\nprobe = \"dummy-healthy\"\nunit = \"dummy-healthy\"\nafter_fails = 1\nbackend = \"none\"\n\n\
             [[repair]]\nprobe = \"dummy-dies\"\nunit = \"dummy-dies\"\nafter_fails = 1\nbackend = \"command\"\n\
             command = [{toml_argv}]\nverify_after_ms = 700\n"
        ),
    );
    say(format!("start dummies (run dir {})", run_dir.0.display()));
    for (port, die, pf) in [(HEALTHY, 0, &hp), (DIES, DIE_S, &dp)] {
        let a = dummy_argv(port, die, pf);
        // Null stdio: `setsid -f` leaves the dummy holding any inherited pipe.
        let st = Command::new(&a[0])
            .args(&a[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(st.success());
    }
    ck.ok(
        "both dummies listening",
        wait_port(HEALTHY, Duration::from_secs(5)) && wait_port(DIES, Duration::from_secs(5)),
        "",
    );
    let state = run_dir.path("state");
    let tick = |allow: bool| -> String {
        let mut c = Command::new(&gt);
        c.arg("heal")
            .arg("--probes")
            .arg(&probes)
            .arg("--state")
            .arg(&state);
        if allow {
            c.args(["--allow", "dummy-dies"]);
        }
        let o = run(&mut c);
        assert!(o.status.success(), "heal tick failed: {}", err(&o));
        print!("{}", out(&o));
        out(&o)
    };

    say("tick 0: both up (dry-run)");
    let t0 = tick(false);
    ck.ok(
        "tick 0 dry-run, both healthy",
        t0.contains("mode=dry-run") && t0.matches("healthy  status 200").count() == 2,
        "",
    );
    std::thread::sleep(Duration::from_secs(DIE_S + 1));
    say("tick 1: dummy-dies has died -> detection + plan, DRY-RUN (no --allow)");
    let first = pid_in(&dp);
    let t1 = tick(false);
    ck.ok(
        "death detected",
        t1.contains("dummy-dies             down"),
        "",
    );
    ck.ok(
        "dry-run does not act",
        t1.contains("would restart") && !port_open(DIES) && pid_in(&dp) == first,
        "",
    );
    let mut pids = vec![first];
    for n in 1..=3 {
        say(format!("tick apply-{n}: --allow dummy-dies"));
        let t = tick(true);
        let p = pid_in(&dp);
        ck.ok(
            &format!("apply-{n} restarted a new process, verified healthy"),
            t.contains("result=restarted via command verify_ok=true") && !pids.contains(&p),
            &format!("pid {p:?}"),
        );
        pids.push(p);
        std::thread::sleep(Duration::from_secs(DIE_S + 1));
    }
    say("tick apply-4: --allow dummy-dies (expect cap-reached, no restart)");
    let before = pid_in(&dp);
    let t4 = tick(true);
    ck.ok("apply-4 cap reached", t4.contains("cap-reached"), "");
    ck.ok(
        "no fourth restart",
        pid_in(&dp) == before && !port_open(DIES),
        "",
    );
    ck.ok("healthy dummy untouched", port_open(HEALTHY), "");
    let store = std::fs::read_to_string(state.join("incidents.jsonl")).unwrap_or_default();
    let nerve = std::fs::read_to_string(state.join("nerve.jsonl")).unwrap_or_default();
    say(format!(
        "incident store: {} lines; nerve: {} lines",
        store.lines().count(),
        nerve.lines().count()
    ));
    print!("{store}{nerve}");
    ck.ok(
        "incident store has cap-hit",
        store.contains("\"kind\":\"cap-hit\""),
        "",
    );
    ck.ok("nerve has heal-cap-hit", nerve.contains("heal-cap-hit"), "");
    ck.ok(
        "Zagreb stamps, no UTC Z",
        store.contains("+02:00") || store.contains("+01:00"),
        "",
    );
    ck.ok(
        "no UTC Z stamps",
        !store.contains("Z\"") && !nerve.contains("Z\""),
        "",
    );

    let capture = repo_root().join("docs/real-tests/captures/vps-8660-status-20261020.json");
    let capture = std::env::var_os("CAPTURE")
        .map(Into::into)
        .unwrap_or(capture);
    if capture.exists() {
        say(format!(
            "classify over captured live status.json: {}",
            capture.display()
        ));
        let o = run(Command::new(&gt)
            .arg("classify")
            .arg("--status-json")
            .arg(&capture));
        print!("{}", out(&o));
        ck.ok(
            "classify over live capture",
            o.status.success() && !out(&o).is_empty(),
            "",
        );
    }
    kill_pidfile(&hp);
    kill_pidfile(&dp);
    ck.ok(
        "teardown",
        wait_until(Duration::from_secs(3), || {
            !port_open(HEALTHY) && !port_open(DIES)
        }),
        "both ports closed",
    );
    ck.finish("real_sh_survival_heal");
}
