// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Model providers behind one trait.
//!
//! The planned wiring is OpenRouter: the boss on the latest Opus with
//! reasoning, each sub-agent on the latest Sonnet in its own session.
//! OpenRouter answers 402 (credit) and Slavik has not approved spend, so
//! [`OpenRouter`] refuses every call without touching the network, and the
//! only provider that runs is the offline [`MockProvider`].

use crate::retry::StopKind;
use serde::Serialize;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

/// Everything a sub-agent session is told. Rendered with [`Brief::render`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Brief {
    pub task: String,
    pub sector: String,
    pub lead: String,
    pub agent: String,
    pub session: String,
    pub model: String,
    pub attempt: u32,
    pub slots: u32,
    pub title: String,
    pub deps: Vec<String>,
}

/// The rules every brief repeats (RULES.md, WAVE2.md, TEACHER/BOSS.md).
pub const BRIEF_RULES: &str = "\
HARD RULES: Rust only, never run Python or Node (first shell command: export PATH=/workspace/swarm/bin:$PATH). \
Never read or print secrets. Never touch /opt/galion/openmind or any live VPS service; VPS access is read-only GETs. \
No paid API calls, no Revolut, no real money. Work only on your own swarm/* branch; never push to main, never force-push, no PRs. \
Times are Europe/Zagreb with an explicit offset. New files carry the SPDX MIT header. HADES admin safety overrides everything.";

impl Brief {
    /// The self-contained text a fresh sub-agent session receives.
    pub fn render(&self) -> String {
        format!(
            "DESK-ID: hermes.{task}\nAGENT: {agent} (session {session}, attempt {attempt}, {slots} slot(s), model {model})\n\
             REPORTS TO: {lead} -> hermes\nGOAL: {title}\nDEPENDS ON (done): {deps}\n{rules}\n\
             REPORT FORMAT:\nTICKET: {task} | RESULT: done|blocked\nFILES: <paths>\nCOMMANDS RUN: <command + exit code + 1-3 lines>\n\
             ACCEPTANCE: <each check ok/fail + evidence>\nNOTES: <anything needing Slavik>\n",
            task = self.task,
            agent = self.agent,
            session = self.session,
            attempt = self.attempt,
            slots = self.slots,
            model = self.model,
            lead = self.lead,
            title = self.title,
            deps = if self.deps.is_empty() {
                "none".to_string()
            } else {
                self.deps.join(", ")
            },
            rules = BRIEF_RULES,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunOutcome {
    Completed { summary: String },
    Stopped(StopKind),
}

pub trait Provider: Send + Sync {
    fn name(&self) -> &'static str;
    /// Run one sub-agent session to its end.
    fn run(&self, brief: &Brief) -> RunOutcome;
}

/// The planned provider. Refuses every call; never opens a socket.
#[derive(Debug, Default)]
pub struct OpenRouter;

impl Provider for OpenRouter {
    fn name(&self) -> &'static str {
        "openrouter"
    }
    fn run(&self, _brief: &Brief) -> RunOutcome {
        RunOutcome::Stopped(StopKind::Error(
            "openrouter is disabled: credit returns 402 and spend is not approved; no call was made"
                .into(),
        ))
    }
}

/// Offline stand-in for the platform. It models the one behaviour Hermes
/// must survive: when more agents are live than the platform limit, new
/// sessions are stopped ("request limit"). Optionally every n-th session
/// stalls, to exercise the stall path. No network, no model, no spend.
#[derive(Debug)]
pub struct MockProvider {
    limit: usize,
    work: Duration,
    stall_every: u64,
    active: AtomicUsize,
    peak: AtomicUsize,
    sessions: AtomicU64,
}

impl MockProvider {
    /// `limit` concurrent agent slots before the mock stops new sessions.
    pub fn new(limit: usize, work: Duration) -> MockProvider {
        MockProvider {
            limit: limit.max(1),
            work,
            stall_every: 0,
            active: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            sessions: AtomicU64::new(0),
        }
    }

    /// Every `n`-th session (1-based) stalls; 0 turns stalls off.
    pub fn with_stall_every(mut self, n: u64) -> Self {
        self.stall_every = n;
        self
    }

    pub fn peak(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }

    pub fn sessions(&self) -> u64 {
        self.sessions.load(Ordering::SeqCst)
    }
}

impl Provider for MockProvider {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn run(&self, brief: &Brief) -> RunOutcome {
        let n = self.sessions.fetch_add(1, Ordering::SeqCst) + 1;
        let slots = brief.slots.max(1) as usize;
        let now = self.active.fetch_add(slots, Ordering::SeqCst) + slots;
        self.peak.fetch_max(now, Ordering::SeqCst);
        let outcome = if now > self.limit {
            // The platform answers the first requests, then cuts the session.
            std::thread::sleep(self.work / 4);
            RunOutcome::Stopped(StopKind::RequestLimit)
        } else if self.stall_every > 0 && n.is_multiple_of(self.stall_every) {
            std::thread::sleep(self.work / 2);
            RunOutcome::Stopped(StopKind::Stall)
        } else {
            std::thread::sleep(self.work);
            RunOutcome::Completed {
                summary: format!("mock: {} done by {}", brief.task, brief.agent),
            }
        };
        self.active.fetch_sub(slots, Ordering::SeqCst);
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brief(slots: u32) -> Brief {
        Brief {
            task: "gaia:S1.1".into(),
            sector: "gaia".into(),
            lead: "gaia.lead".into(),
            agent: "gaia.S1.1.a1".into(),
            session: "s-1".into(),
            model: "m".into(),
            attempt: 1,
            slots,
            title: "Finish lan-routing".into(),
            deps: vec![],
        }
    }

    #[test]
    fn openrouter_never_runs() {
        assert!(matches!(
            OpenRouter.run(&brief(1)),
            RunOutcome::Stopped(StopKind::Error(e)) if e.contains("402")
        ));
    }

    #[test]
    fn mock_stops_sessions_over_its_limit() {
        let m = std::sync::Arc::new(MockProvider::new(2, Duration::from_millis(40)));
        let start = std::sync::Arc::new(std::sync::Barrier::new(4));
        let hs: Vec<_> = (0..4)
            .map(|_| {
                let m = m.clone();
                let start = start.clone();
                std::thread::spawn(move || {
                    start.wait();
                    m.run(&brief(1))
                })
            })
            .collect();
        let outs: Vec<_> = hs.into_iter().map(|h| h.join().unwrap()).collect();
        let stopped = outs
            .iter()
            .filter(|o| matches!(o, RunOutcome::Stopped(StopKind::RequestLimit)))
            .count();
        assert!(stopped >= 1, "{outs:?}");
        assert!(m.peak() > 2);
    }

    #[test]
    fn brief_is_self_contained() {
        let text = brief(2).render();
        for must in [
            "DESK-ID: hermes.gaia:S1.1",
            "Rust only",
            "never push to main",
            "REPORT FORMAT",
        ] {
            assert!(text.contains(must), "{must}");
        }
    }
}
