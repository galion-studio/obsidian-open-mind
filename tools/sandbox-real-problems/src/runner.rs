// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Host side: runs each scenario through openmind_sandbox::run and judges it.
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use openmind_sandbox::Preset;
use openmind_sandbox::{run, Outcome, SandboxPolicy, Status};

const MIB: u64 = 1024 * 1024;

struct Ctx {
    base: PathBuf,
    exe: PathBuf,
    cg: Option<PathBuf>,
    refs: BTreeMap<String, String>,
    summary: Vec<String>,
    fails: u32,
}

struct Scen<'a> {
    c: &'a mut Ctx,
    id: &'static str,
    t0: Instant,
    bad: Vec<String>,
    notes: Vec<String>,
}

impl Scen<'_> {
    fn check(&mut self, ok: bool, what: impl Into<String>) {
        let what = what.into();
        println!(
            "HOST {} [{}] {}",
            if ok { "OK" } else { "FAIL" },
            self.id,
            what
        );
        if !ok {
            self.bad.push(what);
        }
    }
    fn note(&mut self, n: impl Into<String>) {
        let n = n.into();
        println!("NOTE [{}] {n}", self.id);
        self.notes.push(n);
    }
    fn end(self, verdict_if_ok: &str) {
        let ms = self.t0.elapsed().as_millis();
        let v = if self.bad.is_empty() {
            verdict_if_ok.to_string()
        } else {
            "FAIL".to_string()
        };
        if !self.bad.is_empty() {
            self.c.fails += 1;
        }
        let mut s = format!("SCENARIO {} {v} ms={ms}", self.id);
        if !self.bad.is_empty() {
            s.push_str(&format!(" failed: {}", self.bad.join("; ")));
        }
        if !self.notes.is_empty() {
            s.push_str(&format!(" | {}", self.notes.join("; ")));
        }
        println!("{s}");
        self.c.summary.push(s);
    }
}

impl Ctx {
    fn wd(&self, name: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = self.base.join("w").join(name);
        std::fs::create_dir_all(&p).unwrap();
        let p = p.canonicalize().unwrap();
        let dst = p.join("rp");
        std::fs::copy(&self.exe, &dst).unwrap();
        std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }
    fn data(&self, f: &str) -> String {
        self.base
            .join("data")
            .join(f)
            .to_string_lossy()
            .into_owned()
    }
    fn r(&self, k: &str) -> Option<&String> {
        self.refs.get(k)
    }
    fn scen(&mut self, id: &'static str, title: &str, expected: &str) -> Scen<'_> {
        println!("\n=== {id} {title}\nEXPECT {expected}");
        Scen {
            c: self,
            id,
            t0: Instant::now(),
            bad: vec![],
            notes: vec![],
        }
    }
}

/// The shipped read-only-tool preset, as a real caller would use it.
fn pol(wd: &Path) -> SandboxPolicy {
    Preset::read_only_tool().policy(wd)
}

fn sb(wd: &Path, p: &SandboxPolicy, args: &[&str], show_layers: bool) -> Outcome {
    let mut v = vec!["./rp".to_string()];
    v.extend(args.iter().map(|s| s.to_string()));
    let mut p = p.clone();
    p.workdir = wd.to_path_buf();
    let o = run(&v, &p);
    println!(
        "RUN {:?} status={:?} timed_out={} wall_ms={} error={:?} stdout_truncated={}",
        args,
        o.status,
        o.timed_out,
        o.wall.as_millis(),
        o.error,
        o.stdout_truncated
    );
    if show_layers {
        let l: Vec<String> = o
            .enforced
            .iter()
            .map(|(n, e)| {
                format!(
                    "{n}={}",
                    if e.is_enforced() {
                        "Enforced".into()
                    } else {
                        format!("{e:?}")
                    }
                )
            })
            .collect();
        println!("LAYERS {}", l.join(" | "));
    }
    let out = o.stdout_str();
    let mut other = 0;
    for l in out.lines() {
        if l.starts_with("RESULT") {
            println!("{l}");
        } else {
            other += 1;
        }
    }
    if other > 0 {
        let last = out
            .lines()
            .rfind(|l| !l.starts_with("RESULT"))
            .unwrap_or("");
        println!("OUT {other} progress lines, last: {last}");
    }
    let err = String::from_utf8_lossy(&o.stderr);
    if let Some(l) = err.lines().last() {
        println!("STDERR last: {}", l.chars().take(200).collect::<String>());
    }
    if let Some(c) = &o.cgroup {
        println!("CGROUP pids_max_hits={} oom_kills={} mem_max_hits={} mem_peak={} ({} MiB) pids_peak={} removed={}", c.pids_max_hits, c.oom_kills, c.mem_max_hits, c.mem_peak, c.mem_peak / MIB, c.pids_peak, c.removed);
    }
    o
}

/// Live (non-zombie) processes matching a predicate on (cmdline, cwd).
fn scan(f: impl Fn(&[u8], Option<PathBuf>) -> bool) -> usize {
    let me = std::process::id().to_string();
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            let s = n.to_string_lossy();
            if !s.bytes().all(|b| b.is_ascii_digit()) || s == me {
                return false;
            }
            let st = std::fs::read_to_string(e.path().join("stat")).unwrap_or_default();
            if st.contains(") Z ") || st.is_empty() {
                return false;
            }
            let cmd = std::fs::read(e.path().join("cmdline")).unwrap_or_default();
            f(&cmd, std::fs::read_link(e.path().join("cwd")).ok())
        })
        .count()
}

fn contains(h: &[u8], n: &str) -> bool {
    h.windows(n.len()).any(|w| w == n.as_bytes())
}

fn in_wd(wd: &Path) -> usize {
    let wd = wd.to_path_buf();
    scan(move |_, cwd| cwd.as_deref() == Some(wd.as_path()))
}

fn field(out: &str, prefix: &str, key: &str) -> Option<String> {
    out.lines().find(|l| l.starts_with(prefix)).and_then(|l| {
        l.split_whitespace()
            .find_map(|t| t.strip_prefix(&format!("{key}=")).map(|v| v.to_string()))
    })
}

fn cgsetup() -> Option<PathBuf> {
    let s = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let rel = s
        .lines()
        .find_map(|l| l.strip_prefix("0::"))?
        .trim()
        .to_string();
    let cg = PathBuf::from("/sys/fs/cgroup").join(rel.trim_start_matches('/'));
    println!("CGROUP self {}", cg.display());
    if !rel.starts_with("/sbxtest.slice/") {
        println!("NOTE not inside sbxtest.slice; per-run cgroup scenarios will fail");
        return None;
    }
    let sup = cg.join("sup");
    let r = std::fs::create_dir_all(&sup)
        .and_then(|_| std::fs::write(sup.join("cgroup.procs"), std::process::id().to_string()))
        .and_then(|_| std::fs::write(cg.join("cgroup.subtree_control"), "+cpu +memory +pids"));
    match r {
        Ok(()) => {
            let sc = std::fs::read_to_string(cg.join("cgroup.subtree_control")).unwrap_or_default();
            println!(
                "CGROUP delegated parent {} subtree_control={}",
                cg.display(),
                sc.trim()
            );
            Some(cg)
        }
        Err(e) => {
            println!("NOTE cgroup setup failed: {e}");
            None
        }
    }
}

fn s1_diag(c: &mut Ctx) {
    let j = c.data("journal.log");
    let wd = c.wd("s1-diag502");
    let mut s = c.scen("S1", "cloudflared 502 diagnosis on a masked copy of the last 24 h of journal", "tool exits 0; every count equals root's grep count on the live journal for the same window");
    let o = sb(&wd, &pol(&wd), &["diag502", &j], true);
    let out = o.stdout_str();
    s.check(
        o.status == Status::Exited(0),
        format!("tool exited 0 (got {:?})", o.status),
    );
    let mut got: BTreeMap<String, String> = BTreeMap::new();
    for k in ["lines", "requests", "events"] {
        if let Some(v) = field(&out, "RESULT diag502 lines=", k) {
            got.insert(format!("j.{k}"), v);
        }
    }
    for l in out.lines() {
        let t: Vec<&str> = l.split_whitespace().collect();
        if t.len() == 5 && t[0] == "RESULT" && (t[2] == "origin" || t[2] == "dest") {
            got.insert(format!("j.{} {}", t[2], t[3]), t[4].to_string());
        }
    }
    let want: BTreeMap<String, String> =
        s.c.refs
            .iter()
            .filter(|(k, _)| k.starts_with("j.") && k.as_str() != "j.masked_ipv4_left")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
    for (k, v) in &want {
        let g = got.get(k).cloned().unwrap_or_else(|| "missing".into());
        s.check(&g == v, format!("{k}: sandbox {g} vs host {v}"));
    }
    for k in got.keys().filter(|k| !want.contains_key(*k)) {
        s.check(false, format!("{k} reported by sandbox but not by host"));
    }
    let left = s.c.r("j.masked_ipv4_left").cloned().unwrap_or_default();
    s.check(
        left == "0",
        format!("masked copy has {left} non-loopback IPv4 left (host grep)"),
    );
    s.note(format!("{} keys compared", want.len()));
    s.end("PASS");
}

fn s2_db(c: &mut Ctx) {
    let db = c.data("db/vectors.db");
    let wal = c.data("db/vectors.db-wal");
    let wd = c.wd("s2-sqlite");
    let mut s = c.scen("S2", "integrity check of a COPY of the openmind-memory SQLite DB (+WAL)", "copy has 0 structural errors with and without the WAL; header matches root's od reading; sandbox and unsandboxed results identical; both deliberate corruptions detected");
    let mut p = pol(&wd);
    p.fsize_bytes = 64 * MIB;
    p.wall_timeout = Duration::from_secs(30);
    let o = sb(&wd, &p, &["dbsuite", &db, &wal], false);
    let out = o.stdout_str();
    s.check(
        o.status == Status::Exited(0),
        format!("tool exited 0 (got {:?})", o.status),
    );
    let me = std::fs::read(&db).unwrap_or_default();
    let mw = std::fs::read(&wal).ok();
    let mut host = vec![];
    for (tag, w) in [("dbcheck-main", None), ("dbcheck-wal", mw.as_deref())] {
        match crate::sqlite::check(&me, w) {
            Ok(r) => host.extend(crate::payload::sqlite_lines(tag, &r)),
            Err(e) => host.push(format!("RESULT {tag} ERROR {e}")),
        }
    }
    let sbl: Vec<String> = out
        .lines()
        .filter(|l| l.starts_with("RESULT dbcheck-"))
        .map(String::from)
        .collect();
    s.check(
        sbl == host,
        format!(
            "sandboxed and unsandboxed checker output identical ({} lines)",
            host.len()
        ),
    );
    for tag in ["dbcheck-main", "dbcheck-wal"] {
        let e = field(&out, &format!("RESULT {tag} page_size="), "errors").unwrap_or_default();
        s.check(e == "0", format!("{tag} errors={e}"));
    }
    for k in ["page_size", "header_pages"] {
        let g = field(&out, "RESULT dbcheck-main page_size=", k).unwrap_or_default();
        let w = s.c.r(&format!("db.{k}")).cloned().unwrap_or_default();
        s.check(g == w, format!("{k}: checker {g} vs root od {w}"));
    }
    let fb = s.c.r("db.file_bytes").cloned().unwrap_or_default();
    s.check(
        fb == me.len().to_string(),
        format!("copy size {} vs original {fb}", me.len()),
    );
    for t in ["dbcorrupt-type", "dbcorrupt-trunc"] {
        let e: u64 = field(&out, &format!("RESULT {t} "), "errors")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        s.check(e > 0, format!("{t} detected ({e} errors)"));
    }
    s.end("PASS");
}

fn s3_crash(c: &mut Ctx) {
    let j = c.data("journal.log");
    let wd = c.wd("s3-crashloop");
    let cg = c.cg.clone();
    let mut s = c.scen("S3", "crash-looping job under a naive restart-forever supervisor", "every child dies of SIGABRT and is restarted; the run is killed at the 4 s wall timeout; 0 processes left; per-run cgroup removed");
    let mut p = pol(&wd);
    p.wall_timeout = Duration::from_secs(4);
    p.cgroup_parent = cg.clone();
    let o = sb(&wd, &p, &["crashloop", &j], false);
    let out = o.stdout_str();
    let crashes = out.lines().filter(|l| l.starts_with("CRASH ")).count();
    let aborts = out
        .lines()
        .filter(|l| l.starts_with("CRASH ") && l.contains(" sig=Some(6)"))
        .count();
    std::thread::sleep(Duration::from_millis(300));
    let left = in_wd(&wd);
    s.check(o.timed_out, "killed at wall timeout");
    s.check(
        o.wall < Duration::from_millis(4600),
        format!("wall {} ms", o.wall.as_millis()),
    );
    s.check(
        crashes >= 5,
        format!("{crashes} crashes observed and restarted"),
    );
    s.check(
        aborts == crashes,
        format!("{aborts}/{crashes} children ended with SIGABRT"),
    );
    s.check(left == 0, format!("{left} processes left in the workdir"));
    if cg.is_some() {
        s.check(
            o.cgroup.as_ref().map(|x| x.removed).unwrap_or(false),
            "per-run cgroup removed",
        );
    } else {
        s.check(false, "no delegated cgroup");
    }
    s.end("PASS");
}

fn s4_mem(c: &mut Ctx) {
    let j = c.data("journal.log");
    let wd = c.wd("s4-memhog");
    let cg = c.cg.clone();
    let mut s = c.scen("S4", "runaway memory: a 'cache' that keeps every journal line of every re-read", "A: memory.max 128 MiB (RLIMIT_AS 2 GiB) -> cgroup OOM kill, peak <= 128 MiB; B: no cgroup, RLIMIT_AS 128 MiB -> allocation fails, process dies on a signal");
    match cg {
        Some(cg) => {
            let mut p = pol(&wd);
            p.cgroup_parent = Some(cg);
            p.cgroup_mem_bytes = 128 * MIB;
            p.mem_bytes = 2048 * MIB;
            p.cpu_secs = 30;
            p.wall_timeout = Duration::from_secs(40);
            let o = sb(&wd, &p, &["memhog", &j], false);
            let cs = o.cgroup.clone().unwrap_or_default();
            let held = o
                .stdout_str()
                .lines()
                .rfind(|l| l.starts_with("MEM "))
                .unwrap_or("")
                .to_string();
            s.check(
                o.status == Status::Signaled(9) && !o.timed_out,
                format!(
                    "A killed by SIGKILL, not by the timeout (status {:?}, timed_out {})",
                    o.status, o.timed_out
                ),
            );
            s.check(cs.oom_kills >= 1, format!("A oom_kills={}", cs.oom_kills));
            s.check(
                cs.mem_peak > 0 && cs.mem_peak <= 128 * MIB,
                format!("A memory.peak {} MiB <= 128", cs.mem_peak / MIB),
            );
            s.check(cs.removed, "A per-run cgroup removed");
            s.note(format!(
                "A last progress: {held}; wall {} ms",
                o.wall.as_millis()
            ));
        }
        None => s.check(false, "A no delegated cgroup"),
    }
    let mut p = pol(&wd);
    p.mem_bytes = 128 * MIB;
    p.cpu_secs = 30;
    p.wall_timeout = Duration::from_secs(40);
    let o = sb(&wd, &p, &["memhog", &j], false);
    let held = o
        .stdout_str()
        .lines()
        .rfind(|l| l.starts_with("MEM "))
        .unwrap_or("")
        .to_string();
    s.check(
        o.status != Status::Exited(0) && !o.stdout_str().contains("finished") && !o.timed_out,
        format!("B stopped by RLIMIT_AS (status {:?})", o.status),
    );
    s.note(format!(
        "B last progress: {held}; wall {} ms",
        o.wall.as_millis()
    ));
    s.check(in_wd(&wd) == 0, "no processes left");
    s.end("PASS");
}

fn s5_fork(c: &mut Ctx) {
    let j = c.data("journal.log");
    let wd = c.wd("s5-forkjob");
    let cg = c.cg.clone();
    let want = c.r("j.requests").cloned().unwrap_or_default();
    let mut s = c.scen("S5", "fork storm: parallel 502 count that spawns 200 workers at once", "pids.max 24 refuses forks (EAGAIN), pids.peak <= 24, the job falls back in-process and still reports the exact host count");
    let Some(cg) = cg else {
        s.check(false, "no delegated cgroup");
        return s.end("PASS");
    };
    let mut p = pol(&wd);
    p.cgroup_parent = Some(cg);
    p.cgroup_pids_max = 24;
    p.cgroup_mem_bytes = 512 * MIB;
    p.nofile = 1024;
    p.cpu_secs = 30;
    p.wall_timeout = Duration::from_secs(60);
    let o = sb(&wd, &p, &["forkjob", &j, "200"], false);
    let out = o.stdout_str();
    let cs = o.cgroup.clone().unwrap_or_default();
    let spawned: u64 = field(&out, "RESULT forkjob", "spawned")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let failed: u64 = field(&out, "RESULT forkjob", "spawn_failed")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let total = field(&out, "RESULT forkjob", "total_requests").unwrap_or_default();
    s.check(
        o.status == Status::Exited(0),
        format!("job exited 0 ({:?})", o.status),
    );
    s.check(
        failed > 0 && spawned < 24,
        format!("spawned {spawned}, refused {failed}"),
    );
    s.check(
        cs.pids_max_hits > 0,
        format!("pids.events max={}", cs.pids_max_hits),
    );
    s.check(cs.pids_peak <= 24, format!("pids.peak={}", cs.pids_peak));
    s.check(total == want, format!("job total {total} vs host {want}"));
    s.check(in_wd(&wd) == 0, "no processes left");
    s.end("PASS");
}

fn s6_hang(c: &mut Ctx) {
    let wd = c.wd("s6-hang");
    let cg = c.cg.clone();
    let mut s = c.scen("S6", "hung task with a helper and a setsid()-detached daemon", "killed at the 3 s wall timeout (wall < 3.6 s); 3 tagged processes alive mid-run, 0 afterwards; both without and with the per-run cgroup");
    for (v, cgp) in [("nocg", None), ("cg", cg.clone())] {
        if v == "cg" && cgp.is_none() {
            s.check(false, "cg variant: no delegated cgroup");
            continue;
        }
        let tag = format!("hang{}{v}", std::process::id());
        let mut p = pol(&wd);
        p.wall_timeout = Duration::from_secs(3);
        p.cgroup_parent = cgp;
        let t2 = tag.clone();
        let mid = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            scan(|cmd, _| contains(cmd, &t2))
        });
        let o = sb(&wd, &p, &["hang", &tag], false);
        let mid = mid.join().unwrap_or(0);
        std::thread::sleep(Duration::from_millis(500));
        let after = scan(|cmd, _| contains(cmd, &tag));
        s.check(o.timed_out, format!("{v}: timed_out"));
        s.check(
            o.wall >= Duration::from_secs(3) && o.wall < Duration::from_millis(3600),
            format!("{v}: wall {} ms", o.wall.as_millis()),
        );
        s.check(
            mid == 3,
            format!("{v}: {mid} tagged processes mid-run (hang + helper + detached)"),
        );
        s.check(
            after == 0,
            format!("{v}: {after} tagged processes after teardown"),
        );
    }
    s.end("PASS");
}

fn s7_net(c: &mut Ctx) {
    let wd = c.wd("s7-net");
    let mut s = c.scen("S7", "task reaching for live services 8670/8672/8642/8650/8420, the internet and DNS", "every connect/send/resolve fails (ENETUNREACH or resolver error); only 'lo' visible; decoy host listeners get 0 connections and 0 bytes; positive control from the host does reach the decoy");
    let tl = TcpListener::bind("127.0.0.1:0").unwrap();
    let tport = tl.local_addr().unwrap().port();
    tl.set_nonblocking(true).unwrap();
    let ul = UdpSocket::bind("127.0.0.1:0").unwrap();
    let uport = ul.local_addr().unwrap().port();
    ul.set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (conns, bytes, dgrams) = (
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
    );
    let th = {
        let (stop, conns, bytes, dgrams) =
            (stop.clone(), conns.clone(), bytes.clone(), dgrams.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                if let Ok((mut st, _)) = tl.accept() {
                    conns.fetch_add(1, Ordering::SeqCst);
                    st.set_nonblocking(false).ok();
                    st.set_read_timeout(Some(Duration::from_millis(500))).ok();
                    let mut b = vec![];
                    let _ = st.read_to_end(&mut b);
                    bytes.fetch_add(b.len() as u64, Ordering::SeqCst);
                }
                let mut buf = [0u8; 512];
                if ul.recv_from(&mut buf).is_ok() {
                    dgrams.fetch_add(1, Ordering::SeqCst);
                }
            }
        })
    };
    let o = sb(
        &wd,
        &pol(&wd),
        &["net", &tport.to_string(), &uport.to_string()],
        false,
    );
    std::thread::sleep(Duration::from_millis(600));
    let (c0, b0, d0) = (
        conns.load(Ordering::SeqCst),
        bytes.load(Ordering::SeqCst),
        dgrams.load(Ordering::SeqCst),
    );
    // positive control from the host side
    if let Ok(mut st) = TcpStream::connect(("127.0.0.1", tport)) {
        let _ = st.write_all(b"CONTROL");
    }
    let _ =
        UdpSocket::bind("127.0.0.1:0").and_then(|u| u.send_to(b"CONTROL", ("127.0.0.1", uport)));
    std::thread::sleep(Duration::from_millis(900));
    stop.store(true, Ordering::SeqCst);
    let _ = th.join();
    let (c1, b1, d1) = (
        conns.load(Ordering::SeqCst),
        bytes.load(Ordering::SeqCst),
        dgrams.load(Ordering::SeqCst),
    );
    let out = o.stdout_str();
    let probes: Vec<&str> = out
        .lines()
        .filter(|l| l.starts_with("RESULT net ") && !l.starts_with("RESULT net interfaces"))
        .collect();
    let esc = probes.iter().filter(|l| l.contains(" ESCAPED")).count();
    s.check(o.enforced("net"), "net layer Enforced");
    s.check(
        probes.len() == 12 && esc == 0,
        format!("{} probes, {esc} escaped", probes.len()),
    );
    s.check(
        out.contains("RESULT net interfaces 1 [\"lo\"]"),
        "only lo inside the sandbox",
    );
    s.check(
        c0 == 0 && b0 == 0 && d0 == 0,
        format!("decoys during sandbox run: {c0} conns, {b0} bytes, {d0} datagrams"),
    );
    s.check(
        c1 - c0 == 1 && b1 - b0 == 7 && d1 - d0 == 1,
        format!(
            "positive control reached decoys: +{} conn, +{} bytes, +{} datagram",
            c1 - c0,
            b1 - b0,
            d1 - d0
        ),
    );
    s.end("PASS");
}

fn s8_secrets(c: &mut Ctx) {
    let wd = c.wd("s8-secrets");
    let list = std::fs::read_to_string(c.data("secret-paths.txt")).unwrap_or_default();
    let paths: Vec<String> = list
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let mut s = c.scen("S8", "task trying to read secrets: galion-light/secrets, /root, /root/galion-secrets, .env files, /etc/shadow, service /proc/<pid>/environ", "shipped preset (hides /opt/galion since sandbox-hide-galion): 0 of N paths readable, templates included; the two baselines (preset without /opt/galion, hide_paths emptied) are measured to show what the fix closes");
    s.note(format!("{} paths", paths.len()));
    let mut args = vec!["secrets"];
    args.extend(paths.iter().map(String::as_str));
    let galion = PathBuf::from("/opt/galion");
    s.check(
        pol(&wd).hide_paths.contains(&galion),
        "preset: shipped read-only-tool hides /opt/galion",
    );
    // The shipped preset is judged. The two baselines are only measured:
    // the old preset (before the fix) and unix permissions alone.
    for v in ["preset", "preset-before-fix", "nohide"] {
        let mut p = pol(&wd);
        match v {
            "preset-before-fix" => p.hide_paths.retain(|h| h != &galion),
            "nohide" => p.hide_paths.clear(),
            _ => {}
        }
        let o = sb(&wd, &p, &args, false);
        let out = o.stdout_str();
        let res: Vec<&str> = out
            .lines()
            .filter(|l| l.starts_with("RESULT secret "))
            .collect();
        let esc_all: Vec<&str> = res
            .iter()
            .filter(|l| l.contains(" ESCAPED"))
            .copied()
            .collect();
        // *.env.example files are templates kept world-readable on purpose.
        let (tpl, esc): (Vec<&str>, Vec<&str>) = esc_all.iter().partition(|l| {
            l.split_whitespace()
                .nth(2)
                .is_some_and(|p| p.ends_with(".example"))
        });
        let names: Vec<&str> = esc
            .iter()
            .map(|l| l.split_whitespace().nth(2).unwrap_or(""))
            .collect();
        let hidden = res.iter().filter(|l| l.contains(" HIDDEN")).count();
        let line = format!(
            "{v}: {} readable of {} ({} non-template {names:?}, {} .env.example templates); {} contained, {hidden} hidden",
            esc_all.len(),
            paths.len(),
            esc.len(),
            tpl.len(),
            res.len() - esc_all.len() - hidden
        );
        if v == "preset" {
            s.check(
                res.len() == paths.len(),
                format!("{v}: {} of {} paths judged", res.len(), paths.len()),
            );
            s.check(esc_all.is_empty() && !res.is_empty(), line);
        } else {
            s.note(format!("baseline {line}"));
        }
    }
    s.end("PASS");
}

fn s9_disk(c: &mut Ctx) {
    let cg = c.cg.clone();
    let wa = c.wd("s9a-fsize");
    let wb = c.wd("s9b-many");
    let tm = c.base.join("w").join("s9c-tmpfs");
    let mut s = c.scen("S9", "disk filling the workdir", "a: one 64 MiB file under RLIMIT_FSIZE 16 MiB -> SIGXFSZ kill (or EFBIG when SIGXFSZ is ignored), file stops at exactly 16 MiB; b: 32 x 8 MiB files -> all written (no total quota in openmind-sandbox: GAP); c: same on a 32 MiB tmpfs workdir -> ENOSPC at ~32 MiB");
    let mut p = pol(&wa);
    p.fsize_bytes = 16 * MIB;
    p.wall_timeout = Duration::from_secs(20);
    let o = sb(&wa, &p, &["fill", "64", "1"], false);
    let sz = std::fs::metadata(wa.join("fill-000.bin"))
        .map(|m| m.len())
        .unwrap_or(0);
    let efbig = o.stdout_str().contains("File too large");
    s.check(
        o.status == Status::Signaled(libc::SIGXFSZ) || efbig,
        format!(
            "a: status {:?}, EFBIG seen {efbig} (SIGXFSZ kill or EFBIG both stop it)",
            o.status
        ),
    );
    s.check(sz == 16 * MIB, format!("a: file size {sz} bytes"));
    let _ = std::fs::remove_file(wa.join("fill-000.bin"));
    // b
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    let cb = std::ffi::CString::new(wb.to_string_lossy().as_bytes()).unwrap();
    // SAFETY: valid C string and out pointer.
    unsafe { libc::statvfs(cb.as_ptr(), &mut st) };
    let free = st.f_bavail as u64 * st.f_frsize as u64;
    s.note(format!(
        "b: free on the workdir fs before {} MiB",
        free / MIB
    ));
    let mut gap = false;
    if free > 4096 * MIB {
        let mut p = pol(&wb);
        p.fsize_bytes = 16 * MIB;
        p.wall_timeout = Duration::from_secs(90);
        p.cpu_secs = 60;
        p.cgroup_parent = cg.clone();
        p.cgroup_mem_bytes = 128 * MIB;
        let t = Instant::now();
        let o = sb(&wb, &p, &["fill", "8", "32"], false);
        let tot: u64 = std::fs::read_dir(&wb)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("fill-"))
            .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
            .sum();
        s.check(
            o.status == Status::Exited(0),
            format!("b: status {:?}", o.status),
        );
        s.note(format!(
            "b: {} MiB written in {} ms, no quota stopped it",
            tot / MIB,
            t.elapsed().as_millis()
        ));
        gap = tot == 256 * MIB;
        for e in std::fs::read_dir(&wb).into_iter().flatten().flatten() {
            if e.file_name().to_string_lossy().starts_with("fill-") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    } else {
        s.check(false, "b: skipped, less than 4 GiB free");
    }
    // c
    let mi = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    let tms = tm.to_string_lossy().to_string();
    if mi
        .lines()
        .any(|l| l.split_whitespace().nth(4) == Some(tms.as_str()) && l.contains(" - tmpfs "))
    {
        let wc = c_wd_in(&tm, &s.c.exe);
        let mut p = pol(&wc);
        p.fsize_bytes = 16 * MIB;
        p.wall_timeout = Duration::from_secs(30);
        let o = sb(&wc, &p, &["fill", "8", "10"], false);
        let out = o.stdout_str();
        s.check(
            out.contains("RESULT fill stop") && out.contains("No space left"),
            "c: ENOSPC on the 32 MiB tmpfs",
        );
        let tot = field(&out, "RESULT fill stop", "total_mib").unwrap_or_default();
        s.note(format!("c: stopped at {tot} MiB"));
    } else {
        s.check(false, "c: no tmpfs mounted at the s9c dir");
    }
    if gap && s.bad.is_empty() {
        s.note("GAP: openmind-sandbox has only RLIMIT_FSIZE (per file); total workdir size is unbounded unless the caller gives it a size-limited fs");
        return s.end("PASS+GAP");
    }
    s.end("PASS");
}

fn c_wd_in(dir: &Path, exe: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dst = dir.join("rp");
    std::fs::copy(exe, &dst).unwrap();
    std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755)).unwrap();
    dir.to_path_buf()
}

fn s10_proc(c: &mut Ctx) {
    let wd = c.wd("s10-proc");
    let host_pids = c.r("proc.host_pids").cloned().unwrap_or_default();
    let mut s = c.scen("S10", "the known /proc gap: what a sandboxed tool can see of the host", "(known gap, measured) host process list and cmdlines visible, same as the bare temp user sees; other users' environ and exe links NOT readable");
    let o = sb(&wd, &pol(&wd), &["procview"], true);
    let out = o.stdout_str();
    let bare = std::process::Command::new(&s.c.exe)
        .arg("procview")
        .current_dir(&wd)
        .output()
        .map(|x| String::from_utf8_lossy(&x.stdout).into_owned())
        .unwrap_or_default();
    for l in bare
        .lines()
        .filter(|l| l.starts_with("RESULT procview pids="))
    {
        println!("BARE {l}");
    }
    let g = |o: &str, k: &str| {
        field(o, "RESULT procview pids=", k)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(-1)
    };
    let (sp, bp) = (g(&out, "pids"), g(&bare, "pids"));
    s.check(o.status == Status::Exited(0), "tool exited 0");
    s.note(format!(
        "sandbox sees {sp} pids, bare user {bp}, root counted {host_pids}"
    ));
    s.check(
        g(&out, "other_uid_environ_readable") == 0,
        format!(
            "other-uid environ readable: {}",
            g(&out, "other_uid_environ_readable")
        ),
    );
    let gap = g(&out, "other_uid_cmdline_readable") > 0;
    s.note(format!(
        "other-uid cmdlines readable in sandbox: {} (GAP {})",
        g(&out, "other_uid_cmdline_readable"),
        if gap { "confirmed" } else { "not seen" }
    ));
    s.check(
        (sp - bp).abs() <= 10,
        "sandbox view == bare-user view (+-10 churn)",
    );
    s.end(if gap { "MEASURED (known gap)" } else { "PASS" });
}

pub fn run_all(base: &str) -> i32 {
    // SAFETY: plain id getters.
    let (uid, euid) = unsafe { (libc::getuid(), libc::geteuid()) };
    println!("uid={uid} euid={euid}");
    if uid == 0 || euid == 0 {
        println!("REFUSE: never run the sandbox scenarios as root");
        return 3;
    }
    let base = PathBuf::from(base).canonicalize().expect("base");
    if !base.to_string_lossy().starts_with("/var/tmp/sandbox-test-") {
        println!("REFUSE: base must be /var/tmp/sandbox-test-<ts>");
        return 3;
    }
    let cg = cgsetup();
    match cgroup_detect::detect() {
        Ok(r) => println!(
            "DETECT per_run_possible={} reason={:?}",
            r.per_run_possible, r.reason
        ),
        Err(e) => println!("DETECT error {e}"),
    }
    println!(
        "FEATURES {:?} landlock_abi={:?}",
        openmind_sandbox::features(),
        openmind_sandbox::landlock_abi()
    );
    let mut refs = BTreeMap::new();
    for l in std::fs::read_to_string(base.join("data/ref.txt"))
        .unwrap_or_default()
        .lines()
    {
        if let Some((k, v)) = l.split_once('\t') {
            refs.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    println!("REFS {} keys", refs.len());
    let exe = std::env::current_exe().expect("exe");
    let mut c = Ctx {
        base,
        exe,
        cg,
        refs,
        summary: vec![],
        fails: 0,
    };
    let t0 = Instant::now();
    let only = std::env::var("SRP_ONLY").unwrap_or_default();
    type F = fn(&mut Ctx);
    let all: [(&str, F); 10] = [
        ("S1", s1_diag),
        ("S2", s2_db),
        ("S3", s3_crash),
        ("S4", s4_mem),
        ("S5", s5_fork),
        ("S6", s6_hang),
        ("S7", s7_net),
        ("S8", s8_secrets),
        ("S9", s9_disk),
        ("S10", s10_proc),
    ];
    for (id, f) in all {
        if only.is_empty() || only.split(',').any(|x| x == id) {
            f(&mut c);
        }
    }
    println!("\n=== SUMMARY total_ms={}", t0.elapsed().as_millis());
    for l in &c.summary {
        println!("{l}");
    }
    println!("SUMMARY {} scenarios, {} FAIL", c.summary.len(), c.fails);
    i32::from(c.fails > 0)
}
