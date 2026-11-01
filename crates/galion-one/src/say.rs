// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The typed question line, `POST /admin/say` (2026-11-01 CET, ask fix).
//!
//! Why this exists: the line used to send `max_tokens: 180` with the
//! gateway's default reasoning (`high`), so a longer question spent the
//! whole budget thinking, the model returned no words (`finish_reason:
//! length`), the gateway answered 502 "output truncated" and the screen
//! said "No answer came back. Try again." It also gave up after 22 s and
//! refused any question while another was in flight ("Wait").
//!
//! Now:
//! * low reasoning and a real budget ([`MAX_TOKENS`]); a truncated or
//!   failed turn is retried once, quietly, with reasoning off;
//! * every question carries a client id. The answer is computed once per
//!   id in the background and held for [`KEEP`]; the same id posted again
//!   (a retry, a phone waking up, a dropped connection) joins the same
//!   answer instead of asking twice. A request waits at most [`HOLD`] and
//!   then says `pending`, so no proxy ever times out on a long answer;
//! * failures come back as a code (`cap`, `down`, `slow`, `empty`) the
//!   page turns into a calm, specific line.

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Output budget for one answer (reasoning included).
pub const MAX_TOKENS: u64 = 1500;
/// Reasoning for the first try; the quiet retry turns it off.
pub const REASONING: &str = "low";
/// Longest answer kept (characters).
pub const ANSWER_CHARS: usize = 12_000;
/// Longest a single page request waits before it answers `pending`.
pub const HOLD: Duration = Duration::from_secs(25);
/// Wait for a request without an id (the /room page): under the edge's 100 s.
pub const HOLD_NO_ID: Duration = Duration::from_secs(85);
/// Read timeout towards the gateway for one try.
pub const UPSTREAM: Duration = Duration::from_secs(170);
/// How long an answer is held for its id.
pub const KEEP: Duration = Duration::from_secs(15 * 60);

/// One question as the page sends it.
#[derive(Debug, Default, PartialEq)]
pub struct Ask {
    pub text: String,
    pub thread: String,
    pub fresh: bool,
    pub id: String,
}

/// Parse the request body with a real JSON parser (the old substring
/// reader mixed up keys when the question itself quoted "thread").
pub fn parse_ask(body: &[u8]) -> Ask {
    let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    Ask {
        text: s("text"),
        thread: s("thread"),
        fresh: v.get("fresh").and_then(Value::as_bool).unwrap_or(false),
        id: s("id"),
    }
}

/// A client question id: 8..=64 of `[A-Za-z0-9_-]`.
pub fn valid_id(id: &str) -> bool {
    (8..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The gateway request body for one try.
pub fn payload(model: &str, system: &str, text: &str, reasoning: &str) -> String {
    json!({
        "model": model,
        "stream": false,
        "max_tokens": MAX_TOKENS,
        "reasoning_effort": reasoning,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": text},
        ],
    })
    .to_string()
}

/// What one gateway try came back with.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Answer(String),
    /// Budget spent before any words (or an empty reply).
    Truncated,
    /// Daily or session spend cap (402).
    Cap,
    /// Gateway busy (503/429).
    Busy,
    /// No connection to the gateway.
    Down,
    /// Connected but no reply inside [`UPSTREAM`].
    Slow,
    /// Any other error status.
    Failed(u16),
}

impl Outcome {
    pub fn name(&self) -> String {
        match self {
            Outcome::Answer(_) => "ok".into(),
            Outcome::Truncated => "truncated".into(),
            Outcome::Cap => "cap".into(),
            Outcome::Busy => "busy".into(),
            Outcome::Down => "down".into(),
            Outcome::Slow => "slow".into(),
            Outcome::Failed(c) => format!("failed-{c}"),
        }
    }
}

/// Read a gateway reply. `None` = could not connect; `Some((0, _))` = the
/// read timed out before a status line.
pub fn classify(reply: Option<(u16, &[u8])>) -> Outcome {
    let Some((status, body)) = reply else {
        return Outcome::Down;
    };
    let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    match status {
        0 => Outcome::Slow,
        200 => {
            let text = v["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .trim();
            if text.is_empty() {
                Outcome::Truncated
            } else {
                Outcome::Answer(text.chars().take(ANSWER_CHARS).collect())
            }
        }
        402 => Outcome::Cap,
        429 | 503 => Outcome::Busy,
        _ => {
            let msg = v["error"]["message"].as_str().unwrap_or("").to_lowercase();
            if msg.contains("spend cap") || msg.contains("call cap") {
                Outcome::Cap
            } else if msg.contains("truncat") {
                Outcome::Truncated
            } else {
                Outcome::Failed(status)
            }
        }
    }
}

/// Ask with at most one quiet retry. `call(reasoning)` makes one gateway
/// try; `pause` sleeps between tries (a no-op in tests).
pub fn run(
    mut call: impl FnMut(&str) -> Outcome,
    mut pause: impl FnMut(Duration),
) -> (Outcome, usize) {
    let first = call(REASONING);
    let retry = match first {
        Outcome::Truncated | Outcome::Failed(_) => Some(("none", Duration::ZERO)),
        Outcome::Down => Some((REASONING, Duration::from_secs(2))),
        Outcome::Busy => Some((REASONING, Duration::from_secs(3))),
        _ => None,
    };
    let Some((reasoning, wait)) = retry else {
        return (first, 1);
    };
    pause(wait);
    (call(reasoning), 2)
}

/// The JSON the page gets for a finished question. Non-answers keep a
/// calm `say` for the older /room page and an `error` code for this one.
pub fn reply_json(o: &Outcome, thread: &str) -> Vec<u8> {
    let (say, error) = match o {
        Outcome::Answer(s) => (s.as_str(), ""),
        Outcome::Cap => (
            "Today's answer budget is used up. Answers come back after midnight.",
            "cap",
        ),
        Outcome::Down => (
            "The agent is restarting. Your question was not sent.",
            "down",
        ),
        Outcome::Slow => (
            "This one took the agent too long. The conversation is kept.",
            "slow",
        ),
        Outcome::Truncated | Outcome::Busy | Outcome::Failed(_) => (
            "The agent came back without words. Saying it differently usually helps.",
            "empty",
        ),
    };
    let mut v = json!({"say": say, "thread": thread});
    if !error.is_empty() {
        v["error"] = json!(error);
    }
    v.to_string().into_bytes()
}

pub fn pending_json(id: &str, thread: &str) -> Vec<u8> {
    json!({"pending": true, "id": id, "thread": thread})
        .to_string()
        .into_bytes()
}

// ---- Held answers, one per question id.

struct Job {
    thread: String,
    done: Option<Vec<u8>>,
    at: Instant,
}

/// Question id -> its answer (or "still working").
pub struct Jobs {
    map: Mutex<HashMap<String, Job>>,
    cv: Condvar,
}

/// [`Jobs::begin`]: the caller runs the question, or someone already is.
#[derive(Debug, PartialEq)]
pub enum Begin {
    New(String),
    Joined(String),
}

/// [`Jobs::wait`].
#[derive(Debug, PartialEq)]
pub enum Poll {
    Done(Vec<u8>),
    Pending(String),
    Unknown,
}

impl Default for Jobs {
    fn default() -> Self {
        Self::new()
    }
}

impl Jobs {
    pub fn new() -> Self {
        Jobs {
            map: Mutex::new(HashMap::new()),
            cv: Condvar::new(),
        }
    }

    /// Register `id`; `thread` is only called for a new id.
    pub fn begin(&self, id: &str, thread: impl FnOnce() -> String) -> Begin {
        let mut m = self.map.lock().unwrap_or_else(|e| e.into_inner());
        m.retain(|_, j| j.at.elapsed() < KEEP);
        if let Some(j) = m.get(id) {
            return Begin::Joined(j.thread.clone());
        }
        let t = thread();
        m.insert(
            id.to_string(),
            Job {
                thread: t.clone(),
                done: None,
                at: Instant::now(),
            },
        );
        Begin::New(t)
    }

    pub fn finish(&self, id: &str, body: Vec<u8>) {
        let mut m = self.map.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(j) = m.get_mut(id) {
            j.done = Some(body);
            j.at = Instant::now();
        }
        self.cv.notify_all();
    }

    /// Wait up to `hold` for the answer to `id`.
    pub fn wait(&self, id: &str, hold: Duration) -> Poll {
        let end = Instant::now() + hold;
        let mut m = self.map.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            match m.get(id) {
                None => return Poll::Unknown,
                Some(j) => {
                    if let Some(b) = &j.done {
                        return Poll::Done(b.clone());
                    }
                    let now = Instant::now();
                    if now >= end {
                        return Poll::Pending(j.thread.clone());
                    }
                    m = self
                        .cv
                        .wait_timeout(m, end - now)
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
            }
        }
    }
}

pub fn jobs() -> &'static Jobs {
    static J: OnceLock<Jobs> = OnceLock::new();
    J.get_or_init(Jobs::new)
}

// ---- One turn at a time per conversation (keeps the transcript in order);
// different conversations run side by side.

fn lanes() -> &'static (Mutex<HashSet<String>>, Condvar) {
    static L: OnceLock<(Mutex<HashSet<String>>, Condvar)> = OnceLock::new();
    L.get_or_init(|| (Mutex::new(HashSet::new()), Condvar::new()))
}

pub struct Lane(String);

/// Hold the conversation `thread` (waits up to `max` for an earlier turn).
pub fn lane(thread: &str, max: Duration) -> Lane {
    let (m, cv) = lanes();
    let end = Instant::now() + max;
    let mut set = m.lock().unwrap_or_else(|e| e.into_inner());
    while set.contains(thread) {
        let now = Instant::now();
        if now >= end {
            break;
        }
        set = cv
            .wait_timeout(set, end - now)
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
    set.insert(thread.to_string());
    Lane(thread.to_string())
}

impl Drop for Lane {
    fn drop(&mut self) {
        let (m, cv) = lanes();
        let mut set = m.lock().unwrap_or_else(|e| e.into_inner());
        set.remove(&self.0);
        cv.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// The 21:19 CET failure: a 502 "output truncated" (reasoning ate the
    /// budget) is retried once with reasoning off and answers.
    #[test]
    fn truncated_is_retried_quietly_with_reasoning_off() {
        let body = br#"{"error":{"message":"output truncated","type":"agent_error","code":"agent_failed"}}"#;
        assert_eq!(classify(Some((502, body))), Outcome::Truncated);
        let mut seen = Vec::new();
        let (o, tries) = run(
            |r| {
                seen.push(r.to_string());
                if seen.len() == 1 {
                    Outcome::Truncated
                } else {
                    Outcome::Answer("Here.".into())
                }
            },
            |_| {},
        );
        assert_eq!((o, tries), (Outcome::Answer("Here.".into()), 2));
        assert_eq!(seen, ["low", "none"]);
    }

    #[test]
    fn cap_and_slow_are_not_retried() {
        for first in [Outcome::Cap, Outcome::Slow, Outcome::Answer("a".into())] {
            let mut n = 0;
            let want = format!("{first:?}");
            let (o, tries) = run(
                |_| {
                    n += 1;
                    match &first {
                        Outcome::Cap => Outcome::Cap,
                        Outcome::Slow => Outcome::Slow,
                        _ => Outcome::Answer("a".into()),
                    }
                },
                |_| {},
            );
            assert_eq!((format!("{o:?}"), tries, n), (want, 1, 1));
        }
    }

    #[test]
    fn down_and_busy_retry_after_a_pause() {
        let mut paused = Vec::new();
        let mut n = 0;
        let (o, _) = run(
            |_| {
                n += 1;
                if n == 1 {
                    Outcome::Down
                } else {
                    Outcome::Answer("ok".into())
                }
            },
            |d| paused.push(d),
        );
        assert_eq!(o, Outcome::Answer("ok".into()));
        assert_eq!(paused, [Duration::from_secs(2)]);
    }

    #[test]
    fn classify_reads_the_gateway() {
        let ok = br#"{"choices":[{"index":0,"message":{"role":"assistant","content":"Hi \"there\"","reasoning_content":"x"},"finish_reason":"stop"}]}"#;
        assert_eq!(
            classify(Some((200, ok))),
            Outcome::Answer("Hi \"there\"".into())
        );
        let null = br#"{"choices":[{"message":{"content":null,"role":"assistant"}}]}"#;
        assert_eq!(classify(Some((200, null))), Outcome::Truncated);
        let cap = br#"{"error":{"message":"daily spend cap reached ($21.5 of $21.50)"}}"#;
        assert_eq!(classify(Some((402, cap))), Outcome::Cap);
        assert_eq!(classify(Some((503, b""))), Outcome::Busy);
        assert_eq!(classify(Some((0, b""))), Outcome::Slow);
        assert_eq!(classify(None), Outcome::Down);
        assert_eq!(classify(Some((500, b"{}"))), Outcome::Failed(500));
        // Long answers are no longer cut at 600 characters.
        let long = "a".repeat(5000);
        let body = format!(r#"{{"choices":[{{"message":{{"content":"{long}"}}}}]}}"#);
        assert_eq!(
            classify(Some((200, body.as_bytes()))),
            Outcome::Answer(long)
        );
    }

    #[test]
    fn payload_has_budget_and_reasoning() {
        let p: Value = serde_json::from_str(&payload("m", "sys", "q \"x\"", "low")).unwrap();
        assert_eq!(p["max_tokens"], MAX_TOKENS);
        assert_eq!(p["reasoning_effort"], "low");
        assert_eq!(p["stream"], false);
        assert_eq!(p["messages"][1]["content"], "q \"x\"");
        const { assert!(MAX_TOKENS >= 1000, "180 was the bug") };
    }

    #[test]
    fn parse_ask_is_a_real_parser() {
        let a = parse_ask(
            br#"{"text":"say \"thread\":\"x\" and \"fresh\":true","thread":"one-1","fresh":false,"id":"q-abcdef12"}"#,
        );
        assert_eq!(a.text, "say \"thread\":\"x\" and \"fresh\":true");
        assert_eq!(a.thread, "one-1");
        assert!(!a.fresh);
        assert_eq!(a.id, "q-abcdef12");
        assert_eq!(parse_ask(b"not json"), Ask::default());
        assert!(valid_id("q-abcdef12") && !valid_id("short") && !valid_id("q-abc def12"));
    }

    #[test]
    fn replies_never_say_try_again() {
        for o in [
            Outcome::Cap,
            Outcome::Down,
            Outcome::Slow,
            Outcome::Truncated,
            Outcome::Busy,
            Outcome::Failed(502),
        ] {
            let v: Value = serde_json::from_slice(&reply_json(&o, "one-t")).unwrap();
            let say = v["say"].as_str().unwrap();
            assert!(!say.to_lowercase().contains("try again"), "{say}");
            assert!(!say.is_empty() && say != "Again" && say != "Wait");
            assert!(v["error"].is_string());
            assert_eq!(v["thread"], "one-t");
        }
        let v: Value =
            serde_json::from_slice(&reply_json(&Outcome::Answer("A".into()), "t")).unwrap();
        assert_eq!((v["say"].as_str(), v.get("error")), (Some("A"), None));
    }

    /// Same id twice = one question to the gateway; the second request
    /// gets the same answer (idempotency key).
    #[test]
    fn same_id_joins_instead_of_asking_twice() {
        let jobs = Arc::new(Jobs::new());
        let mut threads_made = 0;
        assert_eq!(
            jobs.begin("q-11111111", || {
                threads_made += 1;
                "one-a".into()
            }),
            Begin::New("one-a".into())
        );
        assert_eq!(
            jobs.begin("q-11111111", || {
                threads_made += 1;
                "one-b".into()
            }),
            Begin::Joined("one-a".into())
        );
        assert_eq!(threads_made, 1);
        assert_eq!(
            jobs.wait("q-11111111", Duration::from_millis(10)),
            Poll::Pending("one-a".into())
        );
        let j2 = jobs.clone();
        let h = std::thread::spawn(move || j2.wait("q-11111111", Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(50));
        jobs.finish("q-11111111", b"{\"say\":\"x\"}".to_vec());
        assert_eq!(h.join().unwrap(), Poll::Done(b"{\"say\":\"x\"}".to_vec()));
        assert_eq!(
            jobs.wait("q-11111111", Duration::ZERO),
            Poll::Done(b"{\"say\":\"x\"}".to_vec())
        );
        assert_eq!(jobs.wait("q-nope0000", Duration::ZERO), Poll::Unknown);
    }

    #[test]
    fn one_turn_at_a_time_per_thread() {
        let a = lane("one-lane-test", Duration::from_secs(1));
        let t = Instant::now();
        let h = std::thread::spawn(|| {
            let _b = lane("one-lane-test", Duration::from_secs(5));
        });
        std::thread::sleep(Duration::from_millis(150));
        drop(a);
        h.join().unwrap();
        assert!(t.elapsed() >= Duration::from_millis(150));
        // Another thread is never held up.
        let _c = lane("one-lane-other", Duration::from_millis(1));
    }
}
