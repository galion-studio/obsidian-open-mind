// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The supervisor: starts every unit in `<root>/units`, applies Restart=/RestartSec=,
//! systemd's start rate limit (fixed window, like `ratelimit_below`), SIGTERM then
//! SIGKILL after TimeoutStopSec on stop, and answers control requests on
//! `<root>/ctl.sock`. Child stdout/stderr go to `<root>/journal/<unit>.log`
//! (what `journalctl -u <unit> -o cat` would show); timestamped supervisor events
//! go to stderr.

use crate::unit::{self, Restart, Unit};
use crate::zagreb;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}
const SIGTERM: i32 = 15;
const SIGKILL: i32 = 9;

fn signal_group(pgid: u32, sig: i32) {
    // SAFETY: kill(2) with a negative pid signals a process group we created
    // (process_group(0) at spawn); it touches no memory.
    unsafe {
        kill(-(pgid as i32), sig);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Inactive,
    Active,
    AutoRestart,
    Failed,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            State::Inactive => "inactive",
            State::Active => "active",
            State::AutoRestart => "activating",
            State::Failed => "failed",
        }
    }
}

struct Svc {
    unit: Unit,
    child: Option<Child>,
    state: State,
    result: String,
    restart_at: Option<Instant>,
    rl_begin: Option<Instant>,
    rl_num: u32,
    n_restarts: u32,
    last_exit: String,
}

pub struct Sup {
    root: PathBuf,
    svcs: BTreeMap<String, Svc>,
    quit: bool,
}

fn ev(msg: &str) {
    eprintln!("{} {msg}", zagreb::now());
}

fn describe(st: ExitStatus) -> (String, bool) {
    if let Some(code) = st.code() {
        let name = match code {
            0 => "SUCCESS",
            1 => "FAILURE",
            2 => "INVALIDARGUMENT",
            203 => "EXEC",
            _ => "",
        };
        (format!("code=exited, status={code}/{name}"), code == 0)
    } else {
        let sig = st.signal().unwrap_or(0);
        let name = match sig {
            6 => "ABRT",
            9 => "KILL",
            11 => "SEGV",
            15 => "TERM",
            _ => "",
        };
        (format!("code=killed, status={sig}/{name}"), false)
    }
}

impl Sup {
    pub fn new(root: &Path) -> Result<Sup, String> {
        let mut s = Sup {
            root: root.to_path_buf(),
            svcs: BTreeMap::new(),
            quit: false,
        };
        fs::create_dir_all(root.join("journal")).map_err(|e| e.to_string())?;
        fs::create_dir_all(root.join("run")).map_err(|e| e.to_string())?;
        s.load()?;
        Ok(s)
    }

    /// Read `<root>/units/*.service`. New units are added inactive; known units get
    /// their config replaced (running processes keep running, like daemon-reload).
    fn load(&mut self) -> Result<Vec<String>, String> {
        let dir = self.root.join("units");
        let mut names = Vec::new();
        let rd = fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for ent in rd.flatten() {
            let p = ent.path();
            let Some(fname) = p.file_name().and_then(|f| f.to_str()) else {
                continue;
            };
            let Some(name) = fname.strip_suffix(".service") else {
                continue;
            };
            let text = fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            let u = unit::parse(name, &text)?;
            names.push(name.to_string());
            match self.svcs.get_mut(name) {
                Some(s) => s.unit = u,
                None => {
                    self.svcs.insert(
                        name.to_string(),
                        Svc {
                            unit: u,
                            child: None,
                            state: State::Inactive,
                            result: "success".into(),
                            restart_at: None,
                            rl_begin: None,
                            rl_num: 0,
                            n_restarts: 0,
                            last_exit: String::new(),
                        },
                    );
                }
            }
        }
        self.svcs
            .retain(|k, v| names.contains(k) || v.child.is_some());
        Ok(names)
    }

    fn journal(&self, name: &str) -> PathBuf {
        self.root.join("journal").join(format!("{name}.log"))
    }

    fn jlog(&self, name: &str, msg: &str) {
        ev(&format!("{name}.service: {msg}"));
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.journal(name))
        {
            let _ = writeln!(f, "{name}.service: {msg}");
        }
    }

    /// systemd `ratelimit_below`: fixed window that restarts when it has elapsed.
    fn start_allowed(s: &mut Svc, now: Instant) -> bool {
        if s.unit.start_limit_interval.is_zero() || s.unit.start_limit_burst == 0 {
            return true;
        }
        let expired = s
            .rl_begin
            .is_none_or(|b| now > b + s.unit.start_limit_interval);
        if expired {
            s.rl_begin = Some(now);
            s.rl_num = 0;
        }
        if s.rl_num < s.unit.start_limit_burst {
            s.rl_num += 1;
            true
        } else {
            false
        }
    }

    /// Start one unit. Err only for the start-limit refusal (what systemctl reports
    /// as a failed job); an exec failure is not a job failure for Type=simple.
    fn start(&mut self, name: &str) -> Result<(), String> {
        let now = Instant::now();
        let journal = self.journal(name);
        let Some(s) = self.svcs.get_mut(name) else {
            return Err(format!("Unit {name}.service not found."));
        };
        if s.child.is_some() {
            return Ok(());
        }
        s.restart_at = None;
        if !Self::start_allowed(s, now) {
            s.state = State::Failed;
            s.result = "start-limit-hit".into();
            self.jlog(name, "Start request repeated too quickly.");
            self.jlog(name, "Failed with result 'start-limit-hit'.");
            return Err(format!(
                "Job for {name}.service failed because start of the service was attempted too often."
            ));
        }
        let u = s.unit.clone();
        let out = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&journal)
            .map_err(|e| e.to_string())?;
        let err = out.try_clone().map_err(|e| e.to_string())?;
        let mut cmd = Command::new(&u.exec[0]);
        cmd.args(&u.exec[1..])
            .env_clear()
            .env(
                "PATH",
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
            )
            .envs(u.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            .current_dir(u.workdir.as_deref().unwrap_or("/"))
            .process_group(0);
        match cmd.spawn() {
            Ok(child) => {
                let pid = child.id();
                let _ = fs::write(
                    self.root.join("run").join(format!("{name}.pid")),
                    format!("{pid}\n"),
                );
                let s = self.svcs.get_mut(name).expect("unit exists");
                s.child = Some(child);
                s.state = State::Active;
                s.result = "success".into();
                self.jlog(name, &format!("Started (pid {pid}): {}", u.exec.join(" ")));
            }
            Err(e) => {
                self.jlog(name, &format!("Failed to execute {}: {e}", u.exec[0]));
                self.on_exit(name, ExitStatus::from_raw(203 << 8));
            }
        }
        Ok(())
    }

    fn on_exit(&mut self, name: &str, st: ExitStatus) {
        let (desc, clean) = describe(st);
        self.jlog(name, &format!("Main process exited, {desc}"));
        let s = self.svcs.get_mut(name).expect("unit exists");
        s.child = None;
        s.last_exit = desc;
        let again = match s.unit.restart {
            Restart::Always => true,
            Restart::OnFailure => !clean,
            Restart::No => false,
        };
        if clean {
            s.state = State::Inactive;
        } else {
            s.state = State::Failed;
            s.result = if st.code().is_some() {
                "exit-code"
            } else {
                "signal"
            }
            .into();
        }
        if !clean {
            let r = s.result.clone();
            self.jlog(name, &format!("Failed with result '{r}'."));
        }
        if again {
            let s = self.svcs.get_mut(name).expect("unit exists");
            s.state = State::AutoRestart;
            s.restart_at = Some(Instant::now() + s.unit.restart_sec);
        }
    }

    /// SIGTERM to the process group, wait up to TimeoutStopSec, then SIGKILL.
    fn stop(&mut self, name: &str) {
        let Some(s) = self.svcs.get_mut(name) else {
            return;
        };
        s.restart_at = None;
        let Some(mut child) = s.child.take() else {
            if s.state == State::AutoRestart {
                s.state = State::Inactive;
            }
            return;
        };
        let timeout = s.unit.timeout_stop;
        let pid = child.id();
        self.jlog(name, &format!("Stopping (SIGTERM to pid {pid})..."));
        signal_group(pid, SIGTERM);
        let t = Instant::now();
        let mut killed = false;
        let st = loop {
            match child.try_wait() {
                Ok(Some(st)) => break Some(st),
                Ok(None) if t.elapsed() < timeout => std::thread::sleep(Duration::from_millis(2)),
                Ok(None) if !killed => {
                    self.jlog(name, "State 'stop-sigterm' timed out. Killing.");
                    signal_group(pid, SIGKILL);
                    killed = true;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(2)),
                Err(_) => break None,
            }
        };
        let desc = st.map(|s| describe(s).0).unwrap_or_default();
        self.jlog(
            name,
            &format!("Stopped after {} ms ({desc}).", t.elapsed().as_millis()),
        );
        // The rest of the group (if any) goes too, like KillMode=control-group.
        signal_group(pid, SIGKILL);
        let s = self.svcs.get_mut(name).expect("unit exists");
        s.state = State::Inactive;
    }

    fn reap(&mut self) {
        let names: Vec<String> = self.svcs.keys().cloned().collect();
        for n in names {
            let st = match self.svcs.get_mut(&n).and_then(|s| s.child.as_mut()) {
                Some(c) => c.try_wait().ok().flatten(),
                None => None,
            };
            if let Some(st) = st {
                self.on_exit(&n, st);
            }
        }
        let now = Instant::now();
        let due: Vec<String> = self
            .svcs
            .iter()
            .filter(|(_, s)| s.restart_at.is_some_and(|t| t <= now))
            .map(|(k, _)| k.clone())
            .collect();
        for n in due {
            let c = {
                let s = self.svcs.get_mut(&n).expect("unit exists");
                s.n_restarts += 1;
                s.n_restarts
            };
            self.jlog(
                &n,
                &format!("Scheduled restart job, restart counter is at {c}."),
            );
            let _ = self.start(&n);
        }
    }

    fn handle(&mut self, line: &str) -> (i32, String) {
        let w: Vec<&str> = line.split_whitespace().collect();
        let verb = w.first().copied().unwrap_or("");
        let name = w
            .get(1)
            .map(|n| n.trim_end_matches(".service"))
            .unwrap_or("");
        let needs_unit = !matches!(verb, "status" | "daemon-reload" | "shutdown" | "");
        if needs_unit && !self.svcs.contains_key(name) {
            return (1, format!("Unit {name}.service could not be found.\n"));
        }
        match verb {
            "known" => (0, format!("{name}.service\n")),
            "exec-path" => (0, format!("{}\n", self.svcs[name].unit.exec[0])),
            "cat" => (0, self.svcs[name].unit.text.clone()),
            "is-active" => {
                let st = self.svcs[name].state;
                (
                    if st == State::Active { 0 } else { 3 },
                    format!("{}\n", st.name()),
                )
            }
            "show" => {
                let s = &self.svcs[name];
                let pid = s.child.as_ref().map(|c| c.id()).unwrap_or(0);
                (
                    0,
                    format!(
                        "Id={}.service\nActiveState={}\nResult={}\nMainPID={pid}\nNRestarts={}\nLastExit={}\nStartLimitWindowStarts={}\nIgnoredKeys={}\n",
                        s.unit.name,
                        s.state.name(),
                        s.result,
                        s.n_restarts,
                        s.last_exit,
                        s.rl_num,
                        s.unit.ignored.join(",")
                    ),
                )
            }
            "start" => match self.start(name) {
                Ok(()) => (0, String::new()),
                Err(e) => (1, e + "\n"),
            },
            "stop" => {
                self.stop(name);
                (0, String::new())
            }
            "restart" => {
                self.stop(name);
                match self.start(name) {
                    Ok(()) => (0, String::new()),
                    Err(e) => (1, e + "\n"),
                }
            }
            "reset-failed" => {
                let s = self.svcs.get_mut(name).expect("unit exists");
                s.rl_begin = None;
                s.rl_num = 0;
                if s.state == State::Failed {
                    s.state = State::Inactive;
                }
                s.result = "success".into();
                (0, String::new())
            }
            "journal" => {
                let n: usize = w.get(2).and_then(|v| v.parse().ok()).unwrap_or(20);
                let text = fs::read_to_string(self.journal(name)).unwrap_or_default();
                let lines: Vec<&str> = text.lines().collect();
                let from = lines.len().saturating_sub(n);
                let mut out = lines[from..].join("\n");
                if !out.is_empty() {
                    out.push('\n');
                }
                (0, out)
            }
            "status" => {
                let mut out = String::new();
                for (k, s) in &self.svcs {
                    let pid = s.child.as_ref().map(|c| c.id()).unwrap_or(0);
                    out += &format!(
                        "{k}.service {} result={} pid={pid} restarts={} exec={}\n",
                        s.state.name(),
                        s.result,
                        s.n_restarts,
                        s.unit.exec.join(" ")
                    );
                }
                (0, out)
            }
            "daemon-reload" => match self.load() {
                Ok(n) => (0, format!("reloaded {} unit(s)\n", n.len())),
                Err(e) => (1, e + "\n"),
            },
            "shutdown" => {
                self.quit = true;
                (0, "shutting down\n".into())
            }
            other => (2, format!("unknown verb {other:?}\n")),
        }
    }

    fn serve_one(&mut self, mut c: UnixStream) {
        let _ = c.set_nonblocking(false);
        let _ = c.set_read_timeout(Some(Duration::from_secs(2)));
        let mut buf = Vec::new();
        let mut b = [0u8; 512];
        while !buf.contains(&b'\n') && buf.len() < 4096 {
            match c.read(&mut b) {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&b[..n]),
            }
        }
        let line = String::from_utf8_lossy(&buf).to_string();
        let (code, out) = self.handle(line.trim());
        let _ = c.write_all(format!("{code}\n{out}").as_bytes());
    }

    pub fn run(&mut self) -> Result<(), String> {
        let sock = self.root.join("ctl.sock");
        let _ = fs::remove_file(&sock);
        let l = UnixListener::bind(&sock).map_err(|e| format!("{}: {e}", sock.display()))?;
        l.set_nonblocking(true).map_err(|e| e.to_string())?;
        let _ = fs::write(
            self.root.join("run/simsup.pid"),
            format!("{}\n", std::process::id()),
        );
        ev(&format!(
            "simsup {} up, root {}",
            env!("CARGO_PKG_VERSION"),
            self.root.display()
        ));
        let names: Vec<String> = self.svcs.keys().cloned().collect();
        for n in names {
            let _ = self.start(&n);
        }
        while !self.quit {
            loop {
                match l.accept() {
                    Ok((c, _)) => self.serve_one(c),
                    Err(_) => break,
                }
                if self.quit {
                    break;
                }
            }
            self.reap();
            std::thread::sleep(Duration::from_millis(20));
        }
        let names: Vec<String> = self.svcs.keys().cloned().collect();
        for n in names {
            self.stop(&n);
        }
        let _ = fs::remove_file(&sock);
        ev("simsup down");
        Ok(())
    }
}

/// Client side: send one request, print the answer, return its exit code.
pub fn ctl(root: &Path, args: &[String]) -> i32 {
    let sock = root.join("ctl.sock");
    let mut c = match UnixStream::connect(&sock) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("simsup ctl: {}: {e}", sock.display());
            return 4;
        }
    };
    let _ = c.set_read_timeout(Some(Duration::from_secs(300)));
    if c.write_all(format!("{}\n", args.join(" ")).as_bytes())
        .is_err()
    {
        return 4;
    }
    let mut out = String::new();
    let _ = c.read_to_string(&mut out);
    let (code, body) = out.split_once('\n').unwrap_or(("4", ""));
    let code: i32 = code.trim().parse().unwrap_or(4);
    if code == 0 || args.first().map(String::as_str) == Some("is-active") {
        print!("{body}");
    } else {
        eprint!("{body}");
    }
    code
}

/// Used by tests: write a unit file.
#[cfg(test)]
pub fn write_unit(root: &Path, name: &str, text: &str) -> std::io::Result<()> {
    fs::create_dir_all(root.join("units"))?;
    let mut f = fs::File::create(root.join("units").join(format!("{name}.service")))?;
    f.write_all(text.as_bytes())
}
