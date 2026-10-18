// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! A small YAML subset reader, enough for Hermes' config.yaml: block maps,
//! block lists (incl. `- key: v` items), flow `[..]` / `{..}`, quoted and
//! plain scalars, `|` / `>` block scalars, comments. No anchors, tags or
//! multi-document streams. Output is a serde_json::Value so the rest of the
//! crate reads config the same way it reads JSON.

use serde_json::{Map, Number, Value};

#[derive(Debug, Clone)]
struct Line {
    indent: usize,
    text: String,
    raw: String,
}

pub fn parse(src: &str) -> Result<Value, String> {
    let mut lines = Vec::new();
    for raw in src.lines() {
        let raw = raw.trim_end_matches('\r');
        if raw.trim_start().starts_with("---") && raw.trim().chars().all(|c| c == '-') {
            continue;
        }
        let stripped = strip_comment(raw);
        if stripped.trim().is_empty() {
            // keep blank lines only for block scalars
            lines.push(Line {
                indent: usize::MAX,
                text: String::new(),
                raw: raw.to_string(),
            });
            continue;
        }
        let indent = stripped.len() - stripped.trim_start_matches(' ').len();
        lines.push(Line {
            indent,
            text: stripped.trim().to_string(),
            raw: raw.to_string(),
        });
    }
    let mut p = Parser { lines, i: 0 };
    p.skip_blank();
    if p.i >= p.lines.len() {
        return Ok(Value::Object(Map::new()));
    }
    let ind = p.lines[p.i].indent;
    p.block(ind)
}

fn strip_comment(line: &str) -> String {
    let mut in_s = false;
    let mut in_d = false;
    let mut prev = ' ';
    for (i, c) in line.char_indices() {
        match c {
            '\'' if !in_d => in_s = !in_s,
            '"' if !in_s && prev != '\\' => in_d = !in_d,
            '#' if !in_s && !in_d && (prev == ' ' || prev == '\t' || i == 0) => {
                return line[..i].trim_end().to_string();
            }
            _ => {}
        }
        prev = c;
    }
    line.to_string()
}

struct Parser {
    lines: Vec<Line>,
    i: usize,
}

fn is_item(t: &str) -> bool {
    t == "-" || t.starts_with("- ")
}

impl Parser {
    fn skip_blank(&mut self) {
        while self.i < self.lines.len() && self.lines[self.i].indent == usize::MAX {
            self.i += 1;
        }
    }

    fn block(&mut self, indent: usize) -> Result<Value, String> {
        self.skip_blank();
        if self.i < self.lines.len() && is_item(&self.lines[self.i].text) {
            self.list(indent)
        } else {
            self.map(indent)
        }
    }

    fn map(&mut self, indent: usize) -> Result<Value, String> {
        let mut m = Map::new();
        loop {
            self.skip_blank();
            if self.i >= self.lines.len() {
                break;
            }
            let line = self.lines[self.i].clone();
            if line.indent < indent || (line.indent == indent && is_item(&line.text)) {
                break;
            }
            if line.indent > indent {
                return Err(format!("unexpected indent: {}", line.text));
            }
            let (k, rest) = split_key(&line.text)
                .ok_or_else(|| format!("expected `key:` in: {}", line.text))?;
            self.i += 1;
            let v = self.value_after_key(indent, &rest)?;
            m.insert(k, v);
        }
        Ok(Value::Object(m))
    }

    fn value_after_key(&mut self, indent: usize, rest: &str) -> Result<Value, String> {
        let rest = rest.trim();
        if rest.starts_with('|') || rest.starts_with('>') {
            return Ok(Value::String(
                self.block_scalar(indent, rest.starts_with('>')),
            ));
        }
        if !rest.is_empty() {
            return flow(rest);
        }
        self.skip_blank();
        if self.i < self.lines.len() {
            let next = &self.lines[self.i];
            if next.indent > indent {
                let ni = next.indent;
                return self.block(ni);
            }
            if next.indent == indent && is_item(&next.text) {
                return self.list(indent);
            }
        }
        Ok(Value::Null)
    }

    fn block_scalar(&mut self, indent: usize, folded: bool) -> String {
        let mut out: Vec<String> = Vec::new();
        let mut body_indent: Option<usize> = None;
        while self.i < self.lines.len() {
            let l = &self.lines[self.i];
            let raw_indent = l.raw.len() - l.raw.trim_start_matches(' ').len();
            if l.raw.trim().is_empty() {
                out.push(String::new());
                self.i += 1;
                continue;
            }
            if raw_indent <= indent {
                break;
            }
            let bi = *body_indent.get_or_insert(raw_indent);
            out.push(l.raw.get(bi.min(raw_indent)..).unwrap_or("").to_string());
            self.i += 1;
        }
        while out.last().is_some_and(|s| s.is_empty()) {
            out.pop();
        }
        let sep = if folded { " " } else { "\n" };
        let mut s = out.join(sep);
        s.push('\n');
        s
    }

    fn list(&mut self, indent: usize) -> Result<Value, String> {
        let mut v = Vec::new();
        loop {
            self.skip_blank();
            if self.i >= self.lines.len() {
                break;
            }
            let line = self.lines[self.i].clone();
            if line.indent != indent || !is_item(&line.text) {
                break;
            }
            let item = line.text[1..].trim_start().to_string();
            if item.is_empty() {
                self.i += 1;
                self.skip_blank();
                if self.i < self.lines.len() && self.lines[self.i].indent > indent {
                    let ni = self.lines[self.i].indent;
                    v.push(self.block(ni)?);
                } else {
                    v.push(Value::Null);
                }
                continue;
            }
            if !item.starts_with('[')
                && !item.starts_with('{')
                && !item.starts_with('"')
                && !item.starts_with('\'')
                && split_key(&item).is_some()
            {
                // `- key: v` starts a map whose keys sit at the item's column
                let col = indent + (line.text.len() - item.len());
                self.lines[self.i].indent = col;
                self.lines[self.i].text = item;
                v.push(self.map(col)?);
                continue;
            }
            self.i += 1;
            v.push(flow(&item)?);
        }
        Ok(Value::Array(v))
    }
}

/// Split `key: rest` at the first `:` followed by space/end, outside quotes.
fn split_key(t: &str) -> Option<(String, String)> {
    let b = t.as_bytes();
    if b.first() == Some(&b'"') || b.first() == Some(&b'\'') {
        let q = b[0] as char;
        let end = t[1..].find(q)? + 1;
        let key = t[1..end].to_string();
        let after = t[end + 1..].trim_start();
        let rest = after.strip_prefix(':')?;
        return Some((key, rest.to_string()));
    }
    let mut depth = 0i32;
    for (i, c) in t.char_indices() {
        match c {
            '[' | '{' => depth += 1,
            ']' | '}' => depth -= 1,
            ':' if depth == 0 => {
                let after = &t[i + 1..];
                if after.is_empty() || after.starts_with(' ') || after.starts_with('\t') {
                    let k = t[..i].trim();
                    if k.is_empty() {
                        return None;
                    }
                    return Some((k.to_string(), after.to_string()));
                }
            }
            _ => {}
        }
    }
    None
}

fn flow(s: &str) -> Result<Value, String> {
    let chars: Vec<char> = s.trim().chars().collect();
    let mut pos = 0;
    let v = flow_value(&chars, &mut pos, false)?;
    Ok(v)
}

fn skip_ws(c: &[char], p: &mut usize) {
    while *p < c.len() && c[*p].is_whitespace() {
        *p += 1;
    }
}

fn flow_value(c: &[char], p: &mut usize, nested: bool) -> Result<Value, String> {
    skip_ws(c, p);
    if *p >= c.len() {
        return Ok(Value::Null);
    }
    match c[*p] {
        '[' => {
            *p += 1;
            let mut v = Vec::new();
            loop {
                skip_ws(c, p);
                if *p < c.len() && c[*p] == ']' {
                    *p += 1;
                    break;
                }
                v.push(flow_value(c, p, true)?);
                skip_ws(c, p);
                match c.get(*p) {
                    Some(',') => *p += 1,
                    Some(']') => {
                        *p += 1;
                        break;
                    }
                    _ => return Err("bad flow list".into()),
                }
            }
            Ok(Value::Array(v))
        }
        '{' => {
            *p += 1;
            let mut m = Map::new();
            loop {
                skip_ws(c, p);
                if *p < c.len() && c[*p] == '}' {
                    *p += 1;
                    break;
                }
                let k = match flow_value_key(c, p)? {
                    Value::String(s) => s,
                    other => other.to_string(),
                };
                skip_ws(c, p);
                if c.get(*p) != Some(&':') {
                    return Err("bad flow map".into());
                }
                *p += 1;
                let v = flow_value(c, p, true)?;
                m.insert(k, v);
                skip_ws(c, p);
                match c.get(*p) {
                    Some(',') => *p += 1,
                    Some('}') => {
                        *p += 1;
                        break;
                    }
                    _ => return Err("bad flow map".into()),
                }
            }
            Ok(Value::Object(m))
        }
        '"' | '\'' => Ok(Value::String(quoted(c, p)?)),
        _ => {
            let start = *p;
            while *p < c.len() {
                let ch = c[*p];
                if nested && (ch == ',' || ch == ']' || ch == '}') {
                    break;
                }
                *p += 1;
            }
            let s: String = c[start..*p].iter().collect();
            Ok(scalar(s.trim()))
        }
    }
}

fn flow_value_key(c: &[char], p: &mut usize) -> Result<Value, String> {
    skip_ws(c, p);
    if matches!(c.get(*p), Some('"') | Some('\'')) {
        return Ok(Value::String(quoted(c, p)?));
    }
    let start = *p;
    while *p < c.len() && c[*p] != ':' {
        *p += 1;
    }
    Ok(Value::String(
        c[start..*p].iter().collect::<String>().trim().to_string(),
    ))
}

fn quoted(c: &[char], p: &mut usize) -> Result<String, String> {
    let q = c[*p];
    *p += 1;
    let mut s = String::new();
    while *p < c.len() {
        let ch = c[*p];
        *p += 1;
        if ch == q {
            if q == '\'' && c.get(*p) == Some(&'\'') {
                s.push('\'');
                *p += 1;
                continue;
            }
            return Ok(s);
        }
        if q == '"' && ch == '\\' && *p < c.len() {
            let e = c[*p];
            *p += 1;
            s.push(match e {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '0' => '\0',
                other => other,
            });
            continue;
        }
        s.push(ch);
    }
    Err("unterminated string".into())
}

fn scalar(s: &str) -> Value {
    match s {
        "" | "~" | "null" | "Null" | "NULL" => return Value::Null,
        "true" | "True" | "TRUE" | "yes" | "on" => return Value::Bool(true),
        "false" | "False" | "FALSE" | "no" | "off" => return Value::Bool(false),
        _ => {}
    }
    if let Ok(i) = s.parse::<i64>() {
        return Value::Number(i.into());
    }
    if s.chars().any(|c| c.is_ascii_digit())
        && s.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))
    {
        if let Ok(f) = s.parse::<f64>() {
            if let Some(n) = Number::from_f64(f) {
                return Value::Number(n);
            }
        }
    }
    Value::String(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
model:
  provider: openrouter
  default: anthropic/claude-opus-5.5   # boss
  context_length: 65536
  base_url: https://openrouter.ai/api/v1
agent:
  system_prompt: "Reply in exactly four sentences, # not a comment"
  reasoning_effort: "high"
  reasoning_overrides:
    "anthropic/claude-haiku-5.5": "none"
platform_toolsets:
  cli: [hermes-cli, video]
mcp_servers:
  galion-stack:
    command: /usr/bin/stack-mcp
    args: ["/x/stack-read.py"]
    timeout: 30
    lazy: true
auxiliary:
  compression:
    fallback_chain:
      - {provider: openrouter, model: "a:free"}
      - provider: x
        model: y
skills:
  external_dirs:
  - /opt/skills/local
threshold: 0.50
note: |
  line one
  line two
empty:
"#;

    #[test]
    fn parses_hermes_subset() {
        let v = parse(SAMPLE).unwrap();
        assert_eq!(v["model"]["default"], "anthropic/claude-opus-5.5");
        assert_eq!(v["model"]["context_length"], 65536);
        assert_eq!(v["model"]["base_url"], "https://openrouter.ai/api/v1");
        assert_eq!(
            v["agent"]["system_prompt"],
            "Reply in exactly four sentences, # not a comment"
        );
        assert_eq!(
            v["agent"]["reasoning_overrides"]["anthropic/claude-haiku-5.5"],
            "none"
        );
        assert_eq!(v["platform_toolsets"]["cli"][1], "video");
        assert_eq!(
            v["mcp_servers"]["galion-stack"]["args"][0],
            "/x/stack-read.py"
        );
        assert_eq!(v["mcp_servers"]["galion-stack"]["lazy"], true);
        let chain = &v["auxiliary"]["compression"]["fallback_chain"];
        assert_eq!(chain[0]["model"], "a:free");
        assert_eq!(chain[1]["provider"], "x");
        assert_eq!(chain[1]["model"], "y");
        assert_eq!(v["skills"]["external_dirs"][0], "/opt/skills/local");
        assert_eq!(v["threshold"], 0.5);
        assert_eq!(v["note"], "line one\nline two\n");
        assert!(v["empty"].is_null());
    }
}
