// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Slot 311 (D-269-1 prep): the fold / .sh-retirement gate.
//! Reads `journalctl -u galion-backup.service -o short-iso` text and splits
//! it into runs. A run starts at systemd's `Starting galion-backup.service`
//! and ends at systemd's `Finished galion-backup.service`
//! (success) or `Failed to start galion-backup.service` (failure; `Failed with
//! result` / a non-zero `Main process exited` mark the run failed).
//! A run is CLEAN when it finished, the Rust ELF logged `msg="ok"`, and
//! neither the ELF nor the offsite hook logged `level=error` / `level=warn`.
//! A run counts as a TIMER run when it started in the timer window
//! (`OnCalendar=*-*-* 00/6:17:00` + `RandomizedDelaySec=5min`, VPS clock is
//! Etc/UTC, journal offset `+00:00`): hour % 6 == 0 and minute 17..=23.
//! Manual starts (e.g. the t273u deploy smoke) are reported but never count.
//! The gate is open at >= 12 CONSECUTIVE clean timer runs (any unclean timer
//! run resets the streak) = the 3-day retention window.
#![allow(dead_code)]

pub const GATE_RUNS: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// short-iso timestamp of the first line of the run.
    pub start: String,
    pub finished: bool,
    pub failed: bool,
    pub rust_ok: bool,
    pub bad_lines: usize,
    pub timer: bool,
}

impl Run {
    pub fn clean(&self) -> bool {
        self.finished && !self.failed && self.rust_ok && self.bad_lines == 0
    }
}

/// `2026-10-27T18:19:03+00:00` -> Some((18, 19)) only for a +00:00 offset.
pub fn utc_hm(ts: &str) -> Option<(u32, u32)> {
    if ts.len() < 25 || &ts[19..25] != "+00:00" {
        return None;
    }
    let h = ts.get(11..13)?.parse().ok()?;
    let m = ts.get(14..16)?.parse().ok()?;
    Some((h, m))
}

pub fn in_timer_window(ts: &str) -> bool {
    matches!(utc_hm(ts), Some((h, m)) if h % 6 == 0 && (17..=23).contains(&m))
}

pub fn runs(journal: &str, rust_tag: &str) -> Vec<Run> {
    let mut out = Vec::new();
    let mut cur: Option<Run> = None;
    for l in journal.lines() {
        let ts = l.split_whitespace().next().unwrap_or("");
        let sd = l.contains(" systemd[");
        if sd && l.contains("Starting galion-backup.service") {
            // A run that never reached Finished/Failed (journal gap, reboot)
            // is kept as unfinished = unclean.
            if let Some(prev) = cur.take() {
                out.push(prev);
            }
            cur = Some(Run {
                start: ts.to_string(),
                finished: false,
                failed: false,
                rust_ok: false,
                bad_lines: 0,
                timer: in_timer_window(ts),
            });
            continue;
        }
        // Lines outside a run (`Consumed ... CPU time`, `-- Boot` markers,
        // pre-window tails) are ignored.
        let Some(r) = cur.as_mut() else { continue };
        if l.contains(rust_tag) && l.contains("msg=\"ok\"") {
            r.rust_ok = true;
        }
        if (l.contains(rust_tag) || l.contains("galion-offsite-encrypt"))
            && (l.contains("level=error") || l.contains("level=warn"))
        {
            r.bad_lines += 1;
        }
        let fin = sd && l.contains("Finished galion-backup.service");
        let fail_mark = sd
            && (l.contains("galion-backup.service: Failed with result")
                || (l.contains("galion-backup.service: Main process exited")
                    && !l.contains("status=0/SUCCESS")));
        let fail_end = sd && l.contains("Failed to start galion-backup.service");
        if fail_mark || fail_end {
            r.failed = true;
        }
        if fin || fail_end {
            r.finished = fin;
            out.push(cur.take().unwrap());
        }
    }
    // A trailing run still in progress is not counted.
    out
}

#[derive(Debug, PartialEq, Eq)]
pub struct Gate {
    pub timer_runs: usize,
    pub manual_runs: usize,
    pub unclean_timer_runs: usize,
    pub streak: usize,
}

impl Gate {
    pub fn open(&self) -> bool {
        self.streak >= GATE_RUNS
    }
}

pub fn gate(runs: &[Run]) -> Gate {
    let mut g = Gate {
        timer_runs: 0,
        manual_runs: 0,
        unclean_timer_runs: 0,
        streak: 0,
    };
    for r in runs {
        if !r.timer {
            g.manual_runs += 1;
            continue;
        }
        g.timer_runs += 1;
        if r.clean() {
            g.streak += 1;
        } else {
            g.unclean_timer_runs += 1;
            g.streak = 0;
        }
    }
    g
}
