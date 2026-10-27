// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The hard admin-safety rule.
//!
//! HADES must never lock out, harm or expose the admin. [`review`] runs on the
//! action, the target and the exact plan text **before** any token is looked
//! at, and a refusal here cannot be overridden by any token, valid or not.
//!
//! It refuses:
//! * destructive commands (`rm -rf`, `mkfs`, `dd`, `shred`, `DROP TABLE`, force pushes, ...);
//! * anything that could expose a secret (secret paths, private keys, `printenv`, ...);
//! * admin account, login and access changes (`passwd`, `usermod`, `sudoers`,
//!   `authorized_keys`, `sshd`, firewall rules, permission changes, reboots);
//! * targets that are the admin's access path or HADES itself, so an approval
//!   can never be used to switch off the admin's ability to stop HADES;
//! * plans the reviewer cannot read: non-ASCII text, shell substitution, `eval`,
//!   encoded payloads.
//!
//! Two layers, both must pass. The deny-list (specific reasons) runs first;
//! the guard is the **allow-list** of plan steps ([`OPERATOR_SCRIPTS`],
//! [`SERVICE_PREFIXES`], one template per step, a validator per argument):
//! a line that is not an allow-listed step is refused. On top of that only
//! `heal-apply`, `switch` and `rollback` can be approved at all. Deliberately
//! over-cautious: a false refusal costs the admin one manual step, a false
//! approval could cost them their box. The S3.1 adversarial corpus
//! (`tests/attacks/mod.rs`) found 81 deny-list bypasses in 138 attacks; all are
//! refused now (docs/real-tests/hades-adversarial.md).

/// Longest plan the gate will review.
pub const MAX_PLAN_BYTES: usize = 64 * 1024;

/// Why the safety rule refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hazard {
    /// Deletes, wipes or overwrites data or history.
    Destructive(&'static str),
    /// Could print, copy or move a secret.
    SecretExposure(&'static str),
    /// Changes the admin's account, login, keys, sudo, firewall or permissions.
    AdminAccount(&'static str),
    /// Targets the admin access path or HADES itself.
    ProtectedTarget(&'static str),
    /// The plan cannot be reviewed as plain text.
    Opaque(&'static str),
    /// Not on the allow-list of plan steps (or a bad argument to one).
    NotAllowListed(&'static str),
}

impl Hazard {
    /// Human-readable refusal reason.
    #[must_use]
    pub fn reason(self) -> String {
        match self {
            Self::Destructive(m) => format!("admin-safety: destructive command refused ({m})"),
            Self::SecretExposure(m) => format!("admin-safety: secret exposure refused ({m})"),
            Self::AdminAccount(m) => {
                format!("admin-safety: admin account/access change refused ({m})")
            }
            Self::ProtectedTarget(m) => format!("admin-safety: protected target refused ({m})"),
            Self::Opaque(m) => format!("admin-safety: plan cannot be reviewed ({m})"),
            Self::NotAllowListed(m) => {
                format!("admin-safety: not an allow-listed plan step ({m})")
            }
        }
    }
}

/// Multi-word or punctuation patterns, matched on the lower-cased plan with
/// runs of whitespace folded to one space.
const PHRASES: &[(&str, Hazard)] = &[
    ("rm -r", Hazard::Destructive("rm -r")),
    ("rm -f", Hazard::Destructive("rm -f")),
    ("rm --recursive", Hazard::Destructive("rm --recursive")),
    (
        "--no-preserve-root",
        Hazard::Destructive("--no-preserve-root"),
    ),
    ("dd if=", Hazard::Destructive("dd")),
    ("dd of=", Hazard::Destructive("dd")),
    ("> /dev/sd", Hazard::Destructive("write to a disk device")),
    (">/dev/sd", Hazard::Destructive("write to a disk device")),
    ("> /dev/nvme", Hazard::Destructive("write to a disk device")),
    (">/dev/nvme", Hazard::Destructive("write to a disk device")),
    (
        "> /dev/mmcblk",
        Hazard::Destructive("write to a disk device"),
    ),
    (
        ">/dev/mmcblk",
        Hazard::Destructive("write to a disk device"),
    ),
    ("> /dev/vd", Hazard::Destructive("write to a disk device")),
    (">/dev/vd", Hazard::Destructive("write to a disk device")),
    ("drop database", Hazard::Destructive("drop database")),
    ("drop table", Hazard::Destructive("drop table")),
    ("delete from", Hazard::Destructive("delete from")),
    ("truncate table", Hazard::Destructive("truncate table")),
    ("push --force", Hazard::Destructive("force push")),
    ("push -f", Hazard::Destructive("force push")),
    ("push --mirror", Hazard::Destructive("mirror push")),
    ("reset --hard", Hazard::Destructive("git reset --hard")),
    ("clean -fd", Hazard::Destructive("git clean")),
    ("branch -d", Hazard::Destructive("branch delete")),
    ("zpool destroy", Hazard::Destructive("zpool destroy")),
    ("kill -9 -1", Hazard::AdminAccount("kill every process")),
    ("pkill -u", Hazard::AdminAccount("kill a user's sessions")),
    (
        "loginctl terminate",
        Hazard::AdminAccount("terminate sessions"),
    ),
    ("/etc/shadow", Hazard::AdminAccount("/etc/shadow")),
    ("/etc/passwd", Hazard::AdminAccount("/etc/passwd")),
    ("/etc/group", Hazard::AdminAccount("/etc/group")),
    ("/etc/pam", Hazard::AdminAccount("pam")),
    ("/etc/ssh", Hazard::AdminAccount("ssh config")),
    ("authorized_keys", Hazard::AdminAccount("authorized_keys")),
    ("sshd_config", Hazard::AdminAccount("sshd_config")),
    ("sudoers", Hazard::AdminAccount("sudoers")),
    (
        "galion-control",
        Hazard::SecretExposure("galion-control key"),
    ),
    ("galion-secrets", Hazard::SecretExposure("secrets dir")),
    ("/secrets", Hazard::SecretExposure("secrets dir")),
    ("secrets/", Hazard::SecretExposure("secrets dir")),
    ("/persist", Hazard::SecretExposure("persist dir")),
    ("/environ", Hazard::SecretExposure("process environment")),
    ("private key", Hazard::SecretExposure("private key")),
    ("begin openssh", Hazard::SecretExposure("private key")),
    ("begin rsa", Hazard::SecretExposure("private key")),
    ("begin ec", Hazard::SecretExposure("private key")),
    ("id_rsa", Hazard::SecretExposure("ssh key")),
    ("id_ed25519", Hazard::SecretExposure("ssh key")),
    ("id_ecdsa", Hazard::SecretExposure("ssh key")),
    (".ssh/", Hazard::SecretExposure("ssh dir")),
    (".env", Hazard::SecretExposure(".env file")),
    (".netrc", Hazard::SecretExposure(".netrc")),
    (
        ".git-credentials",
        Hazard::SecretExposure("git credentials"),
    ),
    ("api_key", Hazard::SecretExposure("api key")),
    ("api-key", Hazard::SecretExposure("api key")),
    ("$(", Hazard::Opaque("command substitution")),
    ("`", Hazard::Opaque("command substitution")),
    ("<(", Hazard::Opaque("process substitution")),
    ("\\x", Hazard::Opaque("escaped bytes")),
    (":(){", Hazard::Destructive("fork bomb")),
];

/// Whole words (split on anything that is not `a-z 0-9 _ -`).
const WORDS: &[(&str, Hazard)] = &[
    ("mkfs", Hazard::Destructive("mkfs")),
    ("wipefs", Hazard::Destructive("wipefs")),
    ("shred", Hazard::Destructive("shred")),
    ("fdisk", Hazard::Destructive("fdisk")),
    ("sfdisk", Hazard::Destructive("sfdisk")),
    ("parted", Hazard::Destructive("parted")),
    ("blkdiscard", Hazard::Destructive("blkdiscard")),
    ("lvremove", Hazard::Destructive("lvremove")),
    ("vgremove", Hazard::Destructive("vgremove")),
    ("rmdir", Hazard::Destructive("rmdir")),
    ("unlink", Hazard::Destructive("unlink")),
    ("truncate", Hazard::Destructive("truncate")),
    ("dropdb", Hazard::Destructive("dropdb")),
    ("filter-branch", Hazard::Destructive("history rewrite")),
    ("filter-repo", Hazard::Destructive("history rewrite")),
    ("useradd", Hazard::AdminAccount("useradd")),
    ("userdel", Hazard::AdminAccount("userdel")),
    ("usermod", Hazard::AdminAccount("usermod")),
    ("adduser", Hazard::AdminAccount("adduser")),
    ("deluser", Hazard::AdminAccount("deluser")),
    ("groupmod", Hazard::AdminAccount("groupmod")),
    ("gpasswd", Hazard::AdminAccount("gpasswd")),
    ("passwd", Hazard::AdminAccount("passwd")),
    ("chpasswd", Hazard::AdminAccount("chpasswd")),
    ("chage", Hazard::AdminAccount("chage")),
    ("chsh", Hazard::AdminAccount("chsh")),
    ("visudo", Hazard::AdminAccount("visudo")),
    ("sshd", Hazard::AdminAccount("sshd")),
    ("ssh", Hazard::AdminAccount("ssh")),
    ("iptables", Hazard::AdminAccount("firewall")),
    ("ip6tables", Hazard::AdminAccount("firewall")),
    ("nft", Hazard::AdminAccount("firewall")),
    ("ufw", Hazard::AdminAccount("firewall")),
    ("firewall-cmd", Hazard::AdminAccount("firewall")),
    ("chmod", Hazard::AdminAccount("permission change")),
    ("chown", Hazard::AdminAccount("ownership change")),
    ("chgrp", Hazard::AdminAccount("ownership change")),
    ("chattr", Hazard::AdminAccount("attribute change")),
    ("setfacl", Hazard::AdminAccount("acl change")),
    ("shutdown", Hazard::AdminAccount("shutdown")),
    ("reboot", Hazard::AdminAccount("reboot")),
    ("poweroff", Hazard::AdminAccount("poweroff")),
    ("halt", Hazard::AdminAccount("halt")),
    ("killall", Hazard::AdminAccount("killall")),
    ("crontab", Hazard::AdminAccount("crontab")),
    ("printenv", Hazard::SecretExposure("printenv")),
    ("env", Hazard::SecretExposure("environment dump")),
    ("export", Hazard::SecretExposure("environment")),
    ("secret", Hazard::SecretExposure("secret")),
    ("secrets", Hazard::SecretExposure("secret")),
    ("password", Hazard::SecretExposure("password")),
    ("passphrase", Hazard::SecretExposure("passphrase")),
    ("token", Hazard::SecretExposure("token")),
    ("credentials", Hazard::SecretExposure("credentials")),
    ("eval", Hazard::Opaque("eval")),
    ("base64", Hazard::Opaque("encoded payload")),
    ("xxd", Hazard::Opaque("encoded payload")),
    ("openssl", Hazard::Opaque("openssl")),
];

/// Target names (or parts of them) that are never approvable.
const PROTECTED_TARGETS: &[&str] = &[
    "hades",
    "admin",
    "root",
    "ssh",
    "sshd",
    "sudo",
    "login",
    "pam",
    "user",
    "account",
    "firewall",
    "iptables",
    "nftables",
    "ufw",
    "secret",
    "secrets",
    "persist",
    "key",
    "keys",
    "token",
    "credential",
    "credentials",
    "galion-control",
    "systemd-logind",
];

/// Check the target and plan against the admin-safety rule.
///
/// # Errors
/// The first [`Hazard`] found.
pub fn review(target: &str, plan: &[u8]) -> Result<(), Hazard> {
    review_target(target)?;
    review_plan(plan)
}

/// Only the target name.
///
/// # Errors
/// [`Hazard::ProtectedTarget`].
pub fn review_target(target: &str) -> Result<(), Hazard> {
    let lower = target.to_ascii_lowercase();
    for part in lower.split(|c: char| !c.is_ascii_alphanumeric()) {
        if let Some(hit) = PROTECTED_TARGETS.iter().find(|p| **p == part) {
            return Err(Hazard::ProtectedTarget(hit));
        }
    }
    for multi in PROTECTED_TARGETS.iter().filter(|p| p.contains('-')) {
        if lower.contains(multi) {
            return Err(Hazard::ProtectedTarget(multi));
        }
    }
    // Structure: lower-case [a-z0-9] parts joined by `.` `_` `-`; no `..`,
    // no `:` port, no `@` user, no leading or trailing punctuation.
    let shape = target.split(['.', '_', '-']).all(|p| {
        !p.is_empty()
            && p.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    });
    if !shape {
        return Err(Hazard::NotAllowListed("target name"));
    }
    Ok(())
}

/// Only the plan text.
///
/// # Errors
/// The first [`Hazard`] found.
pub fn review_plan(plan: &[u8]) -> Result<(), Hazard> {
    if plan.is_empty() {
        return Err(Hazard::Opaque("empty plan"));
    }
    if plan.len() > MAX_PLAN_BYTES {
        return Err(Hazard::Opaque("plan too long"));
    }
    if !plan
        .iter()
        .all(|b| matches!(b, b'\n' | b'\t') || (0x20..0x7f).contains(b))
    {
        return Err(Hazard::Opaque("non-printable or non-ASCII bytes"));
    }
    let text = String::from_utf8_lossy(plan).to_ascii_lowercase();
    let folded = text.split_whitespace().collect::<Vec<_>>().join(" ");
    // `kill ... -1` signals every process the caller can reach.
    let args: Vec<&str> = folded.split(' ').collect();
    if args.contains(&"kill") && args.contains(&"-1") {
        return Err(Hazard::AdminAccount("kill every process"));
    }
    for (pat, hazard) in PHRASES {
        if folded.contains(pat) {
            return Err(*hazard);
        }
    }
    for word in folded.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-')) {
        let word = word.trim_matches('-');
        if let Some((_, hazard)) = WORDS.iter().find(|(w, _)| *w == word) {
            return Err(*hazard);
        }
    }
    review_allow_list(&String::from_utf8_lossy(plan))
}

// ---------------------------------------------------------------------------
// Allow-list (security plan S3.1, 2026-10-20). The deny-list above is kept as
// a first layer because its reasons are specific, but it is NOT the guard:
// 81 of 138 adversarial plans got past it with a valid token
// (docs/real-tests/hades-adversarial.md). The guard is this allow-list. Every
// non-empty plan line must be one of the step templates below, and every
// argument slot has its own validator. A line that matches no template is
// refused, so an unknown command can never be approved.
// ---------------------------------------------------------------------------

/// Services a plan may name: ours only, by name prefix.
pub const SERVICE_PREFIXES: &[&str] = &["galion-", "gaia-", "openmind-", "hermes-"];

/// The operator scripts a plan may name, exactly (paths relative to the repo).
pub const OPERATOR_SCRIPTS: &[&str] = &[
    "deploy/rollback.sh",
    "deploy/rollback-console.sh",
    "deploy/switch.sh",
    "deploy/switch-console.sh",
];

/// Box test ports for `gaia heal-tick` (`gaia_live::heal::TEST_PORTS`).
const TEST_PORTS: std::ops::RangeInclusive<u16> = 19000..=19999;

/// Fixed tail of the `start [...]` step in the gaia heal-apply plan v1.
const START_TAIL: &str = " detached in its own process group, stdin closed, no shell";

/// Absolute-path arguments of a started program may not point here.
const SYSTEM_ROOTS: &[&str] = &[
    "/etc",
    "/boot",
    "/dev",
    "/proc",
    "/sys",
    "/root",
    "/bin",
    "/sbin",
    "/usr",
    "/lib",
    "/lib64",
    "/home",
    "/run",
    "/var/spool",
    "/var/log",
];

/// Bytes allowed in a plan line other than the `start [...]` step.
fn plain_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b' ' | b'\t' | b'.' | b',' | b':' | b'/' | b'_' | b'-' | b'(' | b')'
        )
}

/// Bytes allowed inside one argv string of the `start [...]` step.
fn arg_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b':' | b'=' | b'-')
}

const fn bad(why: &'static str) -> Hazard {
    Hazard::NotAllowListed(why)
}

/// One of our services: `galion-tuner`, `gaia-dummy`, ... and not protected.
fn service(s: &str) -> Result<(), Hazard> {
    let shape = !s.is_empty()
        && s.len() <= 48
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.ends_with('-')
        && !s.contains("--")
        && SERVICE_PREFIXES
            .iter()
            .any(|p| s.len() > p.len() && s.starts_with(p));
    if !shape {
        return Err(bad(
            "service name (ours only: galion-*, gaia-*, openmind-*, hermes-*)",
        ));
    }
    review_target(s)
}

/// An HTTP path for a read-only probe: `/health`, `/v1/status`, ...
fn endpoint(p: &str) -> Result<(), Hazard> {
    let Some(rest) = p.strip_prefix('/') else {
        return Err(bad("endpoint must be an absolute URL path"));
    };
    let ok = p.len() <= 128
        && !rest.is_empty()
        && rest.split('/').all(|c| {
            !c.is_empty()
                && c != "."
                && c != ".."
                && c.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'.')
                })
        });
    if ok {
        Ok(())
    } else {
        Err(bad("endpoint path"))
    }
}

fn number(s: &str, range: std::ops::RangeInclusive<u32>) -> Result<u32, Hazard> {
    if s.is_empty() || s.len() > 5 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad("number"));
    }
    s.parse::<u32>()
        .ok()
        .filter(|n| range.contains(n))
        .ok_or(bad("number out of range"))
}

/// `127.0.0.1:PORT` or `localhost:PORT`; returns the port.
fn loopback(h: &str) -> Result<u16, Hazard> {
    let port = h
        .strip_prefix("127.0.0.1:")
        .or_else(|| h.strip_prefix("localhost:"))
        .ok_or(bad("host must be 127.0.0.1:PORT or localhost:PORT"))?;
    let n = number(port, 1..=65535)?;
    u16::try_from(n).map_err(|_| bad("port"))
}

/// `2026-10-20` or `2026-10-20-abc123`.
fn release_id(s: &str) -> Result<(), Hazard> {
    let b = s.as_bytes();
    let date = b.len() >= 10
        && b[..10].iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        });
    let suffix = match &b[10.min(b.len())..] {
        [] => true,
        [b'-', rest @ ..] => {
            !rest.is_empty()
                && rest.len() <= 16
                && rest
                    .iter()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        }
        _ => false,
    };
    if date && suffix {
        Ok(())
    } else {
        Err(bad("release id (YYYY-MM-DD[-tag])"))
    }
}

fn script(s: &str) -> Result<(), Hazard> {
    if OPERATOR_SCRIPTS.contains(&s) {
        Ok(())
    } else {
        Err(bad("operator script (exact allow-list only)"))
    }
}

/// `(box test ports 19000-19999 only)` range, inside the test ports.
fn port_range(s: &str) -> Result<std::ops::RangeInclusive<u16>, Hazard> {
    let (a, b) = s.split_once('-').ok_or(bad("port range"))?;
    let (a, b) = (number(a, 1..=65535)?, number(b, 1..=65535)?);
    let (a, b) = (
        u16::try_from(a).map_err(|_| bad("port"))?,
        u16::try_from(b).map_err(|_| bad("port"))?,
    );
    if a <= b && TEST_PORTS.contains(&a) && TEST_PORTS.contains(&b) {
        Ok(a..=b)
    } else {
        Err(bad(
            "port range must be inside the box test ports 19000-19999",
        ))
    }
}

/// An absolute path argument that does not reach system or admin dirs.
fn safe_abs_path(p: &str) -> Result<(), Hazard> {
    let ok = p.len() > 1
        && !p.ends_with('/')
        && !p.contains("//")
        && p[1..]
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != "..")
        && !SYSTEM_ROOTS
            .iter()
            .any(|r| p == *r || p.strip_prefix(r).is_some_and(|t| t.starts_with('/')));
    if ok {
        Ok(())
    } else {
        Err(bad("path argument (system dir, traversal or root)"))
    }
}

/// `start ["/abs/prog","--port","19231"] detached in its own process group,
/// stdin closed, no shell` (gaia heal-apply plan v1). The program's file name
/// must be one of our services; it is returned so the caller can require a
/// matching `service <name> ...` line.
fn start_step(line: &str) -> Result<String, Hazard> {
    let lower = line.to_ascii_lowercase();
    if !lower.starts_with("start [") || !lower.ends_with(START_TAIL) {
        return Err(bad(
            "start step must be: start [argv] detached ..., no shell",
        ));
    }
    let argv = &line["start ".len()..line.len() - START_TAIL.len()];
    let inner = argv
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or(bad("start argv must be a JSON array of strings"))?;
    let mut args = Vec::new();
    for item in inner.split(',') {
        let s = item
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .ok_or(bad("start argv must be a JSON array of plain strings"))?;
        if s.is_empty() || s.len() > 256 || !s.bytes().all(arg_byte) || s.contains("..") {
            return Err(bad("start argv string (plain bytes only, no traversal)"));
        }
        args.push(s);
    }
    if args.len() > 32 {
        return Err(bad("start argv too long"));
    }
    let (prog, rest) = args.split_first().ok_or(bad("start argv is empty"))?;
    if !prog.starts_with('/') {
        return Err(bad("start program must be an absolute path"));
    }
    safe_abs_path(prog)?;
    let name = prog.rsplit('/').next().unwrap_or_default();
    service(name)?;
    for arg in rest {
        let value = arg.split_once('=').map_or(*arg, |(_, v)| v);
        for p in [*arg, value] {
            if p.starts_with('/') {
                safe_abs_path(p)?;
            }
        }
    }
    Ok(name.to_owned())
}

/// What one line told us, for the cross-line check.
enum Step {
    Plain,
    /// `service X on ...` or `at most N restarts of X ...`.
    Names(String),
    /// `start [...]` with this program name.
    Starts(String),
}

fn step(line: &str) -> Result<Step, Hazard> {
    if line.len() >= 6 && line[..6].eq_ignore_ascii_case("start ") {
        return start_step(line).map(Step::Starts);
    }
    if !line.bytes().all(plain_byte) {
        return Err(bad("character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)"));
    }
    let lower = line.to_ascii_lowercase();
    let w: Vec<&str> = lower.split_whitespace().collect();
    match w.as_slice() {
        ["restart", svc] => service(svc)?,
        ["verify", ep] => endpoint(ep)?,
        ["verify", ep, "on", host] => {
            endpoint(ep)?;
            loopback(host)?;
        }
        ["verify", ep, "on", "the", "previous" | "current" | "new", "release"] => endpoint(ep)?,
        ["switch", "dist/bin", "to", "release", id] => release_id(id)?,
        ["switch", "dist/bin", "to", "the", "previous", "release"]
        | ["read", "the", "current", "and", "the", "previous", "release"]
        | ["keep", "the", "failed", "release", "for", "inspection"]
        | ["take", "the", "heal-dry", "findings", "from", "demeter"]
        | ["append", "every", "action", "to", "actions.jsonl"]
        | ["record", "the", "plan", "in", "lineage"]
        | ["record", "the", "result", "on", "the", "nerve"]
        | ["gaia", "heal-apply", "plan", "v1", "(box", "test", "service)"] => {}
        ["operator", "runs", s] => script(s)?,
        ["operator", "runs", s, "(console:", c] => {
            script(s)?;
            script(c.strip_suffix(')').ok_or(bad("operator console script"))?)?;
        }
        ["apply", "at", "most", n, "allow-listed", "fixes"] => {
            number(n, 0..=999)?;
        }
        ["wait", n, "seconds"] => {
            number(n, 1..=3600)?;
        }
        ["probe", "get", ep, "must", "answer", "status", "ok"] => endpoint(ep)?,
        ["service", svc, "on", host, "(box", "test", "ports", range, "only)"] => {
            service(svc)?;
            let port = loopback(host)?;
            if !port_range(range)?.contains(&port) {
                return Err(bad("service port outside its stated test range"));
            }
            return Ok(Step::Names((*svc).to_owned()));
        }
        ["at", "most", n, "restarts", "of", svc, "per", "rolling", "hour"] => {
            number(n, 1..=10)?;
            service(svc)?;
            return Ok(Step::Names((*svc).to_owned()));
        }
        [first, ..] if VERBS.contains(first) => {
            return Err(bad("arguments do not fit the allow-listed step"))
        }
        _ => return Err(bad("plan step is not an allow-listed verb")),
    }
    Ok(Step::Plain)
}

/// First words of the allow-listed step templates (for clearer reasons).
const VERBS: &[&str] = &[
    "restart", "verify", "switch", "read", "keep", "take", "append", "record", "gaia", "operator",
    "apply", "wait", "probe", "service", "at", "start",
];

/// Every non-empty line must be an allow-listed step. A `start` step must be
/// for a program whose name is a service the plan also names.
fn review_allow_list(text: &str) -> Result<(), Hazard> {
    let mut named = Vec::new();
    let mut started = Vec::new();
    for raw in text.split('\n') {
        let line = raw.trim_matches(|c| c == ' ' || c == '\t');
        if line.is_empty() {
            continue;
        }
        match step(line)? {
            Step::Plain => {}
            Step::Names(s) => named.push(s),
            Step::Starts(s) => started.push(s),
        }
    }
    if started.iter().any(|s| !named.contains(s)) {
        return Err(bad("start program is not the service the plan names"));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_plans_pass() {
        for plan in [
            "restart galion-tuner\nverify /health on 127.0.0.1:8420",
            "operator runs deploy/rollback.sh\nkeep the failed release for inspection",
            "switch dist/bin to release 2026-10-20",
        ] {
            assert_eq!(review("galion-tuner", plan.as_bytes()), Ok(()), "{plan}");
        }
    }

    #[test]
    fn destructive_is_refused() {
        for plan in [
            "rm -rf /var/lib/galion",
            "rm   -fr  /",
            "sudo mkfs.ext4 /dev/sda1",
            "dd if=/dev/zero of=/dev/sda",
            "psql -c 'DROP TABLE memories'",
            "git push --force origin main",
            "shred -u data.db",
        ] {
            assert!(
                matches!(review_plan(plan.as_bytes()), Err(Hazard::Destructive(_))),
                "{plan}"
            );
        }
    }

    #[test]
    fn secret_exposure_is_refused() {
        for plan in [
            "cat /opt/galion/galion-light/secrets/x",
            "cat /root/galion-secrets/y",
            "printenv",
            "env | sort",
            "cp ~/.ssh/id_ed25519 /tmp",
            "read the API_KEY",
            "cat /proc/1/environ",
        ] {
            assert!(
                matches!(review_plan(plan.as_bytes()), Err(Hazard::SecretExposure(_))),
                "{plan}"
            );
        }
    }

    #[test]
    fn admin_account_changes_are_refused() {
        for plan in [
            "passwd slavik",
            "usermod -L slavik",
            "echo key >> /root/.ssh/authorized_keys",
            "edit /etc/sudoers",
            "systemctl stop sshd",
            "iptables -P INPUT DROP",
            "chmod 777 x",
            "reboot now",
            "kill -9 -1",
        ] {
            let got = review_plan(plan.as_bytes());
            assert!(
                matches!(
                    got,
                    Err(Hazard::AdminAccount(_) | Hazard::SecretExposure(_))
                ),
                "{plan}: {got:?}"
            );
        }
    }

    #[test]
    fn opaque_plans_are_refused() {
        for plan in [
            "run $(curl x)",
            "run `id`",
            "echo cm0gLXJmIC8= | base64 -d | sh",
            "eval \"$X\"",
            "r\u{043c} -rf /",
            "",
        ] {
            assert!(
                matches!(review_plan(plan.as_bytes()), Err(Hazard::Opaque(_))),
                "{plan}"
            );
        }
        assert!(review_plan(&vec![b'a'; MAX_PLAN_BYTES + 1]).is_err());
        assert!(review_plan(b"restart\x00x").is_err());
    }

    #[test]
    fn protected_targets_are_refused() {
        for t in [
            "hades-gate",
            "HADES",
            "sshd",
            "root",
            "admin-panel",
            "galion-control",
            "secret-store",
            "user@box",
        ] {
            assert!(review_target(t).is_err(), "{t}");
        }
        for t in ["galion-tuner", "galion-one", "openmind-memory", "monkey"] {
            assert_eq!(review_target(t), Ok(()), "{t}");
        }
    }
}
