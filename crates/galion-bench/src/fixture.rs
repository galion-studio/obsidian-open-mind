// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Scripted request -> response fixtures, one JSON object per line (JSONL).
//! The full format is documented in `docs/bench/harness.md`; the short form:
//!
//! ```text
//! {"id":"accounts","method":"GET","path":"/api/1.0/accounts","status":200,"body":[{"id":"acc-1"}]}
//! ```
//!
//! Matching is first-eligible-wins in file order. Loading is strict: unknown
//! keys, duplicate ids, bad paths and `Z`-suffixed timestamps are errors.

use crate::http::Request;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

/// Longest `delay_ms` a fixture may ask for.
pub const MAX_DELAY_MS: u64 = 10_000;
/// Path prefix reserved for the mock's own admin endpoints.
pub const ADMIN_PREFIX: &str = "/__bench/";

const KEYS: &[&str] = &[
    "id",
    "method",
    "path",
    "query",
    "headers",
    "body_contains",
    "body_json",
    "times",
    "after",
    "status",
    "response_headers",
    "body",
    "delay_ms",
    "note",
];

/// One scripted exchange.
#[derive(Debug, Clone, PartialEq)]
pub struct Fixture {
    /// Unique id; shows up in the journal.
    pub id: String,
    /// Upper-case method, or `*` for any.
    pub method: String,
    /// Exact path, or a prefix when it ends in `*` (`*` alone matches all).
    pub path: String,
    /// Required query parameters (subset match, exact values).
    pub query: Vec<(String, String)>,
    /// Required request headers (lower-cased names, exact values).
    pub headers: Vec<(String, String)>,
    /// Substrings the raw request body must all contain.
    pub body_contains: Vec<String>,
    /// JSON the request body must contain (recursive object subset match).
    pub body_json: Option<Value>,
    /// Answer at most this many times, then fall through. `None` = unlimited.
    pub times: Option<u32>,
    /// Only eligible after the fixture with this id has been hit at least once.
    pub after: Option<String>,
    /// Response status.
    pub status: u16,
    /// Response headers.
    pub response_headers: Vec<(String, String)>,
    /// Response body: a JSON string is sent as text, anything else as JSON.
    pub body: Value,
    /// Artificial latency before answering (capped at [`MAX_DELAY_MS`]).
    pub delay_ms: u64,
}

/// Why a fixture file was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureError {
    /// 1-based line number (0 for file-level errors).
    pub line: usize,
    /// What is wrong.
    pub msg: String,
}

impl fmt::Display for FixtureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fixture line {}: {}", self.line, self.msg)
    }
}

impl std::error::Error for FixtureError {}

fn err(line: usize, msg: impl Into<String>) -> FixtureError {
    FixtureError {
        line,
        msg: msg.into(),
    }
}

fn str_map(
    v: Option<&Value>,
    line: usize,
    key: &str,
    lower: bool,
) -> Result<Vec<(String, String)>, FixtureError> {
    let Some(v) = v else { return Ok(Vec::new()) };
    let obj = v
        .as_object()
        .ok_or_else(|| err(line, format!("`{key}` must be an object of strings")))?;
    obj.iter()
        .map(|(k, v)| {
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => return Err(err(line, format!("`{key}.{k}` must be a string"))),
            };
            Ok((
                if lower {
                    k.to_ascii_lowercase()
                } else {
                    k.clone()
                },
                s,
            ))
        })
        .collect()
}

/// True if `s` looks like an RFC 3339 timestamp written in UTC `Z` form.
pub fn is_zulu_stamp(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 20
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && (b[10] == b'T' || b[10] == b't')
        && (s.ends_with('Z') || s.ends_with('z'))
}

fn find_zulu(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if is_zulu_stamp(s) => Some(s.clone()),
        Value::Array(a) => a.iter().find_map(find_zulu),
        Value::Object(o) => o.values().find_map(find_zulu),
        _ => None,
    }
}

impl Fixture {
    /// Parse one fixture object (`line` is used for error messages).
    pub fn from_value(v: &Value, line: usize) -> Result<Self, FixtureError> {
        let o: &Map<String, Value> = v
            .as_object()
            .ok_or_else(|| err(line, "each line must be a JSON object"))?;
        if let Some(k) = o.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(err(line, format!("unknown key `{k}`")));
        }
        let id = o
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| err(line, "`id` (non-empty string) is required"))?
            .to_string();
        let method = o
            .get("method")
            .map(|m| m.as_str().map(str::to_ascii_uppercase))
            .unwrap_or(Some("GET".into()))
            .ok_or_else(|| err(line, "`method` must be a string"))?;
        let path = o
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| err(line, "`path` (string) is required"))?
            .to_string();
        if !(path.starts_with('/') || path == "*") {
            return Err(err(line, "`path` must start with `/` (or be `*`)"));
        }
        if path.starts_with(ADMIN_PREFIX) {
            return Err(err(
                line,
                format!("`path` under {ADMIN_PREFIX} is reserved"),
            ));
        }
        let body_contains = match o.get("body_contains") {
            None => Vec::new(),
            Some(Value::String(s)) => vec![s.clone()],
            Some(Value::Array(a)) => a
                .iter()
                .map(|x| {
                    x.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| err(line, "`body_contains` entries must be strings"))
                })
                .collect::<Result<_, _>>()?,
            Some(_) => return Err(err(line, "`body_contains` must be a string or array")),
        };
        let times = match o.get("times") {
            None => None,
            Some(t) => Some(
                t.as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| *n > 0)
                    .ok_or_else(|| err(line, "`times` must be a positive integer"))?,
            ),
        };
        let after = match o.get("after") {
            None => None,
            Some(a) => Some(
                a.as_str()
                    .ok_or_else(|| err(line, "`after` must be a fixture id"))?
                    .to_string(),
            ),
        };
        let status = match o.get("status") {
            None => 200,
            Some(s) => {
                s.as_u64()
                    .filter(|n| (100..=599).contains(n))
                    .ok_or_else(|| err(line, "`status` must be 100-599"))? as u16
            }
        };
        let delay_ms = match o.get("delay_ms") {
            None => 0,
            Some(d) => d
                .as_u64()
                .filter(|n| *n <= MAX_DELAY_MS)
                .ok_or_else(|| err(line, format!("`delay_ms` must be 0-{MAX_DELAY_MS}")))?,
        };
        let body = o.get("body").cloned().unwrap_or(Value::Null);
        if let Some(z) = find_zulu(&body) {
            return Err(err(
                line,
                format!("timestamp `{z}` ends in Z; write Europe/Zagreb with +01:00/+02:00"),
            ));
        }
        Ok(Self {
            id,
            method,
            path,
            query: str_map(o.get("query"), line, "query", false)?,
            headers: str_map(o.get("headers"), line, "headers", true)?,
            body_contains,
            body_json: o.get("body_json").cloned(),
            times,
            after,
            status,
            response_headers: str_map(o.get("response_headers"), line, "response_headers", false)?,
            body,
            delay_ms,
        })
    }

    /// Does this fixture's request side match `req`? (Ignores `times`/`after`.)
    pub fn matches(&self, req: &Request) -> bool {
        if self.method != "*" && self.method != req.method {
            return false;
        }
        let path_ok = match self.path.strip_suffix('*') {
            Some(prefix) => req.path.starts_with(prefix),
            None => self.path == req.path,
        };
        if !path_ok {
            return false;
        }
        let params = parse_query(&req.query);
        if !self
            .query
            .iter()
            .all(|(k, v)| params.iter().any(|(pk, pv)| pk == k && pv == v))
        {
            return false;
        }
        if !self
            .headers
            .iter()
            .all(|(k, v)| req.header(k) == Some(v.as_str()))
        {
            return false;
        }
        if !self
            .body_contains
            .iter()
            .all(|s| req.body.contains(s.as_str()))
        {
            return false;
        }
        if let Some(want) = &self.body_json {
            match serde_json::from_str::<Value>(&req.body) {
                Ok(got) if json_subset(want, &got) => {}
                _ => return false,
            }
        }
        true
    }

    /// Render the response body for the `hit`-th answer (1-based).
    /// Templates in strings: `{{now}}`, `{{hit}}`, `{{fixture_id}}`.
    pub fn render_body(&self, hit: u32, now: &str) -> (String, &'static str) {
        let fill = |s: &str| {
            s.replace("{{now}}", now)
                .replace("{{hit}}", &hit.to_string())
                .replace("{{fixture_id}}", &self.id)
        };
        match &self.body {
            Value::Null => (String::new(), "text/plain; charset=utf-8"),
            Value::String(s) => (fill(s), "text/plain; charset=utf-8"),
            other => (fill_json(other, &fill).to_string(), "application/json"),
        }
    }
}

fn fill_json(v: &Value, fill: &dyn Fn(&str) -> String) -> Value {
    match v {
        Value::String(s) => Value::String(fill(s)),
        Value::Array(a) => Value::Array(a.iter().map(|x| fill_json(x, fill)).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| (k.clone(), fill_json(x, fill)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// `want` is contained in `got`: objects recursively by key, everything else equal.
pub fn json_subset(want: &Value, got: &Value) -> bool {
    match (want, got) {
        (Value::Object(w), Value::Object(g)) => w
            .iter()
            .all(|(k, wv)| g.get(k).is_some_and(|gv| json_subset(wv, gv))),
        _ => want == got,
    }
}

/// Split `a=1&b=x%20y` into pairs, percent-decoding `%XX` and `+`.
pub fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (pct_decode(k), pct_decode(v))
        })
        .collect()
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A loaded, validated fixture list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FixtureSet {
    /// Fixtures in match order.
    pub fixtures: Vec<Fixture>,
}

impl FixtureSet {
    /// Append another set (ids must stay unique).
    pub fn extend(&mut self, other: FixtureSet) -> Result<(), FixtureError> {
        let mut all = std::mem::take(&mut self.fixtures);
        all.extend(other.fixtures);
        *self = validate(all)?;
        Ok(())
    }

    /// Fixture by id.
    pub fn get(&self, id: &str) -> Option<&Fixture> {
        self.fixtures.iter().find(|f| f.id == id)
    }
}

fn validate(fixtures: Vec<Fixture>) -> Result<FixtureSet, FixtureError> {
    let mut seen: HashMap<&str, ()> = HashMap::new();
    for f in &fixtures {
        if seen.insert(f.id.as_str(), ()).is_some() {
            return Err(err(0, format!("duplicate fixture id `{}`", f.id)));
        }
    }
    for f in &fixtures {
        if let Some(a) = &f.after {
            if !seen.contains_key(a.as_str()) {
                return Err(err(
                    0,
                    format!("fixture `{}`: `after` names unknown id `{a}`", f.id),
                ));
            }
        }
    }
    Ok(FixtureSet { fixtures })
}

/// Parse JSONL text. Blank lines and lines starting with `#` or `//` are skipped.
pub fn parse_jsonl(text: &str) -> Result<FixtureSet, FixtureError> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let t = raw.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        let v: Value =
            serde_json::from_str(t).map_err(|e| err(line, format!("invalid JSON: {e}")))?;
        out.push(Fixture::from_value(&v, line)?);
    }
    validate(out)
}

/// Read and parse a JSONL fixture file.
pub fn load_jsonl(path: impl AsRef<Path>) -> Result<FixtureSet, FixtureError> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path)
        .map_err(|e| err(0, format!("cannot read {}: {e}", path.display())))?;
    parse_jsonl(&text).map_err(|e| err(e.line, format!("{}: {}", path.display(), e.msg)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(method: &str, target: &str, body: &str) -> Request {
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        Request {
            method: method.into(),
            path: path.into(),
            query: query.into(),
            headers: vec![("authorization".into(), "Bearer mock".into())],
            body: body.into(),
        }
    }

    #[test]
    fn parses_and_matches() {
        let set = parse_jsonl(
            r#"
# comment
{"id":"a","path":"/x","query":{"q":"a b"},"headers":{"Authorization":"Bearer mock"},"body":{"t":"{{now}}"}}
{"id":"b","method":"post","path":"/pay*","body_json":{"amount":{"currency":"EUR"}},"body_contains":"draft","status":202}
"#,
        )
        .unwrap();
        assert_eq!(set.fixtures.len(), 2);
        let a = set.get("a").unwrap();
        assert!(a.matches(&req("GET", "/x?q=a+b", "")));
        assert!(a.matches(&req("GET", "/x?z=1&q=a%20b", "")));
        assert!(!a.matches(&req("GET", "/x?q=c", "")));
        assert!(!a.matches(&req("POST", "/x?q=a+b", "")));
        let b = set.get("b").unwrap();
        assert_eq!(b.method, "POST");
        assert!(b.matches(&req(
            "POST",
            "/pay/1",
            r#"{"mode":"draft","amount":{"value":5,"currency":"EUR"}}"#
        )));
        assert!(!b.matches(&req(
            "POST",
            "/pay/1",
            r#"{"mode":"draft","amount":{"currency":"USD"}}"#
        )));
        assert!(!b.matches(&req("POST", "/pay/1", "not json draft")));
        let (body, ct) = a.render_body(3, "2026-10-20T23:00:00+02:00");
        assert_eq!(ct, "application/json");
        assert_eq!(body, r#"{"t":"2026-10-20T23:00:00+02:00"}"#);
    }

    #[test]
    fn strict_loading() {
        let cases = [
            (r#"{"id":"a","path":"/x","bogus":1}"#, "unknown key"),
            (r#"{"path":"/x"}"#, "`id`"),
            (r#"{"id":"a","path":"x"}"#, "start with"),
            (r#"{"id":"a","path":"/__bench/journal"}"#, "reserved"),
            (r#"{"id":"a","path":"/x","status":42}"#, "status"),
            (r#"{"id":"a","path":"/x","times":0}"#, "times"),
            (r#"{"id":"a","path":"/x","delay_ms":999999}"#, "delay_ms"),
            (
                r#"{"id":"a","path":"/x","body":{"at":"2026-10-20T21:00:00Z"}}"#,
                "ends in Z",
            ),
            (
                "{\"id\":\"a\",\"path\":\"/x\"}\n{\"id\":\"a\",\"path\":\"/y\"}",
                "duplicate",
            ),
            (r#"{"id":"a","path":"/x","after":"nope"}"#, "unknown id"),
            ("{not json", "invalid JSON"),
        ];
        for (text, want) in cases {
            let e = parse_jsonl(text).unwrap_err();
            assert!(e.msg.contains(want), "{text}: {e}");
        }
        assert_eq!(
            parse_jsonl("\n\n// only comments\n")
                .unwrap()
                .fixtures
                .len(),
            0
        );
    }

    #[test]
    fn query_decoding() {
        assert_eq!(
            parse_query("a=1&b=x%2Fy&c&d=%zz&e=%4"),
            vec![
                ("a".into(), "1".into()),
                ("b".into(), "x/y".into()),
                ("c".into(), "".into()),
                ("d".into(), "%zz".into()),
                ("e".into(), "%4".into()),
            ]
        );
    }
}
