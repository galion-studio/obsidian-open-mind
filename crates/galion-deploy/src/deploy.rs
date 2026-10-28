// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The deploy state machine. Order of operations (each step's journal phase is
//! written before the step runs):
//!
//! ```text
//! preflight (unit known, target resolvable, new file is ELF)       nothing changed
//! lock + recover a stale journal                                   [prepare]
//! <new> --version (bounded), backup copy, data backup, log line    [prepare]
//! install <new> -> <target>.new -> rename                          [install]
//! reset-failed, restart, health loop                               [verify]
//!   (optional: GALION_DEPLOY_FAILFAST=N ends the loop early once the unit has
//!    restarted itself N times since our restart, or sits in `failed`)
//!   ok   -> prune backups, drop journal, exit 0
//!   fail -> install backup (or stop + remove on first install),    [rollback]
//!           reset-failed, restart, health on the unit's own path -> exit 3 / 4
//! ```

use crate::backend::{Direct, Manager, Systemd, UnitState};
use crate::journal::{self, Entry, Found, Phase};
use crate::{files, http, sha256, sys, zagreb};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const USAGE: &str = "usage: galion-deploy <service> <new-binary> [health-url]";

/// Exit codes. The bash script used 1 for everything that was not a success (F3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// New binary installed and healthy.
    Deployed = 0,
    /// Refused before anything changed (unknown unit, not ELF, lock held, bad env).
    Refused = 1,
    /// Bad command line.
    Usage = 2,
    /// New binary failed its health check; the old one is back and healthy
    /// (first install: unit stopped, binary removed).
    RolledBack = 3,
    /// The rollback is unhealthy too: manual action needed.
    RollbackFailed = 4,
    /// Interrupted (SIGINT/SIGTERM/SIGHUP); the old state is back and healthy.
    Interrupted = 5,
    /// Interrupted, and the restored old state is unhealthy: manual action needed.
    InterruptedRollbackFailed = 6,
}

/// Default health URLs (the bash `HEALTH` table plus galion-nerve, F4, and room).
pub fn health_table(unit: &str) -> Option<&'static str> {
    Some(match unit {
        "openmind-memory" => "http://127.0.0.1:8420/health",
        "hermes-rs" => "http://127.0.0.1:8642/health",
        "galion-desk" => "http://127.0.0.1:8646/health",
        "tuner" => "http://127.0.0.1:8650/health",
        "galion-health" => "http://127.0.0.1:8660/healthz-self",
        "link-watch-rs" => "none",
        // F4: the live nerve answers /health with 404; /healthz is the path both
        // the old (ws/nerve) and the new (openmind-nerve) builds serve.
        "galion-nerve" => "http://127.0.0.1:8670/healthz",
        // F5: the staged room unit (swarm/room deploy/staged/room.service).
        "room" => "http://127.0.0.1:4020/health",
        _ => return None,
    })
}

fn remap(url: &str) -> String {
    if url == "http://127.0.0.1:8660/healthz-self" {
        "http://127.0.0.1:8660/status.json".into()
    } else {
        url.into()
    }
}

/// F4: the rollback restores the unit's previous binary, so it is judged on the
/// unit's own health *path* (from the table) at the host and port of the URL in use.
/// `galion-deploy galion-nerve <new> http://127.0.0.1:8670/health` therefore judges
/// the new build on /health and the rollback to the old nerve on /healthz.
pub fn rollback_health(unit: &str, url: &str) -> String {
    let own = health_table(unit).map(remap);
    match (
        own.as_deref().and_then(http::split_http),
        http::split_http(url),
    ) {
        (Some((_, path)), Some((hostport, _))) => format!("http://{hostport}{path}"),
        _ => url.to_string(),
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub unit: String,
    pub new: PathBuf,
    /// Health URL for the new binary, or `none` (is-active for 5 s).
    pub url: String,
    /// Health URL for the rollback (the unit's own path, F4).
    pub rollback_url: String,
    pub backup_dir: PathBuf,
    pub data_backup: PathBuf,
    pub skip_data_backup: bool,
    /// `None` = do not run `<new> --version` at all.
    pub version_timeout: Option<Duration>,
    pub reset_failed: bool,
    pub tries: u32,
    pub interval: Duration,
    /// Crash-loop fail-fast for the forward health loop: give up once the unit has
    /// restarted itself this many times since our restart (or sits in `failed` on two
    /// polls in a row). 0 = off (default): the full tries x interval window, as bash.
    pub failfast: u32,
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

/// `5`, `2.5`, `5s`; `0` disables the probe.
pub fn parse_secs(v: &str) -> Option<Option<Duration>> {
    let n = v.strip_suffix('s').unwrap_or(v);
    let f: f64 = n.parse().ok()?;
    if !f.is_finite() || f < 0.0 {
        return None;
    }
    Some(if f == 0.0 {
        None
    } else {
        Some(Duration::from_secs_f64(f))
    })
}

impl Config {
    /// Build from the command line (`<service> <new-binary> [health-url]`) and env.
    pub fn from_env(unit: &str, new: &str, url: Option<&str>) -> Result<Config, String> {
        let unit = unit.strip_suffix(".service").unwrap_or(unit).to_string();
        let url = remap(
            url.filter(|u| !u.is_empty())
                .or(health_table(&unit))
                .unwrap_or("none"),
        );
        let rollback_url =
            env("GALION_DEPLOY_ROLLBACK_HEALTH").unwrap_or_else(|| rollback_health(&unit, &url));
        let version_timeout = match env("GALION_DEPLOY_VERSION_TIMEOUT") {
            None => Some(Duration::from_secs(5)),
            Some(v) => parse_secs(&v)
                .ok_or_else(|| format!("bad GALION_DEPLOY_VERSION_TIMEOUT={v} (seconds)"))?,
        };
        let num = |k: &str, d: u64| -> Result<u64, String> {
            env(k).map_or(Ok(d), |v| v.parse().map_err(|_| format!("bad {k}={v}")))
        };
        Ok(Config {
            unit,
            new: PathBuf::from(new),
            url,
            rollback_url,
            backup_dir: env("GALION_DEPLOY_BACKUP_DIR")
                .unwrap_or_else(|| "/var/backups/galion/deploy".into())
                .into(),
            data_backup: env("GALION_DEPLOY_DATA_BACKUP")
                .unwrap_or_else(|| "/opt/galion/rust/bin/galion-backup.sh".into())
                .into(),
            skip_data_backup: std::env::var("SKIP_DATA_BACKUP").as_deref() == Ok("1"),
            version_timeout,
            reset_failed: std::env::var("GALION_DEPLOY_RESET_FAILED").as_deref() != Ok("0"),
            tries: num("GALION_DEPLOY_HEALTH_TRIES", 20)?.clamp(1, 10_000) as u32,
            interval: Duration::from_millis(num("GALION_DEPLOY_HEALTH_INTERVAL_MS", 1000)?),
            failfast: num("GALION_DEPLOY_FAILFAST", 0)?.min(1000) as u32,
        })
    }
}

pub fn manager_from_env() -> Result<Box<dyn Manager>, String> {
    match std::env::var("GALION_DEPLOY_SM").ok().as_deref() {
        None | Some("") | Some("systemd") => Ok(Box::new(Systemd)),
        Some("direct") => {
            let ctl: Vec<String> = env("GALION_DEPLOY_CTL")
                .ok_or("GALION_DEPLOY_SM=direct needs GALION_DEPLOY_CTL")?
                .split_whitespace()
                .map(str::to_string)
                .collect();
            if ctl.is_empty() {
                return Err("GALION_DEPLOY_SM=direct needs GALION_DEPLOY_CTL".into());
            }
            Ok(Box::new(Direct { ctl }))
        }
        Some(o) => Err(format!("unknown GALION_DEPLOY_SM={o} (systemd|direct)")),
    }
}

/// Log lines go to stdout (as before) and, once the backup dir exists, are appended
/// to `<backup-dir>/<unit>.log`, so the outcome of a deploy whose ssh session
/// dropped can still be read. Write errors (closed stdout) are ignored.
pub struct Log {
    pub file: Option<PathBuf>,
    pub lines: Vec<String>,
}

impl Log {
    pub fn new() -> Log {
        Log {
            file: None,
            lines: Vec::new(),
        }
    }
    pub fn log(&mut self, rest: &str) {
        let line = format!(
            "ts={} svc=galion-deploy {rest}",
            zagreb::iso_seconds(zagreb::now_secs())
        );
        self.raw(&line);
    }
    fn raw(&mut self, line: &str) {
        let mut o = std::io::stdout().lock();
        let _ = writeln!(o, "{line}");
        let _ = o.flush();
        if let Some(f) = &self.file {
            if let Ok(mut h) = fs::OpenOptions::new().create(true).append(true).open(f) {
                let _ = writeln!(h, "{line}");
            }
        }
        self.lines.push(line.to_string());
    }
}

impl Default for Log {
    fn default() -> Self {
        Log::new()
    }
}

fn sha12(p: &Path) -> String {
    sha256::file_hex(p)
        .map(|h| h[..12].to_string())
        .unwrap_or_else(|_| "unreadable".into())
}

/// Sleep `d` in 50 ms slices; true if a signal arrived.
fn nap(d: Duration, interruptible: bool) -> bool {
    let end = Instant::now() + d;
    loop {
        if interruptible && sys::pending() != 0 {
            return true;
        }
        let now = Instant::now();
        if now >= end {
            return false;
        }
        std::thread::sleep((end - now).min(Duration::from_millis(50)));
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Health {
    Ok,
    Fail,
    Interrupted,
}

#[derive(Debug, PartialEq, Eq)]
enum Pause {
    Done,
    Interrupted,
    CrashLoop,
}

/// How often fail-fast polls the unit's state during the pause before a try.
const FAILFAST_POLL: Duration = Duration::from_millis(250);

/// Crash-loop detector of one forward health loop.
#[derive(Debug)]
struct FailFast {
    limit: u32,
    base: u32,
    t0: Instant,
    failed_polls: u32,
}

impl FailFast {
    /// `Some(reason)` once the new build is crash-looping: `limit` crashes since our
    /// restart (automatic restarts done, `NRestarts` growth, plus one that is pending,
    /// `SubState=auto-restart`, so a crash counts when it happens and not
    /// `RestartSec` later), or `failed` on two polls in a row (Restart=no, or
    /// the start limit was hit: systemd will not try again). One `failed` poll is
    /// not enough, because systemd >= 254 passes through `failed` for an instant on
    /// its way to an automatic restart.
    fn verdict(&mut self, s: &UnitState) -> Option<String> {
        if s.n_restarts < self.base {
            // the counter was zeroed under us (reset-failed by someone else)
            self.base = 0;
        }
        let pending = u32::from(s.sub_state == "auto-restart");
        let n = s.n_restarts - self.base + pending;
        if n >= self.limit {
            return Some(format!(
                "{n} crash(es) of the new build since the deploy restart (limit {})",
                self.limit
            ));
        }
        if s.active_state == "failed" {
            self.failed_polls += 1;
            if self.failed_polls >= 2 {
                return Some("unit is in state failed and will not restart itself".into());
            }
        } else {
            self.failed_polls = 0;
        }
        None
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Version {
    Skipped,
    Exited,
    Killed,
    Interrupted,
    NotRun(String),
}

/// F1: `<new> --version` with stdio on /dev/null, in its own process group, killed
/// (the whole group) after `limit`. A binary that ignores its argv and serves on its
/// default port can therefore hold that port for at most `limit`.
pub fn version_probe(new: &Path, limit: Option<Duration>) -> Version {
    let Some(limit) = limit else {
        return Version::Skipped;
    };
    let mut c = Command::new(new);
    c.arg("--version");
    bounded(c, limit)
}

fn bounded(mut cmd: Command, limit: Duration) -> Version {
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return Version::NotRun(e.to_string()),
    };
    let pg = child.id() as i32;
    let t0 = Instant::now();
    let result = loop {
        if let Ok(Some(_)) = child.try_wait() {
            break Version::Exited;
        }
        if sys::pending() != 0 {
            break Version::Interrupted;
        }
        if t0.elapsed() >= limit {
            break Version::Killed;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    // Kill the group in every case: a `--version` that forked must not linger.
    sys::send(-pg, sys::SIGKILL);
    let _ = child.kill();
    let _ = child.wait();
    result
}

struct Run<'a> {
    cfg: &'a Config,
    sm: &'a mut dyn Manager,
    log: &'a mut Log,
}

impl Run<'_> {
    fn die(&mut self, msg: &str) -> Exit {
        self.log.log(&format!("level=error msg=\"{msg}\""));
        Exit::Refused
    }

    /// The health loop: `tries` tries, `interval` apart. With `failfast` (forward
    /// deploy only, and only when `GALION_DEPLOY_FAILFAST` > 0 and the backend can
    /// read `NRestarts`) the pause before each try is cut into polls of the unit's
    /// state, and the loop gives up as soon as the new build is crash-looping.
    fn healthy(&mut self, url: &str, interruptible: bool, failfast: bool) -> Health {
        let unit = self.cfg.unit.clone();
        let min = 5.min(self.cfg.tries);
        let mut ff = if failfast { self.failfast_arm() } else { None };
        for i in 1..=self.cfg.tries {
            match self.pause(interruptible, &mut ff) {
                Pause::Done => {}
                Pause::Interrupted => return Health::Interrupted,
                Pause::CrashLoop => return Health::Fail,
            }
            if !self.sm.is_active(&unit) {
                continue;
            }
            if url == "none" {
                if i >= min && self.sm.is_active(&unit) {
                    return Health::Ok;
                }
                continue;
            }
            if (200..300).contains(&http::status(url, Duration::from_secs(3))) {
                return Health::Ok;
            }
        }
        Health::Fail
    }

    /// Read the restart counter right after our restart; it is the baseline (systemd
    /// may or may not zero `NRestarts` on a manual restart, so only growth counts).
    fn failfast_arm(&mut self) -> Option<FailFast> {
        let limit = self.cfg.failfast;
        if limit == 0 {
            return None;
        }
        let unit = self.cfg.unit.clone();
        match self.sm.unit_state(&unit) {
            Some(s) => {
                self.log.log(&format!(
                    "level=info msg=\"fail-fast armed\" unit={unit} crash_limit={limit} nrestarts_base={} state={}/{}",
                    s.n_restarts, s.active_state, s.sub_state
                ));
                Some(FailFast {
                    limit,
                    base: s.n_restarts,
                    t0: Instant::now(),
                    failed_polls: 0,
                })
            }
            None => {
                self.log.log(&format!(
                    "level=warn msg=\"fail-fast off: cannot read NRestarts; using the full health window\" unit={unit}"
                ));
                None
            }
        }
    }

    /// The pause before a try. Without fail-fast it is `nap(interval)`, exactly as
    /// before. With it, the unit's state is polled every FAILFAST_POLL (or interval,
    /// if shorter) during the pause.
    fn pause(&mut self, interruptible: bool, ff: &mut Option<FailFast>) -> Pause {
        let Some(f) = ff.as_mut() else {
            return if nap(self.cfg.interval, interruptible) {
                Pause::Interrupted
            } else {
                Pause::Done
            };
        };
        let end = Instant::now() + self.cfg.interval;
        let unit = self.cfg.unit.clone();
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if nap(left.min(FAILFAST_POLL), interruptible) {
                return Pause::Interrupted;
            }
            if let Some(s) = self.sm.unit_state(&unit) {
                if let Some(why) = f.verdict(&s) {
                    self.log.log(&format!(
                        "level=error msg=\"crash loop: {why}; failing fast\" unit={unit} after_ms={} nrestarts={} state={}/{}",
                        f.t0.elapsed().as_millis(),
                        s.n_restarts,
                        s.active_state,
                        s.sub_state
                    ));
                    return Pause::CrashLoop;
                }
            }
            if Instant::now() >= end {
                return Pause::Done;
            }
        }
    }

    /// F2: reset-failed first (so a full start-limit window cannot refuse us), and
    /// one retry after reset-failed if the restart is refused anyway (a crash-looping
    /// unit can refill the window between the two calls).
    fn restart(&mut self) -> bool {
        let unit = self.cfg.unit.clone();
        if self.cfg.reset_failed {
            self.sm.reset_failed(&unit);
        }
        if self.sm.restart(&unit) {
            return true;
        }
        if self.cfg.reset_failed {
            self.log.log(&format!(
                "level=warn msg=\"restart refused, reset-failed and retry\" unit={unit}"
            ));
            self.sm.reset_failed(&unit);
            return self.sm.restart(&unit);
        }
        false
    }

    fn show_journal(&mut self) {
        let unit = self.cfg.unit.clone();
        let j = self.sm.journal(&unit, 20);
        for l in j.lines() {
            let mut s = format!("  | {l}").into_bytes();
            s.truncate(240);
            self.log.raw(&String::from_utf8_lossy(&s));
        }
    }

    /// Put the pre-deploy state back. Signals are ignored from here on: a rollback
    /// that was started must finish. True if the restored state is healthy.
    fn rollback(&mut self, e: &mut Entry) -> bool {
        let bdir = self.cfg.backup_dir.clone();
        if journal::set_phase(&bdir, e, Phase::Rollback).is_err() {
            self.log
                .log("level=warn msg=\"cannot update the rollback journal; rolling back anyway\"");
        }
        let unit = e.unit.clone();
        let ok = match e.backup.clone() {
            None => {
                // First install (F5): back to "no binary, unit not running".
                self.sm.stop(&unit);
                let _ = fs::remove_file(&e.target);
                files::sync_dir(&e.target);
                let gone = !e.target.exists() && !self.sm.is_active(&unit);
                if gone {
                    self.log.log(&format!(
                        "level=warn msg=\"first install undone: unit stopped, binary removed\" unit={unit} target={}",
                        e.target.display()
                    ));
                }
                gone
            }
            Some(bak) => {
                if let Err(err) = files::install(&bak, &e.target, 0o755) {
                    self.log.log(&format!(
                        "level=error msg=\"cannot reinstall backup: {err}\" unit={unit} backup={}",
                        bak.display()
                    ));
                    false
                } else {
                    let restarted = self.restart();
                    if !restarted {
                        self.log.log(&format!(
                            "level=error msg=\"rollback restart refused\" unit={unit}"
                        ));
                    }
                    let url = e.health.clone();
                    restarted && self.healthy(&url, false, false) == Health::Ok
                }
            }
        };
        let late = sys::clear();
        if late != 0 {
            self.log.log(&format!(
                "level=warn msg=\"{} received during rollback; ignored until it finished\"",
                sys::name(late)
            ));
        }
        journal::remove(&bdir, &unit);
        ok
    }

    fn rollback_failed_msg(&mut self, e: &Entry) {
        let bak = e
            .backup
            .as_ref()
            .map(|b| b.display().to_string())
            .unwrap_or_else(|| "none, first install".into());
        self.log.log(&format!(
            "level=error msg=\"ROLLBACK ALSO UNHEALTHY for {}; manual action needed (backup {bak})\"",
            e.unit
        ));
    }

    /// Restore after a deploy that died (stale journal). `Ok(true)` = something was
    /// restored and is healthy, `Ok(false)` = nothing to restore.
    fn recover_stale(&mut self) -> Result<bool, Exit> {
        let bdir = self.cfg.backup_dir.clone();
        let unit = self.cfg.unit.clone();
        match journal::look(&bdir, &unit) {
            Found::None => Ok(false),
            Found::Busy(pid) => Err(self.die(&format!(
                "another galion-deploy (pid {pid}) is deploying {unit}; journal {}",
                journal::path(&bdir, &unit).display()
            ))),
            Found::Corrupt(_) => Err(self.die(&format!(
                "unreadable rollback journal {}; inspect it, then remove it",
                journal::path(&bdir, &unit).display()
            ))),
            Found::Stale(mut e) => {
                self.log.log(&format!(
                    "level=warn msg=\"found the journal of an interrupted deploy\" unit={unit} pid={} started={} phase={} backup={}",
                    e.pid,
                    e.started,
                    e.phase.as_str(),
                    e.backup.as_ref().map(|b| b.display().to_string()).unwrap_or_else(|| "none".into())
                ));
                if !e.phase.touched_target() {
                    journal::remove(&bdir, &unit);
                    self.log.log(&format!(
                        "level=info msg=\"it had not installed anything; journal dropped\" unit={unit}"
                    ));
                    return Ok(false);
                }
                if self.rollback(&mut e) {
                    self.log.log(&format!(
                        "level=warn msg=\"rolled back ok\" unit={unit} reason=recovered-interrupted-deploy"
                    ));
                    Ok(true)
                } else {
                    self.rollback_failed_msg(&e);
                    Err(Exit::RollbackFailed)
                }
            }
        }
    }

    fn secure_backup_dir(&mut self) -> Result<(), Exit> {
        let b = self.cfg.backup_dir.clone();
        if let Err(e) = fs::create_dir_all(&b) {
            return Err(self.die(&format!("cannot create {}: {e}", b.display())));
        }
        // `chmod 0700 "${BDIR%/*}" "$BDIR"`
        let s = b.to_string_lossy().to_string();
        let mut dirs = Vec::new();
        if let Some(i) = s.rfind('/') {
            if i > 0 {
                dirs.push(PathBuf::from(&s[..i]));
            }
        }
        dirs.push(b.clone());
        for d in dirs {
            if let Err(e) = fs::set_permissions(&d, fs::Permissions::from_mode(0o700)) {
                return Err(self.die(&format!("cannot chmod 0700 {}: {e}", d.display())));
            }
        }
        self.log.file = Some(b.join(format!("{}.log", self.cfg.unit)));
        Ok(())
    }

    fn data_backup(&mut self) {
        let p = &self.cfg.data_backup;
        let exec = fs::metadata(p)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
        if exec && !self.cfg.skip_data_backup {
            // Failure is ignored, as before (`|| true`).
            let _ = Command::new(p)
                .stdin(Stdio::null())
                .process_group(0)
                .status();
        }
    }

    fn prune(&mut self) {
        let prefix = format!("{}-", self.cfg.unit);
        let Ok(rd) = fs::read_dir(&self.cfg.backup_dir) else {
            return;
        };
        let mut v: Vec<(std::time::SystemTime, String, PathBuf)> = rd
            .flatten()
            .filter_map(|d| {
                let n = d.file_name().to_string_lossy().to_string();
                let stamp = n.strip_prefix(&prefix)?.strip_suffix(".bin")?;
                if !zagreb::is_compact(stamp) {
                    return None; // another unit's backup (e.g. tuner-x-...) or a stray file
                }
                let m = d.metadata().ok()?.modified().ok()?;
                Some((m, n, d.path()))
            })
            .collect();
        // `ls -1t`: newest first (ties: newer stamp first). Keep what bash kept (the
        // 5 newest files) and, in addition, the newest copy of each of the 5 newest
        // distinct binaries: five failed deploys back up the same binary five times,
        // and under the bash rule those copies pushed the previous version (the L2
        // rollback target) out. Nothing bash would keep is ever deleted.
        v.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        let mut seen = std::collections::HashSet::new();
        for (i, (_, _, p)) in v.into_iter().enumerate() {
            let h = sha256::file_hex(&p).unwrap_or_default();
            let newest_of_its_kind = seen.len() < 5 && seen.insert(h);
            if i >= 5 && !newest_of_its_kind {
                let _ = fs::remove_file(p);
            }
        }
    }

    fn interrupted_before_install(&mut self, sig: i32) -> Exit {
        let unit = self.cfg.unit.clone();
        sys::clear();
        journal::remove(&self.cfg.backup_dir, &unit);
        self.log.log(&format!(
            "level=warn msg=\"interrupted by {} before install; nothing changed\" unit={unit}",
            sys::name(sig)
        ));
        Exit::Interrupted
    }

    fn deploy(&mut self) -> Exit {
        let cfg = self.cfg;
        let unit = cfg.unit.clone();
        if !self.sm.known(&unit) {
            return self.die(&format!("unknown unit {unit}.service"));
        }
        let Some(target) = self.sm.exec_path(&unit) else {
            return self.die(&format!("cannot resolve installed binary for {unit}"));
        };
        let target = PathBuf::from(target);
        let first = match fs::symlink_metadata(&target) {
            Ok(m) if m.is_file() => false,
            Ok(m) if m.file_type().is_symlink() && target.is_file() => false,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if self.sm.is_active(&unit) {
                    return self.die(&format!(
                        "{unit} is active but its binary {} is missing; refusing a first install over a running unit",
                        target.display()
                    ));
                }
                let parent_ok = target.parent().map(|d| d.is_dir()).unwrap_or(false);
                if !target.is_absolute() || !parent_ok {
                    return self.die(&format!("cannot resolve installed binary for {unit}"));
                }
                true
            }
            _ => return self.die(&format!("cannot resolve installed binary for {unit}")),
        };
        if !cfg.new.is_file() {
            return self.die(&format!("no such file {}", cfg.new.display()));
        }
        if !files::is_elf(&cfg.new) {
            return self.die(&format!("{} is not an ELF binary", cfg.new.display()));
        }

        if let Err(x) = self.secure_backup_dir() {
            return x;
        }
        match self.recover_stale() {
            Ok(_) => {}
            Err(x) => return x,
        }
        let bdir = cfg.backup_dir.clone();
        let stamp = zagreb::compact(zagreb::now_secs());
        let bak = (!first).then(|| bdir.join(format!("{unit}-{stamp}.bin")));
        let mut e = Entry {
            pid: std::process::id(),
            started: zagreb::iso_seconds(zagreb::now_secs()),
            phase: Phase::Prepare,
            unit: unit.clone(),
            target: target.clone(),
            backup: bak.clone(),
            new: cfg.new.clone(),
            sha_old: if first { "none".into() } else { sha12(&target) },
            sha_new: sha12(&cfg.new),
            health: cfg.rollback_url.clone(),
        };
        if let Err(err) = journal::create(&bdir, &e) {
            return self.die(&format!(
                "cannot take the deploy lock {}: {err}",
                journal::path(&bdir, &unit).display()
            ));
        }

        match version_probe(&cfg.new, cfg.version_timeout) {
            Version::Killed => self.log.log(&format!(
                "level=warn msg=\"{} --version did not exit within {:?}; killed its process group\" unit={unit}",
                cfg.new.display(),
                cfg.version_timeout.unwrap_or_default()
            )),
            Version::Interrupted => return self.interrupted_before_install(sys::pending()),
            Version::NotRun(err) => self.log.log(&format!(
                "level=warn msg=\"{} --version could not run: {err}\" unit={unit}",
                cfg.new.display()
            )),
            Version::Skipped | Version::Exited => {}
        }

        if let Some(b) = &bak {
            if let Err(err) = files::copy_preserving(&target, b) {
                journal::remove(&bdir, &unit);
                return self.die(&format!("backup of {} failed: {err}", target.display()));
            }
        }
        self.data_backup();
        if first {
            self.log.log(&format!(
                "level=info msg=\"first install\" unit={unit} target={} backup=none sha_new={}",
                target.display(),
                e.sha_new
            ));
        } else {
            self.log.log(&format!(
                "level=info msg=\"backup\" unit={unit} target={} backup={} sha_old={} sha_new={}",
                target.display(),
                bak.as_ref()
                    .map(|b| b.display().to_string())
                    .unwrap_or_default(),
                e.sha_old,
                e.sha_new
            ));
        }
        if sys::pending() != 0 {
            return self.interrupted_before_install(sys::pending());
        }

        // ---- from here on the target may change: every exit path restores it ----
        let _ = journal::set_phase(&bdir, &mut e, Phase::Install);
        if let Err(err) = files::install(&cfg.new, &target, 0o755) {
            self.log.log(&format!(
                "level=error msg=\"install failed: {err}\" unit={unit}"
            ));
            return self.after_failure(&mut e, None, "install failed");
        }
        let _ = journal::set_phase(&bdir, &mut e, Phase::Verify);
        if !self.restart() {
            return self.after_failure(&mut e, None, "restart refused");
        }
        match self.healthy(&cfg.url, true, true) {
            Health::Ok => {
                self.log.log(&format!(
                    "level=info msg=\"deployed\" unit={unit} health={}",
                    cfg.url
                ));
                journal::remove(&bdir, &unit);
                self.prune();
                Exit::Deployed
            }
            Health::Interrupted => {
                let sig = sys::pending();
                self.after_failure(&mut e, Some(sig), "interrupted")
            }
            Health::Fail => self.after_failure(&mut e, None, "health failed"),
        }
    }

    fn after_failure(&mut self, e: &mut Entry, sig: Option<i32>, why: &str) -> Exit {
        let unit = e.unit.clone();
        match sig {
            Some(s) => self.log.log(&format!(
                "level=warn msg=\"interrupted by {}, rolling back\" unit={unit}",
                sys::name(s)
            )),
            None => {
                let extra = if self.cfg.rollback_url != self.cfg.url {
                    format!(" rollback_health={}", self.cfg.rollback_url)
                } else {
                    String::new()
                };
                self.log.log(&format!(
                    "level=error msg=\"{why}, rolling back\" unit={unit}{extra}"
                ));
                self.show_journal();
            }
        }
        let ok = self.rollback(e);
        match (ok, sig) {
            (true, None) => {
                self.prune();
                self.log
                    .log(&format!("level=warn msg=\"rolled back ok\" unit={unit}"));
                Exit::RolledBack
            }
            (true, Some(_)) => {
                self.prune();
                self.log.log(&format!(
                    "level=warn msg=\"rolled back ok\" unit={unit} reason=interrupted"
                ));
                Exit::Interrupted
            }
            (false, None) => {
                self.rollback_failed_msg(e);
                Exit::RollbackFailed
            }
            (false, Some(_)) => {
                self.rollback_failed_msg(e);
                Exit::InterruptedRollbackFailed
            }
        }
    }
}

/// Run one deploy with an explicit backend (tests plug in their own).
pub fn deploy(cfg: &Config, sm: &mut dyn Manager, log: &mut Log) -> Exit {
    Run { cfg, sm, log }.deploy()
}

/// `galion-deploy --recover <service>`: only restore after an interrupted deploy.
/// 0 = nothing to do, 3 = restored and healthy, 4 = restored but unhealthy, 1 = busy.
pub fn recover(cfg: &Config, sm: &mut dyn Manager, log: &mut Log) -> Exit {
    let mut r = Run { cfg, sm, log };
    if !r.cfg.backup_dir.is_dir() {
        r.log.log(&format!(
            "level=info msg=\"no interrupted deploy\" unit={}",
            r.cfg.unit
        ));
        return Exit::Deployed;
    }
    if let Err(x) = r.secure_backup_dir() {
        return x;
    }
    match r.recover_stale() {
        Ok(true) => Exit::RolledBack,
        Ok(false) => {
            r.log.log(&format!(
                "level=info msg=\"no interrupted deploy\" unit={}",
                r.cfg.unit
            ));
            Exit::Deployed
        }
        Err(x) => x,
    }
}

pub const HELP: &str = "\
usage: galion-deploy <service> <new-binary> [health-url]
       galion-deploy --recover <service>

Backup -> atomic install -> restart -> health check -> automatic rollback.

exit codes:
  0  deployed (or --recover: nothing to recover)
  1  refused before anything changed (unknown unit, not ELF, deploy lock held, ...)
  2  usage
  3  health failed; rolled back, old binary healthy (first install: unit stopped, binary removed)
  4  rollback also unhealthy: manual action needed
  5  interrupted (SIGINT/SIGTERM/SIGHUP); old state restored and healthy
  6  interrupted; restored state unhealthy: manual action needed

environment (unset = default):
  GALION_DEPLOY_SM=systemd|direct      service manager (default systemd)
  GALION_DEPLOY_CTL=\"simsup --root D ctl\" control command for the direct backend
  GALION_DEPLOY_BACKUP_DIR             default /var/backups/galion/deploy
  GALION_DEPLOY_DATA_BACKUP            default /opt/galion/rust/bin/galion-backup.sh
  SKIP_DATA_BACKUP=1                   do not run the data backup
  GALION_DEPLOY_VERSION_TIMEOUT=5      seconds for `<new> --version`; 0 = skip it
  GALION_DEPLOY_RESET_FAILED=1         reset-failed before each restart; 0 = off
  GALION_DEPLOY_ROLLBACK_HEALTH=URL    health URL for the rollback (default: the unit's own)
  GALION_DEPLOY_HEALTH_TRIES=20        health tries ...
  GALION_DEPLOY_HEALTH_INTERVAL_MS=1000  ... and the pause before each
  GALION_DEPLOY_FAILFAST=0             N>0: end the forward health window early once the
                                       unit restarted itself N times since the deploy
                                       restart (systemd NRestarts) or stays failed;
                                       0 = off, the full window (default)
";

/// The binary's entry point; returns the process exit code.
pub fn main(args: &[String]) -> i32 {
    let mut log = Log::new();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let code = match a.as_slice() {
        ["--help" | "-h", ..] => {
            let _ = std::io::stdout().write_all(HELP.as_bytes());
            return 0;
        }
        ["--version", ..] => {
            let _ = writeln!(
                std::io::stdout(),
                "galion-deploy {}",
                env!("CARGO_PKG_VERSION")
            );
            return 0;
        }
        ["--recover", unit, ..] => match (Config::from_env(unit, "-", None), manager_from_env()) {
            (Ok(cfg), Ok(mut sm)) => recover(&cfg, sm.as_mut(), &mut log),
            (Err(e), _) | (_, Err(e)) => {
                log.log(&format!("level=error msg=\"{e}\""));
                Exit::Refused
            }
        },
        [unit, new, rest @ ..] => {
            match (
                Config::from_env(unit, new, rest.first().copied()),
                manager_from_env(),
            ) {
                (Ok(cfg), Ok(mut sm)) => deploy(&cfg, sm.as_mut(), &mut log),
                (Err(e), _) | (_, Err(e)) => {
                    log.log(&format!("level=error msg=\"{e}\""));
                    Exit::Refused
                }
            }
        }
        _ => {
            let _ = writeln!(std::io::stdout(), "{USAGE}");
            Exit::Usage
        }
    };
    code as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The signal flag is process-global: deploy tests run one at a time.
    static SERIAL: Mutex<()> = Mutex::new(());

    /// In-memory service manager: a unit is "active" after a restart if its target
    /// starts with `\x7fELFgood`.
    #[derive(Default)]
    struct Mock {
        target: PathBuf,
        known: bool,
        active: bool,
        refuse: u32,
        raise_on_restart: Option<i32>,
        calls: Vec<String>,
        /// `unit_state`: None = backend cannot tell. While the unit is not active,
        /// every read counts one more automatic restart (a crash loop), unless
        /// `stuck_failed` (Restart=no: the unit stays `failed`).
        nrestarts: Option<u32>,
        stuck_failed: bool,
    }

    impl Manager for Mock {
        fn known(&mut self, u: &str) -> bool {
            self.calls.push(format!("known {u}"));
            self.known
        }
        fn exec_path(&mut self, u: &str) -> Option<String> {
            self.calls.push(format!("exec-path {u}"));
            Some(self.target.display().to_string())
        }
        fn restart(&mut self, u: &str) -> bool {
            self.calls.push(format!("restart {u}"));
            if let Some(s) = self.raise_on_restart.take() {
                sys::raise_for_test(s);
            }
            if self.refuse > 0 {
                self.refuse -= 1;
                return false;
            }
            self.active = fs::read(&self.target)
                .map(|b| b.starts_with(b"\x7fELFgood"))
                .unwrap_or(false);
            true
        }
        fn stop(&mut self, u: &str) -> bool {
            self.calls.push(format!("stop {u}"));
            self.active = false;
            true
        }
        fn is_active(&mut self, _u: &str) -> bool {
            self.active
        }
        fn journal(&mut self, u: &str, n: u32) -> String {
            self.calls.push(format!("journal {u} {n}"));
            "boom: bind failed\n".into()
        }
        fn reset_failed(&mut self, u: &str) -> bool {
            self.calls.push(format!("reset-failed {u}"));
            true
        }
        fn unit_state(&mut self, u: &str) -> Option<UnitState> {
            self.calls.push(format!("show {u}"));
            let n = self.nrestarts.as_mut()?;
            let (state, sub) = if self.active {
                ("active", "running")
            } else if self.stuck_failed {
                ("failed", "failed")
            } else {
                *n += 1;
                ("activating", "auto-restart")
            };
            Some(UnitState {
                active_state: state.into(),
                sub_state: sub.into(),
                n_restarts: *n,
            })
        }
    }

    struct T {
        dir: PathBuf,
        cfg: Config,
        sm: Mock,
    }

    fn setup(tag: &str, old: Option<&[u8]>, new: &[u8]) -> T {
        let dir = std::env::temp_dir().join(format!("gd-unit-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("opt")).unwrap();
        let target = dir.join("opt/svc");
        if let Some(o) = old {
            fs::write(&target, o).unwrap();
        }
        fs::write(dir.join("new.bin"), new).unwrap();
        let cfg = Config {
            unit: "svc".into(),
            new: dir.join("new.bin"),
            url: "none".into(),
            rollback_url: "none".into(),
            backup_dir: dir.join("backups/galion/deploy"),
            data_backup: dir.join("no-data-backup"),
            skip_data_backup: true,
            version_timeout: None,
            reset_failed: true,
            tries: 6,
            interval: Duration::from_millis(1),
            failfast: 0,
        };
        let sm = Mock {
            target,
            known: true,
            active: old.is_some(),
            ..Mock::default()
        };
        T { dir, cfg, sm }
    }

    impl T {
        fn run(&mut self) -> (Exit, Vec<String>) {
            let mut log = Log::new();
            let x = deploy(&self.cfg, &mut self.sm, &mut log);
            (x, log.lines)
        }
        fn target(&self) -> Vec<u8> {
            fs::read(&self.sm.target).unwrap_or_default()
        }
        fn backups(&self) -> usize {
            fs::read_dir(&self.cfg.backup_dir)
                .map(|r| {
                    r.flatten()
                        .filter(|e| e.file_name().to_string_lossy().ends_with(".bin"))
                        .count()
                })
                .unwrap_or(0)
        }
        fn journal_exists(&self) -> bool {
            journal::path(&self.cfg.backup_dir, "svc").exists()
        }
    }

    impl Drop for T {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn has(lines: &[String], s: &str) -> bool {
        lines.iter().any(|l| l.contains(s))
    }

    fn ff(t: &mut T, limit: u32, tries: u32, interval_ms: u64) {
        t.cfg.failfast = limit;
        t.cfg.tries = tries;
        t.cfg.interval = Duration::from_millis(interval_ms);
    }

    #[test]
    fn failfast_cuts_a_crash_loop_short() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = setup("ff-loop", Some(b"\x7fELFgood-old"), b"\x7fELFbad-new");
        // full window would be 20 x 200 ms = 4 s
        ff(&mut t, 2, 20, 200);
        t.sm.nrestarts = Some(5); // non-zero base (the arming read makes it 6): only growth counts
        let t0 = Instant::now();
        let (x, lines) = t.run();
        let ms = t0.elapsed().as_millis();
        assert_eq!(x, Exit::RolledBack, "{lines:?}");
        assert_eq!(t.target(), b"\x7fELFgood-old");
        assert!(t.sm.active);
        assert!(has(
            &lines,
            "fail-fast armed\" unit=svc crash_limit=2 nrestarts_base=6 state=activating/auto-restart"
        ));
        assert!(has(
            &lines,
            "crash loop: 2 crash(es) of the new build since the deploy restart (limit 2); failing fast"
        ));
        // the documented message and the journal dump still follow
        assert!(has(&lines, "health failed, rolling back"));
        assert!(has(&lines, "msg=\"rolled back ok\" unit=svc"));
        assert!(ms < 2000, "fail-fast took {ms} ms");
        // the rollback keeps the full window and does not poll the state
        let shows = t.sm.calls.iter().filter(|c| c.starts_with("show")).count();
        // arm + 1 poll: the restart the arming read saw pending, plus one done
        assert_eq!(shows, 2, "{:?}", t.sm.calls);
    }

    #[test]
    fn failfast_stuck_failed_unit() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = setup("ff-failed", Some(b"\x7fELFgood-old"), b"\x7fELFbad-new");
        ff(&mut t, 3, 20, 200);
        t.sm.nrestarts = Some(0);
        t.sm.stuck_failed = true;
        let (x, lines) = t.run();
        assert_eq!(x, Exit::RolledBack, "{lines:?}");
        assert!(has(
            &lines,
            "crash loop: unit is in state failed and will not restart itself"
        ));
    }

    #[test]
    fn failfast_one_failed_poll_is_not_enough() {
        let mut f = FailFast {
            limit: 3,
            base: 1,
            t0: Instant::now(),
            failed_polls: 0,
        };
        let st = |a: &str, n: u32| UnitState {
            active_state: a.into(),
            sub_state: String::new(),
            n_restarts: n,
        };
        assert_eq!(f.verdict(&st("failed", 1)), None);
        assert_eq!(f.verdict(&st("activating", 2)), None);
        assert_eq!(f.verdict(&st("failed", 2)), None);
        assert!(f
            .verdict(&st("failed", 2))
            .unwrap()
            .contains("state failed"));
        // a counter zeroed under us re-bases at 0
        let mut g = FailFast {
            limit: 2,
            base: 7,
            t0: Instant::now(),
            failed_polls: 0,
        };
        assert_eq!(g.verdict(&st("activating", 1)), None);
        assert!(g.verdict(&st("activating", 2)).is_some());
        // a pending auto-restart counts as the crash it is
        let mut h = FailFast {
            limit: 1,
            base: 0,
            t0: Instant::now(),
            failed_polls: 0,
        };
        assert_eq!(h.verdict(&st("activating", 0)), None);
        let pend = UnitState {
            active_state: "activating".into(),
            sub_state: "auto-restart".into(),
            n_restarts: 0,
        };
        assert!(h.verdict(&pend).unwrap().starts_with("1 crash(es)"));
    }

    #[test]
    fn failfast_healthy_deploy_is_untouched() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = setup("ff-ok", Some(b"\x7fELFgood-old"), b"\x7fELFgood-new");
        ff(&mut t, 1, 20, 50);
        t.sm.nrestarts = Some(0);
        let (x, lines) = t.run();
        assert_eq!(x, Exit::Deployed, "{lines:?}");
        assert!(!has(&lines, "crash loop"));
        assert_eq!(t.target(), b"\x7fELFgood-new");
    }

    #[test]
    fn failfast_unavailable_falls_back_to_the_full_window() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = setup("ff-none", Some(b"\x7fELFgood-old"), b"\x7fELFbad-new");
        ff(&mut t, 2, 6, 50);
        t.sm.nrestarts = None;
        let t0 = Instant::now();
        let (x, lines) = t.run();
        assert_eq!(x, Exit::RolledBack, "{lines:?}");
        assert!(has(
            &lines,
            "fail-fast off: cannot read NRestarts; using the full health window"
        ));
        assert!(!has(&lines, "crash loop"));
        assert!(t0.elapsed() >= Duration::from_millis(300));
        let shows = t.sm.calls.iter().filter(|c| c.starts_with("show")).count();
        assert_eq!(shows, 1, "only the arming read");
    }

    #[test]
    fn failfast_env_parsing() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("GALION_DEPLOY_FAILFAST");
        assert_eq!(Config::from_env("x", "/n", None).unwrap().failfast, 0);
        std::env::set_var("GALION_DEPLOY_FAILFAST", "3");
        assert_eq!(Config::from_env("x", "/n", None).unwrap().failfast, 3);
        std::env::set_var("GALION_DEPLOY_FAILFAST", "lots");
        assert!(Config::from_env("x", "/n", None).is_err());
        std::env::remove_var("GALION_DEPLOY_FAILFAST");
    }

    #[test]
    fn healthy_deploy() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = setup("ok", Some(b"\x7fELFgood-old"), b"\x7fELFgood-new");
        let (x, lines) = t.run();
        assert_eq!(x, Exit::Deployed, "{lines:?}");
        assert_eq!(t.target(), b"\x7fELFgood-new");
        assert_eq!(t.backups(), 1);
        assert!(!t.journal_exists());
        assert!(has(&lines, "msg=\"backup\" unit=svc"));
        assert!(has(&lines, "msg=\"deployed\" unit=svc health=none"));
        assert!(lines[0].starts_with("ts=") && lines[0].contains("+0") && !lines[0].contains('Z'));
        assert_eq!(
            t.sm.calls,
            [
                "known svc",
                "exec-path svc",
                "reset-failed svc",
                "restart svc"
            ]
        );
        let log = fs::read_to_string(t.cfg.backup_dir.join("svc.log")).unwrap();
        assert!(log.contains("msg=\"deployed\""));
    }

    #[test]
    fn broken_rolls_back_with_exit_3() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = setup("bad", Some(b"\x7fELFgood-old"), b"\x7fELFbad-new");
        let (x, lines) = t.run();
        assert_eq!(x, Exit::RolledBack, "{lines:?}");
        assert_eq!(x as i32, 3);
        assert_eq!(t.target(), b"\x7fELFgood-old");
        assert!(t.sm.active);
        assert!(!t.journal_exists());
        assert!(has(&lines, "health failed, rolling back"));
        assert!(has(&lines, "  | boom: bind failed"));
        assert!(has(&lines, "msg=\"rolled back ok\" unit=svc"));
    }

    #[test]
    fn rollback_also_bad_is_exit_4() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = setup("bad2", Some(b"\x7fELFbad-old"), b"\x7fELFbad-new");
        let (x, lines) = t.run();
        assert_eq!(x, Exit::RollbackFailed, "{lines:?}");
        assert_eq!(t.target(), b"\x7fELFbad-old");
        assert!(has(
            &lines,
            "ROLLBACK ALSO UNHEALTHY for svc; manual action needed (backup "
        ));
        assert!(
            !t.journal_exists(),
            "a finished rollback must not block the next deploy"
        );
    }

    #[test]
    fn refused_restart_is_retried_after_reset_failed() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = setup("refuse", Some(b"\x7fELFgood-old"), b"\x7fELFgood-new");
        t.sm.refuse = 1;
        let (x, lines) = t.run();
        assert_eq!(x, Exit::Deployed, "{lines:?}");
        assert!(has(&lines, "restart refused, reset-failed and retry"));
        assert_eq!(
            t.sm.calls[2..],
            [
                "reset-failed svc",
                "restart svc",
                "reset-failed svc",
                "restart svc"
            ]
        );
    }

    #[test]
    fn refused_restart_without_reset_rolls_back() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = setup("refuse0", Some(b"\x7fELFgood-old"), b"\x7fELFgood-new");
        t.cfg.reset_failed = false;
        t.sm.refuse = 1;
        let (x, lines) = t.run();
        // bash died here under set -e with the NEW binary installed and no rollback.
        assert_eq!(x, Exit::RolledBack, "{lines:?}");
        assert_eq!(t.target(), b"\x7fELFgood-old");
        assert!(!t.sm.calls.iter().any(|c| c.starts_with("reset-failed")));
    }

    #[test]
    fn preflight_refusals() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = setup("pre", Some(b"\x7fELFgood-old"), b"#!/bin/sh\n");
        let (x, lines) = t.run();
        assert_eq!(x, Exit::Refused);
        assert!(has(&lines, "is not an ELF binary"));
        t.cfg.new = t.dir.join("missing");
        assert!(has(&t.run().1, "no such file"));
        t.sm.known = false;
        let (x, lines) = t.run();
        assert_eq!(x, Exit::Refused);
        assert!(has(&lines, "unknown unit svc.service"));
        assert_eq!(t.target(), b"\x7fELFgood-old");
        assert_eq!(t.backups(), 0);
        assert_eq!(main(&[]), 2);
        assert_eq!(main(&["tuner".into()]), 2);
    }

    #[test]
    fn first_install_and_its_rollback() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = setup("first", None, b"\x7fELFgood-new");
        let (x, lines) = t.run();
        assert_eq!(x, Exit::Deployed, "{lines:?}");
        assert_eq!(t.target(), b"\x7fELFgood-new");
        assert_eq!(t.backups(), 0);
        assert!(has(&lines, "msg=\"first install\" unit=svc"));

        let mut t = setup("firstbad", None, b"\x7fELFbad-new");
        let (x, lines) = t.run();
        assert_eq!(x, Exit::RolledBack, "{lines:?}");
        assert!(!t.sm.target.exists(), "the failed first install is removed");
        assert!(!t.sm.active);
        assert!(t.sm.calls.contains(&"stop svc".to_string()));
        assert!(has(&lines, "first install undone"));
    }

    #[test]
    fn missing_binary_of_a_running_unit_is_refused() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = setup("running", None, b"\x7fELFgood-new");
        t.sm.active = true;
        let (x, lines) = t.run();
        assert_eq!(x, Exit::Refused, "{lines:?}");
        assert!(!t.sm.target.exists());
    }

    #[test]
    fn signal_after_install_rolls_back_exit_5() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = setup("sig", Some(b"\x7fELFgood-old"), b"\x7fELFgood-new");
        t.sm.raise_on_restart = Some(sys::SIGTERM);
        let (x, lines) = t.run();
        assert_eq!(x, Exit::Interrupted, "{lines:?}");
        assert_eq!(t.target(), b"\x7fELFgood-old");
        assert!(t.sm.active);
        assert!(has(&lines, "interrupted by SIGTERM, rolling back"));
        assert!(has(&lines, "reason=interrupted"));
        assert!(!t.journal_exists());
        assert_eq!(sys::pending(), 0);
    }

    #[test]
    fn signal_before_install_changes_nothing() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = setup("sig0", Some(b"\x7fELFgood-old"), b"\x7fELFgood-new");
        sys::raise_for_test(sys::SIGINT);
        let (x, lines) = t.run();
        assert_eq!(x, Exit::Interrupted, "{lines:?}");
        assert!(has(
            &lines,
            "interrupted by SIGINT before install; nothing changed"
        ));
        assert_eq!(t.target(), b"\x7fELFgood-old");
        assert!(!t.sm.calls.iter().any(|c| c.starts_with("restart")));
        assert!(!t.journal_exists());
    }

    #[test]
    fn stale_journal_is_recovered_then_deploy_continues() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = setup(
            "stale",
            Some(b"\x7fELFbad-left-by-killed-deploy"),
            b"\x7fELFgood-new",
        );
        fs::create_dir_all(&t.cfg.backup_dir).unwrap();
        let bak = t.cfg.backup_dir.join("svc-20261020-230000+0200.bin");
        fs::write(&bak, b"\x7fELFgood-old").unwrap();
        let e = Entry {
            pid: 4_000_001,
            started: "2026-10-20T23:00:00+02:00".into(),
            phase: Phase::Verify,
            unit: "svc".into(),
            target: t.sm.target.clone(),
            backup: Some(bak),
            new: "/x".into(),
            sha_old: "a".into(),
            sha_new: "b".into(),
            health: "none".into(),
        };
        journal::create(&t.cfg.backup_dir, &e).unwrap();
        // --recover alone
        let mut log = Log::new();
        assert_eq!(recover(&t.cfg, &mut t.sm, &mut log), Exit::RolledBack);
        assert_eq!(t.target(), b"\x7fELFgood-old");
        assert!(!t.journal_exists());
        assert_eq!(recover(&t.cfg, &mut t.sm, &mut Log::new()), Exit::Deployed);
        // and recovery inside a normal deploy
        fs::write(&t.sm.target, b"\x7fELFbad-again").unwrap();
        journal::create(&t.cfg.backup_dir, &e).unwrap();
        let (x, lines) = t.run();
        assert_eq!(x, Exit::Deployed, "{lines:?}");
        assert!(has(&lines, "found the journal of an interrupted deploy"));
        assert!(has(&lines, "reason=recovered-interrupted-deploy"));
        assert_eq!(t.target(), b"\x7fELFgood-new");
    }

    #[test]
    fn corrupt_journal_refuses() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = setup("corrupt", Some(b"\x7fELFgood-old"), b"\x7fELFgood-new");
        fs::create_dir_all(&t.cfg.backup_dir).unwrap();
        fs::write(journal::path(&t.cfg.backup_dir, "svc"), "?").unwrap();
        let (x, lines) = t.run();
        assert_eq!(x, Exit::Refused);
        assert!(has(&lines, "unreadable rollback journal"));
        assert_eq!(t.target(), b"\x7fELFgood-old");
    }

    #[test]
    fn keeps_five_backups_of_its_own_unit_only() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut t = setup("prune", Some(b"\x7fELFgood-old"), b"\x7fELFgood-new");
        fs::create_dir_all(&t.cfg.backup_dir).unwrap();
        for i in 0..7 {
            let p = t
                .cfg
                .backup_dir
                .join(format!("svc-2026010{i}-000000+0100.bin"));
            fs::write(&p, format!("v{i}")).unwrap();
            let m = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 + i);
            fs::File::options()
                .write(true)
                .open(&p)
                .unwrap()
                .set_modified(m)
                .unwrap();
        }
        let other = t.cfg.backup_dir.join("svc-x-20260101-000000+0100.bin");
        fs::write(&other, b"x").unwrap();
        let (x, _) = t.run();
        assert_eq!(x, Exit::Deployed);
        // 8 own (7 + the new backup) -> 5 kept; the other unit's file is untouched
        assert_eq!(t.backups(), 6);
        assert!(other.exists());
        assert!(!t
            .cfg
            .backup_dir
            .join("svc-20260100-000000+0100.bin")
            .exists());
        // duplicates of one binary do not push older versions out
        for i in 0..4 {
            let p = t
                .cfg
                .backup_dir
                .join(format!("svc-2026020{i}-000000+0100.bin"));
            fs::write(&p, b"\x7fELFgood-new").unwrap();
        }
        fs::write(&t.cfg.new, b"\x7fELFgood-newer").unwrap();
        let (x, _) = t.run();
        assert_eq!(x, Exit::Deployed);
        let mut kept: Vec<Vec<u8>> = fs::read_dir(&t.cfg.backup_dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("svc-2"))
            .map(|e| fs::read(e.path()).unwrap())
            .collect();
        kept.sort();
        let n = kept.len();
        kept.dedup();
        assert!(
            n >= 5 && kept.len() == 5,
            "5 distinct binaries kept ({n} files)"
        );
    }

    #[test]
    fn version_probe_is_bounded() {
        let dir = std::env::temp_dir().join(format!("gd-vp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let t = Instant::now();
        let mut c = Command::new("sleep");
        c.arg("30");
        // not under SERIAL: no signal is raised here, but one may be pending from a
        // signal test, so accept Interrupted too
        let v = bounded(c, Duration::from_millis(300));
        assert!(matches!(v, Version::Killed | Version::Interrupted), "{v:?}");
        assert!(t.elapsed() < Duration::from_secs(3));
        assert_eq!(version_probe(&dir.join("nope"), None), Version::Skipped);
        assert!(matches!(
            version_probe(&dir.join("nope"), Some(Duration::from_secs(1))),
            Version::NotRun(_)
        ));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn secs_and_table() {
        assert_eq!(parse_secs("5"), Some(Some(Duration::from_secs(5))));
        assert_eq!(parse_secs("2.5s"), Some(Some(Duration::from_millis(2500))));
        assert_eq!(parse_secs("0"), Some(None));
        assert_eq!(parse_secs("-1"), None);
        assert_eq!(parse_secs("x"), None);
        assert_eq!(
            health_table("galion-nerve"),
            Some("http://127.0.0.1:8670/healthz")
        );
        assert_eq!(health_table("link-watch-rs"), Some("none"));
        assert_eq!(health_table("nosuch"), None);
        assert_eq!(
            remap("http://127.0.0.1:8660/healthz-self"),
            "http://127.0.0.1:8660/status.json"
        );
        assert_eq!(
            rollback_health("galion-nerve", "http://127.0.0.1:19391/health"),
            "http://127.0.0.1:19391/healthz"
        );
        assert_eq!(
            rollback_health("galion-nerve", "http://127.0.0.1:8670/healthz"),
            "http://127.0.0.1:8670/healthz"
        );
        assert_eq!(
            rollback_health("tuner", "http://127.0.0.1:19390/health"),
            "http://127.0.0.1:19390/health"
        );
        assert_eq!(rollback_health("nosuch", "http://h:1/x"), "http://h:1/x");
        assert_eq!(rollback_health("tuner", "none"), "none");
        assert_eq!(
            rollback_health("link-watch-rs", "http://h:1/x"),
            "http://h:1/x"
        );
    }
}
