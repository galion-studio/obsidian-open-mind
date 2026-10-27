// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Minimal Chrome DevTools Protocol client over `--remote-debugging-pipe`:
//! Chrome reads commands on fd 3 and writes replies/events on fd 4, each message
//! is JSON terminated by a NUL byte. No websocket, no HTTP, no extra crates.

use serde_json::{json, Value};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

pub struct Browser {
    child: Child,
    tx: File,
    rx: Receiver<Value>,
    next: u64,
    /// Events received while waiting for a reply, in arrival order.
    pub events: Vec<Value>,
    profile: PathBuf,
}

fn pipe_hi() -> std::io::Result<(RawFd, RawFd)> {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: fds is a valid 2-element buffer.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // Move both ends above fd 10 so dup2 onto 3/4 in the child never clobbers them.
    let mut hi = [0; 2];
    for i in 0..2 {
        // SAFETY: fds[i] is an open descriptor we own.
        let n = unsafe { libc::fcntl(fds[i], libc::F_DUPFD_CLOEXEC, 10) };
        unsafe { libc::close(fds[i]) };
        if n < 0 {
            return Err(std::io::Error::last_os_error());
        }
        hi[i] = n;
    }
    Ok((hi[0], hi[1]))
}

pub fn launch_args(profile: &Path) -> Vec<String> {
    vec![
        "--headless".into(),
        "--remote-debugging-pipe".into(),
        // Playwright's default (chromiumSandbox: false) and needed as root on the VPS.
        "--no-sandbox".into(),
        format!("--user-data-dir={}", profile.display()),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--disable-gpu".into(),
        "--disable-extensions".into(),
        "--disable-background-networking".into(),
        "--disable-component-update".into(),
        "--disable-sync".into(),
        "--hide-scrollbars".into(),
        "--mute-audio".into(),
        "about:blank".into(),
    ]
}

impl Browser {
    pub fn launch(chrome: &Path) -> Result<Browser, String> {
        let profile = std::env::temp_dir().join(format!(
            "pw-check-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&profile).map_err(|e| e.to_string())?;
        let (c_read, p_write) = pipe_hi().map_err(|e| e.to_string())?; // parent -> chrome (fd 3)
        let (p_read, c_write) = pipe_hi().map_err(|e| e.to_string())?; // chrome -> parent (fd 4)
        let mut cmd = Command::new(chrome);
        cmd.args(launch_args(&profile))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: only async-signal-safe dup2 calls between fork and exec.
        unsafe {
            cmd.pre_exec(move || {
                if libc::dup2(c_read, 3) < 0 || libc::dup2(c_write, 4) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let spawned = cmd.spawn();
        // SAFETY: the child ends are ours; the child has its own copies on 3/4.
        unsafe {
            libc::close(c_read);
            libc::close(c_write);
        }
        let child = spawned.map_err(|e| format!("cannot start {}: {e}", chrome.display()))?;
        // SAFETY: p_write/p_read are open descriptors owned only by these Files.
        let tx = unsafe { File::from_raw_fd(p_write) };
        let mut rd = unsafe { File::from_raw_fd(p_read) };
        let (stx, rx) = channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 65536];
            loop {
                match rd.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
                while let Some(i) = buf.iter().position(|&b| b == 0) {
                    let msg: Vec<u8> = buf.drain(..=i).collect();
                    if let Ok(v) = serde_json::from_slice::<Value>(&msg[..msg.len() - 1]) {
                        if stx.send(v).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        Ok(Browser {
            child,
            tx,
            rx,
            next: 0,
            events: Vec::new(),
            profile,
        })
    }

    /// Send one command and wait for its reply (`result`), keeping events.
    pub fn call(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: Value,
        limit: Duration,
    ) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        let mut msg = json!({"id": id, "method": method, "params": params});
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        let mut bytes = serde_json::to_vec(&msg).map_err(|e| e.to_string())?;
        bytes.push(0);
        self.tx
            .write_all(&bytes)
            .map_err(|e| format!("{method}: browser pipe closed: {e}"))?;
        let t0 = Instant::now();
        loop {
            let left = limit.saturating_sub(t0.elapsed());
            match self.rx.recv_timeout(left) {
                Ok(v) if v.get("id").and_then(Value::as_u64) == Some(id) => {
                    if let Some(e) = v.get("error") {
                        return Err(format!("{method}: {}", e.get("message").unwrap_or(e)));
                    }
                    return Ok(v.get("result").cloned().unwrap_or(Value::Null));
                }
                Ok(v) => {
                    if v.get("method").is_some() {
                        self.events.push(v);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!(
                        "{method}: Timeout {}ms exceeded.",
                        limit.as_millis()
                    ))
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(format!("{method}: browser closed"))
                }
            }
        }
    }

    /// Wait until an event matching `pred` arrives (already-buffered events count).
    pub fn wait_event(
        &mut self,
        pred: &dyn Fn(&Value) -> bool,
        limit: Duration,
    ) -> Result<Value, String> {
        if let Some(v) = self.events.iter().find(|v| pred(v)) {
            return Ok(v.clone());
        }
        let t0 = Instant::now();
        loop {
            let left = limit.saturating_sub(t0.elapsed());
            match self.rx.recv_timeout(left) {
                Ok(v) => {
                    let hit = pred(&v);
                    if v.get("method").is_some() {
                        self.events.push(v.clone());
                    }
                    if hit {
                        return Ok(v);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!("Timeout {}ms exceeded.", limit.as_millis()))
                }
                Err(RecvTimeoutError::Disconnected) => return Err("browser closed".into()),
            }
        }
    }

    /// Drain events that are already queued (non-blocking).
    pub fn pump(&mut self) {
        while let Ok(v) = self.rx.try_recv() {
            if v.get("method").is_some() {
                self.events.push(v);
            }
        }
    }

    pub fn close(self) {
        // Drop does the graceful Browser.close, the wait and the cleanup.
        drop(self);
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            // Graceful first so Chromium's helper processes exit and stop writing
            // into the profile dir; SIGKILL only if it does not go within 5 s.
            self.next += 1;
            let mut msg = serde_json::to_vec(
                &json!({"id": self.next, "method": "Browser.close", "params": {}}),
            )
            .unwrap_or_default();
            msg.push(0);
            let _ = self.tx.write_all(&msg);
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_secs(5) {
                if let Ok(Some(_)) = self.child.try_wait() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if let Ok(None) = self.child.try_wait() {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
        for _ in 0..10 {
            let _ = std::fs::remove_dir_all(&self.profile);
            std::thread::sleep(Duration::from_millis(100));
            if !self.profile.exists() {
                break;
            }
        }
    }
}
