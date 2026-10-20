// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio

//! Filter for a `git fast-export` stream.
//!
//! Every byte is passed through unchanged except file changes inside
//! commits (`M` filemodify, `D` filedelete) whose path matches the purge
//! list: those lines (and an `inline` data block that belongs to them) are
//! dropped. `data` blocks (commit and tag messages, blobs) are copied by
//! byte count, so a message that happens to contain `M ...` lines is never
//! touched. Renames and copies (`R`/`C`, only emitted with `-M`/`-C`, which
//! this tool never passes) are refused when they involve a listed path,
//! because dropping half of one would silently change the kept tree.
//!
//! Format reference: git-fast-import(1), "Input format".

use crate::pattern::PathSet;
use std::collections::BTreeMap;
use std::io::{self, BufRead, Read, Write};

/// What the filter saw.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Stats {
    /// `commit` commands passed through.
    pub commits: u64,
    /// `M` lines dropped.
    pub dropped_modify: u64,
    /// `D` lines dropped.
    pub dropped_delete: u64,
    /// File-change lines kept.
    pub kept_changes: u64,
    /// Dropped paths and how many file-change lines named each.
    pub dropped_paths: BTreeMap<Vec<u8>, u64>,
    /// Commits left with no file change (kept, not pruned).
    pub emptied_commits: u64,
}

fn bad(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

/// Undo git's C-style path quoting (`"a\tb\303\251"`). Input without a
/// leading quote is returned as is.
pub fn unquote(p: &[u8]) -> io::Result<Vec<u8>> {
    if p.first() != Some(&b'"') {
        return Ok(p.to_vec());
    }
    if p.len() < 2 || p[p.len() - 1] != b'"' {
        return Err(bad("unterminated quoted path"));
    }
    let inner = &p[1..p.len() - 1];
    let mut out = Vec::with_capacity(inner.len());
    let mut i = 0;
    while i < inner.len() {
        let c = inner[i];
        if c != b'\\' {
            out.push(c);
            i += 1;
            continue;
        }
        let e = *inner.get(i + 1).ok_or_else(|| bad("dangling backslash"))?;
        i += 2;
        out.push(match e {
            b'a' => 7,
            b'b' => 8,
            b'f' => 12,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 11,
            b'\\' => b'\\',
            b'"' => b'"',
            b'0'..=b'3' => {
                let d = inner
                    .get(i - 1..i + 2)
                    .ok_or_else(|| bad("short octal escape"))?;
                if !d.iter().all(|b| (b'0'..=b'7').contains(b)) {
                    return Err(bad("bad octal escape"));
                }
                i += 2;
                (d[0] - b'0') * 64 + (d[1] - b'0') * 8 + (d[2] - b'0')
            }
            _ => return Err(bad("unknown escape in quoted path")),
        });
    }
    Ok(out)
}

/// Split `src dst` of an `R`/`C` line into two raw (possibly quoted) paths.
fn split_two_paths(rest: &[u8]) -> io::Result<(&[u8], &[u8])> {
    if rest.first() == Some(&b'"') {
        let mut i = 1;
        while i < rest.len() {
            match rest[i] {
                b'\\' => i += 2,
                b'"' => break,
                _ => i += 1,
            }
        }
        if i >= rest.len() || rest.get(i + 1) != Some(&b' ') {
            return Err(bad("bad quoted source path"));
        }
        Ok((&rest[..=i], &rest[i + 2..]))
    } else {
        let sp = rest
            .iter()
            .position(|&b| b == b' ')
            .ok_or_else(|| bad("R/C line without destination"))?;
        Ok((&rest[..sp], &rest[sp + 1..]))
    }
}

fn strip_lf(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\n").unwrap_or(line)
}

/// Read one `data` block whose header line is `header` (with LF); returns
/// header + body bytes exactly as they appeared.
fn read_data<R: BufRead>(r: &mut R, header: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = header.to_vec();
    let arg = &strip_lf(header)[5..];
    if let Some(delim) = arg.strip_prefix(b"<<") {
        loop {
            let mut l = Vec::new();
            if r.read_until(b'\n', &mut l)? == 0 {
                return Err(bad("EOF inside delimited data"));
            }
            out.extend_from_slice(&l);
            if strip_lf(&l) == delim {
                break;
            }
        }
    } else {
        let n: u64 = std::str::from_utf8(arg)
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| bad("bad data length"))?;
        let start = out.len();
        r.take(n).read_to_end(&mut out)?;
        if (out.len() - start) as u64 != n {
            return Err(bad("EOF inside data block"));
        }
    }
    Ok(out)
}

/// Filter `input` (a fast-export stream) into `output`.
pub fn filter<R: BufRead, W: Write>(
    mut input: R,
    mut output: W,
    set: &PathSet,
) -> io::Result<Stats> {
    let mut st = Stats::default();
    let mut in_commit = false;
    let mut changes_in_commit = 0u64;
    let mut line = Vec::new();
    loop {
        line.clear();
        if input.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        let l = strip_lf(&line);
        if l.starts_with(b"data ") {
            let block = read_data(&mut input, &line)?;
            output.write_all(&block)?;
            continue;
        }
        let top_level = [
            &b"commit "[..],
            b"blob",
            b"tag ",
            b"reset ",
            b"progress ",
            b"feature ",
            b"option ",
            b"checkpoint",
            b"done",
            b"alias",
        ]
        .iter()
        .any(|k| l.starts_with(k));
        if top_level {
            if in_commit && changes_in_commit == 0 {
                st.emptied_commits += 1;
            }
            in_commit = l.starts_with(b"commit ");
            if in_commit {
                st.commits += 1;
                changes_in_commit = 0;
            }
            output.write_all(&line)?;
            continue;
        }
        if in_commit && l.starts_with(b"M ") {
            // M <mode> <dataref> <path>
            let mut it = l[2..].splitn(3, |&b| b == b' ');
            let (_mode, dataref, raw) = match (it.next(), it.next(), it.next()) {
                (Some(m), Some(d), Some(p)) => (m, d, p),
                _ => return Err(bad("malformed M line")),
            };
            let path = unquote(raw)?;
            let mut inline = None;
            if dataref == b"inline" {
                let mut h = Vec::new();
                input.read_until(b'\n', &mut h)?;
                if !h.starts_with(b"data ") {
                    return Err(bad("inline M without data"));
                }
                inline = Some(read_data(&mut input, &h)?);
            }
            if set.matches(&path) {
                st.dropped_modify += 1;
                *st.dropped_paths.entry(path).or_default() += 1;
            } else {
                st.kept_changes += 1;
                changes_in_commit += 1;
                output.write_all(&line)?;
                if let Some(b) = inline {
                    output.write_all(&b)?;
                }
            }
            continue;
        }
        if in_commit && l.starts_with(b"D ") {
            let path = unquote(&l[2..])?;
            if set.matches(&path) {
                st.dropped_delete += 1;
                *st.dropped_paths.entry(path).or_default() += 1;
            } else {
                st.kept_changes += 1;
                changes_in_commit += 1;
                output.write_all(&line)?;
            }
            continue;
        }
        if in_commit && (l.starts_with(b"R ") || l.starts_with(b"C ")) {
            let (a, b) = split_two_paths(&l[2..])?;
            if set.matches(&unquote(a)?) || set.matches(&unquote(b)?) {
                return Err(bad(
                    "rename/copy touching a listed path; run fast-export without -M/-C",
                ));
            }
            st.kept_changes += 1;
            changes_in_commit += 1;
            output.write_all(&line)?;
            continue;
        }
        if in_commit && l == b"deleteall" {
            changes_in_commit += 1;
        }
        output.write_all(&line)?;
    }
    if in_commit && changes_in_commit == 0 {
        st.emptied_commits += 1;
    }
    output.flush()?;
    Ok(st)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &[u8], pats: &[&str]) -> (Vec<u8>, Stats) {
        let mut set = PathSet::default();
        for p in pats {
            set.add(p).unwrap();
        }
        let mut out = Vec::new();
        let st = filter(input, &mut out, &set).unwrap();
        (out, st)
    }

    const OID: &str = "0123456789012345678901234567890123456789";

    #[test]
    fn drops_listed_paths_only() {
        let msg = "touch M 100644 x contrib/a\n";
        let s = format!(
            "feature done\nreset refs/heads/main\ncommit refs/heads/main\nmark :1\nauthor A <a@x> 1 +0200\ncommitter A <a@x> 1 +0200\ndata {}\n{msg}M 100644 {OID} contrib/a\nM 100644 {OID} src/lib.rs\nM 100644 {OID} \"stack/kms/sp ace\\303\\251.redacted\"\nD contrib/old\nD src/old.rs\n\ndone\n",
            msg.len()
        );
        let (out, st) = run(s.as_bytes(), &["contrib/", "stack/kms/"]);
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains(msg), "message kept byte for byte");
        assert!(!out.contains(&format!("{OID} contrib/a")));
        assert!(!out.contains("D contrib/old"));
        assert!(!out.contains("stack/kms"));
        assert!(out.contains("src/lib.rs"));
        assert!(out.contains("D src/old.rs"));
        assert!(out.ends_with("done\n"));
        assert_eq!(st.commits, 1);
        assert_eq!(st.dropped_modify, 2);
        assert_eq!(st.dropped_delete, 1);
        assert_eq!(st.kept_changes, 2);
        assert!(st
            .dropped_paths
            .contains_key("stack/kms/sp ace\u{e9}.redacted".as_bytes()));
    }

    #[test]
    fn inline_data_and_emptied_commit() {
        let s = "commit refs/heads/main\nmark :1\ncommitter A <a@x> 1 +0200\ndata 3\nhi\nM 100644 inline secret.redacted\ndata 4\nabc\n\ncommit refs/heads/main\nmark :2\ncommitter A <a@x> 2 +0200\ndata <<EOT\nheredoc M 100644 inline x\nEOT\nfrom :1\nM 100644 inline keep.txt\ndata 2\nk\n\n";
        let (out, st) = run(s.as_bytes(), &["*.redacted"]);
        let out = String::from_utf8(out).unwrap();
        assert!(!out.contains("secret.redacted"));
        assert!(!out.contains("abc"));
        assert!(out.contains("M 100644 inline keep.txt\ndata 2\nk\n"));
        assert!(out.contains("heredoc M 100644 inline x\nEOT\n"));
        assert_eq!(st.emptied_commits, 1);
        assert_eq!(st.commits, 2);
    }

    #[test]
    fn rename_touching_listed_path_is_refused() {
        let s = "commit refs/heads/main\ndata 0\nR contrib/a src/a\n";
        let set = {
            let mut p = PathSet::default();
            p.add("contrib/").unwrap();
            p
        };
        assert!(filter(s.as_bytes(), Vec::new(), &set).is_err());
        let ok = "commit refs/heads/main\ndata 0\nR \"a b\" c\n";
        assert!(filter(ok.as_bytes(), Vec::new(), &set).is_ok());
    }

    #[test]
    fn truncated_stream_is_an_error() {
        let s = "commit refs/heads/main\ndata 10\nshort";
        assert!(filter(s.as_bytes(), Vec::new(), &PathSet::default()).is_err());
    }

    #[test]
    fn unquote_cases() {
        assert_eq!(unquote(b"plain").unwrap(), b"plain");
        assert_eq!(unquote(b"\"a\\tb\\\\c\\\"\"").unwrap(), b"a\tb\\c\"");
        assert_eq!(unquote(b"\"\\303\\251\"").unwrap(), "\u{e9}".as_bytes());
        assert!(unquote(b"\"open").is_err());
        assert!(unquote(b"\"\\9\"").is_err());
    }
}
