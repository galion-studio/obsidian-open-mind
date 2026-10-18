// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Plain supervisor: the non-systemd backend (Termux/Android, Alpine/OpenRC
//! boxes, containers, minimal Linux). Same boot.toml as the unit renderer.
//! Starts each service without a shell, appends its output to
//! `<log_dir>/<name>.log`, restarts it per `restart`/`restart_sec` with a
//! start limit (10 starts in 300 s, then a 300 s pause), runs timer services
//! every `timer_every_s`, and honours restart requests written by
//! galion-healer (`<supervisor_dir>/restart/<name>`). It only ever signals
//! its own children. No signal handler (std only): stop it with its
//! process group, e.g. `kill -- -<pgid>` or the Termux session.

use crate::config::{File, Service};
use std::collections::VecDeque;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const LIMIT_BURST: usize = 10;
pub const LIMIT_WINDOW: Duration = Duration::from_secs(300);

/// Parse an env file: `KEY=VALUE`, `export KEY=VALUE`, `#` comments, quotes.
pub fn parse_env_file(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let l = l.strip_prefix("export ").unwrap_or(l);
            let (k, v) = l.split_once('=')?;
            let k = k.trim();
            if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return None;
            }
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|x| x.strip_suffix('"'))
                .or_else(|| v.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
                .unwrap_or(v);
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

pub struct Slot {
    pub svc: Service,
    pub child: Option<Child>,
    pub starts: VecDeque<Instant>,
    pub next_start: Instant,
    pub done: bool,
}

pub struct Supervisor {
    pub slots: Vec<Slot>,
    pub log_dir: std::path::PathBuf,
    pub dir: std::path::PathBuf,
    pub events: Vec<String>,
}

impl Supervisor {
    pub fn new(f: &File) -> Supervisor {
        let now = Instant::now();
        Supervisor {
            slots: f
                .service
                .iter()
                .filter(|s| s.supervise)
                .map(|s| Slot {
                    svc: s.clone(),
                    child: None,
                    starts: VecDeque::new(),
                    next_start: now,
                    done: false,
                })
                .collect(),
            log_dir: f.boot.log_dir.clone(),
            dir: f.boot.supervisor_dir.clone(),
            events: Vec::new(),
        }
    }

    fn note(&mut self, line: String) {
        let l = format!("{} {line}", zagreb_time::now_rfc3339());
        println!("{l}");
        self.events.push(l);
    }

    fn spawn(log_dir: &Path, s: &Service) -> Result<Child, String> {
        std::fs::create_dir_all(log_dir).map_err(|e| e.to_string())?;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_dir.join(format!("{}.log", s.name)))
            .map_err(|e| e.to_string())?;
        let err = log.try_clone().map_err(|e| e.to_string())?;
        let mut c = Command::new(&s.argv[0]);
        c.args(&s.argv[1..])
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(err);
        if let Some(w) = &s.workdir {
            c.current_dir(w);
        }
        for f in &s.env_files {
            let t = std::fs::read_to_string(f).map_err(|e| format!("env file {f}: {e}"))?;
            for (k, v) in parse_env_file(&t) {
                c.env(k, v);
            }
        }
        for e in &s.env {
            if let Some((k, v)) = e.split_once('=') {
                c.env(k, v);
            }
        }
        c.spawn().map_err(|e| e.to_string())
    }

    /// One pass: reap, honour restart requests, start what is due.
    pub fn tick(&mut self, now: Instant) {
        let req_dir = self.dir.join("restart");
        let pid_dir = self.dir.join("pids");
        let _ = std::fs::create_dir_all(&pid_dir);
        let mut notes = Vec::new();
        for slot in &mut self.slots {
            let name = slot.svc.name.clone();
            let req = req_dir.join(&name);
            if req.exists() {
                let _ = std::fs::remove_file(&req);
                if let Some(c) = slot.child.as_mut() {
                    let _ = c.kill();
                    let _ = c.wait();
                    slot.child = None;
                }
                slot.done = false;
                slot.next_start = now;
                notes.push(format!("{name} restart requested"));
            }
            if let Some(c) = slot.child.as_mut() {
                if let Ok(Some(st)) = c.try_wait() {
                    slot.child = None;
                    let _ = std::fs::remove_file(pid_dir.join(&name));
                    notes.push(format!("{name} exited {st}"));
                    let again = match (slot.svc.timer_every_s, slot.svc.restart.as_str()) {
                        (Some(e), _) => Some(Duration::from_secs(e)),
                        (None, "always") => Some(Duration::from_secs(slot.svc.restart_sec)),
                        (None, "on-failure") if !st.success() => {
                            Some(Duration::from_secs(slot.svc.restart_sec))
                        }
                        _ => None,
                    };
                    match again {
                        Some(d) => slot.next_start = now + d,
                        None => slot.done = true,
                    }
                }
            }
            if slot.child.is_none() && !slot.done && now >= slot.next_start {
                while slot
                    .starts
                    .front()
                    .is_some_and(|t| now.duration_since(*t) > LIMIT_WINDOW)
                {
                    slot.starts.pop_front();
                }
                if slot.starts.len() >= LIMIT_BURST && slot.svc.timer_every_s.is_none() {
                    slot.next_start = now + LIMIT_WINDOW;
                    notes.push(format!("{name} start limit hit, pausing 300 s"));
                    continue;
                }
                slot.starts.push_back(now);
                match Self::spawn(&self.log_dir, &slot.svc) {
                    Ok(c) => {
                        let _ = std::fs::write(pid_dir.join(&name), c.id().to_string());
                        notes.push(format!("{name} started pid {}", c.id()));
                        slot.child = Some(c);
                    }
                    Err(e) => {
                        slot.next_start = now + Duration::from_secs(slot.svc.restart_sec.max(1));
                        notes.push(format!("{name} start failed: {e}"));
                    }
                }
            }
        }
        for n in notes {
            self.note(n);
        }
    }

    /// Stop every child (used by tests and `--once`).
    pub fn stop_all(&mut self) {
        for s in &mut self.slots {
            if let Some(c) = s.child.as_mut() {
                let _ = c.kill();
                let _ = c.wait();
            }
            s.child = None;
        }
    }
}
