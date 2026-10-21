// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of nerve plan T2.3 (slot 200, ports 20700-20709): the
//! observe-only specs for openmind-queue :19120, tanga-proto :8671,
//! galion-nerve-shadow :8672, galion-backup.timer and hades-gate, and the
//! known-port list. Gated: `GALION_REAL=1` (box), plus `GALION_REAL_VPS=1`
//! for the read-only VPS part. Run with `--release`.
//!
//! 1. Corpus (slot 186's `replay --reflex` corpus: the fixture and every
//!    `/workspace/swarm/nerve-capture/replay*`): forced dry-run, plain and
//!    `--worst-case --stretch 130`, once with the pre-T2.3 table and once with
//!    this build's table (both as `NERVE_COMPONENTS` files, so only the five
//!    new specs differ) and once with the built-in table: the reflex actions
//!    are identical (0 new actions), no action names a new unit.
//! 2. VPS (read-only over `ssh slavik-vps`: GETs, `ss -ltn`, `systemctl
//!    show`, `journalctl`): a fresh capture with the new capture files
//!    (`systemctl-show.txt`, `systemctl-runs.txt`, `http.txt`), saved as
//!    `nerve-capture/replay200`. Its replay shows the five new components with
//!    their live state, every live listener has an owner (known-port list
//!    matches the live VPS), and the dry reflex replay is unchanged.
//! 3. Negative control: the same capture with every new unit failed, the
//!    timer stopped, its last run failed long ago and every new health port
//!    down, worst case + 130 cycles: the new components go
//!    degrading/failing but the reflex still takes 0 actions.
//! 4. Box: `serve` on 127.0.0.1:20700 observes a real `openmind-queue serve`
//!    (20701) and a second real nerve as the shadow (20702); the queue is
//!    killed and comes back as `degrading`, never restarted (mode dry-run,
//!    reflex recent empty), and is healthy again after a restart by the test.

use openmind_nerve::config::{default_specs, Kind, Spec};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
const PORT: u16 = 20700;
const QUEUE_PORT: u16 = 20701;
const SHADOW_PORT: u16 = 20702;
const NEW: [&str; 5] = [
    "openmind-queue",
    "tanga-proto",
    "galion-nerve-shadow",
    "galion-backup",
    "hades-gate",
];

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn vps() -> bool {
    real() && std::env::var("GALION_REAL_VPS").as_deref() == Ok("1")
}

fn stamp() -> String {
    zagreb_time::now_rfc3339()
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nerve-real-200-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn kind(k: Kind) -> Option<&'static str> {
    match k {
        Kind::Service => Some("service"),
        Kind::Tunnel => Some("tunnel"),
        Kind::Timer => Some("timer"),
        // Host and self are appended by parse_specs; gaia is not file-loadable.
        _ => None,
    }
}

/// A component table as a `NERVE_COMPONENTS` file (every field explicit).
fn table_file(dir: &Path, name: &str, specs: &[Spec]) -> PathBuf {
    let rows: Vec<Value> = specs
        .iter()
        .filter_map(|s| {
            Some(json!({"name": s.name, "unit": s.unit, "port": s.port, "path": s.path,
                "metrics_port": s.metrics_port, "kind": kind(s.kind)?, "gaia": s.gaia,
                "system": s.gaia_system, "group": s.group, "critical": s.critical,
                "expected": s.expected, "restarts_per_hour": s.restarts_per_hour,
                "note": s.note, "max_age_s": s.max_age_s}))
        })
        .collect();
    let f = dir.join(name);
    std::fs::write(&f, Value::Array(rows).to_string()).unwrap();
    f
}

fn old_and_new_tables(dir: &Path) -> (PathBuf, PathBuf) {
    let new = default_specs(None, None);
    let old: Vec<Spec> = new
        .iter()
        .filter(|s| !NEW.contains(&s.name))
        .cloned()
        .collect();
    assert_eq!(new.len(), old.len() + NEW.len(), "all five T2.3 specs built in");
    (
        table_file(dir, "old.json", &old),
        table_file(dir, "new.json", &new),
    )
}

/// `openmind-nerve replay DIR --reflex ...` -> (exit, verdict).
fn dry(dir: &Path, table: Option<&Path>, extra: &[&str]) -> (Option<i32>, Value) {
    let data = tmp("data");
    let mut c = Command::new(BIN);
    c.env_clear().env("NERVE_DATA_DIR", &data);
    if let Some(t) = table {
        c.env("NERVE_COMPONENTS", t);
    }
    let o = c
        .arg("replay")
        .arg(dir)
        .arg("--reflex")
        .args(extra)
        .stderr(Stdio::null())
        .output()
        .expect("run replay --reflex");
    let _ = std::fs::remove_dir_all(&data);
    (
        o.status.code(),
        serde_json::from_slice(&o.stdout).unwrap_or(Value::Null),
    )
}

/// `openmind-nerve replay DIR --state` with the built-in table.
fn state(dir: &Path) -> Value {
    let o = Command::new(BIN)
        .env_clear()
        .arg("replay")
        .arg(dir)
        .arg("--state")
        .stderr(Stdio::null())
        .output()
        .expect("replay --state");
    assert!(o.status.success(), "replay --state failed");
    serde_json::from_slice(&o.stdout).expect("state json")
}

fn corpus() -> Vec<(String, PathBuf)> {
    let mut v = vec![(
        "fixture cortex-replay".to_string(),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cortex-replay"),
    )];
    let root = PathBuf::from(
        std::env::var("NERVE_CAPTURE_ROOT").unwrap_or("/workspace/swarm/nerve-capture".into()),
    );
    let mut caps: Vec<PathBuf> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("replay"))
                && std::fs::read_dir(p)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .any(|e| e.file_name().to_string_lossy().starts_with("state"))
        })
        .collect();
    caps.sort();
    for p in caps {
        v.push((p.file_name().unwrap().to_string_lossy().into_owned(), p));
    }
    v
}

fn row(name: &str, mode: &str, table: &str, code: Option<i32>, v: &Value) -> String {
    format!(
        "| {name} | {mode} | {table} | {} | {} | {} | {} | {} | {} | {}/{} | {:?} |",
        v["cycles"],
        v["restarts"],
        v["cloudflared_restarts"],
        v["rollbacks"],
        v["holds"],
        v["rate_limited"],
        v["score"],
        v["grade"].as_str().unwrap_or("?"),
        code
    )
}

/// No action of a dry replay names one of the new units.
fn no_new_unit_actions(name: &str, v: &Value) {
    for (_, units) in v["actions"].as_object().into_iter().flatten() {
        for u in units.as_object().into_iter().flatten().map(|x| x.0) {
            for n in NEW {
                assert!(!u.starts_with(n), "{name}: action on new unit {u}: {v:#}");
            }
        }
    }
}

/// Old table vs new table vs built-in over one capture: same actions.
fn compare(name: &str, dir: &Path, old: &Path, new: &Path) -> Vec<Value> {
    let mut out = vec![];
    for (mode, extra) in [
        ("plain", &[][..]),
        ("worst+130", &["--worst-case", "--stretch", "130"][..]),
    ] {
        let (co, vo) = dry(dir, Some(old), extra);
        let (cn, vn) = dry(dir, Some(new), extra);
        let (cb, vb) = dry(dir, None, extra);
        println!("{}", row(name, mode, "pre-T2.3", co, &vo));
        println!("{}", row(name, mode, "T2.3 file", cn, &vn));
        println!("{}", row(name, mode, "built-in", cb, &vb));
        assert!(vo.is_object() && vn.is_object() && vb.is_object(), "{name}: no verdict");
        assert_eq!(vo["actions"], vn["actions"], "{name} {mode}: new actions");
        assert_eq!(vo["actions"], vb["actions"], "{name} {mode}: built-in differs");
        for k in ["restarts", "cloudflared_restarts", "rollbacks", "holds", "rate_limited"] {
            assert_eq!(vo[k], vn[k], "{name} {mode}: {k}");
            assert_eq!(vo[k], vb[k], "{name} {mode}: {k} (built-in)");
        }
        assert_eq!(co, cn, "{name} {mode}: exit code");
        assert_eq!(vn["cloudflared_restarts"], 0, "{name}");
        assert_eq!(vn["rollbacks"], 0, "{name}");
        no_new_unit_actions(name, &vn);
        no_new_unit_actions(name, &vb);
        out.push(json!({"capture": name, "mode": mode, "old": vo["actions"], "new": vn["actions"],
            "score_old": vo["score"], "score_new": vn["score"], "score_builtin": vb["score"]}));
    }
    out
}

#[test]
fn real_t23_corpus_zero_new_reflex_actions() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    println!("== {} T2.3 corpus dry replay (binary {BIN})", stamp());
    println!("| capture | mode | table | cycles | restarts | cloudflared | rollbacks | holds | limited | score | exit |");
    let d = tmp("tables");
    let (old, new) = old_and_new_tables(&d);
    let caps = corpus();
    assert!(caps.len() >= 2, "no live capture on this box");
    for (name, dir) in &caps {
        compare(name, dir, &old, &new);
        // Old captures do not name the new components: absent, never failing.
        let st = state(dir);
        for n in NEW {
            let c = st["components"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["name"] == n)
                .unwrap_or_else(|| panic!("{name}: {n} missing"));
            println!("  {name}: {n} -> {} {:?}", c["status"], c["reasons"]);
            if !dir.join("systemctl-show.txt").exists() {
                assert_eq!(c["status"], "absent", "{name}: {n}");
            }
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// Fixed read-only script. Nothing is written on the VPS.
const VPS_SCRIPT: &str = concat!(
    "for i in 1 2 3; do echo '#STATE'; curl -s -m 10 http://127.0.0.1:8670/state; echo; [ $i -lt 3 ] && sleep 31; done; ",
    "echo '#SS'; ss -ltn; ",
    "echo '#SHOW'; systemctl show --no-pager -p Id,LoadState,ActiveState,SubState,Result,UnitFileState,MainPID,NRestarts,MemoryCurrent,MemoryMax,CPUUsageNSec,ActiveEnterTimestampMonotonic openmind-queue.service tanga-proto.service galion-nerve-shadow.service galion-backup.timer hades-gate.service; ",
    "echo '#RUNS'; systemctl show --no-pager --timestamp=unix -p Id,LoadState,ActiveState,Result,ExecMainStatus,ExecMainExitTimestamp galion-backup.service; ",
    "echo '#HTTP'; for p in 19120/healthz 8671/tanga/ 8672/healthz; do printf '%s %s\\n' ${p%%/*} $(curl -s -o /dev/null -m 5 -w '%{http_code}' http://127.0.0.1:$p); done; ",
    "echo '#JOURNAL'; journalctl -o json --since -35min --output-fields=_SYSTEMD_UNIT,PRIORITY,MESSAGE,__REALTIME_TIMESTAMP -u cloudflared -u hermes-rs -u openmind-memory -u galion-one -u tuner -u galion-desk -u console -u galion-health -u link-watch-rs -u openmind-queue -u tanga-proto -u galion-nerve-shadow -u galion-backup.timer -u galion-backup.service --no-pager"
);

fn vps_capture() -> PathBuf {
    let start = stamp();
    let o = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "slavik-vps",
            VPS_SCRIPT,
        ])
        .output()
        .expect("ssh");
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    let mut blocks: Vec<(String, String)> = vec![];
    for l in out.lines() {
        if let Some(k) = l.strip_prefix('#').filter(|k| k.chars().all(|c| c.is_ascii_uppercase())) {
            blocks.push((k.to_string(), String::new()));
        } else if let Some(b) = blocks.last_mut() {
            b.1.push_str(l);
            b.1.push('\n');
        }
    }
    let root = PathBuf::from(
        std::env::var("NERVE_CAPTURE_ROOT").unwrap_or("/workspace/swarm/nerve-capture".into()),
    );
    let dir = root.join("replay200");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut n = 0;
    for b in blocks.iter().filter(|b| b.0 == "STATE") {
        let v: Value = serde_json::from_str(b.1.trim()).expect("live /state json");
        n += 1;
        std::fs::write(dir.join(format!("state-{n}.json")), v.to_string()).unwrap();
    }
    assert_eq!(n, 3, "live /state x3");
    for (k, f) in [
        ("SS", "ss.txt"),
        ("SHOW", "systemctl-show.txt"),
        ("RUNS", "systemctl-runs.txt"),
        ("HTTP", "http.txt"),
        ("JOURNAL", "journal.jsonl"),
    ] {
        let b = blocks.iter().find(|b| b.0 == k).unwrap_or_else(|| panic!("no {k}"));
        std::fs::write(dir.join(f), &b.1).unwrap();
    }
    std::fs::write(
        dir.join("CAPTURED"),
        format!("captured {start} .. {} read-only (slot 200): 3x GET :8670/state 31 s apart, ss -ltn, systemctl show (5 T2.3 units + galion-backup.service --timestamp=unix), GET 19120/healthz 8671/tanga/ 8672/healthz (codes only), journalctl -o json 35 min\n", stamp()),
    )
    .unwrap();
    dir
}

fn comp<'a>(st: &'a Value, n: &str) -> &'a Value {
    st["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == n)
        .unwrap_or_else(|| panic!("{n} missing"))
}

#[test]
fn real_t23_vps_capture_specs_listeners_and_negative_control() {
    if !vps() {
        eprintln!("skipped: set GALION_REAL=1 GALION_REAL_VPS=1");
        return;
    }
    let dir = vps_capture();
    println!("== {} VPS capture {}", stamp(), dir.display());
    println!(
        "{}",
        std::fs::read_to_string(dir.join("http.txt")).unwrap().trim()
    );
    let st = state(&dir);
    println!(
        "replay200 score {} grade {} system {}",
        st["self_eval"]["score"], st["self_eval"]["grade"], st["system"]["status"]
    );
    for n in NEW {
        let c = comp(&st, n);
        println!(
            "  {n}: {} unit {} {} {} http {} port {} rph {} gaia {} reasons {:?} last_run {}",
            c["status"],
            c["unit"],
            c["unit_state"],
            c["unit_sub"],
            c["http"],
            c["port"],
            c["reflex_restarts_per_hour"],
            c["gaia"],
            c["reasons"],
            c["extra"]["last_run"]
        );
        assert_eq!(c["reflex_restarts_per_hour"], 0, "{n}: observe-only");
        assert_eq!(c["consecutive_down"], 0, "{n}");
    }
    for n in ["openmind-queue", "tanga-proto", "galion-nerve-shadow"] {
        assert_eq!(comp(&st, n)["status"], "healthy", "{n}");
        assert_eq!(comp(&st, n)["unit_state"], "active", "{n}");
    }
    let b = comp(&st, "galion-backup");
    assert_eq!(b["status"], "healthy");
    assert_eq!(b["unit_sub"], "waiting");
    assert_eq!(b["extra"]["last_run"]["result"], "success");
    assert!(b["extra"]["last_run"]["finished_unix"].as_u64().is_some());
    assert_eq!(comp(&st, "hades-gate")["status"], "absent");
    // Known-port list matches the live VPS: every listener has an owner.
    let ss = std::fs::read_to_string(dir.join("ss.txt")).unwrap();
    let mut unknown = vec![];
    for x in st["cortex"]["vulnerabilities"].as_array().into_iter().flatten() {
        if x["id"] == "unmanaged-listeners" || x.to_string().contains("unknown owner") {
            unknown.push(x["title"].clone());
        }
    }
    println!(
        "  exposure: listeners_judged {} unmanaged_ports {}",
        st["cortex"]["exposure"]["listeners_judged"], st["cortex"]["exposure"]["unmanaged_ports"]
    );
    assert_eq!(
        st["cortex"]["exposure"]["unmanaged_ports"],
        json!([]),
        "live listeners no component or known port owns"
    );
    let mut owners: Vec<Value> = vec![];
    collect_owners(&st, &mut owners);
    println!(
        "  listeners in ss.txt: {}; owner entries {}; unknown-owner findings {:?}",
        ss.lines().filter(|l| l.starts_with("LISTEN")).count(),
        owners.len(),
        unknown
    );
    for o in &owners {
        println!("    :{} -> {}", o["port"], o["owner"]);
        assert_ne!(o["owner"], "unknown owner", "live listener without owner: {o}");
    }
    assert!(unknown.is_empty(), "unknown-owner findings: {unknown:?}");

    let d = tmp("tables-vps");
    let (old, new) = old_and_new_tables(&d);
    println!("| capture | mode | table | cycles | restarts | cloudflared | rollbacks | holds | limited | score | exit |");
    compare("replay200", &dir, &old, &new);

    // Negative control: everything new broken, worst case, 130 more cycles.
    let neg = tmp("negative");
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        std::fs::copy(e.path(), neg.join(e.file_name())).unwrap();
    }
    let show = std::fs::read_to_string(dir.join("systemctl-show.txt"))
        .unwrap()
        .replace("ActiveState=active", "ActiveState=failed")
        .replace("SubState=running", "SubState=failed")
        .replace("SubState=waiting", "SubState=dead")
        .replace("Result=success", "Result=exit-code");
    std::fs::write(neg.join("systemctl-show.txt"), show).unwrap();
    std::fs::write(
        neg.join("systemctl-runs.txt"),
        "Id=galion-backup.service\nLoadState=loaded\nActiveState=failed\nResult=exit-code\nExecMainStatus=1\nExecMainExitTimestamp=@1792000000\n",
    )
    .unwrap();
    std::fs::write(neg.join("http.txt"), "19120 000\n8671 503\n8672 000\n").unwrap();
    let nst = state(&neg);
    for n in ["openmind-queue", "tanga-proto", "galion-nerve-shadow", "galion-backup"] {
        let c = comp(&nst, n);
        println!("  negative {n}: {} {:?}", c["status"], c["reasons"]);
        assert!(
            ["degrading", "failing-soon", "failing"].contains(&c["status"].as_str().unwrap()),
            "{n}: {c}"
        );
        assert_eq!(c["consecutive_down"], 0, "{n}: never hard-down");
    }
    let (code, v) = dry(&neg, None, &["--worst-case", "--stretch", "130"]);
    println!("{}", row("replay200 negative", "worst+130", "built-in", code, &v));
    let (_, vbase) = dry(&dir, None, &["--worst-case", "--stretch", "130"]);
    assert_eq!(v["actions"], vbase["actions"], "broken new specs caused actions");
    no_new_unit_actions("negative", &v);
    assert_eq!(v["rollbacks"], 0);
    let _ = std::fs::remove_dir_all(&neg);
    let _ = std::fs::remove_dir_all(&d);
}

fn collect_owners(v: &Value, out: &mut Vec<Value>) {
    match v {
        Value::Object(m) => {
            if m.contains_key("port") && m.contains_key("owner") {
                if !out.iter().any(|o| o["port"] == m["port"]) {
                    out.push(v.clone());
                }
            }
            for x in m.values() {
                collect_owners(x, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_owners(x, out)),
        _ => {}
    }
}

fn get(port: u16, path: &str) -> Option<Value> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (_, body) = b.split_once("\r\n\r\n")?;
    serde_json::from_str(body).ok()
}

fn sibling_bin(name: &str) -> PathBuf {
    // target/<profile>/deps/real_t23_specs-* -> target/<profile>/<name>
    let exe = std::env::current_exe().unwrap();
    let p = exe.parent().unwrap().parent().unwrap().join(name);
    assert!(
        p.exists(),
        "{} missing: build it first (cargo build --release -p openmind-queue)",
        p.display()
    );
    p
}

fn queue(data: &Path) -> Kill {
    Kill(
        Command::new(sibling_bin("openmind-queue"))
            .args(["serve", "--bind", &format!("127.0.0.1:{QUEUE_PORT}"), "--data"])
            .arg(data)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("openmind-queue serve"),
    )
}

fn wait_cycle(min: u64, pred: &dyn Fn(&Value) -> bool) -> Value {
    let t = Instant::now();
    loop {
        if let Some(v) =
            get(PORT, "/state").filter(|v| v["cycle"].as_u64() >= Some(min) && pred(v))
        {
            return v;
        }
        assert!(t.elapsed() < Duration::from_secs(90), "timeout waiting for /state");
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn status_of(st: &Value, n: &str) -> String {
    comp(st, n)["status"].as_str().unwrap_or("?").to_string()
}

#[test]
fn real_t23_box_serve_observes_queue_and_shadow() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let root = tmp("box");
    let qdata = root.join("queue");
    std::fs::create_dir_all(&qdata).unwrap();
    let specs: Vec<Spec> = default_specs(None, None)
        .into_iter()
        .filter(|s| NEW.contains(&s.name))
        .map(|mut s| {
            match s.name {
                "openmind-queue" => s.port = Some(QUEUE_PORT),
                "galion-nerve-shadow" => s.port = Some(SHADOW_PORT),
                _ => {}
            }
            s
        })
        .collect();
    let table = table_file(&root, "box.json", &specs);
    let mut q = queue(&qdata);
    let sdata = root.join("shadow");
    let _shadow = Kill(
        Command::new(BIN)
            .env_clear()
            .env("NERVE_DATA_DIR", &sdata)
            .env("NERVE_DRY_RUN", "1")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_LISTEN_SCOPE", "20700-20709")
            .args(["serve", "--listen", &format!("127.0.0.1:{SHADOW_PORT}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("shadow serve"),
    );
    let data = root.join("main");
    let _main = Kill(
        Command::new(BIN)
            .env_clear()
            .env("NERVE_DATA_DIR", &data)
            .env("NERVE_DRY_RUN", "1")
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_FAIL_CYCLES", "1")
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_LISTEN_SCOPE", "20700-20709")
            .env("NERVE_COMPONENTS", &table)
            .args(["serve", "--listen", &format!("127.0.0.1:{PORT}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("serve"),
    );
    let st = wait_cycle(2, &|v| {
        status_of(v, "openmind-queue") == "healthy" && status_of(v, "galion-nerve-shadow") == "healthy"
    });
    println!(
        "box {} :{PORT} cycle {} mode {}: queue {} http {}, shadow {} http {}, backup {}, tanga {} {:?}, hades-gate {}",
        stamp(),
        st["cycle"],
        st["mode"],
        status_of(&st, "openmind-queue"),
        comp(&st, "openmind-queue")["http"],
        status_of(&st, "galion-nerve-shadow"),
        comp(&st, "galion-nerve-shadow")["http"],
        status_of(&st, "galion-backup"),
        status_of(&st, "tanga-proto"),
        comp(&st, "tanga-proto")["reasons"],
        status_of(&st, "hades-gate")
    );
    assert_eq!(st["mode"], "dry-run");
    // tanga-proto has no server on the box: its port 8671 is down -> degrading
    // (critical false), never failing, never acted on.
    assert_eq!(status_of(&st, "tanga-proto"), "degrading");
    assert_eq!(status_of(&st, "hades-gate"), "absent");
    let c0 = st["cycle"].as_u64().unwrap();
    drop(q);
    let down = wait_cycle(c0 + 2, &|v| status_of(v, "openmind-queue") != "healthy");
    let qc = comp(&down, "openmind-queue");
    println!(
        "box {} queue killed: cycle {} status {} reasons {:?} consecutive_down {} reflex recent {}",
        stamp(),
        down["cycle"],
        qc["status"],
        qc["reasons"],
        qc["consecutive_down"],
        down["reflex"]["recent"]
    );
    assert_eq!(qc["status"], "degrading");
    assert_eq!(qc["consecutive_down"], 0, "critical false: never hard-down");
    let after = wait_cycle(down["cycle"].as_u64().unwrap() + 3, &|_| true);
    assert_eq!(after["reflex"]["recent"].as_array().map_or(0, Vec::len), 0);
    let audit = get(PORT, "/audit").unwrap_or(Value::Null);
    println!("box audit after 3 more cycles: {audit}");
    assert!(audit.as_array().is_none_or(|a| a.is_empty()));
    q = queue(&qdata);
    let back = wait_cycle(after["cycle"].as_u64().unwrap() + 1, &|v| {
        status_of(v, "openmind-queue") == "healthy"
    });
    println!(
        "box {} queue restarted by the test: cycle {} status {}",
        stamp(),
        back["cycle"],
        status_of(&back, "openmind-queue")
    );
    drop(q);
    drop(_main);
    drop(_shadow);
    let _ = std::fs::remove_dir_all(&root);
}
