// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Unit and property tests (hand-rolled, deterministic generators: no
//! proptest dependency).

use openmind_compact::synth::{self, Rng};
use openmind_compact::text::est_tokens;
use openmind_compact::{
    compact_text, split_units, Compacted, Compactor, Config, GapCompressor, Message, Rescorer, Role,
};

fn cfg(budget: usize) -> Config {
    Config {
        budget_tokens: budget,
        ..Default::default()
    }
}

/// Every output line is (part of) a source line, a gap marker, or the stub.
fn check_faithful(msgs: &[Message], out: &Compacted, c: &Config) {
    let mut last_src = None;
    for m in &out.messages {
        assert!(last_src < Some(m.src), "order preserved, no repeats");
        last_src = Some(m.src);
        let src = msgs[m.src].content;
        assert_eq!(m.role, msgs[m.src].role);
        if m.content == src {
            continue;
        }
        for line in m.content.lines() {
            for piece in line.split(&format!(" {} ", c.gap_marker)) {
                let p = piece.trim();
                assert!(
                    p.is_empty() || p == c.gap_marker || p == c.stub || src.contains(p),
                    "line not from source: {p:?}"
                );
            }
        }
    }
}

fn check_tool_pairs(out: &Compacted) {
    let calls: Vec<&str> = out
        .messages
        .iter()
        .flat_map(|m| m.tool_calls.iter().copied())
        .collect();
    let results: Vec<&str> = out.messages.iter().filter_map(|m| m.tool_call_id).collect();
    for r in &results {
        assert!(calls.contains(r), "tool result {r} without its call");
    }
    for c in &calls {
        assert!(results.contains(c), "tool call {c} without its result");
    }
}

#[test]
fn prop_budget_order_pairs_faithful_determinism() {
    let mut r = Rng(42);
    for case in 0..40 {
        let size = 2_000 + r.below(60_000);
        let t = synth::transcript(1000 + case, size, 10);
        let msgs = t.messages();
        let budget = (t.tokens() / (2 + r.below(25))).max(300);
        let c = cfg(budget);
        let mut comp = Compactor::new(c.clone());
        let a = comp.compact(&msgs, None);
        if !a.stats.over_budget {
            assert!(
                a.stats.output_tokens <= budget,
                "case {case}: {} > {budget}",
                a.stats.output_tokens
            );
        }
        check_faithful(&msgs, &a, &c);
        check_tool_pairs(&a);
        // determinism: reused scratch and a fresh compactor agree
        let b = comp.compact(&msgs, None);
        let f = Compactor::new(c.clone()).compact(&msgs, None);
        assert_eq!(a.to_text(), b.to_text());
        assert_eq!(a.to_text(), f.to_text());
        assert_eq!(a.stats, f.stats);
        // system message always verbatim
        assert_eq!(a.messages[0].content, msgs[0].content);
        assert_eq!(a.messages[0].role, Role::System);
    }
}

#[test]
fn prop_anchors_last_turns_verbatim_when_they_fit() {
    for seed in 0..10u64 {
        let t = synth::transcript(seed, 30_000, 5);
        let msgs = t.messages();
        let c = cfg(8_000);
        let out = Compactor::new(c).compact(&msgs, None);
        assert!(out.stats.kept_turns >= 1);
        let users: Vec<usize> = (0..msgs.len())
            .filter(|&i| msgs[i].role == Role::User)
            .collect();
        let from = users[users.len() - out.stats.kept_turns];
        for (i, src) in msgs.iter().enumerate().skip(from) {
            let m = out
                .messages
                .iter()
                .find(|m| m.src == i)
                .expect("anchor kept");
            assert_eq!(m.content, src.content);
        }
    }
}

#[test]
fn fast_path_is_identity() {
    let t = synth::transcript(5, 3_000, 3);
    let msgs = t.messages();
    let out = Compactor::new(cfg(1_000_000)).compact(&msgs, None);
    assert_eq!(out.messages.len(), msgs.len());
    for (o, m) in out.messages.iter().zip(&msgs) {
        assert_eq!(o.content, m.content);
    }
}

#[test]
fn exact_and_near_duplicates_keep_newest_and_respect_ids() {
    let old = "2026-10-21T08:00:01+02:00 nerve[1]: heartbeat ok queue=0 conns=3 all good here today\nport 22041 open for nerve relay on the box today\nunique old line about cortex scoring";
    let new = "2026-10-21T09:12:44+02:00 nerve[1]: heartbeat ok queue=0 conns=3 all good here today\nport 22042 open for nerve relay on the box today";
    let mut msgs = vec![Message::new(Role::System, "sys")];
    for _ in 0..30 {
        msgs.push(Message::new(
            Role::User,
            "filler question about something else entirely",
        ));
        msgs.push(Message::new(
            Role::Assistant,
            "filler answer that is long enough to cost tokens",
        ));
    }
    msgs.insert(3, Message::new(Role::Tool, old));
    msgs.insert(40, Message::new(Role::Tool, new));
    let out = Compactor::new(cfg(500)).compact(&msgs, Some("port nerve heartbeat"));
    let s = out.stats.clone();
    assert!(
        s.dup_near >= 1,
        "heartbeat differing only in time is a near dup: {s:?}"
    );
    let text = out.to_text();
    assert!(
        text.contains("port 22041"),
        "different port is not a duplicate"
    );
    assert!(text.contains("port 22042"));
    assert_eq!(
        text.matches("heartbeat ok").count(),
        1,
        "one heartbeat kept"
    );
    assert!(text.contains("09:12:44"), "newest copy kept");
}

#[test]
fn code_blocks_are_atomic_and_utf8_safe_splitting() {
    let s = "intro\n```rust\nfn a() {}\n\nfn b() {}\n```\n".to_string()
        + &"čćžšđ €uro sentence. ".repeat(80)
        + "\ntail";
    let mut spans = Vec::new();
    split_units(&s, 100, |a, e, code| spans.push((a, e, code)));
    let code: Vec<_> = spans.iter().filter(|x| x.2).collect();
    assert_eq!(code.len(), 1);
    assert!(s[code[0].0..code[0].1].starts_with("```rust"));
    assert!(s[code[0].0..code[0].1].ends_with("```"));
    for &(a, e, _) in &spans {
        assert!(s.is_char_boundary(a) && s.is_char_boundary(e));
        assert!(e > a);
    }
    assert!(spans.iter().filter(|x| !x.2).all(|x| x.1 - x.0 <= 100));
}

#[test]
fn prop_random_unicode_never_panics() {
    let alphabet = [
        "a",
        "Z",
        "7",
        " ",
        "\n",
        "```",
        "é",
        "€",
        "😀",
        ".",
        ",",
        "-",
        "\t",
        "\r\n",
        "call_1",
        "0xdeadbeef",
    ];
    let mut r = Rng(9);
    for _ in 0..200 {
        let mut owned = Vec::new();
        for _ in 0..(1 + r.below(30)) {
            let mut s = String::new();
            for _ in 0..r.below(400) {
                s.push_str(r.pick(&alphabet));
            }
            owned.push(s);
        }
        let msgs: Vec<Message> = owned
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let role =
                    [Role::System, Role::User, Role::Assistant, Role::Tool][(i + r.below(4)) % 4];
                Message::new(role, s)
            })
            .collect();
        let budget = 10 + r.below(800);
        let c = cfg(budget);
        let out = Compactor::new(c.clone()).compact(&msgs, None);
        check_faithful(&msgs, &out, &c);
        if !out.stats.over_budget {
            assert!(out.stats.output_tokens <= budget);
        }
    }
}

#[test]
fn query_aware_retention_floor() {
    // regression floor for the bench numbers (100% at the time of writing)
    let t = synth::transcript(77, 100_000, 40);
    let msgs = t.messages();
    let mut c = Compactor::new(cfg(10_000));
    let mut hit = 0;
    for f in &t.facts {
        if synth::retained(&c.compact(&msgs, Some(&f.question)).to_text(), f) {
            hit += 1;
        }
    }
    assert!(
        hit * 100 >= 95 * t.facts.len(),
        "retention {hit}/{}",
        t.facts.len()
    );
}

#[test]
fn referenced_ids_in_anchor_window_are_kept() {
    let mut msgs = vec![Message::new(Role::System, "sys")];
    msgs.push(Message::new(Role::User, "start"));
    let big = "noise line with words that repeat a lot\n".repeat(5)
        + "artifact sha is 9c1e77a for the nerve bundle\n"
        + &"other noise line nobody needs again ever\n".repeat(5);
    let filler: Vec<String> = (0..60)
        .map(|i| format!("filler message number {i} with nothing in it"))
        .collect();
    msgs.push(Message::new(Role::Tool, &big));
    for f in &filler {
        msgs.push(Message::new(Role::Assistant, f));
    }
    msgs.push(Message::new(Role::User, "status?"));
    msgs.push(Message::new(Role::Assistant, "rolling back 9c1e77a now"));
    let out = Compactor::new(cfg(260)).compact(&msgs, Some("status?"));
    assert!(out.stats.referenced_units >= 1);
    assert!(out.to_text().contains("artifact sha is 9c1e77a"));
}

struct Upweight(&'static str);
impl Rescorer for Upweight {
    fn rescore(&self, _q: &str, units: &[&str], scores: &mut [f32]) {
        for (u, s) in units.iter().zip(scores.iter_mut()) {
            if u.contains(self.0) {
                *s += 100.0;
            }
        }
    }
}
struct Count;
impl GapCompressor for Count {
    fn compress_gap(&self, elided: &[&str], _max: usize) -> Option<String> {
        Some(format!("[{}]", elided.len().min(9)))
    }
}

#[test]
fn plug_points_rescorer_and_gap_compressor() {
    let t = synth::transcript(3, 20_000, 0);
    let msgs = t.messages();
    let mut c = Compactor::new(cfg(2_000));
    let out = c.compact_with(&msgs, None, Some(&Upweight("listens")), Some(&Count));
    let _ = out;
    let probe = "zz-marker-line listens here";
    let mut owned: Vec<String> = t.msgs.iter().map(|m| m.content.clone()).collect();
    owned[5].push_str(&format!("\n{probe}\n"));
    let m2: Vec<Message> = msgs
        .iter()
        .zip(&owned)
        .map(|(m, s)| Message {
            content: s,
            ..m.clone()
        })
        .collect();
    let out = c.compact_with(&m2, None, Some(&Upweight("zz-marker")), Some(&Count));
    let text = out.to_text();
    assert!(text.contains(probe));
    assert!(text.contains("["), "gap compressor output used");
    assert!(out.stats.output_tokens <= 2_000);
}

#[test]
fn compact_text_document_mode() {
    let doc = (0..400)
        .map(|i| format!("line {i}: routine status ok"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\nerror: disk full on /var/lib/galion-nerve at 17:02\n";
    let out = compact_text(&doc, "disk full error", 60);
    assert!(est_tokens(&out) <= 60, "{}", est_tokens(&out));
    assert!(out.contains("error: disk full"));
}
