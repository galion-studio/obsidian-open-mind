// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Parser for mDNS-SD style text records.
//!
//! The fixture form is a small zone-file subset: owner name, optional TTL,
//! optional class `IN`, then `PTR`, `SRV`, `TXT`, `A`, or `AAAA`. Comments
//! start at an unquoted `;`. This module does not open sockets.

use crate::{Candidate, Origin, APPROVAL};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

const MAX_LINES: usize = 512;
const MAX_LINE_LEN: usize = 2048;
const MAX_RECORDS: usize = 256;
const MAX_TXT: usize = 32;
const SERVICE_ENUM: &str = "_services._dns-sd._udp.local.";

/// One parsed record. Names are lowercase and end with a dot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub name: String,
    pub ttl: Option<u32>,
    pub rdata: RData,
}

/// Rdata this stub understands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RData {
    Ptr(String),
    Srv {
        priority: u16,
        weight: u16,
        port: u16,
        target: String,
    },
    Txt(Vec<(String, String)>),
    A(String),
    Aaaa(String),
}

/// A fixture that is not valid mDNS-SD style text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl ParseError {
    fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}", self.message)
        } else {
            write!(f, "line {}: {}", self.line, self.message)
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse an mDNS-SD style fixture. Empty input is an empty record list.
pub fn parse_mdns_sd(text: &str) -> Result<Vec<Record>, ParseError> {
    let mut records = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        if line_no > MAX_LINES {
            return Err(ParseError::new(line_no, "too many lines"));
        }
        if raw.len() > MAX_LINE_LEN {
            return Err(ParseError::new(line_no, "line is too long"));
        }
        let body = strip_unquoted_comment(raw).trim();
        if body.is_empty() {
            continue;
        }
        if records.len() >= MAX_RECORDS {
            return Err(ParseError::new(line_no, "too many records"));
        }
        let tokens = tokenize(body).map_err(|msg| ParseError::new(line_no, msg))?;
        records.push(parse_record(&tokens).map_err(|msg| ParseError::new(line_no, msg))?);
    }
    Ok(records)
}

/// Fold records into candidates. Enumeration PTRs do not become candidates.
///
/// Every returned candidate has `origin = mdns` and `approval = pending`.
pub fn fold_records(records: &[Record]) -> Vec<Candidate> {
    let mut addrs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut parts: BTreeMap<String, Partial> = BTreeMap::new();

    for rec in records {
        match &rec.rdata {
            RData::A(addr) | RData::Aaaa(addr) => {
                addrs
                    .entry(rec.name.clone())
                    .or_default()
                    .insert(addr.clone());
            }
            RData::Ptr(target) => {
                if rec.name == SERVICE_ENUM {
                    continue;
                }
                if rec.name.starts_with('_') && is_dns_sd_name(&rec.name) {
                    let part = partial(&mut parts, target);
                    part.service_type = rec.name.clone();
                }
            }
            RData::Srv { port, target, .. } => {
                let part = partial(&mut parts, &rec.name);
                part.port = Some(*port);
                part.host = target.clone();
                if part.service_type.is_empty() {
                    part.service_type = service_type_from_instance(&rec.name);
                }
            }
            RData::Txt(pairs) => {
                let part = partial(&mut parts, &rec.name);
                if part.service_type.is_empty() {
                    part.service_type = service_type_from_instance(&rec.name);
                }
                for (key, value) in pairs {
                    part.txt.insert(key.clone(), value.clone());
                }
            }
        }
    }

    let mut out = Vec::new();
    for (instance, part) in parts {
        let id = part
            .txt
            .get("id")
            .and_then(|raw| safe_id(raw))
            .or_else(|| first_label(&instance).and_then(|label| safe_id(&label)))
            .unwrap_or_else(|| "mdns".to_owned());
        let addrs = match addrs.get(&part.host) {
            Some(set) => set.iter().cloned().collect(),
            None => Vec::new(),
        };
        out.push(Candidate {
            id,
            instance,
            service_type: part.service_type,
            host: part.host,
            port: part.port,
            addrs,
            txt: part.txt,
            origin: Origin::Mdns,
            approval: APPROVAL.to_owned(),
        });
    }
    out
}

struct Partial {
    service_type: String,
    host: String,
    port: Option<u16>,
    txt: BTreeMap<String, String>,
}

fn partial<'a>(parts: &'a mut BTreeMap<String, Partial>, key: &str) -> &'a mut Partial {
    parts.entry(key.to_owned()).or_insert_with(|| Partial {
        service_type: String::new(),
        host: String::new(),
        port: None,
        txt: BTreeMap::new(),
    })
}

fn is_dns_sd_name(name: &str) -> bool {
    name.contains("._tcp.") || name.contains("._udp.")
}

fn service_type_from_instance(instance: &str) -> String {
    match instance.split_once('.') {
        Some((_, rest)) if rest.starts_with('_') && is_dns_sd_name(rest) => rest.to_owned(),
        _ => String::new(),
    }
}

fn first_label(name: &str) -> Option<String> {
    let label = name.trim_end_matches('.').split('.').next()?;
    if label.is_empty() {
        None
    } else {
        Some(label.to_owned())
    }
}

fn safe_id(raw: &str) -> Option<String> {
    if (1..=64).contains(&raw.len())
        && !raw.starts_with('-')
        && raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        Some(raw.to_owned())
    } else {
        None
    }
}

fn strip_unquoted_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_quote = false;
    let mut escaped = false;
    for (idx, &byte) in bytes.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_quote && byte == b'\\' {
            escaped = true;
            continue;
        }
        if byte == b'"' {
            in_quote = !in_quote;
            continue;
        }
        if byte == b';' && !in_quote {
            return &line[..idx];
        }
    }
    line
}

fn tokenize(line: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut buf = String::new();
    let mut in_quote = false;
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        if in_quote {
            match ch {
                '\\' => match chars.next() {
                    Some(escaped @ ('\\' | '"')) => buf.push(escaped),
                    Some(other) => {
                        buf.push('\\');
                        buf.push(other);
                    }
                    None => return Err("truncated escape".into()),
                },
                '"' => {
                    in_quote = false;
                    tokens.push(std::mem::take(&mut buf));
                }
                other => buf.push(other),
            }
            continue;
        }
        if ch == '"' {
            if !buf.is_empty() {
                return Err("quote in the middle of a token".into());
            }
            in_quote = true;
            continue;
        }
        if ch.is_ascii_whitespace() {
            if !buf.is_empty() {
                tokens.push(std::mem::take(&mut buf));
            }
            continue;
        }
        buf.push(ch);
    }
    if in_quote {
        return Err("unterminated quote".into());
    }
    if !buf.is_empty() {
        tokens.push(buf);
    }
    Ok(tokens)
}

fn parse_record(tokens: &[String]) -> Result<Record, String> {
    if tokens.is_empty() {
        return Err("empty record".into());
    }
    let name = canon_name(&tokens[0])?;
    let mut idx = 1;
    let mut ttl = None;
    if tokens.get(idx).is_some_and(|token| is_ttl(token)) {
        ttl = Some(parse_ttl(&tokens[idx])?);
        idx += 1;
    }
    if tokens.get(idx).is_some_and(|token| is_class(token)) {
        if !tokens[idx].eq_ignore_ascii_case("IN") {
            return Err("only class IN is supported".into());
        }
        idx += 1;
    }
    let Some(type_tok) = tokens.get(idx) else {
        return Err("missing record type".into());
    };
    let kind = type_tok.to_ascii_uppercase();
    idx += 1;
    let rest = &tokens[idx..];
    let rdata = match kind.as_str() {
        "PTR" => parse_ptr(rest)?,
        "SRV" => parse_srv(rest)?,
        "TXT" => parse_txt(rest)?,
        "A" => parse_a(rest)?,
        "AAAA" => parse_aaaa(rest)?,
        _ => {
            let shown: String = type_tok.chars().take(32).collect();
            return Err(format!("unsupported type {shown}"));
        }
    };
    Ok(Record { name, ttl, rdata })
}

fn is_ttl(token: &str) -> bool {
    !token.is_empty() && token.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_class(token: &str) -> bool {
    matches!(
        token.to_ascii_uppercase().as_str(),
        "IN" | "CS" | "CH" | "HS"
    )
}

fn parse_ttl(token: &str) -> Result<u32, String> {
    if token.len() > 10 {
        return Err("bad ttl".into());
    }
    token.parse::<u32>().map_err(|_| "bad ttl".to_owned())
}

fn parse_u16(token: &str, what: &str) -> Result<u16, String> {
    if token.is_empty() || token.len() > 5 || !token.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("bad {what}"));
    }
    token.parse::<u16>().map_err(|_| format!("bad {what}"))
}

fn parse_ptr(tokens: &[String]) -> Result<RData, String> {
    if tokens.len() != 1 {
        return Err("PTR expects one name".into());
    }
    Ok(RData::Ptr(canon_name(&tokens[0])?))
}

fn parse_srv(tokens: &[String]) -> Result<RData, String> {
    if tokens.len() != 4 {
        return Err("SRV expects priority, weight, port, and target".into());
    }
    Ok(RData::Srv {
        priority: parse_u16(&tokens[0], "priority")?,
        weight: parse_u16(&tokens[1], "weight")?,
        port: parse_u16(&tokens[2], "port")?,
        target: canon_name(&tokens[3])?,
    })
}

fn parse_txt(tokens: &[String]) -> Result<RData, String> {
    if tokens.is_empty() {
        return Err("TXT expects at least one string".into());
    }
    if tokens.len() > MAX_TXT {
        return Err("too many TXT strings".into());
    }
    let mut pairs = Vec::new();
    for token in tokens {
        if token.is_empty() {
            return Err("empty TXT string".into());
        }
        if token.len() > 255 || !printable_ascii(token) {
            return Err("TXT string is too long or not printable ASCII".into());
        }
        let (key, value) = match token.split_once('=') {
            Some((key, value)) => (key, value),
            None => (token.as_str(), ""),
        };
        if key.is_empty()
            || key.len() > 63
            || !key
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        {
            return Err("bad TXT key".into());
        }
        if !printable_ascii(value) {
            return Err("bad TXT value".into());
        }
        pairs.push((key.to_owned(), value.to_owned()));
    }
    Ok(RData::Txt(pairs))
}

fn parse_a(tokens: &[String]) -> Result<RData, String> {
    if tokens.len() != 1 {
        return Err("A expects one address".into());
    }
    let addr: Ipv4Addr = tokens[0]
        .parse()
        .map_err(|_| "bad IPv4 address".to_owned())?;
    Ok(RData::A(addr.to_string()))
}

fn parse_aaaa(tokens: &[String]) -> Result<RData, String> {
    if tokens.len() != 1 {
        return Err("AAAA expects one address".into());
    }
    let addr: Ipv6Addr = tokens[0]
        .parse()
        .map_err(|_| "bad IPv6 address".to_owned())?;
    Ok(RData::Aaaa(addr.to_string()))
}

fn canon_name(raw: &str) -> Result<String, String> {
    if !raw.is_ascii() {
        return Err("name is not ASCII".into());
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "." {
        return Err("empty name".into());
    }
    let bare = trimmed.trim_end_matches('.');
    if bare.is_empty() {
        return Err("empty name".into());
    }
    let mut octets = 1usize;
    for label in bare.split('.') {
        if !valid_label(label) {
            return Err("bad name label".into());
        }
        octets += label.len() + 1;
        if octets > 255 {
            return Err("name is too long".into());
        }
    }
    let mut lower = bare.to_ascii_lowercase();
    lower.push('.');
    Ok(lower)
}

fn valid_label(label: &str) -> bool {
    if label.is_empty() || label.len() > 63 || label.starts_with('-') || label.ends_with('-') {
        return false;
    }
    label
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn printable_ascii(text: &str) -> bool {
    text.bytes().all(|byte| matches!(byte, b' '..=b'~'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_one(line: &str) -> Record {
        let mut records = parse_mdns_sd(line).unwrap();
        assert_eq!(records.len(), 1);
        records.pop().unwrap()
    }

    #[test]
    fn parses_ptr_srv_txt_and_addresses() {
        let text = "\
; comment only
_services._dns-sd._udp.local. IN PTR _openmind._tcp.local.

_OpenMind._TCP.local. 120 IN PTR Desk._OpenMind._TCP.local.
desk._openmind._tcp.local. IN SRV 0 0 7787 Desk.local.
desk._openmind._tcp.local. IN TXT \"v=1\" \"id=desk-1\" \"arch=x86_64\" \"caps=cpu,sandbox\" \"name=Desk\"
desk.local. IN A 192.0.2.10
desk.local. IN AAAA 2001:db8::10
desk._openmind._tcp.local. IN TXT \"note=a;b\"
";
        let records = parse_mdns_sd(text).unwrap();
        assert_eq!(records.len(), 7);
        assert_eq!(records[1].ttl, Some(120));
        assert_eq!(
            records[1].rdata,
            RData::Ptr("desk._openmind._tcp.local.".into())
        );
        match &records[2].rdata {
            RData::Srv {
                priority,
                weight,
                port,
                target,
            } => {
                assert_eq!((*priority, *weight, *port), (0, 0, 7787));
                assert_eq!(target, "desk.local.");
            }
            other => panic!("expected SRV, got {other:?}"),
        }
        let rows = fold_records(&records);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.id, "desk-1");
        assert_eq!(row.instance, "desk._openmind._tcp.local.");
        assert_eq!(row.service_type, "_openmind._tcp.local.");
        assert_eq!(row.host, "desk.local.");
        assert_eq!(row.port, Some(7787));
        assert_eq!(
            row.addrs,
            vec!["192.0.2.10".to_string(), "2001:db8::10".to_string()]
        );
        assert_eq!(row.txt.get("v").map(String::as_str), Some("1"));
        assert_eq!(row.txt.get("caps").map(String::as_str), Some("cpu,sandbox"));
        assert_eq!(row.txt.get("name").map(String::as_str), Some("Desk"));
        assert_eq!(row.txt.get("note").map(String::as_str), Some("a;b"));
        assert_eq!(row.origin, Origin::Mdns);
        assert_eq!(row.approval, APPROVAL);
    }

    #[test]
    fn enumeration_ptr_is_not_a_candidate() {
        let text = "_services._dns-sd._udp.local. IN PTR _openmind._tcp.local.\n";
        let rows = fold_records(&parse_mdns_sd(text).unwrap());
        assert!(rows.is_empty());
    }

    #[test]
    fn srv_without_ptr_still_folds() {
        let text = "solo._ollama._tcp.local. IN SRV 1 2 11434 solo.local.\n";
        let rows = fold_records(&parse_mdns_sd(text).unwrap());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "solo");
        assert_eq!(rows[0].service_type, "_ollama._tcp.local.");
        assert_eq!(rows[0].port, Some(11434));
        assert!(rows[0].addrs.is_empty());
    }

    #[test]
    fn later_txt_key_wins_and_unsafe_id_falls_back() {
        let text = "\
desk._openmind._tcp.local. IN TXT \"id=one\" \"v=0\"
desk._openmind._tcp.local. IN TXT \"id=has space\" \"v=1\"
";
        let rows = fold_records(&parse_mdns_sd(text).unwrap());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "desk");
        assert_eq!(rows[0].txt.get("v").map(String::as_str), Some("1"));
    }

    #[test]
    fn peers_sort_by_instance() {
        let text = "\
_openmind._tcp.local. IN PTR b._openmind._tcp.local.
_openmind._tcp.local. IN PTR a._openmind._tcp.local.
";
        let rows = fold_records(&parse_mdns_sd(text).unwrap());
        assert_eq!(
            rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
    }

    #[test]
    fn class_and_ttl_are_optional() {
        let rec = parse_one("phone.local. A 192.0.2.9\n");
        assert_eq!(rec.ttl, None);
        assert_eq!(rec.rdata, RData::A("192.0.2.9".into()));
        let rec = parse_one("phone.local. 30 AAAA 2001:db8::9\n");
        assert_eq!(rec.ttl, Some(30));
    }

    #[test]
    fn rejects_bad_records() {
        assert!(parse_mdns_sd("name IN PTR\n").is_err());
        assert!(parse_mdns_sd("name IN SRV 0 0\n").is_err());
        assert!(parse_mdns_sd("name IN A not-an-ip\n").is_err());
        assert!(parse_mdns_sd("name IN AAAA zz\n").is_err());
        assert!(parse_mdns_sd("name IN TXT\n").is_err());
        assert!(parse_mdns_sd("name CH A 192.0.2.1\n").is_err());
        assert!(parse_mdns_sd("name IN MX mail.local.\n").is_err());
        assert!(parse_mdns_sd("name IN PTR \"unterminated\n").is_err());
        assert!(parse_mdns_sd(&format!("{}.local. IN A 192.0.2.1\n", "a".repeat(64))).is_err());
        assert!(parse_mdns_sd(&format!("{}\n", "x".repeat(MAX_LINE_LEN + 1))).is_err());
        let cjk = format!(
            "_openmind._tcp.local. IN PTR {}._openmind._tcp.local.\n",
            "\u{6d4b}"
        );
        assert!(parse_mdns_sd(&cjk).is_err());
        let err = parse_mdns_sd("bad line here\n").unwrap_err();
        assert_eq!(err.line, 1);
    }

    #[test]
    fn comments_and_blank_lines_are_empty() {
        assert!(parse_mdns_sd("; only\n\n  ; still\n").unwrap().is_empty());
    }

    #[test]
    fn too_many_lines_errors() {
        let text = ";\n".repeat(MAX_LINES + 1);
        let err = parse_mdns_sd(&text).unwrap_err();
        assert_eq!(err.message, "too many lines");
    }
}
