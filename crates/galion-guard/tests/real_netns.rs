// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Real-life test for galion-guard (Rust
//! port of the former scripts/real/guard-rust.sh, same 12 phases, same 38
//! checks; stand-in sshd port 19471 = slot 47 range 19470-19479, inside the
//! private netns so it cannot collide with anything on the host).
//!
//! `GALION_REAL=1 cargo test --release -p galion-guard --test real_netns`
//!
//! Everything runs inside an unprivileged user + network namespace
//! (`unshare -rn`, root only inside it): a real kernel nf_tables ruleset, a
//! stand-in "sshd" (a listener thread answering an SSH banner) on
//! 10.99.0.1:19471, attackers and an allowlisted admin as extra /32s on the
//! namespace's lo, synthesized auth.log lines in the VPS shapes. Nothing
//! touches the host firewall or the VPS.
//!
//! nft: `$NFT_ROOT/usr/sbin/nft` (Debian nftables .debs unpacked with
//! `dpkg -x`, fetched with `apt-get download` unless GUARD_FETCH_NFT=0; no
//! root needed); without it the dry-run backend is used and the kernel-set
//! checks are skipped (reported). If `unshare -rn` is not permitted on the
//! host (no unprivileged user namespaces) the test reports SKIP.
//!
//! This file has its own main (harness = false): it is the outer driver, the
//! inner namespace run (env GUARD_NETNS_INNER) and the fake `journalctl`
//! (when invoked as `fake-journalctl`).

use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::symlink;
use std::os::unix::io::FromRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_galion-guard");
const INNER: &str = "GUARD_NETNS_INNER";
const SSHD: &str = "10.99.0.1:19471";
const ADDRS: &[&str] = &[
    "10.99.0.1",
    "10.99.0.7",
    "10.99.0.8",
    "10.99.0.66",
    "10.99.0.67",
    "10.99.0.68",
    "10.99.0.69",
    "10.99.0.70",
];

fn main() {
    let argv0 = std::env::args().next().unwrap_or_default();
    if argv0.ends_with("fake-journalctl") {
        fake_journalctl();
        return;
    }
    if let Ok(work) = std::env::var(INNER) {
        std::process::exit(inner(PathBuf::from(work)));
    }
    // `cargo test` passes libtest flags (e.g. --list, filters); only run on demand.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list") {
        println!("real_netns: test");
        return;
    }
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        println!("real_netns: skipped (set GALION_REAL=1)");
        return;
    }
    std::process::exit(outer());
}

// ------------------------------------------------------------------ outer

fn outer() -> i32 {
    let target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    let nft_root = std::env::var("NFT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| target.join("nftdeb/root"));
    if !nft_root.join("usr/sbin/nft").exists()
        && std::env::var("GUARD_FETCH_NFT").as_deref() != Ok("0")
    {
        fetch_nft(&nft_root);
    }
    let probe = Command::new("unshare")
        .args(["-rn", "true"])
        .stderr(Stdio::null())
        .status();
    if !matches!(probe, Ok(s) if s.success()) {
        println!("real_netns: SKIP: `unshare -rn` not permitted here (needs unprivileged user namespaces)");
        return 0;
    }
    let work = std::env::temp_dir().join(format!("guard-rust-real.{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).unwrap();
    let me = std::env::current_exe().unwrap();
    let st = Command::new("unshare")
        .arg("-rn")
        .arg(&me)
        .env(INNER, &work)
        .env("NFT_ROOT", &nft_root)
        .status()
        .expect("unshare");
    let _ = fs::remove_dir_all(&work);
    st.code().unwrap_or(1)
}

fn fetch_nft(root: &Path) {
    let d = root.parent().unwrap();
    fs::create_dir_all(d).unwrap();
    let ok = Command::new("timeout")
        .args(["180", "apt-get", "download"])
        .args([
            "nftables",
            "libnftables1",
            "libnftnl11",
            "libjansson4",
            "libedit2",
            "libbsd0",
            "libmd0",
        ])
        .current_dir(d)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        println!("note: could not fetch nftables debs; dry-run backend");
        return;
    }
    for e in fs::read_dir(d).unwrap().flatten() {
        if e.path().extension().is_some_and(|x| x == "deb") {
            let _ = Command::new("dpkg")
                .arg("-x")
                .arg(e.path())
                .arg(root)
                .status();
        }
    }
}

// --------------------------------------------------------- fake journalctl

/// `echo "$*" >> $GUARD_FAKE_JARGS; exec tail -n +1 -f $GUARD_FAKE_JOURNAL`,
/// exiting when its parent (the daemon) goes away or stdout closes.
fn fake_journalctl() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Ok(p) = std::env::var("GUARD_FAKE_JARGS") {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{}", args.join(" "));
        }
    }
    let Ok(path) = std::env::var("GUARD_FAKE_JOURNAL") else {
        return;
    };
    // SAFETY: getppid takes no arguments, cannot fail and touches no memory.
    let ppid = unsafe { libc::getppid() };
    let mut f = loop {
        match File::open(&path) {
            Ok(f) => break f,
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let mut out = std::io::stdout().lock();
    let mut buf = [0u8; 8192];
    loop {
        match f.read(&mut buf) {
            Ok(0) => {
                // SAFETY: as above.
                if unsafe { libc::getppid() } != ppid {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(n) => {
                if out.write_all(&buf[..n]).and_then(|_| out.flush()).is_err() {
                    return;
                }
            }
            Err(_) => return,
        }
    }
}

// ------------------------------------------------------------------ inner

struct T {
    work: PathBuf,
    cfg: PathBuf,
    log: PathBuf,
    nft: Option<PathBuf>,
    ld: String,
    backend: &'static str,
    daemon: Option<Child>,
    pass: u32,
    fail: u32,
    n: u32,
}

fn say(m: &str) {
    let ts = std::process::Command::new("date")
        .arg("+%H:%M:%S%:z")
        .output();
    let ts = ts
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    println!("[{ts}] {m}");
}

fn sh(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Connect from `src` to the stand-in sshd: OPEN if it answers with the
/// SSH banner, BLOCKED otherwise (reject = ICMP port unreachable).
fn reach(src: &str) -> &'static str {
    let s: std::net::Ipv4Addr = src.parse().unwrap();
    let d: std::net::SocketAddrV4 = SSHD.parse().unwrap();
    // SAFETY: plain BSD socket calls on a fresh fd, sockaddr_in built in full.
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0);
        if fd < 0 {
            return "BLOCKED";
        }
        let mk = |ip: std::net::Ipv4Addr, port: u16| libc::sockaddr_in {
            sin_family: libc::AF_INET as libc::sa_family_t,
            sin_port: port.to_be(),
            sin_addr: libc::in_addr {
                s_addr: u32::from_ne_bytes(ip.octets()),
            },
            sin_zero: [0; 8],
        };
        let len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
        let sa = mk(s, 0);
        let da = mk(*d.ip(), d.port());
        let tv = libc::timeval {
            tv_sec: 2,
            tv_usec: 0,
        };
        let tvp = &tv as *const _ as *const libc::c_void;
        let tl = std::mem::size_of::<libc::timeval>() as libc::socklen_t;
        libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_SNDTIMEO, tvp, tl);
        libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_RCVTIMEO, tvp, tl);
        if libc::bind(fd, &sa as *const _ as *const libc::sockaddr, len) != 0
            || libc::connect(fd, &da as *const _ as *const libc::sockaddr, len) != 0
        {
            libc::close(fd);
            return "BLOCKED";
        }
        let mut st = std::net::TcpStream::from_raw_fd(fd);
        let mut b = [0u8; 128];
        match st.read(&mut b) {
            Ok(n) if String::from_utf8_lossy(&b[..n]).contains("SSH-2.0") => "OPEN",
            _ => "BLOCKED",
        }
    }
}

fn stamp() -> String {
    // RFC 3339 UTC with microseconds, the VPS journal short-iso shape.
    let o = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%S.%6N+00:00"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

impl T {
    fn ok(&mut self, d: &str, c: bool) {
        if c {
            self.pass += 1;
            say(&format!("PASS {d}"));
        } else {
            self.fail += 1;
            say(&format!("FAIL {d}"));
        }
    }
    fn eq(&mut self, d: &str, got: impl ToString, want: &str) {
        let g = got.to_string();
        if g != want {
            say(&format!("   expected [{want}] got [{g}]"));
        }
        self.ok(d, g == want);
    }

    fn write_cfg(&self, source: &str, ignoreself: bool) {
        let w = self.work.display();
        let nft = self
            .nft
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "nft".into());
        fs::write(
            &self.cfg,
            format!(
                "[guard]\nbackend = \"{b}\"\nnft = \"{nft}\"\ntable = \"galion-guard\"\ndry_run_log = \"{w}/dry-run.nft\"\nstate = \"{w}/state/state.json\"\nsocket = \"{w}/run/control.sock\"\nignoreself = {ignoreself}\nsave_every = 2\n[sshd]\nsource = \"{source}\"\nlogpath = \"{l}\"\njournalctl = \"{w}/fake-journalctl\"\njournal_match = [\"_COMM=sshd\"]\npoll_ms = 200\nmaxretry = 5\nfindtime = \"60s\"\nbantime = \"20s\"\nbantime_increment = true\npurge_age = \"1d\"\nprotect_accepted = \"24h\"\nignoreip = [\"127.0.0.1/8\", \"::1\", \"10.99.0.7\"]\nports = [19471]\n",
                b = self.backend,
                l = self.log.display()
            ),
        )
        .unwrap();
    }

    fn guard(&self, args: &[&str]) -> std::process::Output {
        Command::new(BIN)
            .args(args)
            .arg("--config")
            .arg(&self.cfg)
            .env("LD_LIBRARY_PATH", &self.ld)
            .output()
            .unwrap()
    }

    fn status(&self) -> Value {
        let o = self.guard(&["status", "--json"]);
        serde_json::from_slice(&o.stdout).unwrap_or(Value::Null)
    }

    fn sj(&self, f: impl Fn(&Value) -> Value) -> String {
        match f(&self.status()) {
            Value::String(s) => s,
            Value::Null => "null".into(),
            v => v.to_string(),
        }
    }

    fn bans_for(&self, ip: &str) -> usize {
        self.status()["bans"]
            .as_array()
            .map(|a| a.iter().filter(|b| b["ip"] == ip).count())
            .unwrap_or(0)
    }

    fn start(&mut self) {
        let dlog = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.work.join("daemon.log"))
            .unwrap();
        let child = Command::new(BIN)
            .args(["run", "--config"])
            .arg(&self.cfg)
            .env("LD_LIBRARY_PATH", &self.ld)
            .env("GUARD_FAKE_JOURNAL", self.work.join("journal.jsonl"))
            .env("GUARD_FAKE_JARGS", self.work.join("jargs"))
            .stdout(dlog.try_clone().unwrap())
            .stderr(dlog)
            .spawn()
            .unwrap();
        self.daemon = Some(child);
        let sock = self.work.join("run/control.sock");
        for _ in 0..50 {
            if sock.exists() && self.guard(&["status"]).status.success() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        self.ok("daemon start", false);
    }

    fn stop(&mut self) {
        let _ = self.guard(&["stop"]);
        if let Some(mut c) = self.daemon.take() {
            let _ = c.wait();
        }
    }

    fn kill9(&mut self) {
        if let Some(mut c) = self.daemon.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    fn fail_lines(&mut self, ip: &str, n: usize, file: &Path) {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(file)
            .unwrap();
        for _ in 0..n {
            self.n += 1;
            let (pid, port) = (self.n, 40000 + self.n);
            let ts = stamp();
            writeln!(
                f,
                "{ts} box sshd[{pid}]: Invalid user u{pid} from {ip} port {port}"
            )
            .unwrap();
            writeln!(f, "{ts} box sshd[{pid}]: Connection closed by invalid user u{pid} {ip} port {port} [preauth]").unwrap();
        }
        drop(f);
        std::thread::sleep(Duration::from_millis(800));
    }

    fn fails(&mut self, ip: &str, n: usize) {
        let l = self.log.clone();
        self.fail_lines(ip, n, &l);
    }

    fn nft_out(&self, args: &[&str]) -> Option<String> {
        let nft = self.nft.as_ref()?;
        let o = Command::new(nft)
            .args(args)
            .env("LD_LIBRARY_PATH", &self.ld)
            .output()
            .ok()?;
        o.status
            .success()
            .then(|| String::from_utf8_lossy(&o.stdout).into_owned())
    }

    fn inset(&self, ip: &str) -> bool {
        match self.nft {
            Some(_) => self
                .nft_out(&["list", "set", "inet", "galion-guard", "addr4"])
                .is_some_and(|s| s.contains(ip)),
            None => self.bans_for(ip) > 0,
        }
    }
}

fn indent(s: &str) {
    for l in s.lines() {
        println!("   | {l}");
    }
}

fn inner(work: PathBuf) -> i32 {
    // Network: lo up, our addresses on it.
    sh("ip", &["link", "set", "lo", "up"]);
    for a in ADDRS {
        sh("ip", &["addr", "add", &format!("{a}/32"), "dev", "lo"]);
    }
    let nft_root = PathBuf::from(std::env::var("NFT_ROOT").unwrap_or_default());
    let nft_bin = nft_root.join("usr/sbin/nft");
    let have_nft = nft_bin.exists();
    let mut t = T {
        cfg: work.join("guard.toml"),
        log: work.join("auth.log"),
        nft: have_nft.then(|| nft_bin.clone()),
        ld: nft_root
            .join("usr/lib/x86_64-linux-gnu")
            .display()
            .to_string(),
        backend: if have_nft { "nft" } else { "dry-run" },
        daemon: None,
        pass: 0,
        fail: 0,
        n: 1000,
        work: work.clone(),
    };
    say(&format!("backend={} work={}", t.backend, work.display()));
    fs::write(&t.log, "").unwrap();
    // Fake journalctl = this very test binary under another name.
    symlink(
        std::env::current_exe().unwrap(),
        work.join("fake-journalctl"),
    )
    .unwrap();

    // Stand-in sshd: banner listener thread inside the namespace.
    let l = TcpListener::bind(SSHD).unwrap_or_else(|e| panic!("bind {SSHD}: {e}"));
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let peer = s.peer_addr().map(|p| p.to_string()).unwrap_or_default();
            let _ = s.write_all(format!("SSH-2.0-galion-real-peer hello {peer}\r\n").as_bytes());
        }
    });

    let rc = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| phases(&mut t)));
    // cleanup (trap EXIT)
    t.kill9();
    if t.nft.is_some() {
        let _ = t.nft_out(&["delete", "table", "inet", "galion-guard"]);
    }
    if rc.is_err() {
        say("ABORT: phase panicked");
        return 1;
    }
    say(&format!(
        "RESULT: {} passed, {} failed (backend={})",
        t.pass, t.fail, t.backend
    ));
    (t.fail != 0) as i32
}

fn phases(t: &mut T) {
    let nft_mode = t.nft.is_some();
    t.write_cfg("file", false);
    t.start();

    say("== P1 setup");
    if nft_mode {
        let table = t
            .nft_out(&["list", "table", "inet", "galion-guard"])
            .unwrap_or_default();
        indent(&table);
        t.ok(
            "nft table/chain/sets created with timeout flag",
            table.contains("flags timeout"),
        );
    }
    t.eq(
        "attacker reaches the stand-in sshd before any failure",
        reach("10.99.0.66"),
        "OPEN",
    );

    say("== P2 ban after maxretry=5 within findtime");
    t.fails("10.99.0.66", 4);
    t.eq(
        "4 failures: not banned",
        t.sj(|s| s["currently_banned"].clone()),
        "0",
    );
    t.eq(
        "4 failures counted",
        t.sj(|s| s["total_failed"].clone()),
        "4",
    );
    let t_ban = Instant::now();
    t.fails("10.99.0.66", 1);
    t.eq(
        "5th failure: banned",
        t.sj(|s| s["currently_banned"].clone()),
        "1",
    );
    let c = t.inset("10.99.0.66");
    t.ok("ban is in the kernel set", c);
    if let Some(s) = t.nft_out(&["list", "set", "inet", "galion-guard", "addr4"]) {
        indent(
            &s.lines()
                .filter(|l| l.contains("elements"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    t.eq("attacker now rejected", reach("10.99.0.66"), "BLOCKED");
    t.eq(
        "allowlisted admin still connects",
        reach("10.99.0.7"),
        "OPEN",
    );
    t.eq(
        "unrelated address still connects",
        reach("10.99.0.8"),
        "OPEN",
    );
    let until = t.sj(|s| s["bans"][0]["until"].clone());
    say(&format!(
        "   ban until {until}, ban_number {}",
        t.sj(|s| s["bans"][0]["ban_number"].clone())
    ));

    say("== P3 never ban loopback / allowlist");
    t.fails("10.99.0.7", 10);
    t.fails("127.0.0.1", 10);
    t.fails("::1", 10);
    t.eq(
        "still exactly one ban",
        t.sj(|s| s["currently_banned"].clone()),
        "1",
    );
    t.eq(
        "30 protected hits",
        t.sj(|s| s["protected_hits"].clone()),
        "30",
    );
    let c = !t.guard(&["ban", "127.0.0.1"]).status.success();
    t.ok("manual ban of loopback refused", c);
    let c = !t.guard(&["ban", "10.99.0.7"]).status.success();
    t.ok("manual ban of allowlisted admin refused", c);
    let dl = fs::read_to_string(t.work.join("daemon.log")).unwrap_or_default();
    t.ok(
        "daemon log never names a protected address",
        !dl.contains("10.99.0.7") && !dl.contains("127.0.0.1"),
    );

    say("== P4 restart persistence (clean stop, then kill -9)");
    t.stop();
    let c = t.inset("10.99.0.66");
    if nft_mode {
        t.ok("ban survives in the kernel while the daemon is down", c);
        t.eq(
            "attacker still rejected with the daemon down",
            reach("10.99.0.66"),
            "BLOCKED",
        );
    }
    t.start();
    t.eq(
        "ban restored from state after clean stop",
        t.sj(|s| s["bans"][0]["ip"].clone()),
        "10.99.0.66",
    );
    t.kill9();
    t.start();
    t.eq(
        "ban restored after kill -9",
        t.sj(|s| s["bans"][0]["ip"].clone()),
        "10.99.0.66",
    );
    t.eq(
        "until unchanged across restarts",
        t.sj(|s| s["bans"][0]["until"].clone()),
        &until,
    );

    say("== P5 expiry on time");
    while t.sj(|s| s["currently_banned"].clone()) != "0"
        && t_ban.elapsed() < Duration::from_secs(40)
    {
        std::thread::sleep(Duration::from_millis(250));
    }
    let len = t_ban.elapsed().as_secs();
    say(&format!("   unbanned after {len}s (bantime 20s)"));
    t.ok("unban within 20..22 s", (19..=22).contains(&len));
    let c = !t.inset("10.99.0.66");
    t.ok("kernel set no longer has it", c);
    t.eq("attacker reaches sshd again", reach("10.99.0.66"), "OPEN");

    say("== P6 increment (2nd ban = 2 x bantime) and CLI unban");
    t.fails("10.99.0.66", 5);
    t.eq(
        "second ban is ban #2",
        t.sj(|s| s["bans"][0]["ban_number"].clone()),
        "2",
    );
    let rem: i64 = t
        .sj(|s| s["bans"][0]["remaining_s"].clone())
        .parse()
        .unwrap_or(-1);
    say(&format!("   remaining {rem}s"));
    t.ok("second ban lasts 40 s", (38..=40).contains(&rem));
    indent(&String::from_utf8_lossy(
        &t.guard(&["unban", "10.99.0.66"]).stdout,
    ));
    t.eq("CLI unban lifts it at once", reach("10.99.0.66"), "OPEN");
    t.eq(
        "status shows no bans",
        t.sj(|s| s["currently_banned"].clone()),
        "0",
    );

    say("== P7 log rotation (rename; late lines in the old file)");
    let old = t.work.join("auth.log.1");
    fs::rename(&t.log, &old).unwrap();
    fs::write(&t.log, "").unwrap();
    t.fail_lines("10.99.0.67", 2, &old);
    t.fails("10.99.0.67", 3);
    t.eq(
        "failures across rotation add up to a ban",
        t.bans_for("10.99.0.67"),
        "1",
    );
    fs::copy(&t.log, t.work.join("trunc")).unwrap(); // copytruncate
    OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&t.log)
        .unwrap();
    t.fails("10.99.0.8", 5);
    t.eq(
        "copy-truncate: new lines still read",
        t.bans_for("10.99.0.8"),
        "1",
    );
    let _ = t.guard(&["unban", "10.99.0.8"]);

    say("== P8 successful login protects a roaming admin");
    {
        let mut f = OpenOptions::new().append(true).open(&t.log).unwrap();
        writeln!(f, "{} box sshd[7777]: Accepted publickey for root from 10.99.0.68 port 50000 ssh2: ED25519 SHA256:AAAAexample", stamp()).unwrap();
    }
    t.fails("10.99.0.68", 12);
    t.eq(
        "no ban after a recent successful login",
        t.bans_for("10.99.0.68"),
        "0",
    );

    say("== P9 kernel timeout lifts bans even if the daemon is dead");
    t.kill9();
    if nft_mode {
        t.eq(
            "10.99.0.67 still blocked right after the crash",
            reach("10.99.0.67"),
            "BLOCKED",
        );
        std::thread::sleep(Duration::from_secs(21));
        t.eq(
            "10.99.0.67 open again with no daemon running (kernel timeout)",
            reach("10.99.0.67"),
            "OPEN",
        );
    } else {
        std::thread::sleep(Duration::from_secs(21));
    }
    t.start();
    t.eq(
        "restart after expiry: stale ban dropped",
        t.bans_for("10.99.0.67"),
        "0",
    );
    t.stop();

    say("== P10 ignoreself: this host's own addresses are never banned");
    t.write_cfg("file", true);
    t.start();
    t.fails("10.99.0.69", 8);
    t.eq("local address not banned", t.bans_for("10.99.0.69"), "0");
    let n = t.status()["self_addresses"].as_u64().unwrap_or(0);
    t.ok("self addresses detected", n >= 8);
    t.stop();

    say("== P11 journald source (real child process; fake journalctl replays JSON)");
    let j = t.work.join("journal.jsonl");
    fs::write(&j, "").unwrap();
    t.write_cfg("journal", false);
    t.start();
    {
        let mut f = OpenOptions::new().append(true).open(&j).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        for i in 1..=5u64 {
            let us = now * 1_000_000 + i;
            writeln!(
                f,
                "{{\"__CURSOR\":\"s=guard;i={i}\",\"__REALTIME_TIMESTAMP\":\"{us}\",\"_HOSTNAME\":\"box\",\"SYSLOG_IDENTIFIER\":\"sshd\",\"_PID\":\"{}\",\"MESSAGE\":\"Invalid user j{i} from 10.99.0.70 port {}\"}}",
                900 + i,
                41000 + i
            )
            .unwrap();
        }
    }
    std::thread::sleep(Duration::from_millis(1500));
    t.eq(
        "journal entries ban 10.99.0.70",
        t.bans_for("10.99.0.70"),
        "1",
    );
    t.stop();
    t.start();
    std::thread::sleep(Duration::from_millis(300));
    let jargs = fs::read_to_string(t.work.join("jargs")).unwrap_or_default();
    t.ok(
        "journalctl restarted from the saved cursor",
        jargs.contains("--after-cursor=s=guard;i=5"),
    );
    t.ok(
        "first start used --since (findtime backlog)",
        jargs.contains("--since=@"),
    );
    for l in jargs.lines() {
        println!("   | args: {l}");
    }
    t.stop();

    say("== P12 teardown");
    indent(&String::from_utf8_lossy(&t.guard(&["teardown"]).stdout));
    if nft_mode {
        let c = t
            .nft_out(&["list", "table", "inet", "galion-guard"])
            .is_none();
        t.ok("table removed", c);
    }

    say("daemon log (last 12 lines):");
    let dl = fs::read_to_string(t.work.join("daemon.log")).unwrap_or_default();
    let lines: Vec<&str> = dl.lines().collect();
    indent(&lines[lines.len().saturating_sub(12)..].join("\n"));
}
