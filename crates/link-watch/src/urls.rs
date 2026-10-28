// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Public URL watches (`LINK_WATCH_URLS`).
//!
//! A short list of public pages (for example `https://one.galion.app/`) is
//! fetched with one GET each, at most once per `LINK_WATCH_URLS_EVERY`
//! seconds per URL (default 300, never below 60). The result goes into
//! `link.json` under a trailing `urls` key that is present only when the
//! list is configured, so the historical key set is unchanged otherwise.
//!
//! `http://` uses the in-crate client. `https://` runs the system `curl`
//! (the same TLS route hermes-rs uses; no TLS crate in the tree) with
//! `--proto =https`, no redirects followed, a hard time limit and the body
//! discarded. URLs carrying credentials (`user@host`) are refused.
//!
//! The last check time of each URL is read back from the previous
//! `link.json`, so restarts and `LINK_WATCH_ONCE` runs keep the rate limit.

use serde_json::{Map, Value};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Most URLs one watcher will fetch.
pub const MAX_URLS: usize = 8;
/// Default seconds between two fetches of one URL.
pub const DEFAULT_EVERY: u64 = 300;
/// Lowest accepted interval. Smaller values are raised to this.
pub const MIN_EVERY: u64 = 60;
/// Highest accepted interval.
pub const MAX_EVERY: u64 = 86_400;
/// Per-fetch time limit.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Parse `LINK_WATCH_URLS`: comma or whitespace separated, `http`/`https`
/// only, no credentials, no duplicates, at most [`MAX_URLS`].
pub fn parse_urls(raw: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for item in raw.split(|c: char| c == ',' || c.is_whitespace()) {
        let url = item.trim();
        if url.is_empty() {
            continue;
        }
        let rest = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .ok_or_else(|| format!("not an http(s) URL: {url}"))?;
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        if authority.is_empty() {
            return Err(format!("URL has no host: {url}"));
        }
        if authority.contains('@') {
            return Err("URL with credentials refused".to_string());
        }
        if url.chars().any(|c| c.is_control() || c == '"' || c == '\\') {
            return Err("URL has control or quote characters".to_string());
        }
        if !out.iter().any(|u| u == url) {
            out.push(url.to_string());
        }
    }
    if out.len() > MAX_URLS {
        return Err(format!("{} URLs given, at most {MAX_URLS}", out.len()));
    }
    Ok(out)
}

/// `LINK_WATCH_URLS_EVERY` clamped to [`MIN_EVERY`]..=[`MAX_EVERY`].
pub fn every_secs(raw: Option<&str>) -> u64 {
    raw.and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_EVERY)
        .clamp(MIN_EVERY, MAX_EVERY)
}

/// `true` when a URL last checked at `last` (unix seconds) is due at `now`.
pub fn due(last: Option<i64>, now: i64, every: u64) -> bool {
    match last {
        None => true,
        // A clock that went backwards makes the URL due rather than stuck.
        Some(last) => now < last || now - last >= every as i64,
    }
}

/// One URL check.
#[derive(Clone, Debug, PartialEq)]
pub struct UrlResult {
    pub url: String,
    pub ok: bool,
    /// HTTP status, 0 when no response arrived.
    pub status: u16,
    pub ms: i64,
    /// Unix seconds of the check (rate-limit memory).
    pub checked_unix: i64,
    /// Europe/Zagreb RFC 3339 stamp of the check.
    pub checked_local: String,
    /// Short reason when no status arrived.
    pub error: String,
}

impl UrlResult {
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("url".into(), Value::String(self.url.clone()));
        m.insert("ok".into(), Value::Bool(self.ok));
        m.insert("status".into(), Value::Number(self.status.into()));
        m.insert("ms".into(), Value::Number(self.ms.into()));
        m.insert(
            "checked_at_local".into(),
            Value::String(self.checked_local.clone()),
        );
        m.insert(
            "checked_unix".into(),
            Value::Number(self.checked_unix.into()),
        );
        m.insert("error".into(), Value::String(self.error.clone()));
        Value::Object(m)
    }

    pub fn from_value(v: &Value) -> Option<UrlResult> {
        Some(UrlResult {
            url: v.get("url")?.as_str()?.to_string(),
            ok: v.get("ok")?.as_bool()?,
            status: u16::try_from(v.get("status")?.as_u64()?).ok()?,
            ms: v.get("ms")?.as_i64()?,
            checked_unix: v.get("checked_unix")?.as_i64()?,
            checked_local: v.get("checked_at_local")?.as_str()?.to_string(),
            error: v
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        })
    }
}

/// Previous results for `urls` from an earlier `link.json` value.
pub fn previous_results(report: &Value, urls: &[String]) -> Vec<Option<UrlResult>> {
    let old: Vec<UrlResult> = report
        .get("urls")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(UrlResult::from_value).collect())
        .unwrap_or_default();
    urls.iter()
        .map(|u| old.iter().find(|r| &r.url == u).cloned())
        .collect()
}

/// The argv for an HTTPS GET through `curl`. Body discarded, status printed.
pub fn curl_args(url: &str, timeout: Duration) -> Vec<String> {
    let secs = timeout.as_secs().max(1).to_string();
    [
        "-sS",
        "-o",
        "/dev/null",
        "-w",
        "%{http_code}",
        "--proto",
        "=https",
        "--max-redirs",
        "0",
        "--max-time",
        &secs,
        "--connect-timeout",
        &secs,
        "--max-filesize",
        "5000000",
        "-A",
        "link-watch/0.1 (+https://one.galion.app)",
        "--",
        url,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Status from curl's `-w %{http_code}` output (`000` means none).
pub fn parse_curl_status(stdout: &str) -> u16 {
    stdout.trim().parse().unwrap_or(0)
}

fn short(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    line.chars().take(120).collect()
}

/// GET one URL now.
pub fn fetch(url: &str, now_unix: i64, now_local: String) -> UrlResult {
    let started = Instant::now();
    let (status, error) = if url.starts_with("https://") {
        match Command::new("curl")
            .args(curl_args(url, FETCH_TIMEOUT))
            .stdin(Stdio::null())
            .output()
        {
            Ok(out) => {
                let status = parse_curl_status(&String::from_utf8_lossy(&out.stdout));
                let err = if status == 0 {
                    short(&String::from_utf8_lossy(&out.stderr))
                } else {
                    String::new()
                };
                (status, err)
            }
            Err(e) => (0, format!("curl: {e}")),
        }
    } else {
        match crate::http::http_get(url, FETCH_TIMEOUT) {
            Some((status, _)) => (status, String::new()),
            None => (0, "no response".to_string()),
        }
    };
    UrlResult {
        url: url.to_string(),
        ok: (200..400).contains(&status),
        status,
        ms: started.elapsed().as_millis() as i64,
        checked_unix: now_unix,
        checked_local: now_local,
        error,
    }
}

/// Rate-limited watcher over a fixed URL list.
pub struct UrlWatch {
    pub urls: Vec<String>,
    pub every: u64,
    pub last: Vec<Option<UrlResult>>,
}

impl UrlWatch {
    pub fn new(urls: Vec<String>, every: u64, previous: &Value) -> UrlWatch {
        let last = previous_results(previous, &urls);
        UrlWatch { urls, every, last }
    }

    /// Fetch every due URL with `fetcher`, pausing `gap` between fetches.
    /// Returns how many fetches ran.
    pub fn tick(
        &mut self,
        now_unix: i64,
        gap: Duration,
        fetcher: &mut dyn FnMut(&str) -> UrlResult,
    ) -> usize {
        let mut ran = 0;
        for (i, url) in self.urls.iter().enumerate() {
            let last = self.last[i].as_ref().map(|r| r.checked_unix);
            if !due(last, now_unix, self.every) {
                continue;
            }
            if ran > 0 && !gap.is_zero() {
                std::thread::sleep(gap);
            }
            self.last[i] = Some(fetcher(url));
            ran += 1;
        }
        ran
    }

    /// The `urls` array for link.json (unchecked URLs are left out).
    pub fn report(&self) -> Value {
        Value::Array(
            self.last
                .iter()
                .flatten()
                .map(UrlResult::to_value)
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_accepts_http_and_https_and_dedupes() {
        assert_eq!(
            parse_urls(" https://example.com/, https://one.galion.app/\nhttp://127.0.0.1:1/x https://example.com/").unwrap(),
            vec!["https://example.com/", "https://one.galion.app/", "http://127.0.0.1:1/x"]
        );
        assert_eq!(parse_urls("").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn parse_refuses_bad_urls() {
        assert!(parse_urls("ftp://example.com").is_err());
        assert!(parse_urls("example.com").is_err());
        assert!(parse_urls("https://").is_err());
        assert!(parse_urls("https://user:pw@example.com/").is_err());
        assert!(parse_urls("https://exa\"mple.com/").is_err());
        let nine: Vec<String> = (0..9).map(|i| format!("https://h{i}.example/")).collect();
        assert!(parse_urls(&nine.join(",")).is_err());
        // '@' after the host (path or query) is not a credential
        assert!(parse_urls("https://example.com/@me").is_ok());
    }

    #[test]
    fn interval_is_clamped() {
        assert_eq!(every_secs(None), 300);
        assert_eq!(every_secs(Some("5")), 60);
        assert_eq!(every_secs(Some("600")), 600);
        assert_eq!(every_secs(Some("999999")), 86_400);
        assert_eq!(every_secs(Some("x")), 300);
    }

    #[test]
    fn due_rules() {
        assert!(due(None, 1000, 60));
        assert!(!due(Some(1000), 1059, 60));
        assert!(due(Some(1000), 1060, 60));
        assert!(due(Some(2000), 1000, 60), "clock went backwards");
    }

    #[test]
    fn curl_argv_is_get_only_https_only_and_bounded() {
        let args = curl_args("https://example.com/", Duration::from_secs(10));
        let joined = args.join(" ");
        for need in [
            "--proto =https",
            "--max-redirs 0",
            "--max-time 10",
            "-o /dev/null",
            "-- https://example.com/",
        ] {
            assert!(joined.contains(need), "{need} in {joined}");
        }
        for banned in [
            "-X",
            "-d",
            "--data",
            "-I",
            "--head",
            "-L",
            "-k",
            "--insecure",
            "-K",
        ] {
            assert!(!args.iter().any(|a| a == banned), "{banned}");
        }
        assert_eq!(parse_curl_status("200"), 200);
        assert_eq!(parse_curl_status("000"), 0);
        assert_eq!(parse_curl_status(""), 0);
    }

    fn fake(url: &str, at: i64) -> UrlResult {
        UrlResult {
            url: url.into(),
            ok: true,
            status: 200,
            ms: 5,
            checked_unix: at,
            checked_local: "2026-10-20T19:00:00+02:00".into(),
            error: String::new(),
        }
    }

    #[test]
    fn watch_respects_the_interval_and_survives_a_restart() {
        let urls = vec![
            "https://a.example/".to_string(),
            "https://b.example/".to_string(),
        ];
        let mut w = UrlWatch::new(urls.clone(), 60, &json!({}));
        let mut calls = Vec::new();
        let mut f = |u: &str| {
            calls.push(u.to_string());
            fake(u, 1000)
        };
        assert_eq!(w.tick(1000, Duration::ZERO, &mut f), 2);
        assert_eq!(w.tick(1030, Duration::ZERO, &mut f), 0);
        assert_eq!(calls.len(), 2);

        // A new process reading the previous report keeps the limit.
        let report = json!({"events": [], "urls": w.report()});
        let mut again = UrlWatch::new(urls, 60, &report);
        let mut n = 0;
        let mut g = |u: &str| {
            n += 1;
            fake(u, 1061)
        };
        assert_eq!(again.tick(1030, Duration::ZERO, &mut g), 0);
        assert_eq!(again.tick(1061, Duration::ZERO, &mut g), 2);
        assert_eq!(n, 2);
    }

    #[test]
    fn report_round_trips_and_avoids_desk_keys() {
        let r = fake("https://a.example/", 5);
        assert_eq!(UrlResult::from_value(&r.to_value()), Some(r.clone()));
        let text = serde_json::to_string(&r.to_value()).unwrap();
        // galion-desk scans link.json for the first "name", "model", "events",
        // "france", "hermes_ms", "model_ms", "gpu", "cpu": none may appear here.
        for key in [
            "\"name\"",
            "\"model\"",
            "\"events\"",
            "\"france\"",
            "_ms\"",
            "\"gpu\"",
            "\"cpu\"",
        ] {
            assert!(!text.contains(key), "{key} in {text}");
        }
    }

    #[test]
    fn plain_http_fetch_against_a_closed_port() {
        let r = fetch("http://127.0.0.1:1/", 1, "x".into());
        assert!(!r.ok);
        assert_eq!(r.status, 0);
        assert_eq!(r.error, "no response");
    }
}
