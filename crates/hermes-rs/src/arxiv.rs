// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! `arxiv_search`: Rust port of the `research/arxiv` skill's
//! `scripts/search_arxiv.py` (hermes-agent, MIT). Queries the public arXiv
//! Atom API through the system `curl` (no key, GET only) and returns the same
//! fields the script printed: title, id+version, dates, authors, categories,
//! abstract (300 chars) and links.

use crate::tools::{Tool, ToolCtx, ToolOutput};
use serde_json::{json, Value};
use std::process::Command;

pub const API: &str = "https://export.arxiv.org/api/query";

fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.trim().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                o.push(b as char)
            }
            b' ' => o.push('+'),
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

/// Build the query URL. Mirrors search_arxiv.py: id_list wins, else
/// all:/au:/cat: joined with +AND+; sort relevance|date|updated.
pub fn build_url(args: &Value) -> Result<String, String> {
    let max = args["max_results"].as_u64().unwrap_or(5).clamp(1, 20);
    let sort = match args["sort"].as_str().unwrap_or("relevance") {
        "date" | "submittedDate" => "submittedDate",
        "updated" | "lastUpdatedDate" => "lastUpdatedDate",
        _ => "relevance",
    };
    let mut q = String::new();
    if let Some(ids) = args["ids"].as_str().filter(|s| !s.trim().is_empty()) {
        if !ids
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".,/-".contains(c))
        {
            return Err("ids: only arXiv ids like 2402.03300,2401.12345".into());
        }
        q.push_str(&format!("id_list={}", ids.trim()));
    } else {
        let mut parts = Vec::new();
        if let Some(x) = args["query"].as_str().filter(|s| !s.trim().is_empty()) {
            parts.push(format!("all:{}", enc(x)));
        }
        if let Some(x) = args["author"].as_str().filter(|s| !s.trim().is_empty()) {
            parts.push(format!("au:{}", enc(x)));
        }
        if let Some(x) = args["category"].as_str().filter(|s| !s.trim().is_empty()) {
            if !x
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || ".-".contains(c))
            {
                return Err("category: e.g. cs.AI".into());
            }
            parts.push(format!("cat:{x}"));
        }
        if parts.is_empty() {
            return Err("provide query, author, category or ids".into());
        }
        q.push_str(&format!("search_query={}", parts.join("+AND+")));
    }
    Ok(format!(
        "{API}?{q}&max_results={max}&sortBy={sort}&sortOrder=descending"
    ))
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}");
    let start = xml.find(&open)?;
    let after = &xml[start + open.len()..];
    let gt = after.find('>')?;
    if after[..gt].ends_with('/') {
        return Some("");
    }
    let body = &after[gt + 1..];
    let end = body.find(&format!("</{name}>"))?;
    Some(&body[..end])
}

fn all<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut rest = xml;
    let close = format!("</{name}>");
    while let Some(t) = tag(rest, name) {
        out.push(t);
        let Some(p) = rest.find(&close) else { break };
        rest = &rest[p + close.len()..];
    }
    out
}

fn clean(s: &str) -> String {
    unescape(&s.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Parse the Atom feed into result objects.
pub fn parse(xml: &str) -> Value {
    let total = tag(xml, "opensearch:totalResults").map(|t| t.trim().to_string());
    let mut results = Vec::new();
    for e in all(xml, "entry") {
        let raw_id = clean(tag(e, "id").unwrap_or(""));
        let full = raw_id.rsplit("/abs/").next().unwrap_or(&raw_id).to_string();
        let base = match full.rfind('v') {
            Some(i) if full[i + 1..].chars().all(|c| c.is_ascii_digit()) && i + 1 < full.len() => {
                full[..i].to_string()
            }
            _ => full.clone(),
        };
        let summary = clean(tag(e, "summary").unwrap_or(""));
        let abs: String = if summary.chars().count() > 300 {
            format!("{}...", summary.chars().take(300).collect::<String>())
        } else {
            summary
        };
        let authors: Vec<String> = all(e, "author")
            .into_iter()
            .filter_map(|a| tag(a, "name").map(clean))
            .collect();
        let mut cats = Vec::new();
        let mut rest = e;
        while let Some(p) = rest.find("<category") {
            rest = &rest[p + 9..];
            if let Some(t) = rest.find("term=\"") {
                let v = &rest[t + 6..];
                if let Some(q) = v.find('"') {
                    cats.push(unescape(&v[..q]));
                }
            }
        }
        let d = |n: &str| {
            clean(tag(e, n).unwrap_or(""))
                .chars()
                .take(10)
                .collect::<String>()
        };
        results.push(json!({
            "title": clean(tag(e, "title").unwrap_or("")),
            "id": full, "published": d("published"), "updated": d("updated"),
            "authors": authors, "categories": cats, "abstract": abs,
            "abs_url": format!("https://arxiv.org/abs/{base}"),
            "pdf_url": format!("https://arxiv.org/pdf/{base}"),
        }));
    }
    json!({"total_results": total, "count": results.len(), "results": results})
}

pub struct ArxivTool {
    pub curl: String,
}

impl Default for ArxivTool {
    fn default() -> Self {
        ArxivTool {
            curl: "curl".into(),
        }
    }
}

impl Tool for ArxivTool {
    fn name(&self) -> String {
        "arxiv_search".into()
    }
    fn description(&self) -> String {
        "Search arXiv papers (public API, read-only) by keyword, author, category or id. Returns title, id, dates, authors, categories, abstract and links. Port of the arxiv skill's script.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "query": {"type": "string", "description": "keywords"},
            "author": {"type": "string"},
            "category": {"type": "string", "description": "e.g. cs.AI"},
            "ids": {"type": "string", "description": "comma-separated arXiv ids, e.g. 2402.03300"},
            "max_results": {"type": "integer", "minimum": 1, "maximum": 20},
            "sort": {"type": "string", "enum": ["relevance", "date", "updated"]}
        }})
    }
    fn call(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let url = match build_url(args) {
            Ok(u) => u,
            Err(e) => return ToolOutput::err(e),
        };
        let out = Command::new(&self.curl)
            .args([
                "-sS",
                "-L",
                "--proto",
                "=https",
                "--max-time",
                "20",
                "--max-filesize",
                "4000000",
                "-A",
                "HermesAgent/1.0 (hermes-rs)",
                &url,
            ])
            .output();
        match out {
            Ok(o) if o.status.success() => {
                let xml = String::from_utf8_lossy(&o.stdout);
                if !xml.contains("<feed") {
                    return ToolOutput::err("arXiv returned no Atom feed");
                }
                ToolOutput::ok(parse(&xml).to_string())
            }
            Ok(o) => ToolOutput::err(format!(
                "arXiv request failed: {}",
                crate::redact(String::from_utf8_lossy(&o.stderr).trim())
            )),
            Err(e) => ToolOutput::err(format!("curl not runnable: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <opensearch:totalResults xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/">1</opensearch:totalResults>
  <entry>
    <id>http://arxiv.org/abs/2402.03300v3</id>
    <updated>2024-04-27T10:00:00Z</updated>
    <published>2024-02-05T18:00:00Z</published>
    <title>DeepSeekMath: Pushing the Limits
      of Mathematical Reasoning &amp; More</title>
    <summary>  We introduce GRPO.  </summary>
    <author><name>Zhihong Shao</name></author>
    <author><name>Peiyi Wang</name></author>
    <category term="cs.CL" scheme="http://arxiv.org/schemas/atom"/>
    <category term="cs.AI" scheme="http://arxiv.org/schemas/atom"/>
  </entry>
</feed>"#;

    #[test]
    fn parses_feed_like_the_script() {
        let v = parse(FEED);
        assert_eq!(v["total_results"], "1");
        let r = &v["results"][0];
        assert_eq!(
            r["title"],
            "DeepSeekMath: Pushing the Limits of Mathematical Reasoning & More"
        );
        assert_eq!(r["id"], "2402.03300v3");
        assert_eq!(r["published"], "2024-02-05");
        assert_eq!(r["updated"], "2024-04-27");
        assert_eq!(r["authors"], json!(["Zhihong Shao", "Peiyi Wang"]));
        assert_eq!(r["categories"], json!(["cs.CL", "cs.AI"]));
        assert_eq!(r["abstract"], "We introduce GRPO.");
        assert_eq!(r["abs_url"], "https://arxiv.org/abs/2402.03300");
        assert_eq!(parse("<feed></feed>")["count"], 0);
    }

    #[test]
    fn builds_urls_and_validates() {
        let u =
            build_url(&json!({"query": "GRPO reinforcement", "sort": "date", "max_results": 50}))
                .unwrap();
        assert_eq!(u, format!("{API}?search_query=all:GRPO+reinforcement&max_results=20&sortBy=submittedDate&sortOrder=descending"));
        let u = build_url(&json!({"author": "Yann LeCun", "category": "cs.AI"})).unwrap();
        assert!(u.contains("search_query=au:Yann+LeCun+AND+cat:cs.AI"));
        let u = build_url(&json!({"ids": "2402.03300,2401.12345"})).unwrap();
        assert!(u.contains("id_list=2402.03300,2401.12345"));
        assert!(build_url(&json!({})).is_err());
        assert!(build_url(&json!({"ids": "x&y=1"})).is_err());
        assert!(build_url(&json!({"category": "cs AI;"})).is_err());
        assert!(build_url(&json!({"query": "a&b"}))
            .unwrap()
            .contains("all:a%26b"));
    }
}
