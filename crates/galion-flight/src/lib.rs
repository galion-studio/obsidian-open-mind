// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! galion-flight: read-only go/no-go checks before (preflight) and after
//! (postflight) a change to one service.
//!
//! Rules this crate enforces by construction:
//! - every check is exactly one read: one `systemctl show`, `readlink`,
//!   `sha256sum`, `stat`, `ss` or HTTP GET (see [`target::Remote`]);
//! - no loops: no retries, no polling, no waiting for a state;
//! - no POST, no write, no restart, nothing that changes the target;
//! - bodies are judged, never printed (they may carry ids), and unit
//!   Environment/ExecStart are never read (they may carry tokens).

pub mod manifest;
pub mod target;

use manifest::{Get, Manifest, Match, Public};
use target::{ReadErr, Target, UNIT_PROPS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Pre,
    Post,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Phase::Pre => "preflight",
            Phase::Post => "postflight",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail,
    Skip,
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub id: String,
    pub verdict: Verdict,
    pub detail: String,
}

#[derive(Debug, Default)]
pub struct Report {
    pub outcomes: Vec<Outcome>,
}

impl Report {
    fn push(&mut self, id: impl Into<String>, verdict: Verdict, detail: impl Into<String>) {
        self.outcomes.push(Outcome {
            id: id.into(),
            verdict,
            detail: detail.into(),
        });
    }
    fn pass(&mut self, id: impl Into<String>, d: impl Into<String>) {
        self.push(id, Verdict::Pass, d)
    }
    fn fail(&mut self, id: impl Into<String>, d: impl Into<String>) {
        self.push(id, Verdict::Fail, d)
    }
    fn check(&mut self, id: impl Into<String>, ok: bool, d: impl Into<String>) {
        self.push(id, if ok { Verdict::Pass } else { Verdict::Fail }, d)
    }
    fn read_err(&mut self, id: impl Into<String>, e: ReadErr) {
        match e {
            ReadErr::NotApplicable(s) => self.push(id, Verdict::Skip, s),
            ReadErr::Failed(s) => self.push(id, Verdict::Fail, s),
        }
    }
    pub fn count(&self, v: Verdict) -> usize {
        self.outcomes.iter().filter(|o| o.verdict == v).count()
    }
    pub fn go(&self) -> bool {
        self.count(Verdict::Fail) == 0 && self.count(Verdict::Pass) > 0
    }
    pub fn failed_ids(&self) -> Vec<&str> {
        self.outcomes
            .iter()
            .filter(|o| o.verdict == Verdict::Fail)
            .map(|o| o.id.as_str())
            .collect()
    }
}

/// Judge a body without ever returning its content.
pub fn judge_body(m: &Match, body: &[u8]) -> Result<String, String> {
    let text = String::from_utf8_lossy(body);
    match m {
        Match::Any => Ok(format!("{} B", body.len())),
        Match::Exact(s) => {
            if text.trim() == s {
                Ok(format!("body = {s:?}"))
            } else {
                Err(format!("body ({} B) != {s:?}", body.len()))
            }
        }
        Match::Contains(s) => {
            if text.contains(s.as_str()) {
                Ok(format!("body ({} B) contains {s:?}", body.len()))
            } else {
                Err(format!("body ({} B) lacks {s:?}", body.len()))
            }
        }
        Match::Json(k, want) => {
            let v: serde_json::Value = serde_json::from_slice(body)
                .map_err(|_| format!("body ({} B) is not JSON", body.len()))?;
            let mut cur = &v;
            for part in k.split('.') {
                cur = cur
                    .get(part)
                    .ok_or_else(|| format!("JSON has no key {k:?}"))?;
            }
            let got = match cur {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Null => "null".into(),
                _ => return Err(format!("JSON {k:?} is not a scalar")),
            };
            if &got == want {
                Ok(format!("json {k} = {want:?}"))
            } else {
                Err(format!("json {k} = {got:?}, want {want:?}"))
            }
        }
    }
}

fn judge_get(rep: &mut Report, id: String, g: &Get, res: Result<(u16, Vec<u8>), ReadErr>) {
    match res {
        Err(e) => rep.read_err(id, e),
        Ok((code, body)) => {
            if !g.status.contains(&code) {
                rep.fail(
                    id,
                    format!("HTTP {code} ({} B), want {:?}", body.len(), g.status),
                );
                return;
            }
            match judge_body(&g.body, &body) {
                Ok(d) => rep.pass(id, format!("HTTP {code}, {d}")),
                Err(d) => rep.fail(id, format!("HTTP {code}, {d}")),
            }
        }
    }
}

/// Nerve view of one component in the nerve's /state.
pub fn nerve_status(body: &[u8], name: &str) -> Result<(String, String), String> {
    let v: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| "nerve /state is not JSON".to_string())?;
    let comps = v
        .get("components")
        .and_then(|c| c.as_array())
        .ok_or("nerve /state has no components")?;
    let c = comps
        .iter()
        .find(|c| c.get("name").and_then(|n| n.as_str()) == Some(name))
        .ok_or_else(|| format!("component {name:?} absent from nerve /state"))?;
    let st = c
        .get("status")
        .and_then(|s| s.as_str())
        .unwrap_or("?")
        .to_string();
    let cycle = v.get("cycle").map(|c| c.to_string()).unwrap_or_default();
    let since = c
        .get("status_since")
        .and_then(|s| s.as_str())
        .unwrap_or("?");
    Ok((st, format!("cycle {cycle}, since {since}")))
}

/// The public check runs from the box with curl (TLS), one GET.
pub fn public_get(p: &Public) -> Result<(u16, Vec<u8>), ReadErr> {
    let out = std::process::Command::new("curl")
        .args([
            "-sS",
            "-m",
            "10",
            "--max-filesize",
            "4194304",
            "-w",
            "\n%{http_code}",
            &p.url,
        ])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| ReadErr::Failed(format!("curl: {e}")))?;
    target::parse_curl(&String::from_utf8_lossy(&out.stdout))
        .ok_or(ReadErr::Failed("unparsable curl output".into()))
}

pub struct Options {
    /// Postflight: the unit must have (re)started at or after this time.
    pub since: Option<i64>,
    /// Run the public (box -> Cloudflare) checks.
    pub public: bool,
    pub now: i64,
}

fn unit_checks(
    rep: &mut Report,
    t: &mut dyn Target,
    m: &Manifest,
    unit: &str,
    main: bool,
    opt: &Options,
) -> Option<u32> {
    let id = |s: &str| format!("unit.{s} {unit}");
    let props = match t.unit_show(unit, UNIT_PROPS) {
        Ok(p) => p,
        Err(e) => {
            rep.read_err(id("state"), e);
            return None;
        }
    };
    let g = |k: &str| props.get(k).cloned().unwrap_or_default();
    let (a, s, r) = (g("ActiveState"), g("SubState"), g("Result"));
    rep.check(
        id("state"),
        a == "active" && s == "running" && r == "success",
        format!("{a}/{s} result={r}"),
    );
    let nr: u32 = g("NRestarts").parse().unwrap_or(u32::MAX);
    if main {
        rep.check(
            id("restarts"),
            nr <= m.restarts_max,
            format!("NRestarts {nr} (max {})", m.restarts_max),
        );
        let ufs = g("UnitFileState");
        rep.check(
            id("enabled"),
            ufs == m.unit_file_state,
            format!("UnitFileState {ufs} (want {})", m.unit_file_state),
        );
        if let Some(want) = &m.dropins {
            let mut got: Vec<String> = g("DropInPaths")
                .split_whitespace()
                .filter_map(|p| p.rsplit('/').next())
                .map(str::to_string)
                .collect();
            got.sort();
            let mut w = want.clone();
            w.sort();
            rep.check(
                id("dropins"),
                got == w,
                format!("drop-ins [{}] (want [{}])", got.join(" "), w.join(" ")),
            );
        }
        let started = target::parse_unix_ts(&g("ExecMainStartTimestamp"));
        if let Some(since) = opt.since {
            match started {
                Some(st) => rep.check(
                    id("restarted"),
                    st >= since,
                    format!(
                        "started {} (since {})",
                        galion_common::time::from_unix(st),
                        galion_common::time::from_unix(since)
                    ),
                ),
                None => rep.fail(id("restarted"), "no ExecMainStartTimestamp"),
            }
        }
    } else {
        rep.check(id("restarts"), nr == 0, format!("NRestarts {nr}"));
    }
    g("MainPID").parse::<u32>().ok().filter(|p| *p > 0)
}

/// Run every check of `phase` once, in order. Never loops, never retries.
pub fn run(phase: Phase, m: &Manifest, t: &mut dyn Target, opt: &Options) -> Report {
    let mut rep = Report::default();
    let mut pid = t.given_pid();
    if !m.unit.is_empty() {
        let p = unit_checks(&mut rep, t, m, &m.unit, true, opt);
        if p.is_some() {
            pid = p;
        }
    }
    // The running image.
    if !m.exe.is_empty() || !m.sha256.is_empty() {
        match pid {
            None => rep.fail("exe.running", "no MainPID / --pid"),
            Some(pid) => {
                if !m.exe.is_empty() {
                    match t.readlink_exe(pid) {
                        Ok(p) => rep.check(
                            "exe.running",
                            p == m.exe,
                            format!("pid {pid} runs {p} (want {})", m.exe),
                        ),
                        Err(e) => rep.read_err("exe.running", e),
                    }
                }
                if !m.sha256.is_empty() {
                    match t.sha256(&format!("/proc/{pid}/exe")) {
                        Ok(s) => rep.check(
                            "exe.running-sha256",
                            s == m.sha256,
                            format!("{}… (want {}…)", short(&s), short(&m.sha256)),
                        ),
                        Err(e) => rep.read_err("exe.running-sha256", e),
                    }
                }
            }
        }
    }
    // The file on disk (what a restart would run).
    if !m.exe.is_empty() {
        if !m.sha256.is_empty() {
            match t.sha256(&m.exe) {
                Ok(s) => rep.check(
                    "exe.file-sha256",
                    s == m.sha256,
                    format!("{} {}… (want {}…)", m.exe, short(&s), short(&m.sha256)),
                ),
                Err(e) => rep.read_err("exe.file-sha256", e),
            }
        }
        if !m.mode.is_empty() || !m.owner.is_empty() {
            match t.stat(&m.exe) {
                Ok(s) => {
                    let ok = (m.mode.is_empty() || s.mode == m.mode)
                        && (m.owner.is_empty() || s.owner == m.owner);
                    rep.check(
                        "exe.stat",
                        ok,
                        format!(
                            "{} B mode {} {} mtime {} (want mode {} {})",
                            s.size,
                            s.mode,
                            s.owner,
                            galion_common::time::from_unix(s.mtime),
                            or_any(&m.mode),
                            or_any(&m.owner)
                        ),
                    );
                }
                Err(e) => rep.read_err("exe.stat", e),
            }
        }
    }
    match t.listening(m.port) {
        Ok(l) => rep.check(
            format!("port.listen {}", m.port),
            l,
            if l { "LISTEN" } else { "nothing listening" },
        ),
        Err(e) => rep.read_err(format!("port.listen {}", m.port), e),
    }
    for g in &m.gets {
        let port = g.port.unwrap_or(m.port);
        let id = format!("http.get :{port}{}", g.path);
        let res = t.get(port, &g.path);
        judge_get(&mut rep, id, g, res);
    }
    if !m.nerve.is_empty() {
        let id = format!("nerve.view {}", m.nerve);
        match t.get(m.nerve_port, "/state") {
            Err(e) => rep.read_err(id, e),
            Ok((200, body)) => match nerve_status(&body, &m.nerve) {
                Ok((st, d)) => rep.check(
                    id,
                    m.nerve_ok.contains(&st),
                    format!("{st} ({d}; accept {})", m.nerve_ok.join(",")),
                ),
                Err(d) => rep.fail(id, d),
            },
            Ok((code, _)) => rep.fail(id, format!("nerve /state HTTP {code}")),
        }
    }
    for u in &m.also_units {
        let _ = unit_checks(&mut rep, t, m, u, false, opt);
    }
    if phase == Phase::Pre {
        for (p, sha) in &m.files {
            let id = format!("file.sha256 {p}");
            match t.sha256(p) {
                Ok(s) => rep.check(
                    id,
                    &s == sha,
                    format!("{}… (want {}…)", short(&s), short(sha)),
                ),
                Err(e) => rep.read_err(id, e),
            }
        }
        for p in &m.exists {
            let id = format!("file.exists {p}");
            match t.stat(p) {
                Ok(s) => rep.pass(id, format!("mode {} {}", s.mode, s.owner)),
                Err(e) => rep.read_err(id, e),
            }
        }
        if let Some((timer, quiet)) = &m.quiet_timer {
            let id = format!("timer.quiet {timer}");
            match t.timer_next(timer) {
                Ok(Some(next)) => {
                    let left = next - opt.now;
                    rep.check(
                        id,
                        left > *quiet as i64,
                        format!(
                            "next {} in {left} s (need > {quiet} s)",
                            galion_common::time::from_unix(next)
                        ),
                    );
                }
                Ok(None) => rep.pass(id, "not scheduled"),
                Err(e) => rep.read_err(id, e),
            }
        }
        if let Some((p, mib)) = &m.free {
            let id = format!("disk.free {p}");
            match t.free_mib(p) {
                Ok(f) => rep.check(id, f >= *mib, format!("{f} MiB free (need {mib})")),
                Err(e) => rep.read_err(id, e),
            }
        }
    }
    for p in &m.public {
        let id = format!("public.get {}", p.url);
        if !opt.public {
            rep.push(id, Verdict::Skip, "public checks off (--no-public)");
            continue;
        }
        let g = Get {
            port: None,
            path: String::new(),
            status: p.status.clone(),
            body: p.body.clone(),
        };
        judge_get(&mut rep, id, &g, public_get(p));
    }
    rep
}

fn short(s: &str) -> &str {
    &s[..s.len().min(8)]
}

fn or_any(s: &str) -> &str {
    if s.is_empty() {
        "any"
    } else {
        s
    }
}

/// Render the report: one line per check, then the verdict.
pub fn render(phase: Phase, m: &Manifest, target: &str, at: &str, rep: &Report) -> String {
    let mut s = format!(
        "galion-flight {} {} target={target} at {at}\n",
        phase.name(),
        m.service
    );
    for o in &rep.outcomes {
        let v = match o.verdict {
            Verdict::Pass => "PASS",
            Verdict::Fail => "FAIL",
            Verdict::Skip => "SKIP",
        };
        s.push_str(&format!("{v} {} — {}\n", o.id, o.detail));
    }
    let fails = rep.failed_ids();
    s.push_str(&format!(
        "RESULT {} {} {}: {} pass, {} fail, {} skip{}\n",
        if rep.go() { "GO" } else { "NO-GO" },
        m.service,
        phase.name(),
        rep.count(Verdict::Pass),
        rep.count(Verdict::Fail),
        rep.count(Verdict::Skip),
        if fails.is_empty() {
            String::new()
        } else {
            format!(" (fail: {})", fails.join("; "))
        }
    ));
    s
}

/// The six manifests that ship with the binary (live VPS state as recorded).
pub const BUILTIN: &[(&str, &str)] = &[
    (
        "nerve",
        include_str!("../../../config/ops/flight/nerve.conf"),
    ),
    (
        "memory",
        include_str!("../../../config/ops/flight/memory.conf"),
    ),
    (
        "hermes-rs",
        include_str!("../../../config/ops/flight/hermes-rs.conf"),
    ),
    (
        "galion-one",
        include_str!("../../../config/ops/flight/galion-one.conf"),
    ),
    (
        "tuner",
        include_str!("../../../config/ops/flight/tuner.conf"),
    ),
    (
        "cloudflared",
        include_str!("../../../config/ops/flight/cloudflared.conf"),
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use target::{Stat, R};

    #[test]
    fn builtin_manifests_parse() {
        for (name, text) in BUILTIN {
            let m = Manifest::parse(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(&m.service, name);
            assert!(!m.unit.is_empty() && m.sha256.len() == 64, "{name}");
            assert!(!m.gets.is_empty(), "{name}");
        }
    }

    #[test]
    fn body_judging_never_echoes_bodies() {
        let secret = br#"{"status":"ok","token":"s3cr3t-value"}"#;
        for m in [
            Match::Any,
            Match::Exact("x".into()),
            Match::Contains("nope".into()),
            Match::Json("status".into(), "bad".into()),
            Match::Json("status".into(), "ok".into()),
        ] {
            let d = match judge_body(&m, secret) {
                Ok(d) | Err(d) => d,
            };
            assert!(!d.contains("s3cr3t"), "{d}");
        }
        assert!(judge_body(&Match::Json("a.b".into(), "1".into()), br#"{"a":{"b":1}}"#).is_ok());
        assert!(judge_body(&Match::Exact("ok".into()), b"ok\n").is_ok());
    }

    /// Scripted target: counts reads so the one-read-per-check rule is pinned.
    struct Fake {
        reads: usize,
        sha: String,
        health: (u16, Vec<u8>),
        active: &'static str,
        next_timer: i64,
    }

    impl Target for Fake {
        fn label(&self) -> String {
            "fake".into()
        }
        fn unit_show(&mut self, _u: &str, _p: &[&str]) -> R<BTreeMap<String, String>> {
            self.reads += 1;
            Ok([
                ("ActiveState", self.active),
                ("SubState", "running"),
                ("Result", "success"),
                ("NRestarts", "0"),
                ("MainPID", "7"),
                ("ExecMainStartTimestamp", "@1000"),
                (
                    "DropInPaths",
                    "/etc/systemd/system/x.service.d/50-rust-bin.conf",
                ),
                ("UnitFileState", "enabled"),
            ]
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect())
        }
        fn timer_next(&mut self, _t: &str) -> R<Option<i64>> {
            self.reads += 1;
            Ok(Some(self.next_timer))
        }
        fn readlink_exe(&mut self, _pid: u32) -> R<String> {
            self.reads += 1;
            Ok("/opt/x".into())
        }
        fn sha256(&mut self, _p: &str) -> R<String> {
            self.reads += 1;
            Ok(self.sha.clone())
        }
        fn stat(&mut self, _p: &str) -> R<Stat> {
            self.reads += 1;
            Ok(Stat {
                size: 1,
                mode: "755".into(),
                owner: "root:root".into(),
                mtime: 0,
            })
        }
        fn free_mib(&mut self, _p: &str) -> R<u64> {
            self.reads += 1;
            Ok(10_000)
        }
        fn listening(&mut self, _p: u16) -> R<bool> {
            self.reads += 1;
            Ok(true)
        }
        fn get(&mut self, _port: u16, path: &str) -> R<(u16, Vec<u8>)> {
            self.reads += 1;
            if path == "/state" {
                return Ok((
                    200,
                    br#"{"cycle":3,"components":[{"name":"x","status":"healthy","status_since":"t"}]}"#
                        .to_vec(),
                ));
            }
            Ok(self.health.clone())
        }
    }

    const SHA: &str = "f0994b6db1fd8ed87c16bd72c9583c2894ec4dd5ba89d407016877322aed6a68";

    fn manifest() -> Manifest {
        Manifest::parse(&format!(
            "service = x\nunit = x.service\nport = 1\nexe = /opt/x\nsha256 = {SHA}\nmode = 755\nowner = root:root\n\
             dropins = 50-rust-bin.conf\nget = /health 200 exact:ok\nnerve = x\nfile = /opt/old {SHA}\n\
             exists = /var/backups\nquiet_timer = b.timer 300\nfree = /opt 200\n"
        ))
        .unwrap()
    }

    fn fake() -> Fake {
        Fake {
            reads: 0,
            sha: SHA.into(),
            health: (200, b"ok".to_vec()),
            active: "active",
            next_timer: 100_000,
        }
    }

    fn opt() -> Options {
        Options {
            since: None,
            public: false,
            now: 1_000,
        }
    }

    #[test]
    fn go_when_all_match_and_one_read_per_check() {
        let m = manifest();
        let mut f = fake();
        let rep = run(Phase::Pre, &m, &mut f, &opt());
        assert!(rep.go(), "{:?}", rep.outcomes);
        // unit.show feeds state/restarts/enabled/dropins (one read, 4 lines).
        let lines = rep.outcomes.len();
        assert_eq!(f.reads, lines - 3, "{:?}", rep.outcomes);
        let post = run(Phase::Post, &m, &mut fake(), &opt());
        assert!(post.go());
        assert!(post.outcomes.len() < lines);
    }

    #[test]
    fn no_go_cases() {
        let m = manifest();
        let mut f = fake();
        f.sha = "0".repeat(64);
        let r = run(Phase::Pre, &m, &mut f, &opt());
        assert!(!r.go());
        assert!(r.failed_ids().contains(&"exe.running-sha256"));

        let mut f = fake();
        f.health = (503, b"loading".to_vec());
        assert!(!run(Phase::Pre, &m, &mut f, &opt()).go());

        let mut f = fake();
        f.active = "failed";
        assert!(!run(Phase::Post, &m, &mut f, &opt()).go());

        let mut f = fake();
        f.next_timer = 1_200; // 200 s away, need > 300
        let r = run(Phase::Pre, &m, &mut f, &opt());
        assert_eq!(r.failed_ids(), vec!["timer.quiet b.timer"]);
        // Postflight does not care about the backup timer.
        assert!(run(Phase::Post, &m, &mut fake(), &opt()).go());

        let mut o = opt();
        o.since = Some(2_000); // unit started at 1000: no restart happened
        let r = run(Phase::Post, &m, &mut fake(), &o);
        assert_eq!(r.failed_ids(), vec!["unit.restarted x.service"]);
    }

    #[test]
    fn nerve_view_parsing() {
        let b = br#"{"cycle":9,"components":[{"name":"tuner","status":"failing-soon","status_since":"s"}]}"#;
        assert_eq!(nerve_status(b, "tuner").unwrap().0, "failing-soon");
        assert!(nerve_status(b, "nope").is_err());
        assert!(nerve_status(b"x", "tuner").is_err());
    }
}
