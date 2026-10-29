// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `memory-freshness` (wave-4 slot 405, D-388-3 prep): alert when the
//! OpenMind memory store has not been written for too long.
//!
//! Slot 388 found the live memory DB silently stale since 2026-10-18
//! 23:57 +02:00 (its only writer, the Python Hermes plugin, stopped with
//! hermes.service). Nothing noticed for nine days. This sensor runs the
//! real `openmind-memory freshness <data_dir> <instance> <max_age_h>` CLI
//! (slot 388, read-only SQLite, counts and Zagreb times only, exit 0 fresh /
//! 3 stale / 1 error) and turns exit 3 into a warn finding plus an incident
//! signal, so the engine raises an alert (journald + alerts.json) through
//! the normal incident path.
//!
//! Why a child process and not the code in-process: the freshness logic
//! needs SQLite (`rusqlite`, bundled C), which the nerve deliberately does
//! not link (std + serde only). Running the one shipped binary keeps one
//! implementation and keeps the nerve small.
//!
//! Safety: live cycles only ([`Input::live`]); the child runs on a
//! background thread every `NERVE_MEMFRESH_EVERY_S` so the cycle never
//! waits; argv only (no shell), empty environment except a fixed `PATH`,
//! stdin closed, stderr discarded, stdout capped at 16 KiB, killed after
//! `NERVE_MEMFRESH_TIMEOUT_MS`. Only allow-listed summary keys are parsed
//! (`state=`, `newest_write=`, the instance `l0_rows=`, `db[instance]=..
//! missing`, `stray_root_db=yes`); paths and anything else are dropped, and
//! the CLI itself never prints memory content or session keys. The nerve
//! never writes to the store and never restarts anything.
//!
//! Env (sensor OFF unless `NERVE_MEMFRESH_BIN` is set to an absolute path):
//! - `NERVE_MEMFRESH_BIN` the `openmind-memory` binary with the `freshness`
//!   subcommand (slot 388 / 1d7164b4 or later; the live aa77864 has none);
//! - `NERVE_MEMFRESH_DATA_DIR` (default `/var/lib/memory-tencentdb`);
//! - `NERVE_MEMFRESH_INSTANCE` (default `galion`, the live instance id;
//!   `[A-Za-z0-9_-]{1,64}`);
//! - `NERVE_MEMFRESH_MAX_AGE_H` (24), `NERVE_MEMFRESH_EVERY_S` (900),
//!   `NERVE_MEMFRESH_TIMEOUT_MS` (10000),
//!   `NERVE_MEMFRESH_COMPONENT` (default `openmind-memory`).
//!
//! Findings (bleeding, cause `memory-freshness`): `memory-stale` (warn,
//! plus a warn [`Signal`] -> incident -> alert) and info
//! `memory-freshness-unknown` (check failed: no alert, not scored) /
//! `memory-freshness-note` (bad settings).

use super::{Env, Sensor, Sink};
use crate::cortex::{Finding, FindingKind, Input};
use crate::incident::{Sev, Signal};
use std::ffi::OsString;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Sensor name, B33 source and `NERVE_SENSORS_OFF` key.
pub const NAME: &str = "memory-freshness";
/// Incident cause of every finding.
pub const CAUSE: &str = "memory-freshness";
/// Largest stdout kept from the child.
pub const MAX_OUT: u64 = 16 * 1024;
/// Fixed `PATH` for the child (the environment is otherwise empty).
pub const CHILD_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// What one `freshness` run said.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Exit 0.
    Fresh(Summary),
    /// Exit 3.
    Stale(Summary),
    /// Anything else (spawn failure, timeout, exit 1/64, signal, no verdict).
    Unknown(String),
}

/// The allow-listed part of the CLI report (no paths, no content).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    /// `newest_write=` Zagreb stamp, `None` when the CLI said `none`.
    pub newest: Option<String>,
    pub age_h: Option<f64>,
    /// `l0_rows=` of the instance DB (not the stray root DB).
    pub l0_rows: Option<i64>,
    /// `db[instance]=... missing`.
    pub db_missing: bool,
    /// `stray_root_db=yes` (legacy root DB present, ignored).
    pub stray_root: bool,
    /// `state=` value (`fresh` / `stale`).
    pub state: String,
}

/// Parse the CLI stdout; only known keys, everything else is dropped.
pub fn parse(stdout: &str) -> Summary {
    let mut s = Summary::default();
    let mut in_instance = false;
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("db[") {
            in_instance = rest.starts_with("instance]");
            if in_instance && line.ends_with(" missing") {
                s.db_missing = true;
            }
            continue;
        }
        if in_instance && s.l0_rows.is_none() {
            if let Some(v) = line.trim_start().strip_prefix("l0_rows=") {
                s.l0_rows = v.split_whitespace().next().and_then(|n| n.parse().ok());
            }
        }
        if !line.starts_with(' ') {
            in_instance = false;
        }
        if line == "stray_root_db=yes" || line.starts_with("stray_root_db=yes ") {
            s.stray_root = true;
        } else if let Some(v) = line.strip_prefix("newest_write=") {
            let mut it = v.split_whitespace();
            let stamp = it.next().unwrap_or("none");
            if stamp != "none" && is_stamp(stamp) {
                s.newest = Some(stamp.to_string());
            }
            s.age_h = it
                .find_map(|w| w.strip_prefix("age_h="))
                .and_then(|a| a.parse().ok());
        } else if let Some(v) = line.strip_prefix("state=") {
            let w = v.split_whitespace().next().unwrap_or("");
            if w == "fresh" || w == "stale" {
                s.state = w.to_string();
            }
        }
    }
    s
}

/// `2026-10-19T14:57:55+02:00`: digits and `-:+T` only, bounded length.
fn is_stamp(s: &str) -> bool {
    (19..=32).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'-' | b':' | b'+' | b'T' | b'.' | b'Z'))
}

/// Map an exit code + stdout to an [`Outcome`].
pub fn judge_exit(code: Option<i32>, stdout: &str) -> Outcome {
    let s = parse(stdout);
    match code {
        Some(0) if s.state == "fresh" => Outcome::Fresh(s),
        Some(3) if s.state == "stale" => Outcome::Stale(s),
        Some(0) | Some(3) => Outcome::Unknown(format!(
            "exit {} without a matching state= line",
            code.unwrap_or(-1)
        )),
        Some(64) => Outcome::Unknown("usage error (exit 64): check the settings".into()),
        Some(c) => Outcome::Unknown(format!("freshness exit {c}")),
        None => Outcome::Unknown("freshness killed by a signal".into()),
    }
}

/// Run `argv` with an empty environment (plus [`CHILD_PATH`]), stdin
/// closed, stderr discarded, stdout capped at [`MAX_OUT`]; kill it after
/// `timeout`. Returns `(exit code, stdout)` or a short reason.
pub fn run_capped(argv: &[OsString], timeout: Duration) -> Result<(Option<i32>, String), String> {
    let (bin, args) = argv.split_first().ok_or("empty argv")?;
    let mut child = Command::new(bin)
        .args(args)
        .env_clear()
        .env("PATH", CHILD_PATH)
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn failed ({:?})", e.kind()))?;
    let mut out = child.stdout.take().ok_or("no stdout pipe")?;
    let reader = std::thread::Builder::new()
        .name("nerve-memfresh-out".into())
        .spawn(move || {
            let mut kept = Vec::new();
            let _ = (&mut out).take(MAX_OUT).read_to_end(&mut kept);
            // Drain the rest so the child never blocks on a full pipe.
            let _ = std::io::copy(&mut out, &mut std::io::sink());
            kept
        })
        .map_err(|_| "reader thread failed".to_string())?;
    let t0 = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Ok(st),
            Ok(None) if t0.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("timeout after {} ms (killed)", timeout.as_millis()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("wait failed ({:?})", e.kind()));
            }
        }
    };
    let kept = reader.join().unwrap_or_default();
    let st = status?;
    Ok((st.code(), String::from_utf8_lossy(&kept).into_owned()))
}

/// Settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub bin: PathBuf,
    pub data_dir: PathBuf,
    pub instance: String,
    pub max_age_h: u64,
    pub every_s: u64,
    pub timeout: Duration,
    pub component: String,
}

impl Settings {
    /// The child argv: `<bin> freshness <data_dir> <instance> <max_age_h>`.
    pub fn argv(&self) -> Vec<OsString> {
        vec![
            self.bin.clone().into(),
            "freshness".into(),
            self.data_dir.clone().into(),
            self.instance.clone().into(),
            self.max_age_h.to_string().into(),
        ]
    }
}

fn valid_instance(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Sensor `memory-freshness`.
pub struct MemoryFreshness {
    pub settings: Settings,
    /// Why the settings are unusable (reported as an info note; never runs).
    pub bad: Option<String>,
    next_due: u64,
    worker: Option<JoinHandle<Outcome>>,
    /// Last finished run.
    pub last: Option<Outcome>,
    /// Runs started.
    pub rounds: u64,
}

impl MemoryFreshness {
    pub fn new(settings: Settings) -> Self {
        let bad = if !settings.bin.is_absolute() {
            Some("NERVE_MEMFRESH_BIN must be an absolute path".to_string())
        } else if !settings.data_dir.is_absolute() {
            Some("NERVE_MEMFRESH_DATA_DIR must be an absolute path".to_string())
        } else if !valid_instance(&settings.instance) {
            Some("NERVE_MEMFRESH_INSTANCE must match [A-Za-z0-9_-]{1,64}".to_string())
        } else {
            None
        };
        MemoryFreshness {
            settings,
            bad,
            next_due: 0,
            worker: None,
            last: None,
            rounds: 0,
        }
    }

    /// Off (None) unless `NERVE_MEMFRESH_BIN` is set and non-empty.
    pub fn from_env(env: Env) -> Option<Self> {
        let bin = env("NERVE_MEMFRESH_BIN").filter(|s| !s.trim().is_empty())?;
        let num = |k: &str, d: u64| env(k).and_then(|v| v.trim().parse().ok()).unwrap_or(d);
        let text = |k: &str, d: &str| {
            env(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| d.to_string())
        };
        Some(Self::new(Settings {
            bin: PathBuf::from(bin.trim()),
            data_dir: PathBuf::from(text("NERVE_MEMFRESH_DATA_DIR", "/var/lib/memory-tencentdb")),
            instance: text("NERVE_MEMFRESH_INSTANCE", "galion"),
            max_age_h: num("NERVE_MEMFRESH_MAX_AGE_H", 24),
            every_s: num("NERVE_MEMFRESH_EVERY_S", 900).max(1),
            timeout: Duration::from_millis(num("NERVE_MEMFRESH_TIMEOUT_MS", 10_000).max(100)),
            component: text("NERVE_MEMFRESH_COMPONENT", "openmind-memory"),
        }))
    }

    /// Collect a finished run; start one when due and none is running.
    fn tick(&mut self, now: u64) {
        if self.worker.as_ref().is_some_and(|w| w.is_finished()) {
            if let Some(w) = self.worker.take() {
                self.last = Some(
                    w.join()
                        .unwrap_or_else(|_| Outcome::Unknown("check thread panicked".into())),
                );
            }
        }
        if self.worker.is_none() && now >= self.next_due {
            self.next_due = now + self.settings.every_s;
            self.rounds += 1;
            let (argv, timeout) = (self.settings.argv(), self.settings.timeout);
            self.worker = std::thread::Builder::new()
                .name("nerve-memfresh".into())
                .spawn(move || match run_capped(&argv, timeout) {
                    Ok((code, out)) => judge_exit(code, &out),
                    Err(why) => Outcome::Unknown(why),
                })
                .ok();
        }
    }

    /// Block until the running check (if any) has finished (tests only).
    pub fn wait_idle(&mut self) {
        if let Some(w) = self.worker.take() {
            self.last = Some(
                w.join()
                    .unwrap_or_else(|_| Outcome::Unknown("check thread panicked".into())),
            );
        }
    }
}

/// Findings and signals for one outcome (pure; tests drive it directly).
pub fn report(o: &Outcome, st: &Settings) -> (Vec<Finding>, Vec<Signal>) {
    let comp = st.component.as_str();
    match o {
        Outcome::Fresh(_) => (vec![], vec![]),
        Outcome::Stale(s) => {
            let title = match (s.db_missing, s.age_h) {
                (true, _) => format!(
                    "memory store '{}' has no instance DB: nothing written (limit {} h)",
                    st.instance, st.max_age_h
                ),
                (false, Some(h)) => format!(
                    "memory store '{}' not written for {:.1} h ({:.1} days, limit {} h)",
                    st.instance,
                    h,
                    h / 24.0,
                    st.max_age_h
                ),
                (false, None) => format!(
                    "memory store '{}' shows no write at all (limit {} h)",
                    st.instance, st.max_age_h
                ),
            };
            let mut ev = vec![
                format!("newest_write={}", s.newest.as_deref().unwrap_or("none")),
                format!(
                    "age_h={}",
                    s.age_h
                        .map(|h| format!("{h:.1}"))
                        .unwrap_or_else(|| "none".into())
                ),
                format!("max_age_h={}", st.max_age_h),
                format!(
                    "instance_l0_rows={}",
                    s.l0_rows
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "none".into())
                ),
            ];
            if s.stray_root {
                ev.push("stray legacy root DB present (ignored for freshness)".into());
            }
            let f = Finding::new(FindingKind::Bleeding, "memory-stale", Sev::Warn, comp, title.clone())
                .with_evidence(ev)
                .with_fix("restore a memory writer (hermes-rs capture client, owner decision D-388-1) or retire the store; the nerve only reports, it never writes the store or restarts anything")
                .with_cause(CAUSE);
            (vec![f], vec![Signal::new(CAUSE, comp, Sev::Warn, title)])
        }
        Outcome::Unknown(why) => (
            vec![Finding::new(
                FindingKind::Bleeding,
                "memory-freshness-unknown",
                Sev::Info,
                comp,
                format!("memory freshness check gave no verdict: {why}"),
            )
            .with_fix("check NERVE_MEMFRESH_BIN is an openmind-memory build with the freshness subcommand (1d7164b4 or later)")
            .with_cause(CAUSE)],
            vec![],
        ),
    }
}

impl Sensor for MemoryFreshness {
    fn name(&self) -> &'static str {
        NAME
    }

    fn sense(&mut self, i: &Input<'_>, out: &mut Sink<'_>) {
        if !i.live {
            return;
        }
        if let Some(why) = &self.bad {
            out.finding(
                Finding::new(
                    FindingKind::Bleeding,
                    "memory-freshness-note",
                    Sev::Info,
                    self.settings.component.clone(),
                    format!("memory-freshness sensor not run: {why}"),
                )
                .with_cause(CAUSE),
            );
            return;
        }
        self.tick(i.now);
        if let Some(o) = &self.last {
            let (fs, ss) = report(o, &self.settings);
            for f in fs {
                out.finding(f);
            }
            for s in ss {
                out.signal(s);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logscan::LogInsights;
    use crate::sensors::Sensors;

    const STALE: &str = r#"data_dir=/var/lib/memory-tencentdb
instance_id=galion
db[instance]=/var/lib/memory-tencentdb/instances/galion/vectors.db
  db_mtime=2026-10-19T14:57:55+02:00
  wal_mtime=2026-10-19T14:57:55+02:00
  l0_rows=2610 l0_last=2026-10-19T14:57:55+02:00
  l1_rows=216 l1_last=2026-10-18T21:41:44+02:00
conversations_last_file=2026-10-18.jsonl mtime=2026-10-18T23:57:27+02:00
records_last_file=none
stray_root_db=yes (not the live instance; ignored for freshness)
db[legacy-root]=/var/lib/memory-tencentdb/vectors.db
  l0_rows=4 l0_last=2026-10-13T11:19:53+02:00
newest_write=2026-10-19T14:57:55+02:00 age_h=218.4
state=stale (max_age_h=24)
"#;

    fn settings(bin: &str) -> Settings {
        Settings {
            bin: PathBuf::from(bin),
            data_dir: PathBuf::from("/var/lib/memory-tencentdb"),
            instance: "galion".into(),
            max_age_h: 24,
            every_s: 900,
            timeout: Duration::from_millis(2000),
            component: "openmind-memory".into(),
        }
    }

    #[test]
    fn parse_keeps_allowlisted_keys_and_the_instance_rows_only() {
        let s = parse(STALE);
        assert_eq!(s.state, "stale");
        assert_eq!(s.newest.as_deref(), Some("2026-10-19T14:57:55+02:00"));
        assert_eq!(s.age_h, Some(218.4));
        assert_eq!(s.l0_rows, Some(2610), "instance rows, not the legacy 4");
        assert!(s.stray_root);
        assert!(!s.db_missing);
        let m = parse("db[instance]=/x/instances/galion/vectors.db missing\nnewest_write=none\nstate=stale (max_age_h=24)\n");
        assert!(m.db_missing);
        assert_eq!(m.newest, None);
        assert_eq!(m.l0_rows, None);
    }

    #[test]
    fn exit_codes_map_to_outcomes() {
        assert!(matches!(judge_exit(Some(3), STALE), Outcome::Stale(_)));
        let fresh = STALE.replace("state=stale", "state=fresh");
        assert!(matches!(judge_exit(Some(0), &fresh), Outcome::Fresh(_)));
        // Exit and state line must agree, else no verdict.
        assert!(matches!(judge_exit(Some(0), STALE), Outcome::Unknown(_)));
        assert!(matches!(judge_exit(Some(3), ""), Outcome::Unknown(_)));
        assert!(matches!(judge_exit(Some(1), ""), Outcome::Unknown(_)));
        assert!(matches!(judge_exit(Some(64), ""), Outcome::Unknown(_)));
        assert!(matches!(judge_exit(None, ""), Outcome::Unknown(_)));
    }

    #[test]
    fn stale_is_a_warn_finding_and_signal_without_paths() {
        let st = settings("/opt/x/openmind-memory");
        let (fs, ss) = report(&judge_exit(Some(3), STALE), &st);
        assert_eq!(fs.len(), 1);
        assert_eq!(ss.len(), 1);
        let f = &fs[0];
        assert_eq!((f.id.as_str(), f.sev), ("memory-stale", Sev::Warn));
        assert_eq!(f.cause.as_deref(), Some(CAUSE));
        assert!(f.title.contains("218.4 h"), "{}", f.title);
        assert_eq!(ss[0].sev, Sev::Warn);
        assert_eq!(ss[0].cause, CAUSE);
        let all = format!("{} {:?} {}", f.title, f.evidence, ss[0].msg);
        assert!(!all.contains("/var/lib"), "no paths: {all}");
        assert!(!all.contains(".jsonl"), "no file names: {all}");
        // Fresh: silent. Unknown: info only, no signal (no alert).
        let fresh = STALE.replace("state=stale", "state=fresh");
        assert_eq!(report(&judge_exit(Some(0), &fresh), &st), (vec![], vec![]));
        let (fs, ss) = report(&Outcome::Unknown("x".into()), &st);
        assert_eq!((fs.len(), ss.len(), fs[0].sev), (1, 0, Sev::Info));
    }

    #[test]
    fn off_by_default_and_settings_are_validated() {
        let none = |_: &str| None;
        assert!(MemoryFreshness::from_env(&none).is_none());
        assert!(!Sensors::from_env(&none).names().contains(&NAME));
        let on = |k: &str| match k {
            "NERVE_MEMFRESH_BIN" => Some("/opt/galion/rust/bin/openmind-memory".into()),
            _ => None,
        };
        let m = MemoryFreshness::from_env(&on).unwrap();
        assert_eq!(
            m.settings,
            settings("/opt/galion/rust/bin/openmind-memory").with_timeout(10_000)
        );
        assert!(m.bad.is_none());
        assert!(Sensors::from_env(&on).names().contains(&NAME));
        let off = |k: &str| match k {
            "NERVE_MEMFRESH_BIN" => Some("/b".into()),
            "NERVE_SENSORS_OFF" => Some(NAME.into()),
            _ => None,
        };
        assert!(!Sensors::from_env(&off).names().contains(&NAME));
        for (k, v) in [
            ("NERVE_MEMFRESH_BIN", "relative/bin"),
            ("NERVE_MEMFRESH_INSTANCE", "../etc"),
            ("NERVE_MEMFRESH_DATA_DIR", "rel"),
        ] {
            let e = |q: &str| {
                if q == k {
                    Some(v.to_string())
                } else if q == "NERVE_MEMFRESH_BIN" {
                    Some("/b".into())
                } else {
                    None
                }
            };
            assert!(
                MemoryFreshness::from_env(&e).unwrap().bad.is_some(),
                "{k}={v}"
            );
        }
    }

    impl Settings {
        fn with_timeout(mut self, ms: u64) -> Self {
            self.timeout = Duration::from_millis(ms);
            self
        }
    }

    #[test]
    fn bad_settings_never_spawn_and_not_live_never_runs() {
        let comps = [];
        let logs = LogInsights::default();
        let mut i = Input::new(1000, &comps, &logs);
        let mut m = MemoryFreshness::new(settings("relative"));
        let mut cx = crate::cortex::Cortex::default();
        m.sense(&i, &mut Sink::new(&mut cx, NAME));
        assert_eq!(m.rounds, 0, "not live: nothing runs");
        i.live = true;
        m.sense(&i, &mut Sink::new(&mut cx, NAME));
        assert_eq!(m.rounds, 0, "bad settings: nothing runs");
        assert_eq!(cx.bleeding[0].id, "memory-freshness-note");
        assert!(cx.signals.is_empty());
    }

    #[test]
    fn runner_has_empty_env_caps_output_and_kills_on_timeout() {
        // Empty environment: `env` prints only the fixed PATH.
        let (code, out) = run_capped(&["/usr/bin/env".into()], Duration::from_secs(5)).unwrap();
        assert_eq!(code, Some(0));
        assert_eq!(out.trim(), format!("PATH={CHILD_PATH}"));
        // Output cap: 1 MiB from the child, at most MAX_OUT kept, no hang.
        let (code, out) = run_capped(
            &[
                "/usr/bin/head".into(),
                "-c".into(),
                "1048576".into(),
                "/dev/zero".into(),
            ],
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(code, Some(0));
        assert_eq!(out.len() as u64, MAX_OUT);
        // Timeout: killed well before the child would finish.
        let t = Instant::now();
        let e = run_capped(
            &["/bin/sleep".into(), "30".into()],
            Duration::from_millis(300),
        )
        .unwrap_err();
        assert!(e.starts_with("timeout"), "{e}");
        assert!(t.elapsed() < Duration::from_secs(5));
        // Missing binary: a reason, not a panic.
        let e = run_capped(
            &["/nonexistent/openmind-memory".into()],
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(e.starts_with("spawn failed"), "{e}");
    }

    #[test]
    fn missing_binary_becomes_an_info_note_through_the_cycle() {
        let comps = [];
        let logs = LogInsights::default();
        let mut i = Input::new(1000, &comps, &logs);
        i.live = true;
        let mut m = MemoryFreshness::new(settings("/nonexistent/openmind-memory"));
        let mut cx = crate::cortex::Cortex::default();
        m.sense(&i, &mut Sink::new(&mut cx, NAME));
        assert_eq!(m.rounds, 1);
        m.wait_idle();
        let mut cx = crate::cortex::Cortex::default();
        i.now = 1001;
        m.sense(&i, &mut Sink::new(&mut cx, NAME));
        assert_eq!(m.rounds, 1, "not due again before every_s");
        assert_eq!(cx.bleeding.len(), 1);
        assert_eq!(cx.bleeding[0].id, "memory-freshness-unknown");
        assert!(cx.signals.is_empty(), "no alert for an unknown verdict");
    }
}
