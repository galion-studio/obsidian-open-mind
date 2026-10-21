// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! openmind-compact: model-free, deterministic context compaction for long
//! agent transcripts.
//!
//! Pipeline (all CPU, no model, no network):
//! 1. split every message into units (lines; fenced code blocks stay whole;
//!    very long lines are cut at sentence or word boundaries);
//! 2. hard anchors: system messages and the latest N turns are kept verbatim;
//! 3. exact duplicates (normalised hash) and near duplicates (64-bit SimHash,
//!    Hamming <= 3, with the set of non-volatile IDs required to match) are
//!    dropped, keeping the newest copy;
//! 4. the rest is scored: BM25 against the query (default: the last user
//!    message) + recency decay + mean-IDF salience, with boosts for units that
//!    carry IDs/numbers, code, or an ID that the anchored window or the query
//!    still mentions ("referenced");
//! 5. greedy selection under the token budget, with conservative accounting so
//!    the result never exceeds the budget when the anchors fit; elided runs
//!    become a one-line marker, and assistant tool calls and their tool
//!    results are emitted together so the transcript stays API-valid.
//!
//! [`Rescorer`] and [`GapCompressor`] are the plug points for a model-based
//! compressor later (e.g. an LLMLingua-2 style token classifier on candle).
//!
//! Scratch buffers live in [`Compactor`] and are reused across calls, so the
//! hot loops do not allocate once capacity has warmed up.

pub mod hash;
pub mod simhash;
pub mod synth;
pub mod text;

use hash::{IdMap, IdSet};
use simhash::{NearIndex, SimAcc};
use std::borrow::Cow;
use text::{est_tokens, is_volatile, word_hash, word_kind, words_at, WordKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn parse(s: &str) -> Role {
        match s {
            "system" | "developer" => Role::System,
            "user" => Role::User,
            "tool" | "function" => Role::Tool,
            _ => Role::Assistant,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// One chat message. `tool_calls` are the call ids an assistant message
/// requests; `tool_call_id` is the call a tool message answers.
#[derive(Clone, Debug)]
pub struct Message<'a> {
    pub role: Role,
    pub content: &'a str,
    pub tool_call_id: Option<&'a str>,
    pub tool_calls: Vec<&'a str>,
}

impl<'a> Message<'a> {
    pub fn new(role: Role, content: &'a str) -> Self {
        Message {
            role,
            content,
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    /// Total budget in estimated tokens (bytes/4, as hermes-rs estimates),
    /// including `per_message_overhead` per emitted message.
    pub budget_tokens: usize,
    /// Latest turns (a turn starts at a user message) kept verbatim.
    pub keep_last_turns: usize,
    pub per_message_overhead: usize,
    /// Lines longer than this (bytes) are split into several units.
    pub max_unit_bytes: usize,
    pub near_dup_hamming: u32,
    /// Units with fewer SimHash features are only exact-deduplicated.
    pub min_simhash_features: u32,
    pub bm25_k1: f32,
    pub bm25_b: f32,
    pub w_bm25: f32,
    pub w_recency: f32,
    pub w_salience: f32,
    pub boost_id: f32,
    pub boost_code: f32,
    pub boost_ref: f32,
    /// Recency half-life as a fraction of the transcript length.
    pub recency_half_life: f32,
    /// Line inserted where units were elided.
    pub gap_marker: &'static str,
    /// Content of a tool-call partner that had to be emitted with no units.
    pub stub: &'static str,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            budget_tokens: 8192,
            keep_last_turns: 2,
            per_message_overhead: 4,
            max_unit_bytes: 600,
            near_dup_hamming: 3,
            min_simhash_features: 8,
            bm25_k1: 1.2,
            bm25_b: 0.75,
            w_bm25: 1.0,
            w_recency: 0.35,
            w_salience: 0.2,
            boost_id: 0.25,
            boost_code: 0.1,
            boost_ref: 1.0,
            recency_half_life: 0.25,
            gap_marker: "…",
            stub: "[compacted]",
        }
    }
}

/// Plug point: adjust unit scores (e.g. with a small CPU model). `scores`
/// is parallel to `units` and already holds the extractive score.
pub trait Rescorer {
    fn rescore(&self, query: &str, units: &[&str], scores: &mut [f32]);
}

/// Plug point: turn an elided run of units into a short replacement (e.g. an
/// abstractive summary). Returning `None` uses the plain gap marker. The
/// replacement must fit in `max_tokens` or it is ignored.
pub trait GapCompressor {
    fn compress_gap(&self, elided: &[&str], max_tokens: usize) -> Option<String>;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub messages_in: usize,
    pub messages_out: usize,
    pub units: usize,
    pub hard_tokens: usize,
    pub dup_exact: usize,
    pub dup_near: usize,
    pub selected_units: usize,
    pub referenced_units: usize,
    /// Turns actually kept verbatim (may be < keep_last_turns under pressure).
    pub kept_turns: usize,
    /// The anchors alone exceeded the budget.
    pub over_budget: bool,
}

#[derive(Clone, Debug)]
pub struct OutMessage<'a> {
    /// Index of the source message.
    pub src: usize,
    pub role: Role,
    pub content: Cow<'a, str>,
    pub tool_call_id: Option<&'a str>,
    pub tool_calls: Vec<&'a str>,
}

#[derive(Clone, Debug)]
pub struct Compacted<'a> {
    pub messages: Vec<OutMessage<'a>>,
    pub stats: Stats,
}

impl Compacted<'_> {
    /// Plain-text rendering (role headers), handy for checks and logs.
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        for m in &self.messages {
            s.push_str(m.role.as_str());
            s.push_str(": ");
            s.push_str(&m.content);
            s.push('\n');
        }
        s
    }
}

const F_HARD: u8 = 1;
const F_CODE: u8 = 2;
const F_IDS: u8 = 4;
const F_REF: u8 = 8;
const F_DUP: u8 = 16;
const F_SEL: u8 = 32;

#[derive(Clone, Copy)]
struct Unit {
    msg: u32,
    start: u32,
    end: u32,
    tokens: u32,
    t_off: u32,
    t_len: u32,
    flags: u8,
    score: f32,
}

/// Reusable compactor. Keep one per thread and call [`Compactor::compact`].
#[derive(Default)]
pub struct Compactor {
    pub cfg: Config,
    units: Vec<Unit>,
    terms: Vec<u64>,
    df: IdMap<u32>,
    exact: IdSet,
    refs: IdSet,
    near: NearIndex,
    order: Vec<u32>,
    qterms: Vec<u64>,
    sims: Vec<(u64, u64)>,
    msg_state: Vec<u8>,
    group: Vec<u32>,
    sal: Vec<f32>,
    gm: Vec<(u32, u32)>,
}

const M_HARD: u8 = 1;
const M_ACTIVE: u8 = 2;
const M_STUB: u8 = 4;

impl Compactor {
    pub fn new(cfg: Config) -> Self {
        Compactor {
            cfg,
            ..Default::default()
        }
    }

    fn msg_cost(&self, m: &Message) -> usize {
        self.cfg.per_message_overhead + est_tokens(m.content)
    }

    /// Compact `msgs` for `query` (defaults to the last user message).
    pub fn compact<'a>(&mut self, msgs: &[Message<'a>], query: Option<&str>) -> Compacted<'a> {
        self.compact_with(msgs, query, None, None)
    }

    pub fn compact_with<'a>(
        &mut self,
        msgs: &[Message<'a>],
        query: Option<&str>,
        rescorer: Option<&dyn Rescorer>,
        gap: Option<&dyn GapCompressor>,
    ) -> Compacted<'a> {
        let n = msgs.len();
        let mut stats = Stats {
            messages_in: n,
            input_tokens: msgs.iter().map(|m| self.msg_cost(m)).sum(),
            ..Default::default()
        };
        let budget = self.cfg.budget_tokens;
        if stats.input_tokens <= budget {
            // Fast path: nothing to do.
            stats.output_tokens = stats.input_tokens;
            stats.messages_out = n;
            stats.kept_turns = msgs.iter().filter(|m| m.role == Role::User).count();
            return Compacted {
                messages: msgs
                    .iter()
                    .enumerate()
                    .map(|(i, m)| verbatim(i, m))
                    .collect(),
                stats,
            };
        }

        // ---- 1. hard anchors -------------------------------------------
        self.msg_state.clear();
        self.msg_state.resize(n, 0);
        let users: Vec<usize> = (0..n).filter(|&i| msgs[i].role == Role::User).collect();
        let sys_cost: usize = msgs
            .iter()
            .filter(|m| m.role == Role::System)
            .map(|m| self.msg_cost(m))
            .sum();
        let mut k = self.cfg.keep_last_turns.min(users.len());
        let tail_from = |k: usize| -> usize {
            if k == 0 {
                n
            } else {
                users[users.len() - k]
            }
        };
        let ovh = self.cfg.per_message_overhead;
        let tail_cost = |from: usize| -> usize {
            msgs[from..]
                .iter()
                .filter(|m| m.role != Role::System)
                .map(|m| ovh + est_tokens(m.content))
                .sum()
        };
        while k > 1 && sys_cost + tail_cost(tail_from(k)) > budget {
            k -= 1;
        }
        let from = tail_from(k);
        let mut hard_cost = sys_cost;
        for (i, m) in msgs.iter().enumerate() {
            if m.role == Role::System {
                self.msg_state[i] = M_HARD;
            }
        }
        if sys_cost + tail_cost(from) <= budget {
            for (i, m) in msgs.iter().enumerate().skip(from) {
                if m.role != Role::System {
                    self.msg_state[i] = M_HARD;
                    hard_cost += ovh + est_tokens(m.content);
                }
            }
        } else {
            // Even one turn is too big: keep its user message verbatim, and
            // let the rest of that turn compete as ordinary units.
            if from < n {
                self.msg_state[from] = M_HARD;
                hard_cost += self.msg_cost(&msgs[from]);
            }
        }
        stats.kept_turns = k;
        stats.hard_tokens = hard_cost;
        stats.over_budget = hard_cost > budget;

        // tool-call groups: tool result -> its assistant message
        self.group.clear();
        self.group.extend(0..n as u32);
        {
            let mut call_owner: IdMap<u32> = IdMap::default();
            for (i, m) in msgs.iter().enumerate() {
                for id in &m.tool_calls {
                    call_owner.insert(word_hash(id.as_bytes()), i as u32);
                }
                if let Some(id) = m.tool_call_id {
                    if let Some(&o) = call_owner.get(&word_hash(id.as_bytes())) {
                        self.group[i] = o;
                    }
                }
            }
        }

        // ---- 2. units + term arena -------------------------------------
        self.units.clear();
        self.terms.clear();
        self.sims.clear();
        self.df.clear();
        self.refs.clear();
        for (mi, m) in msgs.iter().enumerate() {
            let hard = self.msg_state[mi] & M_HARD != 0;
            split_units(m.content, self.cfg.max_unit_bytes, |s, e, code| {
                let t = &m.content[s..e];
                let t_off = self.terms.len() as u32;
                let mut acc = SimAcc::default();
                let mut idsig = 0u64;
                let mut flags = if hard { F_HARD } else { 0 } | if code { F_CODE } else { 0 };
                let mut prev = 0u64;
                let tb = t.as_bytes();
                for (o, w) in words_at(t) {
                    let h = word_hash(w);
                    let kind = word_kind(w);
                    let vol = kind != WordKind::Plain && is_volatile(tb, o, o + w.len());
                    let is_id = kind == WordKind::Id && !vol;
                    // low bit tags ID words in the arena
                    let th = (h & !1) | is_id as u64;
                    self.terms.push(th);
                    let feat = if kind == WordKind::Plain {
                        h
                    } else {
                        0x9e37_79b9_7f4a_7c15
                    };
                    acc.add(feat);
                    acc.add(text::finish(text::mix(prev, feat)));
                    prev = feat;
                    if is_id {
                        flags |= F_IDS;
                        idsig = idsig.wrapping_add(text::finish(h));
                        if hard {
                            self.refs.insert(th);
                        }
                    }
                }
                let t_len = self.terms.len() as u32 - t_off;
                let sim = if acc.features() / 2 >= self.cfg.min_simhash_features {
                    acc.finish()
                } else {
                    0
                };
                self.sims.push((sim, idsig));
                self.units.push(Unit {
                    msg: mi as u32,
                    start: s as u32,
                    end: e as u32,
                    tokens: est_tokens(t) as u32,
                    t_off,
                    t_len,
                    flags,
                    score: 0.0,
                });
            });
        }
        stats.units = self.units.len();

        // query terms (deduplicated); query IDs count as references too
        let q_owned;
        let q: &str = match query {
            Some(q) => q,
            None => {
                q_owned = msgs
                    .iter()
                    .rev()
                    .find(|m| m.role == Role::User)
                    .map(|m| m.content)
                    .unwrap_or("");
                q_owned
            }
        };
        self.qterms.clear();
        for (o, w) in words_at(q) {
            let h = word_hash(w);
            let kind = word_kind(w);
            let is_id = kind == WordKind::Id && !is_volatile(q.as_bytes(), o, o + w.len());
            let th = (h & !1) | is_id as u64;
            if is_id {
                self.refs.insert(th);
            }
            self.qterms.push(th);
        }
        self.qterms.sort_unstable();
        self.qterms.dedup();

        // sort each unit's terms in place, count document frequencies
        let mut total_len = 0u64;
        for u in &self.units {
            let sl = &mut self.terms[u.t_off as usize..(u.t_off + u.t_len) as usize];
            sl.sort_unstable();
            total_len += u.t_len as u64;
            let mut last = None;
            for &t in sl.iter() {
                if last != Some(t) {
                    *self.df.entry(t).or_insert(0) += 1;
                    last = Some(t);
                }
            }
        }

        // ---- 3. dedup, newest first ------------------------------------
        self.exact.clear();
        self.near.clear();
        for ui in (0..self.units.len()).rev() {
            let u = self.units[ui];
            let t = &msgs[u.msg as usize].content[u.start as usize..u.end as usize];
            let eh = text::norm_hash(t);
            let hard = u.flags & F_HARD != 0;
            if !self.exact.insert(eh) && !hard {
                self.units[ui].flags |= F_DUP;
                stats.dup_exact += 1;
                continue;
            }
            let (sim, sig) = self.sims[ui];
            if sim != 0 {
                if !hard
                    && self
                        .near
                        .find(sim, sig, self.cfg.near_dup_hamming)
                        .is_some()
                {
                    self.units[ui].flags |= F_DUP;
                    stats.dup_near += 1;
                    continue;
                }
                self.near.insert(sim, sig, ui as u32);
            }
        }

        // ---- 4. score --------------------------------------------------
        let nu = self.units.len().max(1) as f32;
        let avgdl = (total_len as f32 / nu).max(1.0);
        let idf = |df: u32| -> f32 { (1.0 + (nu - df as f32 + 0.5) / (df as f32 + 0.5)).ln() };
        let (k1, b) = (self.cfg.bm25_k1, self.cfg.bm25_b);
        let mut max_bm = 0f32;
        let mut max_sal = 0f32;
        // an ID only counts as a reference if it is rare (a "+0200" or a
        // year on every line references nothing)
        let ref_df_max = (self.units.len() as u32 / 100).max(3);
        self.sal.clear();
        self.sal.resize(self.units.len(), 0.0);
        for (ui, u) in self.units.iter_mut().enumerate() {
            if u.flags & (F_HARD | F_DUP) != 0 {
                continue;
            }
            let sl = &self.terms[u.t_off as usize..(u.t_off + u.t_len) as usize];
            // BM25: merge-walk the two sorted term lists
            let norm = k1 * (1.0 - b + b * u.t_len as f32 / avgdl);
            let mut bm = 0f32;
            let (mut i, mut j) = (0usize, 0usize);
            while i < sl.len() && j < self.qterms.len() {
                let (a, c) = (sl[i], self.qterms[j]);
                if a < c {
                    i += 1;
                } else if a > c {
                    j += 1;
                } else {
                    let mut tf = 0f32;
                    while i < sl.len() && sl[i] == a {
                        tf += 1.0;
                        i += 1;
                    }
                    let d = self.df.get(&a).copied().unwrap_or(1);
                    bm += idf(d) * tf * (k1 + 1.0) / (tf + norm);
                    j += 1;
                }
            }
            // salience = mean IDF over unique terms; references
            // salience = mean IDF over unique terms (tried: mean of the two
            // rarest terms; it retained fewer facts in the bench, 33.5% vs
            // 39.5% at 10x); also detect references to IDs the anchors or the
            // query still mention
            let mut sal = 0f32;
            let mut uniq = 0f32;
            let mut last = None;
            let mut is_ref = false;
            for &t in sl {
                if last != Some(t) {
                    let d = self.df.get(&t).copied().unwrap_or(1);
                    sal += idf(d);
                    uniq += 1.0;
                    last = Some(t);
                    if t & 1 == 1 && !is_ref && d <= ref_df_max && self.refs.contains(&t) {
                        is_ref = true;
                    }
                }
            }
            if is_ref {
                u.flags |= F_REF;
                stats.referenced_units += 1;
            }
            let sal = if uniq > 0.0 { sal / uniq } else { 0.0 };
            max_bm = max_bm.max(bm);
            max_sal = max_sal.max(sal);
            u.score = bm;
            self.sal[ui] = sal;
        }
        let cfg = &self.cfg;
        let hl = cfg.recency_half_life.max(1e-3);
        let last_msg = n.saturating_sub(1).max(1) as f32;
        for (ui, u) in self.units.iter_mut().enumerate() {
            if u.flags & (F_HARD | F_DUP) != 0 {
                continue;
            }
            let p = u.msg as f32 / last_msg;
            let mut s = cfg.w_recency * 0.5f32.powf((1.0 - p) / hl);
            if max_bm > 0.0 {
                s += cfg.w_bm25 * u.score / max_bm;
            }
            if max_sal > 0.0 {
                s += cfg.w_salience * self.sal[ui] / max_sal;
            }
            if u.flags & F_IDS != 0 {
                s += cfg.boost_id;
            }
            if u.flags & F_CODE != 0 {
                s += cfg.boost_code;
            }
            if u.flags & F_REF != 0 {
                s += cfg.boost_ref;
            }
            u.score = s;
        }
        if let Some(r) = rescorer {
            let idx: Vec<usize> = (0..self.units.len())
                .filter(|&i| self.units[i].flags & (F_HARD | F_DUP) == 0)
                .collect();
            let texts: Vec<&str> = idx
                .iter()
                .map(|&i| unit_text(msgs, &self.units[i]))
                .collect();
            let mut sc: Vec<f32> = idx.iter().map(|&i| self.units[i].score).collect();
            r.rescore(q, &texts, &mut sc);
            for (k, &i) in idx.iter().enumerate() {
                if sc[k].is_finite() {
                    self.units[i].score = sc[k];
                }
            }
        }

        // cohesion: pieces of one long source line share the best piece's
        // score (x0.95), so a relevant line is kept whole when it fits
        let mut i = 0;
        while i < self.units.len() {
            let mi = self.units[i].msg;
            let c = msgs[mi as usize].content;
            let mut j = i + 1;
            while j < self.units.len()
                && self.units[j].msg == mi
                && !c[self.units[j - 1].end as usize..self.units[j].start as usize].contains('\n')
            {
                j += 1;
            }
            if j - i > 1 {
                let mx = self.units[i..j]
                    .iter()
                    .filter(|u| u.flags & (F_HARD | F_DUP) == 0)
                    .map(|u| u.score)
                    .fold(f32::MIN, f32::max);
                for u in self.units[i..j].iter_mut() {
                    if u.flags & (F_HARD | F_DUP) == 0 {
                        u.score = u.score.max(mx * 0.95);
                    }
                }
            }
            i = j;
        }

        // ---- 5. greedy selection --------------------------------------
        self.order.clear();
        self.order.extend(
            (0..self.units.len() as u32)
                .filter(|&i| self.units[i as usize].flags & (F_HARD | F_DUP) == 0),
        );
        {
            let units = &self.units;
            self.order.sort_unstable_by(|&a, &b| {
                let (ua, ub) = (&units[a as usize], &units[b as usize]);
                ub.score.total_cmp(&ua.score).then(b.cmp(&a))
            });
        }
        // group member lists: (leader, member) sorted
        self.gm.clear();
        self.gm
            .extend((0..n as u32).map(|i| (self.group[i as usize], i)));
        self.gm.sort_unstable();
        let gm = &self.gm;
        let members_of = |g: u32| -> &[(u32, u32)] {
            let lo = gm.partition_point(|&(x, _)| x < g);
            let hi = gm.partition_point(|&(x, _)| x <= g);
            &gm[lo..hi]
        };
        let mut remaining = budget as isize - hard_cost as isize;
        let stub_cost = (cfg.per_message_overhead + est_tokens(cfg.stub)) as isize;
        let gap_cost = (est_tokens(cfg.gap_marker) + 1) as isize;
        // hard messages are emitted, and so are their tool-call partners
        for i in 0..n {
            if self.msg_state[i] & M_HARD != 0 {
                self.msg_state[i] |= M_ACTIVE;
            }
        }
        for i in 0..n {
            if self.msg_state[i] & M_HARD != 0 {
                for &(_, j) in members_of(self.group[i]) {
                    let j = j as usize;
                    if self.msg_state[j] & M_ACTIVE == 0 {
                        self.msg_state[j] |= M_ACTIVE;
                        remaining -= stub_cost;
                    }
                }
            }
        }
        for k in 0..self.order.len() {
            let ui = self.order[k] as usize;
            let u = self.units[ui];
            let mi = u.msg as usize;
            // unit + newline + a possible gap marker before it, and on first
            // use of a message: its overhead, a trailing gap marker, and stubs
            // for every tool-call partner not yet emitted.
            let mut cost = u.tokens as isize + 1 + gap_cost;
            let fresh = self.msg_state[mi] & M_ACTIVE == 0;
            if fresh {
                cost += cfg.per_message_overhead as isize + gap_cost;
                for &(_, j) in members_of(self.group[mi]) {
                    if j as usize != mi && self.msg_state[j as usize] & M_ACTIVE == 0 {
                        cost += stub_cost;
                    }
                }
            } else if self.msg_state[mi] & M_STUB != 0 {
                // the stub text is replaced by real units: gain nothing back
                // (conservative) but clear the flag
                self.msg_state[mi] &= !M_STUB;
            }
            if cost > remaining {
                continue;
            }
            remaining -= cost;
            self.units[ui].flags |= F_SEL;
            stats.selected_units += 1;
            if fresh {
                self.msg_state[mi] |= M_ACTIVE;
                for &(_, j) in members_of(self.group[mi]) {
                    let j = j as usize;
                    if self.msg_state[j] & M_ACTIVE == 0 {
                        self.msg_state[j] |= M_ACTIVE | M_STUB;
                    }
                }
            }
        }

        // ---- 6. assemble ----------------------------------------------
        let mut out: Vec<OutMessage<'a>> = Vec::new();
        let mut ui = 0usize;
        let mut elided: Vec<&str> = Vec::new();
        for (mi, m) in msgs.iter().enumerate() {
            let start_ui = ui;
            while ui < self.units.len() && self.units[ui].msg as usize == mi {
                ui += 1;
            }
            let st = self.msg_state[mi];
            if st & M_HARD != 0 {
                out.push(verbatim(mi, m));
                continue;
            }
            if st & M_ACTIVE == 0 {
                continue;
            }
            let mut s = String::new();
            let mut pending_gap = false;
            let mut prev_end: Option<usize> = None;
            elided.clear();
            for u in &self.units[start_ui..ui] {
                let t = unit_text(msgs, u);
                if u.flags & F_SEL == 0 {
                    pending_gap = true;
                    if gap.is_some() {
                        elided.push(t);
                    }
                    continue;
                }
                // pieces of one long source line are re-joined on one line
                let same_line =
                    prev_end.is_some_and(|pe| !m.content[pe..u.start as usize].contains('\n'));
                if pending_gap {
                    push_gap(&mut s, cfg.gap_marker, gap, &elided, same_line);
                    if !same_line {
                        s.push('\n');
                    }
                    elided.clear();
                    pending_gap = false;
                } else if same_line {
                    let sep = &m.content[prev_end.unwrap_or(0)..u.start as usize];
                    s.push_str(if sep.len() <= 4 { sep } else { " " });
                } else if !s.is_empty() {
                    s.push('\n');
                }
                s.push_str(t);
                prev_end = Some(u.end as usize);
            }
            if s.is_empty() {
                s.push_str(cfg.stub);
            } else if pending_gap {
                push_gap(&mut s, cfg.gap_marker, gap, &elided, false);
            }
            out.push(OutMessage {
                src: mi,
                role: m.role,
                content: Cow::Owned(s),
                tool_call_id: m.tool_call_id,
                tool_calls: m.tool_calls.clone(),
            });
        }
        stats.messages_out = out.len();
        stats.output_tokens = out
            .iter()
            .map(|m| cfg.per_message_overhead + est_tokens(&m.content))
            .sum();
        Compacted {
            messages: out,
            stats,
        }
    }
}

#[inline]
fn unit_text<'a>(msgs: &[Message<'a>], u: &Unit) -> &'a str {
    &msgs[u.msg as usize].content[u.start as usize..u.end as usize]
}

fn push_gap(
    s: &mut String,
    marker: &str,
    gap: Option<&dyn GapCompressor>,
    elided: &[&str],
    inline: bool,
) {
    let sep = if inline { ' ' } else { '\n' };
    if !s.is_empty() {
        s.push(sep);
    }
    // A model-based gap compressor may replace the marker, but only with
    // something no longer than the marker's reserved cost, so the budget
    // guarantee holds. Callers wanting larger summaries reserve budget by
    // lowering `budget_tokens`.
    let mut text = marker;
    let repl;
    if let Some(g) = gap {
        if let Some(r) = g.compress_gap(elided, est_tokens(marker)) {
            if est_tokens(&r) <= est_tokens(marker) {
                repl = r;
                text = &repl;
            }
        }
    }
    s.push_str(text);
    if inline {
        s.push(' ');
    }
}

fn verbatim<'a>(i: usize, m: &Message<'a>) -> OutMessage<'a> {
    OutMessage {
        src: i,
        role: m.role,
        content: Cow::Borrowed(m.content),
        tool_call_id: m.tool_call_id,
        tool_calls: m.tool_calls.clone(),
    }
}

/// Split `s` into unit byte ranges: non-empty lines, whole fenced code
/// blocks (```), long lines cut at sentence/word boundaries.
pub fn split_units(s: &str, max: usize, mut f: impl FnMut(usize, usize, bool)) {
    let max = max.max(16);
    let mut off = 0usize;
    let mut code_start: Option<usize> = None;
    for line in s.split_inclusive('\n') {
        let lo = off;
        off += line.len();
        let body = line.trim_end_matches(['\n', '\r']);
        let t = body.trim_start();
        if t.starts_with("```") {
            match code_start {
                None => code_start = Some(lo),
                Some(cs) => {
                    f(cs, lo + body.len(), true);
                    code_start = None;
                }
            }
            continue;
        }
        if code_start.is_some() || t.trim_end().is_empty() {
            continue;
        }
        let mut a = lo + (body.len() - t.len());
        let e = lo + body.trim_end().len();
        while e - a > max {
            let cut = cut_point(s, a, a + max);
            f(a, cut, false);
            a = cut;
            while a < e && s.as_bytes()[a] == b' ' {
                a += 1;
            }
        }
        if a < e {
            f(a, e, false);
        }
    }
    if let Some(cs) = code_start {
        let end = s.trim_end().len().max(cs);
        f(cs, end, true);
    }
}

fn cut_point(s: &str, a: usize, hard: usize) -> usize {
    let hard = text::floor_boundary(s, hard);
    let w = &s.as_bytes()[a..hard];
    let half = w.len() / 2;
    for pat in [&b". "[..], b"; ", b", ", b" "] {
        if let Some(p) = w
            .windows(pat.len())
            .rposition(|x| x == pat)
            .filter(|&p| p >= half)
        {
            return a + p + pat.len() - 1;
        }
    }
    if hard > a {
        hard
    } else {
        // a single multi-byte char longer than max: take it whole
        let mut e = a + 1;
        while !s.is_char_boundary(e) {
            e += 1;
        }
        e
    }
}

/// Convenience: compact one plain document (e.g. an offloaded tool result)
/// to `budget_tokens` for `query`. Lines keep their order.
pub fn compact_text(doc: &str, query: &str, budget_tokens: usize) -> String {
    let mut c = Compactor::new(Config {
        budget_tokens: budget_tokens + 4,
        keep_last_turns: 0,
        recency_half_life: 10.0,
        ..Default::default()
    });
    let msgs = [Message::new(Role::Tool, doc)];
    let r = c.compact(&msgs, Some(query));
    r.messages
        .into_iter()
        .next()
        .map(|m| m.content.into_owned())
        .unwrap_or_default()
}
