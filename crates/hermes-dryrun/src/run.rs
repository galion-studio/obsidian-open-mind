// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! `hermes-dryrun run`: the outer process. Hashes the candidate, picks the
//! test ports, re-executes itself as `inner` inside a fresh user + network +
//! pid namespace (`unshare`), waits with a hard deadline, checks that
//! nothing of the run is left, and writes the final result record.
//!
//! `inner` (PID 1 of the namespace): only `lo` exists and a default route
//! into `lo` catches every outbound connect (it can never leave; the
//! monitor sees it). Builds the copy, starts the stubs and the candidate,
//! runs the checks, stops everything and writes `inner-result.json`.

use crate::layout::{self, Ports};
use crate::netwatch::{self, Watch};
use crate::stubs::{self, Ctx};
use crate::util::{http, now_secs, sha256_file, zagreb, Log};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const PORT_LO: u16 = 19000;
pub const PORT_HI: u16 = 23999;

#[derive(Clone)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    pub log: Option<String>,
}

fn check(name: &str, ok: bool, detail: impl Into<String>, log: Option<&str>) -> Check {
    Check {
        name: name.into(),
        ok,
        detail: detail.into(),
        log: log.map(String::from),
    }
}

fn cut(s: &str, n: usize) -> String {
    let s = s.replace('\n', " ");
    if s.len() <= n {
        return s;
    }
    let mut e = n;
    while !s.is_char_boundary(e) {
        e -= 1;
    }
    format!("{}…", &s[..e])
}

fn pick_ports(seed: &str) -> Ports {
    // 7 ports in a block chosen from the id hash, inside 19000-23999
    let h = crate::util::sha256_bytes(seed.as_bytes());
    let n = u32::from_str_radix(&h[..8], 16).unwrap_or(0);
    let span = (PORT_HI - PORT_LO + 1 - 16) as u32;
    let base = PORT_LO + (n % span) as u16;
    Ports {
        gw: base,
        model: base + 1,
        memory: base + 2,
        nerve: base + 3,
        cortex: base + 4,
        queue: base + 5,
        sink: base + 6,
    }
}

pub struct RunArgs {
    pub candidate: PathBuf,
    pub layout: PathBuf,
    pub entry: PathBuf,
    pub id: String,
    pub stack_bin: Option<PathBuf>,
    pub deadline_s: u64,
}

/// Outer process. Returns the process exit code (0 pass, 1 fail, 2 error).
pub fn run(a: RunArgs) -> Result<i32, String> {
    let t0 = Instant::now();
    let started = now_secs();
    let (artifact, size) = sha256_file(&a.candidate)?;
    let dr = a.entry.join("dryrun");
    if dr.join("result.json").exists() || dr.join("work").exists() {
        return Err(format!(
            "{} already holds a dry run; use a new entry (nothing is overwritten or deleted)",
            dr.display()
        ));
    }
    std::fs::create_dir_all(dr.join("logs")).map_err(|e| e.to_string())?;
    if !a
        .id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        || a.id.is_empty()
        || a.id.len() > 120
    {
        return Err("bad --id".into());
    }
    let ports = pick_ports(&format!("{}:{}", a.id, crate::util::nonce()));
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let (uid, gid) = {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata("/proc/self").map_err(|e| e.to_string())?;
        (m.uid(), m.gid())
    };
    if uid == 0 {
        return Err("refusing to run as root: run the dry run as an unprivileged user (the namespace gives it no real privilege)".into());
    }
    let spec = json!({
        "candidate": a.candidate, "artifact_sha256": artifact, "layout": a.layout,
        "dryrun_dir": dr, "id": a.id, "stack_bin": a.stack_bin, "self": me,
        "uid": uid, "gid": gid, "ports": [ports.gw, ports.model, ports.memory, ports.nerve, ports.cortex, ports.queue, ports.sink],
        "started": started,
    });
    let spec_path = dr.join("spec.json");
    std::fs::write(&spec_path, format!("{spec:#}\n")).map_err(|e| e.to_string())?;
    let olog = Log::create(&dr.join("logs/outer.log"))?;
    olog.line(&format!(
        "candidate {} sha256 {artifact} ({size} bytes)",
        a.candidate.display()
    ));
    olog.line(&format!(
        "ports gw={} stubs={:?}",
        ports.gw,
        &ports.all()[1..]
    ));

    // unshare: user (mapped root, no real privilege), net (only lo), pid
    // (namespace PID 1 = inner; when it exits the kernel kills every
    // process left in the run), mount (private /proc).
    let unshare = ["/usr/bin/unshare", "/bin/unshare"]
        .into_iter()
        .find(|p| Path::new(p).exists())
        .ok_or("unshare not found: refusing to run without network isolation")?;
    let mut child = Command::new(unshare)
        .args([
            "--user",
            "--map-root-user",
            "--net",
            "--pid",
            "--fork",
            "--mount-proc",
            "--kill-child",
            "--",
        ])
        .arg(&me)
        .arg("inner")
        .arg(&spec_path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            std::fs::File::create(dr.join("logs/inner.stdout.log")).map_err(|e| e.to_string())?,
        ))
        .stderr(Stdio::from(
            std::fs::File::create(dr.join("logs/inner.stderr.log")).map_err(|e| e.to_string())?,
        ))
        .spawn()
        .map_err(|e| format!("unshare: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(a.deadline_s);
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            break Some(s);
        }
        if Instant::now() > deadline {
            olog.line("deadline hit: killing the namespace");
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    olog.line(&format!("inner exit: {status:?}"));

    // teardown: nothing of the run may survive the namespace
    std::thread::sleep(Duration::from_millis(300));
    let work = dr.join("work").display().to_string();
    let mut left = vec![];
    if let Ok(rd) = std::fs::read_dir("/proc") {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if !n.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let exe = std::fs::read_link(e.path().join("exe"))
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let cmd = std::fs::read(e.path().join("cmdline"))
                .map(|b| String::from_utf8_lossy(&b).replace('\0', " "))
                .unwrap_or_default();
            if exe.starts_with(&work)
                || cmd.contains(&work)
                || cmd.contains(&spec_path.display().to_string())
            {
                left.push(n);
            }
        }
    }
    let inner: Value = std::fs::read(dr.join("inner-result.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null);
    let mut checks: Vec<Value> = inner["checks"].as_array().cloned().unwrap_or_default();
    if checks.is_empty() {
        checks.push(json!({"name": "inner", "ok": false, "detail": format!("the namespace run produced no checks (exit {status:?}); see logs/inner.stderr.log"), "log": "logs/inner.stderr.log"}));
    }
    if status.is_none() {
        checks.push(json!({"name": "deadline", "ok": false, "detail": format!("run exceeded {} s and was killed", a.deadline_s)}));
    }
    checks.push(json!({"name": "teardown", "ok": left.is_empty(),
        "detail": if left.is_empty() { "namespace gone: no process of the run is left (pid namespace PID 1 exited, kernel killed the rest)".to_string() } else { format!("{} process(es) of the run still alive: {}", left.len(), left.join(",")) },
        "log": "logs/outer.log"}));
    let (after, _) = sha256_file(&a.candidate)?;
    checks.push(
        json!({"name": "artifact-unchanged", "ok": after == artifact,
        "detail": format!("candidate sha256 before {} after {}", &artifact[..16], &after[..16])}),
    );
    checks.truncate(64);
    let ok = checks.iter().all(|c| c["ok"].as_bool() == Some(true));
    let model_calls_real = inner["model_calls_real"].as_u64().unwrap_or(1);
    let result = json!({
        "version": "galion-sm/dryrun-result/v1",
        "id": a.id,
        "artifact_sha256": artifact,
        "port": ports.gw,
        "model_calls_real": model_calls_real,
        "checks": checks,
    });
    std::fs::write(dr.join("result.json"), format!("{result:#}\n")).map_err(|e| e.to_string())?;
    let secs = t0.elapsed().as_secs_f64();
    let summary = json!({
        "verdict": if ok && model_calls_real == 0 { "pass" } else { "fail" },
        "started": zagreb(started), "finished": zagreb(now_secs()), "seconds": (secs * 10.0).round() / 10.0,
        "artifact_sha256": artifact, "artifact_bytes": size, "port": ports.gw,
        "deviations": inner["deviations"], "fingerprints": inner["fingerprints"],
        "model_calls_fake": inner["model_calls_fake"], "network": inner["network"],
    });
    std::fs::write(dr.join("summary.json"), format!("{summary:#}\n")).map_err(|e| e.to_string())?;
    olog.line(&format!("verdict {} in {secs:.1} s", summary["verdict"]));
    println!(
        "dry run {}: {} ({:.1} s, port {}, {} checks)",
        a.id,
        summary["verdict"].as_str().unwrap_or("?").to_uppercase(),
        secs,
        ports.gw,
        result["checks"].as_array().map(|c| c.len()).unwrap_or(0)
    );
    for c in result["checks"].as_array().unwrap() {
        println!(
            "  [{}] {:<22} {}",
            if c["ok"].as_bool() == Some(true) {
                "ok"
            } else {
                "FAIL"
            },
            c["name"].as_str().unwrap_or(""),
            cut(c["detail"].as_str().unwrap_or(""), 150)
        );
    }
    println!("record: {}", dr.join("result.json").display());
    Ok(if summary["verdict"] == "pass" { 0 } else { 1 })
}

// ---------------------------------------------------------------- inner ----

struct Gw {
    child: Child,
}

impl Gw {
    fn stop(&mut self, log: &Log) -> Option<i32> {
        let pid = self.child.id();
        let _ = Command::new("kill")
            .args(["-TERM", &format!("-{pid}")])
            .status();
        let t = Instant::now();
        loop {
            if let Ok(Some(s)) = self.child.try_wait() {
                log.line(&format!("gateway stopped: {s:?}"));
                return s.code();
            }
            if t.elapsed() > Duration::from_secs(10) {
                let _ = Command::new("kill")
                    .args(["-KILL", &format!("-{pid}")])
                    .status();
                let _ = self.child.kill();
                let s = self.child.wait().ok();
                log.line(&format!("gateway killed after 10 s: {s:?}"));
                return s.and_then(|s| s.code());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn sh(cmd: &str, args: &[&str]) -> Result<String, String> {
    let o = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("{cmd}: {e}"))?;
    if !o.status.success() {
        return Err(format!(
            "{cmd} {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

pub fn inner(spec_path: &Path) -> i32 {
    let spec: Value = match std::fs::read(spec_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
    {
        Some(v) => v,
        None => {
            eprintln!("inner: bad spec");
            return 2;
        }
    };
    let dr = PathBuf::from(spec["dryrun_dir"].as_str().unwrap_or(""));
    let mut checks: Vec<Check> = vec![];
    let mut extra = json!({});
    let r = inner_run(&spec, &dr, &mut checks, &mut extra);
    if let Err(e) = r {
        checks.push(check("setup", false, cut(&e, 480), Some("logs/inner.log")));
        let _ = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(dr.join("logs/inner.log"))
            .map(|mut f| {
                use std::io::Write;
                let _ = writeln!(f, "{} setup error: {e}", zagreb(now_secs()));
            });
    }
    extra["checks"] = Value::Array(
        checks
            .iter()
            .map(|c| {
                let mut v = json!({"name": c.name, "ok": c.ok, "detail": cut(&c.detail, 480)});
                if let Some(l) = &c.log {
                    v["log"] = json!(l);
                }
                v
            })
            .collect(),
    );
    if extra.get("model_calls_real").is_none() {
        extra["model_calls_real"] = json!(1);
    }
    let _ = std::fs::write(dr.join("inner-result.json"), format!("{extra:#}\n"));
    0
}

fn inner_run(
    spec: &Value,
    dr: &Path,
    checks: &mut Vec<Check>,
    extra: &mut Value,
) -> Result<(), String> {
    let p: Vec<u16> = spec["ports"]
        .as_array()
        .ok_or("ports")?
        .iter()
        .filter_map(|x| x.as_u64().map(|n| n as u16))
        .collect();
    if p.len() != 7 || p.iter().any(|x| !(PORT_LO..=PORT_HI).contains(x)) {
        return Err("ports outside 19000-23999".into());
    }
    let ports = Ports {
        gw: p[0],
        model: p[1],
        memory: p[2],
        nerve: p[3],
        cortex: p[4],
        queue: p[5],
        sink: p[6],
    };
    let logs = dr.join("logs");
    let log = Log::create(&logs.join("inner.log"))?;
    let nonce = crate::util::nonce()[..12].to_string();

    // ---- 1. network isolation ------------------------------------------------
    let ifs = netwatch::interfaces();
    if ifs != ["lo"] {
        return Err(format!(
            "network namespace has interfaces {ifs:?}, expected only lo: refusing"
        ));
    }
    sh("ip", &["link", "set", "lo", "up"])?;
    sh("ip", &["route", "add", "default", "dev", "lo"])?;
    let v6 = sh("ip", &["-6", "route", "add", "default", "dev", "lo"]).is_ok();
    let routes = sh("ip", &["route", "show"]).unwrap_or_default();
    log.line(&format!(
        "netns: interfaces {ifs:?}; routes: {}; v6 default via lo: {v6}",
        routes.trim().replace('\n', " | ")
    ));
    // name resolution inside the namespace goes to the harness DNS sink
    // only (private mount namespace; the host files are not touched)
    let work0 = dr.join("work");
    std::fs::create_dir_all(work0.join("etc")).map_err(|e| e.to_string())?;
    let rc = work0.join("etc/resolv.conf");
    std::fs::write(&rc, "# hermes-dryrun: DNS sink inside the namespace\nnameserver 127.0.0.1\noptions timeout:1 attempts:1\n").map_err(|e| e.to_string())?;
    let ns = work0.join("etc/nsswitch.conf");
    let nss = std::fs::read_to_string("/etc/nsswitch.conf").unwrap_or_default();
    let nss: String = nss
        .lines()
        .filter(|l| !l.trim_start().starts_with("hosts:"))
        .map(|l| format!("{l}\n"))
        .collect::<String>()
        + "hosts: files dns\n";
    std::fs::write(&ns, nss).map_err(|e| e.to_string())?;
    let empty = work0.join("empty");
    std::fs::create_dir_all(&empty).map_err(|e| e.to_string())?;
    let bind = |src: &Path, dst: &str| sh("mount", &["--bind", &src.display().to_string(), dst]);
    bind(&rc, "/etc/resolv.conf").map_err(|e| format!("cannot point DNS at the sink: {e}"))?;
    bind(&ns, "/etc/nsswitch.conf").map_err(|e| format!("cannot pin nsswitch: {e}"))?;
    for d in ["/run/systemd/resolve", "/run/nscd", "/var/run/nscd"] {
        if Path::new(d).is_dir() {
            bind(&empty, d).map_err(|e| format!("cannot hide {d}: {e}"))?;
        }
    }
    let nlog = Log::create(&logs.join("network.log"))?;
    nlog.line(&format!(
        "interfaces {ifs:?}; routes {}",
        routes.trim().replace('\n', " | ")
    ));
    let allowed: BTreeSet<u16> = ports.all().into_iter().collect();
    let watch = Watch::start(allowed, nlog.clone());
    // canary: a connect to TEST-NET-1 must fail AND be seen by the monitor
    let canary_err = {
        let r = std::net::TcpStream::connect_timeout(
            &(netwatch::CANARY.0, netwatch::CANARY.1).into(),
            Duration::from_millis(1500),
        );
        match r {
            Ok(_) => "CONNECTED".to_string(),
            Err(e) => e.to_string(),
        }
    };
    std::thread::sleep(Duration::from_millis(100));
    let canary_seen = watch.report.lock().unwrap().canary_seen;
    nlog.line(&format!(
        "canary connect 192.0.2.1:9 -> {canary_err}; monitor saw it: {canary_seen}"
    ));

    // ---- 2. the copy -----------------------------------------------------------
    let work = dr.join("work");
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let stack_bin = spec["stack_bin"].as_str().map(PathBuf::from);
    let candidate = PathBuf::from(spec["candidate"].as_str().ok_or("candidate")?);
    let self_exe = PathBuf::from(spec["self"].as_str().ok_or("self")?);
    let layout_dir = PathBuf::from(spec["layout"].as_str().ok_or("layout")?);
    let built = layout::build(&layout::Inputs {
        layout: &layout_dir,
        run: &work,
        candidate: &candidate,
        stack_bin: stack_bin.as_deref(),
        self_exe: &self_exe,
        ports: &ports,
        nonce: &nonce,
    })?;
    std::fs::create_dir_all(work.join("logs")).ok();
    let artifact = spec["artifact_sha256"].as_str().unwrap_or("").to_string();
    let (copy_sha, _) = sha256_file(&built.gateway)?;
    log.line(&format!(
        "copy built under {}; gateway copy sha256 {copy_sha}",
        built.root.display()
    ));
    for d in &built.deviations {
        log.line(&format!("deviation: {d}"));
    }
    // env dump without values of dummy secrets (names only for those)
    let envlog = Log::create(&logs.join("gateway-env.log"))?;
    for (k, v) in &built.env {
        let shown = if built.dummies.contains_key(v) {
            "<dummy>".to_string()
        } else {
            v.clone()
        };
        envlog.json(&json!({"k": k, "v": shown}));
    }
    extra["deviations"] = json!(built.deviations);

    // ---- 3. stubs ----------------------------------------------------------------
    let ctx = |f: &str| -> Result<Ctx, String> {
        Ok(Ctx {
            nonce: nonce.clone(),
            dummies: built.dummies.clone(),
            env_file: built.env_file.display().to_string(),
            log: Log::create(&logs.join(f))?,
            calls: Arc::new(AtomicU64::new(0)),
            sink_hits: Arc::new(Mutex::new(vec![])),
        })
    };
    let mctx = ctx("fake-model.jsonl")?;
    let model_calls = mctx.calls.clone();
    stubs::start_model(ports.model, mctx)?;
    let memctx = ctx("stub-memory.jsonl")?;
    stubs::start_memory(ports.memory, memctx)?;
    stubs::start_status(ports.nerve, "nerve", ctx("stub-nerve.jsonl")?)?;
    stubs::start_status(ports.cortex, "cortex", ctx("stub-cortex.jsonl")?)?;
    stubs::start_status(ports.queue, "queue", ctx("stub-queue.jsonl")?)?;
    let dns_hits = Arc::new(Mutex::new(vec![]));
    stubs::start_dns(ctx("stub-dns.jsonl")?, dns_hits.clone())?;
    let sctx = ctx("stub-sink.jsonl")?;
    let sink_hits = sctx.sink_hits.clone();
    stubs::start_sink(ports.sink, sctx)?;
    if !crate::util::port_free(ports.gw) {
        return Err(format!(
            "gateway test port {} is busy inside the namespace",
            ports.gw
        ));
    }

    // The gateway runs as the invoking user's uid again (nested user
    // namespace), not as namespace root: like live, it is not root, and it
    // keeps its own uid for MCP children and tools.
    let as_user = vec![
        "--user".to_string(),
        format!("--map-user={}", spec["uid"].as_u64().unwrap_or(65534)),
        format!("--map-group={}", spec["gid"].as_u64().unwrap_or(65534)),
        "--".to_string(),
    ];
    let run_cmd = |args: &[&str], logname: &str, secs: u64| -> (Option<i32>, String) {
        let out = std::fs::File::create(logs.join(logname)).ok();
        let mut c = Command::new("unshare");
        c.args(&as_user)
            .arg(&built.gateway)
            .args(&built.gateway_args)
            .args(args)
            .env_clear()
            .envs(built.env.iter().map(|(k, v)| (k, v)))
            .current_dir(&built.hermes_home)
            .stdin(Stdio::null());
        if let Some(f) = out {
            let f2 = f.try_clone().ok();
            c.stdout(Stdio::from(f));
            if let Some(f2) = f2 {
                c.stderr(Stdio::from(f2));
            }
        }
        let Ok(mut ch) = c.spawn() else {
            return (None, "spawn failed".into());
        };
        let t = Instant::now();
        loop {
            if let Ok(Some(_)) = ch.try_wait() {
                break;
            } else if t.elapsed() > Duration::from_secs(secs) {
                let _ = ch.kill();
                let _ = ch.wait();
                return (None, format!("timed out after {secs} s"));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let code = ch.wait().ok().and_then(|s| s.code());
        let text = std::fs::read_to_string(logs.join(logname)).unwrap_or_default();
        (code, text)
    };

    // ---- 4. config checks without binding ---------------------------------------------
    let (code, text) = run_cmd(&["--check-config"], "check-config.log", 60);
    checks.push(check(
        "check-config",
        code == Some(0),
        format!(
            "--check-config exit {code:?}: {}",
            cut(text.lines().last().unwrap_or(""), 200)
        ),
        Some("logs/check-config.log"),
    ));
    let (code, text) = run_cmd(&["--check-mcp"], "check-mcp.log", 90);
    let om_line = text.lines().find(|l| l.contains("openmind")).unwrap_or("");
    let om_tools = cut(om_line, 160);
    let om_ok = !om_line.is_empty() && !om_line.contains("NOT CONNECTED");
    checks.push(check(
        "check-mcp",
        code == Some(0) && om_ok,
        format!("--check-mcp exit {code:?}; {om_tools}"),
        Some("logs/check-mcp.log"),
    ));

    // ---- 5. start the candidate -------------------------------------------------------
    let gwlog = logs.join("gateway.log");
    let f = std::fs::File::create(&gwlog).map_err(|e| e.to_string())?;
    use std::os::unix::process::CommandExt;
    let child = Command::new("unshare")
        .args(&as_user)
        .arg(&built.gateway)
        .args(&built.gateway_args)
        .env_clear()
        .envs(built.env.iter().map(|(k, v)| (k, v)))
        .current_dir(&built.hermes_home)
        .stdin(Stdio::null())
        .stdout(Stdio::from(f.try_clone().map_err(|e| e.to_string())?))
        .stderr(Stdio::from(f))
        .process_group(0)
        .spawn()
        .map_err(|e| format!("start gateway: {e}"))?;
    let mut gw = Gw { child };
    let t = Instant::now();
    let mut up = false;
    let mut exited = None;
    while t.elapsed() < Duration::from_secs(60) {
        if let Ok(Some(s)) = gw.child.try_wait() {
            exited = Some(s);
            break;
        }
        if let Ok(r) = http(
            ports.gw,
            "GET",
            "/health",
            None,
            None,
            Duration::from_secs(3),
        ) {
            if r.status == 200 {
                up = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let startup_s = t.elapsed().as_secs_f64();
    log.line(&format!(
        "gateway up={up} after {startup_s:.1} s exited={exited:?}"
    ));
    let key = built.api_key.clone();
    let k = Some(key.as_str());
    let to = Duration::from_secs(60);
    if up {
        let r = http(ports.gw, "GET", "/health", None, None, to);
        checks.push(match r {
            Ok(r) => check(
                "health",
                r.status == 200,
                format!(
                    "GET /health {} in {startup_s:.1} s: {}",
                    r.status,
                    cut(&r.text(), 160)
                ),
                Some("logs/gateway.log"),
            ),
            Err(e) => check("health", false, e, Some("logs/gateway.log")),
        });
    } else {
        checks.push(check(
            "health",
            false,
            format!("no /health 200 within 60 s (gateway exited: {exited:?})"),
            Some("logs/gateway.log"),
        ));
    }
    let glog_text = || std::fs::read_to_string(&gwlog).unwrap_or_default();
    if up {
        let r = http(ports.gw, "GET", "/health/detailed", k, None, to);
        checks.push(match r {
            Ok(r) => check(
                "health-detailed",
                r.status == 200 && r.json().is_object(),
                format!("GET /health/detailed {}: {}", r.status, cut(&r.text(), 200)),
                None,
            ),
            Err(e) => check("health-detailed", false, e, None),
        });
        let r = http(ports.gw, "GET", "/v1/models", k, None, to);
        checks.push(match r {
            Ok(r) => check(
                "models",
                r.status == 200 && r.json()["data"].is_array(),
                format!("GET /v1/models {}: {}", r.status, cut(&r.text(), 160)),
                None,
            ),
            Err(e) => check("models", false, e, None),
        });
        let r = http(
            ports.gw,
            "GET",
            "/v1/models",
            Some("dryrun-wrong-key-0000000000"),
            None,
            to,
        );
        checks.push(match r {
            Ok(r) => check(
                "auth-refuses-bad-key",
                r.status == 401 || r.status == 403,
                format!("wrong key -> {}", r.status),
                None,
            ),
            Err(e) => check("auth-refuses-bad-key", false, e, None),
        });
        let g = glog_text();
        let want = [
            "listening on",
            "mcp: openmind:",
            "self-modify:",
            "agent-tools: on",
        ];
        let missing: Vec<&str> = want.iter().filter(|w| !g.contains(**w)).copied().collect();
        let listen = g
            .lines()
            .find(|l| l.contains("listening on"))
            .map(|l| cut(l, 140))
            .unwrap_or_default();
        checks.push(check(
            "startup-log",
            missing.is_empty(),
            if missing.is_empty() {
                format!("all startup lines present; {listen}")
            } else {
                format!("missing startup lines: {missing:?}")
            },
            Some("logs/gateway.log"),
        ));

        // ---- 6. scripted real-task prompts --------------------------------------------
        let chat = |scn: &str, text: &str| -> (u16, String, String) {
            let body = json!({"model": "hermes-agent", "stream": false,
                "messages": [{"role": "user", "content": format!("[dryrun:{scn}:{nonce}] {text}")}]});
            match http(
                ports.gw,
                "POST",
                "/v1/chat/completions",
                k,
                Some(&body),
                Duration::from_secs(120),
            ) {
                Ok(r) => {
                    let v = r.json();
                    let c = v
                        .pointer("/choices/0/message/content")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    (r.status, c, cut(&r.text(), 300))
                }
                Err(e) => (0, String::new(), e),
            }
        };
        let tlog = Log::create(&logs.join("tasks.log"))?;
        let mut task = |name: &str,
                        scn: &str,
                        prompt: &str,
                        pass: &dyn Fn(&str) -> (bool, String)| {
            let t = Instant::now();
            let (st, content, raw) = chat(scn, prompt);
            let (ok, why) = if st == 200 {
                pass(&content)
            } else {
                (false, format!("HTTP {st}: {raw}"))
            };
            tlog.json(&json!({"check": name, "scenario": scn, "status": st, "ok": ok, "secs": t.elapsed().as_secs_f64(), "content": cut(&content, 2000)}));
            checks.push(check(
                name,
                ok,
                format!("{why} ({:.1} s)", t.elapsed().as_secs_f64()),
                Some("logs/tasks.log"),
            ));
        };
        let n2 = nonce.clone();
        task("task-ping", "ping", "reply with the dry-run pong", &|c| {
            (
                c.starts_with("DRYRUN-PONG") && c.contains(&n2),
                format!("reply: {}", cut(c, 120)),
            )
        });
        task(
            "task-clock",
            "clock",
            "what time is it? use the clock tool",
            &|c| {
                let ok = c.starts_with("DRYRUN-CLOCK")
                    && c.contains("20")
                    && !c.contains("unknown tool")
                    && !c.to_ascii_lowercase().contains("error");
                (
                    ok,
                    format!(
                        "clock tool result: {}",
                        cut(c.split(":: ").nth(1).unwrap_or(c), 160)
                    ),
                )
            },
        );
        let ws = built.workspace.clone();
        let n3 = nonce.clone();
        task(
            "task-files",
            "files",
            "write a file then read it back",
            &|c| {
                let on_disk = std::fs::read_to_string(ws.join(format!("dryrun/{n3}.txt")))
                    .unwrap_or_default();
                let ok = c.starts_with("DRYRUN-FILES")
                    && c.matches(&format!("DRYRUN-FILE-{n3}")).count() >= 1
                    && on_disk.contains(&format!("DRYRUN-FILE-{n3}"));
                (
                    ok,
                    format!(
                        "write_file+read_file; file in workspace copy: {}; reply: {}",
                        !on_disk.is_empty(),
                        cut(c.split(":: ").nth(1).unwrap_or(c), 140)
                    ),
                )
            },
        );
        let n4 = nonce.clone();
        task(
            "task-terminal",
            "terminal",
            "run echo in the terminal tool",
            &|c| {
                (
                    c.starts_with("DRYRUN-TERM")
                        && c.matches(&format!("DRYRUN-TERM-{n4}")).count() >= 1,
                    format!("terminal: {}", cut(c.split(":: ").nth(1).unwrap_or(c), 160)),
                )
            },
        );
        let vc = built.vault_canary.clone();
        task(
            "task-mcp-vault",
            "vault",
            "search the vault for the canary",
            &|c| {
                (
                    c.starts_with("DRYRUN-VAULT") && c.contains(&vc),
                    format!(
                        "mcp__openmind__vault_search found canary: {}",
                        c.contains(&vc)
                    ),
                )
            },
        );
        let mc = stubs::memory_canary(&nonce);
        let mc_short = mc[..mc.find(':').unwrap_or(mc.len())].to_string();
        task(
            "task-mcp-memory",
            "memory",
            "search memory for the canary",
            &|c| {
                (
                    c.starts_with("DRYRUN-MEMORY") && c.contains(&mc_short),
                    format!(
                        "mcp__openmind__memory_search found canary: {}",
                        c.contains(&mc_short)
                    ),
                )
            },
        );
        let dummies: Vec<String> = built.dummies.keys().cloned().collect();
        task("task-deny-secrets", "deny", "read the .env file", &|c| {
            let leaked = dummies.iter().filter(|d| c.contains(d.as_str())).count();
            (c.starts_with("DRYRUN-DENY") && leaked == 0, format!("read_file + terminal cat on the .env copy: {leaked} dummy secret(s) reached the reply"))
        });
        // tool inventory offered to the model
        let fm = std::fs::read_to_string(logs.join("fake-model.jsonl")).unwrap_or_default();
        let offered: BTreeSet<String> = fm
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter(|v| v["scenario"] == "ping")
            .flat_map(|v| v["tools_offered"].as_array().cloned().unwrap_or_default())
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        let need = [
            "clock",
            "read_file",
            "write_file",
            "terminal",
            "mcp__openmind__memory_search",
            "mcp__openmind__vault_search",
            "propose_self_change",
        ];
        let miss: Vec<&str> = need
            .iter()
            .filter(|n| !offered.contains(**n))
            .copied()
            .collect();
        checks.push(check(
            "tool-inventory",
            miss.is_empty() && !offered.is_empty(),
            format!(
                "{} tools offered to the model; required missing: {miss:?}",
                offered.len()
            ),
            Some("logs/fake-model.jsonl"),
        ));
        // leak scan: no dummy secret in anything the model was sent
        let leaked: Vec<&String> = built
            .dummies
            .iter()
            .filter(|(d, l)| {
                fm.contains(d.as_str())
                    && !l.contains("API_KEY")
                    && l.as_str() != "OPENROUTER_API_KEY"
            })
            .map(|(_, l)| l)
            .collect();
        let fm_auth: BTreeSet<String> = fm
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|v| v["auth"].as_str().map(String::from))
            .collect();
        checks.push(check(
            "model-auth-dummy",
            !fm_auth.contains("UNKNOWN-KEY") && leaked.is_empty(),
            format!("fake model saw auth labels {fm_auth:?}; dummy secrets in prompts: {leaked:?}"),
            Some("logs/fake-model.jsonl"),
        ));
        // spend
        let r = http(ports.gw, "GET", "/api/spend", k, None, to);
        checks.push(match r {
            Ok(r) => {
                let v = r.json();
                let mut nonzero = vec![];
                walk_spend(&v, "", &mut nonzero);
                check(
                    "spend-zero",
                    r.status == 200 && nonzero.is_empty(),
                    format!("/api/spend {}: nonzero spend fields {nonzero:?}", r.status),
                    None,
                )
            }
            Err(e) => check("spend-zero", false, e, None),
        });
    }

    // ---- 7. stop, then judge logs and network -----------------------------------------
    let code = if gw.child.try_wait().ok().flatten().is_none() {
        gw.stop(&log)
    } else {
        None
    };
    log.line(&format!("gateway exit code {code:?}"));
    let g = glog_text();
    let panics: Vec<&str> = g
        .lines()
        .filter(|l| l.contains("panicked at") || l.contains("RUST_BACKTRACE"))
        .collect();
    checks.push(check(
        "no-panic",
        panics.is_empty(),
        if panics.is_empty() {
            "no panic in gateway log".to_string()
        } else {
            format!("{} panic line(s): {}", panics.len(), cut(panics[0], 200))
        },
        Some("logs/gateway.log"),
    ));
    std::thread::sleep(Duration::from_millis(150));
    watch.stop.store(true, Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(60));
    let rep = watch.report.lock().unwrap();
    let viol: Vec<String> = rep.violations.keys().cloned().collect();
    let sink: Vec<String> = sink_hits.lock().unwrap().clone();
    let mut dns: Vec<String> = dns_hits.lock().unwrap().clone();
    dns.sort();
    dns.dedup();
    let net_ok = viol.is_empty()
        && canary_seen
        && canary_err != "CONNECTED"
        && sink.is_empty()
        && dns.is_empty();
    let ndetail = format!(
        "{} samples; non-loopback/unexpected sockets: {}; DNS lookups: {} {:?}; external URLs hit (sink): {}; canary 192.0.2.1:9 blocked ({}) and seen: {canary_seen}; loopback ports used: {:?}",
        rep.samples, viol.len(), dns.len(), dns, sink.len(), cut(&canary_err, 60), rep.loopback_ports
    );
    nlog.line(&ndetail);
    for v in &viol {
        nlog.line(&format!("violation: {v}"));
    }
    checks.push(check(
        "network-loopback-only",
        net_ok,
        ndetail.clone(),
        Some("logs/network.log"),
    ));
    extra["network"] = json!({"samples": rep.samples, "violations": viol, "dns_lookups": dns, "sink_hits": sink, "canary_seen": canary_seen});
    // zero real model calls: the only model endpoint reachable is the fake
    // one, and the network check proves nothing else was contacted
    let fake = model_calls.load(Ordering::SeqCst);
    extra["model_calls_fake"] = json!(fake);
    // anything that tried to leave counts as a possible real call
    extra["model_calls_real"] = json!(if viol.is_empty() && sink.is_empty() && dns.is_empty() {
        0
    } else {
        1
    });

    // ---- 8. fingerprints ----------------------------------------------------------------
    let (copy_after, _) = sha256_file(&built.gateway)?;
    let flog = Log::create(&logs.join("fingerprints.log"))?;
    let mut fps = built.fingerprints.clone();
    fps.insert(0, ("candidate (tested artifact)".into(), artifact.clone()));
    fps.insert(1, ("gateway copy before".into(), copy_sha.clone()));
    fps.insert(2, ("gateway copy after".into(), copy_after.clone()));
    for f in [
        "gateway.log",
        "fake-model.jsonl",
        "tasks.log",
        "network.log",
    ] {
        if let Ok((h, _)) = sha256_file(&logs.join(f)) {
            fps.push((format!("logs/{f}"), h));
        }
    }
    for (n, h) in &fps {
        flog.line(&format!("{h}  {n}"));
    }
    extra["fingerprints"] = json!(fps
        .iter()
        .map(|(n, h)| json!({"name": n, "sha256": h}))
        .collect::<Vec<_>>());
    checks.push(check(
        "fingerprint",
        copy_sha == artifact && copy_after == artifact,
        format!(
            "binary run == tested artifact {} (before and after the run)",
            &artifact[..16]
        ),
        Some("logs/fingerprints.log"),
    ));
    Ok(())
}

fn walk_spend(v: &Value, path: &str, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                walk_spend(x, &format!("{path}.{k}"), out);
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                walk_spend(x, &format!("{path}[{i}]"), out);
            }
        }
        Value::Number(n) => {
            let p = path.to_ascii_lowercase();
            let money = p.contains("usd")
                || p.contains("cost")
                || p.contains("spent")
                || p.contains("spend");
            let limit = ["cap", "limit", "max", "budget", "stop", "remaining", "left"]
                .iter()
                .any(|w| p.contains(w));
            if money && !limit && n.as_f64().unwrap_or(0.0) != 0.0 {
                out.push(format!("{path}={n}"));
            }
        }
        _ => {}
    }
}
