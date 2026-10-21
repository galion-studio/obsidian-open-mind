// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Cutover rehearsals on the BOX, gated by GALION_REAL=1 (Rust port of
//! scripts/real/deploy-sim.sh, which this branch deletes). simsup plays systemd, the
//! release galion-deploy runs with the direct backend, `simsup probe` measures
//! downtime. Ports 19390-19393 (slot 39; 19399 is a deliberately dead port for the
//! desk's upstream probes). Nothing touches the VPS; every process started is
//! stopped at the end.
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p galion-deploy --test real_rehearsal -- --nocapture --test-threads 4
//! ```
//!
//! The checks that used to prove a finding (F1 hang, F2 start-limit trap, F3 exit 1,
//! F4 /health, F5 no first install, F6 interrupt) now prove the fix.

mod support;

use galion_deploy::journal;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use support::*;

const TUNER: u16 = 19390;
const NERVE: u16 = 19391;
const ROOM: u16 = 19392;
const DESK: u16 = 19393;
const DEAD: u16 = 19399;

fn gate(name: &str) -> bool {
    if !enabled() {
        eprintln!(
            "SKIP {name}: set GALION_REAL=1 (binaries in {})",
            sim_bin().display()
        );
        return false;
    }
    for b in [
        "simsup",
        "sim-broken",
        "sim-hang",
        "tuner-old",
        "nerve-old",
        "nerve-new",
        "room-new",
        "room-old",
        "desk-old",
        "desk-new",
    ] {
        assert!(sim_bin().join(b).exists(), "missing {}: run GALION_REAL=1 GALION_REAL_BUILD=1 cargo test --release -p galion-deploy --test real_build", sim_bin().join(b).display());
    }
    true
}

/// The route matrix lines are printf formats (scripts/real/tuner-requests.txt); the
/// live fixtures were captured through bash `printf "$fmt"`, which turns `\r\n` into
/// CRLF and (quirk) `%2e` into `0.000000e+00`. Reproduce exactly that.
fn printf_bytes(fmt: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let b = fmt.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' if i + 1 < b.len() => {
                out.push(match b[i + 1] {
                    b'r' => b'\r',
                    b'n' => b'\n',
                    b't' => b'\t',
                    o => o,
                });
                i += 2;
            }
            b'%' if fmt[i..].starts_with("%%") => {
                out.push(b'%');
                i += 2;
            }
            b'%' if fmt[i..].starts_with("%2e") => {
                out.extend_from_slice(b"0.000000e+00");
                i += 3;
            }
            b'%' => panic!("unsupported printf conversion in {fmt}"),
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn tuner_routes(r: &mut Rehearsal) -> (usize, usize) {
    let text = fs::read_to_string(repo().join("scripts/real/tuner-requests.txt")).unwrap();
    let fx = repo().join("crates/tuner/tests/fixtures/live");
    let (mut same, mut tot) = (0, 0);
    for l in text.lines() {
        if l.starts_with('#') || l.trim().is_empty() {
            continue;
        }
        let (name, fmt) = l.split_once(char::is_whitespace).unwrap();
        let fmt = fmt.trim_start();
        assert!(fmt.starts_with("GET"), "GET only");
        let got = raw_request(
            &format!("127.0.0.1:{TUNER}"),
            &printf_bytes(fmt),
            Duration::from_secs(5),
        );
        tot += 1;
        if fs::read(fx.join(format!("{name}.http"))).ok().as_deref() == Some(&got[..]) {
            same += 1;
        } else {
            r.line(&format!("  route DIFF: {name}"));
        }
    }
    (same, tot)
}

fn verify_sums(dir: &Path, file: &str) -> Option<String> {
    let sums = fs::read_to_string(dir.join("SHA256SUMS")).ok()?;
    let want = sums.lines().find_map(|l| {
        let (h, n) = l.split_once(char::is_whitespace)?;
        (n.trim().trim_start_matches('*') == file).then(|| h.to_string())
    })?;
    let got = galion_deploy::sha256::file_hex(&dir.join(file)).ok()?;
    (got == want).then_some(got)
}

/// Deploy in the background, wait until `installed` is on disk, give the health loop
/// `after` more, then send `sig`.
fn interrupt(
    r: &mut Rehearsal,
    unit: &str,
    new: &Path,
    h: &str,
    sig: &str,
    after: Duration,
) -> (support::DeployResult, bool) {
    let target_sha = sha16(new);
    let run = r.spawn_deploy(90, &[unit, new.to_str().unwrap(), h], &[]);
    let pid = run.pid();
    let target = r.root.join(target_path(unit));
    let t = Instant::now();
    let mut seen = false;
    while t.elapsed() < Duration::from_secs(30) {
        if sha16(&target) == target_sha {
            seen = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(after);
    r.line(&format!(
        "  (new binary on disk: {seen}; sending SIG{sig} to galion-deploy pid {pid})"
    ));
    signal(pid, sig);
    (r.finish(run), seen)
}

fn target_path(unit: &str) -> &'static str {
    match unit {
        "tuner" => "opt/galion/tuner/bin/tuner",
        "galion-nerve" => "opt/galion/rust/bin/galion-nerve",
        "room" => "opt/galion/rust/bin/room",
        "galion-desk" => "opt/galion/bin/galion-desk",
        _ => panic!("unit"),
    }
}

// ------------------------------------------------------------------ tuner
#[test]
fn rehearsal_tuner() {
    if !gate("tuner") {
        return;
    }
    let mut r = Rehearsal::new("tuner", TUNER);
    let h = r.url("/health");
    let t = r.root.join(target_path("tuner"));
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
    fs::write(
        r.root.join("units/tuner.service"),
        format!(
            "[Unit]\nDescription=tuner (sim copy of /etc/systemd/system/tuner.service)\n[Service]\nExecStart={}\nEnvironment=TUNER_ADDR=127.0.0.1:{TUNER}\nEnvironment=TUNER_ROOT={}\nRestart=always\nRestartSec=2\nNice=10\nNoNewPrivileges=true\n[Install]\nWantedBy=multi-user.target\n",
            t.display(),
            www.display()
        ),
    )
    .unwrap();
    r.sup_start();
    let ok = wait_code(&h, 200, 5);
    r.check(
        "old tuner serving before cutover",
        ok,
        &format!("{} on {h}", code(&h)),
    );
    let old = sha16(&r.bin.join("tuner-old"));

    r.say("3.1 copy the artefact into the stage dir and verify SHA256SUMS");
    let art = PathBuf::from("/workspace/swarm/artifacts/tuner");
    let stage = r.stage();
    for f in ["tuner-x86_64-unknown-linux-musl", "SHA256SUMS"] {
        fs::copy(art.join(f), stage.join(f)).unwrap();
    }
    let n = stage.join("tuner-x86_64-unknown-linux-musl");
    let v = verify_sums(&stage, "tuner-x86_64-unknown-linux-musl");
    install(&n.clone(), &stage.join("tuner.tmp"));
    fs::rename(stage.join("tuner.tmp"), &n).unwrap();
    let new = sha16(&n);
    r.check(
        "3.1 artefact sha256 verified",
        v.is_some(),
        &format!("sha {new}"),
    );

    r.say("3.2 preflight (read-only, no bind)");
    let ver = std::process::Command::new(&n)
        .arg("--version")
        .output()
        .unwrap();
    let ver = String::from_utf8_lossy(&ver.stdout).trim().to_string();
    r.check(
        "3.2 --version answers without binding",
        ver == "tuner 0.1.0",
        &ver,
    );
    let chk = std::process::Command::new(&n)
        .arg("--check")
        .env("TUNER_ROOT", &www)
        .output()
        .unwrap();
    r.check(
        "3.2 --check",
        chk.status.success(),
        &String::from_utf8_lossy(&chk.stdout)
            .lines()
            .count()
            .to_string(),
    );

    r.say("3.3 swap: galion-deploy tuner <new> <health> (probe every 2 ms)");
    let p = r.probe_start("/health", 2);
    let d = r.deploy(60, &["tuner", n.to_str().unwrap(), &h], &[]);
    let down = r.probe_stop(p);
    r.check("3.3 deploy exit 0", d.rc == 0, &format!("exit {}", d.rc));
    r.check(
        "3.3 downtime < 1 s",
        down < 1000,
        &format!("longest outage {down} ms"),
    );
    r.downtime("swap", down);
    let bak = r.latest_backup("tuner").expect("backup");
    r.line(&format!("backup of the old binary: {}", bak.display()));

    r.say("3.4 health after the switch");
    let pid = r.main_pid("tuner");
    let run_sha = sha16(Path::new(&format!("/proc/{pid}/exe")));
    r.check(
        "3.4 running binary is the new sha",
        run_sha == new,
        &run_sha,
    );
    let (c1, body) = get(&r.url("/"));
    let c2 = code(&r.url("/nope"));
    r.check(
        "3.4 / is 200 2356 and /nope is 404",
        c1 == 200 && body.len() == 2356 && c2 == 404,
        &format!("{c1} {} / {c2}", body.len()),
    );
    let (same, tot) = tuner_routes(&mut r);
    r.check(
        "3.4 all routes byte-identical to the live fixtures",
        same == tot && tot > 0,
        &format!("{same}/{tot}"),
    );

    r.say("rollback level 2 (manual): galion-deploy tuner <backup> <health>");
    let p = r.probe_start("/health", 2);
    let d = r.deploy(60, &["tuner", bak.to_str().unwrap(), &h], &[]);
    let down = r.probe_stop(p);
    r.check(
        "L2 manual rollback exit 0 and old sha back",
        d.rc == 0 && sha16(&t) == old,
        &format!("exit {}, sha {}, outage {down} ms", d.rc, sha16(&t)),
    );
    r.downtime("rollback-L2", down);

    r.say("forward again, then rollback level 3 (last resort, no galion-deploy)");
    let d = r.deploy(60, &["tuner", n.to_str().unwrap(), &h], &[]);
    r.check("re-deploy new exit 0", d.rc == 0, &format!("exit {}", d.rc));
    let p = r.probe_start("/health", 2);
    let t3 = Instant::now();
    install(&bak, &r.root.join("opt/galion/tuner/bin/tuner.new"));
    fs::rename(r.root.join("opt/galion/tuner/bin/tuner.new"), &t).unwrap();
    r.x_ctl(&["restart", "tuner"]);
    wait_code(&h, 200, 5);
    let l3 = ms_since(t3);
    let down = r.probe_stop(p);
    r.check(
        "L3 last resort: old sha back and healthy",
        sha16(&t) == old && code(&h) == 200,
        &format!("sha {}, {l3} ms for install+mv+restart", sha16(&t)),
    );
    r.downtime("rollback-L3", down);

    // F2: no manual reset-failed before these runs any more. The default
    // reset-failed (and the retry) must make every rollback restart succeed.
    let truncated = stage.join("tuner-truncated");
    fs::write(&truncated, &fs::read(&n).unwrap()[..65536]).unwrap();
    install(&truncated.clone(), &stage.join("trunc.tmp"));
    fs::rename(stage.join("trunc.tmp"), &truncated).unwrap();
    let l1 = |r: &mut Rehearsal, label: &str, file: &Path| {
        let before = r.journal_count("tuner", "start-limit-hit");
        let p = r.probe_start("/health", 5);
        let d = r.deploy(90, &["tuner", file.to_str().unwrap(), &h], &[]);
        let down = r.probe_stop(p);
        let hits = r.journal_count("tuner", "start-limit-hit") - before;
        let st = r.active("tuner");
        r.check(
            &format!("{label}: exit 3 (rolled back ok), old sha, unit active, healthy, no manual recovery"),
            d.rc == 3 && sha16(&t) == old && st == "active" && code(&h) == 200,
            &format!("exit {}, unit {st}, start-limit hits during the run {hits}, outage {down} ms", d.rc),
        );
        r.downtime(label, down);
    };
    r.say("rollback level 1 (automatic): broken binary (valid ELF, exits 1 at start)");
    let broken = r.broken();
    l1(&mut r, "L1 broken binary", &broken);
    r.say("rollback level 1 (automatic): truncated ELF, then 3 more runs back to back (start-limit pressure)");
    for i in 0..4 {
        let label = if i == 0 {
            "L1 truncated ELF".to_string()
        } else {
            format!("L1 truncated ELF run {}", i + 1)
        };
        l1(&mut r, &label, &truncated);
    }

    r.say("hang A (F1): sim-hang never answers --version; galion-deploy as shipped (default 5 s bound)");
    let hang = r.hang();
    let p = r.probe_start("/health", 5);
    let d = r.deploy(90, &["tuner", hang.to_str().unwrap(), &h], &[]);
    let down = r.probe_stop(p);
    let orphan = vpids(&hang);
    r.check(
        "hang A: version step bounded, install, health fails, auto rollback (exit 3), no orphan",
        d.rc == 3
            && sha16(&t) == old
            && code(&h) == 200
            && orphan.is_empty()
            && d.out.contains("--version did not exit within"),
        &format!(
            "exit {}, {} ms, orphans {orphan:?}, outage {down} ms",
            d.rc, d.ms
        ),
    );
    r.downtime("auto-rollback-hang", down);

    r.say("hang C (F1): new tuner installed but DOWN; manual rollback (L2) to the OLD binary, which ignores --version and serves");
    let d = r.deploy(60, &["tuner", n.to_str().unwrap(), &h], &[]);
    r.line(&format!("forward first: exit {}", d.rc));
    let bak_old = r
        .backups_of("tuner")
        .into_iter()
        .filter(|b| sha16(b) == old)
        .max_by_key(|b| fs::metadata(b).unwrap().modified().unwrap())
        .unwrap();
    r.x_ctl(&["stop", "tuner"]);
    let run = r.spawn_deploy(60, &["tuner", bak_old.to_str().unwrap(), &h], &[]);
    std::thread::sleep(Duration::from_secs(2));
    let during = vpids(&bak_old);
    let dcode = code(&h);
    let ust = r.active("tuner");
    r.line(&format!(
        "2 s in: unit {}; GET {h} -> {dcode} from '{} --version' (pids {during:?})",
        ust,
        bak_old.display()
    ));
    let d = r.finish(run);
    let orphan = vpids(&bak_old);
    r.check(
        "hang C: completes on its own (exit 0), old sha, healthy, the stray --version was killed",
        d.rc == 0 && sha16(&t) == old && code(&h) == 200 && orphan.is_empty(),
        &format!("exit {}, {} ms (was: hung until the outer timeout, 124), stray --version pids during {during:?}, after {orphan:?}", d.rc, d.ms),
    );

    r.say("F6: SIGTERM / SIGHUP / SIGINT in the health window of a broken deploy");
    for sig in ["TERM", "HUP", "INT"] {
        let (d, seen) = interrupt(
            &mut r,
            "tuner",
            &broken,
            &h,
            sig,
            Duration::from_millis(1500),
        );
        r.check(
            &format!("F6 SIG{sig} after install: exit 5, old sha back, healthy, journal gone"),
            seen && d.rc == 5
                && sha16(&t) == old
                && code(&h) == 200
                && !journal::path(&r.bdir(), "tuner").exists(),
            &format!("exit {} after {} ms", d.rc, d.ms),
        );
    }

    r.say("F6: SIGKILL (uncatchable) in the health window; the journal survives; the lock refuses a second deploy; --recover restores");
    let run = r.spawn_deploy(90, &["tuner", broken.to_str().unwrap(), &h], &[]);
    let pid = run.pid();
    let t0 = Instant::now();
    while sha16(&t) != sha16(&broken) && t0.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(500));
    let second = r.deploy(30, &["tuner", n.to_str().unwrap(), &h], &[]);
    signal(pid, "KILL");
    let killed = r.finish(run);
    let j = fs::read_to_string(journal::path(&r.bdir(), "tuner")).unwrap_or_default();
    r.line(&format!("journal after SIGKILL:\n{j}"));
    let (busy_ok, journal_ok) = (
        second.rc == 1 && second.out.contains("another galion-deploy"),
        j.contains("phase=verify"),
    );
    let sha_after_kill = sha16(&t);
    let rec = r.deploy(60, &["--recover", "tuner"], &[]);
    r.check(
        "F6 SIGKILL: lock held (2nd deploy exit 1), journal phase=verify kept, --recover exit 3, old sha, healthy",
        busy_ok && journal_ok && killed.rc == 137 && sha_after_kill == sha16(&broken) && rec.rc == 3 && sha16(&t) == old && code(&h) == 200,
        &format!("2nd deploy exit {}, killed exit {}, sha after kill {sha_after_kill}, --recover exit {}", second.rc, killed.rc, rec.rc),
    );

    r.say("end state");
    let (_, st) = r.ctl(&["status"]);
    r.line(&st);
    let kept = r.backups_of("tuner").len();
    let shas: std::collections::BTreeSet<String> =
        r.backups_of("tuner").iter().map(|b| sha16(b)).collect();
    let both = shas.contains(&old) && shas.contains(&new);
    let codes: std::collections::BTreeSet<i32> = r.codes.iter().copied().collect();
    r.check(
        "F3: distinct exit codes seen (0 deployed, 1 refused, 3 rolled back, 5 interrupted); pruning kept <= 5 versions incl. old and new",
        [0, 1, 3, 5].iter().all(|c| codes.contains(c)) && both && shas.len() <= 5,
        &format!("codes {codes:?}, {kept} backup files, {} distinct", shas.len()),
    );
    r.sup_stop();
    assert_eq!(r.summary(), 0, "tuner rehearsal had failures");
}

// ------------------------------------------------------------------ nerve
fn nerve_unit(r: &Rehearsal, t: &Path, data: &Path) -> String {
    let src = std::process::Command::new("git")
        .arg("-C")
        .arg(repo())
        .args(["show", "a63b730:deploy/nerve/galion-nerve.service"])
        .output()
        .unwrap();
    assert!(src.status.success(), "git show a63b730 unit");
    let text = String::from_utf8_lossy(&src.stdout).to_string();
    let mut out = String::new();
    for l in text.lines() {
        let l = match l {
            "ExecStart=/opt/galion/rust/bin/galion-nerve serve" => {
                format!("ExecStart={} serve", t.display())
            }
            "Environment=NERVE_ADDR=127.0.0.1:8670" => {
                format!("Environment=NERVE_ADDR=127.0.0.1:{}", r.port)
            }
            "Environment=NERVE_DATA_DIR=/var/lib/galion-nerve" => {
                format!("Environment=NERVE_DATA_DIR={}", data.display())
            }
            "Environment=NERVE_DRY_RUN=0" => "Environment=NERVE_DRY_RUN=1".into(),
            "Environment=NERVE_INTERVAL=30" => "Environment=NERVE_INTERVAL=5".into(),
            o => o.to_string(),
        };
        out += &l;
        out.push('\n');
        if l.starts_with("Environment=NERVE_REPORT_EVERY") {
            out += &format!("Environment=NERVE_SYSTEMD=0 NERVE_JOURNAL=0 NERVE_REFLEX=0 NERVE_ROLLBACK=0 NERVE_GALION_HEALTH_PORT=0 NERVE_DEPLOY_BIN=/nonexistent/galion-deploy NERVE_LISTEN_SCOPE={SLOT_LO}-{SLOT_HI}\n");
        }
    }
    out
}

#[test]
fn rehearsal_nerve() {
    if !gate("nerve") {
        return;
    }
    let mut r = Rehearsal::new("nerve", NERVE);
    let h = r.url("/healthz");
    let t = r.root.join(target_path("galion-nerve"));
    let data = r.root.join("var/lib/galion-nerve");
    fs::create_dir_all(&data).unwrap();
    install(&r.bin.join("nerve-old"), &t);
    r.denv = vec![
        ("NERVE_ADDR".into(), format!("127.0.0.1:{NERVE}")),
        ("NERVE_DATA_DIR".into(), data.display().to_string()),
    ];
    let unit = nerve_unit(&r, &t, &data);
    for l in unit.lines().filter(|l| {
        l.starts_with("ExecStart")
            || l.starts_with("Environment")
            || l.starts_with("Restart")
            || l.starts_with("StartLimit")
    }) {
        r.line(&format!("  unit| {l}"));
    }
    fs::write(r.root.join("units/galion-nerve.service"), unit).unwrap();
    r.sup_start();
    let ok = wait_code(&h, 200, 10);
    r.check(
        "old galion-nerve serving /healthz",
        ok,
        &code(&h).to_string(),
    );
    let c = code(&r.url("/health"));
    r.check(
        "old nerve /health is 404 like live",
        c == 404,
        &c.to_string(),
    );
    r.line("letting the old nerve run 2 cycles so its data dir is real...");
    std::thread::sleep(Duration::from_secs(11));
    r.ctl(&["reset-failed", "galion-nerve"]);
    let old = sha16(&r.bin.join("nerve-old"));
    let new = sha16(&r.bin.join("nerve-new"));

    r.say("copy + verify (stand-in for scp to /var/tmp/galion-cutover)");
    let n = r.stage().join("openmind-nerve-x86_64-unknown-linux-musl");
    install(&r.bin.join("nerve-new"), &n);
    let full = galion_deploy::sha256::file_hex(&n).unwrap();
    r.check(
        "artefact sha256 verified (pinned reproducible sha)",
        full == "ad9a5b29d8ce71337c9945e10e8117700da6c4b3dd3d5aeb1f2f96e183bd106a",
        &full[..16],
    );

    r.say("preflight");
    let ver = std::process::Command::new(&n)
        .arg("--version")
        .output()
        .unwrap();
    let ver = String::from_utf8_lossy(&ver.stdout).trim().to_string();
    r.check(
        "new --version answers without binding",
        ver == "openmind-nerve 0.1.0",
        &ver,
    );

    r.say("swap (probe every 2 ms on /healthz)");
    let p = r.probe_start("/healthz", 2);
    let d = r.deploy(60, &["galion-nerve", n.to_str().unwrap(), &h], &[]);
    let down = r.probe_stop(p);
    r.check("swap exit 0", d.rc == 0, &format!("exit {}", d.rc));
    r.check(
        "swap downtime < 1 s",
        down < 1000,
        &format!("longest outage {down} ms"),
    );
    r.downtime("swap", down);
    let bak = r.latest_backup("galion-nerve").unwrap();
    let pid = r.main_pid("galion-nerve");
    let rs = sha16(Path::new(&format!("/proc/{pid}/exe")));
    r.check("running binary is the new sha", rs == new, &rs);
    std::thread::sleep(Duration::from_secs(6));
    let names: Vec<String> = fs::read_dir(&data)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    let st = r.active("galion-nerve");
    r.check(
        "new nerve reads the old nerve's data dir and keeps serving",
        code(&h) == 200 && st == "active",
        &format!("data: {names:?}"),
    );

    r.say("rollback level 2 (manual): galion-deploy galion-nerve <backup> /healthz");
    let p = r.probe_start("/healthz", 2);
    let d = r.deploy(60, &["galion-nerve", bak.to_str().unwrap(), &h], &[]);
    let down = r.probe_stop(p);
    std::thread::sleep(Duration::from_secs(6));
    r.check(
        "L2 manual rollback exit 0, old sha, old nerve healthy on the new nerve's data dir",
        d.rc == 0 && sha16(&t) == old && code(&h) == 200,
        &format!("exit {}, outage {down} ms", d.rc),
    );
    r.downtime("rollback-L2", down);

    r.say("forward again, then rollback level 3 (last resort)");
    let d = r.deploy(60, &["galion-nerve", n.to_str().unwrap(), &h], &[]);
    r.check("re-deploy new exit 0", d.rc == 0, &format!("exit {}", d.rc));
    let p = r.probe_start("/healthz", 2);
    install(&bak, &r.root.join("opt/galion/rust/bin/galion-nerve.new"));
    fs::rename(r.root.join("opt/galion/rust/bin/galion-nerve.new"), &t).unwrap();
    r.x_ctl(&["restart", "galion-nerve"]);
    wait_code(&h, 200, 10);
    let down = r.probe_stop(p);
    r.check(
        "L3 last resort: old sha, healthy",
        sha16(&t) == old && code(&h) == 200,
        &format!("outage {down} ms"),
    );
    r.downtime("rollback-L3", down);

    let broken = r.broken();
    r.say("F2: L1 broken binary as the 5th start in the 600 s window (swap, L2, re-deploy, L3 used 4); the 6th start used to be refused");
    let p = r.probe_start("/healthz", 5);
    let d = r.deploy(90, &["galion-nerve", broken.to_str().unwrap(), &h], &[]);
    let down = r.probe_stop(p);
    let st = r.active("galion-nerve");
    r.check(
        "F2 fixed: full start-limit window, rollback restart goes through (exit 3, old sha, unit active, healthy)",
        d.rc == 3 && sha16(&t) == old && st == "active" && code(&h) == 200,
        &format!("exit {}, unit {st}, outage {down} ms (was: exit 1, unit failed, down until manual reset-failed)", d.rc),
    );
    r.downtime("auto-rollback-full-window", down);

    r.say("the old behaviour on request (GALION_DEPLOY_RESET_FAILED=0) with a full window: trap reproduced, now reported as exit 4");
    r.ctl(&["reset-failed", "galion-nerve"]);
    for _ in 0..4 {
        r.ctl(&["restart", "galion-nerve"]);
    }
    wait_code(&h, 200, 10);
    let d = r.deploy(
        90,
        &["galion-nerve", broken.to_str().unwrap(), &h],
        &[("GALION_DEPLOY_RESET_FAILED", "0")],
    );
    let st = r.active("galion-nerve");
    r.check(
        "RESET_FAILED=0: start limit refuses the rollback restart; exit 4 (manual action), not the ambiguous 1",
        d.rc == 4 && sha16(&t) == old && st == "failed" && d.out.contains("ROLLBACK ALSO UNHEALTHY"),
        &format!("exit {}, unit {st}", d.rc),
    );
    let t0 = Instant::now();
    r.x_ctl(&["reset-failed", "galion-nerve"]);
    r.x_ctl(&["restart", "galion-nerve"]);
    let ok = wait_code(&h, 200, 10);
    r.check(
        "recovery: reset-failed + restart brings the old nerve back",
        ok,
        &format!("{} ms", ms_since(t0)),
    );

    r.say("L1 broken binary with an empty start-limit window");
    r.ctl(&["reset-failed", "galion-nerve"]);
    let p = r.probe_start("/healthz", 5);
    let d = r.deploy(90, &["galion-nerve", broken.to_str().unwrap(), &h], &[]);
    let down = r.probe_stop(p);
    r.check(
        "L1 broken binary: exit 3, old sha, healthy",
        d.rc == 3 && sha16(&t) == old && code(&h) == 200,
        &format!("exit {}, outage {down} ms", d.rc),
    );
    r.downtime("auto-rollback-broken", down);

    r.say("F4: the same broken deploy judged on /health (the new nerve's path; the old one answers 404)");
    let d = r.deploy(
        90,
        &["galion-nerve", broken.to_str().unwrap(), &r.url("/health")],
        &[],
    );
    r.check(
        "F4 fixed: forward judged on /health, rollback on the unit's own /healthz: exit 3 (was 'ROLLBACK ALSO UNHEALTHY')",
        d.rc == 3 && sha16(&t) == old && code(&h) == 200 && d.out.contains(&format!("rollback_health={h}")),
        &format!("exit {}, /healthz {}, /health {}", d.rc, code(&h), code(&r.url("/health"))),
    );

    r.say("F1: sim-hang with the default version bound");
    let hang = r.hang();
    let d = r.deploy(90, &["galion-nerve", hang.to_str().unwrap(), &h], &[]);
    r.check(
        "F1 fixed: deploy returns on its own (exit 3), service restored, no orphan",
        d.rc == 3 && sha16(&t) == old && code(&h) == 200 && vpids(&hang).is_empty(),
        &format!("exit {} after {} ms (was: hung, 124)", d.rc, d.ms),
    );

    r.say("F6: SIGTERM in the health window of a broken nerve deploy");
    let (d, seen) = interrupt(
        &mut r,
        "galion-nerve",
        &broken,
        &h,
        "TERM",
        Duration::from_millis(1500),
    );
    r.check(
        "F6 SIGTERM: exit 5, old sha back, healthy",
        seen && d.rc == 5 && sha16(&t) == old && code(&h) == 200,
        &format!("exit {}", d.rc),
    );

    r.say("final forward swap (leave the sim on the new nerve) and end state");
    let p = r.probe_start("/healthz", 2);
    let d = r.deploy(60, &["galion-nerve", n.to_str().unwrap(), &h], &[]);
    let down = r.probe_stop(p);
    r.check(
        "final swap exit 0, downtime < 1 s",
        d.rc == 0 && down < 1000 && sha16(&t) == new,
        &format!("exit {}, outage {down} ms", d.rc),
    );
    r.downtime("final-swap", down);
    let (_, st) = r.ctl(&["status"]);
    r.line(&st);
    r.sup_stop();
    assert_eq!(r.summary(), 0, "nerve rehearsal had failures");
}

// ------------------------------------------------------------------ room
#[test]
fn rehearsal_room() {
    if !gate("room") {
        return;
    }
    let mut r = Rehearsal::new("room", ROOM);
    let h = r.url("/health");
    let t = r.root.join(target_path("room"));
    let w = r.root.join("workspace");
    for (f, body) in [
        ("galion-light/docs/README.md", "# Operator notes (sim)\n"),
        ("galion-src/NOTES.txt", "sim source tree\n"),
        ("galion-light/room/index.html", "<html>room</html>\n"),
    ] {
        fs::create_dir_all(w.join(f).parent().unwrap()).unwrap();
        fs::write(w.join(f), body).unwrap();
    }
    r.denv = vec![
        ("ROOM_BIND".into(), format!("127.0.0.1:{ROOM}")),
        ("ROOM_ROOT".into(), w.display().to_string()),
    ];
    r.sup_start();
    r.say(&format!(
        "today: no room unit, nothing on :{ROOM} (live: nothing on :4020)"
    ));
    let (kc, ko) = r.ctl(&["known", "room"]);
    r.line(&format!(
        "listeners: {}; known room -> {kc} {}",
        listeners(ROOM),
        ko.trim()
    ));
    let rn = r.stage().join("room.new");
    install(&r.bin.join("room-new"), &rn);

    r.say("install the staged unit (deploy/staged/room.service from swarm/room 5bb27e1, sim paths) + daemon-reload");
    let src = std::process::Command::new("git")
        .arg("-C")
        .arg(repo())
        .args(["show", "5bb27e1:deploy/staged/room.service"])
        .output()
        .unwrap();
    assert!(src.status.success());
    let unit: String = String::from_utf8_lossy(&src.stdout)
        .lines()
        .map(|l| {
            let l = if let Some(rest) = l.strip_prefix("ExecStart=/opt/galion/rust/bin/room") {
                format!("ExecStart={}{rest}", t.display())
            } else if l == "Environment=ROOM_BIND=127.0.0.1:4020" {
                format!("Environment=ROOM_BIND=127.0.0.1:{ROOM}")
            } else if l == "Environment=ROOM_ROOT=/workspace" {
                format!("Environment=ROOM_ROOT={}", w.display())
            } else {
                l.to_string()
            };
            l + "\n"
        })
        .collect();
    fs::write(r.root.join("units/room.service"), unit).unwrap();
    r.x_ctl(&["daemon-reload"]);
    let st = r.active("room");
    r.line(&format!(
        "state after reload: {st} (daemon-reload does not start it)"
    ));

    r.say("F5: first install of a BROKEN build on the never-run unit");
    let broken = r.broken();
    let d = r.deploy(90, &["room", broken.to_str().unwrap(), &h], &[]);
    let st = r.active("room");
    r.check(
        "F5: failed first install is undone (exit 3, no binary left, unit not active, nothing listening)",
        d.rc == 3 && !t.exists() && st != "active" && listeners(ROOM) == 0,
        &format!("exit {}, unit {st}", d.rc),
    );

    r.say("F5: first install of room.new through galion-deploy (was: 'cannot resolve installed binary', exit 1)");
    let p = r.probe_start("/health", 5);
    let d = r.deploy(60, &["room", rn.to_str().unwrap(), &h], &[]);
    r.probe_stop(p);
    r.check(
        "F5 fixed: galion-deploy does the first install (exit 0, new sha, healthy, no backup)",
        d.rc == 0
            && sha16(&t) == sha16(&rn)
            && code(&h) == 200
            && r.backups_of("room").is_empty()
            && d.out.contains("msg=\"first install\""),
        &format!("exit {} in {} ms", d.rc, d.ms),
    );
    let (c, body) = get(&h);
    let (_, objs) = get(&r.url("/api/objects"));
    let pw = code(&r.url("/api/read?path=/etc/passwd"));
    let lo = listeners(ROOM);
    r.line(&format!("/health -> {c} '{body}'; /api/objects -> {}; /api/read?path=/etc/passwd -> {pw}; listeners {lo}", objs.chars().take(12).collect::<String>()));
    r.check(
        "room health checks (ok, {\"objects\":[, 404, one loopback listener)",
        body.trim() == "ok" && objs.starts_with("{\"objects\":[") && pw == 404 && lo == 1,
        &format!("{} / {pw}", body.trim()),
    );

    r.say("upgrades go through galion-deploy: same binary (restart gap) and a broken one (auto rollback)");
    let p = r.probe_start("/health", 2);
    let d = r.deploy(60, &["room", rn.to_str().unwrap(), &h], &[]);
    let down = r.probe_stop(p);
    r.check(
        "redeploy exit 0, downtime < 1 s",
        d.rc == 0 && down < 1000,
        &format!("exit {}, outage {down} ms", d.rc),
    );
    r.downtime("redeploy", down);
    let p = r.probe_start("/health", 5);
    let d = r.deploy(90, &["room", broken.to_str().unwrap(), &h], &[]);
    let down = r.probe_stop(p);
    r.check(
        "L1 broken: exit 3, room.new back, healthy",
        d.rc == 3 && sha16(&t) == sha16(&rn) && code(&h) == 200,
        &format!("exit {}, outage {down} ms", d.rc),
    );
    r.downtime("auto-rollback-broken", down);
    let old = r.bin.join("room-old");
    let d1 = r.deploy(60, &["room", old.to_str().unwrap(), &h], &[]);
    let d2 = r.deploy(60, &["room", rn.to_str().unwrap(), &h], &[]);
    r.check(
        "room-old (main 13a296c) deploy and back to room.new: both exit 0",
        d1.rc == 0 && d2.rc == 0 && sha16(&t) == sha16(&rn),
        &format!("exit {} / {}", d1.rc, d2.rc),
    );

    r.say("manual rollback to today's state (room.md): stop, remove the unit, daemon-reload");
    r.x_ctl(&["stop", "room"]);
    fs::remove_file(r.root.join("units/room.service")).unwrap();
    r.x_ctl(&["daemon-reload"]);
    let (kc, _) = r.ctl(&["known", "room"]);
    r.check(
        &format!("rollback: nothing on :{ROOM}, unit gone"),
        listeners(ROOM) == 0 && kc != 0,
        &format!("{} listeners", listeners(ROOM)),
    );
    r.sup_stop();
    assert_eq!(r.summary(), 0, "room rehearsal had failures");
}

// ------------------------------------------------------------------ desk
#[test]
fn rehearsal_desk() {
    if !gate("desk") {
        return;
    }
    let mut r = Rehearsal::new("desk", DESK);
    let h = r.url("/health");
    let t = r.root.join(target_path("galion-desk"));
    for d in ["opt/galion/bin", "state", "one", "hermes"] {
        fs::create_dir_all(r.root.join(d)).unwrap();
    }
    install(&r.bin.join("desk-old"), &t);
    let root = r.root.display().to_string();
    let e: Vec<(String, String)> = [
        ("DESK_BIND", format!("127.0.0.1:{DESK}")),
        ("DESK_STATE_DIR", format!("{root}/state")),
        ("DESK_ONE_DIR", format!("{root}/one")),
        ("DESK_HERMES_DIR", format!("{root}/hermes")),
        (
            "DESK_HERMES_BIN",
            format!("{root}/opt/galion/rust/bin/desk-hermes"),
        ),
        ("DESK_OLLAMA", format!("127.0.0.1:{DEAD}")),
        ("OPENMIND_CONFIG", format!("{root}/none.toml")),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let pr: Vec<String> = [
        "HERMES", "ONE", "TUNNEL", "CONSOLE", "ORIGIN", "ROOM", "TULU",
    ]
    .iter()
    .map(|k| format!("DESK_PROBE_{k}=127.0.0.1:{DEAD}"))
    .collect();
    r.denv = e.clone();
    let env_line: Vec<String> = e.iter().map(|(k, v)| format!("{k}={v}")).collect();
    fs::write(
        r.root.join("units/galion-desk.service"),
        format!(
            "[Unit]\nDescription=galion-desk (sim; live: /opt/galion/bin/galion-desk, no Environment=)\n[Service]\nExecStart={}\nEnvironment={}\nEnvironment={}\nRestart=always\nRestartSec=2\n",
            t.display(),
            env_line.join(" "),
            pr.join(" ")
        ),
    )
    .unwrap();
    r.sup_start();
    let ok = wait_code(&h, 200, 5);
    let (_, hb) = get(&h);
    r.check("old desk serving", ok, hb.trim());
    let old = sha16(&r.bin.join("desk-old"));
    let new = sha16(&r.bin.join("desk-new"));
    let n = r.stage().join("galion-desk.new");
    install(&r.bin.join("desk-new"), &n);
    let (_, desk) = get(&r.url("/desk"));
    r.line(&format!(
        "old sha {old}, new sha {new}; /desk top-level keys (old): {}",
        json_top_keys(&desk).len()
    ));

    r.say("manual cutover per docs/cutover/desk.md steps 8-11 (probe every 2 ms)");
    let oct10 = r.root.join("opt/galion/bin/galion-desk.oct10");
    fs::copy(&t, &oct10).unwrap();
    let p = r.probe_start("/health", 2);
    install(&n, &t);
    r.x_ctl(&["restart", "galion-desk"]);
    wait_code(&h, 200, 5);
    let down = r.probe_stop(p);
    r.downtime("manual-swap", down);
    let (_, desk) = get(&r.url("/desk"));
    let keys = json_top_keys(&desk);
    r.check(
        "manual swap: new sha running, 22 /desk keys, downtime < 1 s",
        sha16(&t) == new && keys.len() == 22 && down < 1000,
        &format!("{} keys, outage {down} ms", keys.len()),
    );

    r.say("manual rollback per step 13");
    let p = r.probe_start("/health", 2);
    install(&oct10, &t);
    r.x_ctl(&["restart", "galion-desk"]);
    wait_code(&h, 200, 5);
    let down = r.probe_stop(p);
    r.check(
        "step 13 rollback: old sha, healthy, downtime < 1 s",
        sha16(&t) == old && code(&h) == 200 && down < 1000,
        &format!("outage {down} ms"),
    );
    r.downtime("manual-rollback", down);

    r.say("the galion-deploy route: swap, broken, manual rollback");
    let p = r.probe_start("/health", 2);
    let d = r.deploy(60, &["galion-desk", n.to_str().unwrap(), &h], &[]);
    let down = r.probe_stop(p);
    r.check(
        "galion-deploy swap exit 0, downtime < 1 s",
        d.rc == 0 && down < 1000,
        &format!("exit {}, outage {down} ms", d.rc),
    );
    r.downtime("deploy-swap", down);
    let bak = r.latest_backup("galion-desk").unwrap();
    let broken = r.broken();
    let p = r.probe_start("/health", 5);
    let d = r.deploy(90, &["galion-desk", broken.to_str().unwrap(), &h], &[]);
    let down = r.probe_stop(p);
    r.check(
        "L1 broken: exit 3, rolled back to the new desk",
        d.rc == 3 && sha16(&t) == new && code(&h) == 200,
        &format!("exit {}, outage {down} ms", d.rc),
    );
    r.downtime("auto-rollback-broken", down);
    let p = r.probe_start("/health", 2);
    let d = r.deploy(60, &["galion-desk", bak.to_str().unwrap(), &h], &[]);
    let down = r.probe_stop(p);
    r.check(
        "L2 manual rollback via galion-deploy: old sha back",
        d.rc == 0 && sha16(&t) == old,
        &format!("exit {}, outage {down} ms", d.rc),
    );
    r.downtime("rollback-L2", down);

    r.say(
        "F1 hang trap: desk DOWN, forward deploy of the new desk (it ignores --version and serves)",
    );
    r.x_ctl(&["stop", "galion-desk"]);
    let run = r.spawn_deploy(60, &["galion-desk", n.to_str().unwrap(), &h], &[]);
    std::thread::sleep(Duration::from_secs(2));
    let during = vpids(&n);
    let dcode = code(&h);
    let ust = r.active("galion-desk");
    r.line(&format!(
        "2 s in: unit {ust}; GET {h} -> {dcode} from '{} --version' (pids {during:?})",
        n.display()
    ));
    let d = r.finish(run);
    let orphan = vpids(&n);
    r.check(
        "F1 fixed: deploy completes on its own (exit 0, new sha, healthy); the stray --version was bounded and killed",
        d.rc == 0 && sha16(&t) == new && code(&h) == 200 && orphan.is_empty() && d.out.contains("--version did not exit within"),
        &format!("exit {} in {} ms (was: hung, 124); --version pids during {during:?}, after {orphan:?}", d.rc, d.ms),
    );

    r.say("F6: SIGHUP (dropped ssh session) in the health window of a broken desk deploy");
    let (d, seen) = interrupt(
        &mut r,
        "galion-desk",
        &broken,
        &h,
        "HUP",
        Duration::from_millis(1500),
    );
    r.check(
        "F6 SIGHUP: exit 5, new desk back, healthy",
        seen && d.rc == 5 && sha16(&t) == new && code(&h) == 200,
        &format!("exit {}", d.rc),
    );
    let (_, st) = r.ctl(&["status"]);
    r.line(&st);
    r.sup_stop();
    assert_eq!(r.summary(), 0, "desk rehearsal had failures");
}
