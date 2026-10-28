// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for Hermes device enrollment (GALION_REAL=1). Rust port of the
//! former scripts/real/hermes-enroll.sh (Rust-only rule, 2026-10-20 20:41 CEST).
//!
//! Uses the REAL release `hermes-gateway` binary built by cargo for this test
//! (`--mock`: offline provider, no network, no spend), builds the REAL `omnode`
//! (edge/openmind-node, its own workspace) in release mode, and runs two omnode
//! nodes (`omnode boot`, own state dirs; node B is aarch64 musl under
//! qemu-aarch64 when available). Walks:
//!   route off by default (404) -> unsafe enroll config refuses to start ->
//!   node A enrolls (202 pending) -> claim before approval = pending, no token ->
//!   wrong fingerprint refused -> approve -> A claims a scoped credential ->
//!   Hermes accepts A, scope check -> refuses replayed nonce, expired payload,
//!   tampered payloads, unenrolled node (forged token, rejected claim) ->
//!   node B enrolls too -> revoke A -> A refused, B unaffected -> restart
//!   Hermes: revocation, B's credential and the nonce store persist ->
//!   corrupt / too-open store refuses to start.
//!
//! Ports (wave-4 slot 102, range 19720-19729), loopback only:
//!   19720 hermes-gateway   19721 omnode A (x86_64)   19722 omnode B
//!   19723 hermes fail-closed start probes (never binds when refusing)
//!
//! The throwaway owner key and the node tokens stay in this process's memory;
//! they are never printed, never put in argv, and the test asserts they do not
//! appear in omnode output, the Hermes logs, the store or the device list.
//!
//! `GALION_REAL=1 cargo test --release -p hermes-gateway --test real_enroll -- --nocapture`
//! Env: `OMNODE_BIN=<path>` skips the omnode build; `NO_QEMU=1` runs B as x86_64.

use serde_json::Value;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HERMES: &str = env!("CARGO_BIN_EXE_hermes-gateway");
use galion_realtest::ports::Block;

/// Slot 383 (real-ports-batch-3): PPROBE +3, PA +1, PB +2, P +0; `HERMES_ENROLL_REAL_PORT_BASE`
/// moves the block, unset keeps the old 19720-19729.
static PORTS: Block = Block::new("HERMES_ENROLL_REAL_PORT_BASE", 19720, 10);

#[allow(non_snake_case)]
fn P() -> u16 {
    PORTS.port(0)
}
#[allow(non_snake_case)]
fn PA() -> u16 {
    PORTS.port(1)
}
#[allow(non_snake_case)]
fn PB() -> u16 {
    PORTS.port(2)
}
#[allow(non_snake_case)]
fn PPROBE() -> u16 {
    PORTS.port(3)
}

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn ts() -> String {
    Command::new("date")
        .arg("+%Y-%m-%dT%H:%M:%S%:z")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn rand_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .expect("read /dev/urandom");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn mode(p: &Path) -> String {
    match fs::metadata(p) {
        Ok(m) => format!("{:o}", m.permissions().mode() & 0o777),
        Err(_) => "missing".into(),
    }
}

fn port_free(port: u16) -> bool {
    let t0 = Instant::now();
    loop {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return true;
        }
        if t0.elapsed() > Duration::from_secs(10) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Minimal HTTP/1.1 client (Connection: close; Content-Length or chunked).
/// Returns (status, body); status 0 when the connection failed.
fn http(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&str>,
    bearer: Option<&str>,
) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
    let mut req =
        format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n");
    if let Some(t) = bearer {
        req.push_str(&format!("Authorization: Bearer {t}\r\n"));
    }
    let b = body.unwrap_or("");
    if body.is_some() {
        req.push_str("Content-Type: application/json\r\n");
    }
    req.push_str(&format!("Content-Length: {}\r\n\r\n{b}", b.len()));
    if s.write_all(req.as_bytes()).is_err() {
        return (0, String::new());
    }
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let Some(split) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return (0, String::new());
    };
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let mut rest = raw[split + 4..].to_vec();
    let code = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let lower = head.to_ascii_lowercase();
    if lower.contains("transfer-encoding: chunked") {
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(nl) = rest[i..].windows(2).position(|w| w == b"\r\n") {
            let len_s = String::from_utf8_lossy(&rest[i..i + nl]).to_string();
            let len = usize::from_str_radix(len_s.split(';').next().unwrap_or("0").trim(), 16)
                .unwrap_or(0);
            i += nl + 2;
            if len == 0 || i + len > rest.len() {
                break;
            }
            out.extend_from_slice(&rest[i..i + len]);
            i += len + 2;
        }
        rest = out;
    }
    (code, String::from_utf8_lossy(&rest).to_string())
}

fn js(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or(Value::Null)
}

fn sv(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        o => o.to_string(),
    }
}

fn join(v: &Value) -> String {
    v.as_array()
        .map(|a| a.iter().map(sv).collect::<Vec<_>>().join(","))
        .unwrap_or_default()
}

struct T {
    pass: u32,
    fail: u32,
}

impl T {
    fn ok(&mut self, m: &str) {
        self.pass += 1;
        println!("  PASS {m}");
    }
    fn no(&mut self, m: &str) {
        self.fail += 1;
        println!("  FAIL {m}");
    }
    fn check(&mut self, label: &str, want: impl AsRef<str>, got: impl AsRef<str>) {
        let (w, g) = (want.as_ref(), got.as_ref());
        if w == g {
            self.ok(&format!("{label} ({g})"));
        } else {
            self.no(&format!("{label}: want {w}, got {g}"));
        }
    }
}

/// Owns every started process and the work dir; tears both down on drop.
struct Env {
    work: PathBuf,
    key: String,
    url: String,
    procs: Vec<Child>,
    hermes: Option<Child>,
}

impl Drop for Env {
    fn drop(&mut self) {
        self.stop_hermes();
        for c in &mut self.procs {
            let _ = c.kill();
            let _ = c.wait();
        }
        let _ = fs::remove_dir_all(&self.work);
    }
}

impl Env {
    fn hermes_cmd(&self, port: u16, extra: &[(&str, String)], log: &Path) -> Command {
        let mut c = Command::new(HERMES);
        let home = self.work.join("hermes");
        c.env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", &home)
            .env("HERMES_HOME", &home)
            .env("HERMES_RS_HOST", "127.0.0.1")
            .env("HERMES_RS_PORT", port.to_string())
            .env("HERMES_RS_MCP", "0")
            .env("API_SERVER_KEY", &self.key)
            .arg("--mock");
        for (k, v) in extra {
            c.env(k, v);
        }
        let f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .expect("log");
        c.stdout(Stdio::from(f.try_clone().expect("log")))
            .stderr(Stdio::from(f));
        c
    }

    fn enr(&self) -> Vec<(&'static str, String)> {
        vec![
            ("HERMES_RS_ENROLL", "1".into()),
            ("HERMES_RS_ENROLL_URL", self.url.clone()),
            ("HERMES_RS_ENROLL_RATE_PER_MIN", "200".into()),
        ]
    }

    fn start_hermes(&mut self, t: &mut T, log: &str, extra: &[(&str, String)]) {
        assert!(port_free(P()), "port {} busy", P());
        let child = self
            .hermes_cmd(P(), extra, &self.work.join(log))
            .spawn()
            .expect("spawn hermes-gateway");
        self.hermes = Some(child);
        for _ in 0..100 {
            if http(P(), "GET", "/health", None, None).0 == 200 {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        t.no(&format!("hermes did not come up ({log})"));
    }

    fn stop_hermes(&mut self) {
        if let Some(mut c) = self.hermes.take() {
            let _ = Command::new("kill")
                .arg("-TERM")
                .arg(c.id().to_string())
                .status();
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_secs(5) {
                if let Ok(Some(_)) = c.try_wait() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// Start hermes on the probe port; it must exit 2 before binding and log `want`.
    fn probe_start(&mut self, t: &mut T, label: &str, want: &str, extra: &[(&str, String)]) {
        let log = self.work.join("probe.log");
        let _ = fs::remove_file(&log);
        let mut c = self
            .hermes_cmd(PPROBE(), extra, &log)
            .spawn()
            .expect("spawn probe");
        let t0 = Instant::now();
        let rc = loop {
            if let Ok(Some(st)) = c.try_wait() {
                break st.code().unwrap_or(-1);
            }
            if t0.elapsed() > Duration::from_secs(10) {
                let _ = c.kill();
                let _ = c.wait();
                break 124;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let text = fs::read_to_string(&log).unwrap_or_default();
        if rc == 2 && text.contains(want) {
            t.ok(&format!("{label}: exit 2, \"{want}\""));
        } else {
            t.no(&format!(
                "{label}: rc={rc} {}",
                text.lines().last().unwrap_or("")
            ));
        }
    }
}

/// One omnode node: binary (+ optional qemu runner) and its XDG config dir.
struct Node {
    run: Vec<PathBuf>,
    xdg: PathBuf,
    state: PathBuf,
}

struct Out {
    rc: i32,
    out: String,
    err: String,
}

impl Node {
    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(&self.run[0]);
        c.args(&self.run[1..])
            .args(args)
            .env("XDG_CONFIG_HOME", &self.xdg);
        c
    }
    fn run(&self, args: &[&str]) -> Out {
        let o = self.cmd(args).output().expect("run omnode");
        Out {
            rc: o.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&o.stdout).into(),
            err: String::from_utf8_lossy(&o.stderr).into(),
        }
    }
    fn st(&self) -> String {
        self.state.display().to_string()
    }
    fn fingerprint(&self) -> String {
        let o = self.run(&["identity", "show"]);
        o.out
            .lines()
            .find(|l| l.starts_with("fingerprint"))
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("")
            .to_string()
    }
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Builds omnode (release) for `target` (None = host) in its own target dir.
fn build_omnode(target: Option<&str>) -> Option<PathBuf> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let tdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("omnode-target");
    let mut c = Command::new(cargo);
    c.current_dir(repo().join("edge/openmind-node"))
        .args(["build", "-q", "--release", "-j", "2"])
        .env("CARGO_TARGET_DIR", &tdir)
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER", "rust-lld")
        .env_remove("RUSTFLAGS");
    if let Some(t) = target {
        c.args(["--target", t]);
    }
    let st = c.status().ok()?;
    if !st.success() {
        return None;
    }
    let p = match target {
        Some(t) => tdir.join(t).join("release/omnode"),
        None => tdir.join("release/omnode"),
    };
    p.exists().then_some(p)
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::var("PATH")
        .ok()?
        .split(':')
        .map(|d| Path::new(d).join(name))
        .find(|p| p.is_file())
}

#[test]
fn real_enroll() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1 for the hermes-enroll real-life test");
        return;
    }
    for p in [P(), PA(), PB(), PPROBE()] {
        assert!(
            (PORTS.base()..=PORTS.port(9)).contains(&p),
            "{} block is {} only",
            PORTS.var(),
            PORTS.range()
        );
        assert!(port_free(p), "port {p} busy before start");
    }
    let omx = match std::env::var("OMNODE_BIN") {
        Ok(p) => PathBuf::from(p),
        Err(_) => build_omnode(None).expect("build omnode (host, release)"),
    };
    let qemu = if std::env::var("NO_QEMU").as_deref() == Ok("1") {
        None
    } else {
        which("qemu-aarch64-static").or_else(|| which("qemu-aarch64"))
    };
    let (run_b, label_b) =
        match qemu.and_then(|q| build_omnode(Some("aarch64-unknown-linux-musl")).map(|a| (q, a))) {
            Some((q, a)) => (vec![q, a], "aarch64 (qemu-user)"),
            None => (vec![omx.clone()], "x86_64"),
        };

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("real-hermes-enroll.{}.{nanos}", std::process::id()));
    fs::create_dir_all(&work).unwrap();
    fs::set_permissions(&work, fs::Permissions::from_mode(0o700)).unwrap();
    for d in ["hermes", "a", "b"] {
        fs::create_dir_all(work.join(d)).unwrap();
    }
    let url = format!("http://127.0.0.1:{}/openmind/enroll", P());
    let mut env = Env {
        work: work.clone(),
        key: rand_hex(24),
        url: url.clone(),
        procs: vec![],
        hermes: None,
    };
    let key = env.key.clone();
    let forged = format!("omn1_{}", rand_hex(32));
    let mut t = T { pass: 0, fail: 0 };
    let w = |s: &str| work.join(s);

    println!("== hermes-enroll real-life test {}", ts());
    println!(
        "  hermes-gateway {} bytes (cargo test release profile)",
        fs::metadata(HERMES).map(|m| m.len()).unwrap_or(0)
    );
    println!(
        "  omnode A x86_64 {} bytes; omnode B {label_b}",
        fs::metadata(&omx).map(|m| m.len()).unwrap_or(0)
    );

    println!("== 1. enrollment is off by default {}", ts());
    env.start_hermes(&mut t, "hermes-off.log", &[]);
    t.check(
        "off: GET /openmind/node/whoami",
        "404",
        http(P(), "GET", "/openmind/node/whoami", None, None)
            .0
            .to_string(),
    );
    t.check(
        "off: POST /openmind/enroll",
        "404",
        http(P(), "POST", "/openmind/enroll", Some("{}"), None)
            .0
            .to_string(),
    );
    env.stop_hermes();

    println!("== 2. unsafe enrollment config refuses to start {}", ts());
    let on = || ("HERMES_RS_ENROLL", "1".to_string());
    env.probe_start(&mut t, "no URL", "HERMES_RS_ENROLL_URL is not set", &[on()]);
    env.probe_start(
        &mut t,
        "plain http to a public host",
        "plain http is only allowed for a loopback",
        &[
            on(),
            (
                "HERMES_RS_ENROLL_URL",
                "http://hermes.example.invalid/openmind/enroll".into(),
            ),
        ],
    );
    env.probe_start(
        &mut t,
        "wrong path",
        "path must be /openmind/enroll",
        &[
            on(),
            (
                "HERMES_RS_ENROLL_URL",
                format!("http://127.0.0.1:{}/enroll", PPROBE()),
            ),
        ],
    );
    env.probe_start(
        &mut t,
        "non-loopback bind",
        "needs HERMES_RS_ENROLL_ALLOW_PUBLIC=1",
        &[
            ("HERMES_RS_HOST", "0.0.0.0".into()),
            on(),
            (
                "HERMES_RS_ENROLL_URL",
                "https://hermes.example.invalid/openmind/enroll".into(),
            ),
        ],
    );
    env.probe_start(
        &mut t,
        "nonce TTL shorter than payload life",
        "NONCE_TTL must be",
        &[
            on(),
            ("HERMES_RS_ENROLL_URL", url.clone()),
            ("HERMES_RS_ENROLL_NONCE_TTL", "60".into()),
        ],
    );

    println!("== 3. hermes (enroll on) + two omnode nodes {}", ts());
    let enr = env.enr();
    env.start_hermes(&mut t, "hermes-1.log", &enr);
    let log1 = fs::read_to_string(w("hermes-1.log")).unwrap_or_default();
    if let Some(l) = log1
        .lines()
        .find_map(|l| l.find("enroll on: ").map(|i| &l[i..]))
    {
        println!("  log: {}", l.replace(&work.display().to_string(), "$WORK"));
    }
    let store = w("hermes/rs-enroll.json");
    t.check("store created, mode", "600", mode(&store));
    let a = Node {
        run: vec![omx.clone()],
        xdg: w("a"),
        state: w("a/openmind"),
    };
    let b = Node {
        run: run_b,
        xdg: w("b"),
        state: w("b/openmind"),
    };
    for (n, port, d) in [(&a, PA(), "a"), (&b, PB(), "b")] {
        let listen = format!("127.0.0.1:{port}");
        let code = w(d).join("code").display().to_string();
        let child = n
            .cmd(&[
                "boot",
                "--state",
                &n.st(),
                "--listen",
                &listen,
                "--code-file",
                &code,
            ])
            .stdout(Stdio::from(
                fs::File::create(w(d).join("boot.out")).unwrap(),
            ))
            .stderr(Stdio::from(
                fs::File::create(w(d).join("boot.err")).unwrap(),
            ))
            .spawn()
            .expect("spawn omnode boot");
        env.procs.push(child);
    }
    for port in [PA(), PB()] {
        let mut up = "no";
        for _ in 0..150 {
            if http(port, "GET", "/v1/node", None, None).0 == 200 {
                up = "yes";
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        t.check(&format!("omnode on {port} up"), "yes", up);
    }
    let na = js(&http(PA(), "GET", "/v1/node", None, None).1);
    let nb = js(&http(PB(), "GET", "/v1/node", None, None).1);
    let dev_a = sv(&na["device_id"]);
    let dev_b = sv(&nb["device_id"]);
    let fp_a = a.fingerprint();
    let fp_b = b.fingerprint();
    println!("  node A {dev_a} {fp_a}");
    println!(
        "  node B {dev_b} {fp_b} {}",
        nb["arch"].as_str().unwrap_or("")
    );

    println!(
        "== 4. node A enrolls, owner approves by fingerprint, A claims {}",
        ts()
    );
    // A advertises a payment capability: enrollment must never grant it.
    fs::write(
        a.state.join("node.toml"),
        "[node]\ncapabilities = [\"cpu\", \"sandbox\", \"revolut-pay\"]\n",
    )
    .unwrap();
    let o = a.run(&["enroll", "request", &url, "--state", &a.st()]);
    t.check(
        "A: enroll request -> pending (exit 3)",
        "3",
        o.rc.to_string(),
    );
    let v = js(&o.out);
    t.check(
        "A: hermes answered",
        "202 pending",
        format!(
            "{} {}",
            sv(&v["result"]["http"]),
            sv(&v["result"]["hermes"]["status"])
        ),
    );
    t.check(
        "pending list without owner key",
        "401",
        http(P(), "GET", "/openmind/enroll/pending", None, None)
            .0
            .to_string(),
    );
    let (c, body) = http(P(), "GET", "/openmind/enroll/pending", None, Some(&key));
    t.check("pending list (owner)", "200", c.to_string());
    let pfp = js(&body)["pending"]
        .as_array()
        .and_then(|a| a.iter().find(|p| p["device_id"] == dev_a.as_str()))
        .map(|p| sv(&p["fingerprint"]))
        .unwrap_or_default();
    t.check("pending fingerprint == omnode identity show", &fp_a, pfp);
    let cred_a = a.state.join("hermes-credential.json");
    let o = a.run(&["enroll", "claim", "--state", &a.st()]);
    t.check(
        "A: claim before approval -> pending (exit 3), no credential",
        "3 absent",
        format!(
            "{} {}",
            o.rc,
            if cred_a.exists() { "present" } else { "absent" }
        ),
    );
    let bad = format!(
        r#"{{"device_id":"{dev_a}","fingerprint":"SHA256:0000-0000-0000-0000-0000-0000-0000-0000"}}"#
    );
    t.check(
        "approve with wrong fingerprint",
        "409",
        http(
            P(),
            "POST",
            "/openmind/enroll/approve",
            Some(&bad),
            Some(&key),
        )
        .0
        .to_string(),
    );
    let appr =
        format!(r#"{{"device_id":"{dev_a}","fingerprint":"{fp_a}","approved_by":"slavik"}}"#);
    t.check(
        "approve without owner key",
        "401",
        http(P(), "POST", "/openmind/enroll/approve", Some(&appr), None)
            .0
            .to_string(),
    );
    let (c, body) = http(
        P(),
        "POST",
        "/openmind/enroll/approve",
        Some(&appr),
        Some(&key),
    );
    t.check("approve (owner)", "200", c.to_string());
    let v = js(&body);
    t.check(
        "A: scopes granted / denied",
        "cpu,sandbox / revolut-pay",
        format!("{} / {}", join(&v["scopes"]), join(&v["denied"])),
    );
    let o = a.run(&["enroll", "claim", "--state", &a.st()]);
    t.check("A: claim -> credential (exit 0)", "0", o.rc.to_string());
    t.check("A: credential file mode", "600", mode(&cred_a));
    let tok_a = sv(&js(&fs::read_to_string(&cred_a).unwrap_or_default())["token"]);
    assert!(
        tok_a.starts_with("omn1_"),
        "A credential has no omn1_ token"
    );
    let log1 = fs::read_to_string(w("hermes-1.log")).unwrap_or_default();
    if o.out.contains(&tok_a) || o.err.contains(&tok_a) || log1.contains(&tok_a) {
        t.no("A token leaked to stdout/stderr/log");
    } else {
        t.ok("A token not printed by omnode or hermes");
    }
    if fs::read_to_string(&store)
        .unwrap_or_default()
        .contains(&tok_a)
    {
        t.no("A token stored in clear by hermes");
    } else {
        t.ok("hermes store holds no clear token");
    }
    t.check(
        "A: status (whoami) accepted",
        "0",
        a.run(&["enroll", "status", "--state", &a.st()])
            .rc
            .to_string(),
    );
    t.check(
        "A: scope cpu",
        "0",
        a.run(&["enroll", "status", "--state", &a.st(), "--scope", "cpu"])
            .rc
            .to_string(),
    );
    let o = a.run(&[
        "enroll",
        "status",
        "--state",
        &a.st(),
        "--scope",
        "revolut-pay",
    ]);
    let e = sv(&js(&o.out)["error"]);
    t.check(
        "A: scope revolut-pay refused",
        "2 true",
        format!("{} {}", o.rc, e.contains("HTTP 403")),
    );
    t.check(
        "node token on the owner API (/v1/models)",
        "401",
        http(P(), "GET", "/v1/models", None, Some(&tok_a))
            .0
            .to_string(),
    );
    t.check(
        "node token on owner enroll routes",
        "401",
        http(P(), "GET", "/openmind/enroll/devices", None, Some(&tok_a))
            .0
            .to_string(),
    );
    let chat = r#"{"model":"hermes-agent","messages":[{"role":"user","content":"ping"}]}"#;
    t.check(
        "owner API unchanged: mock chat with owner key",
        "200",
        http(P(), "POST", "/v1/chat/completions", Some(chat), Some(&key))
            .0
            .to_string(),
    );

    println!(
        "== 5. refusals: replayed nonce, expired, tampered, unenrolled {}",
        ts()
    );
    let doc = |now: Option<u64>| -> Value {
        let mut args = vec![
            "pair".to_string(),
            url.clone(),
            "--state".into(),
            b.st(),
            "--dry-run".into(),
        ];
        if let Some(n) = now {
            args.push("--now".into());
            args.push(n.to_string());
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        js(&b.run(&refs).out)["would_send"]["body"].clone()
    };
    let doc_b = doc(None);
    let doc_b_s = doc_b.to_string();
    let (c, body) = http(P(), "POST", "/openmind/enroll", Some(&doc_b_s), None);
    t.check(
        "B: first POST of its payload",
        "202 pending",
        format!("{c} {}", sv(&js(&body)["status"])),
    );
    let (c, body) = http(P(), "POST", "/openmind/enroll", Some(&doc_b_s), None);
    t.check(
        "B: same payload replayed",
        "409 nonce already used",
        format!("{c} {}", sv(&js(&body)["error"])),
    );
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let old = doc(Some(now - 1000)).to_string();
    let (c, body) = http(P(), "POST", "/openmind/enroll", Some(&old), None);
    t.check(
        "B: expired payload (created 1000 s ago)",
        "400 true",
        format!("{c} {}", sv(&js(&body)["error"]).contains("expired")),
    );
    let tamper = |f: &dyn Fn(&mut Value)| -> String {
        let mut d = doc_b.clone();
        f(&mut d);
        d.to_string()
    };
    let t1 = tamper(&|d| {
        d["payload"]["capabilities"]
            .as_array_mut()
            .unwrap()
            .push("money".into())
    });
    let (c, body) = http(P(), "POST", "/openmind/enroll", Some(&t1), None);
    t.check(
        "tampered capabilities",
        "400 signature does not verify",
        format!("{c} {}", sv(&js(&body)["error"])),
    );
    let t2 = tamper(&|d| {
        let e = d["payload"]["expires_unix"].as_u64().unwrap_or(0);
        d["payload"]["expires_unix"] = (e + 86400).into();
    });
    t.check(
        "tampered expires_unix",
        "400",
        http(P(), "POST", "/openmind/enroll", Some(&t2), None)
            .0
            .to_string(),
    );
    let t3 = tamper(&|d| {
        let pk = sv(&d["payload"]["public_key"]);
        let first = if pk.starts_with('f') { "e" } else { "f" };
        d["payload"]["public_key"] = format!("{first}{}", &pk[1..]).into();
    });
    t.check(
        "foreign public key",
        "400",
        http(P(), "POST", "/openmind/enroll", Some(&t3), None)
            .0
            .to_string(),
    );
    let t4 = tamper(&|d| {
        d["payload"]["enroll_url"] = "https://other.example.invalid/openmind/enroll".into()
    });
    t.check(
        "payload for another server",
        "400",
        http(P(), "POST", "/openmind/enroll", Some(&t4), None)
            .0
            .to_string(),
    );
    let t5 = tamper(&|d| d["payload"]["admin"] = true.into());
    t.check(
        "unknown field",
        "400",
        http(P(), "POST", "/openmind/enroll", Some(&t5), None)
            .0
            .to_string(),
    );
    let (c, body) = http(P(), "GET", "/openmind/node/whoami", None, Some(&forged));
    t.check(
        "unenrolled: forged token",
        "401 not enrolled",
        format!("{c} {}", sv(&js(&body)["error"])),
    );
    t.check(
        "unenrolled: no token",
        "401",
        http(P(), "GET", "/openmind/node/whoami", None, None)
            .0
            .to_string(),
    );
    t.check(
        "unenrolled B (no request file yet) cannot claim",
        "2",
        b.run(&["enroll", "claim", "--state", &b.st()])
            .rc
            .to_string(),
    );
    // B's request is pending (from the raw POST above); record it for omnode and claim.
    fs::write(
        b.state.join("enroll.json"),
        format!(r#"{{"url":"{url}","device_id":"{dev_b}"}}"#),
    )
    .unwrap();
    t.check(
        "pending B: claim -> pending (exit 3)",
        "3",
        b.run(&["enroll", "claim", "--state", &b.st()])
            .rc
            .to_string(),
    );
    let rej = format!(r#"{{"device_id":"{dev_b}"}}"#);
    t.check(
        "owner rejects B",
        "200",
        http(
            P(),
            "POST",
            "/openmind/enroll/reject",
            Some(&rej),
            Some(&key),
        )
        .0
        .to_string(),
    );
    let o = b.run(&["enroll", "claim", "--state", &b.st()]);
    t.check(
        "rejected B: claim refused",
        "2 true",
        format!(
            "{} {}",
            o.rc,
            sv(&js(&o.out)["error"]).contains("not enrolled")
        ),
    );
    let cred_b = b.state.join("hermes-credential.json");
    t.check(
        "B has no credential",
        "absent",
        if cred_b.exists() { "present" } else { "absent" },
    );

    println!("== 6. node B enrolls properly ({label_b}) {}", ts());
    t.check(
        "B: enroll request -> pending",
        "3",
        b.run(&["enroll", "request", &url, "--state", &b.st()])
            .rc
            .to_string(),
    );
    let appr_b = format!(r#"{{"device_id":"{dev_b}","fingerprint":"{fp_b}","scopes":["cpu"]}}"#);
    let (c, body) = http(
        P(),
        "POST",
        "/openmind/enroll/approve",
        Some(&appr_b),
        Some(&key),
    );
    t.check(
        "approve B with scopes [cpu]",
        "200 cpu",
        format!("{c} {}", join(&js(&body)["scopes"])),
    );
    t.check(
        "B: claim -> credential",
        "0",
        b.run(&["enroll", "claim", "--state", &b.st()])
            .rc
            .to_string(),
    );
    let tok_b = sv(&js(&fs::read_to_string(&cred_b).unwrap_or_default())["token"]);
    t.check(
        "B: accepted",
        "0",
        b.run(&["enroll", "status", "--state", &b.st()])
            .rc
            .to_string(),
    );
    t.check(
        "B: sandbox not granted",
        "2",
        b.run(&["enroll", "status", "--state", &b.st(), "--scope", "sandbox"])
            .rc
            .to_string(),
    );

    println!("== 7. revoke A {}", ts());
    let rev = format!(r#"{{"device_id":"{dev_a}","revoked_by":"slavik"}}"#);
    t.check(
        "revoke without owner key",
        "401",
        http(P(), "POST", "/openmind/enroll/revoke", Some(&rev), None)
            .0
            .to_string(),
    );
    t.check(
        "revoke A (owner)",
        "200",
        http(
            P(),
            "POST",
            "/openmind/enroll/revoke",
            Some(&rev),
            Some(&key),
        )
        .0
        .to_string(),
    );
    t.check(
        "A's credential refused after revoke",
        "401",
        http(P(), "GET", "/openmind/node/whoami", None, Some(&tok_a))
            .0
            .to_string(),
    );
    t.check(
        "A: omnode status refused",
        "2",
        a.run(&["enroll", "status", "--state", &a.st()])
            .rc
            .to_string(),
    );
    let o = a.run(&["enroll", "claim", "--state", &a.st()]);
    t.check(
        "A: new claim refused",
        "2 true",
        format!(
            "{} {}",
            o.rc,
            sv(&js(&o.out)["error"]).contains("device revoked")
        ),
    );
    let o = a.run(&["enroll", "request", &url, "--state", &a.st()]);
    t.check(
        "A: re-enroll refused",
        "2 true",
        format!(
            "{} {}",
            o.rc,
            sv(&js(&o.out)["error"]).contains("device revoked")
        ),
    );
    let (c, body) = http(P(), "GET", "/openmind/node/whoami", None, Some(&tok_b));
    t.check(
        "B unaffected",
        format!("200 {dev_b}"),
        format!("{c} {}", sv(&js(&body)["device_id"])),
    );

    println!("== 8. restart hermes: persistence {}", ts());
    env.stop_hermes();
    env.start_hermes(&mut t, "hermes-2.log", &enr);
    t.check(
        "after restart: A still refused",
        "401",
        http(P(), "GET", "/openmind/node/whoami", None, Some(&tok_a))
            .0
            .to_string(),
    );
    let (c, body) = http(P(), "GET", "/openmind/node/whoami", None, Some(&tok_b));
    t.check(
        "after restart: B's credential still accepted",
        "200 cpu",
        format!("{c} {}", join(&js(&body)["scopes"])),
    );
    let (c, body) = http(P(), "POST", "/openmind/enroll", Some(&doc_b_s), None);
    t.check(
        "after restart: replayed nonce still refused",
        "409 nonce already used",
        format!("{c} {}", sv(&js(&body)["error"])),
    );
    let (_, body) = http(P(), "GET", "/openmind/enroll/devices", None, Some(&key));
    let mut want = [format!("{dev_a}:revoked"), format!("{dev_b}:active")];
    want.sort();
    let mut got: Vec<String> = js(&body)["devices"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|d| format!("{}:{}", sv(&d["device_id"]), sv(&d["status"])))
                .collect()
        })
        .unwrap_or_default();
    got.sort();
    t.check("devices after restart", want.join(" "), got.join(" "));
    if body.contains(&tok_b) || body.contains(&tok_a) {
        t.no("devices list leaks token");
    } else {
        t.ok("devices list shows no token");
    }
    t.check("store mode after writes", "600", mode(&store));
    let sj = js(&fs::read_to_string(&store).unwrap_or_default());
    let len = |k: &str| {
        sj[k]
            .as_array()
            .map(|a| a.len())
            .or_else(|| sj[k].as_object().map(|o| o.len()))
            .unwrap_or(0)
    };
    println!(
        "  store: nonces={} pending={} devices={} flags={}",
        len("nonces"),
        len("pending"),
        len("devices"),
        len("flags")
    );
    env.stop_hermes();
    let logs = [w("hermes-off.log"), w("hermes-1.log"), w("hermes-2.log")]
        .iter()
        .map(|p| fs::read_to_string(p).unwrap_or_default())
        .collect::<String>();
    if logs.contains(&tok_a) || logs.contains(&tok_b) || logs.contains(&key) {
        t.no("secret found in hermes logs");
    } else {
        t.ok("no token or owner key in any hermes log");
    }

    println!(
        "== 9. damaged store refuses to start (never reset) {}",
        ts()
    );
    let mut bytes = fs::read(&store).unwrap();
    bytes.push(b'x');
    let corrupt = w("corrupt.json");
    fs::write(&corrupt, &bytes).unwrap();
    fs::set_permissions(&corrupt, fs::Permissions::from_mode(0o600)).unwrap();
    let mut e = enr.clone();
    e.push(("HERMES_RS_ENROLL_STORE", corrupt.display().to_string()));
    env.probe_start(&mut t, "corrupt store", "is corrupt", &e);
    t.check(
        "corrupt store left untouched",
        "true",
        (fs::read(&corrupt).unwrap_or_default() == bytes).to_string(),
    );
    let open = w("open.json");
    fs::copy(&store, &open).unwrap();
    fs::set_permissions(&open, fs::Permissions::from_mode(0o644)).unwrap();
    let mut e = enr.clone();
    e.push(("HERMES_RS_ENROLL_STORE", open.display().to_string()));
    env.probe_start(&mut t, "store mode 644", "must be 600", &e);
    let mut e = enr.clone();
    e.push((
        "HERMES_RS_ENROLL_STORE",
        w("nope/s.json").display().to_string(),
    ));
    env.probe_start(&mut t, "store dir missing", "write", &e);

    println!("== hermes log lines (no secrets) ==");
    for l in logs.lines().filter(|l| l.contains("enroll")).take(40) {
        println!("  {}", l.replace(&work.display().to_string(), "$WORK"));
    }
    drop(env);
    for p in [P(), PA(), PB(), PPROBE()] {
        t.check(
            &format!("port {p} free after teardown"),
            "true",
            port_free(p).to_string(),
        );
    }
    println!("RESULT {} pass={} fail={}", ts(), t.pass, t.fail);
    assert_eq!(t.fail, 0, "{} checks failed", t.fail);
}
