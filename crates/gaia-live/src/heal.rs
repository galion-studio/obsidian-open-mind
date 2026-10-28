// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Gaia M2: the first apply path. It is scoped to **test services on the
//! box**, and every guard here is a refusal, not a warning:
//!
//! * [`check_target`]: the service must listen on 127.0.0.1 on a port inside
//!   the operator-given range, and that range must sit inside
//!   [`TEST_PORTS`] (19000-19999; real tests only may move it inside
//!   19000-22999 with `GAIA_TEST_PORTS`, see [`test_ports`]). Live stack ports (8080, 8091, 8420, 8642,
//!   8650, 8670, ...) are therefore always refused. The start command must be
//!   an absolute path to a program that is not a service manager, shell,
//!   remote shell or killer, and it must name the target port.
//! * Approval is NOT here any more (2026-10-20, gaia-hades-tokens): the old
//!   per-run random test token is gone. `gaia heal-tick` asks hades-gate
//!   (`hades_gate::adapter::check_gaia`) for an Ed25519-signed, expiring,
//!   single-use token scoped to `heal-apply`, this service and the exact plan
//!   hash, consumed in hades-gate's fsynced replay ledger.
//! * [`cap_check`] and [`Ledger`]: at most [`MAX_RESTARTS_PER_HOUR`]
//!   restarts per service in any rolling hour, persisted on disk so separate
//!   ticks share the budget. Since Gaia S3.1 ([`Budget`]) the budget is the
//!   `--state` ledger plus a global ledger next to hades-gate's replay
//!   ledger, so a new `--state` dir cannot reset it, with an OS file lock
//!   around check-and-record.
//! * [`spawn_detached`]: starts the test service in its own process group
//!   with stdin closed and output in a log file. It never signals, stops or
//!   restarts anything else, and never calls systemctl.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The only ports the apply path may ever touch.
pub const TEST_PORTS: RangeInclusive<u16> = 19000..=19999;
/// Restarts allowed per service per rolling hour.
pub const MAX_RESTARTS_PER_HOUR: usize = 3;
/// Rolling window for [`MAX_RESTARTS_PER_HOUR`].
pub const WINDOW_S: u64 = 3600;

/// Programs the apply path refuses to start, whatever the path prefix.
pub const FORBIDDEN_PROGRAMS: &[&str] = &[
    "systemctl",
    "service",
    "init",
    "telinit",
    "shutdown",
    "reboot",
    "kill",
    "pkill",
    "killall",
    "sh",
    "bash",
    "dash",
    "zsh",
    "env",
    "sudo",
    "su",
    "doas",
    "ssh",
    "scp",
    "rsync",
    "nohup",
    "setsid",
    "rm",
    "dd",
    "python",
    "python3",
    "pip",
    "pip3",
];

/// A test service on the box that Gaia may restart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestTarget {
    /// Short name (`[a-z0-9-]`, 1..=48).
    pub name: String,
    /// Loopback port; must be inside the allowed range.
    pub port: u16,
    /// Health path probed with `GET` (expects JSON `status: ok`).
    pub health_path: String,
    /// Start command: absolute program path plus arguments (no shell).
    pub argv: Vec<String>,
}

/// Env var for a TEST-ONLY override of [`TEST_PORTS`] (slot 423, D-383-2 b).
/// Value `"LO-HI"` (or one port). Honoured only when `GALION_REAL=1` (the
/// real-test switch) and only inside [`TEST_PORTS_OVERRIDE_BOUNDS`]; it
/// replaces the box test range, it never adds live stack ports.
pub const TEST_PORTS_ENV: &str = "GAIA_TEST_PORTS";
/// The swarm slot port ranges on the box (19000-22999). A
/// [`TEST_PORTS_ENV`] override outside this is refused. Every live stack
/// port (8080, 8091, 8420, 8642, 8650, 8670, ...) is below it.
pub const TEST_PORTS_OVERRIDE_BOUNDS: RangeInclusive<u16> = 19000..=22999;

/// The box test range in force, from explicit inputs (pure, for tests):
/// `value` is the [`TEST_PORTS_ENV`] value, `real` whether `GALION_REAL=1`.
///
/// * unset or empty: [`TEST_PORTS`] (production default, unchanged);
/// * set without `real`: refused (a stray override never reaches a
///   production heal-tick; fail closed, nothing is restarted);
/// * set with `real`: must parse as `LO-HI` with `LO <= HI`, both inside
///   [`TEST_PORTS_OVERRIDE_BOUNDS`], else refused.
pub fn test_ports_from(value: Option<&str>, real: bool) -> Result<RangeInclusive<u16>, String> {
    let v = match value.map(str::trim) {
        None | Some("") => return Ok(TEST_PORTS),
        Some(v) => v,
    };
    if !real {
        return Err(format!(
            "{TEST_PORTS_ENV}={v:?} is a real-test override and needs GALION_REAL=1; unset it (production range {}-{})",
            TEST_PORTS.start(),
            TEST_PORTS.end()
        ));
    }
    let r = parse_range(v).map_err(|e| format!("{TEST_PORTS_ENV}: {e}"))?;
    if r.is_empty()
        || !TEST_PORTS_OVERRIDE_BOUNDS.contains(r.start())
        || !TEST_PORTS_OVERRIDE_BOUNDS.contains(r.end())
    {
        return Err(format!(
            "{TEST_PORTS_ENV}={v:?} is outside the override bounds {}-{}",
            TEST_PORTS_OVERRIDE_BOUNDS.start(),
            TEST_PORTS_OVERRIDE_BOUNDS.end()
        ));
    }
    Ok(r)
}

/// [`test_ports_from`] with the process environment.
pub fn test_ports() -> Result<RangeInclusive<u16>, String> {
    let v = std::env::var(TEST_PORTS_ENV).ok();
    let real = std::env::var("GALION_REAL").as_deref() == Ok("1");
    test_ports_from(v.as_deref(), real)
}

/// Refuse anything that is not a box-local test service. The box test range
/// is [`test_ports`] (default [`TEST_PORTS`]); see [`check_target_in`].
pub fn check_target(t: &TestTarget, allowed: &RangeInclusive<u16>) -> Result<(), String> {
    check_target_in(t, allowed, &test_ports()?)
}

/// [`check_target`] against an explicit box test range (pure, for tests).
pub fn check_target_in(
    t: &TestTarget,
    allowed: &RangeInclusive<u16>,
    test_ports: &RangeInclusive<u16>,
) -> Result<(), String> {
    if test_ports.is_empty()
        || !TEST_PORTS_OVERRIDE_BOUNDS.contains(test_ports.start())
        || !TEST_PORTS_OVERRIDE_BOUNDS.contains(test_ports.end())
    {
        return Err(format!(
            "test range {}-{} is outside the override bounds {}-{}",
            test_ports.start(),
            test_ports.end(),
            TEST_PORTS_OVERRIDE_BOUNDS.start(),
            TEST_PORTS_OVERRIDE_BOUNDS.end()
        ));
    }
    if t.name.is_empty()
        || t.name.len() > 48
        || !t
            .name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(format!(
            "service name {:?} is not [a-z0-9-]{{1,48}}",
            t.name
        ));
    }
    if allowed.is_empty()
        || !test_ports.contains(allowed.start())
        || !test_ports.contains(allowed.end())
    {
        return Err(format!(
            "allowed range {}-{} is outside the test range {}-{}",
            allowed.start(),
            allowed.end(),
            test_ports.start(),
            test_ports.end()
        ));
    }
    if !allowed.contains(&t.port) {
        return Err(format!(
            "port {} is outside the allowed test range {}-{}",
            t.port,
            allowed.start(),
            allowed.end()
        ));
    }
    if !t.health_path.starts_with('/') || t.health_path.chars().any(|c| c.is_whitespace()) {
        return Err(format!(
            "health path {:?} is not an absolute URL path",
            t.health_path
        ));
    }
    let Some(prog) = t.argv.first() else {
        return Err("empty start command".into());
    };
    let p = Path::new(prog);
    if !p.is_absolute() {
        return Err(format!("start program {prog:?} must be an absolute path"));
    }
    let base = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let stem = base.split('.').next().unwrap_or(base);
    if FORBIDDEN_PROGRAMS.contains(&base) || FORBIDDEN_PROGRAMS.contains(&stem) {
        return Err(format!(
            "start program {base:?} is forbidden on the apply path"
        ));
    }
    if !p.is_file() {
        return Err(format!("start program {prog:?} does not exist"));
    }
    let port = t.port.to_string();
    if !t.argv[1..]
        .iter()
        .any(|a| a == &port || a.ends_with(&format!(":{port}")))
    {
        return Err(format!(
            "start command does not name the target port {port}; refusing to start something else"
        ));
    }
    Ok(())
}

/// Parse `"19030-19039"` or `"19032"`.
pub fn parse_range(s: &str) -> Result<RangeInclusive<u16>, String> {
    let (a, b) = s.split_once('-').unwrap_or((s, s));
    let a: u16 = a
        .trim()
        .parse()
        .map_err(|_| format!("bad port range {s:?}"))?;
    let b: u16 = b
        .trim()
        .parse()
        .map_err(|_| format!("bad port range {s:?}"))?;
    Ok(a..=b)
}

// ----------------------------------------------------------- restart cap

pub fn epoch_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `Ok(used)` when one more restart fits, `Err(retry_after_s)` when the
/// rolling-hour budget is spent. `history` is restart times (epoch seconds).
pub fn cap_check(history: &[u64], now: u64) -> Result<usize, u64> {
    let mut recent: Vec<u64> = history
        .iter()
        .copied()
        .filter(|t| *t <= now && now - *t < WINDOW_S)
        .collect();
    recent.sort_unstable();
    if recent.len() < MAX_RESTARTS_PER_HOUR {
        return Ok(recent.len());
    }
    let oldest = recent[recent.len() - MAX_RESTARTS_PER_HOUR];
    Err(WINDOW_S - (now - oldest))
}

/// Restart history on disk: `<state>/restarts.jsonl`, one line per restart.
#[derive(Clone, Debug)]
pub struct Ledger {
    path: PathBuf,
}

impl Ledger {
    pub fn new(state_dir: &Path) -> Ledger {
        Ledger {
            path: state_dir.join("restarts.jsonl"),
        }
    }

    /// A ledger at an explicit file path (the global ledger).
    pub fn at(path: PathBuf) -> Ledger {
        Ledger { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Restart times (epoch seconds) recorded for `service`.
    pub fn history(&self, service: &str) -> io::Result<Vec<u64>> {
        Ok(self
            .entries(service)?
            .into_iter()
            .map(|r| r.epoch_s)
            .collect())
    }

    /// Restart records (time and pid) for `service`. A line that is not JSON
    /// (corrupt or torn) is an error, so callers fail closed.
    pub fn entries(&self, service: &str) -> io::Result<Vec<Restart>> {
        let text = match fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e),
        };
        let mut out = Vec::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let v: serde_json::Value = serde_json::from_str(line).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}: {e}", self.path.display()),
                )
            })?;
            if v.get("service").and_then(|s| s.as_str()) == Some(service) {
                if let Some(t) = v.get("epoch_s").and_then(|t| t.as_u64()) {
                    out.push(Restart {
                        epoch_s: t,
                        pid: v
                            .get("pid")
                            .and_then(|p| p.as_u64())
                            .and_then(|p| u32::try_from(p).ok()),
                    });
                }
            }
        }
        Ok(out)
    }

    fn line(service: &str, epoch_s: u64, pid: u32) -> String {
        serde_json::json!({
            "service": service,
            "epoch_s": epoch_s,
            "ts": zagreb_time::from_unix(epoch_s as i64),
            "pid": pid,
        })
        .to_string()
    }

    pub fn record(&self, service: &str, epoch_s: u64, pid: u32) -> io::Result<()> {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(f, "{}", Self::line(service, epoch_s, pid))
    }

    /// [`Ledger::record`] as one `write` of the whole line, then `fsync`;
    /// creates the parent directory if needed.
    pub fn record_synced(&self, service: &str, epoch_s: u64, pid: u32) -> io::Result<()> {
        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            fs::create_dir_all(dir)?;
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let mut line = Self::line(service, epoch_s, pid);
        line.push('\n');
        f.write_all(line.as_bytes())?;
        f.sync_data()
    }
}

// ----------------------------------------------------------------- apply

/// Start `argv` detached from the caller: own process group, stdin closed,
/// stdout and stderr appended to `log`. Returns the child pid. The child is
/// not waited for; it keeps running after Gaia exits.
pub fn spawn_detached(argv: &[String], log: &Path) -> io::Result<u32> {
    use std::os::unix::process::CommandExt;
    let (prog, args) = argv
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty argv"))?;
    let out = OpenOptions::new().create(true).append(true).open(log)?;
    let err = out.try_clone()?;
    let child = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .process_group(0)
        .spawn()?;
    let pid = child.id();
    if let Ok(mut kids) = CHILDREN.lock() {
        kids.push(child);
    }
    Ok(pid)
}

/// Children started by [`spawn_detached`] that may still need reaping.
static CHILDREN: std::sync::Mutex<Vec<std::process::Child>> = std::sync::Mutex::new(Vec::new());

/// Reap restarted test services that have exited, so a long-running loop
/// does not collect zombies (the one-shot command exits and init reaps).
/// Never signals anything; only `waitpid(WNOHANG)` on our own children.
/// Returns `(reaped now, still running)`.
pub fn reap() -> (usize, usize) {
    let Ok(mut kids) = CHILDREN.lock() else {
        return (0, 0);
    };
    let before = kids.len();
    kids.retain_mut(|c| matches!(c.try_wait(), Ok(None)));
    (before - kids.len(), kids.len())
}

/// Probe `t` until it is up or `timeout` passes (200 ms between tries).
pub fn wait_up(t: &TestTarget, timeout: Duration) -> (bool, u32) {
    let probe = test_probe(t);
    let o = crate::Opts {
        systemd: false,
        connect_timeout: Duration::from_millis(300),
        read_timeout: Duration::from_millis(600),
        ..crate::Opts::default()
    };
    let t0 = Instant::now();
    let mut tries = 0;
    loop {
        tries += 1;
        if crate::probe(&probe, &o).state == crate::State::Up {
            return (true, tries);
        }
        if t0.elapsed() >= timeout {
            return (false, tries);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The read-only probe for a test target (owned by DEMETER guardian-tick).
pub fn test_probe(t: &TestTarget) -> crate::Probe {
    crate::Probe {
        name: t.name.clone(),
        system: gaia_core::System::GuardianTick,
        http: Some(crate::HttpCheck {
            port: t.port,
            path: t.health_path.clone(),
            expect: crate::Expect::JsonOk,
        }),
        unit: None,
        fresh_file: None,
    }
}

// ------------------------------------------------- global restart budget

/// File name of the global restart ledger. It lives in the directory of
/// hades-gate's replay ledger (from the `--hades-config` file), not in
/// `--state`, so a new or empty `--state` directory cannot reset the
/// [`MAX_RESTARTS_PER_HOUR`] budget (gaia-live.md limit, Gaia S3.1).
pub const GLOBAL_LEDGER_FILE: &str = "gaia-restarts.jsonl";

/// Where the global restart ledger sits for a hades-gate replay ledger path.
#[must_use]
pub fn global_ledger_path(hades_replay_ledger: &Path) -> PathBuf {
    match hades_replay_ledger.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.join(GLOBAL_LEDGER_FILE),
        _ => PathBuf::from(GLOBAL_LEDGER_FILE),
    }
}

/// One restart record: when (epoch seconds) and which pid was started.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Restart {
    pub epoch_s: u64,
    pub pid: Option<u32>,
}

/// The restart budget seen by one tick: the per-`--state` ledger and the
/// global ledger together. A restart counts once even when both files have
/// it (same epoch and pid). Records go to the global ledger first (fsynced),
/// then to the state ledger. [`Budget::lock`] serialises the check-and-record
/// window between concurrent ticks and loops (an OS file lock on
/// `<global>.lock`; released when the returned file is dropped or the
/// process dies).
#[derive(Clone, Debug)]
pub struct Budget {
    state: Ledger,
    global: Ledger,
}

impl Budget {
    #[must_use]
    pub fn new(state_dir: &Path, global_path: &Path) -> Budget {
        Budget {
            state: Ledger::new(state_dir),
            global: Ledger::at(global_path.to_path_buf()),
        }
    }

    #[must_use]
    pub fn global_path(&self) -> &Path {
        self.global.path()
    }

    #[must_use]
    pub fn state_path(&self) -> &Path {
        self.state.path()
    }

    /// Restart times of `service` from both ledgers, de-duplicated. An
    /// unreadable or corrupt ledger is an error (the caller fails closed).
    pub fn history(&self, service: &str) -> io::Result<Vec<u64>> {
        let mut all = self.global.entries(service)?;
        all.extend(self.state.entries(service)?);
        all.sort_unstable();
        all.dedup();
        Ok(all.into_iter().map(|r| r.epoch_s).collect())
    }

    /// Exclusive lock for the check-then-record window. Blocks until free.
    pub fn lock(&self) -> io::Result<fs::File> {
        let mut p = self.global.path().as_os_str().to_owned();
        p.push(".lock");
        let lock_path = PathBuf::from(p);
        if let Some(dir) = lock_path.parent().filter(|d| !d.as_os_str().is_empty()) {
            fs::create_dir_all(dir)?;
        }
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)?;
        f.lock()?;
        Ok(f)
    }

    /// Record one restart in the global ledger (fsynced) and the state one.
    pub fn record(&self, service: &str, epoch_s: u64, pid: u32) -> io::Result<()> {
        self.global.record_synced(service, epoch_s, pid)?;
        self.state.record(service, epoch_s, pid)
    }
}
