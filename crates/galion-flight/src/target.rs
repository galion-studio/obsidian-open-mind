// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Where the checks run. [`Vps`] sends exactly one read-only command per
//! check over ssh (rendered from the closed [`Remote`] set below, every
//! argument single-quoted and pre-validated). [`Local`] does the same reads
//! natively on the box for box instances. Neither writes, POSTs, restarts
//! or retries anything.

use crate::manifest::{safe_path, safe_unit, safe_url_path};
use std::collections::BTreeMap;
use std::io::Read;
use std::net::SocketAddr;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Result of one read. `NotApplicable` turns into SKIP.
#[derive(Debug)]
pub enum ReadErr {
    NotApplicable(String),
    Failed(String),
}

pub type R<T> = Result<T, ReadErr>;

fn failed<T>(s: impl Into<String>) -> R<T> {
    Err(ReadErr::Failed(s.into()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    pub size: u64,
    pub mode: String,
    pub owner: String,
    pub mtime: i64,
}

/// Properties read from a unit in one `systemctl show`. Environment and
/// ExecStart are deliberately never requested (they can carry secrets).
pub const UNIT_PROPS: &[&str] = &[
    "ActiveState",
    "SubState",
    "Result",
    "NRestarts",
    "MainPID",
    "ExecMainStartTimestamp",
    "DropInPaths",
    "UnitFileState",
];

pub trait Target {
    fn label(&self) -> String;
    fn unit_show(&mut self, unit: &str, props: &[&str]) -> R<BTreeMap<String, String>>;
    fn timer_next(&mut self, timer: &str) -> R<Option<i64>>;
    fn readlink_exe(&mut self, pid: u32) -> R<String>;
    fn sha256(&mut self, path: &str) -> R<String>;
    fn stat(&mut self, path: &str) -> R<Stat>;
    fn free_mib(&mut self, path: &str) -> R<u64>;
    fn listening(&mut self, port: u16) -> R<bool>;
    /// GET http://127.0.0.1:PORT/PATH -> (status, body). Status 0 = no answer.
    fn get(&mut self, port: u16, path: &str) -> R<(u16, Vec<u8>)>;
    /// The pid of the service when there is no unit (box instances).
    fn given_pid(&self) -> Option<u32> {
        None
    }
}

/// The closed set of commands the VPS may be sent. One per check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remote {
    Show { unit: String, props: Vec<String> },
    TimerNext { timer: String },
    Readlink { pid: u32 },
    Sha { path: String },
    Stat { path: String },
    Statfs { path: String },
    Ss { port: u16 },
    Curl { port: u16, path: String },
}

fn q(s: &str) -> String {
    // Inputs are validated to exclude quotes; quote anyway.
    format!("'{}'", s.replace('\'', ""))
}

impl Remote {
    /// The exact command line run on the VPS.
    pub fn render(&self) -> Result<String, String> {
        Ok(match self {
            Remote::Show { unit, props } => {
                safe_unit(unit)?;
                let mut s = String::from("systemctl show --timestamp=unix");
                for p in props {
                    if !p.bytes().all(|b| b.is_ascii_alphabetic())
                        || p == "Environment"
                        || (p.starts_with("Exec") && p != "ExecMainStartTimestamp")
                    {
                        return Err(format!("property {p:?} not allowed"));
                    }
                    s.push_str(" -p ");
                    s.push_str(p);
                }
                s.push(' ');
                s.push_str(&q(unit));
                s
            }
            Remote::TimerNext { timer } => {
                safe_unit(timer)?;
                format!(
                    "systemctl show --timestamp=unix -p NextElapseUSecRealtime {}",
                    q(timer)
                )
            }
            Remote::Readlink { pid } => format!("readlink {}", q(&format!("/proc/{pid}/exe"))),
            Remote::Sha { path } => {
                safe_path(path)?;
                format!("sha256sum {}", q(path))
            }
            Remote::Stat { path } => {
                safe_path(path)?;
                format!("stat -c '%s %a %U:%G %Y' {}", q(path))
            }
            Remote::Statfs { path } => {
                safe_path(path)?;
                format!("stat -f -c '%a %S' {}", q(path))
            }
            Remote::Ss { port } => format!("ss -ltnH {}", q(&format!("sport = :{port}"))),
            Remote::Curl { port, path } => {
                safe_url_path(path)?;
                format!(
                    "curl -sS -m 5 --max-filesize 4194304 -w '\\n%{{http_code}}' {}",
                    q(&format!("http://127.0.0.1:{port}{path}"))
                )
            }
        })
    }
}

/// Live VPS over ssh (BatchMode, one multiplexed connection, box-side
/// control socket only).
pub struct Vps {
    pub host: String,
    pub control: String,
    /// Every command sent, for the record and the tests.
    pub sent: Vec<String>,
}

impl Vps {
    pub fn new(host: &str) -> Vps {
        let control = format!(
            "{}/galion-flight-cm-{}",
            std::env::temp_dir().display(),
            std::process::id()
        );
        Vps {
            host: host.into(),
            control,
            sent: Vec::new(),
        }
    }

    fn run(&mut self, r: &Remote) -> R<(i32, String)> {
        let cmd = r.render().map_err(ReadErr::Failed)?;
        self.sent.push(cmd.clone());
        let out = Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                "-o",
                "ServerAliveInterval=5",
                "-o",
                "ServerAliveCountMax=2",
                "-o",
                "ControlMaster=auto",
                "-o",
                &format!("ControlPath={}", self.control),
                "-o",
                "ControlPersist=60",
                &self.host,
                "--",
                &cmd,
            ])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| ReadErr::Failed(format!("ssh: {e}")))?;
        let code = out.status.code().unwrap_or(-1);
        if code == 255 {
            return failed("ssh transport failed (exit 255)");
        }
        Ok((code, String::from_utf8_lossy(&out.stdout).into_owned()))
    }

    /// Close the control master (box-side socket only).
    pub fn close(&self) {
        let _ = Command::new("ssh")
            .args([
                "-o",
                &format!("ControlPath={}", self.control),
                "-O",
                "exit",
                &self.host,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

pub fn parse_show(out: &str) -> BTreeMap<String, String> {
    out.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_string(), v.trim().to_string()))
        .collect()
}

/// `@1761070903` (systemctl --timestamp=unix) -> seconds.
pub fn parse_unix_ts(v: &str) -> Option<i64> {
    v.trim().strip_prefix('@')?.parse().ok()
}

/// systemd's realtime form `Wed 2026-10-21 18:21:43 UTC` (NextElapseUSecRealtime
/// ignores `--timestamp=unix` on systemd 255). Only UTC is accepted, anything
/// else is `None` (the caller fails closed).
pub fn parse_systemd_utc(v: &str) -> Option<i64> {
    let w: Vec<&str> = v.split_whitespace().collect();
    if w.len() != 4 || !(w[3] == "UTC" || w[3] == "GMT") {
        return None;
    }
    let d: Vec<&str> = w[1].split('-').collect();
    let t: Vec<&str> = w[2].split(':').collect();
    if d.len() != 3 || t.len() != 3 {
        return None;
    }
    let (y, mo, da): (i64, u32, u32) = (d[0].parse().ok()?, d[1].parse().ok()?, d[2].parse().ok()?);
    let (h, mi, s): (i64, i64, i64) = (t[0].parse().ok()?, t[1].parse().ok()?, t[2].parse().ok()?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&da) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    Some(galion_common::time::days_from_civil(y, mo, da) * 86_400 + h * 3600 + mi * 60 + s)
}

/// Timer property value -> Ok(None) not scheduled, Ok(Some(t)), Err unparsable.
pub fn parse_timer_value(v: Option<&String>) -> Result<Option<i64>, String> {
    match v.map(|s| s.trim()) {
        None => Err("no NextElapseUSecRealtime".into()),
        Some("") | Some("n/a") => Ok(None),
        Some(s) => parse_unix_ts(s)
            .or_else(|| parse_systemd_utc(s))
            .map(Some)
            .ok_or_else(|| format!("unparsable timer time {s:?}")),
    }
}

pub fn parse_stat(s: &str) -> Option<Stat> {
    let w: Vec<&str> = s.split_whitespace().collect();
    if w.len() != 4 {
        return None;
    }
    Some(Stat {
        size: w[0].parse().ok()?,
        mode: w[1].to_string(),
        owner: w[2].to_string(),
        mtime: w[3].parse().ok()?,
    })
}

/// curl output: body, "\n", status code (000 = no answer).
pub fn parse_curl(out: &str) -> Option<(u16, Vec<u8>)> {
    let (body, code) = out.rsplit_once('\n')?;
    Some((code.trim().parse().ok()?, body.as_bytes().to_vec()))
}

impl Target for Vps {
    fn label(&self) -> String {
        format!("vps:{}", self.host)
    }
    fn unit_show(&mut self, unit: &str, props: &[&str]) -> R<BTreeMap<String, String>> {
        let (code, out) = self.run(&Remote::Show {
            unit: unit.into(),
            props: props.iter().map(|s| s.to_string()).collect(),
        })?;
        if code != 0 {
            return failed(format!("systemctl show exit {code}"));
        }
        Ok(parse_show(&out))
    }
    fn timer_next(&mut self, timer: &str) -> R<Option<i64>> {
        let (code, out) = self.run(&Remote::TimerNext {
            timer: timer.into(),
        })?;
        if code != 0 {
            return failed(format!("systemctl show exit {code}"));
        }
        let m = parse_show(&out);
        parse_timer_value(m.get("NextElapseUSecRealtime")).map_err(ReadErr::Failed)
    }
    fn readlink_exe(&mut self, pid: u32) -> R<String> {
        let (code, out) = self.run(&Remote::Readlink { pid })?;
        if code != 0 {
            return failed(format!("readlink exit {code}"));
        }
        Ok(first_line(&out))
    }
    fn sha256(&mut self, path: &str) -> R<String> {
        let (code, out) = self.run(&Remote::Sha { path: path.into() })?;
        if code != 0 {
            return failed(format!("sha256sum exit {code} (missing or unreadable)"));
        }
        let l = first_line(&out);
        Ok(l.split_whitespace().next().unwrap_or("").to_string())
    }
    fn stat(&mut self, path: &str) -> R<Stat> {
        let (code, out) = self.run(&Remote::Stat { path: path.into() })?;
        if code != 0 {
            return failed(format!("stat exit {code} (missing)"));
        }
        parse_stat(&first_line(&out)).ok_or(ReadErr::Failed("unparsable stat".into()))
    }
    fn free_mib(&mut self, path: &str) -> R<u64> {
        let (code, out) = self.run(&Remote::Statfs { path: path.into() })?;
        if code != 0 {
            return failed(format!("stat -f exit {code}"));
        }
        let l = first_line(&out);
        let w: Vec<u64> = l
            .split_whitespace()
            .filter_map(|x| x.parse().ok())
            .collect();
        if w.len() != 2 {
            return failed("unparsable stat -f");
        }
        Ok(w[0].saturating_mul(w[1]) / (1024 * 1024))
    }
    fn listening(&mut self, port: u16) -> R<bool> {
        let (code, out) = self.run(&Remote::Ss { port })?;
        if code != 0 {
            return failed(format!("ss exit {code}"));
        }
        Ok(out.lines().any(|l| l.contains("LISTEN")))
    }
    fn get(&mut self, port: u16, path: &str) -> R<(u16, Vec<u8>)> {
        let (_code, out) = self.run(&Remote::Curl {
            port,
            path: path.into(),
        })?;
        parse_curl(&out).ok_or(ReadErr::Failed("unparsable curl output".into()))
    }
}

/// Box instance: same reads, natively. No systemd unit; the pid is given.
pub struct Local {
    pub pid: Option<u32>,
}

/// Parse /proc/net/tcp{,6}: any LISTEN (state 0A) socket on `port`.
pub fn proc_net_listening(text: &str, port: u16) -> bool {
    text.lines().skip(1).any(|l| {
        let w: Vec<&str> = l.split_whitespace().collect();
        w.len() > 3
            && w[3] == "0A"
            && w[1]
                .rsplit_once(':')
                .and_then(|(_, p)| u16::from_str_radix(p, 16).ok())
                == Some(port)
    })
}

impl Target for Local {
    fn label(&self) -> String {
        "box".into()
    }
    fn given_pid(&self) -> Option<u32> {
        self.pid
    }
    fn unit_show(&mut self, _unit: &str, _props: &[&str]) -> R<BTreeMap<String, String>> {
        Err(ReadErr::NotApplicable("no systemd unit on the box".into()))
    }
    fn timer_next(&mut self, _timer: &str) -> R<Option<i64>> {
        Err(ReadErr::NotApplicable("no timers on the box".into()))
    }
    fn readlink_exe(&mut self, pid: u32) -> R<String> {
        std::fs::read_link(format!("/proc/{pid}/exe"))
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(|e| ReadErr::Failed(format!("readlink: {e}")))
    }
    fn sha256(&mut self, path: &str) -> R<String> {
        let mut f = std::fs::File::open(path).map_err(|e| ReadErr::Failed(format!("open: {e}")))?;
        let mut h = galion_common::sha256::Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = f
                .read(&mut buf)
                .map_err(|e| ReadErr::Failed(format!("read: {e}")))?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        Ok(galion_common::hex::encode(&h.finalize()))
    }
    fn stat(&mut self, path: &str) -> R<Stat> {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata(path).map_err(|e| ReadErr::Failed(format!("stat: {e}")))?;
        Ok(Stat {
            size: m.len(),
            mode: format!("{:o}", m.mode() & 0o7777),
            owner: format!("{}:{}", m.uid(), m.gid()),
            mtime: m.mtime(),
        })
    }
    fn free_mib(&mut self, _path: &str) -> R<u64> {
        Err(ReadErr::NotApplicable("statfs not in std".into()))
    }
    fn listening(&mut self, port: u16) -> R<bool> {
        let mut any = false;
        for f in ["/proc/net/tcp", "/proc/net/tcp6"] {
            if let Ok(t) = std::fs::read_to_string(f) {
                any |= proc_net_listening(&t, port);
            }
        }
        Ok(any)
    }
    fn get(&mut self, port: u16, path: &str) -> R<(u16, Vec<u8>)> {
        use galion_common::http::{send, ClientRequest};
        let addr: SocketAddr = ([127, 0, 0, 1], port).into();
        let req = ClientRequest::get(path, &format!("127.0.0.1:{port}"));
        match send(addr, &req, Duration::from_secs(5), 4 << 20) {
            Ok(r) => Ok((r.status, r.body)),
            Err(_) => Ok((0, Vec::new())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<Remote> {
        vec![
            Remote::Show {
                unit: "galion-nerve.service".into(),
                props: UNIT_PROPS.iter().map(|s| s.to_string()).collect(),
            },
            Remote::TimerNext {
                timer: "galion-backup.timer".into(),
            },
            Remote::Readlink { pid: 42 },
            Remote::Sha {
                path: "/opt/galion/rust/bin/x".into(),
            },
            Remote::Stat {
                path: "/opt/galion/rust/bin/x".into(),
            },
            Remote::Statfs {
                path: "/opt/galion".into(),
            },
            Remote::Ss { port: 8670 },
            Remote::Curl {
                port: 8670,
                path: "/healthz".into(),
            },
        ]
    }

    #[test]
    fn every_remote_command_is_a_single_read() {
        let verbs = [
            "systemctl show ",
            "readlink ",
            "sha256sum ",
            "stat ",
            "ss -ltnH ",
            "curl -sS ",
        ];
        for r in all() {
            let c = r.render().unwrap();
            assert!(verbs.iter().any(|v| c.starts_with(v)), "{c}");
            // Outside quotes: no shell operators.
            let mut inq = false;
            for ch in c.chars() {
                if ch == '\'' {
                    inq = !inq;
                    continue;
                }
                if !inq {
                    assert!(!";|&><`$\n(".contains(ch), "operator {ch:?} in {c}");
                }
            }
            for bad in [
                "-X", " -d ", "--data", "POST", "PUT", "DELETE", " -o ", "-T ", "--upload", "-F ",
            ] {
                assert!(!c.contains(bad), "{bad} in {c}");
            }
            assert!(!c.contains("Environment"), "{c}");
            assert!(!c.contains("ExecStart "), "{c}");
        }
    }

    #[test]
    fn rejects_secret_properties_and_bad_args() {
        let r = Remote::Show {
            unit: "x.service".into(),
            props: vec!["Environment".into()],
        };
        assert!(r.render().is_err());
        let r = Remote::Show {
            unit: "x.service".into(),
            props: vec!["ExecStart".into()],
        };
        assert!(r.render().is_err());
        assert!(Remote::Sha {
            path: "/etc/x;id".into()
        }
        .render()
        .is_err());
        assert!(Remote::Curl {
            port: 1,
            path: "/x' -d 'a".into()
        }
        .render()
        .is_err());
    }

    #[test]
    fn parsers() {
        assert_eq!(parse_unix_ts("@1761070903"), Some(1761070903));
        assert_eq!(parse_unix_ts("n/a"), None);
        let s = parse_stat("1213216 755 root:root 1792561280").unwrap();
        assert_eq!(s.mode, "755");
        assert_eq!(parse_curl("ok\n200"), Some((200, b"ok".to_vec())));
        assert_eq!(parse_curl("\n000"), Some((0, vec![])));
        let t = "  sl  local_address rem_address   st\n   0: 0100007F:21DE 00000000:0000 0A 0\n";
        assert!(proc_net_listening(t, 0x21DE));
        assert!(!proc_net_listening(t, 8671));
    }
}

#[cfg(test)]
mod timer_tests {
    use super::*;

    #[test]
    fn systemd_realtime_values() {
        // 2026-10-21 18:21:43 UTC = 20:21:43 +02:00
        let t = parse_systemd_utc("Wed 2026-10-21 18:21:43 UTC").unwrap();
        assert_eq!(
            galion_common::time::from_unix(t),
            "2026-10-21T20:21:43+02:00"
        );
        assert_eq!(parse_systemd_utc("Wed 2026-10-21 18:21:43 CEST"), None);
        assert_eq!(parse_timer_value(Some(&"".to_string())), Ok(None));
        assert_eq!(parse_timer_value(Some(&"@5".to_string())), Ok(Some(5)));
        // Fail closed: an unparsable time is an error, never "not scheduled".
        assert!(parse_timer_value(Some(&"tomorrow".to_string())).is_err());
        assert!(parse_timer_value(None).is_err());
    }
}
