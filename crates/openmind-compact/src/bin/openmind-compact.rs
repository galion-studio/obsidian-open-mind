// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! openmind-compact CLI.
//!
//!   openmind-compact compact --budget N [--query Q] [--keep-turns K] < in.jsonl > out.jsonl
//!   openmind-compact bench [--reps R] [--real FILE]...
//!
//! JSONL messages: {"role","content","tool_call_id"?,"tool_calls"?:[{"id"}]}.
//! `bench` prints latency, compression and exact-match fact retention for the
//! compactor and for the hermes-rs front-trim baseline (`fit_context`).

use openmind_compact::synth::{self, Fact, OwnedMsg, Rng, Transcript};
use openmind_compact::text::{est_tokens, word_kind, words, WordKind};
use openmind_compact::{Compactor, Config, Message, Role};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::time::Instant;

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter()
        .position(|a| a == k)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("compact") => compact_cmd(&args),
        Some("bench") => bench_cmd(&args),
        Some("diagnose") => diagnose_cmd(&args),
        _ => {
            eprintln!("usage: openmind-compact compact --budget N [--query Q] [--keep-turns K] < in.jsonl\n       openmind-compact bench [--reps R] [--real FILE]...");
            std::process::exit(2);
        }
    }
}

fn compact_cmd(args: &[String]) {
    let budget: usize = arg(args, "--budget")
        .and_then(|s| s.parse().ok())
        .unwrap_or(8192);
    let keep: usize = arg(args, "--keep-turns")
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    let query = arg(args, "--query");
    let mut owned = Vec::new();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            eprintln!("skip: not JSON");
            continue;
        };
        owned.push(OwnedMsg {
            role: Role::parse(v["role"].as_str().unwrap_or("assistant")),
            content: v["content"].as_str().unwrap_or("").to_string(),
            tool_call_id: v["tool_call_id"].as_str().map(str::to_string),
            tool_calls: v["tool_calls"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|c| c["id"].as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    let t = Transcript {
        msgs: owned,
        facts: vec![],
    };
    let msgs = t.messages();
    let mut c = Compactor::new(Config {
        budget_tokens: budget,
        keep_last_turns: keep,
        ..Default::default()
    });
    let t0 = Instant::now();
    let r = c.compact(&msgs, query.as_deref());
    let us = t0.elapsed().as_micros();
    let out = std::io::stdout();
    let mut out = out.lock();
    for m in &r.messages {
        let mut v = json!({"role": m.role.as_str(), "content": m.content});
        if let Some(id) = m.tool_call_id {
            v["tool_call_id"] = json!(id);
        }
        if !m.tool_calls.is_empty() {
            v["tool_calls"] = json!(m
                .tool_calls
                .iter()
                .map(|id| json!({"id": id}))
                .collect::<Vec<_>>());
        }
        let _ = writeln!(out, "{v}");
    }
    let s = &r.stats;
    eprintln!(
        "{}",
        json!({"input_tokens": s.input_tokens, "output_tokens": s.output_tokens,
               "messages_in": s.messages_in, "messages_out": s.messages_out, "units": s.units,
               "dup_exact": s.dup_exact, "dup_near": s.dup_near, "selected_units": s.selected_units,
               "referenced_units": s.referenced_units, "kept_turns": s.kept_turns,
               "over_budget": s.over_budget, "elapsed_us": us})
    );
}

/// hermes-rs `fit_context` equivalent: newest messages that fit, never
/// starting on an orphaned tool result, system kept.
fn front_trim(msgs: &[Message], budget: usize) -> String {
    let sys: Vec<&Message> = msgs.iter().filter(|m| m.role == Role::System).collect();
    let mut used: usize = sys.iter().map(|m| 4 + est_tokens(m.content)).sum();
    let mut from = msgs.len();
    for (i, m) in msgs.iter().enumerate().rev() {
        if m.role == Role::System {
            continue;
        }
        let t = 4 + est_tokens(m.content);
        if used + t > budget && from < msgs.len() {
            break;
        }
        used += t;
        from = i;
    }
    while from < msgs.len() && msgs[from].role == Role::Tool {
        from += 1;
    }
    let mut s = String::new();
    for m in sys
        .iter()
        .copied()
        .chain(msgs[from..].iter().filter(|m| m.role != Role::System))
    {
        s.push_str(m.content);
        s.push('\n');
    }
    s
}

struct Row {
    name: String,
    tokens: usize,
    budget: usize,
    ratio: f64,
    ms_med: f64,
    ms_min: f64,
    ret_agnostic: (usize, usize),
    ret_aware: (usize, usize),
    ret_front: (usize, usize),
    ret_aware_ms: f64, // mean ms per query-aware call
    dups: (usize, usize),
    over: bool,
}

fn run_case(name: &str, t: &Transcript, budget: usize, reps: usize) -> Row {
    let msgs = t.messages();
    let tokens = t.tokens();
    let mut c = Compactor::new(Config {
        budget_tokens: budget,
        ..Default::default()
    });
    // warm-up, then timed reps (query-agnostic: query = last user message)
    let r = c.compact(&msgs, None);
    let mut times = Vec::with_capacity(reps);
    for _ in 0..reps {
        let t0 = Instant::now();
        let rr = c.compact(&msgs, None);
        times.push(t0.elapsed().as_secs_f64() * 1e3);
        std::hint::black_box(&rr);
    }
    times.sort_by(f64::total_cmp);
    let text = r.to_text();
    let agn = t.facts.iter().filter(|f| synth::retained(&text, f)).count();
    // query-aware: the question is the compaction query
    let mut aware = 0;
    let mut aware_ms = 0.0;
    for f in &t.facts {
        let t0 = Instant::now();
        let rq = c.compact(&msgs, Some(&f.question));
        aware_ms += t0.elapsed().as_secs_f64() * 1e3;
        if synth::retained(&rq.to_text(), f) {
            aware += 1;
        }
    }
    let ft = front_trim(&msgs, budget);
    let front = t.facts.iter().filter(|f| synth::retained(&ft, f)).count();
    let nf = t.facts.len();
    Row {
        name: name.to_string(),
        tokens,
        budget,
        ratio: tokens as f64 / r.stats.output_tokens.max(1) as f64,
        ms_med: times.get(times.len() / 2).copied().unwrap_or(0.0),
        ms_min: times.first().copied().unwrap_or(0.0),
        ret_agnostic: (agn, nf),
        ret_aware: (aware, nf),
        ret_front: (front, nf),
        ret_aware_ms: if nf > 0 { aware_ms / nf as f64 } else { 0.0 },
        dups: (r.stats.dup_exact, r.stats.dup_near),
        over: r.stats.over_budget || r.stats.output_tokens > budget,
    }
}

fn pct(x: (usize, usize)) -> String {
    if x.1 == 0 {
        return "-".into();
    }
    format!("{:.1}% ({}/{})", 100.0 * x.0 as f64 / x.1 as f64, x.0, x.1)
}

fn print_rows(rows: &[Row]) {
    println!("| case | in tok | budget | ratio | ms med | ms min | ms/100k | ms per query-aware call | dup exact/near | retention agnostic | retention query-aware | front-trim baseline |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
    for r in rows {
        println!(
            "| {} | {} | {} | {:.1}x | {:.2} | {:.2} | {:.2} | {:.2} | {}/{} | {} | {} | {} |{}",
            r.name,
            r.tokens,
            r.budget,
            r.ratio,
            r.ms_med,
            r.ms_min,
            r.ms_med * 100_000.0 / r.tokens as f64,
            r.ret_aware_ms,
            r.dups.0,
            r.dups.1,
            pct(r.ret_agnostic),
            pct(r.ret_aware),
            pct(r.ret_front),
            if r.over { " OVER" } else { "" }
        );
    }
}

/// Real-shaped corpus from a text log: entries (blank-line or "- 20" bullet
/// separated) become tool results inside user/assistant turns. Facts are
/// cloze questions: a line with a hex/number ID and a rare subject word.
fn real_transcript(raw: &str, n_facts: usize, seed: u64) -> Transcript {
    let mut entries: Vec<String> = Vec::new();
    let mut cur = String::new();
    for line in raw.lines() {
        let long = cur.lines().count() >= 40;
        if (long || line.starts_with("- 20") || line.starts_with("## ")) && !cur.trim().is_empty() {
            entries.push(std::mem::take(&mut cur));
        }
        cur.push_str(line);
        cur.push('\n');
    }
    if !cur.trim().is_empty() {
        entries.push(cur);
    }
    let mut t = Transcript::default();
    t.msgs.push(OwnedMsg {
        role: Role::System,
        content: "You are the Gaia swarm record keeper. Answer from the record.".into(),
        tool_call_id: None,
        tool_calls: vec![],
    });
    let mut call = 0;
    for chunk in entries.chunks(3) {
        t.msgs.push(OwnedMsg {
            role: Role::User,
            content: "Read the next part of the record.".into(),
            tool_call_id: None,
            tool_calls: vec![],
        });
        call += 1;
        let id = format!("call_{call:05}");
        t.msgs.push(OwnedMsg {
            role: Role::Assistant,
            content: "Reading.".into(),
            tool_call_id: None,
            tool_calls: vec![id.clone()],
        });
        t.msgs.push(OwnedMsg {
            role: Role::Tool,
            content: chunk.concat(),
            tool_call_id: Some(id),
            tool_calls: vec![],
        });
    }
    t.msgs.push(OwnedMsg {
        role: Role::User,
        content: "Summarise the open decisions for Slavik.".into(),
        tool_call_id: None,
        tool_calls: vec![],
    });
    // document frequency of words (per line) for subject rarity
    let mut df: std::collections::HashMap<String, usize> = Default::default();
    for m in &t.msgs {
        for l in m.content.lines() {
            let mut seen = std::collections::HashSet::new();
            for w in words(l) {
                let w = String::from_utf8_lossy(w).to_lowercase();
                if seen.insert(w.clone()) {
                    *df.entry(w).or_insert(0) += 1;
                }
            }
        }
    }
    let users: Vec<usize> = (0..t.msgs.len())
        .filter(|&i| t.msgs[i].role == Role::User)
        .collect();
    let limit = users[users.len().saturating_sub(2)];
    let mut cands: Vec<Fact> = Vec::new();
    for (mi, m) in t.msgs.iter().enumerate().take(limit) {
        if m.role != Role::Tool {
            continue;
        }
        for l in m.content.lines() {
            let ws: Vec<&[u8]> = words(l).collect();
            let ans = ws.iter().find(|w| {
                w.len() >= 7
                    && word_kind(w) == WordKind::Id
                    && w.iter().all(|b| b.is_ascii_hexdigit())
                    && w.iter().any(|b| b.is_ascii_alphabetic())
            });
            let Some(ans) = ans else { continue };
            let ans = String::from_utf8_lossy(ans).to_string();
            let subj = ws
                .iter()
                .map(|w| String::from_utf8_lossy(w).to_string())
                .filter(|w| w.len() >= 6 && w.bytes().all(|b| b.is_ascii_alphabetic() || b == b'_'))
                .filter(|w| df.get(&w.to_lowercase()).copied().unwrap_or(0) <= 3)
                .max_by_key(|w| (w.len(), w.clone()));
            let Some(subj) = subj else { continue };
            // ambiguous answers (seen on several lines) make a weak test
            if df.get(&ans.to_lowercase()).copied().unwrap_or(0) > 2 {
                continue;
            }
            // short cloze question: the subject plus the ~8 words either side
            // of the answer (answer removed), like a human follow-up
            let wl: Vec<String> = ws
                .iter()
                .map(|w| String::from_utf8_lossy(w).to_string())
                .collect();
            let ai = wl.iter().position(|w| *w == ans).unwrap_or(0);
            let lo = ai.saturating_sub(8);
            let hi = (ai + 9).min(wl.len());
            let mut question = format!("{subj}: ");
            for (k, w) in wl[lo..hi].iter().enumerate() {
                if lo + k != ai {
                    question.push_str(w);
                    question.push(' ');
                }
            }
            question.push('?');
            cands.push(Fact {
                question,
                subject: subj,
                answer: ans,
                msg: mi,
            });
        }
    }
    let mut r = Rng(seed);
    while t.facts.len() < n_facts && !cands.is_empty() {
        let i = r.below(cands.len());
        t.facts.push(cands.swap_remove(i));
    }
    t
}

fn bench_cmd(args: &[String]) {
    let reps: usize = arg(args, "--reps")
        .and_then(|s| s.parse().ok())
        .unwrap_or(15);
    let mut rows = Vec::new();
    println!("## synthetic agent transcripts (40 planted facts each, none in the last 2 turns)\n");
    for &size in &[10_000usize, 50_000, 100_000, 200_000] {
        for &(seed, div) in [(1u64, 4usize), (2, 10), (3, 20)].iter() {
            let t = synth::transcript(seed + size as u64, size, 40);
            let budget = size / div;
            rows.push(run_case(
                &format!("synth {}k s{} /{}", size / 1000, seed, div),
                &t,
                budget,
                reps,
            ));
        }
    }
    print_rows(&rows);
    // aggregate over 5 seeds at 100k
    println!("\n## 100k tokens, 5 seeds per budget (aggregate)\n");
    println!("| budget | ratio mean | ms med (median of seeds) | retention agnostic | query-aware | front-trim |");
    println!("|---|---|---|---|---|---|");
    for &div in &[4usize, 10, 20] {
        let mut agg = (0usize, 0usize, 0usize, 0usize);
        let mut ratio = 0.0;
        let mut ms = Vec::new();
        for seed in 10..15u64 {
            let t = synth::transcript(seed, 100_000, 40);
            let r = run_case("", &t, 100_000 / div, reps);
            agg.0 += r.ret_agnostic.0;
            agg.1 += r.ret_aware.0;
            agg.2 += r.ret_front.0;
            agg.3 += r.ret_agnostic.1;
            ratio += r.ratio / 5.0;
            ms.push(r.ms_med);
        }
        ms.sort_by(f64::total_cmp);
        println!(
            "| {} ({}x) | {:.1}x | {:.2} | {} | {} | {} |",
            100_000 / div,
            div,
            ratio,
            ms[2],
            pct((agg.0, agg.3)),
            pct((agg.1, agg.3)),
            pct((agg.2, agg.3))
        );
    }
    let reals: Vec<String> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "--real")
        .filter_map(|(i, _)| args.get(i + 1).cloned())
        .collect();
    if !reals.is_empty() {
        println!("\n## real box logs (cloze facts: hex ID on a line with a rare subject word)\n");
        let mut rows = Vec::new();
        for p in &reals {
            let Ok(raw) = std::fs::read_to_string(p) else {
                eprintln!("skip unreadable {p}");
                continue;
            };
            let t = real_transcript(&raw, 40, 7);
            let tok = t.tokens();
            let base = std::path::Path::new(p)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            for &div in &[4usize, 10, 20] {
                rows.push(run_case(&format!("{base} /{div}"), &t, tok / div, reps));
            }
        }
        print_rows(&rows);
    }
}

/// Why do query-aware facts fail on a real log? Prints flags only.
fn diagnose_cmd(args: &[String]) {
    let p = args.get(1).cloned().unwrap_or_default();
    let div: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    let raw = std::fs::read_to_string(&p).unwrap_or_default();
    let t = real_transcript(&raw, 40, 7);
    let msgs = t.messages();
    let mut c = Compactor::new(Config {
        budget_tokens: t.tokens() / div,
        ..Default::default()
    });
    for f in &t.facts {
        let r = c.compact(&msgs, Some(&f.question));
        let txt = r.to_text();
        let ok = synth::retained(&txt, f);
        if !ok {
            let src_ok = synth::retained(msgs[f.msg].content, f);
            let line_len = msgs[f.msg]
                .content
                .lines()
                .find(|l| l.contains(&f.answer))
                .map(|l| l.len())
                .unwrap_or(0);
            println!(
                "FAIL ans_anywhere={} subj_anywhere={} src_retained={} line_bytes={} subj_len={} ",
                txt.contains(&f.answer),
                txt.contains(&f.subject),
                src_ok,
                line_len,
                f.subject.len()
            );
        }
    }
}
