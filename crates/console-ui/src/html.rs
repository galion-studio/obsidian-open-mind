// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! HTML escaping. Every API string goes through [`esc`] before it reaches a
//! page; there is no "raw" path for upstream data.

/// Escape text for an HTML text node or a double-quoted attribute value.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Percent-encode a value for a query string (RFC 3986 unreserved kept).
pub fn url_enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Decode an `application/x-www-form-urlencoded` component (`+` is space).
/// Invalid escapes are kept literally; invalid UTF-8 is replaced.
pub fn form_dec(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let h = std::str::from_utf8(&b[i + 1..i + 3])
                    .ok()
                    .and_then(|t| u8::from_str_radix(t, 16).ok());
                match h {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                        continue;
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

/// Parse `a=1&b=2` into pairs (form bodies and query strings).
pub fn form_pairs(s: &str) -> Vec<(String, String)> {
    s.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (form_dec(k), form_dec(v))
        })
        .collect()
}

pub fn form_get<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes() {
        assert_eq!(
            esc(r#"<script>"x"&'y'</script>"#),
            "&lt;script&gt;&quot;x&quot;&amp;&#39;y&#39;&lt;/script&gt;"
        );
        assert_eq!(esc("plain ünï"), "plain ünï");
    }

    #[test]
    fn forms() {
        let p = form_pairs("cmd=health&x=a+b%21&bad=%zz&t=%e2%82%ac&end=%4");
        assert_eq!(form_get(&p, "cmd"), Some("health"));
        assert_eq!(form_get(&p, "x"), Some("a b!"));
        assert_eq!(form_get(&p, "bad"), Some("%zz"));
        assert_eq!(form_get(&p, "t"), Some("€"));
        assert_eq!(form_get(&p, "end"), Some("%4"));
        assert_eq!(url_enc("/a b?&"), "/a%20b%3F%26");
    }
}
