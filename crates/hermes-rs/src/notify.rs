// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Owner notify channel (publish blocker B5): Hermes tells the owner about
//! things that wait for them (proposals awaiting approval, refusals, budget
//! stops) through an existing Telegram bot.
//!
//! * Config is env only: `TELEGRAM_BOT_TOKEN` + `TELEGRAM_CHAT_ID`
//!   (`TELEGRAM_HOME_CHANNEL` accepted as the chat, same as galion-nerve).
//!   Nothing is read from or written to a file, nothing goes into git.
//! * The token is never printed, logged, returned or put on argv: the curl
//!   transport gets the URL (which carries the token, as the Bot API wants)
//!   in a curl config on stdin, and every error string is scrubbed of it.
//! * No channel configured is NOT an error: [`Outcome::NoChannelConfigured`]
//!   is a logged no-op (kind + length only, never the text), same status
//!   string the cortex dispatcher uses (`no-channel-configured`).
//! * Bounded: at most `HERMES_RS_NOTIFY_PER_HOUR` sends per rolling hour
//!   (default 6, hard cap 30), text capped at 3800 chars and run through
//!   [`crate::redact`] first.
//! * `HERMES_RS_TELEGRAM_API_BASE` (tests, mocks) may only point at loopback
//!   or the real `https://api.telegram.org`, so a bad setting can never hand
//!   the token to a third host.

use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

pub const TELEGRAM_API: &str = "https://api.telegram.org";
pub const DEFAULT_PER_HOUR: u32 = 6;
pub const MAX_PER_HOUR: u32 = 30;
/// Telegram's limit is 4096 chars; keep room for the prefix.
pub const MAX_TEXT_CHARS: usize = 3800;

/// What happened to one notification. `as_str` values are stable (they show
/// up in cortex/Hermes reports and the gateway reply).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// No token or no chat id in the env: logged no-op.
    NoChannelConfigured,
    Sent,
    /// The token has characters a Bot API token never has; nothing sent.
    BadToken,
    /// The chat id is not `-?digits` or `@name`; nothing sent.
    BadChat,
    /// `HERMES_RS_TELEGRAM_API_BASE` is not loopback / api.telegram.org.
    BadApiBase,
    /// Our own per-hour cap is reached; nothing sent.
    RateLimited,
    /// Empty text after cleaning; nothing sent.
    EmptyText,
    /// Telegram or the transport refused / failed. The detail never holds the token.
    Failed(String),
}

impl Outcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Outcome::NoChannelConfigured => "no-channel-configured",
            Outcome::Sent => "telegram-sent",
            Outcome::BadToken => "bad-token",
            Outcome::BadChat => "bad-chat",
            Outcome::BadApiBase => "bad-api-base",
            Outcome::RateLimited => "rate-limited",
            Outcome::EmptyText => "empty-text",
            Outcome::Failed(_) => "telegram-failed",
        }
    }

    pub fn to_json(&self) -> Value {
        match self {
            Outcome::Failed(d) => json!({"notify": self.as_str(), "detail": d}),
            o => json!({"notify": o.as_str()}),
        }
    }
}

/// One HTTPS POST. `url` carries the bot token: implementations must keep
/// it off argv and out of every log line and error.
pub trait Transport: Send + Sync {
    fn post_json(&self, url: &str, body: &str, timeout_s: u64) -> Result<(u16, String), String>;
}

/// System `curl`, same pattern as [`crate::provider::CurlProvider`]: URL,
/// headers and body go in a curl config on stdin, never argv.
pub struct CurlTransport {
    pub curl: String,
}

impl Default for CurlTransport {
    fn default() -> Self {
        CurlTransport {
            curl: "curl".into(),
        }
    }
}

impl CurlTransport {
    /// argv for curl. Public so tests can prove no secret is on it.
    pub fn argv(timeout_s: u64) -> Vec<String> {
        vec![
            "-sS".into(),
            "--connect-timeout".into(),
            "10".into(),
            "--max-time".into(),
            timeout_s.max(1).to_string(),
            "-K".into(),
            "-".into(),
        ]
    }

    /// The curl config fed on stdin (holds the token inside `url`).
    pub fn config(url: &str, body: &str) -> String {
        use crate::provider::curl_quote;
        format!(
            "url = {}\nrequest = \"POST\"\nheader = \"Content-Type: application/json\"\ndata-binary = {}\nwrite-out = \"\\n%{{http_code}}\"\n",
            curl_quote(url),
            curl_quote(body)
        )
    }
}

impl Transport for CurlTransport {
    fn post_json(&self, url: &str, body: &str, timeout_s: u64) -> Result<(u16, String), String> {
        let mut child = Command::new(&self.curl)
            .args(Self::argv(timeout_s))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("curl: {e}"))?;
        {
            let mut si = child.stdin.take().ok_or("curl stdin")?;
            si.write_all(Self::config(url, body).as_bytes())
                .map_err(|e| format!("curl stdin: {e}"))?;
        }
        let mut out = String::new();
        if let Some(mut so) = child.stdout.take() {
            let _ = so.by_ref().take(64 * 1024).read_to_string(&mut out);
        }
        let mut err = String::new();
        if let Some(mut se) = child.stderr.take() {
            let _ = se.by_ref().take(4096).read_to_string(&mut err);
        }
        let st = child.wait().map_err(|e| format!("curl: {e}"))?;
        let (resp, code) = out.rsplit_once('\n').unwrap_or(("", out.as_str()));
        match code.trim().parse::<u16>() {
            Ok(c) if c > 0 => Ok((c, resp.to_string())),
            _ => Err(format!(
                "curl exit {:?}: {}",
                st.code(),
                err.trim().chars().take(300).collect::<String>()
            )),
        }
    }
}

#[derive(Clone)]
struct Channel {
    token: String,
    chat: String,
    api_base: String,
}

/// Owner notifier. Cheap to share (`Arc`); `send` is thread-safe.
pub struct Notifier {
    channel: Option<Channel>,
    /// Set when the env had a channel but it is unusable (bad token, ...).
    config_error: Option<Outcome>,
    transport: Box<dyn Transport>,
    per_hour: u32,
    sent: Mutex<VecDeque<u64>>,
    clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    log: Arc<dyn Fn(&str) + Send + Sync>,
    pub timeout_s: u64,
}

fn token_ok(t: &str) -> bool {
    // Bot API tokens look like `123456789:AA...` (digits, colon, base64url-ish).
    let Some((id, rest)) = t.split_once(':') else {
        return false;
    };
    !id.is_empty()
        && id.bytes().all(|b| b.is_ascii_digit())
        && rest.len() >= 8
        && rest
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn chat_ok(c: &str) -> bool {
    let digits = c.strip_prefix('-').unwrap_or(c);
    (!digits.is_empty() && digits.len() <= 20 && digits.bytes().all(|b| b.is_ascii_digit()))
        || c.strip_prefix('@').is_some_and(|n| {
            (5..=32).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
}

/// Only loopback (mocks) or the real Bot API host may receive the token.
pub fn api_base_ok(base: &str) -> bool {
    let b = base.trim_end_matches('/');
    if b == TELEGRAM_API {
        return true;
    }
    let Some(rest) = b.strip_prefix("http://") else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or("");
    let host = if let Some(h) = host.strip_prefix('[') {
        h.split(']').next().unwrap_or("")
    } else {
        host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host)
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

fn now_secs() -> u64 {
    crate::time::now_secs().max(0) as u64
}

impl Notifier {
    /// Build from env. Never fails: a missing channel is a no-op notifier,
    /// a broken one reports its [`Outcome`] on every send (and in `status`).
    pub fn from_env(get: &dyn Fn(&str) -> Option<String>, transport: Box<dyn Transport>) -> Self {
        let val = |k: &str| {
            get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let per_hour = val("HERMES_RS_NOTIFY_PER_HOUR")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(DEFAULT_PER_HOUR)
            .min(MAX_PER_HOUR);
        let api_base = val("HERMES_RS_TELEGRAM_API_BASE").unwrap_or_else(|| TELEGRAM_API.into());
        let token = val("TELEGRAM_BOT_TOKEN");
        let chat = val("TELEGRAM_CHAT_ID").or_else(|| val("TELEGRAM_HOME_CHANNEL"));
        let (channel, config_error) = match (token, chat) {
            (Some(token), Some(chat)) => {
                if !token_ok(&token) {
                    (None, Some(Outcome::BadToken))
                } else if !chat_ok(&chat) {
                    (None, Some(Outcome::BadChat))
                } else if !api_base_ok(&api_base) {
                    (None, Some(Outcome::BadApiBase))
                } else {
                    (
                        Some(Channel {
                            token,
                            chat,
                            api_base: api_base.trim_end_matches('/').to_string(),
                        }),
                        None,
                    )
                }
            }
            _ => (None, None),
        };
        Notifier {
            channel,
            config_error,
            transport,
            per_hour,
            sent: Mutex::new(VecDeque::new()),
            clock: Arc::new(now_secs),
            log: Arc::new(|m: &str| {
                eprintln!(
                    "{} hermes-rs notify {m}",
                    crate::time::zagreb_rfc3339(crate::time::now_secs())
                )
            }),
            timeout_s: 20,
        }
    }

    /// A notifier with no channel (logged no-op). Used as the default.
    pub fn disabled() -> Self {
        Self::from_env(&|_| None, Box::new(CurlTransport::default()))
    }

    pub fn with_clock(mut self, f: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(f);
        self
    }

    pub fn with_log(mut self, f: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.log = Arc::new(f);
        self
    }

    pub fn configured(&self) -> bool {
        self.channel.is_some()
    }

    /// No secrets: channel kind, whether a chat is set, limits, recent count.
    pub fn status(&self) -> Value {
        let now = (self.clock)();
        let recent = {
            let mut q = self.sent.lock().unwrap_or_else(|e| e.into_inner());
            prune(&mut q, now);
            q.len()
        };
        let state = match (&self.channel, &self.config_error) {
            (Some(_), _) => "telegram",
            (None, Some(e)) => e.as_str(),
            (None, None) => Outcome::NoChannelConfigured.as_str(),
        };
        json!({
            "channel": state,
            "api_base_default": self.channel.as_ref().map(|c| c.api_base == TELEGRAM_API),
            "per_hour": self.per_hour,
            "sent_last_hour": recent,
        })
    }

    fn scrub(&self, s: &str) -> String {
        let mut s = crate::redact(s);
        if let Some(c) = &self.channel {
            s = s.replace(&c.token, "[REDACTED]");
            if let Some((_, secret)) = c.token.split_once(':') {
                s = s.replace(secret, "[REDACTED]");
            }
        }
        s.chars().take(300).collect()
    }

    /// Send one owner notification. `kind` is a short tag for the log
    /// (`proposal`, `budget-stop`, ...); only kind + length are ever logged.
    pub fn send(&self, kind: &str, text: &str) -> Outcome {
        let kind: String = kind
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || "-_.".contains(*c))
            .take(40)
            .collect();
        let kind = if kind.is_empty() { "note".into() } else { kind };
        let clean: String = crate::redact(text)
            .chars()
            .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
            .collect();
        let clean = clean.trim();
        let n = clean.chars().count();
        let o = self.send_inner(clean, n);
        let detail = match &o {
            Outcome::Failed(d) => format!(" detail={d}"),
            _ => String::new(),
        };
        let tail = if o == Outcome::NoChannelConfigured {
            " (no-op: set TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID to deliver)"
        } else {
            ""
        };
        (self.log)(&format!(
            "kind={kind} chars={n} outcome={}{detail}{tail}",
            o.as_str()
        ));
        o
    }

    fn send_inner(&self, clean: &str, n: usize) -> Outcome {
        if let Some(e) = &self.config_error {
            return e.clone();
        }
        let Some(ch) = &self.channel else {
            return Outcome::NoChannelConfigured;
        };
        if n == 0 {
            return Outcome::EmptyText;
        }
        let mut text: String = format!("[hermes] {clean}");
        if text.chars().count() > MAX_TEXT_CHARS {
            text = text.chars().take(MAX_TEXT_CHARS - 1).collect();
            text.push('…');
        }
        let now = (self.clock)();
        {
            let mut q = self.sent.lock().unwrap_or_else(|e| e.into_inner());
            prune(&mut q, now);
            if q.len() as u32 >= self.per_hour {
                return Outcome::RateLimited;
            }
            // Count attempts, not only successes: a failing API must not be hammered.
            q.push_back(now);
        }
        let body =
            json!({"chat_id": ch.chat, "text": text, "disable_web_page_preview": true}).to_string();
        let url = format!("{}/bot{}/sendMessage", ch.api_base, ch.token);
        match self.transport.post_json(&url, &body, self.timeout_s) {
            Ok((200, resp)) => {
                let v: Value = serde_json::from_str(&resp).unwrap_or(Value::Null);
                if v["ok"] == true {
                    Outcome::Sent
                } else {
                    Outcome::Failed(self.scrub("200 without ok=true"))
                }
            }
            Ok((code, resp)) => {
                let v: Value = serde_json::from_str(&resp).unwrap_or(Value::Null);
                let desc = v["description"].as_str().unwrap_or("");
                let retry = v["parameters"]["retry_after"]
                    .as_u64()
                    .map(|s| format!(" retry_after={s}s"))
                    .unwrap_or_default();
                Outcome::Failed(self.scrub(&format!("http {code} {desc}{retry}")))
            }
            Err(e) => Outcome::Failed(self.scrub(&e)),
        }
    }
}

fn prune(q: &mut VecDeque<u64>, now: u64) {
    while q.front().is_some_and(|t| t + 3600 <= now) {
        q.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    // Fake test token, not a real bot (shape only).
    const TOK: &str = "123456789:TESTfakeTokenNotReal_abc-XYZ";

    #[derive(Clone, Default)]
    struct Rec(Arc<Mutex<Vec<(String, String)>>>);

    struct MockT {
        rec: Rec,
        reply: Result<(u16, String), String>,
    }

    impl Transport for MockT {
        fn post_json(&self, url: &str, body: &str, _t: u64) -> Result<(u16, String), String> {
            self.rec.0.lock().unwrap().push((url.into(), body.into()));
            self.reply.clone()
        }
    }

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(a, _)| *a == k)
                .map(|(_, v)| v.to_string())
        }
    }

    fn mk(
        pairs: &[(&str, &str)],
        reply: Result<(u16, String), String>,
    ) -> (Notifier, Rec, Arc<Mutex<Vec<String>>>) {
        let rec = Rec::default();
        let logs = Arc::new(Mutex::new(vec![]));
        let l2 = logs.clone();
        let n = Notifier::from_env(
            &env(pairs),
            Box::new(MockT {
                rec: rec.clone(),
                reply,
            }),
        )
        .with_log(move |m| l2.lock().unwrap().push(m.to_string()));
        (n, rec, logs)
    }

    fn ok() -> Result<(u16, String), String> {
        Ok((200, r#"{"ok":true,"result":{"message_id":1}}"#.into()))
    }

    #[test]
    fn no_channel_is_logged_noop_without_text() {
        let (n, rec, logs) = mk(&[], ok());
        assert!(!n.configured());
        let o = n.send("proposal", "hoff-23 waits for Mate");
        assert_eq!(o, Outcome::NoChannelConfigured);
        assert_eq!(o.as_str(), "no-channel-configured");
        assert!(rec.0.lock().unwrap().is_empty(), "nothing sent");
        let l = logs.lock().unwrap();
        assert_eq!(l.len(), 1);
        assert!(l[0].contains("outcome=no-channel-configured") && l[0].contains("no-op"));
        assert!(l[0].contains("kind=proposal") && l[0].contains("chars=22"));
        assert!(!l[0].contains("hoff-23"), "text is never logged");
        assert_eq!(n.status()["channel"], "no-channel-configured");
        // only one of the two set: still no channel
        let (n, rec, _) = mk(&[("TELEGRAM_BOT_TOKEN", TOK)], ok());
        assert_eq!(n.send("x", "y"), Outcome::NoChannelConfigured);
        assert!(rec.0.lock().unwrap().is_empty());
        let (n, _, _) = mk(
            &[("TELEGRAM_CHAT_ID", "42"), ("TELEGRAM_BOT_TOKEN", "  ")],
            ok(),
        );
        assert_eq!(n.send("x", "y"), Outcome::NoChannelConfigured);
    }

    #[test]
    fn sends_bot_api_body_and_never_logs_token() {
        let (n, rec, logs) = mk(
            &[("TELEGRAM_BOT_TOKEN", TOK), ("TELEGRAM_CHAT_ID", "-100123")],
            ok(),
        );
        assert!(n.configured());
        assert_eq!(
            n.send("proposal", "hoff-23 \"quoted\"\nline 2"),
            Outcome::Sent
        );
        let r = rec.0.lock().unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0, format!("{TELEGRAM_API}/bot{TOK}/sendMessage"));
        let b: Value = serde_json::from_str(&r[0].1).unwrap();
        assert_eq!(b["chat_id"], "-100123");
        assert_eq!(b["text"], "[hermes] hoff-23 \"quoted\"\nline 2");
        assert_eq!(b["disable_web_page_preview"], true);
        let all = logs.lock().unwrap().join("\n") + &n.status().to_string();
        assert!(!all.contains(TOK) && !all.contains("TESTfake"));
        assert_eq!(n.status()["channel"], "telegram");
        assert_eq!(n.status()["sent_last_hour"], 1);
    }

    #[test]
    fn home_channel_fallback_and_bad_inputs() {
        let (n, _, _) = mk(
            &[
                ("TELEGRAM_BOT_TOKEN", TOK),
                ("TELEGRAM_HOME_CHANNEL", "@galion_owner"),
            ],
            ok(),
        );
        assert!(n.configured());
        for (tok, chat, want) in [
            ("no-colon-token", "42", Outcome::BadToken),
            ("123:short", "42", Outcome::BadToken),
            ("12a:AAAAAAAAAAAA", "42", Outcome::BadToken),
            ("123456:AAAA/../AAAA", "42", Outcome::BadToken),
            (TOK, "42; rm", Outcome::BadChat),
            (TOK, "@ab", Outcome::BadChat),
        ] {
            let (n, rec, logs) = mk(
                &[("TELEGRAM_BOT_TOKEN", tok), ("TELEGRAM_CHAT_ID", chat)],
                ok(),
            );
            assert_eq!(n.send("k", "t"), want, "{tok} {chat}");
            assert_eq!(n.status()["channel"], want.as_str());
            assert!(rec.0.lock().unwrap().is_empty());
            assert!(!logs.lock().unwrap().join("").contains(tok));
        }
    }

    #[test]
    fn api_base_only_loopback_or_telegram() {
        for (b, ok_) in [
            ("https://api.telegram.org", true),
            ("https://api.telegram.org/", true),
            ("http://127.0.0.1:21360", true),
            ("http://localhost:9", true),
            ("http://[::1]:21360", true),
            ("http://api.telegram.org", false),
            ("https://evil.example", false),
            ("http://127.0.0.1.evil.example:80", false),
            ("https://api.telegram.org.evil.example", false),
            ("http://10.0.0.1:80", false),
            ("ftp://127.0.0.1", false),
        ] {
            assert_eq!(api_base_ok(b), ok_, "{b}");
        }
        let (n, rec, _) = mk(
            &[
                ("TELEGRAM_BOT_TOKEN", TOK),
                ("TELEGRAM_CHAT_ID", "42"),
                ("HERMES_RS_TELEGRAM_API_BASE", "https://evil.example"),
            ],
            ok(),
        );
        assert_eq!(n.send("k", "t"), Outcome::BadApiBase);
        assert!(rec.0.lock().unwrap().is_empty());
        let (n, rec, _) = mk(
            &[
                ("TELEGRAM_BOT_TOKEN", TOK),
                ("TELEGRAM_CHAT_ID", "42"),
                ("HERMES_RS_TELEGRAM_API_BASE", "http://127.0.0.1:21360/"),
            ],
            ok(),
        );
        assert_eq!(n.send("k", "t"), Outcome::Sent);
        assert_eq!(
            rec.0.lock().unwrap()[0].0,
            format!("http://127.0.0.1:21360/bot{TOK}/sendMessage")
        );
    }

    #[test]
    fn rate_limit_rolls_per_hour_and_counts_failures() {
        let t = Arc::new(AtomicU64::new(1_000_000));
        let t2 = t.clone();
        let (n, rec, _) = mk(
            &[
                ("TELEGRAM_BOT_TOKEN", TOK),
                ("TELEGRAM_CHAT_ID", "42"),
                ("HERMES_RS_NOTIFY_PER_HOUR", "2"),
            ],
            Err("curl exit Some(7): connection refused".into()),
        );
        let n = n.with_clock(move || t2.load(Ordering::SeqCst));
        assert!(matches!(n.send("a", "1"), Outcome::Failed(_)));
        assert!(matches!(n.send("a", "2"), Outcome::Failed(_)));
        assert_eq!(n.send("a", "3"), Outcome::RateLimited);
        assert_eq!(rec.0.lock().unwrap().len(), 2);
        t.fetch_add(3600, Ordering::SeqCst);
        assert!(matches!(n.send("a", "4"), Outcome::Failed(_)));
        assert_eq!(rec.0.lock().unwrap().len(), 3);
        // per-hour hard cap
        let (n, _, _) = mk(&[("HERMES_RS_NOTIFY_PER_HOUR", "9999")], ok());
        assert_eq!(n.status()["per_hour"], MAX_PER_HOUR);
    }

    #[test]
    fn failures_are_scrubbed_of_the_token() {
        let (n, _, logs) = mk(
            &[("TELEGRAM_BOT_TOKEN", TOK), ("TELEGRAM_CHAT_ID", "42")],
            Err(format!(
                "curl: (6) bad url https://api.telegram.org/bot{TOK}/sendMessage"
            )),
        );
        let o = n.send("k", "t");
        let Outcome::Failed(d) = &o else {
            panic!("{o:?}")
        };
        assert!(!d.contains(TOK) && !d.contains("TESTfake"), "{d}");
        assert!(!o.to_json().to_string().contains("TESTfake"));
        assert!(!logs.lock().unwrap().join("").contains("TESTfake"));
        // 429 from Telegram: retry_after surfaced, nothing secret
        let (n, _, _) = mk(
            &[("TELEGRAM_BOT_TOKEN", TOK), ("TELEGRAM_CHAT_ID", "42")],
            Ok((429, r#"{"ok":false,"error_code":429,"description":"Too Many Requests: retry after 7","parameters":{"retry_after":7}}"#.into())),
        );
        let Outcome::Failed(d) = n.send("k", "t") else {
            panic!()
        };
        assert!(
            d.contains("http 429") && d.contains("retry_after=7s"),
            "{d}"
        );
        // 200 but ok=false
        let (n, _, _) = mk(
            &[("TELEGRAM_BOT_TOKEN", TOK), ("TELEGRAM_CHAT_ID", "42")],
            Ok((200, r#"{"ok":false}"#.into())),
        );
        assert!(matches!(n.send("k", "t"), Outcome::Failed(_)));
    }

    #[test]
    fn text_is_redacted_capped_and_empty_refused() {
        let (n, rec, _) = mk(
            &[("TELEGRAM_BOT_TOKEN", TOK), ("TELEGRAM_CHAT_ID", "42")],
            ok(),
        );
        assert_eq!(n.send("k", " \u{7} \n "), Outcome::EmptyText);
        assert!(rec.0.lock().unwrap().is_empty());
        let leak = concat!("key OPENROUTER_API_KEY=", "supersecretvalue1 end");
        assert_eq!(n.send("k", leak), Outcome::Sent);
        let long = "x".repeat(10_000);
        assert_eq!(n.send("k", &long), Outcome::Sent);
        let r = rec.0.lock().unwrap();
        let b0: Value = serde_json::from_str(&r[0].1).unwrap();
        assert!(!b0["text"].as_str().unwrap().contains("supersecretvalue1"));
        let b1: Value = serde_json::from_str(&r[1].1).unwrap();
        let t = b1["text"].as_str().unwrap();
        assert_eq!(t.chars().count(), MAX_TEXT_CHARS);
        assert!(t.ends_with('…'));
    }

    #[test]
    fn curl_argv_holds_no_secret_and_config_is_quoted() {
        let argv = CurlTransport::argv(20).join(" ");
        assert!(!argv.contains("bot") && !argv.contains("http"), "{argv}");
        assert!(argv.ends_with("-K -"));
        let url = format!("{TELEGRAM_API}/bot{TOK}/sendMessage");
        let c = CurlTransport::config(&url, r#"{"text":"a\"b"}"#);
        assert!(c.contains(&format!("url = \"{url}\"")));
        assert!(
            c.contains(r#"data-binary = "{\"text\":\"a\\\"b\"}""#),
            "{c}"
        );
        assert!(c.contains("write-out = \"\\n%{http_code}\""));
    }

    #[test]
    fn missing_curl_is_a_clean_failure() {
        let n = Notifier::from_env(
            &env(&[("TELEGRAM_BOT_TOKEN", TOK), ("TELEGRAM_CHAT_ID", "42")]),
            Box::new(CurlTransport {
                curl: "/nonexistent/curl".into(),
            }),
        )
        .with_log(|_| {});
        let Outcome::Failed(d) = n.send("k", "t") else {
            panic!()
        };
        assert!(d.starts_with("curl:") && !d.contains("TESTfake"), "{d}");
    }
}
