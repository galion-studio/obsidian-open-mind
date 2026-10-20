// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// OpenRouter model list refresh. Replaces the old one/models.py launcher; the
// output file format is the same: a JSON array of {"id","name"} objects sorted
// by lowercased name, written the way models.py json.dumps writes it
// (", " and ": " separators, non-ASCII escaped as \uXXXX).
//
// The HTTP fetch is injected so tests never touch the network. The API key is
// read from the process env var OPENROUTER_API_KEY at call time only and is
// handed to curl through stdin, never argv, and never logged.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub const DEFAULT_BASE: &str = "https://openrouter.ai/api/v1";

#[derive(Debug, Clone, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Num,
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn ws(&mut self) {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn lit(&mut self, word: &str, v: Json) -> Result<Json, String> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(v)
        } else {
            Err(format!("bad json at {}", self.i))
        }
    }

    fn value(&mut self, depth: u32) -> Result<Json, String> {
        if depth > 64 {
            return Err("json too deep".into());
        }
        self.ws();
        match self.b.get(self.i) {
            None => Err("unexpected end of json".into()),
            Some(b'n') => self.lit("null", Json::Null),
            Some(b't') => self.lit("true", Json::Bool(true)),
            Some(b'f') => self.lit("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::Str),
            Some(b'[') => {
                self.i += 1;
                let mut out = Vec::new();
                loop {
                    self.ws();
                    if self.b.get(self.i) == Some(&b']') {
                        self.i += 1;
                        return Ok(Json::Arr(out));
                    }
                    out.push(self.value(depth + 1)?);
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {}
                        _ => return Err(format!("bad array at {}", self.i)),
                    }
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut out = Vec::new();
                loop {
                    self.ws();
                    if self.b.get(self.i) == Some(&b'}') {
                        self.i += 1;
                        return Ok(Json::Obj(out));
                    }
                    let k = self.string()?;
                    self.ws();
                    if self.b.get(self.i) != Some(&b':') {
                        return Err(format!("expected colon at {}", self.i));
                    }
                    self.i += 1;
                    out.push((k, self.value(depth + 1)?));
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {}
                        _ => return Err(format!("bad object at {}", self.i)),
                    }
                }
            }
            Some(_) => {
                let start = self.i;
                while self.i < self.b.len()
                    && matches!(
                        self.b[self.i],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.i += 1;
                }
                if self.i == start {
                    Err(format!("bad json at {}", self.i))
                } else {
                    Ok(Json::Num)
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let s = self
            .b
            .get(self.i..self.i + 4)
            .and_then(|s| std::str::from_utf8(s).ok())
            .ok_or("bad unicode escape")?;
        let v = u32::from_str_radix(s, 16).map_err(|_| "bad unicode escape".to_string())?;
        self.i += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, String> {
        if self.b.get(self.i) != Some(&b'"') {
            return Err(format!("expected string at {}", self.i));
        }
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = *self.b.get(self.i).ok_or("unterminated string")?;
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = *self.b.get(self.i).ok_or("unterminated escape")?;
                    self.i += 1;
                    let ch = match e {
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp)
                                && self.b.get(self.i..self.i + 2) == Some(b"\\u")
                            {
                                self.i += 2;
                                let lo = self.hex4()?;
                                cp = 0x10000
                                    + ((cp - 0xD800) << 10)
                                    + (lo.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            char::from_u32(cp).unwrap_or('\u{fffd}')
                        }
                        other => other as char,
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                _ => out.push(c),
            }
        }
        String::from_utf8(out).map_err(|_| "invalid utf-8 in string".to_string())
    }
}

fn parse(text: &str) -> Result<Json, String> {
    let mut p = Parser {
        b: text.as_bytes(),
        i: 0,
    };
    let v = p.value(0)?;
    p.ws();
    if p.i != p.b.len() {
        return Err("trailing data after json".into());
    }
    Ok(v)
}

fn get<'a>(obj: &'a Json, key: &str) -> Option<&'a Json> {
    match obj {
        Json::Obj(kv) => kv.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn text_of(v: Option<&Json>) -> String {
    match v {
        Some(Json::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

/// models.py json.dumps string form (ensure_ascii).
fn dump_str(s: &str) -> String {
    let mut o = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            '\u{8}' => o.push_str("\\b"),
            '\u{c}' => o.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    o.push_str(&format!("\\u{u:04x}"));
                }
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

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
    let key = std::env::var("OPENROUTER_API_KEY").unwrap_or_default();
    let key = key.trim();
    if key.is_empty() {
        return Err("OPENROUTER_API_KEY is not set; keeping the stale cache".into());
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

/// Live refresh used by galion-one. Errors are returned; callers keep serving the stale cache.
pub fn refresh_live(out: &Path) -> Result<usize, String> {
    refresh(&curl_fetch, out)
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
    fn bad_json_is_error() {
        assert!(transform("{\"data\": [").is_err());
    }
}
