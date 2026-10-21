// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! T2.8 stray process owner: listening sockets owned by an unexpected user,
//! and listeners whose process runs a deleted binary (REVIEW.md:100 "one on a
//! deleted binary").
//!
//! Live only ([`Input::live`]): reads `/proc/net/tcp`, `/proc/net/tcp6`
//! (LISTEN rows: port, socket uid, inode), maps inodes to pids through
//! `/proc/<pid>/fd` (rescanned only when the set of listening inodes changes
//! or a cached pid exits), then `/proc/<pid>/status` (effective uid) and
//! `/proc/<pid>/exe` (`" (deleted)"` suffix). Users come from `/etc/passwd`.
//! Nothing is written, signalled or stopped. Respects `NERVE_LISTEN_SCOPE`.
//!
//! Settings (sensor off unless `NERVE_PROC_OWNER_USERS` is set):
//! - `NERVE_PROC_OWNER_USERS=root,galion` users (names or uids) any listener
//!   may run as;
//! - `NERVE_PROC_OWNER_PORTS=8420=galion,8670=root` per-port expected user
//!   (replaces the general list for that port);
//! - `NERVE_PROC_OWNER_PROC` (`/proc`), `NERVE_PROC_OWNER_PASSWD`
//!   (`/etc/passwd`) for fixture roots.
//!
//! Findings (aggregated, so 30 strays cost one deduction, not 30):
//! `stray-owner` (warn; error when one of them is not loopback),
//! `deleted-binary` (error), `proc-owner-unresolved` (info: sockets whose
//! process could not be read, e.g. the nerve is not root). Titles carry
//! ports, uids and counts only; user names, pids and exe paths stay in the
//! loopback-only evidence.

use super::{Env, Sensor, Sink};
use crate::cortex::{Finding, FindingKind, Input};
use crate::incident::Sev;
use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};

/// One LISTEN row of `/proc/net/tcp{,6}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SockRow {
    pub addr: String,
    pub port: u16,
    pub loopback: bool,
    pub uid: u32,
    pub inode: u64,
}

/// A listening socket with what is known about its process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owned {
    pub row: SockRow,
    /// Owning pid (None: no readable `/proc/<pid>/fd` holds the inode).
    pub pid: Option<u32>,
    /// Effective uid of the process (falls back to the socket uid).
    pub uid: u32,
    /// `/proc/<pid>/exe` target, `" (deleted)"` suffix kept.
    pub exe: Option<String>,
}

impl Owned {
    pub fn deleted(&self) -> bool {
        self.exe
            .as_deref()
            .is_some_and(|e| e.ends_with(" (deleted)"))
    }
}

/// Who may own listeners.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policy {
    /// Allowed users (names or numeric uids).
    pub users: Vec<String>,
    /// Per-port expected user, replacing `users` for that port.
    pub ports: BTreeMap<u16, String>,
}

impl Policy {
    fn allows(&self, port: u16, uid: u32, names: &BTreeMap<u32, String>) -> bool {
        let is = |want: &str| {
            want.parse::<u32>().ok() == Some(uid) || names.get(&uid).is_some_and(|n| n == want)
        };
        match self.ports.get(&port) {
            Some(want) => is(want),
            None => self.users.iter().any(|w| is(w)),
        }
    }

    fn expected(&self, port: u16) -> String {
        match self.ports.get(&port) {
            Some(u) => u.clone(),
            None => self.users.join(","),
        }
    }
}

/// Sensor `proc-owner`.
pub struct ProcOwner {
    pub policy: Policy,
    pub proc_root: PathBuf,
    pub passwd: PathBuf,
    /// inode -> pid from the last fd scan.
    cache: BTreeMap<u64, u32>,
    cache_key: BTreeSet<u64>,
    /// Number of `/proc/<pid>/fd` scans done (budget check in tests).
    pub scans: u64,
}

impl ProcOwner {
    pub fn new(policy: Policy) -> Self {
        ProcOwner {
            policy,
            proc_root: PathBuf::from("/proc"),
            passwd: PathBuf::from("/etc/passwd"),
            cache: BTreeMap::new(),
            cache_key: BTreeSet::new(),
            scans: 0,
        }
    }

    /// `NERVE_PROC_OWNER_USERS` set and non-empty, else None (off).
    pub fn from_env(env: Env) -> Option<Self> {
        let users: Vec<String> = split_list(&env("NERVE_PROC_OWNER_USERS")?);
        if users.is_empty() {
            return None;
        }
        let ports = split_list(&env("NERVE_PROC_OWNER_PORTS").unwrap_or_default())
            .iter()
            .filter_map(|kv| {
                let (p, u) = kv.split_once('=')?;
                Some((p.trim().parse().ok()?, u.trim().to_string()))
            })
            .collect();
        let mut s = Self::new(Policy { users, ports });
        if let Some(p) = env("NERVE_PROC_OWNER_PROC").filter(|p| !p.is_empty()) {
            s.proc_root = p.into();
        }
        if let Some(p) = env("NERVE_PROC_OWNER_PASSWD").filter(|p| !p.is_empty()) {
            s.passwd = p.into();
        }
        Some(s)
    }

    /// Read the host (only called when live): listening sockets in scope
    /// with their owning processes.
    pub fn read_host(&mut self, scope: Option<(u16, u16)>) -> Vec<Owned> {
        let mut rows = vec![];
        for f in ["net/tcp", "net/tcp6"] {
            if let Ok(t) = std::fs::read_to_string(self.proc_root.join(f)) {
                rows.extend(parse_proc_net_tcp(&t));
            }
        }
        rows.retain(|r| scope.is_none_or(|(a, b)| r.port >= a && r.port <= b));
        let key: BTreeSet<u64> = rows.iter().map(|r| r.inode).filter(|&i| i != 0).collect();
        let stale = self
            .cache
            .values()
            .any(|pid| !self.proc_root.join(pid.to_string()).exists());
        if key != self.cache_key || stale {
            self.cache = scan_fds(&self.proc_root, &key);
            self.cache_key = key;
            self.scans += 1;
        }
        rows.into_iter()
            .map(|row| {
                let pid = self.cache.get(&row.inode).copied();
                let dir = pid.map(|p| self.proc_root.join(p.to_string()));
                let uid = dir
                    .as_ref()
                    .and_then(|d| std::fs::read_to_string(d.join("status")).ok())
                    .and_then(|s| status_euid(&s))
                    .unwrap_or(row.uid);
                let exe = dir
                    .as_ref()
                    .and_then(|d| std::fs::read_link(d.join("exe")).ok())
                    .map(|p| p.to_string_lossy().into_owned());
                Owned { row, pid, uid, exe }
            })
            .collect()
    }
}

impl Sensor for ProcOwner {
    fn name(&self) -> &'static str {
        "proc-owner"
    }

    fn sense(&mut self, i: &Input<'_>, out: &mut Sink<'_>) {
        if !i.live {
            return;
        }
        let socks = self.read_host(i.listen_scope);
        let names = std::fs::read_to_string(&self.passwd)
            .map(|t| parse_passwd(&t))
            .unwrap_or_default();
        for f in judge(&socks, &self.policy, &names) {
            out.finding(f);
        }
    }
}

fn split_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

/// Pure verdict over one snapshot (unit tests and fixtures drive this).
pub fn judge(socks: &[Owned], policy: &Policy, names: &BTreeMap<u32, String>) -> Vec<Finding> {
    // One entry per port (v4 + v6 sockets of one process are one listener).
    let mut by_port: BTreeMap<u16, Vec<&Owned>> = BTreeMap::new();
    for s in socks {
        by_port.entry(s.row.port).or_default().push(s);
    }
    let user = |uid: u32| names.get(&uid).cloned().unwrap_or_else(|| "?".into());
    let mut stray = vec![];
    let mut stray_ev = vec![];
    let mut stray_public = false;
    let mut deleted = vec![];
    let mut deleted_ev = vec![];
    let mut unresolved = BTreeSet::new();
    for (&port, ss) in &by_port {
        let mut uids: Vec<u32> = ss.iter().map(|s| s.uid).collect();
        uids.sort();
        uids.dedup();
        let bad: Vec<u32> = uids
            .iter()
            .copied()
            .filter(|&u| !policy.allows(port, u, names))
            .collect();
        if !bad.is_empty() {
            stray_public |= ss.iter().any(|s| !s.row.loopback);
            stray.push(format!(
                ":{port} (uid {})",
                bad.iter()
                    .map(|u| u.to_string())
                    .collect::<Vec<_>>()
                    .join("/")
            ));
            for s in ss.iter().filter(|s| bad.contains(&s.uid)) {
                stray_ev.push(format!(
                    "{}:{port} user {} (uid {}) pid {} exe {}; expected {}",
                    s.row.addr,
                    user(s.uid),
                    s.uid,
                    s.pid.map_or("?".into(), |p| p.to_string()),
                    s.exe.as_deref().unwrap_or("?"),
                    policy.expected(port)
                ));
            }
        }
        if let Some(s) = ss.iter().find(|s| s.deleted()) {
            deleted.push(format!(":{port}"));
            deleted_ev.push(format!(
                "{}:{port} pid {} user {} exe {}",
                s.row.addr,
                s.pid.map_or("?".into(), |p| p.to_string()),
                user(s.uid),
                s.exe.as_deref().unwrap_or("?")
            ));
        }
        if ss.iter().all(|s| s.pid.is_none()) {
            unresolved.insert(port);
        }
    }
    let mut out = vec![];
    if !stray.is_empty() {
        out.push(
            Finding::new(
                FindingKind::Vulnerability,
                "stray-owner",
                if stray_public { Sev::Error } else { Sev::Warn },
                "host",
                format!(
                    "{} listener(s) run as an unexpected user: {}",
                    stray.len(),
                    stray.join(" ")
                ),
            )
            .with_evidence(stray_ev)
            .with_fix("run it under its unit's User= (or add the user to NERVE_PROC_OWNER_USERS/PORTS if intended); stopping a stray is an owner-approved HADES plan (T4.2)"),
        );
    }
    if !deleted.is_empty() {
        out.push(
            Finding::new(
                FindingKind::Vulnerability,
                "deleted-binary",
                Sev::Error,
                "host",
                format!(
                    "{} listener(s) run a deleted binary: {}",
                    deleted.len(),
                    deleted.join(" ")
                ),
            )
            .with_evidence(deleted_ev)
            .with_fix("the process runs code no longer on disk (old build, unpatched, unauditable): restart its unit onto the installed binary (owner approval)"),
        );
    }
    if !unresolved.is_empty() {
        out.push(
            Finding::new(
                FindingKind::Vulnerability,
                "proc-owner-unresolved",
                Sev::Info,
                "host",
                format!(
                    "{} listener(s) without a readable owning process (socket uid used)",
                    unresolved.len()
                ),
            )
            .with_evidence(unresolved.iter().map(|p| format!(":{p}")).collect())
            .with_fix("expected when the nerve is not root; the live unit runs as root"),
        );
    }
    out
}

/// LISTEN (`st` 0A) rows of `/proc/net/tcp` or `/proc/net/tcp6` text.
pub fn parse_proc_net_tcp(text: &str) -> Vec<SockRow> {
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 10 || f[3] != "0A" {
                return None;
            }
            let (a, p) = f[1].split_once(':')?;
            let port = u16::from_str_radix(p, 16).ok()?;
            let (addr, loopback) = hex_addr(a)?;
            Some(SockRow {
                addr,
                port,
                loopback,
                uid: f[7].parse().ok()?,
                inode: f[9].parse().ok()?,
            })
        })
        .collect()
}

/// Kernel hex address (32-bit words in host byte order) -> text, loopback.
fn hex_addr(h: &str) -> Option<(String, bool)> {
    let word = |i: usize| -> Option<[u8; 4]> {
        Some(
            u32::from_str_radix(h.get(i * 8..i * 8 + 8)?, 16)
                .ok()?
                .to_ne_bytes(),
        )
    };
    match h.len() {
        8 => {
            let ip = Ipv4Addr::from(word(0)?);
            Some((ip.to_string(), ip.is_loopback()))
        }
        32 => {
            let mut b = [0u8; 16];
            for i in 0..4 {
                b[i * 4..i * 4 + 4].copy_from_slice(&word(i)?);
            }
            let ip = Ipv6Addr::from(b);
            let lo = ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|v| v.is_loopback());
            Some((ip.to_string(), lo))
        }
        _ => None,
    }
}

/// Effective uid from `/proc/<pid>/status` (`Uid: real effective saved fs`).
pub fn status_euid(status: &str) -> Option<u32> {
    let l = status.lines().find(|l| l.starts_with("Uid:"))?;
    l.split_whitespace().nth(2)?.parse().ok()
}

/// uid -> name from `/etc/passwd` text.
pub fn parse_passwd(text: &str) -> BTreeMap<u32, String> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split(':');
            let name = f.next()?;
            let uid = f.nth(1)?.parse().ok()?;
            (!name.is_empty() && !name.starts_with('#')).then(|| (uid, name.to_string()))
        })
        .collect()
}

/// `socket:[N]` link target -> N.
pub fn socket_inode(link: &str) -> Option<u64> {
    link.strip_prefix("socket:[")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

/// inode -> pid for the wanted inodes, scanning readable `/proc/<pid>/fd`.
/// Stops early once every wanted inode is found.
fn scan_fds(proc_root: &Path, want: &BTreeSet<u64>) -> BTreeMap<u64, u32> {
    let mut found = BTreeMap::new();
    if want.is_empty() {
        return found;
    }
    let Ok(rd) = std::fs::read_dir(proc_root) else {
        return found;
    };
    for e in rd.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        let Ok(fds) = std::fs::read_dir(e.path().join("fd")) else {
            continue; // another user's process (not root): unresolved
        };
        for fd in fds.flatten() {
            if let Some(ino) = std::fs::read_link(fd.path())
                .ok()
                .and_then(|l| socket_inode(&l.to_string_lossy()))
            {
                if want.contains(&ino) {
                    found.entry(ino).or_insert(pid);
                }
            }
        }
        if found.len() == want.len() {
            break;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logscan::LogInsights;
    use crate::sensors::Sensors;

    const TCP: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:4C72 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 5001 1 0 100 0 0 10 0
   1: 00000000:0016 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 5002 1 0 100 0 0 10 0
   2: 0100007F:4C72 0100007F:9C40 01 00000000:00000000 00:00000000 00000000  1000        0 0 1 0 20 4 30 10 -1
";
    const TCP6: &str = "  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000001000000:4C73 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000   998        0 5003 1 0 100 0 0 10 0
   1: 0000000000000000FFFF00000100007F:4C74 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 5004 1 0 100 0 0 10 0
";

    fn owned(port: u16, uid: u32, lo: bool, pid: Option<u32>, exe: Option<&str>) -> Owned {
        Owned {
            row: SockRow {
                addr: if lo { "127.0.0.1" } else { "0.0.0.0" }.into(),
                port,
                loopback: lo,
                uid,
                inode: port as u64,
            },
            pid,
            uid,
            exe: exe.map(Into::into),
        }
    }

    fn names() -> BTreeMap<u32, String> {
        parse_passwd("root:x:0:0:root:/root:/bin/bash\n# c\ngalion:x:998:998::/opt:/usr/sbin/nologin\nbox:x:1000:1000::/home/box:/bin/bash\n")
    }

    #[test]
    fn parses_listen_rows_v4_v6_and_loopback() {
        let v4 = parse_proc_net_tcp(TCP);
        assert_eq!(v4.len(), 2, "ESTABLISHED row skipped");
        assert_eq!(
            v4[0],
            SockRow {
                addr: "127.0.0.1".into(),
                port: 19570,
                loopback: true,
                uid: 1000,
                inode: 5001
            }
        );
        assert_eq!(
            (v4[1].addr.as_str(), v4[1].port, v4[1].loopback),
            ("0.0.0.0", 22, false)
        );
        let v6 = parse_proc_net_tcp(TCP6);
        assert_eq!(
            (v6[0].addr.as_str(), v6[0].loopback, v6[0].uid),
            ("::1", true, 998)
        );
        assert_eq!(
            (v6[1].addr.as_str(), v6[1].loopback),
            ("::ffff:127.0.0.1", true)
        );
        assert_eq!(status_euid("Name:\tx\nUid:\t1000\t0\t0\t0\n"), Some(0));
        assert_eq!(socket_inode("socket:[5001]"), Some(5001));
        assert_eq!(socket_inode("pipe:[5001]"), None);
        assert_eq!(names().get(&998).map(String::as_str), Some("galion"));
    }

    #[test]
    fn stray_owner_deleted_binary_and_policy() {
        let p = Policy {
            users: vec!["root".into(), "998".into()],
            ports: [(19573u16, "galion".to_string())].into(),
        };
        let socks = [
            owned(19570, 1000, true, Some(41), Some("/usr/bin/x")),
            owned(19571, 0, true, Some(42), Some("/opt/old (deleted)")),
            owned(19572, 998, true, None, None),
            owned(19573, 0, true, Some(43), Some("/x")), // root but port wants galion
            owned(19573, 0, true, Some(43), Some("/x")), // v6 twin: one listener
        ];
        let f = judge(&socks, &p, &names());
        let ids: Vec<&str> = f.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(
            ids,
            ["stray-owner", "deleted-binary", "proc-owner-unresolved"]
        );
        assert_eq!(f[0].sev, Sev::Warn);
        assert_eq!(
            f[0].title,
            "2 listener(s) run as an unexpected user: :19570 (uid 1000) :19573 (uid 0)"
        );
        assert_eq!(f[0].evidence.len(), 3);
        assert!(f[0].evidence[0]
            .contains("user box (uid 1000) pid 41 exe /usr/bin/x; expected root,998"));
        assert_eq!(f[1].title, "1 listener(s) run a deleted binary: :19571");
        assert_eq!(f[1].sev, Sev::Error);
        assert_eq!(f[2].sev, Sev::Info);
        // Public stray is an error; titles carry no names/paths/pids.
        let f = judge(&[owned(19570, 1000, false, Some(1), None)], &p, &names());
        assert_eq!(f[0].sev, Sev::Error);
        let ev = f[0].to_nerve_event(1_792_520_000).to_string();
        assert!(!ev.contains("box") && !ev.contains("pid"), "{ev}");
        // Clean snapshot: nothing.
        assert!(judge(&[owned(1, 0, true, Some(1), Some("/a"))], &p, &names()).is_empty());
    }

    #[test]
    fn from_env_and_not_live_reads_nothing() {
        assert!(ProcOwner::from_env(&|_| None).is_none());
        assert!(
            ProcOwner::from_env(&|k| (k == "NERVE_PROC_OWNER_USERS").then(|| " , ".into()))
                .is_none()
        );
        let env = |k: &str| match k {
            "NERVE_PROC_OWNER_USERS" => Some("root, galion".into()),
            "NERVE_PROC_OWNER_PORTS" => Some("8420=galion, bad, 9=root".into()),
            "NERVE_PROC_OWNER_PROC" => Some("/nonexistent-proc".into()),
            _ => None,
        };
        let s = ProcOwner::from_env(&env).unwrap();
        assert_eq!(s.policy.users, ["root", "galion"]);
        assert_eq!(s.policy.ports.len(), 2);
        let mut reg = Sensors::empty();
        reg.add(s);
        let logs = LogInsights::default();
        let cx = reg.run(&Input::new(1, &[], &logs));
        assert!(cx.findings().next().is_none());
        assert_eq!(reg.names(), ["proc-owner"]);
    }

    #[test]
    fn fd_scan_on_a_fixture_proc_tree_is_cached() {
        let root = std::env::temp_dir().join(format!("proc-owner-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("net")).unwrap();
        std::fs::write(root.join("net/tcp"), TCP).unwrap();
        std::fs::write(root.join("net/tcp6"), TCP6).unwrap();
        for (pid, ino, uid, exe) in [
            (41, 5001, 1000, "/usr/bin/stray"),
            (42, 5003, 998, "/opt/svc (deleted)"),
        ] {
            let d = root.join(pid.to_string());
            std::fs::create_dir_all(d.join("fd")).unwrap();
            std::os::unix::fs::symlink(format!("socket:[{ino}]"), d.join("fd/3")).unwrap();
            std::os::unix::fs::symlink("/dev/null", d.join("fd/0")).unwrap();
            std::os::unix::fs::symlink(exe, d.join("exe")).unwrap();
            std::fs::write(
                d.join("status"),
                format!("Uid:\t{uid}\t{uid}\t{uid}\t{uid}\n"),
            )
            .unwrap();
        }
        let mut s = ProcOwner::new(Policy {
            users: vec!["root".into(), "galion".into()],
            ports: BTreeMap::new(),
        });
        s.proc_root = root.clone();
        let got = s.read_host(Some((19570, 19579)));
        assert_eq!(got.len(), 3, "port 22 out of scope");
        assert_eq!(got[0].pid, Some(41));
        assert_eq!(got[1].pid, Some(42));
        assert!(got[1].deleted());
        assert_eq!(got[2].pid, None);
        s.read_host(Some((19570, 19579)));
        assert_eq!(s.scans, 1, "same inode set: no rescan");
        std::fs::remove_dir_all(root.join("41")).unwrap();
        s.read_host(Some((19570, 19579)));
        assert_eq!(s.scans, 2, "cached pid exited: rescan");
        let f = judge(&got, &s.policy, &names());
        assert_eq!(
            f[0].title,
            "1 listener(s) run as an unexpected user: :19570 (uid 1000)"
        );
        assert_eq!(f[1].title, "1 listener(s) run a deleted binary: :19571");
        let _ = std::fs::remove_dir_all(&root);
    }
}
