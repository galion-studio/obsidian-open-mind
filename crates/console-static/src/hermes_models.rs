// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Hermes model picker (owner t364u/t365u, 2026-10-29 CET +01:00): the
//! provider model catalogs and the picker's saved choices, server-side.
//!
//! * **Catalogs.** `GET /api/hermes/models?provider=openrouter|river` lists
//!   the models a research run can use. The lists are fetched here, on the
//!   server: OpenRouter's public `/api/v1/models` (no key needed; only
//!   text-out models that support tool calls, since Hermes research always
//!   sends tools) and River AI's `/v1/models` (needs `RIVER_API_KEY`, read
//!   from the gateway env file by name, sent to curl on stdin, never in argv,
//!   a reply or a log). Lists are cached for [`TTL`]; a failed refresh keeps
//!   serving the last good list marked `stale`.
//! * **Prefs.** `GET/POST /api/hermes/prefs`: the chosen provider, the chosen
//!   model per provider and the starred (favourite) models per provider. The
//!   old Node panel kept these server-side in `/workspace/hermes/modes.json`
//!   (`stars`, `provider`, `modes`); this keeps them in `--hermes-prefs FILE`
//!   (atomic write), so favourites follow the owner across devices. Without
//!   the flag the reply says `stored:false` and the page keeps them in
//!   `localStorage` only.

use console_ui::json::{self, obj, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// OpenRouter's public model list.
pub const OPENROUTER_MODELS: &str = "https://openrouter.ai/api/v1/models";
/// River AI's model list (OpenAI-style, needs the River key).
pub const RIVER_MODELS: &str = "https://api.river.ai/v1/models";
/// How long a fetched catalog is served before a refresh.
pub const TTL: Duration = Duration::from_secs(10 * 60);
/// Largest catalog body accepted from a provider (bytes).
pub const MAX_CATALOG: usize = 16 * 1024 * 1024;
/// Starred models kept per provider.
pub const MAX_STARS: usize = 48;
/// Providers with a model catalog (the others have one fixed model).
pub const CATALOG_PROVIDERS: &[&str] = &["openrouter", "river"];

fn s(v: &str) -> Value {
    Value::Str(v.to_string())
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// A model id the page may send: `[A-Za-z0-9._:/@+-]{1,160}`.
pub fn model_ok(m: &str) -> bool {
    !m.is_empty()
        && m.len() <= 160
        && m.bytes().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b':' | b'/' | b'-' | b'@' | b'+')
        })
}

/// A catalog URL: `https://...`, or plain `http://127.0.0.1...` (tests).
pub fn url_ok(u: &str) -> bool {
    let safe = !u.is_empty()
        && u.len() <= 300
        && u.bytes()
            .all(|c| c.is_ascii_graphic() && c != b'\'' && c != b'"' && c != b'\\');
    safe && (u.starts_with("https://") || u.starts_with("http://127.0.0.1:"))
}

/// USD per million tokens from OpenRouter's per-token price string.
fn per_mtok(v: &Value) -> Option<f64> {
    let p = match v {
        Value::Str(x) => x.trim().parse::<f64>().ok()?,
        Value::Num(n) => *n,
        _ => return None,
    };
    (p.is_finite() && p >= 0.0).then(|| (p * 1_000_000.0 * 1000.0).round() / 1000.0)
}

fn has(list: &Value, want: &str) -> bool {
    list.as_array().iter().any(|x| x.as_str() == Some(want))
}

/// OpenRouter `/models` -> picker rows: text-out models with tool calls,
/// sorted by name.
pub fn openrouter_rows(v: &Value) -> Vec<Value> {
    let mut rows: Vec<(String, Value)> = v
        .get("data")
        .as_array()
        .iter()
        .filter_map(|m| {
            let id = m.get("id").as_str()?.trim();
            if !model_ok(id) || !id.contains('/') {
                return None;
            }
            let outs = m.get("architecture").get("output_modalities");
            if !outs.as_array().is_empty() && !has(outs, "text") {
                return None;
            }
            if !has(m.get("supported_parameters"), "tools") {
                return None;
            }
            let name = m.get("name").as_str().unwrap_or(id).trim();
            let name = if name.is_empty() { id } else { name };
            let mut pairs: Vec<(&str, Value)> = vec![("id", s(id)), ("name", s(name))];
            if let Some(c) = m.get("context_length").as_f64() {
                pairs.push(("ctx", Value::Num(c)));
            }
            if let Some(p) = per_mtok(m.get("pricing").get("prompt")) {
                pairs.push(("in", Value::Num(p)));
            }
            if let Some(p) = per_mtok(m.get("pricing").get("completion")) {
                pairs.push(("out", Value::Num(p)));
            }
            Some((name.to_ascii_lowercase(), obj(&pairs)))
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows.dedup_by(|a, b| a.1.get("id").as_str() == b.1.get("id").as_str());
    rows.into_iter().map(|(_, v)| v).collect()
}

/// River `/v1/models` -> picker rows (ids only; River gives no metadata).
pub fn river_rows(v: &Value) -> Vec<Value> {
    let mut rows: Vec<String> = v
        .get("data")
        .as_array()
        .iter()
        .filter_map(|m| m.get("id").as_str().map(|x| x.trim().to_string()))
        .filter(|id| model_ok(id))
        .collect();
    rows.sort_by_key(|a| a.to_ascii_lowercase());
    rows.dedup();
    rows.into_iter()
        .map(|id| obj(&[("id", s(&id)), ("name", s(&id))]))
        .collect()
}

/// GET `url` with curl; `bearer` (if any) goes to curl on stdin as a header
/// file (`-H @-`), so it is never in argv. curl's stderr is dropped (it could
/// echo a header).
pub fn fetch(url: &str, bearer: Option<&str>) -> Result<Vec<u8>, String> {
    if !url_ok(url) {
        return Err("catalog URL refused".into());
    }
    let proto = if url.starts_with("https://") {
        "=https"
    } else {
        "=http"
    };
    let mut cmd = Command::new("curl");
    cmd.args([
        "-sS",
        "--fail",
        "--max-time",
        "25",
        "--connect-timeout",
        "8",
        "--max-filesize",
        &MAX_CATALOG.to_string(),
        "--proto",
        proto,
        "--proto-redir",
        "=https",
        "-H",
        "Accept: application/json",
    ]);
    if bearer.is_some() {
        cmd.args(["-H", "@-"]);
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.arg(url).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = cmd
        .spawn()
        .map_err(|_| "curl is not available".to_string())?;
    if let Some(k) = bearer {
        if let Some(mut w) = child.stdin.take() {
            let _ = w.write_all(format!("Authorization: Bearer {k}\n").as_bytes());
        }
    }
    let out = child
        .wait_with_output()
        .map_err(|_| "the model list fetch failed".to_string())?;
    if !out.status.success() {
        return Err(match out.status.code() {
            Some(22) => "the provider refused the model list request".into(),
            Some(28) => "the provider did not answer in time".into(),
            Some(6) | Some(7) => "the provider is not reachable".into(),
            _ => "the model list fetch failed".into(),
        });
    }
    if out.stdout.len() > MAX_CATALOG {
        return Err("the model list is too large".into());
    }
    Ok(out.stdout)
}

/// Where the catalogs come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sources {
    pub openrouter: String,
    pub river: String,
}

impl Default for Sources {
    fn default() -> Self {
        Sources {
            openrouter: OPENROUTER_MODELS.into(),
            river: RIVER_MODELS.into(),
        }
    }
}

/// One cached catalog.
#[derive(Debug, Clone)]
pub struct Catalog {
    pub models: Vec<Value>,
    pub fetched_at: f64,
    pub stale: bool,
    pub error: Option<String>,
}

impl Catalog {
    pub fn has(&self, id: &str) -> bool {
        self.models.iter().any(|m| m.get("id").as_str() == Some(id))
    }
}

/// Catalog cache. `Debug` never shows the River key.
pub struct Catalogs {
    pub sources: Sources,
    river_key: Option<String>,
    cache: Mutex<HashMap<String, (Instant, Catalog)>>,
}

impl std::fmt::Debug for Catalogs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Catalogs")
            .field("sources", &self.sources)
            .field("river_key", &self.river_key.as_ref().map(|_| "<set>"))
            .finish()
    }
}

impl Catalogs {
    pub fn new(sources: Sources, river_key: Option<String>) -> Catalogs {
        Catalogs {
            sources,
            river_key,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn has_river_key(&self) -> bool {
        self.river_key.is_some()
    }

    fn load(&self, provider: &str) -> Result<Vec<Value>, String> {
        match provider {
            "openrouter" => {
                let body = fetch(&self.sources.openrouter, None)?;
                let v = json::parse(&body).map_err(|_| "unreadable model list".to_string())?;
                Ok(openrouter_rows(&v))
            }
            "river" => {
                let key = self
                    .river_key
                    .as_deref()
                    .ok_or_else(|| "the River AI key is not set on the server".to_string())?;
                let body = fetch(&self.sources.river, Some(key))?;
                let v = json::parse(&body).map_err(|_| "unreadable model list".to_string())?;
                Ok(river_rows(&v))
            }
            _ => Err("no model list for this provider".into()),
        }
    }

    /// The catalog for `provider` (cached for [`TTL`]). `Err` only when no
    /// list was ever fetched.
    pub fn get(&self, provider: &str) -> Result<Catalog, String> {
        if let Some((at, c)) = self
            .cache
            .lock()
            .ok()
            .and_then(|m| m.get(provider).cloned())
        {
            if at.elapsed() < TTL {
                return Ok(c);
            }
        }
        match self.load(provider) {
            Ok(models) if !models.is_empty() => {
                let c = Catalog {
                    models,
                    fetched_at: now_secs(),
                    stale: false,
                    error: None,
                };
                if let Ok(mut m) = self.cache.lock() {
                    m.insert(provider.to_string(), (Instant::now(), c.clone()));
                }
                Ok(c)
            }
            other => {
                let why = match other {
                    Err(e) => e,
                    Ok(_) => "the provider listed no usable models".into(),
                };
                let old = self
                    .cache
                    .lock()
                    .ok()
                    .and_then(|m| m.get(provider).cloned());
                match old {
                    Some((_, mut c)) => {
                        c.stale = true;
                        c.error = Some(why);
                        Ok(c)
                    }
                    None => Err(why),
                }
            }
        }
    }
}

/// Saved picker choices (`--hermes-prefs`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Prefs {
    /// Chosen provider ("" = the page default).
    pub provider: String,
    /// Chosen model per catalog provider ("" = that provider's default).
    pub models: Vec<(String, String)>,
    /// Starred models per catalog provider, newest first.
    pub stars: Vec<(String, Vec<String>)>,
}

impl Prefs {
    pub fn model(&self, p: &str) -> &str {
        self.models
            .iter()
            .find(|(k, _)| k == p)
            .map(|(_, v)| v.as_str())
            .unwrap_or("")
    }

    pub fn stars(&self, p: &str) -> &[String] {
        self.stars
            .iter()
            .find(|(k, _)| k == p)
            .map(|(_, v)| v.as_slice())
            .unwrap_or(&[])
    }

    fn set_model(&mut self, p: &str, m: &str) {
        self.models.retain(|(k, _)| k != p);
        self.models.push((p.to_string(), m.to_string()));
    }

    /// Star (`on`), unstar, or toggle (`None`).
    fn star(&mut self, p: &str, m: &str, on: Option<bool>) {
        let mut cur: Vec<String> = self.stars(p).to_vec();
        let had = cur.iter().any(|x| x == m);
        let want = on.unwrap_or(!had);
        cur.retain(|x| x != m);
        if want {
            cur.insert(0, m.to_string());
        }
        cur.truncate(MAX_STARS);
        self.stars.retain(|(k, _)| k != p);
        self.stars.push((p.to_string(), cur));
    }

    /// Read from JSON, dropping anything that is not a valid id.
    pub fn from_value(v: &Value) -> Prefs {
        let mut p = Prefs::default();
        if let Some(x) = v.get("provider").as_str() {
            if crate::hermes::PROVIDERS.iter().any(|q| q.id == x) {
                p.provider = x.to_string();
            }
        }
        for c in CATALOG_PROVIDERS {
            if let Some(m) = v.get("models").get(c).as_str() {
                if m.is_empty() || model_ok(m) {
                    p.set_model(c, m);
                }
            }
            let mut st: Vec<String> = Vec::new();
            for x in v.get("stars").get(c).as_array() {
                if let Some(m) = x.as_str() {
                    if model_ok(m) && !st.iter().any(|y| y == m) {
                        st.push(m.to_string());
                    }
                }
            }
            st.truncate(MAX_STARS);
            p.stars.push((c.to_string(), st));
        }
        p
    }

    pub fn to_value(&self, stored: bool) -> Value {
        let models: Vec<(&str, Value)> = CATALOG_PROVIDERS
            .iter()
            .map(|c| (*c, s(self.model(c))))
            .collect();
        let stars: Vec<(&str, Value)> = CATALOG_PROVIDERS
            .iter()
            .map(|c| (*c, Value::Arr(self.stars(c).iter().map(|m| s(m)).collect())))
            .collect();
        obj(&[
            ("ok", Value::Bool(true)),
            ("stored", Value::Bool(stored)),
            ("provider", s(&self.provider)),
            ("models", obj(&models)),
            ("stars", obj(&stars)),
        ])
    }
}

/// The prefs file (one writer at a time within this process; writes are
/// atomic renames, so the other console unit never reads half a file).
#[derive(Debug)]
pub struct PrefsStore {
    pub path: Option<PathBuf>,
    lock: Mutex<()>,
}

impl PrefsStore {
    pub fn new(path: Option<PathBuf>) -> PrefsStore {
        PrefsStore {
            path,
            lock: Mutex::new(()),
        }
    }

    fn read_at(path: &Path) -> Prefs {
        std::fs::read(path)
            .ok()
            .and_then(|b| json::parse(&b).ok())
            .map(|v| Prefs::from_value(&v))
            .unwrap_or_else(|| Prefs::from_value(&Value::Null))
    }

    pub fn read(&self) -> Prefs {
        match &self.path {
            Some(p) => Self::read_at(p),
            None => Prefs::from_value(&Value::Null),
        }
    }

    fn write_at(path: &Path, p: &Prefs) -> std::io::Result<()> {
        let mut v = p.to_value(true);
        if let Value::Obj(pairs) = &mut v {
            pairs.retain(|(k, _)| k != "ok" && k != "stored");
        }
        let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
        std::fs::write(&tmp, v.to_json() + "\n")?;
        std::fs::rename(&tmp, path)
    }

    /// Apply a `POST /api/hermes/prefs` body. `Err((status, message))`.
    pub fn apply(&self, body: &[u8]) -> Result<Value, (u16, String)> {
        let req = match json::parse(body) {
            Ok(v @ Value::Obj(_)) => v,
            _ => {
                return Err((
                    400,
                    "send JSON: {\"action\":\"select\"|\"star\",...}".into(),
                ))
            }
        };
        let action = req.get("action").as_str().unwrap_or("");
        let provider = req.get("provider").as_str().unwrap_or("");
        let model = req.get("model").as_str().unwrap_or("").trim();
        if !crate::hermes::PROVIDERS.iter().any(|q| q.id == provider) {
            return Err((400, "unknown provider".into()));
        }
        let catalog = CATALOG_PROVIDERS.contains(&provider);
        if !model.is_empty() && (!catalog || !model_ok(model)) {
            return Err((400, "that model id is not valid for this provider".into()));
        }
        let _g = self
            .lock
            .lock()
            .map_err(|_| (500, "prefs lock".to_string()))?;
        let mut p = self.read();
        match action {
            "select" => {
                p.provider = provider.to_string();
                if catalog {
                    p.set_model(provider, model);
                }
            }
            "star" if catalog && !model.is_empty() => {
                p.star(provider, model, req.get("on").as_bool());
            }
            "star" => return Err((400, "star needs an OpenRouter or River AI model".into())),
            _ => return Err((400, "action must be select or star".into())),
        }
        match &self.path {
            Some(path) => {
                Self::write_at(path, &p).map_err(|_| {
                    (
                        503,
                        "the picker choices could not be saved on the server".to_string(),
                    )
                })?;
                Ok(p.to_value(true))
            }
            None => Ok(p.to_value(false)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_urls() {
        assert!(model_ok("anthropic/claude-sonnet-5.5"));
        assert!(model_ok("GLM-5.3"));
        assert!(model_ok("openai/gpt-6:online"));
        assert!(!model_ok(""));
        assert!(!model_ok("a b"));
        assert!(!model_ok("x\"y"));
        assert!(url_ok(OPENROUTER_MODELS) && url_ok(RIVER_MODELS));
        assert!(url_ok("http://127.0.0.1:9/models"));
        assert!(!url_ok("http://example.com/models"));
        assert!(!url_ok("file:///etc/passwd"));
        assert!(!url_ok("https://x/'y"));
    }

    #[test]
    fn openrouter_filter_keeps_text_tool_models() {
        let v = json::parse(br#"{"data":[
          {"id":"b/tools","name":"B Tools","context_length":1000,"architecture":{"output_modalities":["text"]},"pricing":{"prompt":"0.000003","completion":"0.000015"},"supported_parameters":["tools","reasoning"]},
          {"id":"a/notools","name":"A","architecture":{"output_modalities":["text"]},"supported_parameters":["temperature"]},
          {"id":"c/image","name":"C","architecture":{"output_modalities":["image"]},"supported_parameters":["tools"]},
          {"id":"noslash","name":"D","supported_parameters":["tools"]},
          {"id":"a/first","name":"A First","supported_parameters":["tools"]}
        ]}"#).unwrap();
        let rows = openrouter_rows(&v);
        let ids: Vec<&str> = rows.iter().map(|r| r.get("id").as_str().unwrap()).collect();
        assert_eq!(ids, ["a/first", "b/tools"]);
        assert_eq!(rows[1].get("in").as_f64(), Some(3.0));
        assert_eq!(rows[1].get("out").as_f64(), Some(15.0));
        assert_eq!(rows[1].get("ctx").as_f64(), Some(1000.0));
    }

    #[test]
    fn river_rows_sorted_and_clean() {
        let v = json::parse(br#"{"data":[{"id":"zai-org/GLM-5.3-Flash"},{"id":"GLM-5.3"},{"id":"bad id"},{"id":"deepseek-ai/DeepSeek-V4.1-Flash"}]}"#).unwrap();
        let ids: Vec<String> = river_rows(&v)
            .iter()
            .map(|r| r.get("id").as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            ids,
            [
                "deepseek-ai/DeepSeek-V4.1-Flash",
                "GLM-5.3",
                "zai-org/GLM-5.3-Flash"
            ]
        );
    }

    #[test]
    fn prefs_select_star_and_persist() {
        let dir = std::env::temp_dir().join(format!("hm-prefs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("prefs.json");
        let _ = std::fs::remove_file(&f);
        let st = PrefsStore::new(Some(f.clone()));
        let v = st
            .apply(br#"{"action":"select","provider":"river","model":"GLM-5.3"}"#)
            .unwrap();
        assert_eq!(v.get("provider").as_str(), Some("river"));
        assert_eq!(v.get("models").get("river").as_str(), Some("GLM-5.3"));
        st.apply(br#"{"action":"star","provider":"openrouter","model":"a/b"}"#)
            .unwrap();
        st.apply(br#"{"action":"star","provider":"openrouter","model":"c/d"}"#)
            .unwrap();
        let v = st
            .apply(br#"{"action":"star","provider":"openrouter","model":"a/b"}"#)
            .unwrap();
        assert_eq!(v.get("stars").get("openrouter").as_array().len(), 1);
        let again = PrefsStore::new(Some(f.clone())).read();
        assert_eq!(again.provider, "river");
        assert_eq!(again.stars("openrouter"), ["c/d".to_string()]);
        for bad in [
            &br#"{"action":"star","provider":"laptop","model":"x/y"}"#[..],
            br#"{"action":"select","provider":"nope"}"#,
            br#"{"action":"select","provider":"openrouter","model":"a b"}"#,
            br#"{"action":"drop","provider":"openrouter"}"#,
            b"nope",
        ] {
            assert_eq!(st.apply(bad).unwrap_err().0, 400);
        }
        // No file: answers, stores nothing.
        let mem = PrefsStore::new(None);
        let v = mem
            .apply(br#"{"action":"select","provider":"openrouter","model":"a/b"}"#)
            .unwrap();
        assert_eq!(v.get("stored").as_bool(), Some(false));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
