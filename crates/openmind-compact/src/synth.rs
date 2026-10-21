// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Deterministic synthetic agent transcripts with planted facts, for the
//! benchmark and the property tests. Shapes follow real Hermes/OpenMind
//! sessions: a long system prompt, user tasks, assistant tool calls, and tool
//! results that are mostly journal tails (heavily repetitive heartbeats),
//! JSON status blobs, `cargo test` output, source files in fenced code, and
//! prose search results. Each fact is planted once, as one line inside an
//! older message, and later questions ask for it.

use crate::{Message, Role};

/// SplitMix64: tiny, deterministic, good enough for test data.
#[derive(Clone)]
pub struct Rng(pub u64);

impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        crate::text::finish(self.0)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
    pub fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len())]
    }
    pub fn chance(&mut self, pct: usize) -> bool {
        self.below(100) < pct
    }
}

#[derive(Clone, Debug)]
pub struct OwnedMsg {
    pub role: Role,
    pub content: String,
    pub tool_call_id: Option<String>,
    pub tool_calls: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Fact {
    pub question: String,
    /// A word that must appear on the same line as the answer.
    pub subject: String,
    pub answer: String,
    pub msg: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Transcript {
    pub msgs: Vec<OwnedMsg>,
    pub facts: Vec<Fact>,
}

impl Transcript {
    pub fn messages(&self) -> Vec<Message<'_>> {
        self.msgs
            .iter()
            .map(|m| Message {
                role: m.role,
                content: &m.content,
                tool_call_id: m.tool_call_id.as_deref(),
                tool_calls: m.tool_calls.iter().map(|s| s.as_str()).collect(),
            })
            .collect()
    }
    pub fn tokens(&self) -> usize {
        self.messages()
            .iter()
            .map(|m| 4 + crate::text::est_tokens(m.content))
            .sum()
    }
}

const WORDS: &[&str] = &[
    "nerve",
    "memory",
    "hermes",
    "gateway",
    "tunnel",
    "backup",
    "tuner",
    "cortex",
    "voice",
    "sandbox",
    "ledger",
    "queue",
    "persona",
    "deploy",
    "health",
    "relay",
    "origin",
    "guard",
    "vending",
    "survival",
    "discovery",
    "speech",
    "llama",
    "schema",
    "provider",
    "console",
];
const PEOPLE: &[&str] = &[
    "Slavik", "Mate", "Ivana", "Luka", "Petra", "Marko", "Ana", "Tomislav",
];
const VERBS: &[&str] = &[
    "checked",
    "restarted",
    "rebuilt",
    "probed",
    "measured",
    "compared",
    "patched",
    "reviewed",
    "deployed",
    "rolled back",
    "benchmarked",
    "inspected",
];
const PROSE: &[&str] = &[
    "The service reported healthy after the last restart and latency stayed flat.",
    "No regressions were observed in the p99 latency during the soak window.",
    "The dependency tree was audited and every licence is on the allow list.",
    "This change keeps the wire format identical so older clients keep working.",
    "We should keep the rollback path armed until the next maintenance window.",
    "The queue drained within the expected time and no messages were lost.",
    "Disk usage grew slowly, which matches the retention policy for logs.",
    "The configuration loader falls back to defaults when a key is missing.",
    "Memory growth was linear in the number of sessions, not in their length.",
    "The handshake retries with exponential backoff capped at thirty seconds.",
    "Static musl builds avoid glibc drift between the box and the server.",
    "The test corpus mixes real captures with synthetic worst cases.",
];
const CODE: &[&str] = &[
    "pub fn health(&self) -> Value {\n    json!({\"ok\": true, \"uptime\": self.uptime()})\n}",
    "fn backoff(n: u32) -> Duration {\n    Duration::from_millis(100u64 << n.min(8))\n}",
    "impl Drop for Guard {\n    fn drop(&mut self) {\n        self.release();\n    }\n}",
    "let cfg = Config::from_env()?;\nlet srv = Server::bind(cfg.addr)?;\nsrv.run()?;",
    "match req.path() {\n    \"/health\" => ok(),\n    _ => not_found(),\n}",
];

fn sha(r: &mut Rng) -> String {
    format!("{:07x}", r.next_u64() & 0xfff_ffff)
}

fn ts(r: &mut Rng, base: u64) -> String {
    let s = base + r.below(3600) as u64;
    format!(
        "2026-10-21T{:02}:{:02}:{:02}+02:00",
        (s / 3600) % 24,
        (s / 60) % 60,
        s % 60
    )
}

struct Gen {
    r: Rng,
    t: Transcript,
    call: usize,
    clock: u64,
}

impl Gen {
    fn push(&mut self, role: Role, content: String) -> usize {
        self.t.msgs.push(OwnedMsg {
            role,
            content,
            tool_call_id: None,
            tool_calls: Vec::new(),
        });
        self.t.msgs.len() - 1
    }

    fn journal(&mut self) -> String {
        let svc = *self.r.pick(WORDS);
        let n = 30 + self.r.below(60);
        let mut s = String::new();
        for _ in 0..n {
            let t = ts(&mut self.r, self.clock);
            let line = match self.r.below(10) {
                0..=6 => format!("{t} {svc}[812]: heartbeat ok queue=0 conns=3"),
                7 => format!("{t} {svc}[812]: GET /health 200 1ms"),
                8 => format!(
                    "{t} {svc}[812]: slow request {}ms on /v1/{}",
                    200 + self.r.below(900),
                    self.r.pick(WORDS)
                ),
                _ => format!("{t} systemd[1]: Started {svc}.service."),
            };
            s.push_str(&line);
            s.push('\n');
        }
        s
    }

    fn json(&mut self) -> String {
        let mut s = String::from("{\"status\":\"ok\",\"services\":[");
        for i in 0..(8 + self.r.below(12)) {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!(
                "{{\"name\":\"{}\",\"active\":true,\"restarts\":{},\"rss_kb\":{}}}",
                self.r.pick(WORDS),
                self.r.below(4),
                10_000 + self.r.below(90_000)
            ));
        }
        s.push_str("]}");
        s
    }

    fn cargo_test(&mut self) -> String {
        let n = 20 + self.r.below(40);
        let mut s = format!("running {n} tests\n");
        for _ in 0..n {
            s.push_str(&format!(
                "test {}::tests::{}_{} ... ok\n",
                self.r.pick(WORDS),
                self.r.pick(WORDS),
                self.r.pick(WORDS)
            ));
        }
        s.push_str(&format!(
            "\ntest result: ok. {n} passed; 0 failed; 0 ignored; finished in 0.{}s\n",
            self.r.below(99)
        ));
        s
    }

    fn code(&mut self) -> String {
        let mut s = format!("src/{}.rs:\n```rust\n", self.r.pick(WORDS));
        for _ in 0..(1 + self.r.below(3)) {
            s.push_str(self.r.pick(CODE));
            s.push_str("\n\n");
        }
        s.push_str("```\n");
        s
    }

    fn prose(&mut self, lines: usize) -> String {
        let mut s = String::new();
        for _ in 0..lines {
            s.push_str(self.r.pick(PROSE));
            s.push('\n');
        }
        s
    }

    fn tool_result(&mut self) -> String {
        match self.r.below(5) {
            0 | 1 => self.journal(),
            2 => self.json(),
            3 => self.cargo_test(),
            _ => {
                let p = 3 + self.r.below(6);
                let mut s = self.code();
                s.push_str(&self.prose(p));
                s
            }
        }
    }

    fn fact(&mut self, id: usize) -> (String, Fact) {
        let svc = format!("{}{}", self.r.pick(WORDS), id);
        let (line, q, a) = match self.r.below(5) {
            0 => {
                let a = sha(&mut self.r);
                (
                    format!("The deploy SHA for {svc} is {a}."),
                    format!("What is the deploy SHA for {svc}?"),
                    a,
                )
            }
            1 => {
                let a = format!("{}", 20000 + self.r.below(9000));
                (
                    format!("{svc} listens on port {a} on the box."),
                    format!("Which port does {svc} listen on?"),
                    a,
                )
            }
            2 => {
                let a = PEOPLE[self.r.below(PEOPLE.len())].to_string();
                (
                    format!("Ticket for {svc} is owned by {a} until Friday."),
                    format!("Who owns the ticket for {svc}?"),
                    a,
                )
            }
            3 => {
                let a = format!("{}", 5 + self.r.below(495));
                (
                    format!("The spend cap for {svc} is {a} EUR per day."),
                    format!("What is the daily spend cap for {svc}?"),
                    a,
                )
            }
            _ => {
                let a = format!("/var/backups/{svc}-{}.bin", sha(&mut self.r));
                (
                    format!("Backup of {svc} was written to {a} before the change."),
                    format!("Where was the backup of {svc} written?"),
                    a,
                )
            }
        };
        (
            line,
            Fact {
                question: q,
                subject: svc,
                answer: a,
                msg: 0,
            },
        )
    }
}

/// Insert `line` as a new line at a random line boundary of `s`.
fn plant(r: &mut Rng, s: &mut String, line: &str) {
    let bounds: Vec<usize> = s.match_indices('\n').map(|(i, _)| i + 1).collect();
    let at = if bounds.is_empty() {
        s.len()
    } else {
        bounds[r.below(bounds.len())]
    };
    let mut ins = String::with_capacity(line.len() + 1);
    if at == s.len() && !s.ends_with('\n') && !s.is_empty() {
        ins.push('\n');
    }
    ins.push_str(line);
    ins.push('\n');
    // never plant inside a fenced block: move to the end instead
    let fences = s[..at].matches("```").count();
    if fences % 2 == 1 {
        if !s.ends_with('\n') {
            s.push('\n');
        }
        s.push_str(line);
        s.push('\n');
    } else {
        s.insert_str(at, &ins);
    }
}

/// Generate a transcript of about `target_tokens` with `n_facts` planted facts
/// (none in the last two turns, so the anchors alone never answer them).
pub fn transcript(seed: u64, target_tokens: usize, n_facts: usize) -> Transcript {
    let mut g = Gen {
        r: Rng(seed),
        t: Transcript::default(),
        call: 0,
        clock: 8 * 3600,
    };
    let mut sys = String::from(
        "You are Hermes, the PROJECT 42 / Gaia operations agent. Times are Europe/Zagreb (+02:00).\n",
    );
    for w in WORDS {
        sys.push_str(&format!(
            "- tool `{w}_status`: read-only status of the {w} service; returns JSON.\n"
        ));
    }
    sys.push_str("Never print secrets. Never touch /opt/galion/openmind. Rust only.\n");
    g.push(Role::System, sys);
    while g.t.tokens() < target_tokens {
        let task = format!(
            "Please {} the {} service and tell me if anything changed since {}.",
            ["check", "inspect", "benchmark", "review"][g.r.below(4)],
            g.r.pick(WORDS),
            ts(&mut g.r, g.clock)
        );
        g.push(Role::User, task);
        let calls = 1 + g.r.below(3);
        let mut ids = Vec::new();
        for _ in 0..calls {
            g.call += 1;
            ids.push(format!("call_{:05}", g.call));
        }
        let pre = format!(
            "I {} the logs first; calling {} tool(s).",
            g.r.pick(VERBS),
            calls
        );
        let a = g.push(Role::Assistant, pre);
        g.t.msgs[a].tool_calls = ids.clone();
        for id in ids {
            let body = g.tool_result();
            let i = g.push(Role::Tool, body);
            g.t.msgs[i].tool_call_id = Some(id);
        }
        let p = 2 + g.r.below(4);
        let reply = g.prose(p);
        g.push(Role::Assistant, reply);
        g.clock += 600;
    }
    // final turn: an unrelated new task (query-agnostic case)
    g.push(
        Role::User,
        "Now summarise today's state of the cortex scoring work for Slavik.".into(),
    );
    // plant facts into random messages outside the last two turns
    let users: Vec<usize> = (0..g.t.msgs.len())
        .filter(|&i| g.t.msgs[i].role == Role::User)
        .collect();
    let limit = if users.len() >= 2 {
        users[users.len() - 2]
    } else {
        g.t.msgs.len()
    };
    for id in 0..n_facts {
        let (line, mut f) = g.fact(id);
        let mi = 1 + g.r.below(limit.saturating_sub(1).max(1));
        let mut c = std::mem::take(&mut g.t.msgs[mi].content);
        let mut r = g.r.clone();
        plant(&mut r, &mut c, &line);
        g.r = r;
        g.t.msgs[mi].content = c;
        f.msg = mi;
        g.t.facts.push(f);
    }
    g.t
}

/// Exact-match fact retention: the answer and the subject appear together on
/// one line of `text`.
pub fn retained(text: &str, f: &Fact) -> bool {
    text.lines()
        .any(|l| l.contains(&f.answer) && contains_word(l, &f.subject))
}

fn contains_word(l: &str, w: &str) -> bool {
    l.match_indices(w).any(|(i, _)| {
        let after = l[i + w.len()..].chars().next();
        !matches!(after, Some(c) if c.is_ascii_digit())
    })
}
