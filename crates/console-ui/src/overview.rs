// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Facts for the Desk landing (`/console`): the whole-project overview.
//!
//! This is the only place the overview's words and numbers live; the page in
//! `pages/overview.rs` only lays them out. To update the landing, edit the
//! constants below (no HTML here). Every fact cites where it came from:
//!
//! - `CALL-RECORD.md`: /workspace/swarm/CALL-RECORD.md (Slavik call record,
//!   2026-10-20, up to the wave-3 launch at 22:52 CEST +02:00)
//! - `plans/<sector>.md`: /workspace/swarm/plans/*.md (sector planners,
//!   "Totals" tables)
//! - `swarm-settings.toml`: config/swarm/swarm-settings.toml on
//!   swarm/settings-snapshot aab8d85
//! - `SLAVIK-TODO.md`: docs/SLAVIK-TODO.md on swarm/slavik-todo 87fdd91

/// When these facts were last checked.
pub const AS_OF: &str = "2026-10-20 22:52 CEST (+02:00)";

pub const KICKER: &str = "PROJECT 42 · Gaia · Galion Studio";

/// Vision headline.
pub const HEADLINE: &str = "Discovering the universe together with humans.";

pub const LEDE: &str = "One Rust platform: Gaia is the body, Hermes coordinates, the swarm builds it, and Slavik decides. \
This page is the whole project on one screen: what exists, where it lives, and what is waiting for the owner.";

/// The top-priority rule (`swarm-settings.toml` `[priority_rules]`, rank 1).
pub struct Rule {
    pub text: &'static str,
    pub meaning: &'static str,
    pub set: &'static str,
}

pub const RULE: Rule = Rule {
    text: "Slavik values good company and productive conversations: the kind where you leave with more than you came in with.",
    meaning: "Every interaction (voice, chat, agent report, UI) must leave Slavik with more than he came in with: real substance, a useful next step or insight, no filler, no wasted turns.",
    set: "Rank 1 · set by Slavik 2026-10-20 20:56 CEST (+02:00) · applies to all software, agents, personas, reports and conversations",
};

/// Big numbers across the top.
pub struct Stat {
    pub value: &'static str,
    pub label: &'static str,
}

pub const STATS: &[Stat] = &[
    Stat {
        value: "9",
        label: "Gaia functions",
    },
    Stat {
        value: "52",
        label: "Gaia systems",
    },
    Stat {
        value: "46",
        label: "crates in the ownership registry",
    },
    Stat {
        value: "7",
        label: "sectors, one planner each",
    },
    Stat {
        value: "200",
        label: "planned tasks in 47 sessions",
    },
    Stat {
        value: "18–20",
        label: "agents that hold at once (measured)",
    },
];

/// How far a sector is, as one word for the badge colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Running in production.
    Live,
    /// Built and tested on a branch; not live.
    Built,
    /// Work in progress.
    Building,
    /// Waiting on the owner or failing a gate.
    Blocked,
}

impl State {
    pub fn word(self) -> &'static str {
        match self {
            State::Live => "live",
            State::Built => "built, not live",
            State::Building => "in progress",
            State::Blocked => "waiting",
        }
    }
}

/// One card on the sector map.
pub struct Sector {
    pub id: &'static str,
    pub name: &'static str,
    pub role: &'static str,
    pub state: State,
    /// Short status line next to the badge.
    pub status: &'static str,
    pub facts: &'static [&'static str],
    /// (branch, short SHA); SHA may be empty for branches still running.
    pub refs: &'static [(&'static str, &'static str)],
}

pub const SECTORS: &[Sector] = &[
    Sector {
        id: "gaia",
        name: "Gaia",
        role: "The body: 9 functions and 52 systems that run, heal and guard the platform.",
        state: State::Built,
        status: "landed on a branch, none live",
        facts: &[
            "HADES 5 · MINERVA 6 · APOLLO 5 · HEPHAESTUS 8 · ARTEMIS 6 · POSEIDON 4 · AETHER 9 · DEMETER 6 · ELEUTHIA 3 = 52 systems",
            "HADES approval gate: single-use Ed25519 tokens; 627 tests on gaia-hades-tokens f3f41a2",
            "POSEIDON queue + adapter: real test 60/60, 639 tests on gaia-poseidon 498e3d2",
            "Ownership registry maps all 46 crates + 16 VPS units; a test fails on unmapped crates",
            "Open: POSEIDON vs HEPHAESTUS ownership of compute (owner decision)",
        ],
        refs: &[("swarm/gaia-land", "9d3ec18")],
    },
    Sector {
        id: "hermes",
        name: "Hermes",
        role: "Boss operator, research, and coordinator of the 7 sectors.",
        state: State::Blocked,
        status: "readiness 11/15, not ready",
        facts: &[
            "Coordinator: sector registry, task queue, hash-chained ledger, governor cap 16",
            "Retry backoff 30 → 600 s, 8 tries; model calls mocked",
            "HADES gate: merge always refused; deploy and destructive steps need Slavik's signature",
            "Failing gates: G9 HADES admin key, G11 OpenRouter 402, G12 branches not on main, G14 7 open decisions",
            "hermes-rs live on 8642 since 2026-10-18 22:00 CEST",
        ],
        refs: &[("swarm/hermes-core", "45b7980")],
    },
    Sector {
        id: "console",
        name: "Console",
        role: "Rust replacement for the Node/Vite operator console: server-rendered HTML, no JavaScript.",
        state: State::Building,
        status: "31 of 31 routes in Rust on console-port-merge",
        facts: &[
            "Strict CSP, server-side phone gate, ~2.9 MB RSS",
            "Live 8080 is still the Vite dev server (~1.23 GiB RSS) and serves /package.json and src/**",
            "The 17 rooms left at c242fed are ported in 5 slices (console-port-1..5), merged here",
            "With an SPA fallback /arena, /bank and /hermes stay SPA-first (writes held for owner rulings)",
            "Mic + streaming parts of Hermes still need a ruling on browser JS",
        ],
        refs: &[
            ("swarm/console-ui-rust", "c242fed"),
            ("swarm/landing-overview", "669dc18"),
            ("swarm/console-port-merge", ""),
        ],
    },
    Sector {
        id: "voice",
        name: "Voice · Galion One",
        role: "The mouth and the ear. Voice: Samantha.",
        state: State::Built,
        status: "speaks and hears end to end, not deployed",
        facts: &[
            "Chatterbox rewritten in Rust (candle); Whisper large-v3-turbo ear",
            "CPU: mouth 25–34 s for 2.4–3.4 s of speech; ear 12.4–14.0 s",
            "Voice clone similarity 0.87–0.91 (was 0.46–0.60)",
        ],
        refs: &[("swarm/voice-rust", "03d21a9")],
    },
    Sector {
        id: "nerve",
        name: "Nerve + cortex",
        role: "The senses: probes, incidents and real-time awareness of vulnerabilities, bleeding and DDoS.",
        state: State::Building,
        status: "replay of live capture: 61/100 (D)",
        facts: &[
            "Live galion-nerve is an Oct 18 build and misses ~59% of tunnel failures",
            "Cortex module (585 lines) replay-tested only, not live",
            "Cutover must start in dry-run; nerve-wiring still running",
        ],
        refs: &[("swarm/nerve", "5802d0e")],
    },
    Sector {
        id: "vps",
        name: "VPS operations",
        role: "One production machine, one service cutover at a time.",
        state: State::Live,
        status: "frozen: read-only until cutovers are approved",
        facts: &[
            "Working copy /opt/galion/openmind stays at 2f5a730",
            "Deploy rehearsal on the box: tuner 21/0, nerve 18/0 checks; swaps 2–5 ms, auto-rollback ~20 s",
            "Strays without a unit: 8431–8433, 8652, 8653",
        ],
        refs: &[("swarm/deploy-sim", "c5ab8f9")],
    },
    Sector {
        id: "integration",
        name: "Integration · CI",
        role: "Merging the branches into one line the owner can land on main.",
        state: State::Built,
        status: "20/20 wave-1 branches merged, not on main",
        facts: &[
            "950 workspace tests pass, 0 fail; realtest stack 76/77",
            "main is 13a296c; integration-2 (Gaia merge) running in wave 3",
        ],
        refs: &[("swarm/integration-1", "8fd26f6")],
    },
    Sector {
        id: "security",
        name: "Security",
        role: "Fuzzing, SAST, HADES admin safety, SEC-1..5, launch gates.",
        state: State::Blocked,
        status: "runbooks ready; execution is the owner's",
        facts: &[
            "SEC-1..5 runbooks written, not executed",
            "galion-guard (Rust fail2ban): 102/102 same bans as live on a 2-day replay (d5f7874)",
            "HADES adversarial review S3.1 running in wave 3",
        ],
        refs: &[("swarm/sec-runbooks", "c5fc269")],
    },
    Sector {
        id: "docs",
        name: "Docs · white paper",
        role: "The record of the full-capacity swarm run: only measured facts, every number cited.",
        state: State::Building,
        status: "evidence being collected",
        facts: &[
            "Run started 2026-10-20 20:34 CEST; launch deadline Oct 31",
            "whitepaper-evidence and swarm-record running in wave 3",
        ],
        refs: &[("swarm/launch-gates", "79ee761")],
    },
];

/// One sector planner's totals (`plans/<sector>.md`, "Totals").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub sector: &'static str,
    pub sessions: u32,
    pub tasks: u32,
    pub target: u32,
    pub useful: u32,
}

pub const PLANS: &[Plan] = &[
    Plan {
        sector: "gaia",
        sessions: 8,
        tasks: 31,
        target: 77,
        useful: 61,
    },
    Plan {
        sector: "nerve",
        sessions: 6,
        tasks: 20,
        target: 55,
        useful: 52,
    },
    Plan {
        sector: "cortex",
        sessions: 6,
        tasks: 39,
        target: 119,
        useful: 87,
    },
    Plan {
        sector: "vps-ops",
        sessions: 7,
        tasks: 28,
        target: 72,
        useful: 44,
    },
    Plan {
        sector: "integration-ci",
        sessions: 6,
        tasks: 27,
        target: 74,
        useful: 60,
    },
    Plan {
        sector: "security",
        sessions: 9,
        tasks: 33,
        target: 166,
        useful: 69,
    },
    Plan {
        sector: "docs-whitepaper",
        sessions: 5,
        tasks: 22,
        target: 82,
        useful: 34,
    },
];

/// Final planner totals as recorded (CALL-RECORD.md 20:46); a test checks
/// they equal the sum of [`PLANS`].
pub const PLAN_TOTAL: Plan = Plan {
    sector: "total",
    sessions: 47,
    tasks: 200,
    target: 645,
    useful: 407,
};

/// The automated process (`swarm-settings.toml` `[limits]`, CALL-RECORD.md).
pub struct Process {
    pub ceiling: u32,
    pub auto_accept: bool,
    pub measured_lo: u32,
    pub measured_hi: u32,
    pub governor: u32,
    pub wave3_running: u32,
    pub retry: &'static str,
    pub notes: &'static [&'static str],
}

pub const PROCESS: Process = Process {
    ceiling: 250,
    auto_accept: true,
    measured_lo: 18,
    measured_hi: 20,
    governor: 16,
    wave3_running: 17,
    retry: "stopped by the request limit → back in the queue → restarted until done (30 → 600 s backoff)",
    notes: &[
        "20:14 burst of 15 new agents: 11 stopped by the request limit",
        "20:34 restart to 16 at once: all kept running",
        "20:38 22 running: 4 stopped, 18 kept running",
        "22:52 wave 3 launched at 17, under the measured limit",
    ],
};

// The process numbers must stay coherent; checked when the crate compiles.
const _: () = assert!(PROCESS.measured_lo <= PROCESS.measured_hi);
const _: () = assert!(PROCESS.governor <= PROCESS.measured_lo);
const _: () = assert!(PROCESS.ceiling > PROCESS.measured_hi);

/// Pending owner items (SLAVIK-TODO.md).
pub struct OwnerItem {
    pub title: &'static str,
    pub done: bool,
    pub detail: &'static str,
    pub refs: &'static str,
}

pub const OWNER_ITEMS: &[OwnerItem] = &[
    OwnerItem {
        title: "HADES admin key",
        done: false,
        detail: "Make the key on your own device; only the public half goes to the server. Unblocks Hermes gate G9.",
        refs: "docs/SLAVIK-TODO.md item 1",
    },
    OwnerItem {
        title: "OpenRouter credit",
        done: true,
        detail: "50 USD added 2026-10-20 22:35 CEST (reported by Mate, not re-verified). Was HTTP 402.",
        refs: "docs/SLAVIK-TODO.md item 2",
    },
    OwnerItem {
        title: "Strict HTTP handling approval",
        done: false,
        detail: "Approve H1–H3, M1–M3, D1–D4, R1–R4, C1–C5. Each service can be reverted on its own.",
        refs: "docs/approvals/common-migrate-2-strict.md on swarm/common-migrate-2 123eab2",
    },
];

pub const SOURCES: &[&str] = &[
    "CALL-RECORD.md (Slavik call record, 2026-10-20)",
    "plans/*.md (7 sector plans, Totals tables)",
    "config/swarm/swarm-settings.toml on swarm/settings-snapshot aab8d85",
    "docs/SLAVIK-TODO.md on swarm/slavik-todo 87fdd91",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_totals_add_up() {
        let sum = PLANS.iter().fold((0, 0, 0, 0), |a, p| {
            (
                a.0 + p.sessions,
                a.1 + p.tasks,
                a.2 + p.target,
                a.3 + p.useful,
            )
        });
        assert_eq!(
            sum,
            (
                PLAN_TOTAL.sessions,
                PLAN_TOTAL.tasks,
                PLAN_TOTAL.target,
                PLAN_TOTAL.useful
            )
        );
        assert_eq!(PLANS.len(), 7);
        for p in PLANS {
            assert!(p.useful <= p.target, "{}", p.sector);
        }
    }

    #[test]
    fn facts_are_consistent() {
        let gaia = &SECTORS[0];
        let total: u32 = gaia.facts[0]
            .split(" = ")
            .next()
            .unwrap()
            .split(" · ")
            .map(|s| s.rsplit(' ').next().unwrap().parse::<u32>().unwrap())
            .sum();
        assert_eq!(total, 52);
        let ids: Vec<_> = SECTORS.iter().map(|s| s.id).collect();
        let mut d = ids.clone();
        d.sort();
        d.dedup();
        assert_eq!(d.len(), ids.len());
        // No UTC in the facts (RULES.md 7).
        for s in SECTORS {
            for f in s.facts {
                assert!(!f.contains("UTC"), "{f}");
            }
        }
    }
}
