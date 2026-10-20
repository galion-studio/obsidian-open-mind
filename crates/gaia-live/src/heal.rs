// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Gaia M2: the first apply path. It is scoped to **test services on the
//! box**, and every guard here is a refusal, not a warning:
//!
//! * [`check_target`]: the service must listen on 127.0.0.1 on a port inside
//!   the operator-given range, and that range must sit inside
//!   [`TEST_PORTS`] (19000-19999). Live stack ports (8080, 8091, 8420, 8642,
//!   8650, 8670, ...) are therefore always refused. The start command must be
//!   an absolute path to a program that is not a service manager, shell,
//!   remote shell or killer, and it must name the target port.
//! * [`check_approval`]: a HADES heal-apply only runs with an approval file
//!   that names a registered Alpha, the action `heal-apply`, this exact
//!   service, and a test token equal (constant-time) to the run's trusted
//!   token. The token is never printed; only [`fingerprint`] is.
//! * [`cap_check`] and [`Ledger`]: at most [`MAX_RESTARTS_PER_HOUR`]
//!   restarts per service in any rolling hour, persisted on disk so separate
//!   ticks share the budget.
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
/// Shortest trusted test token, in hex characters (128 bits).
pub const MIN_TOKEN_HEX: usize = 32;

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

/// Refuse anything that is not a box-local test service.
pub fn check_target(t: &TestTarget, allowed: &RangeInclusive<u16>) -> Result<(), String> {
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
        || !TEST_PORTS.contains(allowed.start())
        || !TEST_PORTS.contains(allowed.end())
    {
        return Err(format!(
            "allowed range {}-{} is outside the test range {}-{}",
            allowed.start(),
            allowed.end(),
            TEST_PORTS.start(),
            TEST_PORTS.end()
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

// ---------------------------------------------------------------- approval

/// One Alpha's approval, as read from an approval file:
/// `alpha=ID action=heal-apply service=NAME token=HEX` (one line).
#[derive(Clone, PartialEq, Eq)]
pub struct Approval {
    pub alpha: String,
    pub action: String,
    pub service: String,
    token: String,
}

impl std::fmt::Debug for Approval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Approval")
            .field("alpha", &self.alpha)
            .field("action", &self.action)
            .field("service", &self.service)
            .field("token_fp", &fingerprint(&self.token))
            .finish()
    }
}

impl Approval {
    pub fn new(alpha: &str, action: &str, service: &str, token: &str) -> Approval {
        Approval {
            alpha: alpha.into(),
            action: action.into(),
            service: service.into(),
            token: token.into(),
        }
    }

    pub fn parse(text: &str) -> Result<Approval, String> {
        let line = text
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#'))
            .ok_or("empty approval file")?;
        let mut a = Approval::new("", "", "", "");
        for kv in line.split_whitespace() {
            let (k, v) = kv.split_once('=').ok_or("approval fields are key=value")?;
            match k {
                "alpha" => a.alpha = v.into(),
                "action" => a.action = v.into(),
                "service" => a.service = v.into(),
                "token" => a.token = v.into(),
                _ => return Err(format!("unknown approval field {k:?}")),
            }
        }
        if a.alpha.is_empty() || a.action.is_empty() || a.service.is_empty() || a.token.is_empty() {
            return Err("approval needs alpha, action, service and token".into());
        }
        Ok(a)
    }

    pub fn token_fingerprint(&self) -> String {
        fingerprint(&self.token)
    }
}

/// Non-reversible short id of a token for logs (FNV-1a 64, first 8 hex).
pub fn fingerprint(token: &str) -> String {
    let h = gaia_core::fnv1a(token.as_bytes());
    format!("fnv:{:08x}", h >> 32)
}

/// Read the run's trusted test token. It must be at least
/// [`MIN_TOKEN_HEX`] hex characters and (on unix) not readable by group or
/// others.
pub fn load_trusted_token(path: &Path) -> Result<String, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = fs::metadata(path)
            .map_err(|e| format!("trust file {}: {e}", path.display()))?
            .permissions()
            .mode();
        if m & 0o077 != 0 {
            return Err(format!(
                "trust file {} is readable by group/others (mode {:o}); chmod 600",
                path.display(),
                m & 0o777
            ));
        }
    }
    let t = fs::read_to_string(path)
        .map_err(|e| format!("trust file {}: {e}", path.display()))?
        .trim()
        .to_string();
    if t.len() < MIN_TOKEN_HEX || !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "trusted test token must be at least {MIN_TOKEN_HEX} hex characters"
        ));
    }
    Ok(t)
}

/// Constant-time equality (length is not secret).
pub fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Everything except the Alpha registry (GAIA core checks that, and binds
/// the approval to the directive id and target).
pub fn check_approval(a: &Approval, trusted: &str, service: &str) -> Result<(), String> {
    if a.action != "heal-apply" {
        return Err(format!("approval is for {:?}, not heal-apply", a.action));
    }
    if a.service != service {
        return Err(format!(
            "approval is for service {:?}, not {service:?}",
            a.service
        ));
    }
    if !ct_eq(&a.token, trusted) {
        return Err(format!(
            "approval token {} does not match the trusted test token",
            a.token_fingerprint()
        ));
    }
    Ok(())
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

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Restart times (epoch seconds) recorded for `service`.
    pub fn history(&self, service: &str) -> io::Result<Vec<u64>> {
        let text = match fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e),
        };
        let mut out = Vec::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let v: serde_json::Value = serde_json::from_str(line)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            if v.get("service").and_then(|s| s.as_str()) == Some(service) {
                if let Some(t) = v.get("epoch_s").and_then(|t| t.as_u64()) {
                    out.push(t);
                }
            }
        }
        Ok(out)
    }

    pub fn record(&self, service: &str, epoch_s: u64, pid: u32) -> io::Result<()> {
        let line = serde_json::json!({
            "service": service,
            "epoch_s": epoch_s,
            "ts": zagreb_time::from_unix(epoch_s as i64),
            "pid": pid,
        });
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(f, "{line}")
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
    Ok(child.id())
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
