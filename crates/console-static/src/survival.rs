// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Timer-free `survival.json` refresh (slot 389, F-371-1).
//!
//! `survival.json` was rewritten every 15 min by the Python Hermes
//! `survival-cash` cron (last run 2026-10-18 21:35 CEST, `hermes.service`
//! stopped and masked since). The Rust writer `galion-survival` replaced
//! `survival.py` but was only run once by hand (2026-10-27 18:19 CET, S4.7
//! install); no timer runs it, so the file goes stale.
//!
//! Instead of a new systemd timer, the console keeps it fresh itself:
//!
//! - `on-request`: a GET of `/bank`, `/api/v1/bank` or `/api/v1/survival`
//!   starts the writer when the file is older than `refresh_s`;
//! - `interval`: a background thread checks every `min(refresh_s, 60)` s.
//!
//! The console never reads the OpenRouter key and never parses the file: it
//! spawns the fixed absolute writer path (no shell, no arguments, cleared
//! environment plus an allowlist), discards its stdout (the SCOPE line carries
//! cash figures), and only looks at the file mtime. Single-flight; after a
//! success the next run waits for the file to age again, after a failure it
//! backs off a full `refresh_s`, so a request flood starts at most one run per
//! interval. A run is killed after [`RUN_TIMEOUT`].

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The retired cron interval; default refresh.
pub const REFRESH_DEFAULT_S: u64 = 900;
pub const REFRESH_MIN_S: u64 = 60;
pub const REFRESH_MAX_S: u64 = 3_600;
/// Older than this many intervals = stale (same rule as the `/bank` page).
pub const STALE_INTERVALS: u64 = 3;
/// Minimum gap between two starts, even after a success.
pub const MIN_GAP_S: i64 = 30;
/// A writer run longer than this is killed (curl itself caps at 30 s).
pub const RUN_TIMEOUT: Duration = Duration::from_secs(90);
/// Writer environment variables a unit may set (`--survival-env`).
pub const ENV_ALLOW: &[&str] = &[
    "GALION_LIGHT_ROOT",
    "GALION_HERMES_ROOT",
    "GALION_SURVIVAL_DISK",
    "OPENROUTER_KEY_FILE",
];
/// PATH given to the writer (it runs `curl`, `df`, `nvidia-smi`).
pub const WRITER_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
/// Paths whose GET starts an on-request refresh.
pub const TRIGGER_PATHS: &[&str] = &["/bank", "/api/v1/bank", "/api/v1/survival"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    OnRequest,
    Interval,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "on-request" => Some(Mode::OnRequest),
            "interval" => Some(Mode::Interval),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Mode::OnRequest => "on-request",
            Mode::Interval => "interval",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurvivalConfig {
    /// The file whose age is watched (`--survival-file`).
    pub file: PathBuf,
    /// Absolute writer binary (`--survival-writer`); `None` = status only.
    pub writer: Option<PathBuf>,
    pub refresh_s: u64,
    pub mode: Mode,
    /// `NAME=VALUE` pairs from `--survival-env` (names in [`ENV_ALLOW`]).
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Runs {
    running: bool,
    last_start: Option<i64>,
    last_end: Option<i64>,
    /// `Some(0)` ok; other codes; `-1` spawn error; `-2` killed (timeout);
    /// `-3` signalled.
    last_exit: Option<i32>,
    runs: u64,
    failures: u64,
}

pub struct Refresher {
    pub cfg: SurvivalConfig,
    runs: Mutex<Runs>,
}

pub fn now_s() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// File mtime as Unix seconds.
pub fn mtime_s(path: &Path) -> Option<i64> {
    let m = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(m.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64)
}

/// Europe/Zagreb RFC 3339 with explicit offset (never `Z`).
pub fn zagreb_iso(secs: i64) -> String {
    let (y, m, d, hh, mm, off) = console_ui::pages::p2::zagreb(secs);
    let ss = secs.rem_euclid(60);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}+{off:02}:00")
}

/// Why a refresh may (not) start now. Pure: the decision table under test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Start,
    NoWriter,
    Running,
    Fresh,
    Backoff,
}

fn decide(writer: bool, refresh_s: u64, runs: &Runs, age: Option<i64>, now: i64) -> Decision {
    if !writer {
        return Decision::NoWriter;
    }
    if runs.running {
        return Decision::Running;
    }
    if let Some(start) = runs.last_start {
        let failed = runs.last_exit != Some(0);
        let gap = if failed { refresh_s as i64 } else { MIN_GAP_S };
        if now - start < gap {
            return Decision::Backoff;
        }
    }
    match age {
        Some(a) if a < refresh_s as i64 => Decision::Fresh,
        _ => Decision::Start,
    }
}

impl Refresher {
    pub fn new(cfg: SurvivalConfig) -> Arc<Refresher> {
        Arc::new(Refresher {
            cfg,
            runs: Mutex::new(Runs::default()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Runs> {
        self.runs.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// File age in seconds (`None` = missing).
    pub fn age(&self, now: i64) -> Option<i64> {
        mtime_s(&self.cfg.file).map(|t| now - t)
    }

    /// Start the writer if the file is due; returns the decision. Never
    /// blocks the caller: the run happens on its own thread.
    pub fn kick(self: &Arc<Self>) -> Decision {
        let now = now_s();
        let age = self.age(now);
        let writer = match &self.cfg.writer {
            Some(w) => w.clone(),
            None => return Decision::NoWriter,
        };
        {
            let mut r = self.lock();
            let d = decide(true, self.cfg.refresh_s, &r, age, now);
            if d != Decision::Start {
                return d;
            }
            r.running = true;
            r.last_start = Some(now);
            r.runs += 1;
        }
        let me = Arc::clone(self);
        let spawned = thread::Builder::new()
            .name("survival".into())
            .stack_size(128 * 1024)
            .spawn(move || {
                let code = run_writer(&writer, &me.cfg.env);
                let mut r = me.lock();
                r.running = false;
                r.last_end = Some(now_s());
                r.last_exit = Some(code);
                if code != 0 {
                    r.failures += 1;
                    eprintln!("console-static: survival writer exit {code}");
                }
            });
        if spawned.is_err() {
            let mut r = self.lock();
            r.running = false;
            r.last_exit = Some(-1);
            r.failures += 1;
        }
        Decision::Start
    }

    /// Spawn the interval checker (mode `interval`). No busy loop: it sleeps
    /// `min(refresh_s, 60)` s between checks.
    pub fn spawn_interval(self: &Arc<Self>) {
        let me = Arc::clone(self);
        let every = Duration::from_secs(self.cfg.refresh_s.min(60));
        let _ = thread::Builder::new()
            .name("survival-tick".into())
            .stack_size(64 * 1024)
            .spawn(move || loop {
                me.kick();
                thread::sleep(every);
            });
    }

    /// Wait until no run is in flight (tests / shutdown), up to `max`.
    pub fn wait_idle(&self, max: Duration) -> bool {
        let until = std::time::Instant::now() + max;
        while std::time::Instant::now() < until {
            if !self.lock().running {
                return true;
            }
            thread::sleep(Duration::from_millis(50));
        }
        !self.lock().running
    }

    /// `GET /api/v1/survival`: file age and writer state, never the content.
    pub fn status_json(&self) -> String {
        let now = now_s();
        let mtime = mtime_s(&self.cfg.file);
        let age = mtime.map(|t| now - t);
        let stale_after = (self.cfg.refresh_s * STALE_INTERVALS) as i64;
        let r = self.lock().clone();
        let opt_ts =
            |v: Option<i64>| v.map_or("null".to_string(), |t| format!("\"{}\"", zagreb_iso(t)));
        let opt_i = |v: Option<i64>| v.map_or("null".to_string(), |t| t.to_string());
        let writer = self
            .cfg
            .writer
            .as_ref()
            .map_or("null".to_string(), |w| json_str(&w.display().to_string()));
        format!(
            concat!(
                "{{\"ok\":{ok},\"file\":{file},\"exists\":{exists},\"asOf\":{as_of},",
                "\"ageS\":{age},\"stale\":{stale},\"staleAfterS\":{stale_after},",
                "\"refreshS\":{refresh},\"mode\":\"{mode}\",\"writer\":{writer},",
                "\"running\":{running},\"lastStart\":{ls},\"lastEnd\":{le},",
                "\"lastExit\":{lx},\"runs\":{runs},\"failures\":{fails},\"now\":\"{now}\"}}"
            ),
            ok = age.is_some_and(|a| a <= stale_after),
            file = json_str(&self.cfg.file.display().to_string()),
            exists = mtime.is_some(),
            as_of = opt_ts(mtime),
            age = opt_i(age),
            stale = age.is_none_or(|a| a > stale_after),
            stale_after = stale_after,
            refresh = self.cfg.refresh_s,
            mode = self.cfg.mode.name(),
            writer = writer,
            running = r.running,
            ls = opt_ts(r.last_start),
            le = opt_ts(r.last_end),
            lx = r.last_exit.map_or("null".to_string(), |c| c.to_string()),
            runs = r.runs,
            fails = r.failures,
            now = zagreb_iso(now),
        )
    }
}

fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Run the writer once: no shell, no args, cleared env + allowlist, stdout
/// and stderr discarded (stdout carries cash figures), killed after
/// [`RUN_TIMEOUT`]. Returns the exit code (see [`Runs::last_exit`]).
fn run_writer(writer: &Path, env: &[(String, String)]) -> i32 {
    let mut cmd = Command::new(writer);
    cmd.env_clear()
        .env("PATH", WRITER_PATH)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return -1,
    };
    let until = std::time::Instant::now() + RUN_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return st.code().unwrap_or(-3),
            Ok(None) if std::time::Instant::now() >= until => {
                let _ = child.kill();
                let _ = child.wait();
                return -2;
            }
            Ok(None) => thread::sleep(Duration::from_millis(200)),
            Err(_) => return -1,
        }
    }
}

/// Parse one `--survival-env NAME=VALUE` (name allowlisted, no NUL/newline).
pub fn parse_env(s: &str) -> Result<(String, String), String> {
    let (k, v) = s
        .split_once('=')
        .ok_or_else(|| format!("--survival-env {s:?} must be NAME=VALUE"))?;
    if !ENV_ALLOW.contains(&k) {
        return Err(format!(
            "--survival-env name {k:?} not allowed (allowed: {})",
            ENV_ALLOW.join(", ")
        ));
    }
    if v.is_empty() || v.contains(['\0', '\n', '\r']) {
        return Err(format!("--survival-env {k} needs a plain value"));
    }
    Ok((k.to_string(), v.to_string()))
}

/// Path part of a request target is one of [`TRIGGER_PATHS`].
pub fn is_trigger(path: &str) -> bool {
    let p = path.split('?').next().unwrap_or(path);
    TRIGGER_PATHS.contains(&p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(start: Option<i64>, exit: Option<i32>) -> Runs {
        Runs {
            last_start: start,
            last_exit: exit,
            ..Runs::default()
        }
    }

    #[test]
    fn decision_table() {
        let r0 = Runs::default();
        assert_eq!(decide(false, 900, &r0, None, 0), Decision::NoWriter);
        assert_eq!(decide(true, 900, &r0, None, 0), Decision::Start); // missing file
        assert_eq!(decide(true, 900, &r0, Some(899), 0), Decision::Fresh);
        assert_eq!(decide(true, 900, &r0, Some(900), 0), Decision::Start);
        let busy = Runs {
            running: true,
            ..Runs::default()
        };
        assert_eq!(decide(true, 900, &busy, Some(10_000), 0), Decision::Running);
        // Success: short gap only.
        let ok = runs(Some(1_000), Some(0));
        assert_eq!(
            decide(true, 900, &ok, Some(5_000), 1_010),
            Decision::Backoff
        );
        assert_eq!(decide(true, 900, &ok, Some(5_000), 1_030), Decision::Start);
        // Failure: full interval back-off, then retry.
        let bad = runs(Some(1_000), Some(1));
        assert_eq!(
            decide(true, 900, &bad, Some(5_000), 1_899),
            Decision::Backoff
        );
        assert_eq!(decide(true, 900, &bad, Some(5_000), 1_900), Decision::Start);
        let killed = runs(Some(1_000), Some(-2));
        assert_eq!(decide(true, 60, &killed, None, 1_059), Decision::Backoff);
    }

    #[test]
    fn zagreb_iso_both_seasons_never_z() {
        // 2026-10-27 17:19:50 UTC = 18:19:50 CET (the S4.7 one-shot).
        assert_eq!(zagreb_iso(1_793_121_590), "2026-10-27T18:19:50+01:00");
        // 2026-10-18 19:35:50 UTC = 21:35:50 CEST (last Python cron run).
        assert_eq!(zagreb_iso(1_792_352_150), "2026-10-18T21:35:50+02:00");
    }

    #[test]
    fn env_allowlist() {
        assert_eq!(
            parse_env("GALION_LIGHT_ROOT=/var/tmp/x").unwrap(),
            ("GALION_LIGHT_ROOT".into(), "/var/tmp/x".into())
        );
        assert!(parse_env("OPENROUTER_API_KEY=sk-x").is_err());
        assert!(parse_env("LD_PRELOAD=/x.so").is_err());
        assert!(parse_env("PATH=/tmp").is_err());
        assert!(parse_env("GALION_LIGHT_ROOT=").is_err());
        assert!(parse_env("GALION_LIGHT_ROOT=/a\nb").is_err());
        assert!(parse_env("noequals").is_err());
    }

    #[test]
    fn triggers() {
        assert!(is_trigger("/bank"));
        assert!(is_trigger("/api/v1/bank?x=1"));
        assert!(is_trigger("/api/v1/survival"));
        assert!(!is_trigger("/bank/x"));
        assert!(!is_trigger("/console"));
    }

    #[test]
    fn mode_names() {
        assert_eq!(Mode::parse("on-request"), Some(Mode::OnRequest));
        assert_eq!(Mode::parse("interval"), Some(Mode::Interval));
        assert_eq!(Mode::parse("timer"), None);
        assert_eq!(Mode::Interval.name(), "interval");
    }

    #[test]
    fn status_missing_file_and_no_writer() {
        let r = Refresher::new(SurvivalConfig {
            file: PathBuf::from("/nonexistent/survival-389.json"),
            writer: None,
            refresh_s: 900,
            mode: Mode::OnRequest,
            env: vec![],
        });
        assert_eq!(r.kick(), Decision::NoWriter);
        let s = r.status_json();
        let v = console_ui::json::parse(s.as_bytes()).expect("valid json");
        assert_eq!(v.get("exists").as_bool(), Some(false));
        assert_eq!(v.get("stale").as_bool(), Some(true));
        assert_eq!(v.get("ok").as_bool(), Some(false));
        assert!(!s.contains('Z'), "no Zulu stamps: {s}");
    }

    #[test]
    fn spawn_error_is_failure_with_backoff() {
        let dir = std::env::temp_dir().join(format!("cs-surv-unit-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let r = Refresher::new(SurvivalConfig {
            file: dir.join("survival.json"),
            writer: Some(dir.join("no-such-writer")),
            refresh_s: 900,
            mode: Mode::OnRequest,
            env: vec![],
        });
        assert_eq!(r.kick(), Decision::Start);
        assert!(r.wait_idle(Duration::from_secs(5)));
        assert_eq!(r.kick(), Decision::Backoff);
        let v = console_ui::json::parse(r.status_json().as_bytes()).unwrap();
        assert_eq!(v.get("lastExit").as_f64(), Some(-1.0));
        assert_eq!(v.get("failures").as_f64(), Some(1.0));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
