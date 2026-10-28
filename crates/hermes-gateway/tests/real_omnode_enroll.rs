// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life end-to-end test of the omnode phone pairing flow (GALION_REAL=1):
//! the real release `hermes-gateway` (mock model, no network, no spend) and
//! real `omnode` binaries (x86_64 host build and the static aarch64 musl
//! phone build under qemu-aarch64) on box loopback ports 22000-22009.
//!
//! Ports: 22000 hermes (main), 22001 hermes with a 3 s approval window
//! (timeout path). Nothing else is bound; the phone never listens.
//!
//! `GALION_REAL=1 cargo test --release -p hermes-gateway --test real_omnode_enroll -- --nocapture`
//!
//! Covered: pair -> QR + short code -> owner `hermes-gateway enroll pending`
//! / `approve CODE` -> credential (600, never printed) -> outbound tunnel UP
//! (device-key proof) -> owner `devices` + `ping` round trip over the tunnel;
//! the phone has no listening socket; owner refusal (exit 4); approval
//! timeout (exit 5); bad signature / expired / replayed nonce over HTTP;
//! restart keeps the device and the tunnel reconnects; revoke closes the
//! live tunnel (exit 6); the aarch64 phone binary does the whole flow
//! through the curl transport (the path Termux uses for https).

use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const HERMES: &str = env!("CARGO_BIN_EXE_hermes-gateway");
use galion_realtest::ports::Block;

/// Slot 383 (real-ports-batch-3): PT +1, P +0; `OMNODE_ENROLL_REAL_PORT_BASE`
/// moves the block, unset keeps the old 22000-22009.
static PORTS: Block = Block::new("OMNODE_ENROLL_REAL_PORT_BASE", 22000, 10);

#[allow(non_snake_case)]
fn P() -> u16 {
    PORTS.port(0)
}
#[allow(non_snake_case)]
fn PT() -> u16 {
    PORTS.port(1)
}

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn ts() -> String {
    // Box clock is Europe/Zagreb; `date` prints the explicit offset.
    Command::new("date")
        .arg("+%Y-%m-%dT%H:%M:%S%:z")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn rand_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .unwrap();
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn port_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

fn http(port: u16, method: &str, path: &str, body: Option<&str>) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return (0, String::new());
    };
    s.set_read_timeout(Some(Duration::from_secs(15))).ok();
    let b = body.unwrap_or("");
    let mut head =
        format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n");
    if body.is_some() {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            b.len()
        ));
    }
    head.push_str("\r\n");
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(b.as_bytes());
    let mut raw = String::new();
    let _ = s.read_to_string(&mut raw);
    let (h, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    let code = h
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (code, body.to_string())
}

fn js(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or(Value::Null)
}

struct T {
    pass: u32,
    fail: u32,
}

impl T {
    fn check(&mut self, label: &str, want: impl AsRef<str>, got: impl AsRef<str>) {
        let (w, g) = (want.as_ref(), got.as_ref());
        if w == g {
            self.pass += 1;
            println!("  PASS {label}: {g}");
        } else {
            self.fail += 1;
            println!("  FAIL {label}: want {w:?}, got {g:?}");
        }
    }
    fn yes(&mut self, label: &str, ok: bool, detail: impl AsRef<str>) {
        self.check(label, "true", ok.to_string());
        if !ok {
            println!("       detail: {}", detail.as_ref());
        }
    }
}

/// A running omnode with its output collected line by line.
struct Node {
    child: Child,
    lines: Arc<Mutex<Vec<String>>>,
}

impl Node {
    fn spawn(mut c: Command) -> Node {
        c.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        let mut child = c.spawn().expect("spawn omnode");
        let lines = Arc::new(Mutex::new(Vec::new()));
        for r in [
            Box::new(child.stdout.take().unwrap()) as Box<dyn Read + Send>,
            Box::new(child.stderr.take().unwrap()),
        ] {
            let l = lines.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(r).lines().map_while(Result::ok) {
                    l.lock().unwrap().push(line);
                }
            });
        }
        Node { child, lines }
    }
    fn text(&self) -> String {
        self.lines.lock().unwrap().join("\n")
    }
    fn wait_for(&self, pat: &str, secs: u64) -> bool {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(secs) {
            if self.text().contains(pat) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }
    fn code(&self) -> String {
        self.text()
            .lines()
            .find_map(|l| l.trim().strip_prefix("CODE").map(|c| c.trim().to_string()))
            .unwrap_or_default()
    }
    fn device(&self) -> String {
        self.text()
            .lines()
            .find_map(|l| {
                l.trim()
                    .strip_prefix("device ")
                    .map(|c| c.trim().to_string())
            })
            .unwrap_or_default()
    }
    fn exit(&mut self, secs: u64) -> Option<i32> {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(secs) {
            if let Ok(Some(s)) = self.child.try_wait() {
                std::thread::sleep(Duration::from_millis(100));
                return s.code();
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        None
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Env {
    work: PathBuf,
    key: String,
    hermes: Vec<(u16, Child)>,
    omnode: PathBuf,
    phone: Option<(PathBuf, PathBuf)>, // (qemu, aarch64 omnode)
}

impl Drop for Env {
    fn drop(&mut self) {
        for (_, c) in self.hermes.iter_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Env {
    fn url(port: u16) -> String {
        format!("http://127.0.0.1:{port}/openmind/enroll")
    }

    fn hermes_env(&self, c: &mut Command, port: u16, home: &Path) {
        c.env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", home)
            .env("HERMES_HOME", home)
            .env("HERMES_RS_HOST", "127.0.0.1")
            .env("HERMES_RS_PORT", port.to_string())
            .env("HERMES_RS_MCP", "0")
            .env("API_SERVER_KEY", &self.key);
    }

    fn start(&mut self, port: u16, extra: &[(&str, &str)]) {
        assert!(port_free(port), "port {port} busy");
        let home = self.work.join(format!("hermes-{port}"));
        std::fs::create_dir_all(&home).unwrap();
        let mut c = Command::new(HERMES);
        self.hermes_env(&mut c, port, &home);
        c.env("HERMES_RS_ENROLL", "1")
            .env("HERMES_RS_ENROLL_URL", Self::url(port))
            .env("HERMES_RS_ENROLL_RATE_PER_MIN", "200")
            .env("HERMES_RS_ENROLL_TUNNEL_PING", "1")
            .arg("--mock");
        for (k, v) in extra {
            c.env(k, v);
        }
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.work.join(format!("hermes-{port}.log")))
            .unwrap();
        c.stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log));
        let child = c.spawn().expect("spawn hermes-gateway");
        self.hermes.push((port, child));
        for _ in 0..100 {
            if http(port, "GET", "/health", None).0 == 200 {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("hermes on {port} did not come up");
    }

    fn stop(&mut self, port: u16) {
        if let Some(i) = self.hermes.iter().position(|(p, _)| *p == port) {
            let (_, mut c) = self.hermes.remove(i);
            let _ = Command::new("kill")
                .args(["-TERM", &c.id().to_string()])
                .status();
            let t = Instant::now();
            while t.elapsed() < Duration::from_secs(5) {
                if let Ok(Some(_)) = c.try_wait() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// `hermes-gateway enroll ARGS` (the owner command), as Slavik would run it.
    fn owner(&self, port: u16, args: &[&str]) -> (i32, String) {
        let home = self.work.join(format!("hermes-{port}"));
        let mut c = Command::new(HERMES);
        self.hermes_env(&mut c, port, &home);
        let o = c.arg("enroll").args(args).output().expect("owner cmd");
        (
            o.status.code().unwrap_or(-1),
            format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            ),
        )
    }

    /// `omnode ARGS` as a phone would run it (own HOME, nothing inherited).
    fn omnode_cmd(&self, phone: bool, home: &Path, args: &[&str]) -> Command {
        let mut c = match (&self.phone, phone) {
            (Some((qemu, bin)), true) => {
                let mut c = Command::new(qemu);
                c.arg(bin);
                c
            }
            _ => Command::new(&self.omnode),
        };
        std::fs::create_dir_all(home).unwrap();
        c.env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", home)
            .args(args);
        c
    }

    fn pair(&self, phone: bool, name: &str, port: u16, extra: &[&str]) -> (Node, PathBuf) {
        let home = self.work.join(name);
        let st = home.join("state");
        let url = Self::url(port);
        let mut args = vec![
            "pair",
            url.as_str(),
            "--state",
            st.to_str().unwrap(),
            "--poll-ms",
            "300",
            "--verbose",
        ];
        args.extend_from_slice(extra);
        (Node::spawn(self.omnode_cmd(phone, &home, &args)), st)
    }
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

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
    if !c.status().ok()?.success() {
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
        .find(|p| p.exists())
}

fn mode(p: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| format!("{:o}", m.permissions().mode() & 0o777))
        .unwrap_or_else(|_| "missing".into())
}

/// Listening TCP sockets owned by `pid` (from `ss -ltnp`).
fn listeners_of(pid: u32) -> Vec<String> {
    let o = Command::new("ss")
        .args(["-H", "-ltnp"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    o.lines()
        .filter(|l| l.contains(&format!("pid={pid},")))
        .map(String::from)
        .collect()
}

/// Established TCP connections of `pid` to `port` (from `ss -tnp`).
fn conns_to(pid: u32, port: u16) -> usize {
    let o = Command::new("ss")
        .args(["-H", "-tnp", "state", "established"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    o.lines()
        .filter(|l| l.contains(&format!("pid={pid},")) && l.contains(&format!("127.0.0.1:{port} ")))
        .count()
}

#[test]
fn real_omnode_enroll() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1 for the omnode pairing end-to-end test");
        return;
    }
    let mut t = T { pass: 0, fail: 0 };
    for p in P()..=PT() {
        assert!((PORTS.base()..=PORTS.port(9)).contains(&p));
        assert!(port_free(p), "port {p} busy before start");
    }
    println!("== 0. build {}", ts());
    let omnode = build_omnode(None).expect("omnode host build");
    let qemu = which("qemu-aarch64").or_else(|| which("qemu-aarch64-static"));
    let phone = match (&qemu, build_omnode(Some("aarch64-unknown-linux-musl"))) {
        (Some(q), Some(b)) => {
            let f = Command::new("file")
                .arg(&b)
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                .unwrap_or_default();
            println!(
                "  phone binary: {} ({} bytes) {}",
                b.display(),
                std::fs::metadata(&b).unwrap().len(),
                f.trim()
            );
            t.yes(
                "aarch64 phone binary is a static AArch64 ELF",
                f.contains("aarch64") && f.contains("statically linked"),
                &f,
            );
            Some((q.clone(), b))
        }
        _ => {
            println!(
                "  SKIP phone (aarch64) leg: qemu-aarch64 or the aarch64 musl build is missing"
            );
            None
        }
    };
    let work = std::env::temp_dir().join(format!("omnode-enroll-real-{}", rand_hex(4)));
    std::fs::create_dir_all(&work).unwrap();
    let mut env = Env {
        work: work.clone(),
        key: rand_hex(24),
        hermes: vec![],
        omnode,
        phone,
    };
    env.start(P(), &[]);
    env.start(PT(), &[("HERMES_RS_ENROLL_PENDING_TTL", "3")]);

    println!(
        "== 1. laptop omnode (x86_64): pair, owner approves by code, tunnel {}",
        ts()
    );
    let (mut a, a_st) = env.pair(false, "laptop", P(), &["--tunnel-secs", "60"]);
    t.yes("laptop shows its CODE", a.wait_for("  CODE ", 15), a.text());
    let code = a.code();
    let dev = a.device();
    t.yes(
        "code is XXXX-XXXX",
        code.len() == 9 && &code[4..5] == "-",
        &code,
    );
    t.yes(
        "QR is printed in the terminal",
        a.text()
            .lines()
            .filter(|l| l.contains('█') || l.contains('▀'))
            .count()
            > 10,
        "",
    );
    t.yes(
        "new device key, mode 600",
        mode(&a_st.join("node.key")) == "600",
        mode(&a_st.join("node.key")),
    );
    let (c, out) = env.owner(P(), &["pending"]);
    println!(
        "  owner> hermes-gateway enroll pending\n{}",
        out.trim_end()
            .lines()
            .map(|l| format!("    {l}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    t.check("owner pending exit", "0", c.to_string());
    t.yes(
        "owner sees the same code and device",
        out.contains(&code) && out.contains(&dev),
        &out,
    );
    // wrong code first: nothing approved
    let (c, _) = env.owner(P(), &["approve", "0000-0000"]);
    t.check("approve with a wrong code", "1", c.to_string());
    let (c, out) = env.owner(P(), &["approve", &code.to_lowercase()]);
    t.check("owner approve CODE (typed lower case)", "0", c.to_string());
    t.yes(
        "approve answer says approved",
        out.contains("\"approved\""),
        &out,
    );
    t.yes("laptop: tunnel UP", a.wait_for("tunnel: UP", 20), a.text());
    let cred = a_st.join("hermes-credential.json");
    t.check("credential file mode", "600", mode(&cred));
    let token = js(&std::fs::read_to_string(&cred).unwrap_or_default())["token"]
        .as_str()
        .unwrap_or("")
        .to_string();
    t.yes(
        "token never printed by omnode",
        !token.is_empty() && !a.text().contains(&token),
        "",
    );
    let pid = a.child.id();
    t.check(
        "laptop omnode listening sockets",
        "0",
        listeners_of(pid).len().to_string(),
    );
    t.check(
        "laptop omnode outbound connections to hermes",
        "1",
        conns_to(pid, P()).to_string(),
    );
    let (_, out) = env.owner(P(), &["devices"]);
    let d = js(&out);
    let dv = d["devices"]
        .as_array()
        .and_then(|l| l.iter().find(|x| x["device_id"] == dev.as_str()).cloned())
        .unwrap_or(Value::Null);
    t.check(
        "owner devices: tunnel connected",
        "true",
        dv["tunnel"]["connected"].to_string(),
    );
    t.check(
        "owner devices: tunnel verified (device key)",
        "true",
        dv["tunnel"]["verified"].to_string(),
    );
    t.yes(
        "owner devices: no token or hash listed",
        !out.contains(&token) && !out.contains("token_sha256"),
        "",
    );
    let (c, out) = env.owner(P(), &["ping", &dev]);
    println!(
        "  owner> hermes-gateway enroll ping {dev}\n    {}",
        out.split_whitespace().collect::<Vec<_>>().join(" ")
    );
    t.check("owner ping over the tunnel exit", "0", c.to_string());
    t.yes(
        "ping has a round-trip time",
        js(&out)["rtt_ms"].as_u64().is_some(),
        &out,
    );
    t.yes(
        "laptop answered the challenge",
        a.wait_for("answered challenge p", 5),
        a.text(),
    );
    t.yes("pings flow", a.wait_for("ping 2 at", 10), a.text());

    println!("== 2. refusal, timeout, signature/expiry/replay {}", ts());
    let (mut r, _) = env.pair(false, "refused", P(), &[]);
    t.yes("refused: CODE shown", r.wait_for("  CODE ", 15), r.text());
    let (c, _) = env.owner(P(), &["reject", &r.code()]);
    t.check("owner reject CODE", "0", c.to_string());
    t.check(
        "refused device exit code",
        "Some(4)",
        format!("{:?}", r.exit(15)),
    );
    t.yes(
        "refused device is told",
        r.text().contains("the owner refused this device"),
        r.text(),
    );
    let (mut x, _) = env.pair(false, "late", PT(), &[]);
    t.yes("timeout: CODE shown", x.wait_for("  CODE ", 15), x.text());
    t.check(
        "unapproved device exit code (3 s window)",
        "Some(5)",
        format!("{:?}", x.exit(20)),
    );
    t.yes(
        "timed-out device is told",
        x.text().contains("did not approve in time"),
        x.text(),
    );
    let (_, out) = env.owner(PT(), &["approve", &x.code()]);
    t.yes(
        "owner can not approve after the timeout",
        out.contains("no pending request"),
        &out,
    );
    let (_, out) = env.owner(PT(), &["pending"]);
    t.yes(
        "owner list shows it as expired",
        out.contains("expired"),
        &out,
    );
    // raw documents from `omnode pair --dry-run`
    let probe_home = work.join("probe");
    let probe_st = probe_home.join("state");
    let o = env
        .omnode_cmd(false, &probe_home, &["identity", "init", "--no-bind"])
        .output()
        .unwrap();
    let _ = o;
    let o = env
        .omnode_cmd(
            false,
            &probe_home,
            &[
                "pair",
                &Env::url(P()),
                "--dry-run",
                "--state",
                probe_home.join(".config/openmind").to_str().unwrap(),
            ],
        )
        .output()
        .unwrap();
    let doc = js(&String::from_utf8_lossy(&o.stdout))["would_send"]["body"].clone();
    t.yes(
        "dry-run document",
        doc.is_object(),
        String::from_utf8_lossy(&o.stderr),
    );
    let _ = probe_st;
    let mut bad = doc.clone();
    bad["payload"]["capabilities"] = serde_json::json!(["cpu", "admin"]);
    let (c, b) = http(P(), "POST", "/openmind/enroll", Some(&bad.to_string()));
    t.check(
        "tampered payload",
        "400 true",
        format!(
            "{c} {}",
            js(&b)["error"].as_str().unwrap_or("").contains("signature")
        ),
    );
    let (c, _) = http(P(), "POST", "/openmind/enroll", Some(&doc.to_string()));
    t.check("fresh payload", "202", c.to_string());
    let (c, b) = http(P(), "POST", "/openmind/enroll", Some(&doc.to_string()));
    t.check(
        "same payload replayed",
        "409 nonce already used",
        format!("{c} {}", js(&b)["error"].as_str().unwrap_or("")),
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs(); // repo-check: allow utc (unix seconds for --now, not a displayed time)
    let o = env
        .omnode_cmd(
            false,
            &probe_home,
            &[
                "pair",
                &Env::url(P()),
                "--dry-run",
                "--now",
                &(now - 1000).to_string(),
                "--state",
                probe_home.join(".config/openmind").to_str().unwrap(),
            ],
        )
        .output()
        .unwrap();
    let old = js(&String::from_utf8_lossy(&o.stdout))["would_send"]["body"].to_string();
    let (c, b) = http(P(), "POST", "/openmind/enroll", Some(&old));
    t.check(
        "expired payload",
        "400 true",
        format!(
            "{c} {}",
            js(&b)["error"].as_str().unwrap_or("").contains("expired")
        ),
    );
    let (c, _) = http(P(), "GET", "/openmind/node/tunnel", None);
    t.check("tunnel without a credential", "401", c.to_string());
    let (c, _) = http(P(), "GET", "/openmind/enroll/pending", None);
    t.check("owner route without the key", "401", c.to_string());

    println!(
        "== 3. phone build (aarch64 musl, qemu-aarch64, curl transport) {}",
        ts()
    );
    let phone_dev = if env.phone.is_some() {
        let (q, q_st) = env.pair(true, "phone", P(), &["--via-curl", "--tunnel-secs", "60"]);
        t.yes("phone: CODE shown", q.wait_for("  CODE ", 60), q.text());
        let qd = q.device();
        let (c, _) = env.owner(P(), &["approve", &q.code()]);
        t.check("owner approves the phone", "0", c.to_string());
        t.yes("phone: tunnel UP", q.wait_for("tunnel: UP", 40), q.text());
        t.check(
            "phone credential mode",
            "600",
            mode(&q_st.join("hermes-credential.json")),
        );
        let (c, out) = env.owner(P(), &["ping", &qd]);
        t.check("owner ping to the phone", "0", c.to_string());
        println!(
            "    phone ping: {}",
            out.split_whitespace().collect::<Vec<_>>().join(" ")
        );
        let qpid = q.child.id();
        t.check(
            "phone (qemu) listening sockets",
            "0",
            listeners_of(qpid).len().to_string(),
        );
        println!(
            "  phone transcript:\n{}",
            q.text()
                .lines()
                .filter(|l| !l.contains('█') && !l.contains('▀') && !l.contains('▄'))
                .map(|l| format!("    | {l}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        drop(q);
        Some(qd)
    } else {
        None
    };

    println!(
        "== 4. restart keeps the device; tunnel reconnects; revoke closes it {}",
        ts()
    );
    env.stop(P());
    t.yes(
        "laptop notices the restart",
        a.wait_for("connection lost", 15) || a.wait_for("stream ended", 1),
        a.text(),
    );
    env.start(P(), &[]);
    let before = a.text().matches("tunnel: UP").count();
    let t0 = Instant::now();
    let mut back = false;
    while t0.elapsed() < Duration::from_secs(40) {
        if a.text().matches("tunnel: UP").count() > before {
            back = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    t.yes(
        "laptop tunnel back UP after restart (backoff reconnect)",
        back,
        a.text(),
    );
    let (_, out) = env.owner(P(), &["devices"]);
    t.yes(
        "device still active after restart",
        out.contains(&dev) && out.contains("\"active\""),
        &out,
    );
    let (c, _) = env.owner(P(), &["revoke", &dev]);
    t.check("owner revoke", "0", c.to_string());
    t.check(
        "laptop exit after revoke",
        "Some(6)",
        format!("{:?}", a.exit(15)),
    );
    t.yes(
        "laptop is told it was revoked",
        a.text().contains("revoked"),
        a.text(),
    );
    let o = env
        .omnode_cmd(
            false,
            &work.join("laptop"),
            &["tunnel", "--state", a_st.to_str().unwrap(), "--secs", "5"],
        )
        .output()
        .unwrap();
    t.check(
        "`omnode tunnel` after revoke exit",
        "Some(2)",
        format!("{:?}", o.status.code()),
    );
    println!(
        "  laptop transcript:\n{}",
        a.text()
            .lines()
            .filter(|l| !l.contains('█') && !l.contains('▀') && !l.contains('▄'))
            .map(|l| format!("    | {l}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let _ = phone_dev;

    println!("== 5. teardown {}", ts());
    drop(a);
    drop(r);
    drop(x);
    for port in [P(), PT()] {
        env.stop(port);
    }
    std::thread::sleep(Duration::from_millis(300));
    for p in P()..=PT() {
        t.yes(&format!("port {p} free after teardown"), port_free(p), "");
    }
    for port in [P(), PT()] {
        let log =
            std::fs::read_to_string(work.join(format!("hermes-{port}.log"))).unwrap_or_default();
        t.yes(
            &format!("hermes {port} log has no token"),
            !log.contains("omn1_"),
            "",
        );
        println!("  hermes {port} log (enroll lines):");
        for l in log.lines().filter(|l| l.contains("enroll")).take(40) {
            println!("    {l}");
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    println!("== RESULT {} passed, {} failed {}", t.pass, t.fail, ts());
    assert_eq!(t.fail, 0, "{} checks failed", t.fail);
}
