// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! The running daemon: source thread, control socket thread, main loop.

use std::io::{BufRead, BufReader, Write};
use std::net::IpAddr;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use crate::config::{self, Effective};
use crate::engine::{log, Engine};
use crate::jail::{Jail, Protect};
use crate::nft::{Backend, DryRun, Nft, NftConfig};
use crate::source::{journal_args, FileTail, Journal, LogLine};
use crate::state::{now_secs, FilePos};

pub enum Msg {
    Line(LogLine),
    Control(String, Sender<String>),
}

pub fn nft_config(e: &Effective) -> NftConfig {
    NftConfig {
        bin: e.cfg.guard.nft.clone(),
        table: e.cfg.guard.table.clone(),
        ports: e.cfg.sshd.ports.clone(),
        allow: e.allow.clone(),
        blocktype: e.cfg.sshd.blocktype.clone(),
    }
}

pub fn backend_for(e: &Effective) -> Result<Box<dyn Backend>, String> {
    let nc = nft_config(e);
    Ok(match e.cfg.guard.backend.as_str() {
        "dry-run" => Box::new(DryRun::new(Some(nc), e.cfg.guard.dry_run_log.clone())),
        _ => Box::new(Nft::new(nc)?),
    })
}

pub fn build_engine(e: &Effective) -> Result<Engine, String> {
    let mut protect = Protect {
        allow: e.allow.clone(),
        self_addrs: Vec::new(),
    };
    if e.cfg.guard.ignoreself {
        protect.self_addrs = config::self_addresses();
    }
    let jail = Jail::new(e.params.clone(), protect);
    let mut eng = Engine::new(jail, backend_for(e)?, Some(e.cfg.guard.state.clone()));
    eng.bsd_offset = e.cfg.sshd.bsd_utc_offset_minutes * 60;
    eng.save_every = e.cfg.guard.save_every.max(1) as i64;
    eng.verbose = true;
    Ok(eng)
}

fn spawn_journal(e: &Effective, cursor: Option<String>, tx: Sender<Msg>) {
    let bin = e.cfg.sshd.journalctl.clone();
    let matches = e.cfg.sshd.journal_match.clone();
    let findtime = e.params.findtime as i64;
    std::thread::spawn(move || {
        let mut cursor = cursor;
        loop {
            let args = journal_args(&matches, cursor.as_deref(), now_secs() - findtime);
            match Journal::spawn(&bin, &args) {
                Ok(mut j) => loop {
                    match j.next_line() {
                        Ok(Some(l)) => {
                            if l.cursor.is_some() {
                                cursor = l.cursor.clone();
                            }
                            if tx.send(Msg::Line(l)).is_err() {
                                return;
                            }
                        }
                        Ok(None) => {
                            log("journalctl exited; restarting in 5 s");
                            break;
                        }
                        Err(err) => {
                            log(&format!("journal read error: {err}; restarting in 5 s"));
                            break;
                        }
                    }
                },
                Err(err) => log(&format!(
                    "cannot start {}: {err}; retry in 5 s",
                    bin.display()
                )),
            }
            std::thread::sleep(Duration::from_secs(5));
        }
    });
}

fn spawn_file(path: PathBuf, resume: Option<FilePos>, poll_ms: u64, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let mut tail = FileTail::new(path.clone(), resume);
        let mut warned = false;
        loop {
            match tail.poll() {
                Ok(lines) => {
                    warned = false;
                    for l in lines {
                        if tx.send(Msg::Line(l)).is_err() {
                            return;
                        }
                    }
                }
                Err(e) => {
                    if !warned {
                        log(&format!("read {}: {e}", path.display()));
                        warned = true;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(poll_ms.max(50)));
        }
    });
}

fn bind_socket(path: &Path) -> std::io::Result<UnixListener> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    if path.exists() {
        // Refuse to steal the socket of a live daemon.
        if UnixStream::connect(path).is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "another galion-guard is running on this socket",
            ));
        }
        std::fs::remove_file(path)?;
    }
    let l = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(l)
}

fn spawn_control(listener: UnixListener, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut s) = conn else { continue };
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
            let mut line = String::new();
            {
                let mut r = BufReader::new(&s).take(4096);
                if r.read_line(&mut line).is_err() {
                    continue;
                }
            }
            let (rtx, rrx) = mpsc::channel();
            if tx.send(Msg::Control(line.trim().to_string(), rtx)).is_err() {
                return;
            }
            let reply = rrx
                .recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|_| "error: daemon busy\n".into());
            let _ = s.write_all(reply.as_bytes());
        }
    });
}

use std::io::Read as _;

/// Handle one control request; `None` means "shut down after replying".
pub fn control(eng: &mut Engine, req: &str, now: i64) -> (String, bool) {
    let mut it = req.split_whitespace();
    let cmd = it.next().unwrap_or("");
    let arg = it.next();
    let arg2 = it.next();
    match (cmd, arg) {
        ("status", Some("json")) => (format!("{}\n", eng.status_json(now)), false),
        ("status", _) => (eng.status_text(now), false),
        ("unban", Some("--all")) => (format!("unbanned {}\n", eng.unban_all(now)), false),
        ("unban", Some(a)) => match a.parse::<IpAddr>() {
            Ok(ip) => {
                let had = eng.unban(ip, now);
                (
                    if had {
                        format!("unbanned {ip}\n")
                    } else {
                        format!("{ip} was not banned (firewall element cleared anyway)\n")
                    },
                    false,
                )
            }
            Err(_) => ("error: not an IP address\n".into(), false),
        },
        ("ban", Some(a)) => {
            let Ok(ip) = a.parse::<IpAddr>() else {
                return ("error: not an IP address\n".into(), false);
            };
            let dur = match arg2.map(crate::duration::parse) {
                None => None,
                Some(Ok(d)) if d > 0 => Some(d),
                Some(_) => return ("error: bad duration\n".into(), false),
            };
            match eng.manual_ban(ip, dur, now) {
                Ok(b) => (
                    format!("banned {ip} until {}\n", zagreb_time::from_unix(b.end)),
                    false,
                ),
                Err(e) => (format!("error: {e}\n"), false),
            }
        }
        ("stop", _) => {
            eng.save(now);
            ("stopping (state saved)\n".into(), true)
        }
        _ => (
            "error: commands: status [json] | unban <ip>|--all | ban <ip> [duration] | stop\n"
                .into(),
            false,
        ),
    }
}

pub fn run(e: Effective) -> Result<(), String> {
    let mut eng = build_engine(&e)?;
    let now = now_secs();
    log(&format!(
        "starting: source={} backend={} maxretry={} findtime={}s bantime={}s increment={} allowlist={} entries, self={} addresses",
        e.cfg.sshd.source,
        eng.backend.name(),
        e.params.maxretry,
        e.params.findtime,
        e.params.bantime,
        e.params.increment,
        eng.jail.protect.allow.len(),
        eng.jail.protect.self_addrs.len()
    ));
    let listener = bind_socket(&e.cfg.guard.socket)
        .map_err(|err| format!("control socket {}: {err}", e.cfg.guard.socket.display()))?;
    eng.startup(now)?;
    let (tx, rx): (Sender<Msg>, Receiver<Msg>) = mpsc::channel();
    match e.cfg.sshd.source.as_str() {
        "file" => spawn_file(
            e.cfg.sshd.logpath.clone(),
            eng.file_pos.clone(),
            e.cfg.sshd.poll_ms,
            tx.clone(),
        ),
        _ => spawn_journal(&e, eng.cursor.clone(), tx.clone()),
    }
    spawn_control(listener, tx);
    log("running");
    let mut last_tick = 0;
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Msg::Line(l)) => eng.handle_line(&l, now_secs()),
            Ok(Msg::Control(req, reply)) => {
                let (out, stop) = control(&mut eng, &req, now_secs());
                let _ = reply.send(out);
                if stop {
                    // Give the control thread a moment to write the reply.
                    std::thread::sleep(Duration::from_millis(200));
                    let _ = std::fs::remove_file(&e.cfg.guard.socket);
                    log("stopped by control request (bans stay in the kernel with their timeouts)");
                    return Ok(());
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err("all sources ended".into()),
        }
        let n = now_secs();
        if n != last_tick {
            last_tick = n;
            eng.tick(n);
        }
    }
}

/// Client side of the control socket.
pub fn request(socket: &Path, req: &str) -> Result<String, String> {
    let mut s = UnixStream::connect(socket)
        .map_err(|e| format!("cannot reach galion-guard at {}: {e}", socket.display()))?;
    s.set_read_timeout(Some(Duration::from_secs(15))).ok();
    s.write_all(format!("{req}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    let mut out = String::new();
    s.read_to_string(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}
