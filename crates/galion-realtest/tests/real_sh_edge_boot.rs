// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Rust port of `scripts/real/edge-boot.sh` (slot 20; deleted on
//! swarm/realscript-port-1). Real-life test for the omnode edge boot path.
//! Builds the static release binaries of `edge/openmind-node` (its own
//! workspace), then runs the REAL binary (x86_64 natively, aarch64 under
//! qemu-user when present) through:
//!   first boot -> pairing refused (no/wrong code) -> paired -> code is single
//!   use -> expired code refused -> 5 refusals lock -> seccomp enforcement with
//!   real tools (strace, unshare) -> kill -9 + restart by a minimal supervisor
//!   (identity and token survive) -> SIGTERM exit 0 -> PID-1 run in its own
//!   PID namespace -> offline `pair verify` accept/refuse,
//! and finally the crate's own gated `tests/real_edge_boot.rs`.
//! No systemd: the node runs directly / under the small supervisor thread
//! below. Pairing codes and tokens are never printed.
//!
//! Ports: block 19200 (`GALION_REAL_PORT_BASE`): +0 x86_64 main node,
//! +1 x86_64 expiry/lockout node, +2 x86_64 PID-1 node, +3 aarch64 main node
//! (qemu), +4 aarch64 expiry node, +5/+6 `real_edge_boot.rs`.
//! `GALION_REAL_NO_QEMU=1` skips the aarch64 suite.
//! Run: `GALION_REAL=1 cargo test -p galion-realtest --test real_sh_edge_boot -- --nocapture`

use galion_realtest::http;
use galion_realtest::script::*;
use galion_realtest::sha256;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const URL: &str = "https://hermes.example.invalid/openmind/enroll";

/// bash-style exit code: the code, or 128 + signal.
fn sh_code(st: ExitStatus) -> i32 {
    st.code().unwrap_or_else(|| 128 + st.signal().unwrap_or(0))
}

/// (code as text, "000" on connection failure; body JSON)
fn req(method: &str, port: u16, path: &str, body: &str, bearer: &str) -> (String, Value) {
    let mut h: Vec<(&str, String)> = Vec::new();
    if !body.is_empty() {
        h.push(("content-type", "application/json".into()));
    }
    if !bearer.is_empty() {
        h.push(("authorization", format!("Bearer {bearer}")));
    }
    let hr: Vec<(&str, &str)> = h.iter().map(|(k, v)| (*k, v.as_str())).collect();
    match http::request(
        &format!("127.0.0.1:{port}"),
        method,
        path,
        &hr,
        body.as_bytes(),
        Duration::from_secs(5),
    ) {
        Ok(r) => (r.status.to_string(), r.json().unwrap_or(Value::Null)),
        Err(_) => ("000".into(), Value::Null),
    }
}

/// Poll GET /v1/node (100 x 0.2 s) until 200 and, if given, boot_count.
fn wait_health(port: u16, want_boot: Option<u64>) -> Option<Value> {
    for _ in 0..100 {
        let (c, b) = req("GET", port, "/v1/node", "", "");
        if c == "200" && want_boot.is_none_or(|w| b["boot_count"].as_u64() == Some(w)) {
            return Some(b);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    None
}

struct Runner {
    prefix: Option<PathBuf>,
    bin: PathBuf,
}

impl Runner {
    fn cmd(&self) -> Command {
        match &self.prefix {
            Some(p) => {
                let mut c = Command::new(p);
                c.arg(&self.bin);
                c
            }
            None => Command::new(&self.bin),
        }
    }
    fn out(&self, args: &[&str], xdg: Option<&Path>) -> std::process::Output {
        let mut c = self.cmd();
        c.args(args);
        if let Some(x) = xdg {
            c.env("XDG_CONFIG_HOME", x);
        }
        run(c)
    }
}

/// The small supervisor: restart the node until the keep file is removed.
struct Sup {
    keep: PathBuf,
    current: Arc<Mutex<Option<u32>>>,
    handle: Option<JoinHandle<()>>,
}

impl Sup {
    fn start(keep: &Path, log: &Path, mut make: impl FnMut() -> Command + Send + 'static) -> Sup {
        fs::write(keep, "").unwrap();
        let current = Arc::new(Mutex::new(None));
        let (k, l, cur) = (keep.to_path_buf(), log.to_path_buf(), current.clone());
        let handle = std::thread::spawn(move || {
            let app = |ext: &str| {
                fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(format!("{}.{ext}", l.display()))
                    .unwrap()
            };
            while k.exists() {
                let mut c = make();
                c.stdin(Stdio::null()).stdout(app("out")).stderr(app("err"));
                let Ok(mut child) = c.spawn() else { break };
                *cur.lock().unwrap() = Some(child.id());
                let st = child.wait().unwrap();
                *cur.lock().unwrap() = None;
                writeln!(app("exits"), "exit={}", sh_code(st)).unwrap();
                if k.exists() {
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        });
        Sup {
            keep: keep.to_path_buf(),
            current,
            handle: Some(handle),
        }
    }

    /// Wait at most 10 s for the supervisor to end, then kill what runs.
    fn wait(&mut self) -> bool {
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            if self.handle.as_ref().is_none_or(|h| h.is_finished()) {
                if let Some(h) = self.handle.take() {
                    let _ = h.join();
                }
                return true;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        false
    }
}

impl Drop for Sup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.keep);
        if let Some(p) = *self.current.lock().unwrap() {
            signal(p, "TERM");
        }
        if !self.wait() {
            if let Some(p) = *self.current.lock().unwrap() {
                signal(p, "KILL");
            }
            self.wait();
        }
    }
}

fn binary_facts(bin: &Path) {
    let rh = out(&run({
        let mut c = Command::new("readelf");
        c.arg("-h").arg(bin);
        c
    }));
    let machine = rh
        .lines()
        .find_map(|l| l.trim().strip_prefix("Machine:"))
        .unwrap_or("?")
        .trim()
        .to_string();
    let rd = out(&run({
        let mut c = Command::new("readelf");
        c.arg("-d").arg(bin);
        c
    }));
    let rl = out(&run({
        let mut c = Command::new("readelf");
        c.arg("-l").arg(bin);
        c
    }));
    let data = fs::read(bin).unwrap();
    println!("  machine: {machine}");
    println!(
        "  NEEDED: {}  INTERP: {}",
        rd.matches("NEEDED").count(),
        rl.matches("program interpreter").count()
    );
    println!(
        "  size: {}  sha256: {}",
        data.len(),
        &sha256::hex(&sha256::digest(&data))[..16]
    );
}

fn first_line_json(p: &Path, n: usize) -> Value {
    json(read(p).lines().nth(n).unwrap_or(""))
}

fn flip_last(s: &str) -> String {
    let last = if s.ends_with('0') { '1' } else { '0' };
    format!("{}{last}", &s[..s.len().saturating_sub(1)])
}

#[allow(clippy::too_many_arguments)]
fn suite(
    ck: &mut Checks,
    work: &Path,
    label: &str,
    r: Runner,
    port: u16,
    port2: u16,
    enforced: bool,
) {
    let r = Arc::new(r);
    let xdg = work.join(format!("xdg-{label}"));
    let state = xdg.join("openmind");
    let cf = work.join(format!("code-{label}"));
    fs::create_dir_all(&xdg).unwrap();
    say(&format!("== {label}"));
    binary_facts(&r.bin);
    println!(
        "  version: {}",
        out(&r.out(&["version"], Some(&xdg))).trim()
    );

    // -- seccomp gate (fail-closed when not enforceable)
    let mut req_sec: Vec<&str> = Vec::new();
    if enforced {
        let o = r.out(&["sandbox", "selftest"], None);
        let v = json(&out(&o));
        ck.eq(
            &format!("{label} sandbox selftest exit"),
            "0",
            code(&o).to_string(),
        );
        ck.eq(
            &format!("{label} selftest enforced"),
            "true",
            jt(&v, "enforced"),
        );
        ck.eq(
            &format!("{label} fsconfig(431) denied"),
            "true",
            jt(&v, "fsconfig_denied"),
        );
        let sb = |extra: &[&str], tail_args: &[&str]| -> std::process::Output {
            let mut c = r.cmd();
            c.current_dir(work)
                .args(["sandbox", "run", "--wall", "10"])
                .args(extra)
                .arg("--")
                .args(tail_args);
            run(c)
        };
        let s1 = sb(&[], &["strace", "-f", "-o", "/dev/null", "true"]);
        let s2 = sb(
            &["--no-seccomp"],
            &["strace", "-f", "-o", "/dev/null", "true"],
        );
        let s3 = sb(&[], &["unshare", "-U", "true"]);
        let s4 = sb(&[], &["sh", "-c", "echo hello >out.txt && cat out.txt"]);
        let all = |o: &std::process::Output| format!("{}{}", out(o), err(o));
        ck.ok(
            &format!("{label} strace under seccomp refused (EPERM)"),
            code(&s1) != 0 && all(&s1).contains("Operation not permitted"),
            &format!("rc {}", code(&s1)),
        );
        ck.eq(
            &format!("{label} strace with --no-seccomp (control)"),
            "0",
            code(&s2).to_string(),
        );
        ck.ok(
            &format!("{label} unshare -U under seccomp refused (EPERM)"),
            code(&s3) != 0 && all(&s3).contains("Operation not permitted"),
            &format!("rc {}", code(&s3)),
        );
        ck.eq(
            &format!("{label} everyday shell work allowed"),
            "0",
            code(&s4).to_string(),
        );
        ck.has(
            &format!("{label} sandbox report seccomp=deny-list"),
            "\"seccomp\": \"deny-list\"",
            &all(&s4),
        );
        req_sec.push("--require-seccomp");
    } else {
        let o = r.out(&["sandbox", "selftest"], None);
        let v = json(&out(&o));
        ck.eq(
            &format!("{label} sandbox selftest refuses (not enforceable here)"),
            "2",
            code(&o).to_string(),
        );
        ck.eq(
            &format!("{label} selftest enforced"),
            "false",
            jt(&v, "enforced"),
        );
        let rc_code = work.join(format!("refuse-code-{label}"));
        let o = r.out(
            &[
                "boot",
                "--state",
                work.join(format!("refuse-{label}")).to_str().unwrap(),
                "--listen",
                &format!("127.0.0.1:{port}"),
                "--require-seccomp",
                "--code-file",
                rc_code.to_str().unwrap(),
            ],
            Some(&xdg),
        );
        ck.eq(
            &format!("{label} boot --require-seccomp refuses"),
            "2",
            code(&o).to_string(),
        );
        ck.has(
            &format!("{label} refusal reason: not enforced"),
            "not enforced",
            &err(&o),
        );
        ck.ok(
            &format!("{label} no pairing code opened when refused"),
            !rc_code.exists(),
            "",
        );
    }

    // -- first boot under the supervisor
    let keep = work.join(format!("keep-{label}"));
    let log = work.join(format!("log-{label}"));
    let logf = |ext: &str| PathBuf::from(format!("{}.{ext}", log.display()));
    let boot_args: Vec<String> = [
        "boot",
        "--state",
        state.to_str().unwrap(),
        "--listen",
        &format!("127.0.0.1:{port}"),
        "--code-file",
        cf.to_str().unwrap(),
        "--code-ttl",
        "300",
    ]
    .iter()
    .map(|s| s.to_string())
    .chain(req_sec.iter().map(|s| s.to_string()))
    .collect();
    let (r2, xdg2) = (r.clone(), xdg.clone());
    let mut sup = Sup::start(&keep, &log, move || {
        let mut c = r2.cmd();
        c.env("XDG_CONFIG_HOME", &xdg2).args(&boot_args);
        c
    });
    let Some(node) = wait_health(port, Some(1)) else {
        ck.ok(
            &format!("{label} first boot listening on :{port}"),
            false,
            &tail(&logf("err"), 20),
        );
        return;
    };
    ck.ok(
        &format!("{label} first boot listening on :{port}"),
        true,
        "",
    );
    let ev = first_line_json(&logf("out"), 0);
    ck.eq(
        &format!("{label} boot event first_boot"),
        "true",
        jt(&ev, "first_boot"),
    );
    ck.eq(
        &format!("{label} boot event seccomp.enforced"),
        enforced.to_string(),
        jt(&ev, "seccomp.enforced"),
    );
    let dev = jt(&node, "device_id");
    let pid1 = node["pid"].as_u64().unwrap_or(0) as u32;
    println!(
        "  device_id {dev}  fingerprint {}  first_boot {}",
        jt(&node, "fingerprint"),
        jt(&node, "first_boot")
    );
    ck.eq(
        &format!("{label} key file mode"),
        "600",
        mode(&state.join("node.key")),
    );
    ck.eq(
        &format!("{label} pair store mode"),
        "600",
        mode(&state.join("pair-store.json")),
    );
    ck.eq(&format!("{label} code file mode"), "600", mode(&cf));
    let thecode = read(&cf).trim().to_string();
    ck.ok(
        &format!("{label} store holds only the code hash"),
        !thecode.is_empty() && !read(&state.join("pair-store.json")).contains(&thecode),
        "",
    );

    // -- pairing
    let pair = |p: u16, c: &str| req("POST", p, "/v1/pair", &format!("{{\"code\":\"{c}\"}}"), "");
    ck.eq(
        &format!("{label} whoami unpaired"),
        "401",
        req("GET", port, "/v1/whoami", "", "").0,
    );
    let (c, b) = pair(port, "AAAA-AAAA");
    ck.eq(
        &format!("{label} wrong code"),
        "403 no",
        format!("{c} {}", jt(&b, "error")),
    );
    ck.eq(
        &format!("{label} malformed body"),
        "400",
        req("POST", port, "/v1/pair", "not json", "").0,
    );
    let (c, b) = pair(port, &thecode.to_lowercase());
    ck.eq(
        &format!("{label} right code (lower-case)"),
        "200 true",
        format!("{c} {}", jt(&b, "ok")),
    );
    let tok = jt(&b, "token");
    println!("  token: <redacted, {} hex chars>", tok.len());
    let (c, b) = req("GET", port, "/v1/whoami", "", &tok);
    ck.eq(
        &format!("{label} whoami with token"),
        format!("200 {dev}"),
        format!("{c} {}", jt(&b, "device_id")),
    );
    ck.eq(
        &format!("{label} whoami with wrong token"),
        "401",
        req("GET", port, "/v1/whoami", "", &flip_last(&tok)).0,
    );
    let (c, b) = pair(port, &thecode);
    ck.eq(
        &format!("{label} code is single use"),
        "403 no",
        format!("{c} {}", jt(&b, "error")),
    );
    let leaked = [state.join("pair-store.json"), logf("out"), logf("err")]
        .iter()
        .any(|p| read(p).contains(&tok));
    ck.ok(
        &format!("{label} token not on disk or in logs"),
        tok.len() > 8 && !leaked,
        "",
    );

    // -- restart: kill -9, supervisor brings it back
    signal(pid1, "KILL");
    let node2 = wait_health(port, Some(2));
    ck.ok(
        &format!("{label} restarted by supervisor after kill -9"),
        node2.is_some(),
        "",
    );
    let node2 = node2.unwrap_or(Value::Null);
    ck.eq(
        &format!("{label} same device after restart"),
        &dev,
        jt(&node2, "device_id"),
    );
    let pid2 = node2["pid"].as_u64().unwrap_or(0) as u32;
    ck.eq(
        &format!("{label} new pid after restart"),
        "yes",
        if pid2 != pid1 { "yes" } else { "no" },
    );
    let at16: String = jt(&ev, "at").chars().take(16).collect();
    let fb16: String = jt(&node2, "first_boot").chars().take(16).collect();
    ck.eq(&format!("{label} first_boot timestamp kept"), at16, fb16);
    let ev2 = first_line_json(&logf("out"), 1);
    ck.eq(
        &format!("{label} boot event on restart"),
        "false 2 null",
        format!(
            "{} {} {}",
            jt(&ev2, "first_boot"),
            jt(&ev2, "boot_count"),
            jt(&ev2, "pairing_open_until")
        ),
    );
    ck.eq(
        &format!("{label} token survives restart"),
        "200",
        req("GET", port, "/v1/whoami", "", &tok).0,
    );
    let (c, b) = pair(port, &thecode);
    ck.eq(
        &format!("{label} old code still refused after restart"),
        "403 no",
        format!("{c} {}", jt(&b, "error")),
    );

    // -- offline pair verify against the same identity
    let dry = json(&out(&r.out(&["pair", "--dry-run", URL], Some(&xdg))));
    let pfile = work.join(format!("pair-{label}.json"));
    fs::write(&pfile, dry["would_send"]["body"].to_string()).unwrap();
    let mut tampered = dry["would_send"]["body"].clone();
    if let Some(caps) = tampered["payload"]["capabilities"].as_array_mut() {
        caps.push("money".into());
    }
    let tfile = work.join("pair-t.json");
    fs::write(&tfile, tampered.to_string()).unwrap();
    let later = (std::time::SystemTime::now() // repo-check: allow (unix secs for pair verify --now)
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600)
        .to_string();
    let pf = pfile.to_str().unwrap();
    let v1 = r.out(&["pair", "verify", pf, "--url", URL], None);
    let v2 = r.out(&["pair", "verify", pf, "--url", URL, "--now", &later], None);
    let v3 = r.out(
        &[
            "pair",
            "verify",
            pf,
            "--url",
            "https://other.example.invalid/enroll",
        ],
        None,
    );
    let v4 = r.out(
        &["pair", "verify", tfile.to_str().unwrap(), "--url", URL],
        None,
    );
    let j1 = json(&out(&v1));
    ck.eq(
        &format!("{label} pair verify fresh payload"),
        format!("0 true {dev}"),
        format!(
            "{} {} {}",
            code(&v1),
            jt(&j1, "ok"),
            jt(&j1, "verified.device_id")
        ),
    );
    let has = |o: &std::process::Output, w: &str| {
        if jt(&json(&out(o)), "error").contains(w) {
            w.to_string()
        } else {
            String::new()
        }
    };
    ck.eq(
        &format!("{label} pair verify expired payload"),
        "2 expired",
        format!("{} {}", code(&v2), has(&v2, "expired")),
    );
    ck.eq(
        &format!("{label} pair verify other server"),
        "2 enroll_url",
        format!("{} {}", code(&v3), has(&v3, "enroll_url")),
    );
    ck.eq(
        &format!("{label} pair verify tampered caps"),
        "2 signature",
        format!("{} {}", code(&v4), has(&v4, "signature")),
    );

    // -- SIGTERM: clean exit 0, supervisor stops
    let _ = fs::remove_file(&keep);
    signal(pid2, "TERM");
    if !sup.wait() {
        ck.ok(
            &format!("{label} supervisor stopped within 10 s"),
            false,
            "killed",
        );
    }
    drop(sup);
    let exits = read(&logf("exits"))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    ck.eq(
        &format!("{label} exits (kill -9, then SIGTERM)"),
        "exit=137 exit=0",
        exits,
    );
    ck.eq(
        &format!("{label} port closed after stop"),
        "000",
        req("GET", port, "/health", "", "").0,
    );

    // -- expired code and lockout on a second node
    let xdg2 = work.join(format!("xdg2-{label}"));
    let cf2 = work.join(format!("code2-{label}"));
    fs::create_dir_all(&xdg2).unwrap();
    let mut c = r.cmd();
    c.env("XDG_CONFIG_HOME", &xdg2)
        .args([
            "boot",
            "--state",
            xdg2.join("openmind").to_str().unwrap(),
            "--listen",
            &format!("127.0.0.1:{port2}"),
        ])
        .args(["--code-file", cf2.to_str().unwrap(), "--code-ttl", "1"])
        .stdin(Stdio::null())
        .stdout(fs::File::create(work.join("n2.out")).unwrap())
        .stderr(fs::File::create(work.join("n2.err")).unwrap());
    let mut n2 = c.spawn().expect("start node 2");
    if wait_health(port2, Some(1)).is_none() {
        ck.ok(&format!("{label} node 2 came up"), false, "");
    }
    std::thread::sleep(Duration::from_millis(2200));
    let (c2, b) = pair(port2, read(&cf2).trim());
    ck.eq(
        &format!("{label} expired code refused"),
        "403 expired",
        format!("{c2} {}", jt(&b, "error")),
    );
    let mut last = (String::new(), Value::Null);
    for _ in 0..4 {
        last = pair(port2, "BBBB-BBBB");
    }
    ck.eq(
        &format!("{label} 5th refusal"),
        "403 no",
        format!("{} {}", last.0, jt(&last.1, "error")),
    );
    let (c3, b) = pair(port2, "CCCC-CCCC");
    ck.eq(
        &format!("{label} locked after 5 refusals"),
        "429 later",
        format!("{c3} {}", jt(&b, "error")),
    );
    ck.eq(
        &format!("{label} fails counted in store"),
        "5",
        jt(
            &json(&read(&xdg2.join("openmind/pair-store.json"))),
            "fails",
        ),
    );
    signal(n2.id(), "TERM");
    let st = wait_exit(&mut n2, Duration::from_secs(10))
        .unwrap_or_else(|| term_wait(&mut n2, Duration::from_secs(1)));
    ck.eq(
        &format!("{label} node 2 SIGTERM exit"),
        "0",
        sh_code(st).to_string(),
    );
}

fn pid1_suite(ck: &mut Checks, work: &Path, x86: &Path, port: u16) {
    say("== x86_64 as PID 1 (unshare --pid --fork)");
    let probe = run({
        let mut c = Command::new("unshare");
        c.args([
            "--user",
            "--map-root-user",
            "--pid",
            "--fork",
            "--mount-proc",
            "true",
        ]);
        c
    });
    if !probe.status.success() {
        println!("  SKIP: unprivileged PID namespaces unavailable");
        return;
    }
    let xdg = work.join("xdg-pid1");
    fs::create_dir_all(&xdg).unwrap();
    let mut c = Command::new("unshare");
    c.args([
        "--user",
        "--map-root-user",
        "--pid",
        "--fork",
        "--mount-proc",
    ])
    .arg(x86)
    .args([
        "boot",
        "--state",
        xdg.join("openmind").to_str().unwrap(),
        "--listen",
        &format!("127.0.0.1:{port}"),
    ])
    .args(["--code-file", work.join("code-pid1").to_str().unwrap()])
    .env("XDG_CONFIG_HOME", &xdg)
    .stdin(Stdio::null())
    .stdout(fs::File::create(work.join("p1.out")).unwrap())
    .stderr(fs::File::create(work.join("p1.err")).unwrap());
    let mut u = c.spawn().expect("unshare");
    let Some(node) = wait_health(port, Some(1)) else {
        ck.ok("PID-1 node up", false, &tail(&work.join("p1.err"), 20));
        term_wait(&mut u, Duration::from_secs(2));
        return;
    };
    ck.ok("PID-1 node up", true, "");
    ck.eq("PID-1 node sees itself as pid", "1", jt(&node, "pid"));
    // The forked child of unshare is the node (pid 1 inside, a host pid outside).
    let kids = out(&run({
        let mut c = Command::new("pgrep");
        c.args(["-P", &u.id().to_string(), "-f", "omnode boot"]);
        c
    }));
    let hostpid: u32 = kids
        .lines()
        .next()
        .and_then(|l| l.trim().parse().ok())
        .unwrap_or(0);
    ck.ok(
        "PID-1 node host pid found",
        hostpid != 0,
        &hostpid.to_string(),
    );
    signal(hostpid, "TERM");
    let st = wait_exit(&mut u, Duration::from_secs(10))
        .unwrap_or_else(|| term_wait(&mut u, Duration::from_secs(1)));
    ck.eq(
        "PID-1 node exits 0 on SIGTERM (handler installed)",
        "0",
        sh_code(st).to_string(),
    );
    ck.has(
        "PID-1 stop message",
        "stopping (signal)",
        &read(&work.join("p1.err")),
    );
}

#[test]
fn real_sh_edge_boot() {
    if !enabled("edge-boot") {
        return;
    }
    let b = port_base(19200);
    let krate = repo_root().join("edge/openmind-node");
    build_in(&krate, &["--target", "x86_64-unknown-linux-musl"], &[]);
    build_in(
        &krate,
        &["--target", "aarch64-unknown-linux-musl"],
        &[("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER", "rust-lld")],
    );
    let x86 = need_bin(&target_dir().join("x86_64-unknown-linux-musl/release/omnode"));
    let arm = need_bin(&target_dir().join("aarch64-unknown-linux-musl/release/omnode"));
    require_free(&[b, b + 1, b + 2, b + 3, b + 4, b + 5, b + 6]);
    let work = TempDir(scratch("real-edge-boot"));
    let mut ck = Checks::new();
    let kernel = out(&run({
        let mut c = Command::new("uname");
        c.arg("-r");
        c
    }));
    say(&format!(
        "omnode edge-boot real-life test, kernel {}, PID 1 on this box: {}",
        kernel.trim(),
        read(Path::new("/proc/1/comm")).trim()
    ));

    suite(
        &mut ck,
        &work.0,
        "x86_64",
        Runner {
            prefix: None,
            bin: x86.clone(),
        },
        b,
        b + 1,
        true,
    );
    pid1_suite(&mut ck, &work.0, &x86, b + 2);
    let qemu = Path::new("/usr/bin/qemu-aarch64");
    if std::env::var("GALION_REAL_NO_QEMU").as_deref() != Ok("1") && qemu.exists() {
        suite(
            &mut ck,
            &work.0,
            "aarch64-qemu",
            Runner {
                prefix: Some(qemu.into()),
                bin: arm,
            },
            b + 3,
            b + 4,
            false,
        );
    } else {
        say("== aarch64: SKIP (no qemu-aarch64 or GALION_REAL_NO_QEMU=1)");
    }

    say(&format!(
        "== cargo real test (GALION_REAL=1, x86_64 release binary, ports {}-{})",
        b + 5,
        b + 6
    ));
    let jobs = std::env::var("GALION_REAL_JOBS").unwrap_or_else(|_| "2".into());
    let o = run({
        let mut c = Command::new("nice");
        c.args(["-n", "10"])
            .arg(cargo())
            .args(["test", "-q", "-j", &jobs, "--test", "real_edge_boot"])
            .current_dir(&krate)
            .env("CARGO_INCREMENTAL", "0")
            .env("GALION_REAL", "1")
            .env("GALION_REAL_PORT_BASE", b.to_string())
            .env("OMNODE_BIN", &x86);
        c
    });
    for l in format!("{}{}", out(&o), err(&o))
        .lines()
        .filter(|l| l.starts_with("test result") || l.contains("panicked"))
    {
        println!("  {l}");
    }
    ck.ok(
        "tests/real_edge_boot.rs",
        o.status.success(),
        &format!("exit {}", code(&o)),
    );
    for p in b..=b + 6 {
        ck.ok(&format!("port {p} closed at the end"), !port_open(p), "");
    }
    ck.finish("edge-boot");
}
