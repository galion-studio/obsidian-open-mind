// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! github-mcp-rs: a stdio MCP server whose tool list is EXACTLY the Grok Bot
//! `cursor-github` interface (names, descriptions, input schemas; see
//! `tools.json` next to this file, a copy of
//! `tests/fixtures/grokbot-github-mcp-schemas.json`; the
//! `github_mcp_interface` test diffs what Hermes registers against that
//! reference), implemented over the GitHub
//! REST and GraphQL APIs with the system `curl` (config on stdin, so the
//! token never appears on a command line).
//!
//! Read/write split (from the reference): `get_*`, `list_*`, `search_*` and
//! `compare_commits` are reads; every other tool writes. Write tools are
//! listed (so the interface is identical) but refused unless the server was
//! started with `--allow-writes`.
//!
//! The token is read on every call from `GITHUB_PERSONAL_ACCESS_TOKEN` or the
//! `--token-file` (mode 0600/0400; `KEY=VALUE` or a bare token), so a token
//! dropped in later is picked up without a restart. It is never logged or
//! echoed; errors name only the file.

use serde_json::{json, Map, Value};
use std::io::{Read, Write};
use std::process::{Command, Stdio};

pub const REFERENCE: &str = include_str!("tools.json");
pub const TOKEN_ENV: &str = "GITHUB_PERSONAL_ACCESS_TOKEN";

/// The reference tool list (`name`, `description`, `inputSchema`).
pub fn reference_tools() -> Vec<Value> {
    let v: Value = serde_json::from_str(REFERENCE).expect("reference json");
    let mut tools = v["tools"].as_array().cloned().unwrap_or_default();
    tools.iter_mut().for_each(join_descriptions);
    tools
}

/// `tools.json` stores long `description` strings as arrays of chunks (so no
/// line is over 400 chars); joined with "" they are the exact reference text.
pub fn join_descriptions(v: &mut Value) {
    match v {
        Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                if k == "description" {
                    if let Value::Array(parts) = x {
                        if parts.iter().all(Value::is_string) {
                            let s: String = parts.iter().filter_map(Value::as_str).collect();
                            *x = Value::String(s);
                            continue;
                        }
                    }
                }
                join_descriptions(x);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(join_descriptions),
        _ => {}
    }
}

pub fn is_read(name: &str) -> bool {
    name.starts_with("get_")
        || name.starts_with("list_")
        || name.starts_with("search_")
        || name == "compare_commits"
}

fn is_destructive(name: &str) -> bool {
    name == "merge_pull_request" || name.starts_with("delete_")
}

/// `tools/list` result: the reference list plus MCP annotations for the split.
pub fn tools_list() -> Value {
    let tools: Vec<Value> = reference_tools()
        .into_iter()
        .map(|mut t| {
            let n = t["name"].as_str().unwrap_or("").to_string();
            t["annotations"] = json!({
                "readOnlyHint": is_read(&n),
                "destructiveHint": is_destructive(&n),
                "openWorldHint": true,
            });
            t
        })
        .collect();
    json!({ "tools": tools })
}

pub struct Server {
    pub api: String,
    pub token_file: Option<String>,
    pub allow_writes: bool,
    pub curl: String,
    pub timeout_s: u64,
    /// Cap on one reply's text (bytes).
    pub max_reply: usize,
}

impl Default for Server {
    fn default() -> Self {
        Server {
            api: "https://api.github.com".into(),
            token_file: None,
            allow_writes: false,
            curl: "curl".into(),
            timeout_s: 60,
            max_reply: 4 << 20,
        }
    }
}

pub struct Resp {
    pub status: u16,
    pub body: String,
}

type R<T> = Result<T, String>;

fn curl_quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Percent-encode one path segment (`keep_slash` for paths and branch names).
pub fn enc(s: &str, keep_slash: bool) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        let ok = b.is_ascii_alphanumeric() || b"-._~".contains(&b) || (keep_slash && b == b'/');
        if ok {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn loopback(url: &str) -> bool {
    let rest = url.strip_prefix("http://").unwrap_or("");
    let host = rest.split(['/', ':']).next().unwrap_or("");
    matches!(host, "127.0.0.1" | "localhost") || rest.starts_with("[::1]")
}

impl Server {
    /// The token, re-read on every call. Never returned in an error.
    pub fn token(&self) -> R<String> {
        if let Ok(t) = std::env::var(TOKEN_ENV) {
            if !t.trim().is_empty() {
                return Ok(t.trim().to_string());
            }
        }
        let Some(f) = &self.token_file else {
            return Err(format!(
                "GitHub token not configured yet ({TOKEN_ENV} unset and no --token-file)."
            ));
        };
        let waiting = || {
            format!(
                "GitHub token not configured yet: put a read-only fine-grained token in {f} \
                 as {TOKEN_ENV}=<token> (mode 0600). It is picked up on the next call; no restart needed."
            )
        };
        let Ok(meta) = std::fs::metadata(f) else {
            return Err(waiting());
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o077 != 0 {
                return Err(format!(
                    "token file {f} is readable by group/others; chmod 600 it (refused)."
                ));
            }
        }
        let text = std::fs::read_to_string(f).map_err(|_| waiting())?;
        for line in text.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            let v = match l.split_once('=') {
                Some((k, v)) if k.trim().trim_start_matches("export ").trim() == TOKEN_ENV => v,
                Some(_) => continue,
                None => l,
            };
            let v = v.trim().trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                return Ok(v.to_string());
            }
        }
        Err(waiting())
    }

    fn url(&self, path_and_query: &str) -> String {
        format!("{}{}", self.api.trim_end_matches('/'), path_and_query)
    }

    /// One HTTP request through curl. `accept` defaults to GitHub JSON.
    pub fn http(
        &self,
        method: &str,
        url: &str,
        body: Option<&Value>,
        accept: Option<&str>,
    ) -> R<Resp> {
        if !(url.starts_with("https://") || loopback(url)) {
            return Err(
                "refused: GitHub API URL must be https (plain http only to loopback)".into(),
            );
        }
        let token = self.token()?;
        let mut cfg = String::new();
        let mut kv = |k: &str, v: &str| cfg.push_str(&format!("{k} = {}\n", curl_quote(v)));
        kv("url", url);
        kv("request", method);
        kv("header", &format!("Authorization: Bearer {token}"));
        kv(
            "header",
            &format!(
                "Accept: {}",
                accept.unwrap_or("application/vnd.github+json")
            ),
        );
        kv("header", "X-GitHub-Api-Version: 2022-11-28");
        kv("user-agent", "github-mcp-rs (hermes-rs)");
        kv("proto", "=http,https");
        kv("proto-redir", "=https");
        kv("max-time", &self.timeout_s.to_string());
        kv("connect-timeout", "15");
        kv("max-filesize", "60000000");
        kv("write-out", "\n%{http_code}");
        if let Some(b) = body {
            kv("header", "Content-Type: application/json");
            kv("data-binary", &b.to_string());
        }
        let mut child = Command::new(&self.curl)
            .args(["-q", "-sS", "--location", "-K", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot run curl: {e}"))?;
        {
            let mut si = child.stdin.take().ok_or("curl stdin")?;
            si.write_all(cfg.as_bytes())
                .map_err(|e| format!("curl stdin: {e}"))?;
        }
        drop(token);
        let mut out = Vec::new();
        let mut so = child.stdout.take().ok_or("curl stdout")?;
        let mut se = child.stderr.take().ok_or("curl stderr")?;
        let eh = std::thread::spawn(move || {
            let mut e = String::new();
            let _ = se.read_to_string(&mut e);
            e
        });
        let _ = so.read_to_end(&mut out);
        let st = child.wait().map_err(|e| e.to_string())?;
        let err = eh.join().unwrap_or_default();
        let text = String::from_utf8_lossy(&out).into_owned();
        let (body, code) = match text.rfind('\n') {
            Some(i) => (text[..i].to_string(), text[i + 1..].trim().to_string()),
            None => (String::new(), text.trim().to_string()),
        };
        let status: u16 = code.parse().unwrap_or(0);
        if !st.success() && status == 0 {
            let e: String = err
                .lines()
                .last()
                .unwrap_or("curl failed")
                .chars()
                .take(300)
                .collect();
            return Err(format!("network error talking to GitHub: {e}"));
        }
        Ok(Resp { status, body })
    }

    fn rest(
        &self,
        method: &str,
        path: &str,
        q: &[(String, String)],
        body: Option<Value>,
        accept: Option<&str>,
    ) -> R<String> {
        let mut url = self.url(path);
        if !q.is_empty() {
            let qs: Vec<String> = q
                .iter()
                .map(|(k, v)| format!("{}={}", enc(k, false), enc(v, false)))
                .collect();
            url.push('?');
            url.push_str(&qs.join("&"));
        }
        let r = self.http(method, &url, body.as_ref(), accept)?;
        if (200..300).contains(&r.status) {
            Ok(if r.body.is_empty() {
                json!({"status": r.status}).to_string()
            } else {
                r.body
            })
        } else {
            Err(format!(
                "GitHub API {method} {path} answered {}: {}",
                r.status,
                r.body.chars().take(4000).collect::<String>()
            ))
        }
    }

    fn get(&self, path: &str, q: &[(String, String)]) -> R<String> {
        self.rest("GET", path, q, None, None)
    }

    fn get_json(&self, path: &str, q: &[(String, String)]) -> R<Value> {
        let s = self.get(path, q)?;
        serde_json::from_str(&s).map_err(|e| format!("bad JSON from GitHub: {e}"))
    }

    pub fn graphql(&self, query: &str, vars: Value) -> R<Value> {
        let url = format!("{}/graphql", self.api.trim_end_matches('/'));
        let r = self.http(
            "POST",
            &url,
            Some(&json!({"query": query, "variables": vars})),
            None,
        )?;
        let v: Value = serde_json::from_str(&r.body).map_err(|_| {
            format!(
                "GitHub GraphQL answered {}: {}",
                r.status,
                r.body.chars().take(2000).collect::<String>()
            )
        })?;
        if !(200..300).contains(&r.status) || v.get("errors").is_some_and(|e| !e.is_null()) {
            return Err(format!(
                "GitHub GraphQL error ({}): {}",
                r.status,
                v.get("errors").unwrap_or(&v)
            ));
        }
        Ok(v["data"].clone())
    }
}

// ---------- argument validation against the reference schema ----------

fn check(v: &Value, s: &Value, at: &str) -> R<()> {
    if let Some(any) = s["anyOf"].as_array() {
        if any.iter().any(|a| check(v, a, at).is_ok()) {
            return Ok(());
        }
        return Err(format!("{at}: does not match any allowed type"));
    }
    match s["type"].as_str() {
        Some("string") => {
            let t = v.as_str().ok_or(format!("{at}: must be a string"))?;
            let n = t.chars().count() as u64;
            if s["minLength"].as_u64().is_some_and(|m| n < m) {
                return Err(format!("{at}: must not be empty"));
            }
            if s["maxLength"].as_u64().is_some_and(|m| n > m) {
                return Err(format!("{at}: longer than {}", s["maxLength"]));
            }
            if s["pattern"].as_str() == Some("^[0-9a-fA-F]{6}$")
                && !(t.len() == 6 && t.bytes().all(|b| b.is_ascii_hexdigit()))
            {
                return Err(format!("{at}: must be six hex digits"));
            }
        }
        Some("integer") => {
            let i = v.as_i64().ok_or(format!("{at}: must be an integer"))?;
            if s["minimum"].as_i64().is_some_and(|m| i < m)
                || s["maximum"].as_i64().is_some_and(|m| i > m)
            {
                return Err(format!("{at}: out of range"));
            }
        }
        Some("boolean") => {
            v.as_bool().ok_or(format!("{at}: must be true or false"))?;
        }
        Some("null") => {
            if !v.is_null() {
                return Err(format!("{at}: must be null"));
            }
        }
        Some("array") => {
            let a = v.as_array().ok_or(format!("{at}: must be an array"))?;
            if s["maxItems"].as_u64().is_some_and(|m| a.len() as u64 > m) {
                return Err(format!("{at}: more than {} items", s["maxItems"]));
            }
            for (i, x) in a.iter().enumerate() {
                check(x, &s["items"], &format!("{at}[{i}]"))?;
            }
        }
        Some("object") => {
            let o = v.as_object().ok_or(format!("{at}: must be an object"))?;
            let props = s["properties"].as_object().cloned().unwrap_or_default();
            for r in s["required"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if !o.contains_key(r) {
                    return Err(format!(
                        "{at}{}{r}: required",
                        if at.is_empty() { "" } else { "." }
                    ));
                }
            }
            for (k, x) in o {
                let p = if at.is_empty() {
                    k.clone()
                } else {
                    format!("{at}.{k}")
                };
                match props.get(k) {
                    Some(ps) => check(x, ps, &p)?,
                    None if s["additionalProperties"] == false => {
                        return Err(format!("{p}: unknown argument"))
                    }
                    None => {}
                }
            }
        }
        _ => {}
    }
    if let Some(e) = s["enum"].as_array() {
        if !e.contains(v) {
            return Err(format!("{at}: must be one of {}", Value::Array(e.clone())));
        }
    }
    Ok(())
}

pub fn validate(name: &str, args: &Value) -> R<()> {
    let t = reference_tools()
        .into_iter()
        .find(|t| t["name"] == name)
        .ok_or(format!("unknown tool {name}"))?;
    check(args, &t["inputSchema"], "")
}

// ---------- tool implementations ----------

struct A<'a>(&'a Map<String, Value>);

impl A<'_> {
    fn s(&self, k: &str) -> Option<String> {
        match self.0.get(k)? {
            Value::String(s) => Some(s.clone()),
            Value::Null => None,
            v => Some(v.to_string()),
        }
    }
    fn req(&self, k: &str) -> R<String> {
        self.s(k).ok_or(format!("{k}: required"))
    }
    fn i(&self, k: &str) -> Option<i64> {
        self.0.get(k)?.as_i64()
    }
    fn b(&self, k: &str) -> Option<bool> {
        self.0.get(k)?.as_bool()
    }
    /// `/repos/{owner}/{repo}`
    fn repo(&self) -> R<String> {
        Ok(format!(
            "/repos/{}/{}",
            enc(&self.req("owner")?, false),
            enc(&self.req("repo")?, false)
        ))
    }
    fn seg(&self, k: &str) -> R<String> {
        Ok(enc(&self.req(k)?, false))
    }
    /// Query pairs for the keys present.
    fn q(&self, keys: &[&str]) -> Vec<(String, String)> {
        keys.iter()
            .filter_map(|k| self.s(k).map(|v| (k.to_string(), v)))
            .collect()
    }
    /// JSON body with the keys present (renamed `from` -> `to` when a pair).
    fn body(&self, keys: &[&str]) -> Value {
        let mut m = Map::new();
        for k in keys {
            let (from, to) = k.split_once("->").unwrap_or((k, k));
            if let Some(v) = self.0.get(from) {
                m.insert(to.to_string(), v.clone());
            }
        }
        Value::Object(m)
    }
}

const PAGE: &[&str] = &["per_page", "page"];

fn cat<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<&'a str> {
    a.iter().chain(b).copied().collect()
}

fn tail(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut i = s.len() - n;
    while !s.is_char_boundary(i) {
        i += 1;
    }
    format!(
        "[…first {} bytes cut; last {n} bytes follow]\n{}",
        i,
        &s[i..]
    )
}

impl Server {
    /// Run one tool. `Ok(text)` or `Err(text)` (an MCP `isError` result).
    pub fn call(&self, name: &str, args: &Value) -> R<String> {
        let args = if args.is_null() {
            json!({})
        } else {
            args.clone()
        };
        validate(name, &args)?;
        if !is_read(name) && !self.allow_writes {
            return Err(format!(
                "{name} is a write tool and writes are OFF on this GitHub MCP server \
                 (started without --allow-writes; the owner has not enabled MCP writes). Nothing was sent."
            ));
        }
        let o = args.as_object().cloned().unwrap_or_default();
        let a = A(&o);
        let out = self.dispatch(name, &a)?;
        Ok(if out.len() > self.max_reply {
            let mut i = self.max_reply;
            while !out.is_char_boundary(i) {
                i -= 1;
            }
            format!(
                "{}\n[…truncated at {} bytes of {}]",
                &out[..i],
                i,
                out.len()
            )
        } else {
            out
        })
    }

    fn dispatch(&self, name: &str, a: &A) -> R<String> {
        let none: &[(String, String)] = &[];
        match name {
            // ----- reads: account / search -----
            "get_me" => Ok(format!("[{}]", self.get("/user", none)?)),
            "list_my_repositories" => Ok(format!(
                "[{}]",
                self.get(
                    "/user/repos",
                    &a.q(&cat(
                        &["affiliation", "visibility", "sort", "direction"],
                        PAGE
                    ))
                )?
            )),
            "search_code" => self.get("/search/code", &a.q(&["q", "per_page", "page"])),
            "search_commits"
            | "search_issues"
            | "search_pull_requests"
            | "search_repositories"
            | "search_users"
            | "search_orgs" => {
                let (path, extra) = match name {
                    "search_commits" => ("/search/commits", ""),
                    "search_issues" => ("/search/issues", " is:issue"),
                    "search_pull_requests" => ("/search/issues", " is:pr"),
                    "search_repositories" => ("/search/repositories", ""),
                    "search_users" => ("/search/users", " type:user"),
                    _ => ("/search/users", " type:org"),
                };
                let mut q = vec![("q".to_string(), format!("{}{extra}", a.req("q")?))];
                q.extend(a.q(&["sort", "order", "per_page", "page"]));
                self.get(path, &q)
            }
            // ----- reads: repository -----
            "get_repository" => self.get(&a.repo()?, none),
            "get_branch" => self.get(
                &format!("{}/branches/{}", a.repo()?, enc(&a.req("branch")?, true)),
                none,
            ),
            "list_branches" => self.get(
                &format!("{}/branches", a.repo()?),
                &a.q(&cat(&["protected"], PAGE)),
            ),
            "get_commit" => self.get(
                &format!("{}/commits/{}", a.repo()?, enc(&a.req("ref")?, true)),
                &a.q(PAGE),
            ),
            "get_commit_combined_status" => self.get(
                &format!("{}/commits/{}/status", a.repo()?, enc(&a.req("ref")?, true)),
                none,
            ),
            "list_check_runs_for_ref" => self.get(
                &format!(
                    "{}/commits/{}/check-runs",
                    a.repo()?,
                    enc(&a.req("ref")?, true)
                ),
                &a.q(&cat(&["check_name", "status", "filter"], PAGE)),
            ),
            "list_commits" => self.get(
                &format!("{}/commits", a.repo()?),
                &a.q(&cat(&["sha", "path", "author", "since", "until"], PAGE)),
            ),
            "compare_commits" => self.get(
                &format!(
                    "{}/compare/{}...{}",
                    a.repo()?,
                    enc(&a.req("base")?, true),
                    enc(&a.req("head")?, true)
                ),
                &a.q(PAGE),
            ),
            "get_file_contents" => self.rest(
                "GET",
                &format!(
                    "{}/contents/{}",
                    a.repo()?,
                    enc(a.req("path")?.trim_start_matches('/'), true)
                ),
                &a.q(&["ref"]),
                None,
                Some("application/vnd.github.raw"),
            ),
            "get_git_tree" => {
                let q = if a.b("recursive") == Some(true) {
                    vec![("recursive".to_string(), "1".to_string())]
                } else {
                    vec![]
                };
                self.get(
                    &format!("{}/git/trees/{}", a.repo()?, enc(&a.req("tree_sha")?, true)),
                    &q,
                )
            }
            "list_tags" => self.get(&format!("{}/tags", a.repo()?), &a.q(PAGE)),
            "get_tag" => {
                let r = self.get_json(
                    &format!("{}/git/ref/tags/{}", a.repo()?, enc(&a.req("tag")?, true)),
                    none,
                )?;
                let tag = if r["object"]["type"] == "tag" {
                    let sha = r["object"]["sha"].as_str().unwrap_or("");
                    self.get_json(&format!("{}/git/tags/{}", a.repo()?, enc(sha, false)), none)?
                } else {
                    Value::Null
                };
                Ok(json!({"ref": r, "tag": tag}).to_string())
            }
            "list_releases" => self.get(&format!("{}/releases", a.repo()?), &a.q(PAGE)),
            "get_latest_release" => self.get(&format!("{}/releases/latest", a.repo()?), none),
            "get_release_by_tag" => self.get(
                &format!("{}/releases/tags/{}", a.repo()?, a.seg("tag")?),
                none,
            ),
            "list_repository_collaborators" => self.get(
                &format!("{}/collaborators", a.repo()?),
                &a.q(&cat(&["affiliation", "permission"], PAGE)),
            ),
            "list_labels" => match a.i("issue_number") {
                Some(n) => self.get(&format!("{}/issues/{n}/labels", a.repo()?), &a.q(PAGE)),
                None => self.get(&format!("{}/labels", a.repo()?), &a.q(PAGE)),
            },
            "get_label" => self.get(&format!("{}/labels/{}", a.repo()?, a.seg("name")?), none),
            // ----- reads: issues -----
            "list_issues" => self.get(
                &format!("{}/issues", a.repo()?),
                &a.q(&cat(
                    &[
                        "state",
                        "labels",
                        "assignee",
                        "creator",
                        "mentioned",
                        "sort",
                        "direction",
                        "since",
                    ],
                    PAGE,
                )),
            ),
            "get_issue" => self.get(
                &format!("{}/issues/{}", a.repo()?, a.req("issue_number")?),
                none,
            ),
            "list_issue_comments" => self.get(
                &format!("{}/issues/{}/comments", a.repo()?, a.req("issue_number")?),
                &a.q(&cat(&["since"], PAGE)),
            ),
            "get_issue_comment" => self.get(
                &format!("{}/issues/comments/{}", a.repo()?, a.req("comment_id")?),
                none,
            ),
            "list_sub_issues" => self.get(
                &format!("{}/issues/{}/sub_issues", a.repo()?, a.req("issue_number")?),
                &a.q(PAGE),
            ),
            // ----- reads: pull requests -----
            "list_pull_requests" => self.get(
                &format!("{}/pulls", a.repo()?),
                &a.q(&cat(&["state", "head", "base", "sort", "direction"], PAGE)),
            ),
            "get_pull_request" => self.get(
                &format!("{}/pulls/{}", a.repo()?, a.req("pull_number")?),
                none,
            ),
            "get_pull_request_diff" => self.rest(
                "GET",
                &format!("{}/pulls/{}", a.repo()?, a.req("pull_number")?),
                none,
                None,
                Some("application/vnd.github.diff"),
            ),
            "list_pull_request_comments" => self.get(
                &format!("{}/issues/{}/comments", a.repo()?, a.req("pull_number")?),
                &a.q(PAGE),
            ),
            "list_pull_request_commits" => self.get(
                &format!("{}/pulls/{}/commits", a.repo()?, a.req("pull_number")?),
                &a.q(PAGE),
            ),
            "list_pull_request_files" => self.get(
                &format!("{}/pulls/{}/files", a.repo()?, a.req("pull_number")?),
                &a.q(PAGE),
            ),
            "list_pull_request_review_comments" => self.get(
                &format!("{}/pulls/{}/comments", a.repo()?, a.req("pull_number")?),
                &a.q(&cat(&["sort", "direction", "since"], PAGE)),
            ),
            "list_pull_request_reviews" => self.get(
                &format!("{}/pulls/{}/reviews", a.repo()?, a.req("pull_number")?),
                &a.q(PAGE),
            ),
            // ----- reads: actions -----
            "list_workflows" => self.get(&format!("{}/actions/workflows", a.repo()?), &a.q(PAGE)),
            "get_workflow" => self.get(
                &format!("{}/actions/workflows/{}", a.repo()?, a.seg("workflow_id")?),
                none,
            ),
            "list_workflow_runs" => {
                let base = match a.s("workflow_id") {
                    Some(w) => format!("{}/actions/workflows/{}/runs", a.repo()?, enc(&w, false)),
                    None => format!("{}/actions/runs", a.repo()?),
                };
                self.get(
                    &base,
                    &a.q(&cat(
                        &[
                            "actor",
                            "branch",
                            "event",
                            "status",
                            "created",
                            "head_sha",
                            "exclude_pull_requests",
                        ],
                        PAGE,
                    )),
                )
            }
            "get_workflow_run" => self.get(
                &format!("{}/actions/runs/{}", a.repo()?, a.req("run_id")?),
                none,
            ),
            "list_workflow_run_jobs" => self.get(
                &format!("{}/actions/runs/{}/jobs", a.repo()?, a.req("run_id")?),
                &a.q(&cat(&["filter"], PAGE)),
            ),
            "get_job" => self.get(
                &format!("{}/actions/jobs/{}", a.repo()?, a.req("job_id")?),
                none,
            ),
            "get_job_logs" => self.job_logs(a),
            // ----- reads: discussions (GraphQL) -----
            "list_discussion_categories" => {
                let d = self.graphql(
                    "query($o:String!,$r:String!){repository(owner:$o,name:$r){discussionCategories(first:100){nodes{id name slug emoji description isAnswerable}}}}",
                    json!({"o": a.req("owner")?, "r": a.req("repo")?}),
                )?;
                Ok(d["repository"]["discussionCategories"].to_string())
            }
            "list_discussions" => {
                let d = self.graphql(
                    "query($o:String!,$r:String!,$first:Int!,$after:String,$cat:ID,$ob:DiscussionOrder){repository(owner:$o,name:$r){discussions(first:$first,after:$after,categoryId:$cat,orderBy:$ob){totalCount pageInfo{hasNextPage endCursor} nodes{id number title url createdAt updatedAt author{login} category{id name} isAnswered comments{totalCount}}}}}",
                    json!({
                        "o": a.req("owner")?, "r": a.req("repo")?, "first": a.i("first").unwrap_or(30),
                        "after": a.s("after"), "cat": a.s("category_id"),
                        "ob": {"field": a.s("order_by").unwrap_or_else(|| "UPDATED_AT".into()), "direction": a.s("direction").unwrap_or_else(|| "DESC".into())},
                    }),
                )?;
                Ok(d["repository"]["discussions"].to_string())
            }
            "get_discussion" => {
                let d = self.graphql(
                    "query($o:String!,$r:String!,$n:Int!){repository(owner:$o,name:$r){discussion(number:$n){id number title body url createdAt updatedAt closed author{login} category{id name isAnswerable} labels(first:50){nodes{name}} answer{id body author{login} url} comments{totalCount} upvoteCount}}}",
                    json!({"o": a.req("owner")?, "r": a.req("repo")?, "n": a.i("discussion_number")}),
                )?;
                Ok(d["repository"]["discussion"].to_string())
            }
            "get_discussion_comments" => {
                let replies = if a.b("include_replies") == Some(true) {
                    " replies(first:100){totalCount nodes{id body createdAt author{login} url}}"
                } else {
                    ""
                };
                let q = format!(
                    "query($o:String!,$r:String!,$n:Int!,$first:Int!,$after:String){{repository(owner:$o,name:$r){{discussion(number:$n){{comments(first:$first,after:$after){{totalCount pageInfo{{hasNextPage endCursor}} nodes{{id body createdAt updatedAt isAnswer url author{{login}} upvoteCount{replies}}}}}}}}}}}"
                );
                let d = self.graphql(
                    &q,
                    json!({"o": a.req("owner")?, "r": a.req("repo")?, "n": a.i("discussion_number"), "first": a.i("first").unwrap_or(30), "after": a.s("after")}),
                )?;
                Ok(d["repository"]["discussion"]["comments"].to_string())
            }
            // ----- writes (only reached with --allow-writes) -----
            _ => self.write(name, a),
        }
    }

    fn job_logs(&self, a: &A) -> R<String> {
        let repo = a.repo()?;
        let tailn = a.i("tail_bytes").map(|n| n as usize);
        let one = |id: i64, t: Option<usize>| -> R<String> {
            let s = self.rest(
                "GET",
                &format!("{repo}/actions/jobs/{id}/logs"),
                &[],
                None,
                Some("*/*"),
            )?;
            Ok(match t {
                Some(n) => tail(&s, n),
                None => s,
            })
        };
        match (a.i("job_id"), a.i("run_id")) {
            (Some(_), Some(_)) => Err("give job_id or run_id, not both".into()),
            (None, None) => Err("give job_id (preferred) or run_id".into()),
            (Some(j), None) => one(j, tailn),
            (None, Some(r)) => {
                let jobs = self.get_json(
                    &format!("{repo}/actions/runs/{r}/jobs"),
                    &[
                        ("filter".into(), "latest".into()),
                        ("per_page".into(), "100".into()),
                    ],
                )?;
                let failed: Vec<&Value> = jobs["jobs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|j| {
                        matches!(
                            j["conclusion"].as_str(),
                            Some("failure") | Some("timed_out")
                        )
                    })
                    .take(3)
                    .collect();
                if failed.is_empty() {
                    return Ok(jobs.to_string());
                }
                let mut out = String::new();
                for j in failed {
                    let id = j["id"].as_i64().unwrap_or(0);
                    out.push_str(&format!(
                        "===== job {} (id {id}, {}) =====\n",
                        j["name"].as_str().unwrap_or("?"),
                        j["conclusion"].as_str().unwrap_or("?")
                    ));
                    match one(id, Some(tailn.unwrap_or(65_536))) {
                        Ok(s) => out.push_str(&s),
                        Err(e) => out.push_str(&format!("[logs unavailable: {e}]")),
                    }
                    out.push('\n');
                }
                Ok(out)
            }
        }
    }

    fn pending_review(&self, a: &A) -> R<i64> {
        if let Some(id) = a.i("review_id") {
            return Ok(id);
        }
        let v = self.get_json(
            &format!("{}/pulls/{}/reviews", a.repo()?, a.req("pull_number")?),
            &[("per_page".into(), "100".into())],
        )?;
        v.as_array()
            .into_iter()
            .flatten()
            .find(|r| r["state"] == "PENDING")
            .and_then(|r| r["id"].as_i64())
            .ok_or("no PENDING review of yours on this pull request".into())
    }

    fn write(&self, name: &str, a: &A) -> R<String> {
        let repo = a.repo()?;
        let none: &[(String, String)] = &[];
        let post = |p: String, b: Value| self.rest("POST", &p, none, Some(b), None);
        let patch = |p: String, b: Value| self.rest("PATCH", &p, none, Some(b), None);
        let put = |p: String, b: Value| self.rest("PUT", &p, none, Some(b), None);
        match name {
            "add_issue_comment" => post(
                format!("{repo}/issues/{}/comments", a.req("issue_number")?),
                a.body(&["body"]),
            ),
            "add_sub_issue" => post(
                format!("{repo}/issues/{}/sub_issues", a.req("issue_number")?),
                a.body(&["sub_issue_id", "replace_parent"]),
            ),
            "remove_sub_issue" => self.rest(
                "DELETE",
                &format!("{repo}/issues/{}/sub_issue", a.req("issue_number")?),
                none,
                Some(a.body(&["sub_issue_id"])),
                None,
            ),
            "reprioritize_sub_issue" => patch(
                format!(
                    "{repo}/issues/{}/sub_issues/priority",
                    a.req("issue_number")?
                ),
                a.body(&["sub_issue_id", "after_id", "before_id"]),
            ),
            "create_issue" => post(
                format!("{repo}/issues"),
                a.body(&["title", "body", "labels", "assignees", "milestone"]),
            ),
            "update_issue" => patch(
                format!("{repo}/issues/{}", a.req("issue_number")?),
                a.body(&[
                    "title",
                    "body",
                    "state",
                    "state_reason",
                    "labels",
                    "assignees",
                    "milestone",
                ]),
            ),
            "create_label" => post(
                format!("{repo}/labels"),
                a.body(&["name", "color", "description"]),
            ),
            "update_label" => patch(
                format!("{repo}/labels/{}", a.seg("name")?),
                a.body(&["new_name", "color", "description"]),
            ),
            "delete_label" => self.rest(
                "DELETE",
                &format!("{repo}/labels/{}", a.seg("name")?),
                none,
                None,
                None,
            ),
            "create_branch" => {
                let sha = match a.s("sha") {
                    Some(s) => s,
                    None => {
                        let from = match a.s("from_branch") {
                            Some(b) => b,
                            None => self.get_json(&repo, none)?["default_branch"]
                                .as_str()
                                .unwrap_or("main")
                                .to_string(),
                        };
                        let r = self.get_json(
                            &format!("{repo}/git/ref/heads/{}", enc(&from, true)),
                            none,
                        )?;
                        r["object"]["sha"]
                            .as_str()
                            .ok_or("cannot resolve from_branch")?
                            .to_string()
                    }
                };
                post(
                    format!("{repo}/git/refs"),
                    json!({"ref": format!("refs/heads/{}", a.req("branch")?), "sha": sha}),
                )
            }
            "create_pull_request" => post(
                format!("{repo}/pulls"),
                a.body(&[
                    "title",
                    "head",
                    "base",
                    "body",
                    "draft",
                    "maintainer_can_modify",
                ]),
            ),
            "update_pull_request" => patch(
                format!("{repo}/pulls/{}", a.req("pull_number")?),
                a.body(&["title", "body", "base", "state", "maintainer_can_modify"]),
            ),
            "update_pull_request_branch" => put(
                format!("{repo}/pulls/{}/update-branch", a.req("pull_number")?),
                a.body(&["expected_head_sha"]),
            ),
            "merge_pull_request" => put(
                format!("{repo}/pulls/{}/merge", a.req("pull_number")?),
                a.body(&["merge_method", "commit_title", "commit_message", "sha"]),
            ),
            "create_pull_request_review" => post(
                format!("{repo}/pulls/{}/reviews", a.req("pull_number")?),
                a.body(&["event", "body", "commit_id", "comments"]),
            ),
            "submit_pending_pull_request_review" => {
                let id = self.pending_review(a)?;
                post(
                    format!("{repo}/pulls/{}/reviews/{id}/events", a.req("pull_number")?),
                    a.body(&["event", "body"]),
                )
            }
            "delete_pending_pull_request_review" => {
                let id = self.pending_review(a)?;
                self.rest(
                    "DELETE",
                    &format!("{repo}/pulls/{}/reviews/{id}", a.req("pull_number")?),
                    none,
                    None,
                    None,
                )
            }
            "reply_to_pull_request_review_comment" => post(
                format!(
                    "{repo}/pulls/{}/comments/{}/replies",
                    a.req("pull_number")?,
                    a.req("comment_id")?
                ),
                a.body(&["body"]),
            ),
            "add_comment_to_pending_review" => {
                let id = self.pending_review(a)?;
                let rv = self.get_json(
                    &format!("{repo}/pulls/{}/reviews/{id}", a.req("pull_number")?),
                    none,
                )?;
                let node = rv["node_id"].as_str().ok_or("review has no node_id")?;
                let mut input = json!({"pullRequestReviewId": node, "path": a.req("path")?, "body": a.req("body")?});
                for (k, g) in [
                    ("line", "line"),
                    ("side", "side"),
                    ("start_line", "startLine"),
                    ("start_side", "startSide"),
                    ("subject_type", "subjectType"),
                ] {
                    if let Some(v) = a.0.get(k) {
                        input[g] = v.clone();
                    }
                }
                let d = self.graphql(
                    "mutation($i:AddPullRequestReviewThreadInput!){addPullRequestReviewThread(input:$i){thread{id path line isResolved comments(first:1){nodes{id databaseId body url}}}}}",
                    json!({"i": input}),
                )?;
                Ok(d["addPullRequestReviewThread"]["thread"].to_string())
            }
            "set_pull_request_draft" => {
                let d = self.graphql(
                    "query($o:String!,$r:String!,$n:Int!){repository(owner:$o,name:$r){pullRequest(number:$n){id number isDraft url title}}}",
                    json!({"o": a.req("owner")?, "r": a.req("repo")?, "n": a.i("pull_number")}),
                )?;
                let pr = &d["repository"]["pullRequest"];
                let want = a.b("draft").unwrap_or(false);
                if pr["isDraft"].as_bool() == Some(want) {
                    return Ok(pr.to_string());
                }
                let m = if want {
                    "convertPullRequestToDraft"
                } else {
                    "markPullRequestReadyForReview"
                };
                let q = format!("mutation($id:ID!){{{m}(input:{{pullRequestId:$id}}){{pullRequest{{id number isDraft url title}}}}}}");
                let d = self.graphql(&q, json!({"id": pr["id"]}))?;
                Ok(d[m]["pullRequest"].to_string())
            }
            "add_discussion_comment" => {
                let d = self.graphql(
                    "query($o:String!,$r:String!,$n:Int!){repository(owner:$o,name:$r){discussion(number:$n){id}}}",
                    json!({"o": a.req("owner")?, "r": a.req("repo")?, "n": a.i("discussion_number")}),
                )?;
                let id = d["repository"]["discussion"]["id"].clone();
                let d = self.graphql(
                    "mutation($d:ID!,$b:String!,$rt:ID){addDiscussionComment(input:{discussionId:$d,body:$b,replyToId:$rt}){comment{id body url}}}",
                    json!({"d": id, "b": a.req("body")?, "rt": a.s("reply_to_id")}),
                )?;
                Ok(d["addDiscussionComment"]["comment"].to_string())
            }
            "update_discussion_comment" => {
                let d = self.graphql(
                    "mutation($id:ID!,$b:String!){updateDiscussionComment(input:{commentId:$id,body:$b}){comment{id body url}}}",
                    json!({"id": a.req("comment_id")?, "b": a.req("body")?}),
                )?;
                Ok(d["updateDiscussionComment"]["comment"].to_string())
            }
            "delete_discussion_comment" => {
                let d = self.graphql(
                    "mutation($id:ID!){deleteDiscussionComment(input:{id:$id}){comment{id}}}",
                    json!({"id": a.req("comment_id")?}),
                )?;
                Ok(d["deleteDiscussionComment"]["comment"].to_string())
            }
            "mark_discussion_comment_as_answer" => {
                let m = if a.b("answer") == Some(true) {
                    "markDiscussionCommentAsAnswer"
                } else {
                    "unmarkDiscussionCommentAsAnswer"
                };
                let q = format!("mutation($id:ID!){{{m}(input:{{id:$id}}){{discussion{{id number isAnswered}}}}}}");
                let d = self.graphql(&q, json!({"id": a.req("comment_id")?}))?;
                Ok(d[m]["discussion"].to_string())
            }
            other => Err(format!("unknown tool {other}")),
        }
    }

    /// Handle one JSON-RPC message; `None` for notifications.
    pub fn handle(&self, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned()?;
        let method = msg["method"].as_str().unwrap_or("");
        let result = match method {
            "initialize" => json!({
                "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or("2025-03-26"),
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "github-mcp-rs", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "GitHub tools with the Grok Bot interface. Writes are refused unless the server runs with --allow-writes.",
            }),
            "ping" => json!({}),
            "tools/list" => tools_list(),
            "tools/call" => {
                let name = msg["params"]["name"].as_str().unwrap_or("");
                let (text, err) = match self.call(name, &msg["params"]["arguments"]) {
                    Ok(t) => (t, false),
                    Err(e) => (e, true),
                };
                json!({"content": [{"type": "text", "text": text}], "isError": err})
            }
            _ => {
                return Some(
                    json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("method not found: {method}")}}),
                );
            }
        };
        Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_has_75_tools_and_the_split() {
        let t = reference_tools();
        assert_eq!(t.len(), 75);
        let reads = t
            .iter()
            .filter(|x| is_read(x["name"].as_str().unwrap()))
            .count();
        assert_eq!((reads, 75 - reads), (51, 24));
    }

    #[test]
    fn validation_follows_the_schema() {
        assert!(validate(
            "get_issue",
            &json!({"owner":"o","repo":"r","issue_number":1})
        )
        .is_ok());
        assert!(validate("get_issue", &json!({"owner":"o","repo":"r"}))
            .unwrap_err()
            .contains("issue_number"));
        assert!(validate(
            "get_issue",
            &json!({"owner":"o","repo":"r","issue_number":0})
        )
        .is_err());
        assert!(validate(
            "get_issue",
            &json!({"owner":"o","repo":"r","issue_number":1,"x":1})
        )
        .unwrap_err()
        .contains("unknown"));
        assert!(validate(
            "list_issues",
            &json!({"owner":"o","repo":"r","state":"weird"})
        )
        .is_err());
        assert!(validate(
            "get_workflow",
            &json!({"owner":"o","repo":"r","workflow_id":"ci.yml"})
        )
        .is_ok());
        assert!(validate(
            "get_workflow",
            &json!({"owner":"o","repo":"r","workflow_id":7})
        )
        .is_ok());
        assert!(validate(
            "create_label",
            &json!({"owner":"o","repo":"r","name":"x","color":"zz0000"})
        )
        .is_err());
        assert!(validate(
            "update_issue",
            &json!({"owner":"o","repo":"r","issue_number":1,"milestone":null})
        )
        .is_ok());
        assert!(validate("get_me", &json!({})).is_ok());
    }

    #[test]
    fn writes_are_refused_without_allow_writes_before_any_network() {
        let s = Server {
            curl: "/nonexistent/curl".into(),
            ..Default::default()
        };
        let e = s
            .call(
                "merge_pull_request",
                &json!({"owner":"o","repo":"r","pull_number":1}),
            )
            .unwrap_err();
        assert!(e.contains("writes are OFF"), "{e}");
    }

    #[test]
    fn missing_token_waits_and_names_only_the_file() {
        let s = Server {
            token_file: Some("/nonexistent/github.env".into()),
            ..Default::default()
        };
        std::env::remove_var(TOKEN_ENV);
        let e = s.call("get_me", &json!({})).unwrap_err();
        assert!(
            e.contains("not configured yet") && e.contains("/nonexistent/github.env"),
            "{e}"
        );
    }

    #[test]
    fn encoding_and_tail() {
        assert_eq!(enc("feat/x y", true), "feat/x%20y");
        assert_eq!(enc("a/b", false), "a%2Fb");
        assert!(tail("abcdef", 2).ends_with("ef"));
        assert!(!loopback("http://example.com"));
        assert!(loopback("http://127.0.0.1:9/x"));
    }
}
