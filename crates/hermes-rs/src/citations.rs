// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! `citations`: Rust port of the `research/grounded-citations` skill's
//! `scripts/sources.py` ledger (hermes-agent, MIT). One ledger per session
//! (`<dir>/<session>.json`): URLs get stable ids `[n]` at retrieval time; a
//! quote is accepted only if it appears verbatim in the page text given with
//! it; `verify` checks a draft's `[n]` markers against the ledger, so a
//! citation the model invented is caught. Actions: reset, add, quote, list,
//! render, verify.

use crate::tools::{Tool, ToolCtx, ToolOutput};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub fn normalize_url(u: &str) -> String {
    let u = u.trim();
    let u = u.split('#').next().unwrap_or(u);
    let s = u.trim_end_matches('/');
    if s.is_empty() {
        u.to_string()
    } else {
        s.to_string()
    }
}

fn ws(t: &str) -> String {
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whitespace-, case- and markdown-insensitive key (links -> label, *_`~ dropped).
pub fn match_key(t: &str) -> String {
    let mut out = String::new();
    let c: Vec<char> = t.chars().collect();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '[' {
            if let Some(close) = c[i + 1..].iter().position(|&x| x == ']').map(|p| p + i + 1) {
                if c.get(close + 1) == Some(&'(') {
                    if let Some(rp) = c[close + 2..]
                        .iter()
                        .position(|&x| x == ')')
                        .map(|p| p + close + 2)
                    {
                        out.extend(&c[i + 1..close]);
                        i = rp + 1;
                        continue;
                    }
                }
            }
        }
        if "*_`~".contains(c[i]) {
            i += 1;
            continue;
        }
        if c[i] == '\\'
            && c.get(i + 1)
                .map(|n| !n.is_alphanumeric() && !n.is_whitespace())
                .unwrap_or(false)
        {
            i += 1;
            continue;
        }
        out.push(c[i]);
        i += 1;
    }
    ws(&out).to_lowercase()
}

pub fn quote_in_evidence(q: &str, ev: &str) -> bool {
    let k = match_key(q);
    !k.is_empty() && match_key(ev).contains(&k)
}

/// `[12]` markers not followed by `(` or `:`.
pub fn cites(text: &str) -> Vec<u32> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'[' {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_digit() && j - i <= 4 {
                j += 1;
            }
            if j > i + 1
                && j < b.len()
                && b[j] == b']'
                && !matches!(b.get(j + 1), Some(b'(') | Some(b':'))
            {
                if let Ok(n) = text[i + 1..j].parse() {
                    out.push(n);
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn is_sources_header(l: &str) -> bool {
    let t = l
        .trim()
        .trim_start_matches('#')
        .trim()
        .trim_matches('*')
        .trim()
        .to_lowercase();
    t == "sources" || t == "sources:"
}

/// (prose without fences, listed `[n] url` lines after the last Sources header)
fn split_draft(text: &str) -> (String, BTreeMap<u32, String>) {
    let lines: Vec<&str> = text.lines().collect();
    let hdr = lines.iter().rposition(|l| is_sources_header(l));
    let mut listed = BTreeMap::new();
    let body = match hdr {
        Some(h) => {
            for l in &lines[h + 1..] {
                let t = l.trim();
                if let Some(rest) = t.strip_prefix('[') {
                    if let Some(e) = rest.find(']') {
                        if let Ok(n) = rest[..e].parse::<u32>() {
                            let after = &rest[e + 1..];
                            let url = after
                                .split_whitespace()
                                .find(|w| w.starts_with("http://") || w.starts_with("https://"))
                                .or_else(|| {
                                    after
                                        .trim_start_matches([' ', '-', '–', ':'])
                                        .split_whitespace()
                                        .next()
                                })
                                .unwrap_or("");
                            listed.insert(
                                n,
                                url.trim_end_matches([')', ']', '}', '"', '\'']).to_string(),
                            );
                        }
                    }
                }
            }
            &lines[..h]
        }
        None => &lines[..],
    };
    let mut prose = Vec::new();
    let mut fence = false;
    for l in body {
        let t = l.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if !fence {
            prose.push(*l);
        }
    }
    (prose.join("\n"), listed)
}

fn sentences(prose: &str) -> Vec<String> {
    let mut out = Vec::new();
    for l in prose.lines() {
        let mut s = l.trim();
        if s.is_empty() || s.starts_with('#') || s.starts_with('|') {
            continue;
        }
        if s.starts_with('>') {
            s = s.trim_start_matches(['>', ' ']).trim();
        }
        let mut cur = String::new();
        let cs: Vec<char> = s.chars().collect();
        for (i, ch) in cs.iter().enumerate() {
            cur.push(*ch);
            if ".!?".contains(*ch) && cs.get(i + 1).map(|n| n.is_whitespace()).unwrap_or(false) {
                if cur.split_whitespace().count() >= 4 {
                    out.push(cur.trim().to_string());
                }
                cur.clear();
            }
        }
        if cur.split_whitespace().count() >= 4 {
            out.push(cur.trim().to_string());
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct Ledger {
    pub path: PathBuf,
    pub sources: Vec<Value>,
}

impl Ledger {
    pub fn open(path: PathBuf) -> Ledger {
        let sources = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v["sources"].as_array().cloned())
            .unwrap_or_default();
        Ledger { path, sources }
    }
    pub fn save(&self) -> Result<(), String> {
        if let Some(d) = self.path.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("ledger dir: {e}"))?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            json!({"version": 1, "sources": self.sources}).to_string(),
        )
        .map_err(|e| format!("ledger write: {e}"))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("ledger write: {e}"))
    }
    pub fn add(&mut self, urls: &[String], title: Option<&str>, day: &str) -> Vec<Value> {
        let mut out = Vec::new();
        for raw in urls.iter().map(|u| u.trim()).filter(|u| !u.is_empty()) {
            let key = normalize_url(raw);
            if let Some(e) = self.sources.iter_mut().find(|s| s["url"] == key.as_str()) {
                if let Some(t) = title {
                    if e["title"].as_str().unwrap_or("").is_empty() {
                        e["title"] = json!(t.trim());
                    }
                }
                out.push(e.clone());
                continue;
            }
            let e = json!({"id": self.sources.len() + 1, "url": key, "title": title.unwrap_or("").trim(), "accessed": day});
            self.sources.push(e.clone());
            out.push(e);
        }
        out
    }
    pub fn quote(
        &mut self,
        id: u64,
        quote: &str,
        evidence: &str,
        day: &str,
    ) -> Result<Value, String> {
        let q = quote.trim();
        if ws(q).split(' ').count() < 3 {
            return Err("quote too short: use at least 3 words of verbatim text".into());
        }
        if !quote_in_evidence(q, evidence) {
            return Err("quote not found verbatim in the evidence text: copy the exact wording from the fetched page, do not paraphrase".into());
        }
        let Some(e) = self
            .sources
            .iter_mut()
            .find(|s| s["id"].as_u64() == Some(id))
        else {
            return Err(format!("no source [{id}] in the ledger"));
        };
        if !e["quotes"].is_array() {
            e["quotes"] = json!([]);
        }
        let k = match_key(q);
        let qs = e["quotes"].as_array_mut().expect("array");
        if !qs
            .iter()
            .any(|x| match_key(x["text"].as_str().unwrap_or("")) == k)
        {
            qs.push(json!({"text": q, "added": day}));
        }
        Ok(e.clone())
    }
    pub fn render(&self, style: &str, only: Option<&BTreeSet<u32>>) -> String {
        let mut picked: Vec<&Value> = self
            .sources
            .iter()
            .filter(|s| {
                only.map(|o| o.contains(&(s["id"].as_u64().unwrap_or(0) as u32)))
                    .unwrap_or(true)
            })
            .collect();
        picked.sort_by_key(|s| s["id"].as_u64());
        if picked.is_empty() {
            return String::new();
        }
        let mut lines = Vec::new();
        let t = |s: &Value| s["title"].as_str().unwrap_or("").to_string();
        match style {
            "footnotes" => {
                for s in picked {
                    let tt = t(s);
                    lines.push(format!(
                        "[^{}]: {}{}",
                        s["id"],
                        s["url"].as_str().unwrap_or(""),
                        if tt.is_empty() {
                            String::new()
                        } else {
                            format!(" — {tt}")
                        }
                    ));
                }
            }
            _ => {
                lines.push(if style == "plain" {
                    "Sources:".into()
                } else {
                    "## Sources".into()
                });
                if style != "plain" {
                    lines.push(String::new());
                }
                for s in picked {
                    let tt = t(s);
                    lines.push(format!(
                        "[{}] {}{}",
                        s["id"],
                        s["url"].as_str().unwrap_or(""),
                        if tt.is_empty() {
                            String::new()
                        } else {
                            format!(" — {tt}")
                        }
                    ));
                    if style == "evidence" {
                        for q in s["quotes"].as_array().into_iter().flatten() {
                            lines.push(format!("    > \"{}\"", q["text"].as_str().unwrap_or("")));
                        }
                    }
                }
            }
        }
        lines.join("\n")
    }
    /// (ok, errors, warnings) like sources.py verify_draft.
    pub fn verify(
        &self,
        draft: &str,
        min_coverage: Option<f64>,
        require_evidence: bool,
    ) -> (bool, Vec<String>, Vec<String>) {
        let (prose, listed) = split_draft(draft);
        let by_id: BTreeMap<u32, &Value> = self
            .sources
            .iter()
            .map(|s| (s["id"].as_u64().unwrap_or(0) as u32, s))
            .collect();
        let cited: BTreeSet<u32> = cites(&prose).into_iter().collect();
        let fmt = |v: &[u32]| {
            v.iter()
                .map(|i| format!("[{i}]"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let unknown: Vec<u32> = cited
            .iter()
            .copied()
            .filter(|i| !by_id.contains_key(i))
            .collect();
        if !unknown.is_empty() {
            errors.push(format!(
                "citations not in the ledger (hallucinated or renumbered): {}",
                fmt(&unknown)
            ));
        }
        if !cited.is_empty() && listed.is_empty() {
            errors.push("draft cites sources but has no Sources block: run render".into());
        }
        if !listed.is_empty() {
            let miss: Vec<u32> = cited
                .iter()
                .copied()
                .filter(|i| !listed.contains_key(i))
                .collect();
            if !miss.is_empty() {
                errors.push(format!(
                    "cited but absent from the Sources block: {}",
                    fmt(&miss)
                ));
            }
        }
        for (sid, url) in &listed {
            match by_id.get(sid) {
                None => errors.push(format!("Sources block lists [{sid}], which is not in the ledger")),
                Some(e) if normalize_url(url) != e["url"].as_str().unwrap_or("") => errors.push(format!(
                    "Sources block URL for [{sid}] does not match the ledger (block: {url} / ledger: {}): re-run render",
                    e["url"].as_str().unwrap_or("")
                )),
                _ => {}
            }
        }
        let extra: Vec<u32> = listed
            .keys()
            .copied()
            .filter(|i| !cited.contains(i))
            .collect();
        if !extra.is_empty() {
            warnings.push(format!(
                "listed in Sources but never cited inline: {}",
                fmt(&extra)
            ));
        }
        let sents = sentences(&prose);
        let unv = |s: &str| s.to_lowercase().contains("[unverified]");
        let covered = sents
            .iter()
            .filter(|s| !cites(s).is_empty() || unv(s))
            .count();
        let cov = if sents.is_empty() {
            0.0
        } else {
            covered as f64 / sents.len() as f64
        };
        if let Some(m) = min_coverage {
            if !sents.is_empty() && cov < m {
                errors.push(format!("citation coverage {:.0}% is below the required {:.0}% ({covered}/{} sentences cited or marked [unverified])", cov * 100.0, m * 100.0, sents.len()));
            }
        }
        if require_evidence {
            let un: Vec<u32> = cited
                .iter()
                .copied()
                .filter(|i| {
                    by_id
                        .get(i)
                        .map(|e| e["quotes"].as_array().map(|a| a.is_empty()).unwrap_or(true))
                        .unwrap_or(false)
                })
                .collect();
            if !un.is_empty() {
                errors.push(format!("cited sources carry no verbatim evidence quote (run quote with the fetched page text): {}", fmt(&un)));
            }
        }
        warnings.insert(0, format!("stats: {} prose sentence(s), {covered} with declared provenance ({:.0}%); {} distinct source(s) cited, {} in ledger", sents.len(), cov * 100.0, cited.len(), by_id.len()));
        (errors.is_empty(), errors, warnings)
    }
}

pub struct CitationsTool {
    pub dir: PathBuf,
}

fn session_file(sid: &str) -> String {
    let s: String = sid
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(96)
        .collect();
    if s.is_empty() {
        "default".into()
    } else {
        s
    }
}

impl Tool for CitationsTool {
    fn name(&self) -> String {
        "citations".into()
    }
    fn description(&self) -> String {
        "Citation ledger for grounded answers (port of the grounded-citations skill). action=add (urls, title) gives stable ids [n]; quote (id, quote, evidence) accepts only text found verbatim in the page text; list; render (style markdown|plain|footnotes|evidence); verify (draft) catches invented or mismatched [n]; reset. One ledger per chat session.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "action": {"type": "string", "enum": ["add", "quote", "list", "render", "verify", "reset"]},
            "urls": {"type": "array", "items": {"type": "string"}},
            "title": {"type": "string"},
            "id": {"type": "integer"},
            "quote": {"type": "string"},
            "evidence": {"type": "string", "description": "fetched page text the quote must appear in"},
            "style": {"type": "string", "enum": ["markdown", "plain", "footnotes", "evidence"]},
            "draft": {"type": "string"},
            "min_coverage": {"type": "number"},
            "require_evidence": {"type": "boolean"}
        }, "required": ["action"]})
    }
    fn call(&self, a: &Value, ctx: &ToolCtx) -> ToolOutput {
        let path = self
            .dir
            .join(format!("{}.json", session_file(ctx.spend_session)));
        let mut l = Ledger::open(path);
        let day = crate::time::zagreb_day(crate::time::now_secs());
        let save = |l: &Ledger, v: Value| match l.save() {
            Ok(()) => ToolOutput::ok(v.to_string()),
            Err(e) => ToolOutput::err(e),
        };
        match a["action"].as_str().unwrap_or("") {
            "reset" => {
                l.sources.clear();
                save(&l, json!({"ok": true, "sources": 0}))
            }
            "add" => {
                let urls: Vec<String> = a["urls"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|u| u.as_str().map(str::to_string))
                    .collect();
                if urls.is_empty() {
                    return ToolOutput::err("urls is required");
                }
                if urls
                    .iter()
                    .any(|u| !(u.starts_with("http://") || u.starts_with("https://")))
                {
                    return ToolOutput::err("urls must be http(s)");
                }
                let out = l.add(&urls, a["title"].as_str(), &day);
                save(&l, json!({"added": out}))
            }
            "quote" => match l.quote(
                a["id"].as_u64().unwrap_or(0),
                a["quote"].as_str().unwrap_or(""),
                a["evidence"].as_str().unwrap_or(""),
                &day,
            ) {
                Ok(e) => save(&l, json!({"source": e})),
                Err(e) => ToolOutput::err(e),
            },
            "list" => ToolOutput::ok(json!({"sources": l.sources}).to_string()),
            "render" => ToolOutput::ok(l.render(a["style"].as_str().unwrap_or("markdown"), None)),
            "verify" => {
                let Some(d) = a["draft"].as_str() else {
                    return ToolOutput::err("draft is required");
                };
                let (ok, errors, warnings) = l.verify(
                    d,
                    a["min_coverage"].as_f64(),
                    a["require_evidence"].as_bool().unwrap_or(false),
                );
                ToolOutput::ok(
                    json!({"ok": ok, "errors": errors, "warnings": warnings}).to_string(),
                )
            }
            _ => ToolOutput::err("action must be add, quote, list, render, verify or reset"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn led() -> Ledger {
        let p = std::env::temp_dir()
            .join(crate::new_id("hermes-rs-cite-"))
            .join("s.json");
        Ledger::open(p)
    }

    #[test]
    fn ids_are_stable_and_urls_normalized() {
        let mut l = led();
        let a = l.add(
            &["https://x.org/page/".into(), "https://y.org/a#s".into()],
            Some("X"),
            "2026-10-29",
        );
        assert_eq!(a[0]["id"], 1);
        let b = l.add(&["https://x.org/page#top".into()], None, "2026-10-29");
        assert_eq!(b[0]["id"], 1);
        assert_eq!(l.sources.len(), 2);
        assert_eq!(l.sources[1]["url"], "https://y.org/a");
        l.save().unwrap();
        assert_eq!(Ledger::open(l.path.clone()).sources.len(), 2);
        let _ = std::fs::remove_dir_all(l.path.parent().unwrap());
    }

    #[test]
    fn quotes_must_be_verbatim() {
        let mut l = led();
        l.add(&["https://x.org".into()], None, "d");
        let page = "The **GRPO** method, see [paper](https://a.b), cuts   memory use by half.";
        assert!(l
            .quote(1, "GRPO method, see paper, cuts memory", page, "d")
            .is_ok());
        assert!(l.quote(1, "GRPO doubles memory use", page, "d").is_err());
        assert!(l.quote(1, "two words", page, "d").is_err());
        assert!(l.quote(9, "cuts memory use", page, "d").is_err());
        assert!(l.render("evidence", None).contains("> \"GRPO method"));
    }

    #[test]
    fn verify_catches_invented_and_mismatched() {
        let mut l = led();
        l.add(
            &["https://x.org/a".into(), "https://y.org/b".into()],
            None,
            "d",
        );
        let good = format!(
            "GRPO cuts memory use by half [1].\nThe model is quite old [unverified].\n\n{}",
            l.render("markdown", Some(&[1u32].into_iter().collect()))
        );
        let (ok, e, _) = l.verify(&good, Some(1.0), false);
        assert!(ok, "{e:?}");
        let bad = "GRPO cuts memory use by half [7].\n\n## Sources\n[7] https://z.org\n[1] https://wrong.org";
        let (ok, e, _) = l.verify(bad, None, false);
        assert!(!ok);
        assert!(e.iter().any(|x| x.contains("hallucinated")));
        assert!(e.iter().any(|x| x.contains("does not match")));
        let (ok, e, _) = l.verify("A claim with four words [1].", None, false);
        assert!(!ok && e[0].contains("no Sources block"));
        let (ok, _, _) = l.verify(&good, None, true);
        assert!(!ok, "evidence required but no quotes");
        let (ok, e, _) = l.verify(
            "One two three four. Five six seven eight [1].\n## Sources\n[1] https://x.org/a",
            Some(0.9),
            false,
        );
        assert!(!ok && e[0].contains("coverage"));
        assert_eq!(
            cites("see [3] and [x](u) and [4](http) and [12]: ref and [5]"),
            vec![3, 5]
        );
    }
}
