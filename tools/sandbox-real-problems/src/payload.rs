// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! The tools that run INSIDE the sandbox. Each is a real piece of work (or a
//! realistic failure of one) on copies of real VPS data. They print
//! `RESULT ...` lines; contents of secrets or rows are never printed.
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const UNREACH: &str = "error=\"Unable to reach the origin service";

/// Counts that the cloudflared 502 diagnosis produces.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Diag {
    pub lines: u64,
    pub err_lines: u64,
    /// `ERR Request failed error="Unable to reach the origin service..."`:
    /// one per failed public request, i.e. one per 502 served by the edge.
    pub requests: u64,
    /// `ERR  error="Unable to reach..." originService=...`: the matching
    /// origin dial failure (carries the origin, not the public host).
    pub events: u64,
    pub by_origin: BTreeMap<String, u64>,
    pub by_dest: BTreeMap<String, u64>,
    pub first: String,
    pub last: String,
}

fn field<'a>(l: &'a str, key: &str) -> Option<&'a str> {
    let i = l.find(key)? + key.len();
    let rest = &l[i..];
    Some(rest.split(' ').next().unwrap_or(rest))
}

pub fn diag(text: &str) -> Diag {
    let mut d = Diag::default();
    for l in text.lines() {
        d.lines += 1;
        if let Some(ts) = l.split(' ').next() {
            if ts.len() >= 19 && ts.as_bytes()[4] == b'-' {
                if d.first.is_empty() {
                    d.first = ts.to_string();
                }
                d.last = ts.to_string();
            }
        }
        if !l.contains(" ERR ") {
            continue;
        }
        d.err_lines += 1;
        if l.contains(&format!("ERR Request failed {UNREACH}")) {
            d.requests += 1;
            if let Some(dest) = field(l, " dest=") {
                let host = dest
                    .split("://")
                    .nth(1)
                    .unwrap_or(dest)
                    .split('/')
                    .next()
                    .unwrap_or("");
                *d.by_dest.entry(host.to_string()).or_default() += 1;
            }
        } else if l.contains(&format!("ERR  {UNREACH}")) {
            d.events += 1;
            let o = field(l, " originService=").unwrap_or("?");
            *d.by_origin.entry(o.to_string()).or_default() += 1;
        }
    }
    d
}

pub fn diag_main(path: &str) -> i32 {
    let t0 = Instant::now();
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            println!("RESULT diag502 ERROR read {path}: {e}");
            return 1;
        }
    };
    let d = diag(&text);
    println!(
        "RESULT diag502 lines={} err_lines={} requests={} events={} first={} last={} ms={}",
        d.lines,
        d.err_lines,
        d.requests,
        d.events,
        d.first,
        d.last,
        t0.elapsed().as_millis()
    );
    for (k, v) in &d.by_origin {
        println!("RESULT diag502 origin {k} {v}");
    }
    for (k, v) in &d.by_dest {
        println!("RESULT diag502 dest {k} {v}");
    }
    0
}

pub fn sqlite_print(tag: &str, r: &crate::sqlite::Report) {
    for l in sqlite_lines(tag, r) {
        println!("{l}");
    }
}

pub fn sqlite_lines(tag: &str, r: &crate::sqlite::Report) -> Vec<String> {
    let mut v = vec![];
    v.push(format!(
        "RESULT {tag} page_size={} header_pages={} file_pages={} pages={} btree={} overflow={} freelist={}/{} ptrmap={} orphans={} objects={} errors={}",
        r.page_size, r.header_pages, r.file_pages, r.pages, r.btree_pages, r.overflow_pages, r.freelist_found, r.freelist_header, r.ptrmap_pages, r.orphans, r.objects.len(), r.errors.len()
    ));
    if let Some(w) = &r.wal {
        v.push(format!("RESULT {tag} wal frames={} valid={} commits={} db_pages={} distinct_pages={} error={:?}", w.frames_total, w.frames_valid, w.commits, w.db_pages_after_commit, w.distinct_pages, w.error));
    }
    for (t, n, root, e) in &r.objects {
        v.push(format!(
            "RESULT {tag} object {t} {n} root={root} entries={e}"
        ));
    }
    for e in r.errors.iter().take(5) {
        v.push(format!("RESULT {tag} error {e}"));
    }
    v
}

/// Check a DB copy (main file, then main+WAL), then two deliberately
/// corrupted copies made in the workdir; the checker must flag both.
pub fn dbsuite(db: &str, wal: &str) -> i32 {
    let t0 = Instant::now();
    let Ok(main) = std::fs::read(db) else {
        println!("RESULT dbcheck ERROR cannot read {db}");
        return 1;
    };
    let w = std::fs::read(wal).ok();
    let mut rc = 0;
    match crate::sqlite::check(&main, None) {
        Ok(r) => sqlite_print("dbcheck-main", &r),
        Err(e) => {
            println!("RESULT dbcheck-main ERROR {e}");
            rc = 1;
        }
    }
    let big = match crate::sqlite::check(&main, w.as_deref()) {
        Ok(r) => {
            sqlite_print("dbcheck-wal", &r);
            r.objects
                .iter()
                .filter(|o| o.2 > 1)
                .max_by_key(|o| o.3)
                .map(|o| o.2)
        }
        Err(e) => {
            println!("RESULT dbcheck-wal ERROR {e}");
            None
        }
    };
    println!("RESULT dbcheck ms={}", t0.elapsed().as_millis());
    // corruption 1: flip the page-type byte of the biggest object's root page
    // (main file only, so the WAL cannot mask it)
    let root = crate::sqlite::check(&main, None)
        .ok()
        .and_then(|r| {
            r.objects
                .iter()
                .filter(|o| o.2 > 1)
                .max_by_key(|o| o.3)
                .map(|o| o.2)
        })
        .or(big)
        .unwrap_or(2);
    let ps = crate::sqlite::check(&main, None)
        .map(|r| r.page_size)
        .unwrap_or(4096);
    let mut c1 = main.clone();
    let off = (root as usize - 1) * ps;
    if off < c1.len() {
        c1[off] = 0x07;
    }
    let _ = std::fs::write("corrupt-type.db", &c1);
    let c1 = std::fs::read("corrupt-type.db").unwrap_or_default();
    match crate::sqlite::check(&c1, None) {
        Ok(r) => println!(
            "RESULT dbcorrupt-type root={root} errors={} first={:?}",
            r.errors.len(),
            r.errors.first()
        ),
        Err(e) => println!("RESULT dbcorrupt-type root={root} errors=1 first={e:?}"),
    }
    // corruption 2: truncate one page off the end
    let c2 = &main[..main.len().saturating_sub(ps)];
    let _ = std::fs::write("corrupt-trunc.db", c2);
    let c2 = std::fs::read("corrupt-trunc.db").unwrap_or_default();
    match crate::sqlite::check(&c2, None) {
        Ok(r) => println!(
            "RESULT dbcorrupt-trunc errors={} first={:?}",
            r.errors.len(),
            r.errors.first()
        ),
        Err(e) => println!("RESULT dbcorrupt-trunc errors=1 first={e:?}"),
    }
    rc
}

/// A real job with a crash bug: reads the journal copy, then aborts.
pub fn crashjob(path: &str) -> i32 {
    let n = std::fs::read_to_string(path)
        .map(|t| t.lines().count())
        .unwrap_or(0);
    eprintln!("crashjob read {n} lines, now hitting its bug");
    std::process::abort();
}

/// A naive supervisor that restarts the crashing job forever.
pub fn crashloop(path: &str) -> i32 {
    let mut n = 0u64;
    loop {
        let st = Command::new("./rp")
            .args(["crashjob", path])
            .stderr(Stdio::null())
            .status();
        n += 1;
        let s = match st {
            Ok(s) => {
                use std::os::unix::process::ExitStatusExt;
                format!("sig={:?} code={:?}", s.signal(), s.code())
            }
            Err(e) => format!("spawn error {e}"),
        };
        println!("CRASH {n} {s}");
        let _ = std::io::stdout().flush();
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A "cache" bug: keeps every line of every re-read of the journal.
pub fn memhog(path: &str) -> i32 {
    let mut keep: Vec<String> = vec![];
    let mut bytes = 0usize;
    for it in 0..10_000 {
        let Ok(t) = std::fs::read_to_string(path) else {
            println!("RESULT memhog ERROR read");
            return 1;
        };
        for l in t.lines() {
            let s = format!("{it}:{l}");
            bytes += s.len();
            keep.push(s);
        }
        println!("MEM iter={it} held_mib={}", bytes >> 20);
        let _ = std::io::stdout().flush();
    }
    println!(
        "RESULT memhog finished held_mib={} (no cap hit)",
        bytes >> 20
    );
    0
}

/// Count the 502 requests in lines [a, b) of the journal (one worker).
pub fn chunk(path: &str, a: usize, b: usize) -> i32 {
    let t = std::fs::read_to_string(path).unwrap_or_default();
    let n = t
        .lines()
        .skip(a)
        .take(b - a)
        .filter(|l| l.contains(&format!("ERR Request failed {UNREACH}")))
        .count();
    std::thread::sleep(Duration::from_millis(800)); // the "I/O wait" of a real worker
    println!("{n}");
    0
}

/// Parallel job that forks one worker per chunk with no bound (the bug),
/// then falls back to doing the chunks that could not be spawned itself.
pub fn forkjob(path: &str, workers: usize) -> i32 {
    let t0 = Instant::now();
    let total_lines = std::fs::read_to_string(path)
        .map(|t| t.lines().count())
        .unwrap_or(0);
    let per = total_lines.div_ceil(workers).max(1);
    let mut kids = vec![];
    let mut failed = vec![];
    let mut first_err = String::new();
    for w in 0..workers {
        let (a, b) = (w * per, ((w + 1) * per).min(total_lines));
        match Command::new("./rp")
            .args(["chunk", path, &a.to_string(), &b.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => kids.push(c),
            Err(e) => {
                if first_err.is_empty() {
                    first_err = e.to_string();
                }
                failed.push((a, b));
            }
        }
    }
    let spawned = kids.len();
    let mut total = 0u64;
    for mut c in kids {
        let mut s = String::new();
        if let Some(mut o) = c.stdout.take() {
            let _ = o.read_to_string(&mut s);
        }
        let _ = c.wait();
        total += s.trim().parse::<u64>().unwrap_or(0);
    }
    let t = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = t.lines().collect();
    for (a, b) in &failed {
        total += lines[*a..*b]
            .iter()
            .filter(|l| l.contains(&format!("ERR Request failed {UNREACH}")))
            .count() as u64;
    }
    println!("RESULT forkjob workers={workers} spawned={spawned} spawn_failed={} first_err={first_err:?} total_requests={total} ms={}", failed.len(), t0.elapsed().as_millis());
    0
}

pub fn sleeper() -> i32 {
    std::thread::sleep(Duration::from_secs(3600));
    0
}

/// setsid() then start a sleeper and exit: the classic escaped daemon.
pub fn detach(tag: &str) -> i32 {
    // SAFETY: plain syscall.
    unsafe { libc::setsid() };
    let _ = Command::new("./rp")
        .args(["sleeper", tag])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    0
}

/// A task that hangs: two helpers (one detached with setsid), then blocks.
pub fn hang(tag: &str) -> i32 {
    let _a = Command::new("./rp")
        .args(["sleeper", &format!("{tag}-a")])
        .stdout(Stdio::null())
        .spawn();
    let _ = Command::new("./rp")
        .args(["detach", &format!("{tag}-b")])
        .status();
    println!("HANG started helpers, now blocking");
    let _ = std::io::stdout().flush();
    std::thread::sleep(Duration::from_secs(3600));
    0
}

/// Try live loopback services, the internet and DNS; never send data to a
/// live port (connect only). The decoy ports belong to the runner.
pub fn net(decoy_tcp: u16, decoy_udp: u16) -> i32 {
    let mut targets: Vec<(String, String)> = vec![];
    for p in [8670u16, 8672, 8642, 8650, 8420] {
        targets.push((format!("live-{p}"), format!("127.0.0.1:{p}")));
    }
    targets.push(("live-v6-8670".into(), "[::1]:8670".into()));
    targets.push(("internet-1.1.1.1:443".into(), "1.1.1.1:443".into()));
    targets.push(("internet-8.8.8.8:53".into(), "8.8.8.8:53".into()));
    targets.push(("decoy-tcp".into(), format!("127.0.0.1:{decoy_tcp}")));
    for (name, a) in &targets {
        let addr: SocketAddr = a.parse().unwrap();
        let t0 = Instant::now();
        match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
            Ok(mut s) => {
                if name.starts_with("decoy") {
                    let _ = s.write_all(b"PING-FROM-SANDBOX");
                }
                println!(
                    "RESULT net {name} ESCAPED connected ms={}",
                    t0.elapsed().as_millis()
                );
            }
            Err(e) => println!(
                "RESULT net {name} CONTAINED {e} ms={}",
                t0.elapsed().as_millis()
            ),
        }
    }
    for (name, a) in [
        ("decoy-udp".to_string(), format!("127.0.0.1:{decoy_udp}")),
        ("dns-udp-8.8.8.8".to_string(), "8.8.8.8:53".to_string()),
    ] {
        let r = UdpSocket::bind("0.0.0.0:0").and_then(|s| s.send_to(b"\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\x03one\x06galion\x03app\x00\x00\x01\x00\x01", a.as_str()));
        match r {
            Ok(n) => println!("RESULT net {name} ESCAPED sent {n} bytes"),
            Err(e) => println!("RESULT net {name} CONTAINED {e}"),
        }
    }
    match "one.galion.app:443".to_socket_addrs() {
        Ok(it) => println!("RESULT net resolve ESCAPED {} addrs", it.count()),
        Err(e) => println!("RESULT net resolve CONTAINED {e}"),
    }
    let dev = std::fs::read_to_string("/proc/net/dev").ok();
    let mut ifs: Vec<String> = dev
        .as_deref()
        .unwrap_or_default()
        .lines()
        .skip(2)
        .filter_map(|l| l.split(':').next())
        .map(|s| s.trim().to_string())
        .collect();
    if dev.is_none() {
        // No /proc/net (the /proc mask of slot 191 has no pid dirs): ask the
        // kernel for the interface names of this netns directly.
        for idx in 1..64u32 {
            let mut buf = [0 as libc::c_char; libc::IF_NAMESIZE];
            // SAFETY: buf is IF_NAMESIZE long, as if_indextoname requires.
            let p = unsafe { libc::if_indextoname(idx, buf.as_mut_ptr()) };
            if !p.is_null() {
                // SAFETY: if_indextoname wrote a NUL-terminated name into buf.
                let n = unsafe { std::ffi::CStr::from_ptr(p) };
                ifs.push(n.to_string_lossy().into_owned());
            }
        }
    }
    println!("RESULT net interfaces {} {:?}", ifs.len(), ifs);
    0
}

/// Try to read secrets: directories are listed (count only), files are
/// opened and ONE byte is read to prove readability; nothing is printed.
pub fn secrets(paths: &[String]) -> i32 {
    for p in paths {
        let md = std::fs::metadata(p);
        let r = match md {
            Err(e) => format!("CONTAINED {e}"),
            Ok(m) if m.is_dir() => match std::fs::read_dir(p) {
                Ok(rd) => {
                    let n = rd.count();
                    if n == 0 {
                        "HIDDEN listed 0 entries (empty cover)".to_string()
                    } else {
                        format!("ESCAPED listed {n} entries")
                    }
                }
                Err(e) => format!("CONTAINED {e}"),
            },
            Ok(_) => match std::fs::File::open(p) {
                Ok(mut f) => {
                    let mut b = [0u8; 1];
                    match f.read(&mut b) {
                        Ok(n) => format!("ESCAPED readable ({n} byte read, not shown)"),
                        Err(e) => format!("CONTAINED read: {e}"),
                    }
                }
                Err(e) => format!("CONTAINED {e}"),
            },
        };
        println!("RESULT secret {p} {r}");
    }
    0
}

/// Write `count` files of `mb` MiB in the cwd (1 MiB chunks).
pub fn fill(mb: usize, count: usize) -> i32 {
    // how SIGXFSZ will be delivered (ignored => write returns EFBIG instead)
    let st = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let mask = |k: &str| {
        st.lines()
            .find(|l| l.starts_with(k))
            .and_then(|l| u64::from_str_radix(l[k.len()..].trim(), 16).ok())
            .unwrap_or(0)
    };
    let bit = 1u64 << (libc::SIGXFSZ - 1);
    println!(
        "RESULT fill sigxfsz ignored={} blocked={} pid_in_ns={}",
        mask("SigIgn:") & bit != 0,
        mask("SigBlk:") & bit != 0,
        std::process::id()
    );
    let chunk = vec![0xa5u8; 1 << 20];
    let mut total = 0usize;
    for i in 0..count {
        let name = format!("fill-{i:03}.bin");
        let mut f = match std::fs::File::create(&name) {
            Ok(f) => f,
            Err(e) => {
                println!("RESULT fill stop file={i} total_mib={total} err={e}");
                return 0;
            }
        };
        for _ in 0..mb {
            if let Err(e) = f.write_all(&chunk) {
                println!("RESULT fill stop file={i} total_mib={total} err={e}");
                return 0;
            }
            total += 1;
            if total.is_multiple_of(4) {
                println!("W {total}");
                let _ = std::io::stdout().flush();
            }
        }
        if let Err(e) = f.sync_all() {
            println!("RESULT fill stop file={i} total_mib={total} err=sync {e}");
            return 0;
        }
    }
    println!("RESULT fill done files={count} total_mib={total}");
    0
}

/// What a sandboxed tool can learn about the host through the shared /proc.
/// Counts only; cmdlines are never printed.
pub fn procview() -> i32 {
    // SAFETY: plain id getter.
    let me = unsafe { libc::getuid() };
    let (
        mut pids,
        mut comm,
        mut cmd,
        mut env,
        mut exe,
        mut other_uid,
        mut other_cmd,
        mut other_env,
        mut secretish,
    ) = (0, 0, 0, 0, 0, 0, 0, 0, 0);
    let mut names = std::collections::BTreeSet::new();
    let mut known = BTreeMap::new();
    for k in [
        "galion-nerve",
        "galion-nerve-sh",
        "hermes-gateway",
        "openmind-memory",
        "cloudflared",
        "tuner",
        "sshd",
        "systemd",
    ] {
        known.insert(k, false);
    }
    for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let n = e.file_name();
        let Some(s) = n.to_str() else { continue };
        if !s.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        pids += 1;
        let d = e.path();
        let status = std::fs::read_to_string(d.join("status")).unwrap_or_default();
        let uid: u32 = status
            .lines()
            .find(|l| l.starts_with("Uid:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|x| x.parse().ok())
            .unwrap_or(u32::MAX);
        let other = uid != me;
        if other {
            other_uid += 1;
        }
        if let Ok(c) = std::fs::read_to_string(d.join("comm")) {
            comm += 1;
            let c = c.trim().to_string();
            if let Some(v) = known.get_mut(c.as_str()) {
                *v = true;
            }
            names.insert(c);
        }
        if let Ok(c) = std::fs::read(d.join("cmdline")) {
            if !c.is_empty() {
                cmd += 1;
                if other {
                    other_cmd += 1;
                }
                let low = String::from_utf8_lossy(&c).to_ascii_lowercase();
                if [
                    "token", "secret", "passw", "api_key", "apikey", "bearer", "--key",
                ]
                .iter()
                .any(|k| low.contains(k))
                {
                    secretish += 1;
                }
            }
        }
        if std::fs::File::open(d.join("environ"))
            .and_then(|mut f| f.read(&mut [0u8; 1]))
            .is_ok()
        {
            env += 1;
            if other {
                other_env += 1;
            }
        }
        if std::fs::read_link(d.join("exe")).is_ok() {
            exe += 1;
        }
    }
    println!("RESULT procview pids={pids} other_uid={other_uid} comm_readable={comm} distinct_names={} cmdline_readable={cmd} other_uid_cmdline_readable={other_cmd} environ_readable={env} other_uid_environ_readable={other_env} exe_link_readable={exe} cmdlines_with_secretish_words={secretish}", names.len());
    let k: Vec<String> = known.iter().map(|(k, v)| format!("{k}={v}")).collect();
    println!("RESULT procview services_identifiable {}", k.join(" "));
    for f in [
        "/proc/meminfo",
        "/proc/loadavg",
        "/proc/uptime",
        "/proc/cpuinfo",
        "/proc/net/tcp",
        "/proc/mounts",
        "/proc/1/cmdline",
        "/proc/1/environ",
    ] {
        match std::fs::read(f) {
            Ok(b) => println!(
                "RESULT procview file {f} readable lines={}",
                b.split(|c| *c == b'\n').filter(|l| !l.is_empty()).count()
            ),
            Err(e) => println!("RESULT procview file {f} denied {e}"),
        }
    }
    0
}
