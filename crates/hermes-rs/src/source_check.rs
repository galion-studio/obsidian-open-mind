// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! `source_check`: verify that a cited source exists and says what a card
//! claims, before the card counts. Same shape as `arxiv_search` (system
//! `curl`, GET only, no key, size and time caps).
//!
//! - `doi`: looks the DOI up in Crossref (`api.crossref.org/works/<doi>`),
//!   falling back to DataCite (`api.datacite.org/dois/<doi>`), and returns
//!   title, authors, venue, publisher, issued date, type and landing URL.
//!   Optional `expect_title` reports whether the registered title matches.
//! - `url`: fetches one https page (redirects followed, 4 MB cap) and returns
//!   status, final URL, content type, page title and a text excerpt.
//!   Optional `expect` phrases are searched case- and whitespace-insensitively
//!   in the page text, so a claim can be checked against the page.
//!
//! Policy: https only; IP-literal hosts and localhost refused; Wikipedia and
//! anything derived from it (Wikimedia, Wikidata, DBpedia, Wikiwand, ...)
//! refused, because the knowledge base must not cite them.

use crate::tools::{Tool, ToolCtx, ToolOutput};
use serde_json::{json, Value};
use std::process::Command;

pub const CROSSREF: &str = "https://api.crossref.org/works/";
pub const DATACITE: &str = "https://api.datacite.org/dois/";
const UA: &str = "GalionResearch/1.0 (hermes-rs source_check; mailto:research@galion.app)";
const MAX_TEXT: usize = 6_000;

/// Hosts (and their subdomains) that may never be cited.
pub const BLOCKED_HOSTS: &[&str] = &[
    "wikipedia.org",
    "wikimedia.org",
    "wikidata.org",
    "wikiwand.com",
    "dbpedia.org",
    "wikibooks.org",
    "wikisource.org",
    "wiktionary.org",
    "wikiversity.org",
    "wikinews.org",
    "wikiquote.org",
    "wikivoyage.org",
    "everybodywiki.com",
    "wiki2.org",
    "justapedia.org",
];

/// Host of an https URL, lower-cased, or an error saying why it is refused.
pub fn check_url(u: &str) -> Result<String, String> {
    let u = u.trim();
    let rest = u
        .strip_prefix("https://")
        .ok_or_else(|| "only https:// URLs are fetched".to_string())?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return Err("URLs with credentials are refused".into());
    }
    let host = authority
        .rsplit_once(':')
        .filter(|(_, p)| p.chars().all(|c| c.is_ascii_digit()))
        .map(|(h, _)| h)
        .unwrap_or(authority)
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() || !host.contains('.') {
        return Err("URL has no public host name".into());
    }
    if host.starts_with('[') || host.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err("IP-literal hosts are refused; cite the site by name".into());
    }
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") || host.ends_with(".internal") {
        return Err("local hosts are refused".into());
    }
    if let Some(b) = blocked(&host) {
        return Err(format!(
            "{b} is Wikipedia or derived from it: not an allowed source; find the primary source"
        ));
    }
    Ok(host)
}

pub fn blocked(host: &str) -> Option<&'static str> {
    BLOCKED_HOSTS
        .iter()
        .copied()
        .find(|b| host == *b || host.ends_with(&format!(".{b}")))
        .or_else(|| host.contains("wikipedia").then_some("wikipedia"))
}

/// Bare DOI from `10.x/y`, `doi:10.x/y` or a doi.org URL.
pub fn norm_doi(d: &str) -> Result<String, String> {
    let d = d.trim();
    let lower = d.to_ascii_lowercase();
    let mut s = d;
    for p in ["https://doi.org/", "http://doi.org/", "https://dx.doi.org/", "http://dx.doi.org/", "doi:"] {
        if lower.starts_with(p) {
            s = &d[p.len()..];
            break;
        }
    }
    let s = s.trim();
    if !s.starts_with("10.") || !s.contains('/') || s.chars().any(|c| c.is_whitespace()) {
        return Err(format!("not a DOI: {d}"));
    }
    Ok(s.to_string())
}

fn enc_path(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b'(' | b')' | b';' | b':' => {
                o.push(b as char)
            }
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

/// Lower-case, whitespace-collapsed text for phrase matching.
pub fn squash(t: &str) -> String {
    t.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Crude HTML to text: drops script/style, tags and decodes a few entities.
pub fn html_text(h: &str) -> String {
    let mut out = String::with_capacity(h.len() / 2);
    let lower = h.to_ascii_lowercase();
    let mut i = 0;
    let b = h.as_bytes();
    while i < b.len() {
        if b[i] == b'<' {
            for tag in ["script", "style", "noscript"] {
                if lower[i + 1..].starts_with(tag) {
                    if let Some(end) = lower[i..].find(&format!("</{tag}")) {
                        i += end;
                    }
                    break;
                }
            }
            match h[i..].find('>') {
                Some(j) => {
                    i += j + 1;
                    out.push(' ');
                }
                None => break,
            }
        } else {
            let next = h[i..].find('<').map(|j| i + j).unwrap_or(b.len());
            out.push_str(&h[i..next]);
            i = next;
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn html_title(h: &str) -> String {
    let l = h.to_ascii_lowercase();
    match (l.find("<title"), l.find("</title>")) {
        (Some(a), Some(z)) if a < z => {
            let s = &h[a..z];
            html_text(s.split_once('>').map(|x| x.1).unwrap_or(""))
        }
        _ => String::new(),
    }
}

fn cut(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let head = crate::utf8::prefix_bytes(s, n);
    format!("{head}…")
}

pub struct SourceCheckTool {
    pub curl: String,
    pub pdftotext: String,
}

impl Default for SourceCheckTool {
    fn default() -> Self {
        SourceCheckTool {
            curl: "curl".into(),
            pdftotext: "pdftotext".into(),
        }
    }
}

struct Fetched {
    status: u16,
    final_url: String,
    ctype: String,
    body: Vec<u8>,
}

impl SourceCheckTool {
    fn get(&self, url: &str, accept: &str) -> Result<Fetched, String> {
        let out = Command::new(&self.curl)
            .args([
                "-sS", "-L", "--proto", "=https", "--proto-redir", "=https",
                "--max-redirs", "8", "--max-time", "25", "--max-filesize", "4000000",
                "--compressed", "-A", UA, "-H", &format!("Accept: {accept}"),
                "-w", "\n@@SC@@%{http_code} %{content_type} %{url_effective}", url,
            ])
            .output()
            .map_err(|e| format!("curl not runnable: {e}"))?;
        if !out.status.success() && out.stdout.is_empty() {
            return Err(format!(
                "request failed: {}",
                crate::redact(String::from_utf8_lossy(&out.stderr).trim())
            ));
        }
        let raw = out.stdout;
        let mark = b"\n@@SC@@";
        let pos = raw
            .windows(mark.len())
            .rposition(|w| w == mark)
            .ok_or("no status line from curl")?;
        let tail = String::from_utf8_lossy(&raw[pos + mark.len()..]).to_string();
        let mut it = tail.splitn(3, ' ');
        let status = it.next().unwrap_or("0").parse().unwrap_or(0);
        let ctype = it.next().unwrap_or("").to_string();
        let final_url = it.next().unwrap_or("").trim().to_string();
        Ok(Fetched { status, final_url, ctype, body: raw[..pos].to_vec() })
    }

    fn doi(&self, args: &Value) -> ToolOutput {
        let doi = match norm_doi(args["doi"].as_str().unwrap_or("")) {
            Ok(d) => d,
            Err(e) => return ToolOutput::err(e),
        };
        let mut result = json!({"doi": doi, "exists": false});
        let cr = self.get(&format!("{CROSSREF}{}", enc_path(&doi)), "application/json");
        match cr {
            Ok(f) if f.status == 200 => {
                let v: Value = serde_json::from_slice(&f.body).unwrap_or(Value::Null);
                let m = &v["message"];
                let authors: Vec<String> = m["author"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .take(12)
                            .map(|x| {
                                let fam = x["family"].as_str().or(x["name"].as_str()).unwrap_or("");
                                let giv = x["given"].as_str().unwrap_or("");
                                format!("{giv} {fam}").trim().to_string()
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let date = m["issued"]["date-parts"][0]
                    .as_array()
                    .map(|p| p.iter().filter_map(|n| n.as_i64()).map(|n| format!("{n:02}")).collect::<Vec<_>>().join("-"))
                    .unwrap_or_default();
                result = json!({
                    "doi": doi, "exists": true, "registry": "crossref",
                    "title": m["title"][0].as_str().unwrap_or(""),
                    "authors": authors,
                    "container": m["container-title"][0].as_str().unwrap_or(""),
                    "publisher": m["publisher"].as_str().unwrap_or(""),
                    "issued": date,
                    "type": m["type"].as_str().unwrap_or(""),
                    "url": m["URL"].as_str().unwrap_or(""),
                });
            }
            _ => {
                if let Ok(f) = self.get(&format!("{DATACITE}{}", enc_path(&doi)), "application/vnd.api+json") {
                    if f.status == 200 {
                        let v: Value = serde_json::from_slice(&f.body).unwrap_or(Value::Null);
                        let a = &v["data"]["attributes"];
                        let authors: Vec<String> = a["creators"]
                            .as_array()
                            .map(|c| c.iter().take(12).filter_map(|x| x["name"].as_str().map(String::from)).collect())
                            .unwrap_or_default();
                        result = json!({
                            "doi": doi, "exists": true, "registry": "datacite",
                            "title": a["titles"][0]["title"].as_str().unwrap_or(""),
                            "authors": authors,
                            "container": a["container"]["title"].as_str().unwrap_or(""),
                            "publisher": a["publisher"].as_str().or(a["publisher"]["name"].as_str()).unwrap_or(""),
                            "issued": a["publicationYear"].to_string().trim_matches('"').to_string(),
                            "type": a["types"]["resourceTypeGeneral"].as_str().unwrap_or(""),
                            "url": a["url"].as_str().unwrap_or(""),
                        });
                    }
                }
            }
        }
        if result["exists"] == json!(false) {
            result["note"] = json!("DOI not found in Crossref or DataCite: do not cite it");
        }
        if let Some(t) = args["expect_title"].as_str() {
            let got = squash(result["title"].as_str().unwrap_or(""));
            let want = squash(t);
            result["title_match"] = json!(!want.is_empty() && (got == want || got.contains(&want) || want.contains(&got) && !got.is_empty()));
        }
        ToolOutput::ok(result.to_string())
    }

    fn url(&self, args: &Value) -> ToolOutput {
        let url = args["url"].as_str().unwrap_or("").trim().to_string();
        if let Err(e) = check_url(&url) {
            return ToolOutput::err(e);
        }
        let f = match self.get(&url, "text/html,application/xhtml+xml,application/pdf,text/plain;q=0.9,*/*;q=0.5") {
            Ok(f) => f,
            Err(e) => return ToolOutput::err(e),
        };
        if let Err(e) = check_url(&f.final_url) {
            return ToolOutput::err(format!("redirected to a refused URL: {e}"));
        }
        let is_pdf = f.ctype.contains("pdf") || f.body.starts_with(b"%PDF");
        let (title, text) = if is_pdf {
            (String::new(), self.pdf_text(&f.body).unwrap_or_default())
        } else {
            let h = String::from_utf8_lossy(&f.body);
            if f.ctype.contains("html") || h.contains("<html") || h.contains("<HTML") {
                (html_title(&h), html_text(&h))
            } else {
                (String::new(), h.split_whitespace().collect::<Vec<_>>().join(" "))
            }
        };
        let hay = squash(&text);
        let mut checks = vec![];
        if let Some(arr) = args["expect"].as_array() {
            for p in arr.iter().filter_map(|x| x.as_str()).take(10) {
                let n = squash(p);
                checks.push(json!({"phrase": p, "found": !n.is_empty() && hay.contains(&n)}));
            }
        }
        let ok = (200..300).contains(&f.status);
        let excerpt_at = args["excerpt_from"].as_str().and_then(|p| hay.find(&squash(p)));
        let excerpt = match excerpt_at {
            Some(i) => {
                let s = crate::utf8::floor_char_boundary(&hay, i);
                cut(&hay[s..], MAX_TEXT)
            }
            None => cut(&text, MAX_TEXT),
        };
        ToolOutput::ok(
            json!({
                "url": url, "final_url": f.final_url, "status": f.status, "ok": ok,
                "content_type": f.ctype, "pdf": is_pdf, "title": title,
                "text_chars": text.len(), "expect": checks, "excerpt": excerpt,
            })
            .to_string(),
        )
    }

    fn pdf_text(&self, body: &[u8]) -> Option<String> {
        use std::io::Write;
        let mut child = Command::new(&self.pdftotext)
            .args(["-q", "-l", "40", "-", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        child.stdin.take()?.write_all(body).ok()?;
        let o = child.wait_with_output().ok()?;
        Some(String::from_utf8_lossy(&o.stdout).split_whitespace().collect::<Vec<_>>().join(" "))
    }
}

impl Tool for SourceCheckTool {
    fn name(&self) -> String {
        "source_check".into()
    }
    fn description(&self) -> String {
        "Verify a source before citing it. action=doi (doi, optional expect_title): Crossref/DataCite lookup, returns title, authors, venue, issued date, landing URL, exists=false if unregistered. action=url (url, optional expect: [phrases], excerpt_from): fetch one https page or PDF (read-only, 4 MB cap) and report status, final URL, title, text excerpt and whether each expected phrase is on the page. Wikipedia and Wikipedia-derived sites are refused.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "action": {"type": "string", "enum": ["doi", "url"]},
            "doi": {"type": "string", "description": "10.x/y, doi:10.x/y or https://doi.org/10.x/y"},
            "expect_title": {"type": "string"},
            "url": {"type": "string", "description": "https URL"},
            "expect": {"type": "array", "items": {"type": "string"}, "description": "phrases that must be on the page (max 10)"},
            "excerpt_from": {"type": "string", "description": "start the excerpt at this phrase"}
        }, "required": ["action"]})
    }
    fn call(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        match args["action"].as_str().unwrap_or("") {
            "doi" => self.doi(args),
            "url" => self.url(args),
            _ => ToolOutput::err("action must be doi or url"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_policy() {
        assert!(check_url("https://www.bipm.org/en/publications/si-brochure").is_ok());
        assert!(check_url("http://www.bipm.org/").is_err());
        assert!(check_url("https://en.wikipedia.org/wiki/Pi").unwrap_err().contains("Wikipedia"));
        assert!(check_url("https://www.wikiwand.com/en/Pi").is_err());
        assert!(check_url("https://m.wikidata.org/x").is_err());
        assert!(check_url("https://127.0.0.1:8642/health").is_err());
        assert!(check_url("https://[::1]/").is_err());
        assert!(check_url("https://localhost/").is_err());
        assert!(check_url("https://user:pw@example.org/").is_err());
        assert_eq!(check_url("https://Example.ORG:443/a").unwrap(), "example.org");
    }

    #[test]
    fn doi_forms() {
        assert_eq!(norm_doi("https://doi.org/10.1000/xyz").unwrap(), "10.1000/xyz");
        assert_eq!(norm_doi("doi:10.1000/xyz").unwrap(), "10.1000/xyz");
        assert_eq!(norm_doi(" 10.1000/a(b)c ").unwrap(), "10.1000/a(b)c");
        assert!(norm_doi("11.1/x").is_err());
        assert!(norm_doi("10.1000 /x").is_err());
    }

    #[test]
    fn html_to_text() {
        let h = "<html><head><title>SI &amp; units</title><style>x{}</style><script>var a=1</script></head><body><p>The  metre is\n the length</p></body></html>";
        assert_eq!(html_title(h), "SI & units");
        let t = html_text(h);
        assert!(t.contains("The metre is the length"), "{t}");
        assert!(!t.contains("var a"));
        assert!(squash(&t).contains(&squash("the METRE is the length")));
    }

    #[test]
    fn unknown_action() {
        let t = SourceCheckTool::default();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let emit = |_e: crate::tools::Event| {};
        let ctx = ToolCtx {
            session_id: "t",
            spend_session: "t",
            depth: 0,
            user_text: "",
            cancel: &cancel,
            emit: &emit,
        };
        assert!(t.call(&json!({"action": "x"}), &ctx).is_error);
        assert!(t.call(&json!({"action": "url", "url": "https://en.wikipedia.org/wiki/E"}), &ctx).is_error);
    }
}
