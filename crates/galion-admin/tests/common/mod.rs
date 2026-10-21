// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Helpers shared by the real tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

pub struct Admin {
    pub bin: PathBuf,
    pub box_bin: PathBuf,
    pub state: PathBuf,
}

impl Admin {
    pub fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(&self.bin);
        c.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("TZ", "Europe/Zagreb")
            .arg("--state")
            .arg(&self.state)
            .arg("--box")
            .arg(&self.box_bin)
            .args(args);
        c
    }
    pub fn run(&self, args: &[&str]) -> (i32, String, String) {
        let o: Output = self.cmd(args).output().expect("run galion-admin");
        (
            o.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        )
    }
    pub fn audit(&self) -> String {
        std::fs::read_to_string(self.state.join("audit.jsonl")).unwrap_or_default()
    }
    pub fn state_file(&self, name: &str) -> String {
        std::fs::read_to_string(self.state.join(format!("sandboxes/{name}.state")))
            .unwrap_or_default()
    }
    pub fn field(&self, name: &str, key: &str) -> String {
        self.state_file(name)
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{key}=")).map(str::to_string))
            .unwrap_or_default()
    }
    /// Wait until `sandbox status NAME` stops reporting running.
    pub fn wait_exited(&self, name: &str, max: Duration) -> bool {
        let end = Instant::now() + max;
        while Instant::now() < end {
            if self.run(&["sandbox", "status", name]).0 == 4 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }
}

pub fn pid_alive(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(s) => !s[s.rfind(')').unwrap() + 1..].trim_start().starts_with('Z'),
        Err(_) => false,
    }
}

/// Pids of our uid whose cmdline contains `needle` (excluding this test process).
pub fn procs_with(needle: &str) -> Vec<u32> {
    // SAFETY: trivial getter.
    let uid = unsafe { libc::getuid() };
    let me = std::process::id();
    let mut v = Vec::new();
    for e in std::fs::read_dir("/proc").unwrap().flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if pid == me {
            continue;
        }
        let Ok(st) = std::fs::read_to_string(format!("/proc/{pid}/status")) else {
            continue;
        };
        let ruid = st
            .lines()
            .find_map(|l| l.strip_prefix("Uid:"))
            .and_then(|l| l.split_whitespace().next())
            .and_then(|s| s.parse::<u32>().ok());
        if ruid != Some(uid)
            || st
                .lines()
                .any(|l| l.starts_with("State:") && l.contains('Z'))
        {
            continue;
        }
        let cl = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        if String::from_utf8_lossy(&cl)
            .replace('\0', " ")
            .contains(needle)
        {
            v.push(pid);
        }
    }
    v
}

pub fn wait_until(max: Duration, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + max;
    loop {
        if f() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn fresh_dir(base: &Path, name: &str) -> PathBuf {
    let d = base.join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn stamp() -> String {
    let o = Command::new("date")
        .env("TZ", "Europe/Zagreb")
        .arg("+%Y-%m-%d %H:%M:%S %:z")
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}
