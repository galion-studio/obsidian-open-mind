// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// OpenRouter model list refresh. Replaces the old one/models.py launcher; the
// output file format is the same: a JSON array of {"id","name"} objects sorted
// by lowercased name, written the way Python's json.dumps writes it
// (", " and ": " separators, non-ASCII escaped as \uXXXX).
//
// The HTTP fetch is injected so tests never touch the network. The API key is
// read from the process env var OPENROUTER_API_KEY at call time only and is
// handed to curl through stdin, never argv, and never logged.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub const DEFAULT_BASE: &str = "https://openrouter.ai/api/v1";

use crate::json::{dump_str, get, parse, text_of, Json};

/// Turn the API response body into the cache file text. Returns (text, count).
pub fn transform(body: &str) -> Result<(String, usize), String> {
    let root = parse(body)?;
    let mut rows: Vec<(String, String)> = Vec::new();
    if let Some(Json::Arr(items)) = get(&root, "data") {
        for item in items {
            let id = text_of(get(item, "id"));
            if id.is_empty() {
                continue;
            }
            let mut name = text_of(get(item, "name"));
            if name.is_empty() {
                name = id.clone();
            }
            rows.push((id, name));
        }
    }
    rows.sort_by_key(|r| r.1.to_lowercase());
    let parts: Vec<String> = rows
        .iter()
        .map(|(id, name)| format!("{{\"id\": {}, \"name\": {}}}", dump_str(id), dump_str(name)))
        .collect();
    Ok((format!("[{}]", parts.join(", ")), rows.len()))
}

/// Fetch, transform, atomically write `out`. On any error `out` is untouched.
pub fn refresh(
    fetch: &dyn Fn(&str) -> Result<String, String>,
    out: &Path,
) -> Result<usize, String> {
    let base = std::env::var("OPENROUTER_BASE_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BASE.to_string());
    let url = format!("{}/models", base.trim().trim_end_matches('/'));
    let body = fetch(&url)?;
    let (text, n) = transform(&body)?;
    let tmp = out.with_extension("json.tmp");
    fs::write(&tmp, text).map_err(|e| format!("write tmp: {e}"))?;
    fs::rename(&tmp, out).map_err(|e| format!("rename: {e}"))?;
    Ok(n)
}

/// Real fetcher: curl, key via stdin config so it never shows in argv.
pub fn curl_fetch(url: &str) -> Result<String, String> {
    let key = api_key();
    let key = key.trim();
    if key.is_empty() {
        return Err("no OpenRouter key (OPENROUTER_API_KEY or OPENROUTER_KEY_FILE); keeping the stale cache".into());
    }
    if key.contains('"') || key.contains('\n') {
        return Err("OPENROUTER_API_KEY has unusable characters".into());
    }
    let mut child = Command::new("curl")
        .args(["-sS", "--fail", "--max-time", "20", "-K", "-", url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    if let Some(mut si) = child.stdin.take() {
        let _ = writeln!(si, "header = \"Authorization: Bearer {key}\"");
    }
    let o = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
    if !o.status.success() {
        return Err(format!("fetch failed (curl exit {:?})", o.status.code()));
    }
    String::from_utf8(o.stdout).map_err(|_| "response not utf-8".to_string())
}

/// The OpenRouter key, read at call time and never logged: OPENROUTER_API_KEY,
/// else the `OPENROUTER_API_KEY=` line of the env-format file named by
/// OPENROUTER_KEY_FILE (the old models.py read /workspace/hermes/.env the same
/// way; the staged unit names that file explicitly).
fn api_key() -> String {
    if let Ok(k) = std::env::var("OPENROUTER_API_KEY") {
        if !k.trim().is_empty() {
            return k;
        }
    }
    let Ok(path) = std::env::var("OPENROUTER_KEY_FILE") else {
        return String::new();
    };
    key_from_env_file(&fs::read_to_string(path.trim()).unwrap_or_default())
}

fn key_from_env_file(text: &str) -> String {
    let mut key = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("OPENROUTER_API_KEY=") {
            key = rest.trim().trim_matches('"').to_string();
        }
    }
    key
}

/// Failure backoff for the refresh (asked for in STAGED.md, B11): after a
/// failed fetch the next try waits 60 s, then 120, 240 ... capped at 3600 s;
/// one success resets it. Only one refresh runs at a time; a request that
/// finds one running just serves the cache.
pub struct Gate {
    state: Mutex<(u32, u64)>,
    busy: AtomicBool,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Refreshed(usize),
    Failed(u32),
    BackingOff(u64),
    Busy,
}

pub fn delay_for(fails: u32) -> u64 {
    if fails == 0 {
        return 0;
    }
    (60u64 << (fails - 1).min(6)).min(3600)
}

impl Gate {
    pub const fn new() -> Gate {
        Gate {
            state: Mutex::new((0, 0)),
            busy: AtomicBool::new(false),
        }
    }

    pub fn run(
        &self,
        now: u64,
        fetch: &dyn Fn(&str) -> Result<String, String>,
        out: &Path,
    ) -> Outcome {
        let next_at = self.state.lock().map(|s| s.1).unwrap_or(0);
        if now < next_at {
            return Outcome::BackingOff(next_at - now);
        }
        if self.busy.swap(true, Ordering::SeqCst) {
            return Outcome::Busy;
        }
        let result = refresh(fetch, out);
        let outcome = match self.state.lock() {
            Ok(mut s) => match result {
                Ok(n) => {
                    *s = (0, 0);
                    Outcome::Refreshed(n)
                }
                Err(_) => {
                    s.0 = s.0.saturating_add(1);
                    s.1 = now + delay_for(s.0);
                    Outcome::Failed(s.0)
                }
            },
            Err(_) => Outcome::Failed(0),
        };
        self.busy.store(false, Ordering::SeqCst);
        outcome
    }
}

static LIVE: Gate = Gate::new();

/// Live refresh used by galion-one: gated (backoff + single flight). Callers
/// keep serving the stale cache whatever the outcome.
pub fn refresh_live(out: &Path) -> Outcome {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let o = LIVE.run(now, &curl_fetch, out);
    if let Outcome::Failed(n) = o {
        eprintln!(
            "models: refresh failed ({n} in a row), next try in {} s",
            delay_for(n)
        );
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        fs::read_to_string(p).expect("fixture")
    }

    fn out_path(name: &str) -> std::path::PathBuf {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-out");
        fs::create_dir_all(&d).unwrap();
        d.join(name)
    }

    #[test]
    fn golden_refresh_matches_expected_file() {
        let out = out_path("models-cache.json");
        let _ = fs::remove_file(&out);
        let body = fixture("openrouter_models.json");
        let mock = move |_u: &str| -> Result<String, String> { Ok(body.clone()) };
        let n = refresh(&mock, &out).expect("refresh");
        assert_eq!(n, 3);
        assert_eq!(
            fs::read_to_string(&out).unwrap(),
            fixture("models_cache_expected.json")
        );
    }

    #[test]
    fn fetch_error_keeps_old_file() {
        let out = out_path("models-cache-stale.json");
        fs::write(&out, "[]").unwrap();
        let mock = |_u: &str| -> Result<String, String> { Err("down".into()) };
        assert!(refresh(&mock, &out).is_err());
        assert_eq!(fs::read_to_string(&out).unwrap(), "[]");
    }

    #[test]
    fn backoff_doubles_caps_and_resets() {
        assert_eq!(
            (1..=8).map(delay_for).collect::<Vec<_>>(),
            [60, 120, 240, 480, 960, 1920, 3600, 3600]
        );
        let out = out_path("models-cache-gate.json");
        fs::write(&out, "[]").unwrap();
        let gate = Gate::new();
        let calls = std::cell::Cell::new(0);
        let down = |_u: &str| -> Result<String, String> {
            calls.set(calls.get() + 1);
            Err("down".into())
        };
        assert_eq!(gate.run(1000, &down, &out), Outcome::Failed(1));
        assert_eq!(gate.run(1010, &down, &out), Outcome::BackingOff(50));
        assert_eq!(gate.run(1060, &down, &out), Outcome::Failed(2));
        assert_eq!(gate.run(1100, &down, &out), Outcome::BackingOff(80));
        assert_eq!(calls.get(), 2, "no fetch while backing off");
        let body = fixture("openrouter_models.json");
        let up = move |_u: &str| -> Result<String, String> { Ok(body.clone()) };
        assert_eq!(gate.run(1180, &up, &out), Outcome::Refreshed(3));
        assert_eq!(gate.run(1181, &down, &out), Outcome::Failed(1));
        assert_eq!(
            fs::read_to_string(&out).unwrap(),
            fixture("models_cache_expected.json")
        );
    }

    #[test]
    fn key_file_parsing() {
        let text = "A=1\nOPENROUTER_API_KEY=\"sk-test-not-real\"\nB=2\n";
        assert_eq!(key_from_env_file(text), "sk-test-not-real");
        assert_eq!(key_from_env_file("A=1\n"), "");
    }

    #[test]
    fn bad_json_is_error() {
        assert!(transform("{\"data\": [").is_err());
    }
}
