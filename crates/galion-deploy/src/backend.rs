// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Service-manager backends. `GALION_DEPLOY_SM=systemd` (default) issues the same
//! `systemctl`/`journalctl` calls as the bash script; `GALION_DEPLOY_SM=direct` sends
//! every call to `$GALION_DEPLOY_CTL <verb> <unit> [n]`, e.g. the box simulator
//! `simsup --root DIR ctl` (sim/deploy-sim). Tests plug in their own `Manager`.
//!
//! Every child runs in its own process group with stdin from /dev/null, so a Ctrl-C
//! or a dropped ssh session reaches galion-deploy (which rolls back) and not a
//! `systemctl restart` halfway through.

use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

pub trait Manager {
    fn known(&mut self, unit: &str) -> bool;
    /// The installed binary (first `ExecStart` path); `None` if it cannot be resolved.
    fn exec_path(&mut self, unit: &str) -> Option<String>;
    fn restart(&mut self, unit: &str) -> bool;
    fn stop(&mut self, unit: &str) -> bool;
    fn is_active(&mut self, unit: &str) -> bool;
    fn journal(&mut self, unit: &str, lines: u32) -> String;
    fn reset_failed(&mut self, unit: &str) -> bool;
}

fn cmd(argv: &[String]) -> Command {
    let mut c = Command::new(&argv[0]);
    c.args(&argv[1..]).stdin(Stdio::null()).process_group(0);
    c
}

fn quiet(argv: &[String]) -> bool {
    cmd(argv)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Output goes where the bash script's went: inherited stdout/stderr.
fn loud(argv: &[String]) -> bool {
    cmd(argv).status().map(|s| s.success()).unwrap_or(false)
}

fn stdout_of(argv: &[String]) -> String {
    cmd(argv)
        .stderr(Stdio::inherit())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

fn v(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// `sed -n 's/.*path=\([^ ;]*\).*/\1/p' | head -1` over `systemctl show -p ExecStart --value`.
pub fn parse_exec_start(show: &str) -> Option<String> {
    show.lines().find_map(|l| {
        let i = l.rfind("path=")?;
        let rest = &l[i + 5..];
        let end = rest.find([' ', ';']).unwrap_or(rest.len());
        Some(rest[..end].to_string())
    })
}

pub struct Systemd;

impl Manager for Systemd {
    fn known(&mut self, u: &str) -> bool {
        quiet(&v(&["systemctl", "cat", &format!("{u}.service")]))
    }
    fn exec_path(&mut self, u: &str) -> Option<String> {
        let out = stdout_of(&v(&[
            "systemctl",
            "show",
            "-p",
            "ExecStart",
            "--value",
            &format!("{u}.service"),
        ]));
        parse_exec_start(&out).filter(|p| !p.is_empty())
    }
    fn restart(&mut self, u: &str) -> bool {
        loud(&v(&["systemctl", "restart", &format!("{u}.service")]))
    }
    fn stop(&mut self, u: &str) -> bool {
        loud(&v(&["systemctl", "stop", &format!("{u}.service")]))
    }
    fn is_active(&mut self, u: &str) -> bool {
        quiet(&v(&[
            "systemctl",
            "is-active",
            "--quiet",
            &format!("{u}.service"),
        ]))
    }
    fn journal(&mut self, u: &str, n: u32) -> String {
        stdout_of(&v(&[
            "journalctl",
            "-u",
            &format!("{u}.service"),
            "-n",
            &n.to_string(),
            "--no-pager",
            "-o",
            "cat",
        ]))
    }
    fn reset_failed(&mut self, u: &str) -> bool {
        loud(&v(&["systemctl", "reset-failed", &format!("{u}.service")]))
    }
}

/// `$GALION_DEPLOY_CTL` split on whitespace, like bash `read -a`.
pub struct Direct {
    pub ctl: Vec<String>,
}

impl Direct {
    fn with(&self, extra: &[&str]) -> Vec<String> {
        let mut a = self.ctl.clone();
        a.extend(extra.iter().map(|s| s.to_string()));
        a
    }
}

impl Manager for Direct {
    fn known(&mut self, u: &str) -> bool {
        quiet(&self.with(&["known", u]))
    }
    fn exec_path(&mut self, u: &str) -> Option<String> {
        let out = stdout_of(&self.with(&["exec-path", u]));
        out.lines()
            .next()
            .map(str::to_string)
            .filter(|p| !p.is_empty())
    }
    fn restart(&mut self, u: &str) -> bool {
        loud(&self.with(&["restart", u]))
    }
    fn stop(&mut self, u: &str) -> bool {
        loud(&self.with(&["stop", u]))
    }
    fn is_active(&mut self, u: &str) -> bool {
        quiet(&self.with(&["is-active", u]))
    }
    fn journal(&mut self, u: &str, n: u32) -> String {
        stdout_of(&self.with(&["journal", u, &n.to_string()]))
    }
    fn reset_failed(&mut self, u: &str) -> bool {
        loud(&self.with(&["reset-failed", u]))
    }
}

#[cfg(test)]
mod tests {
    use super::parse_exec_start;

    #[test]
    fn exec_start() {
        let s = "{ path=/opt/galion/tuner/bin/tuner ; argv[]=/opt/galion/tuner/bin/tuner ; ignore_errors=no ; start_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }\n";
        assert_eq!(
            parse_exec_start(s).as_deref(),
            Some("/opt/galion/tuner/bin/tuner")
        );
        assert_eq!(parse_exec_start("").as_deref(), None);
        assert_eq!(parse_exec_start("{ path= ; }").as_deref(), Some(""));
        // greedy like sed: the LAST path= on the line wins
        assert_eq!(
            parse_exec_start("path=/a ; x path=/b").as_deref(),
            Some("/b")
        );
        assert_eq!(
            parse_exec_start("nothing\npath=/c\n").as_deref(),
            Some("/c")
        );
    }
}
