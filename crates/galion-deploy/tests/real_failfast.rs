// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Rehearsal numbers for the opt-in crash-loop fail-fast (`GALION_DEPLOY_FAILFAST=N`),
//! on the BOX, gated by GALION_REAL=1. Same harness as tests/real_rehearsal.rs
//! (simsup plays systemd, the release galion-deploy runs with the direct backend,
//! `simsup probe` measures the outage), on wave-4 slot 121 ports: 19910 (tuner),
//! 19911 (galion-nerve). Nothing touches the VPS; every process is stopped at the end.
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p galion-deploy --test real_failfast -- --nocapture --test-threads 2
//! ```
//!
//! simsup's `ctl show` has `NRestarts` but no `SubState`, so here a crash counts when
//! simsup restarts the unit (RestartSec after the crash). Under systemd the pending
//! `SubState=auto-restart` counts it at once, so these numbers are an upper bound.

mod support;

use std::fs;
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::*;

const TUNER: u16 = 19910;
const NERVE: u16 = 19911;

fn gate(name: &str) -> bool {
    if !enabled() {
        eprintln!("SKIP {name}: set GALION_REAL=1");
        return false;
    }
    for b in [
        "simsup",
        "sim-broken",
        "sim-hang",
        "tuner-old",
        "tuner-new",
        "nerve-old",
    ] {
        assert!(
            sim_bin().join(b).exists(),
            "missing {}: run GALION_REAL=1 GALION_REAL_BUILD=1 cargo test --release -p galion-deploy --test real_build",
            sim_bin().join(b).display()
        );
    }
    true
}

/// One L1 run: deploy `file`, expect exit 3 and the old binary healthy again.
/// Returns (exit, deploy ms, outage ms, crash-loop line seen).
fn l1(
    r: &mut Rehearsal,
    unit: &str,
    target: &Path,
    old: &str,
    file: &Path,
    h: &str,
    ff: Option<&str>,
) -> (i32, u128, u64, bool) {
    let path = format!("/{}", h.splitn(4, '/').nth(3).unwrap_or(""));
    let extra: Vec<(&str, &str)> = ff
        .map(|n| vec![("GALION_DEPLOY_FAILFAST", n)])
        .unwrap_or_default();
    let p = r.probe_start(&path, 5);
    let d = r.deploy(120, &[unit, file.to_str().unwrap(), h], &extra);
    let down = r.probe_stop(p);
    let st = r.active(unit);
    let ff_hit = d.out.contains("crash loop:");
    let label = format!(
        "{unit} L1 {} FAILFAST={}",
        file.file_name().unwrap().to_string_lossy(),
        ff.unwrap_or("off")
    );
    r.check(
        &format!("{label}: exit 3, old sha, unit active, healthy"),
        d.rc == 3 && sha16(target) == old && st == "active" && code(h) == 200,
        &format!(
            "exit {}, deploy {} ms, outage {down} ms, crash-loop line {ff_hit}",
            d.rc, d.ms
        ),
    );
    r.downtime(&label, down);
    // let the unit settle and empty its start-limit window between runs
    r.ctl(&["reset-failed", unit]);
    assert!(wait_code(h, 200, 15), "{unit} healthy again after {label}");
    (d.rc, d.ms, down, ff_hit)
}

#[test]
fn failfast_tuner() {
    if !gate("failfast tuner") {
        return;
    }
    let mut r = Rehearsal::new("ff-tuner", TUNER);
    let h = r.url("/health");
    let t = r.root.join("opt/galion/tuner/bin/tuner");
    let www = r.root.join("opt/galion/tuner/www");
    fs::create_dir_all(t.parent().unwrap()).unwrap();
    fs::create_dir_all(&www).unwrap();
    install(&r.bin.join("tuner-old"), &t);
    for f in ["index.html", "app.js", "pitch.js", "style.css"] {
        fs::copy(repo().join("crates/tuner/www").join(f), www.join(f)).unwrap();
    }
    r.denv = vec![
        ("TUNER_ADDR".into(), format!("127.0.0.1:{TUNER}")),
        ("TUNER_ROOT".into(), www.display().to_string()),
    ];
    // Restart=, RestartSec= and the start limit as on the VPS (read-only
    // `systemctl show tuner.service`, 2026-10-21 ~04:08 +02:00: always, 2s, 10s/5).
    fs::write(
        r.root.join("units/tuner.service"),
        format!(
            "[Unit]\nDescription=tuner (sim)\nStartLimitIntervalSec=10\nStartLimitBurst=5\n[Service]\nExecStart={}\nEnvironment=TUNER_ADDR=127.0.0.1:{TUNER}\nEnvironment=TUNER_ROOT={}\nRestart=always\nRestartSec=2\n",
            t.display(),
            www.display()
        ),
    )
    .unwrap();
    r.sup_start();
    r.check(
        "old tuner serving",
        wait_code(&h, 200, 5),
        &code(&h).to_string(),
    );
    let old = sha16(&r.bin.join("tuner-old"));
    let stage = r.stage();
    let broken = r.broken();
    let truncated = stage.join("tuner-truncated");
    fs::write(
        &truncated,
        &fs::read(r.bin.join("tuner-new")).unwrap()[..65536],
    )
    .unwrap();
    install(&truncated.clone(), &stage.join("trunc.tmp"));
    fs::rename(stage.join("trunc.tmp"), &truncated).unwrap();
    let hang = r.hang();

    r.say("L1 matrix: broken (exits 1 at start), truncated ELF (exec fails), hang (alive, never binds)");
    let mut rows = Vec::new();
    for (file, ff) in [
        (&broken, None),
        (&broken, Some("1")),
        (&broken, Some("2")),
        (&broken, Some("3")),
        (&truncated, None),
        (&truncated, Some("2")),
        (&hang, Some("2")),
    ] {
        let row = l1(&mut r, "tuner", &t, &old, file, &h, ff);
        rows.push((
            file.file_name().unwrap().to_string_lossy().to_string(),
            ff.unwrap_or("off"),
            row,
        ));
    }
    let off = rows[0].2 .2;
    let ff2 = rows[2].2 .2;
    r.check(
        "fail-fast 2 cuts the broken-build outage to under half of the 20 s window",
        ff2 * 2 < off && rows[2].2 .3,
        &format!("off {off} ms, FAILFAST=2 {ff2} ms"),
    );
    r.check(
        "hang build: no crash, so fail-fast cannot help (full window, no crash-loop line)",
        !rows[6].2 .3 && rows[6].2 .2 > 19_000,
        &format!("{} ms", rows[6].2 .2),
    );

    r.say("healthy deploy with FAILFAST=1: no false alarm");
    let new = r.stage().join("tuner-new");
    install(&r.bin.join("tuner-new"), &new);
    let p = r.probe_start("/health", 2);
    let d = r.deploy(
        60,
        &["tuner", new.to_str().unwrap(), &h],
        &[("GALION_DEPLOY_FAILFAST", "1")],
    );
    let down = r.probe_stop(p);
    r.check(
        "healthy FAILFAST=1: exit 0, new sha, no crash-loop line, armed line logged",
        d.rc == 0
            && sha16(&t) == sha16(&new)
            && !d.out.contains("crash loop:")
            && d.out.contains("fail-fast armed"),
        &format!("exit {}, {} ms, outage {down} ms", d.rc, d.ms),
    );
    r.downtime("tuner swap FAILFAST=1", down);
    // back to the old binary for the transient case
    install(&r.bin.join("tuner-old"), &t);
    r.ctl(&["restart", "tuner"]);
    assert!(wait_code(&h, 200, 10));

    r.say("transient: the first start of a GOOD build fails (port still taken), the automatic restart 2 s later succeeds");
    let mut transient = Vec::new();
    for ff in ["1", "2"] {
        r.ctl(&["stop", "tuner"]);
        r.ctl(&["reset-failed", "tuner"]);
        let blocker = TcpListener::bind(("127.0.0.1", TUNER)).expect("blocker bind");
        let journal = r.root.join("journal/tuner.log");
        let before = fs::read_to_string(&journal)
            .unwrap_or_default()
            .matches("Failed with result")
            .count();
        let done = Arc::new(AtomicBool::new(false));
        let d2 = done.clone();
        // release the port as soon as the new build's first start has failed
        let th = std::thread::spawn(move || {
            let t0 = Instant::now();
            while !d2.load(Ordering::SeqCst) && t0.elapsed() < Duration::from_secs(60) {
                let n = fs::read_to_string(&journal)
                    .unwrap_or_default()
                    .matches("Failed with result")
                    .count();
                if n > before {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            drop(blocker);
        });
        let d = r.deploy(
            90,
            &["tuner", new.to_str().unwrap(), &h],
            &[("GALION_DEPLOY_FAILFAST", ff), ("SKIP_DATA_BACKUP", "1")],
        );
        done.store(true, Ordering::SeqCst);
        th.join().unwrap();
        let hit = d.out.contains("crash loop:");
        r.line(&format!(
            "transient FAILFAST={ff}: exit {}, {} ms, crash-loop line {hit}",
            d.rc, d.ms
        ));
        transient.push((ff, d.rc, d.ms, hit));
        install(&r.bin.join("tuner-old"), &t);
        r.ctl(&["reset-failed", "tuner"]);
        r.ctl(&["restart", "tuner"]);
        assert!(wait_code(&h, 200, 10));
    }
    r.check(
        "transient FAILFAST=2: the good build is kept (exit 0)",
        transient[1].1 == 0 && !transient[1].3,
        &format!("exit {}, {} ms", transient[1].1, transient[1].2),
    );
    r.line(&format!(
        "FINDING transient FAILFAST=1: exit {} (3 = a good build was rolled back after one crash), crash-loop line {}",
        transient[0].1, transient[0].3
    ));

    r.say("table");
    for (f, ff, (rc, ms, down, hit)) in &rows {
        r.line(&format!(
            "ROW tuner {f} FAILFAST={ff}: exit {rc}, deploy {ms} ms, outage {down} ms, crash-loop {hit}"
        ));
    }
    let (_, st) = r.ctl(&["status"]);
    r.line(&st);
    r.sup_stop();
    assert_eq!(r.summary(), 0, "fail-fast tuner rehearsal had failures");
}

#[test]
fn failfast_nerve() {
    if !gate("failfast nerve") {
        return;
    }
    let mut r = Rehearsal::new("ff-nerve", NERVE);
    let h = r.url("/healthz");
    let t = r.root.join("opt/galion/rust/bin/galion-nerve");
    let data = r.root.join("var/lib/galion-nerve");
    fs::create_dir_all(&data).unwrap();
    install(&r.bin.join("nerve-old"), &t);
    r.denv = vec![
        ("NERVE_ADDR".into(), format!("127.0.0.1:{NERVE}")),
        ("NERVE_DATA_DIR".into(), data.display().to_string()),
    ];
    // RestartSec=10 and the 600 s / 5 start limit of deploy/nerve/galion-nerve.service
    // (a63b730), same as the live unit (`systemctl show`: always, 10s, 10min/5).
    fs::write(
        r.root.join("units/galion-nerve.service"),
        format!(
            "[Unit]\nDescription=galion-nerve (sim)\nStartLimitIntervalSec=600\nStartLimitBurst=5\n[Service]\nExecStart={} serve\nEnvironment=NERVE_ADDR=127.0.0.1:{NERVE}\nEnvironment=NERVE_DATA_DIR={}\nEnvironment=NERVE_DRY_RUN=1\nEnvironment=NERVE_INTERVAL=5\nEnvironment=NERVE_SYSTEMD=0 NERVE_JOURNAL=0 NERVE_REFLEX=0 NERVE_ROLLBACK=0 NERVE_GALION_HEALTH_PORT=0 NERVE_DEPLOY_BIN=/nonexistent/galion-deploy NERVE_LISTEN_SCOPE={FF_LO}-{FF_HI}\nRestart=always\nRestartSec=10\n",
            t.display(),
            data.display()
        ),
    )
    .unwrap();
    r.sup_start();
    r.check(
        "old nerve serving /healthz",
        wait_code(&h, 200, 10),
        &code(&h).to_string(),
    );
    let old = sha16(&r.bin.join("nerve-old"));
    let broken = r.broken();
    let mut rows = Vec::new();
    for ff in [None, Some("1"), Some("2")] {
        let row = l1(&mut r, "galion-nerve", &t, &old, &broken, &h, ff);
        rows.push((ff.unwrap_or("off"), row));
    }
    r.check(
        "nerve FAILFAST=1 beats the 20 s window (RestartSec=10 bounds it on simsup)",
        rows[1].1 .2 < rows[0].1 .2 && rows[1].1 .3,
        &format!("off {} ms, FAILFAST=1 {} ms", rows[0].1 .2, rows[1].1 .2),
    );
    r.say("table");
    for (ff, (rc, ms, down, hit)) in &rows {
        r.line(&format!(
            "ROW galion-nerve sim-broken FAILFAST={ff}: exit {rc}, deploy {ms} ms, outage {down} ms, crash-loop {hit}"
        ));
    }
    r.sup_stop();
    assert_eq!(r.summary(), 0, "fail-fast nerve rehearsal had failures");
}
