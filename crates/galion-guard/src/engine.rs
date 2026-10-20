// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Engine: log line -> filter -> sessions -> jail -> firewall + state.
//! Synchronous and clock-injected, so the daemon, `replay` and the tests all
//! run the same code.

use std::net::IpAddr;
use std::path::PathBuf;

use serde::Serialize;

use crate::filter::SshdFilter;
use crate::jail::{Action, Ban, FailResult, Jail, Protected};
use crate::nft::Backend;
use crate::session::{Outcome, Sessions};
use crate::source::LogLine;
use crate::state::{self, FilePos, Load, State};
use crate::timefmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum Event {
    Ban {
        ip: IpAddr,
        at: i64,
        until: i64,
        count: u32,
        failures: u32,
        manual: bool,
    },
    Unban {
        ip: IpAddr,
        at: i64,
        reason: String,
    },
    /// Address withheld on purpose: protected addresses are the owner's.
    Protected {
        at: i64,
        reason: Protected,
    },
    Unresolved {
        at: i64,
        rule: usize,
    },
    BackendError {
        at: i64,
        error: String,
    },
}

pub struct Engine {
    pub filter: SshdFilter,
    pub sessions: Sessions,
    pub jail: Jail,
    pub backend: Box<dyn Backend>,
    pub state_path: Option<PathBuf>,
    pub cursor: Option<String>,
    pub file_pos: Option<FilePos>,
    pub bsd_offset: i64,
    pub save_every: i64,
    pub backend_ready: bool,
    /// Keep a copy of every event (replay, tests).
    pub record: bool,
    pub events: Vec<Event>,
    /// Print events to stderr (daemon).
    pub verbose: bool,
    pub lines_seen: u64,
    pub lines_matched: u64,
    dirty: bool,
    last_save: i64,
    last_setup_try: i64,
    last_prune: i64,
}

pub fn log(msg: &str) {
    eprintln!("{} galion-guard: {msg}", zagreb_time::now_rfc3339());
}

impl Engine {
    pub fn new(jail: Jail, backend: Box<dyn Backend>, state_path: Option<PathBuf>) -> Self {
        Engine {
            filter: SshdFilter::new(),
            sessions: Sessions::new(),
            jail,
            backend,
            state_path,
            cursor: None,
            file_pos: None,
            bsd_offset: 0,
            save_every: 30,
            backend_ready: false,
            record: false,
            events: Vec::new(),
            verbose: false,
            lines_seen: 0,
            lines_matched: 0,
            dirty: false,
            last_save: 0,
            last_setup_try: i64::MIN / 2,
            last_prune: 0,
        }
    }

    fn emit(&mut self, e: Event) {
        if self.verbose {
            let msg = match &e {
                Event::Ban {
                    ip,
                    until,
                    count,
                    failures,
                    manual,
                    ..
                } => format!(
                    "BAN {ip} until {} (ban #{count}, {failures} failures{})",
                    zagreb_time::from_unix(*until),
                    if *manual { ", manual" } else { "" }
                ),
                Event::Unban { ip, reason, .. } => format!("UNBAN {ip} ({reason})"),
                Event::Protected { reason, .. } => {
                    format!("failure from a protected address ignored ({reason:?})")
                }
                Event::Unresolved { rule, .. } => {
                    format!("failure with a non-IP host ignored (rule {rule}, no DNS lookups)")
                }
                Event::BackendError { error, .. } => format!("firewall error: {error}"),
            };
            log(&msg);
        }
        if self.record {
            self.events.push(e);
        }
    }

    /// Load persisted state, set up the firewall and re-apply live bans.
    pub fn startup(&mut self, now: i64) -> Result<(), String> {
        if let Some(p) = self.state_path.clone() {
            match state::load(&p).map_err(|e| format!("state {}: {e}", p.display()))? {
                Load::Fresh => {}
                Load::Corrupt { moved_to, error } => log(&format!(
                    "state unreadable ({error}); moved to {} and starting with no bans",
                    moved_to.display()
                )),
                Load::Loaded(st) => {
                    let st = *st;
                    self.cursor = st.journal_cursor;
                    self.file_pos = st.file_pos;
                    let acts = self
                        .jail
                        .restore(st.bans, st.history, st.accepted, st.totals, now);
                    for a in acts {
                        if let Action::Unban { ip, reason } = a {
                            self.emit(Event::Unban {
                                ip,
                                at: now,
                                reason: reason.into(),
                            });
                        }
                    }
                    if self.verbose {
                        log(&format!(
                            "restored {} active bans",
                            self.jail.bans().count()
                        ));
                    }
                }
            }
        }
        self.try_setup(now);
        self.dirty = true;
        self.save(now);
        Ok(())
    }

    fn try_setup(&mut self, now: i64) {
        self.last_setup_try = now;
        match self.backend.setup() {
            Ok(()) => {
                self.backend_ready = true;
                self.sync(now);
            }
            Err(e) => {
                self.backend_ready = false;
                self.emit(Event::BackendError {
                    at: now,
                    error: format!("setup: {e}"),
                });
            }
        }
    }

    /// Make the firewall match the jail: re-add live bans with their
    /// remaining time, remove elements we do not know (fail open).
    pub fn sync(&mut self, now: i64) {
        let bans: Vec<Ban> = self.jail.bans().cloned().collect();
        for b in &bans {
            let left = (b.end - now).max(1) as u64;
            if let Err(e) = self.backend.ban(b.ip, left) {
                self.emit(Event::BackendError { at: now, error: e });
            }
        }
        match self.backend.list() {
            Ok(elems) => {
                for (ip, _) in elems {
                    if self.jail.ban_of(ip).is_none() {
                        if let Err(e) = self.backend.unban(ip) {
                            self.emit(Event::BackendError { at: now, error: e });
                        } else {
                            self.emit(Event::Unban {
                                ip,
                                at: now,
                                reason: "not-in-state".into(),
                            });
                        }
                    }
                }
            }
            Err(e) => self.emit(Event::BackendError {
                at: now,
                error: format!("list: {e}"),
            }),
        }
    }

    fn fw_ban(&mut self, b: &Ban, now: i64) {
        if !self.backend_ready {
            return; // applied by sync() once the backend is up
        }
        let left = (b.end - now).max(1) as u64;
        if let Err(e) = self.backend.ban(b.ip, left) {
            self.emit(Event::BackendError { at: now, error: e });
        }
    }

    fn fw_unban(&mut self, ip: IpAddr, now: i64) {
        if !self.backend_ready {
            return;
        }
        if let Err(e) = self.backend.unban(ip) {
            self.emit(Event::BackendError { at: now, error: e });
        }
    }

    /// Process one log entry at wall-clock `now`.
    pub fn handle_line(&mut self, line: &LogLine, now: i64) {
        self.lines_seen += 1;
        if let Some(c) = &line.cursor {
            self.cursor = Some(c.clone());
            self.dirty = true;
        }
        if let Some(p) = &line.file_pos {
            self.file_pos = Some(p.clone());
            self.dirty = true;
        }
        let (t, text) = match line.time {
            Some(t) => (t, line.text.as_str()),
            None => match timefmt::split_stamp(&line.text, now, self.bsd_offset) {
                Some((t, rest)) => (t, rest),
                None => return,
            },
        };
        let Some(m) = self.filter.matches(text) else {
            return;
        };
        self.lines_matched += 1;
        match self.sessions.feed(&m, t) {
            Outcome::Failure { ip, .. } => match self.jail.on_failure(ip, t, now) {
                FailResult::Banned(b) => {
                    self.fw_ban(&b, now);
                    self.emit(Event::Ban {
                        ip: b.ip,
                        at: now,
                        until: b.end,
                        count: b.count,
                        failures: b.failures,
                        manual: false,
                    });
                    self.dirty = true;
                    self.save(now);
                    self.last_save = now;
                }
                FailResult::Protected(reason) => self.emit(Event::Protected { at: now, reason }),
                _ => self.dirty = true,
            },
            Outcome::Accepted { ip } => {
                if let Some(Action::Unban { ip, reason }) = self.jail.on_accepted(ip, t, now) {
                    self.fw_unban(ip, now);
                    self.emit(Event::Unban {
                        ip,
                        at: now,
                        reason: reason.into(),
                    });
                    self.save(now);
                }
                self.dirty = true;
            }
            Outcome::Unresolved { rule, .. } => self.emit(Event::Unresolved { at: now, rule }),
            Outcome::Nothing => {}
        }
    }

    /// Periodic work: expiry, retrying the firewall, pruning, saving.
    pub fn tick(&mut self, now: i64) {
        for a in self.jail.tick(now) {
            if let Action::Unban { ip, reason } = a {
                self.fw_unban(ip, now);
                self.emit(Event::Unban {
                    ip,
                    at: now,
                    reason: reason.into(),
                });
                self.dirty = true;
            }
        }
        if !self.backend_ready && now - self.last_setup_try >= 30 {
            self.try_setup(now);
        }
        if now - self.last_prune >= 60 {
            self.last_prune = now;
            self.jail.prune(now);
            self.sessions.gc(now);
        }
        if self.dirty && now - self.last_save >= self.save_every {
            self.save(now);
        }
    }

    pub fn manual_ban(&mut self, ip: IpAddr, dur: Option<u64>, now: i64) -> Result<Ban, String> {
        match self.jail.manual_ban(ip, now, dur) {
            Ok(b) => {
                self.fw_ban(&b, now);
                self.emit(Event::Ban {
                    ip: b.ip,
                    at: now,
                    until: b.end,
                    count: b.count,
                    failures: 0,
                    manual: true,
                });
                self.dirty = true;
                self.save(now);
                Ok(b)
            }
            Err(p) => Err(format!("refused: address is protected ({p:?})")),
        }
    }

    pub fn unban(&mut self, ip: IpAddr, now: i64) -> bool {
        let had = self.jail.unban(ip).is_some();
        // Always clear the firewall too (operator intent: let it in).
        self.fw_unban(crate::ipnet::canonical(ip), now);
        if had {
            self.emit(Event::Unban {
                ip,
                at: now,
                reason: "operator".into(),
            });
            self.dirty = true;
            self.save(now);
        }
        had
    }

    pub fn unban_all(&mut self, now: i64) -> usize {
        let ips: Vec<IpAddr> = self.jail.bans().map(|b| b.ip).collect();
        for ip in &ips {
            self.unban(*ip, now);
        }
        ips.len()
    }

    pub fn snapshot(&self, now: i64) -> State {
        State {
            version: state::VERSION,
            saved_at: now,
            bans: self.jail.bans().cloned().collect(),
            history: self.jail.history().cloned().collect(),
            accepted: self.jail.accepted().map(|(a, t)| (*a, *t)).collect(),
            totals: self.jail.totals.clone(),
            journal_cursor: self.cursor.clone(),
            file_pos: self.file_pos.clone(),
        }
    }

    pub fn save(&mut self, now: i64) {
        let Some(p) = self.state_path.clone() else {
            self.dirty = false;
            return;
        };
        match state::save(&p, &self.snapshot(now)) {
            Ok(()) => {
                self.dirty = false;
                self.last_save = now;
            }
            Err(e) => log(&format!("state save {} failed: {e}", p.display())),
        }
    }

    pub fn status_json(&self, now: i64) -> serde_json::Value {
        let bans: Vec<serde_json::Value> = self
            .jail
            .bans()
            .map(|b| {
                serde_json::json!({
                    "ip": b.ip.to_string(),
                    "since": zagreb_time::from_unix(b.start),
                    "until": zagreb_time::from_unix(b.end),
                    "remaining_s": (b.end - now).max(0),
                    "ban_number": b.count,
                    "failures": b.failures,
                    "manual": b.manual,
                })
            })
            .collect();
        serde_json::json!({
            "jail": "sshd",
            "now": zagreb_time::from_unix(now),
            "currently_failed": self.jail.currently_failed(),
            "total_failed": self.jail.totals.failed,
            "currently_banned": bans.len(),
            "total_banned": self.jail.totals.banned,
            "protected_hits": self.jail.totals.protected_hits,
            "backend": self.backend.name(),
            "backend_ready": self.backend_ready,
            "allowlist_entries": self.jail.protect.allow.len(),
            "self_addresses": self.jail.protect.self_addrs.len(),
            "lines_seen": self.lines_seen,
            "lines_matched": self.lines_matched,
            "params": {
                "maxretry": self.jail.params.maxretry,
                "findtime_s": self.jail.params.findtime,
                "bantime_s": self.jail.params.bantime,
                "bantime_increment": self.jail.params.increment,
                "bantime_maxtime_s": self.jail.params.maxtime,
                "protect_accepted_s": self.jail.params.protect_accepted,
            },
            "bans": bans,
        })
    }

    pub fn status_text(&self, now: i64) -> String {
        let j = &self.jail;
        let mut s = String::new();
        s += "Status for the jail: sshd (galion-guard)\n";
        s += &format!(
            "|- Filter\n|  |- Currently failed:\t{}\n",
            j.currently_failed()
        );
        s += &format!("|  |- Total failed:\t{}\n", j.totals.failed);
        s += &format!(
            "|  `- Lines seen/matched:\t{}/{}\n",
            self.lines_seen, self.lines_matched
        );
        let bans: Vec<&Ban> = j.bans().collect();
        s += &format!(
            "`- Actions ({}{})\n   |- Currently banned:\t{}\n",
            self.backend.name(),
            if self.backend_ready {
                ""
            } else {
                ", NOT READY"
            },
            bans.len()
        );
        s += &format!("   |- Total banned:\t{}\n", j.totals.banned);
        s += &format!("   |- Protected hits:\t{}\n", j.totals.protected_hits);
        s += "   `- Banned IP list:\n";
        for b in bans {
            s += &format!(
                "      {}  until {}  ({} left, ban #{}{})\n",
                b.ip,
                zagreb_time::from_unix(b.end),
                crate::duration::format((b.end - now).max(0) as u64),
                b.count,
                if b.manual { ", manual" } else { "" }
            );
        }
        s
    }
}
