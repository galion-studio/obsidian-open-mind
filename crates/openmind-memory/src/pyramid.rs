// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Memory pyramid (2026-10-29): the two layers that were missing above raw
//! L0 capture.
//!
//! * **Long-term facts.** Rule-based extraction (no model call, so nothing
//!   is invented) of what the user explicitly asked to keep ("remember ...",
//!   "save ...", "note: ...", `memory_save` notes) and a few identity
//!   statements ("my name is ...", "call me ..."). Stored as L1 rows
//!   (`type = "fact"`, `scene_name = "pyramid"`), so `/search/memories` and
//!   the MCP `memory_search` tool find them too. A later fact about the same
//!   subject ("my demo codeword is ...") supersedes the older one at recall
//!   time; nothing is deleted.
//! * **Episodes.** One row per conversation (`pyramid_episodes`): first and
//!   last activity, turn count, title, a short extractive summary and topic
//!   keywords. Episodic recall ("what did we talk about earlier today about
//!   X") searches L0 inside a time window derived from the question and
//!   groups the matching messages by conversation, with Europe/Zagreb times.
//!
//! Both are cross-chat by design and only used when the caller asks for them
//! (`/recall` `"pyramid": true`, `POST /episodes`), so the session-scoped
//! default of `/recall` is unchanged.

use crate::fts::tokenize_for_fts;
use crate::store::Store;
use rusqlite::params;
use serde_json::{json, Value};

pub const FACT_KIND: &str = "fact";
pub const FACT_SCENE: &str = "pyramid";
/// `memory_save` (openmind-mcp-stack) writes into this session key: every
/// note there is an explicit "keep this".
pub const NOTES_SESSION: &str = "hermes-mcp-notes";
const MAX_FACT_CHARS: usize = 400;
const MAX_FACTS_PER_TEXT: usize = 5;
pub const MAX_RECALL_FACTS: usize = 6;
pub const MAX_RECALL_EPISODES: usize = 3;
const MAX_KEYWORDS: usize = 24;
const MAX_SUMMARY_LINES: usize = 5;

pub const PYRAMID_SQL: &str = "CREATE TABLE IF NOT EXISTS pyramid_episodes (\
  session_key TEXT PRIMARY KEY, started_ms INTEGER NOT NULL, last_ms INTEGER NOT NULL, \
  turns INTEGER NOT NULL DEFAULT 0, title TEXT NOT NULL DEFAULT '', summary TEXT NOT NULL DEFAULT '', \
  keywords TEXT NOT NULL DEFAULT ''); \
CREATE INDEX IF NOT EXISTS idx_pyr_ep_last ON pyramid_episodes(last_ms);";

const STOP: &[&str] = &[
    "a",
    "an",
    "the",
    "is",
    "are",
    "was",
    "were",
    "be",
    "been",
    "being",
    "am",
    "i",
    "me",
    "my",
    "mine",
    "you",
    "your",
    "yours",
    "we",
    "our",
    "ours",
    "us",
    "it",
    "its",
    "this",
    "that",
    "these",
    "those",
    "what",
    "which",
    "who",
    "whom",
    "whose",
    "when",
    "where",
    "why",
    "how",
    "do",
    "does",
    "did",
    "done",
    "doing",
    "have",
    "has",
    "had",
    "to",
    "of",
    "in",
    "on",
    "at",
    "for",
    "from",
    "by",
    "with",
    "about",
    "as",
    "and",
    "or",
    "but",
    "if",
    "so",
    "not",
    "no",
    "yes",
    "can",
    "could",
    "would",
    "should",
    "will",
    "shall",
    "may",
    "might",
    "must",
    "please",
    "tell",
    "know",
    "there",
    "here",
    "then",
    "than",
    "also",
    "too",
    "just",
    "any",
    "some",
    "all",
    "again",
    "him",
    "her",
    "they",
    "them",
    "their",
    "he",
    "she",
    "his",
    "hers",
    "get",
    "got",
    "check",
    "memory",
    "memories",
    "remember",
    "save",
    "saved",
    "note",
    "notes",
    "recall",
    "find",
    "look",
    "said",
    "say",
    "told",
    "ask",
    "asked",
    "thing",
    "things",
    "something",
    "anything",
    "one",
    "into",
    "out",
    "up",
    "down",
    "let",
    "lets",
    "now",
    "okay",
    "ok",
    "thanks",
    "thank",
    "hey",
    "hi",
    "hello",
    "later",
    "keep",
    "mind",
    "forget",
    "don",
    "dont",
    "it's",
    "that's",
    "what's",
    "via",
    "per",
    "very",
    "really",
    "much",
    "more",
    "most",
    "only",
    "own",
];

/// Words that only say "this is about the past", not what about.
const CUE_WORDS: &[&str] = &[
    "earlier",
    "today",
    "yesterday",
    "week",
    "last",
    "time",
    "talk",
    "talked",
    "talking",
    "discuss",
    "discussed",
    "discussing",
    "conversation",
    "conversations",
    "chat",
    "chats",
    "agree",
    "agreed",
    "before",
    "previous",
    "previously",
    "ago",
    "morning",
    "afternoon",
    "evening",
    "tonight",
    "session",
    "spoke",
    "speak",
    "mentioned",
    "mention",
    "decide",
    "decided",
    "remind",
];

const EPISODIC_CUES: &[&str] = &[
    "earlier",
    "yesterday",
    "last time",
    "last week",
    "this week",
    "this morning",
    "we talk",
    "we talked",
    "we discuss",
    "we discussed",
    "we agreed",
    "we decided",
    "did we",
    "previous chat",
    "previous conversation",
    "last conversation",
    "other chat",
    "remind me what",
    "what did i say",
    "what did i tell",
    "did i mention",
    "did i tell you",
];

const INTENT: &[&str] = &[
    "remember",
    "save ",
    "save:",
    "save it",
    "save that",
    "store this",
    "store that",
    "note:",
    "note that",
    "please note",
    "also note",
    "don't forget",
    "do not forget",
    "dont forget",
    "keep in mind",
    "for later",
    "memorize",
    "memorise",
    "make a note",
];

const IDENTITY: &[&str] = &[
    "my name is",
    "call me ",
    "i live in",
    "my birthday",
    "my favourite",
    "my favorite",
    "i work at",
    "i work for",
    "my wife",
    "my husband",
    "my partner",
    "my cat",
    "my dog",
    "my son",
    "my daughter",
    "my company is",
    "my timezone",
];

/// Content tokens: lowercase words (keeping `-`/`_` inside codewords), no
/// stop words, at least 3 chars or containing a digit.
pub fn content_tokens(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_')) {
        let t = raw.trim_matches(|c| c == '-' || c == '_').to_lowercase();
        if t.is_empty() || STOP.contains(&t.as_str()) {
            continue;
        }
        if t.chars().count() < 3 && !t.chars().any(|c| c.is_ascii_digit()) {
            continue;
        }
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

fn topic_tokens(text: &str) -> Vec<String> {
    content_tokens(text)
        .into_iter()
        .filter(|t| !CUE_WORDS.contains(&t.as_str()))
        .collect()
}

fn is_specific(t: &str) -> bool {
    t.chars().any(|c| c.is_ascii_digit()) || t.contains('-') || t.contains('_')
}

/// Split into sentences on `.` `!` `?` newline (a `.` inside a token such as
/// `v1.2` or `12.30` does not split). Keeps the terminator.
fn sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        cur.push(c);
        let next = chars.get(i + 1).copied();
        let end = match c {
            '\n' | '!' | '?' => true,
            '.' => next.is_none_or(|n| n.is_whitespace()),
            _ => false,
        };
        if end {
            let s = cur.trim().to_string();
            if !s.is_empty() {
                out.push(s);
            }
            cur.clear();
        }
    }
    let s = cur.trim().to_string();
    if !s.is_empty() {
        out.push(s);
    }
    out
}

fn has_any(lower: &str, list: &[&str]) -> bool {
    list.iter().any(|w| lower.contains(w))
}

/// Drop a leading "Remember this for later:" / "Also note:" style prefix.
fn strip_intent_prefix(s: &str) -> String {
    if let Some((head, tail)) = s.split_once(':') {
        let hl = head.to_lowercase();
        if has_any(&format!("{hl} "), INTENT) && !tail.trim().is_empty() && head.len() < 60 {
            return tail.trim().to_string();
        }
    }
    s.trim().to_string()
}

fn clean_fact(s: &str) -> String {
    let s = s
        .trim()
        .trim_end_matches(['.', '!'])
        .trim()
        .replace(['\n', '\r', '\t'], " ");
    let s: String = s.chars().take(MAX_FACT_CHARS).collect();
    s.trim().to_string()
}

/// Facts the user explicitly gave in one message. `explicit` = the whole
/// text was saved on purpose (memory_save note).
pub fn extract_facts(text: &str, explicit: bool) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() || text.to_lowercase().contains("<memory-context>") {
        return Vec::new();
    }
    if explicit {
        if text.ends_with('?') || content_tokens(text).len() < 2 {
            return Vec::new();
        }
        return vec![clean_fact(&strip_intent_prefix(text))];
    }
    let sents = sentences(text);
    let mut out: Vec<String> = Vec::new();
    for (i, s) in sents.iter().enumerate() {
        if s.ends_with('?') {
            continue;
        }
        let lower = s.to_lowercase();
        let intent = has_any(&format!("{lower} "), INTENT);
        let identity = has_any(&lower, IDENTITY);
        if !intent && !identity {
            continue;
        }
        let body = strip_intent_prefix(s);
        let mut candidate = if content_tokens(&body).len() >= 2 {
            Some(body)
        } else {
            None
        };
        // "My locker code is 4471. Remember that." -> the previous statement.
        if candidate.is_none() && intent && i > 0 {
            let prev = &sents[i - 1];
            if !prev.ends_with('?') && content_tokens(prev).len() >= 2 {
                candidate = Some(strip_intent_prefix(prev));
            }
        }
        if let Some(c) = candidate {
            let c = clean_fact(&c);
            if !c.is_empty() && !out.contains(&c) {
                out.push(c);
            }
        }
        if out.len() >= MAX_FACTS_PER_TEXT {
            break;
        }
    }
    out
}

/// "my demo codeword is X" -> "demo codeword": newer facts with the same
/// subject supersede older ones at recall time.
pub fn fact_subject(fact: &str) -> Option<String> {
    let l = fact.to_lowercase();
    let start = l.find("my ")? + 3;
    let rest = &l[start..];
    let end = [" is ", " are ", " was ", " = "]
        .iter()
        .filter_map(|w| rest.find(w))
        .min()?;
    let subj = rest[..end].trim();
    let n = subj.split_whitespace().count();
    (1..=5).contains(&n).then(|| subj.to_string())
}

pub fn fact_id(fact: &str) -> String {
    let norm: String = fact
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "pyr_fact_{}",
        &crate::hash::sha256_hex(norm.as_bytes())[..24]
    )
}

fn label_ms(ms: i64) -> String {
    zagreb_time::label(ms.div_euclid(1000))
}

/// Start of the Europe/Zagreb day containing `now_ms`.
pub fn zagreb_midnight_ms(now_ms: i64) -> i64 {
    let secs = now_ms.div_euclid(1000);
    let s = zagreb_time::from_unix(secs); // 2026-10-29T20:41:05+01:00
    let b = s.as_bytes();
    let num = |a: usize, z: usize| s.get(a..z).and_then(|x| x.parse::<i64>().ok());
    match (b.get(10), num(11, 13), num(14, 16), num(17, 19)) {
        (Some(b'T'), Some(h), Some(m), Some(sec)) => (secs - (h * 3600 + m * 60 + sec)) * 1000,
        _ => now_ms - 86_400_000,
    }
}

/// Time window (ms, inclusive) and a label for an episodic question, or
/// `None` when the question is not about the past.
pub fn episodic_window(query: &str, now_ms: i64) -> Option<(i64, i64, &'static str)> {
    let l = query.to_lowercase();
    let today = l.contains("today") || l.contains("this morning") || l.contains("tonight");
    if !has_any(&l, EPISODIC_CUES) && !(today && l.contains(" we ")) {
        return None;
    }
    let midnight = zagreb_midnight_ms(now_ms);
    Some(if l.contains("yesterday") {
        (midnight - 86_400_000 - 3_600_000, midnight, "yesterday")
    } else if today {
        (midnight, now_ms, "today")
    } else if l.contains("week") {
        (now_ms - 7 * 86_400_000, now_ms, "last 7 days")
    } else {
        (now_ms - 14 * 86_400_000, now_ms, "last 14 days")
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fact {
    pub id: String,
    pub content: String,
    pub session_key: String,
    pub ts_ms: i64,
    pub subject: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Episode {
    pub session_key: String,
    pub started_ms: i64,
    pub last_ms: i64,
    pub turns: i64,
    pub title: String,
    pub summary: String,
    pub keywords: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snippet {
    pub session_key: String,
    pub role: String,
    pub text: String,
    pub ts_ms: i64,
}

impl Store {
    /// Create the episodes table (additive; writable stores only).
    pub fn ensure_pyramid(&self) -> rusqlite::Result<()> {
        if self.read_only {
            return Ok(());
        }
        self.lock().execute_batch(PYRAMID_SQL)
    }

    /// Insert or refresh one fact (same normalised text = same row).
    pub fn upsert_fact(
        &self,
        content: &str,
        session_key: &str,
        ts_ms: i64,
    ) -> rusqlite::Result<String> {
        let id = fact_id(content);
        if self.read_only {
            return Ok(id);
        }
        let subject = fact_subject(content);
        let meta = json!({"source": "pyramid-extract", "cross_chat": true, "ts_ms": ts_ms,
                          "subject": subject, "origin_session": session_key})
        .to_string();
        let iso = crate::util::iso_utc(ts_ms);
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO l1_records (record_id, content, type, priority, scene_name, session_key, session_id, \
             timestamp_str, created_time, updated_time, metadata_json) VALUES (?, ?, ?, 80, ?, ?, ?, ?, ?, ?, ?)",
            params![id, content, FACT_KIND, FACT_SCENE, session_key, session_key, iso, iso, iso, meta],
        )?;
        if self.fts_available {
            tx.execute("DELETE FROM l1_fts WHERE record_id = ?", params![id])?;
            tx.execute(
                "INSERT INTO l1_fts (content, content_original, record_id, type, priority, scene_name, session_key, \
                 session_id, timestamp_str, timestamp_start, timestamp_end, metadata_json) \
                 VALUES (?, ?, ?, ?, 80, ?, ?, ?, ?, '', '', ?)",
                params![tokenize_for_fts(content), content, id, FACT_KIND, FACT_SCENE, session_key, session_key, iso, meta],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    /// Every stored pyramid fact (bounded), newest first.
    pub fn all_facts(&self, cap: usize) -> Vec<Fact> {
        let conn = self.lock();
        let r = conn
            .prepare_cached(
                "SELECT record_id, content, session_key, metadata_json FROM l1_records \
                 WHERE type = ?1 AND scene_name = ?2 ORDER BY updated_time DESC LIMIT ?3",
            )
            .and_then(|mut st| {
                let rows = st.query_map(params![FACT_KIND, FACT_SCENE, cap as i64], |r| {
                    let meta: String = r.get::<_, Option<String>>(3)?.unwrap_or_default();
                    let m: Value = serde_json::from_str(&meta).unwrap_or(Value::Null);
                    Ok(Fact {
                        id: r.get(0)?,
                        content: r.get(1)?,
                        session_key: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                        ts_ms: m["ts_ms"].as_i64().unwrap_or(0),
                        subject: m["subject"].as_str().map(String::from),
                    })
                })?;
                Ok(rows.filter_map(Result::ok).collect::<Vec<_>>())
            });
        r.unwrap_or_default()
    }

    /// Record one captured turn in its episode row.
    pub fn touch_episode(
        &self,
        session_key: &str,
        user_text: &str,
        ts_ms: i64,
    ) -> rusqlite::Result<()> {
        if self.read_only || session_key.is_empty() {
            return Ok(());
        }
        let conn = self.lock();
        let old: Option<Episode> = conn
            .query_row(
                "SELECT session_key, started_ms, last_ms, turns, title, summary, keywords FROM pyramid_episodes WHERE session_key = ?",
                params![session_key],
                |r| {
                    Ok(Episode {
                        session_key: r.get(0)?,
                        started_ms: r.get(1)?,
                        last_ms: r.get(2)?,
                        turns: r.get(3)?,
                        title: r.get(4)?,
                        summary: r.get(5)?,
                        keywords: r.get(6)?,
                    })
                },
            )
            .ok();
        let ep = merge_episode(old, session_key, user_text, ts_ms);
        conn.execute(
            "INSERT OR REPLACE INTO pyramid_episodes (session_key, started_ms, last_ms, turns, title, summary, keywords) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![ep.session_key, ep.started_ms, ep.last_ms, ep.turns, ep.title, ep.summary, ep.keywords],
        )?;
        Ok(())
    }

    pub fn episodes_between(&self, since_ms: i64, until_ms: i64, limit: usize) -> Vec<Episode> {
        let conn = self.lock();
        conn.prepare_cached(
            "SELECT session_key, started_ms, last_ms, turns, title, summary, keywords FROM pyramid_episodes \
             WHERE last_ms >= ?1 AND started_ms <= ?2 ORDER BY last_ms DESC LIMIT ?3",
        )
        .and_then(|mut st| {
            let rows = st.query_map(params![since_ms, until_ms, limit as i64], |r| {
                Ok(Episode {
                    session_key: r.get(0)?,
                    started_ms: r.get(1)?,
                    last_ms: r.get(2)?,
                    turns: r.get(3)?,
                    title: r.get(4)?,
                    summary: r.get(5)?,
                    keywords: r.get(6)?,
                })
            })?;
            Ok(rows.filter_map(Result::ok).collect::<Vec<_>>())
        })
        .unwrap_or_default()
    }

    /// L0 messages matching any topic token inside a time window, best first.
    pub fn l0_window_search(
        &self,
        tokens: &[String],
        since_ms: i64,
        until_ms: i64,
        limit: usize,
    ) -> Vec<Snippet> {
        if !self.fts_available || tokens.is_empty() {
            return Vec::new();
        }
        let q = tokens
            .iter()
            .map(|t| format!("\"{}\"", t.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" OR ");
        let conn = self.lock();
        conn.prepare_cached(
            "SELECT session_key, role, message_text_original, CAST(timestamp AS INTEGER) FROM l0_fts \
             WHERE l0_fts MATCH ?1 AND CAST(timestamp AS INTEGER) BETWEEN ?2 AND ?3 ORDER BY bm25(l0_fts) LIMIT ?4",
        )
        .and_then(|mut st| {
            let rows = st.query_map(params![q, since_ms, until_ms, limit as i64], |r| {
                Ok(Snippet {
                    session_key: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    role: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    text: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    ts_ms: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                })
            })?;
            Ok(rows.filter_map(Result::ok).collect::<Vec<_>>())
        })
        .unwrap_or_else(|e| {
            crate::log::warn(&format!("[pyramid] window search failed (non-fatal): {e}"));
            Vec::new()
        })
    }

    pub fn episode(&self, session_key: &str) -> Option<Episode> {
        self.episodes_for(&[session_key.to_string()])
            .into_iter()
            .next()
    }

    fn episodes_for(&self, keys: &[String]) -> Vec<Episode> {
        let conn = self.lock();
        keys.iter()
            .filter_map(|k| {
                conn.query_row(
                    "SELECT session_key, started_ms, last_ms, turns, title, summary, keywords FROM pyramid_episodes WHERE session_key = ?",
                    params![k],
                    |r| {
                        Ok(Episode {
                            session_key: r.get(0)?,
                            started_ms: r.get(1)?,
                            last_ms: r.get(2)?,
                            turns: r.get(3)?,
                            title: r.get(4)?,
                            summary: r.get(5)?,
                            keywords: r.get(6)?,
                        })
                    },
                )
                .ok()
            })
            .collect()
    }

    /// Rebuild facts and episodes from L0 (`since_ms` onwards). Idempotent:
    /// facts dedupe by text, episodes are recomputed per conversation.
    pub fn pyramid_backfill(&self, since_ms: i64) -> rusqlite::Result<(usize, usize)> {
        self.ensure_pyramid()?;
        let rows: Vec<(String, String, String, i64)> = {
            let conn = self.lock();
            let mut st = conn.prepare(
                "SELECT session_key, role, message_text, timestamp FROM l0_conversations \
                 WHERE timestamp >= ? ORDER BY timestamp ASC",
            )?;
            let v = st
                .query_map(params![since_ms], |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                        r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                        r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    ))
                })?
                .filter_map(Result::ok)
                .collect();
            v
        };
        let mut eps: std::collections::BTreeMap<String, Episode> = Default::default();
        let mut facts = 0usize;
        for (sk, role, text, ts) in &rows {
            if role != "user" || sk.is_empty() {
                continue;
            }
            for f in extract_facts(text, sk == NOTES_SESSION) {
                self.upsert_fact(&f, sk, *ts)?;
                facts += 1;
            }
            let old = eps.remove(sk);
            eps.insert(sk.clone(), merge_episode(old, sk, text, *ts));
        }
        let conn = self.lock();
        for ep in eps.values() {
            conn.execute(
                "INSERT OR REPLACE INTO pyramid_episodes (session_key, started_ms, last_ms, turns, title, summary, keywords) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                params![ep.session_key, ep.started_ms, ep.last_ms, ep.turns, ep.title, ep.summary, ep.keywords],
            )?;
        }
        Ok((facts, eps.len()))
    }
}

fn one_line(s: &str, n: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= n {
        flat
    } else {
        format!("{}…", flat.chars().take(n).collect::<String>())
    }
}

pub fn merge_episode(
    old: Option<Episode>,
    session_key: &str,
    user_text: &str,
    ts_ms: i64,
) -> Episode {
    let mut ep = old.unwrap_or(Episode {
        session_key: session_key.to_string(),
        started_ms: ts_ms,
        last_ms: ts_ms,
        turns: 0,
        title: String::new(),
        summary: String::new(),
        keywords: String::new(),
    });
    ep.started_ms = ep.started_ms.min(ts_ms);
    ep.last_ms = ep.last_ms.max(ts_ms);
    ep.turns += 1;
    let line = one_line(user_text, 140);
    if ep.title.is_empty() {
        ep.title = one_line(user_text, 90);
    }
    let mut lines: Vec<String> = ep
        .summary
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    if !line.is_empty() && !lines.contains(&line) {
        if lines.len() >= MAX_SUMMARY_LINES {
            lines.pop(); // keep the first lines, refresh the latest one
        }
        lines.push(line);
    }
    ep.summary = lines.join("\n");
    let mut kw: Vec<String> = ep
        .keywords
        .split(' ')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    for t in topic_tokens(user_text) {
        if kw.len() >= MAX_KEYWORDS {
            break;
        }
        if !kw.contains(&t) {
            kw.push(t);
        }
    }
    ep.keywords = kw.join(" ");
    ep
}

/// L1 scene names that are quarantined: the rows stay on disk (no deletes
/// without an owner yes) but are never recalled or searched. Extra names via
/// `OPENMIND_MEMORY_QUARANTINE_SCENES` (separated by `|`).
pub const QUARANTINED_SCENES: &[&str] = &["我是AI助手，和用户聊科技产品知识"];

pub fn is_quarantined_scene(scene: &str) -> bool {
    let s = scene.trim();
    if s.is_empty() {
        return false;
    }
    if QUARANTINED_SCENES.contains(&s) {
        return true;
    }
    std::env::var("OPENMIND_MEMORY_QUARANTINE_SCENES")
        .map(|v| v.split('|').any(|x| !x.trim().is_empty() && x.trim() == s))
        .unwrap_or(false)
}

/// A test or agent-probe conversation (demo-test-*, t39x-*, *test*, pyr-*).
pub fn is_test_session(key: &str) -> bool {
    let k = key.to_lowercase();
    k.contains("test") || k.starts_with("pyr-") || k.starts_with("t39")
}

/// Text that carries a test marker (test codewords saved via notes).
pub fn is_test_text(text: &str) -> bool {
    let t = text.to_lowercase();
    t.contains("demo-test-") || t.contains("test-t39")
}

/// Whether a fact or conversation from `origin` may be shown to `viewer`:
/// test chats only see test material; real chats never see it (so other
/// agents' probes are not presented as the user's own conversations).
pub fn visible(viewer: &str, origin: &str, text: &str) -> bool {
    is_test_session(viewer) || (!is_test_session(origin) && !is_test_text(text))
}

/// Facts relevant to `query`, newest per subject, best first.
pub fn recall_facts(store: &Store, query: &str, viewer: &str) -> Vec<Fact> {
    let q = content_tokens(query);
    if q.is_empty() {
        return Vec::new();
    }
    let need = q.len().min(2);
    let mut scored: Vec<(usize, Fact)> = Vec::new();
    let all: Vec<Fact> = store
        .all_facts(5000)
        .into_iter()
        .filter(|f| visible(viewer, &f.session_key, &f.content))
        .collect();
    // newest first per subject: the first seen wins
    let mut subjects_newest: std::collections::HashMap<String, i64> = Default::default();
    for f in &all {
        if let Some(s) = &f.subject {
            let e = subjects_newest.entry(s.clone()).or_insert(f.ts_ms);
            if f.ts_ms > *e {
                *e = f.ts_ms;
            }
        }
    }
    for f in all {
        if let Some(s) = &f.subject {
            if subjects_newest.get(s).is_some_and(|&n| n > f.ts_ms) {
                continue; // superseded by a newer fact about the same subject
            }
        }
        let ft = content_tokens(&f.content);
        let hits: Vec<&String> = q.iter().filter(|t| ft.contains(t)).collect();
        let strong = hits.iter().any(|t| is_specific(t));
        if hits.len() >= need || strong {
            scored.push((hits.len(), f));
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.ts_ms.cmp(&a.1.ts_ms)));
    scored
        .into_iter()
        .take(MAX_RECALL_FACTS)
        .map(|(_, f)| f)
        .collect()
}

#[derive(Debug, Clone)]
pub struct EpisodeHit {
    pub episode: Option<Episode>,
    pub session_key: String,
    pub snippets: Vec<Snippet>,
}

/// Past conversations relevant to `query` inside `[since, until]`, the
/// current conversation excluded.
pub fn recall_episodes(
    store: &Store,
    query: &str,
    since_ms: i64,
    until_ms: i64,
    exclude_session: &str,
    limit: usize,
) -> Vec<EpisodeHit> {
    let topic = topic_tokens(query);
    let mut hits: Vec<EpisodeHit> = Vec::new();
    if topic.is_empty() {
        for ep in store.episodes_between(since_ms, until_ms, limit + 1) {
            if ep.session_key == exclude_session
                || hits.len() >= limit
                || !visible(exclude_session, &ep.session_key, &ep.summary)
            {
                continue;
            }
            hits.push(EpisodeHit {
                session_key: ep.session_key.clone(),
                episode: Some(ep),
                snippets: Vec::new(),
            });
        }
        return hits;
    }
    let need = topic.len().min(2);
    for sn in store.l0_window_search(&topic, since_ms, until_ms, 60) {
        if sn.session_key == exclude_session
            || sn.text.to_lowercase().contains("<memory-context>")
            || !visible(exclude_session, &sn.session_key, &sn.text)
        {
            continue;
        }
        let st = content_tokens(&sn.text);
        let n = topic.iter().filter(|t| st.contains(t)).count();
        let strong = topic.iter().any(|t| is_specific(t) && st.contains(t));
        if n < need && !strong {
            continue;
        }
        match hits.iter_mut().find(|h| h.session_key == sn.session_key) {
            Some(h) => {
                if h.snippets.len() < 3 {
                    h.snippets.push(sn)
                }
            }
            None => {
                if hits.len() < limit {
                    hits.push(EpisodeHit {
                        session_key: sn.session_key.clone(),
                        episode: store.episode(&sn.session_key),
                        snippets: vec![sn],
                    })
                }
            }
        }
    }
    for h in &mut hits {
        h.snippets.sort_by_key(|s| s.ts_ms);
    }
    hits
}

pub fn render_facts(facts: &[Fact]) -> String {
    facts
        .iter()
        .map(|f| {
            let when = if f.ts_ms > 0 {
                format!(" (saved {})", label_ms(f.ts_ms))
            } else {
                String::new()
            };
            format!("- {}{when}", f.content)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render_episodes(hits: &[EpisodeHit]) -> String {
    let mut out = Vec::new();
    for h in hits {
        match &h.episode {
            Some(e) => out.push(format!(
                "- Conversation {} ({} to {}, {} turns), started with: \"{}\"",
                h.session_key,
                label_ms(e.started_ms),
                label_ms(e.last_ms),
                e.turns,
                e.title
            )),
            None => out.push(format!("- Conversation {}", h.session_key)),
        }
        for s in &h.snippets {
            out.push(format!(
                "  - {} {} said: \"{}\"",
                label_ms(s.ts_ms),
                if s.role == "assistant" {
                    "assistant"
                } else {
                    "user"
                },
                one_line(&s.text, 300)
            ));
        }
        if h.snippets.is_empty() {
            if let Some(e) = &h.episode {
                for l in e.summary.lines().take(3) {
                    out.push(format!("  - user: \"{l}\""));
                }
            }
        }
    }
    out.join("\n")
}

pub fn episode_json(h: &EpisodeHit) -> Value {
    let e = h.episode.as_ref();
    json!({
        "session_key": h.session_key,
        "started": e.map(|e| zagreb_time::from_unix(e.started_ms.div_euclid(1000))),
        "last": e.map(|e| zagreb_time::from_unix(e.last_ms.div_euclid(1000))),
        "turns": e.map(|e| e.turns),
        "title": e.map(|e| e.title.clone()),
        "summary": e.map(|e| e.summary.clone()),
        "keywords": e.map(|e| e.keywords.clone()),
        "matches": h.snippets.iter().map(|s| json!({
            "time": zagreb_time::from_unix(s.ts_ms.div_euclid(1000)), "role": s.role, "text": one_line(&s.text, 500)
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_explicit_facts_only() {
        let f = extract_facts(
            "Remember this for later: my demo codeword is demo-test-1a2b. Please save it to memory.",
            false,
        );
        assert_eq!(f, vec!["my demo codeword is demo-test-1a2b"]);
        let f = extract_facts(
            "Also note: earlier today we agreed the lighthouse plan moves the investor demo to Friday at 10:25. Save that too.",
            false,
        );
        assert_eq!(
            f,
            vec!["earlier today we agreed the lighthouse plan moves the investor demo to Friday at 10:25"]
        );
        assert!(extract_facts("What is my demo codeword? Check your memory.", false).is_empty());
        assert!(extract_facts("How do I build the gateway.", false).is_empty());
        assert_eq!(
            extract_facts("My locker code is 4471. Remember that.", false),
            vec!["My locker code is 4471"]
        );
        assert_eq!(extract_facts("Call me Mate.", false), vec!["Call me Mate"]);
        assert!(extract_facts("Call me.", false).is_empty()); // too little content
        assert_eq!(
            extract_facts("My name is Mate Grajczyk and I run the demo.", false),
            vec!["My name is Mate Grajczyk and I run the demo"]
        );
        assert!(extract_facts(
            "x <memory-context>remember my pin is 1</memory-context>",
            false
        )
        .is_empty());
        assert_eq!(
            extract_facts("the staging port is 19070", true),
            vec!["the staging port is 19070"]
        );
        assert!(extract_facts("is it 19070?", true).is_empty());
    }

    #[test]
    fn subjects_and_ids() {
        assert_eq!(
            fact_subject("my demo codeword is demo-test-1").as_deref(),
            Some("demo codeword")
        );
        assert_eq!(fact_subject("the sky is blue"), None);
        assert_eq!(fact_id("A  b"), fact_id("a b"));
    }

    #[test]
    fn episodic_windows() {
        let now = 1_793_302_861_000; // 2026-10-29 ~20:41 CET
        let mid = zagreb_midnight_ms(now);
        assert!(now - mid < 86_400_000 && now > mid);
        assert_eq!(
            zagreb_time::from_unix(mid / 1000).get(11..19),
            Some("00:00:00")
        );
        let (s, e, l) =
            episodic_window("What did we talk about earlier today about X?", now).unwrap();
        assert_eq!((s, e, l), (mid, now, "today"));
        assert_eq!(
            episodic_window("what did we discuss yesterday", now)
                .unwrap()
                .2,
            "yesterday"
        );
        assert!(episodic_window("What is my demo codeword?", now).is_none());
        assert!(episodic_window("What is my cat's name?", now).is_none());
    }

    #[test]
    fn tokens() {
        assert_eq!(
            content_tokens("What is my cat's name?"),
            vec!["cat", "name"]
        );
        assert_eq!(
            content_tokens("What is my demo codeword?"),
            vec!["demo", "codeword"]
        );
        assert_eq!(
            topic_tokens("What did we talk about earlier today about the lighthouse plan? What time did we agree?"),
            vec!["lighthouse", "plan"]
        );
    }

    fn store(name: &str) -> Store {
        let d = std::env::temp_dir().join(format!("om-pyr-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let s = Store::open(&d.join("vectors.db"), false).unwrap();
        s.ensure_pyramid().unwrap();
        s
    }

    #[test]
    fn fact_recall_supersedes_and_does_not_guess() {
        let s = store("facts");
        s.upsert_fact("my demo codeword is blue-harbor-42", "a", 1_000_000)
            .unwrap();
        s.upsert_fact("my demo codeword is demo-test-77aa", "b", 2_000_000)
            .unwrap();
        s.upsert_fact("my favourite tea is oolong", "c", 1_500_000)
            .unwrap();
        let got = recall_facts(&s, "What is my demo codeword?", "demo-test-viewer");
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].content, "my demo codeword is demo-test-77aa");
        assert!(recall_facts(&s, "What is my cat's name?", "demo-test-viewer").is_empty());
        assert!(
            recall_facts(&s, "What does zebra-quartz-1 unlock?", "demo-test-viewer").is_empty()
        );
        assert_eq!(
            recall_facts(&s, "what is my favourite tea", "demo-test-viewer").len(),
            1
        );
        // same text again = same row
        s.upsert_fact("My favourite tea is oolong", "d", 3_000_000)
            .unwrap();
        assert_eq!(s.all_facts(100).len(), 3);
    }

    #[test]
    fn test_material_hidden_from_real_chats_and_quarantine() {
        let s = store("vis");
        s.upsert_fact("my demo codeword is blue-harbor-42", "api-1", 1_000_000)
            .unwrap();
        s.upsert_fact(
            "my demo codeword is demo-test-77aa",
            "hermes-mcp-notes",
            2_000_000,
        )
        .unwrap();
        s.upsert_fact("my locker number is 4417", "t395-a-0029", 2_500_000)
            .unwrap();
        let real = recall_facts(&s, "What is my demo codeword?", "api-real");
        assert_eq!(real.len(), 1);
        assert_eq!(real[0].content, "my demo codeword is blue-harbor-42");
        assert!(recall_facts(&s, "my locker number", "api-real").is_empty());
        assert_eq!(recall_facts(&s, "my locker number", "t395-b-0029").len(), 1);
        assert!(is_quarantined_scene("我是AI助手，和用户聊科技产品知识"));
        assert!(!is_quarantined_scene("pyramid"));
        assert!(!is_quarantined_scene(""));
        assert!(is_test_session("demo-test-pyr-after-r1-s1"));
        assert!(!is_test_session("api-3f2a9c"));
    }

    #[test]
    fn episodes_group_by_conversation_and_window() {
        let s = store("eps");
        let t0 = 1_793_290_000_000;
        for (i, (sk, txt)) in [
            (
                "s1",
                "the lighthouse plan moves the investor demo to Friday at 10:25",
            ),
            ("s1", "and the lighthouse plan needs a new budget"),
            ("s2", "unrelated chat about cats and plans"),
        ]
        .iter()
        .enumerate()
        {
            let ts = t0 + i as i64 * 60_000;
            s.upsert_l0(&crate::store::L0Record {
                id: format!("r{i}"),
                session_key: sk.to_string(),
                session_id: sk.to_string(),
                role: "user".into(),
                message_text: txt.to_string(),
                recorded_at: crate::util::iso_utc(ts),
                timestamp: ts,
            })
            .unwrap();
            s.touch_episode(sk, txt, ts).unwrap();
        }
        let h = recall_episodes(
            &s,
            "what did we say about the lighthouse plan earlier",
            t0 - 1,
            t0 + 600_000,
            "now",
            3,
        );
        assert_eq!(h.len(), 1, "{h:?}");
        assert_eq!(h[0].session_key, "s1");
        assert_eq!(h[0].snippets.len(), 2);
        assert_eq!(h[0].episode.as_ref().unwrap().turns, 2);
        let text = render_episodes(&h);
        assert!(
            text.contains("Friday at 10:25") && text.contains("CET"),
            "{text}"
        );
        // window excludes, current session excluded
        assert!(
            recall_episodes(&s, "lighthouse plan", t0 + 600_000, t0 + 900_000, "now", 3).is_empty()
        );
        assert!(recall_episodes(&s, "lighthouse plan", t0 - 1, t0 + 600_000, "s1", 3).is_empty());
        // backfill is idempotent
        let (f1, e1) = s.pyramid_backfill(0).unwrap();
        let (f2, e2) = s.pyramid_backfill(0).unwrap();
        assert_eq!((f1, e1), (f2, e2));
        assert_eq!(s.episode("s1").unwrap().turns, 2);
    }
}
