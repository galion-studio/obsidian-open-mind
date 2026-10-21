// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Append-only, hash-chained audit log (`audit.jsonl`, mode 0600).
//!
//! Each line is one JSON object; `hash` = SHA-256 of the line up to (not including) `,"hash":`,
//! and that prefix contains `prev` = the previous line's hash. Editing, deleting or reordering a
//! line breaks the chain and `verify` names the first bad line. Truncating the newest lines is
//! only detectable against a remembered head hash (`verify` prints it; see docs/admin/galion-admin.md).
//!
//! What is recorded: action, target name, result, and a short non-secret detail. Payload
//! arguments are NEVER recorded (only the program's base name and the argument count), so a
//! token on a command line cannot leak into the log. Times are local with an explicit offset.

use crate::json::{escape, get, parse_flat};
use crate::{sha256, timefmt};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::Path;

pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const HASH_KEY: &str = ",\"hash\":\"";
const MAX_FIELD: usize = 200;

/// One parsed record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub seq: u64,
    pub ts: String,
    pub uid: u64,
    pub action: String,
    pub target: String,
    pub result: String,
    pub detail: String,
    pub prev: String,
    pub hash: String,
}

/// Cap a field and drop control characters (keeps the log one line per record, bounded).
pub fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(MAX_FIELD)
        .collect()
}

/// Exclusive advisory lock held for the life of the guard (closing the fd releases it).
pub struct Locked(pub File);

pub fn lock_exclusive(f: File) -> io::Result<Locked> {
    loop {
        // SAFETY: flock on a valid fd we own.
        let r = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) };
        if r == 0 {
            return Ok(Locked(f));
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

fn last_line(f: &mut File) -> io::Result<Option<String>> {
    let len = f.seek(SeekFrom::End(0))?;
    if len == 0 {
        return Ok(None);
    }
    let mut back = 4096u64.min(len);
    loop {
        f.seek(SeekFrom::Start(len - back))?;
        let mut buf = Vec::with_capacity(back as usize);
        Read::take(&mut *f, back).read_to_end(&mut buf)?;
        let body = buf.strip_suffix(b"\n").unwrap_or(&buf);
        if let Some(p) = body.iter().rposition(|&c| c == b'\n') {
            return Ok(Some(String::from_utf8_lossy(&body[p + 1..]).into_owned()));
        }
        if back == len {
            return Ok(Some(String::from_utf8_lossy(body).into_owned()));
        }
        back = (back * 2).min(len);
    }
}

/// Build the hashed body (everything before `,"hash":`).
pub fn body(r: &Record) -> String {
    format!(
        "{{\"v\":1,\"seq\":{},\"ts\":\"{}\",\"uid\":{},\"action\":\"{}\",\"target\":\"{}\",\"result\":\"{}\",\"detail\":\"{}\",\"prev\":\"{}\"",
        r.seq,
        escape(&r.ts),
        r.uid,
        escape(&r.action),
        escape(&r.target),
        escape(&r.result),
        escape(&r.detail),
        escape(&r.prev)
    )
}

/// Parse and hash-check one line in isolation (chain linkage is checked by `verify`).
pub fn parse_line(line: &str) -> Result<Record, String> {
    let idx = line.rfind(HASH_KEY).ok_or("no hash field")?;
    let obj = parse_flat(line)?;
    let s = |k: &str| -> Result<String, String> {
        get(&obj, k)
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| format!("missing string field {k}"))
    };
    let n = |k: &str| -> Result<u64, String> {
        get(&obj, k)
            .and_then(|v| v.as_u64())
            .ok_or_else(|| format!("missing number field {k}"))
    };
    if n("v")? != 1 {
        return Err("unknown record version".into());
    }
    let r = Record {
        seq: n("seq")?,
        ts: s("ts")?,
        uid: n("uid")?,
        action: s("action")?,
        target: s("target")?,
        result: s("result")?,
        detail: s("detail")?,
        prev: s("prev")?,
        hash: s("hash")?,
    };
    if obj.len() != 10 || !line.ends_with("\"}") || line[..idx] != body(&r) {
        return Err("line is not in canonical form".into());
    }
    if sha256::hex(&line.as_bytes()[..idx]) != r.hash {
        return Err("hash mismatch (line edited)".into());
    }
    Ok(r)
}

/// Fail-closed check before an action: the newest record must parse and hash-check.
pub fn check_tail(path: &Path) -> io::Result<()> {
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    match last_line(&mut f)? {
        None => Ok(()),
        Some(l) => parse_line(&l).map(|_| ()).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "audit log tail is damaged ({e}); refusing, run `galion-admin audit verify`"
                ),
            )
        }),
    }
}

/// Append one record under an exclusive lock, fsync it, and return it.
pub fn append(
    path: &Path,
    action: &str,
    target: &str,
    result: &str,
    detail: &str,
) -> io::Result<Record> {
    let f = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC)
        .open(path)?;
    let mut g = lock_exclusive(f)?;
    let (seq, prev) = match last_line(&mut g.0)? {
        None => (1, GENESIS.to_string()),
        Some(l) => match parse_line(&l) {
            Ok(p) => (p.seq + 1, p.hash),
            Err(e) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("audit log tail is damaged ({e}); refusing to append, run `galion-admin audit verify`"),
                ))
            }
        },
    };
    // SAFETY: trivial getter.
    let uid = unsafe { libc::getuid() } as u64;
    let mut r = Record {
        seq,
        ts: timefmt::now_local(),
        uid,
        action: clean(action),
        target: clean(target),
        result: clean(result),
        detail: clean(detail),
        prev,
        hash: String::new(),
    };
    let b = body(&r);
    r.hash = sha256::hex(b.as_bytes());
    let line = format!("{b}{HASH_KEY}{}\"}}\n", r.hash);
    g.0.write_all(line.as_bytes())?;
    g.0.sync_data()?;
    Ok(r)
}

/// Outcome of `verify`.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Ok { records: u64, head: String },
    Broken { line: usize, reason: String },
}

/// Check every line's hash, the `prev` chain and the sequence numbers.
pub fn verify_str(text: &str) -> Verdict {
    let mut prev = GENESIS.to_string();
    let mut seq = 0u64;
    for (i, line) in text.lines().enumerate() {
        let r = match parse_line(line) {
            Ok(r) => r,
            Err(reason) => {
                return Verdict::Broken {
                    line: i + 1,
                    reason,
                }
            }
        };
        if r.prev != prev {
            return Verdict::Broken {
                line: i + 1,
                reason: "prev does not match the previous line's hash (line removed or reordered)"
                    .into(),
            };
        }
        if r.seq != seq + 1 {
            return Verdict::Broken {
                line: i + 1,
                reason: format!("sequence gap: expected {}, found {}", seq + 1, r.seq),
            };
        }
        prev = r.hash;
        seq = r.seq;
    }
    Verdict::Ok {
        records: seq,
        head: prev,
    }
}

pub fn verify(path: &Path) -> io::Result<Verdict> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(verify_str(&t)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(verify_str("")),
        Err(e) => Err(e),
    }
}

/// Human one-line rendering used by `audit tail`.
pub fn render(r: &Record) -> String {
    format!(
        "{} #{} uid={} {} {} {}{}",
        r.ts,
        r.seq,
        r.uid,
        r.action,
        if r.target.is_empty() { "-" } else { &r.target },
        r.result,
        if r.detail.is_empty() {
            String::new()
        } else {
            format!(" ({})", r.detail)
        }
    )
}

/// The last `n` raw lines of the log.
pub fn tail_lines(path: &Path, n: usize) -> io::Result<Vec<String>> {
    let t = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let v: Vec<&str> = t.lines().collect();
    Ok(v[v.len().saturating_sub(n)..]
        .iter()
        .map(|s| s.to_string())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("galion-admin-ut-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("audit.jsonl")
    }

    #[test]
    fn chain_appends_and_verifies() {
        let p = tmp("chain");
        for i in 0..5 {
            append(
                &p,
                "sandbox.start",
                &format!("box{i}"),
                "ok",
                "cmd=sleep argc=2",
            )
            .unwrap();
        }
        match verify(&p).unwrap() {
            Verdict::Ok { records, head } => {
                assert_eq!(records, 5);
                assert_eq!(head.len(), 64);
            }
            v => panic!("{v:?}"),
        }
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }

    #[test]
    fn tamper_edit_delete_reorder_detected() {
        let p = tmp("tamper");
        for i in 0..4 {
            append(&p, "a", &format!("t{i}"), "ok", "").unwrap();
        }
        let t = std::fs::read_to_string(&p).unwrap();
        let lines: Vec<&str> = t.lines().collect();
        // edit
        let edited = t.replacen("\"t2\"", "\"tX\"", 1);
        assert!(matches!(
            verify_str(&edited),
            Verdict::Broken { line: 3, .. }
        ));
        // delete a middle line
        let del = format!("{}\n{}\n{}\n", lines[0], lines[1], lines[3]);
        assert!(matches!(verify_str(&del), Verdict::Broken { line: 3, .. }));
        // reorder
        let re = format!("{}\n{}\n{}\n{}\n", lines[0], lines[2], lines[1], lines[3]);
        assert!(matches!(verify_str(&re), Verdict::Broken { line: 2, .. }));
        // garbage line refuses further appends
        std::fs::write(&p, format!("{t}not json\n")).unwrap();
        assert!(append(&p, "a", "", "ok", "").is_err());
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }

    #[test]
    fn fields_are_cleaned_and_bounded() {
        let p = tmp("clean");
        let r = append(&p, "x\ny", &"z".repeat(1000), "ok\u{1b}[31m", "a\"b").unwrap();
        assert_eq!(r.action, "xy");
        assert_eq!(r.target.len(), MAX_FIELD);
        assert_eq!(r.result, "ok[31m");
        let t = std::fs::read_to_string(&p).unwrap();
        assert_eq!(t.lines().count(), 1);
        assert_eq!(parse_line(t.trim_end()).unwrap(), r);
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }

    #[test]
    fn concurrent_writers_keep_the_chain() {
        let p = tmp("conc");
        let hs: Vec<_> = (0..4)
            .map(|t| {
                let p = p.clone();
                std::thread::spawn(move || {
                    for i in 0..25 {
                        append(&p, "c", &format!("{t}-{i}"), "ok", "").unwrap();
                    }
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
        assert!(matches!(
            verify(&p).unwrap(),
            Verdict::Ok { records: 100, .. }
        ));
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
}
