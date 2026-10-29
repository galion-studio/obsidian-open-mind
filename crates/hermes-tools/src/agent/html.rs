// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! A small, forgiving HTML -> text converter for browser snapshots: drops
//! script/style/noscript/svg/template/iframe content (keeps <title>), turns
//! block tags into line breaks, decodes common entities and collects links.
//! Not a full HTML parser; good enough for reading a page.

/// Text, title and up to `max_links` (text, href) pairs.
pub struct Page {
    pub title: String,
    pub text: String,
    pub links: Vec<(String, String)>,
}

const SKIP: &[&str] = &["script", "style", "noscript", "svg", "template", "iframe"];
const BLOCK: &[&str] = &[
    "p",
    "div",
    "br",
    "li",
    "ul",
    "ol",
    "tr",
    "table",
    "section",
    "article",
    "header",
    "footer",
    "nav",
    "main",
    "aside",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "pre",
    "blockquote",
    "hr",
    "form",
    "dt",
    "dd",
    "figcaption",
    "summary",
    "details",
];

pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let end = tail[..tail.len().min(12)].find(';');
        let Some(end) = end else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let ent = &tail[1..end];
        let rep: Option<String> = match ent {
            "amp" => Some("&".into()),
            "lt" => Some("<".into()),
            "gt" => Some(">".into()),
            "quot" => Some("\"".into()),
            "apos" | "#39" => Some("'".into()),
            "nbsp" => Some(" ".into()),
            "mdash" => Some("—".into()),
            "ndash" => Some("–".into()),
            "hellip" => Some("…".into()),
            "copy" => Some("©".into()),
            _ => {
                let n = if let Some(h) = ent.strip_prefix("#x").or_else(|| ent.strip_prefix("#X")) {
                    u32::from_str_radix(h, 16).ok()
                } else if let Some(d) = ent.strip_prefix('#') {
                    d.parse::<u32>().ok()
                } else {
                    None
                };
                n.and_then(char::from_u32).map(|c| c.to_string())
            }
        };
        match rep {
            Some(r) => {
                out.push_str(&r);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn tag_name(t: &str) -> (bool, String) {
    let t = t.trim_start();
    let (close, t) = match t.strip_prefix('/') {
        Some(r) => (true, r),
        None => (false, t),
    };
    let n: String = t
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    (close, n)
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let l = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = l[from..].find(name) {
        let i = from + i;
        let before_ok = i == 0 || l.as_bytes()[i - 1].is_ascii_whitespace();
        let after = l[i + name.len()..].trim_start();
        if before_ok && after.starts_with('=') {
            let off = tag.len() - after.len() + 1;
            let v = tag[off..].trim_start();
            let (q, body) = match v.chars().next() {
                Some(c @ ('"' | '\'')) => (Some(c), &v[1..]),
                _ => (None, v),
            };
            let end = match q {
                Some(c) => body.find(c).unwrap_or(body.len()),
                None => body
                    .find(|c: char| c.is_whitespace() || c == '>')
                    .unwrap_or(body.len()),
            };
            return Some(decode_entities(&body[..end]));
        }
        from = i + name.len();
    }
    None
}

/// Make `href` absolute against `base` (scheme://host[:port]/path).
pub fn absolutize(base: &str, href: &str) -> Option<String> {
    let h = href.trim();
    if h.starts_with("http://") || h.starts_with("https://") {
        return Some(h.to_string());
    }
    if h.is_empty() || h.starts_with('#') || h.contains(':') && !h.starts_with('/') {
        return None; // mailto:, javascript:, data:, fragments
    }
    let (scheme, rest) = base.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    if let Some(r) = h.strip_prefix("//") {
        return Some(format!("{scheme}://{r}"));
    }
    if h.starts_with('/') {
        return Some(format!("{scheme}://{host}{h}"));
    }
    let path = &rest[host.len()..];
    let path = path.split(['?', '#']).next().unwrap_or("");
    let dir = match path.rfind('/') {
        Some(i) => &path[..=i],
        None => "/",
    };
    Some(format!("{scheme}://{host}{dir}{h}"))
}

pub fn to_text(html: &str, base: &str, max_links: usize) -> Page {
    let mut text = String::new();
    let mut title = String::new();
    let mut links = vec![];
    let mut skip: Option<String> = None;
    let mut in_title = false;
    let mut link: Option<(String, String)> = None;
    let mut rest = html;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            if skip.is_none() {
                text.push_str(&decode_entities(rest));
            }
            break;
        };
        let chunk = &rest[..lt];
        if in_title {
            title.push_str(&decode_entities(chunk));
        } else if skip.is_none() {
            let d = decode_entities(chunk);
            if let Some(l) = link.as_mut() {
                l.0.push_str(&d);
            }
            text.push_str(&d);
        }
        rest = &rest[lt..];
        if rest.starts_with("<!--") {
            rest = match rest.find("-->") {
                Some(e) => &rest[e + 3..],
                None => "",
            };
            continue;
        }
        let Some(gt) = rest.find('>') else {
            break;
        };
        let tag = &rest[1..gt];
        rest = &rest[gt + 1..];
        let (close, name) = tag_name(tag);
        if name.is_empty() {
            continue;
        }
        if let Some(s) = &skip {
            if close && *s == name {
                skip = None;
            }
            continue;
        }
        if name == "title" {
            in_title = !close;
            continue;
        }
        if !close && SKIP.contains(&name.as_str()) && !tag.trim_end().ends_with('/') {
            skip = Some(name);
            continue;
        }
        if name == "a" {
            if close {
                if let Some((t, h)) = link.take() {
                    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
                    if links.len() < max_links && !t.is_empty() {
                        links.push((t, h));
                    }
                }
            } else if let Some(h) = attr(tag, "href").and_then(|h| absolutize(base, &h)) {
                link = Some((String::new(), h));
            }
        }
        if name == "img" {
            if let Some(a) = attr(tag, "alt").filter(|a| !a.trim().is_empty()) {
                text.push_str(&format!(" [image: {}] ", a.trim()));
            }
        }
        if BLOCK.contains(&name.as_str()) {
            text.push('\n');
        } else if name == "td" || name == "th" {
            text.push_str(" | ");
        }
    }
    // collapse whitespace, keep paragraph breaks
    let mut out = String::new();
    let mut blank = 0;
    for line in text.lines() {
        let l = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if l.is_empty() {
            blank += 1;
            if blank == 1 && !out.is_empty() {
                out.push('\n');
            }
        } else {
            blank = 0;
            out.push_str(&l);
            out.push('\n');
        }
    }
    Page {
        title: title.split_whitespace().collect::<Vec<_>>().join(" "),
        text: out.trim().to_string(),
        links,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_scripts_and_keeps_text_links() {
        let h = r#"<html><head><title>Hi &amp; bye</title><script>var x="<p>no</p>";</script></head>
<body><h1>Head</h1><p>One&nbsp;two &#x41;&#66;</p><a href="/x?y=1">Link <b>A</b></a>
<style>.a{}</style><a href="mailto:a@b">mail</a><img alt="cat"><!-- c --><table><tr><td>a</td><td>b</td></tr></table></body></html>"#;
        let p = to_text(h, "https://ex.org/dir/page", 10);
        assert_eq!(p.title, "Hi & bye");
        assert!(p.text.contains("Head"));
        assert!(p.text.contains("One two AB"));
        assert!(!p.text.contains("var x"));
        assert!(!p.text.contains(".a{}"));
        assert!(p.text.contains("[image: cat]"));
        assert_eq!(
            p.links,
            vec![("Link A".to_string(), "https://ex.org/x?y=1".to_string())]
        );
    }

    #[test]
    fn absolutize_cases() {
        let b = "https://ex.org/a/b.html?q";
        assert_eq!(absolutize(b, "c.html").unwrap(), "https://ex.org/a/c.html");
        assert_eq!(absolutize(b, "//cdn.x/y").unwrap(), "https://cdn.x/y");
        assert!(absolutize(b, "javascript:alert(1)").is_none());
        assert!(absolutize(b, "#top").is_none());
    }
}
