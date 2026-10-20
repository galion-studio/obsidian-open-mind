// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Log sources: a rotation-safe auth.log tail and a journald follower.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use crate::state::FilePos;

/// One raw log entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// Entry time (unix seconds) when the source knows it (journald).
    /// File lines carry their stamp in `text`.
    pub time: Option<i64>,
    /// `host sshd[pid]: message` (journal) or the full syslog line (file).
    pub text: String,
    pub cursor: Option<String>,
    pub file_pos: Option<FilePos>,
}

/// Bytes read per poll at most (a huge backlog is read over several polls).
const READ_CHUNK: usize = 4 << 20;
/// A partial line longer than this is dropped (not an sshd line).
const MAX_PARTIAL: usize = 64 * 1024;

/// Tail one file across rename rotation (`auth.log` -> `auth.log.1`) and
/// copy-truncate: the old file is drained to EOF before switching, so no
/// line written just before the rotation is lost.
pub struct FileTail {
    path: PathBuf,
    file: Option<File>,
    dev: u64,
    ino: u64,
    pos: u64,
    partial: Vec<u8>,
    resume: Option<FilePos>,
}

impl FileTail {
    /// `resume`: saved position; used only if the same file (dev, inode)
    /// is still at `path` and is not shorter than the position.
    pub fn new(path: PathBuf, resume: Option<FilePos>) -> Self {
        FileTail {
            path,
            file: None,
            dev: 0,
            ino: 0,
            pos: 0,
            partial: Vec::new(),
            resume,
        }
    }

    pub fn position(&self) -> FilePos {
        FilePos {
            dev: self.dev,
            ino: self.ino,
            pos: self.pos - self.partial.len() as u64,
        }
    }

    fn open(&mut self) -> std::io::Result<bool> {
        let mut f = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        let md = f.metadata()?;
        let mut start = 0;
        if let Some(r) = self.resume.take() {
            if r.dev == md.dev() && r.ino == md.ino() && r.pos <= md.len() {
                start = r.pos;
            }
        }
        f.seek(SeekFrom::Start(start))?;
        self.dev = md.dev();
        self.ino = md.ino();
        self.pos = start;
        self.partial.clear();
        self.file = Some(f);
        Ok(true)
    }

    /// Complete new lines since the last poll.
    pub fn poll(&mut self) -> std::io::Result<Vec<LogLine>> {
        let mut out = Vec::new();
        if self.file.is_none() && !self.open()? {
            return Ok(out);
        }
        let eof = self.read_some(&mut out)?;
        if eof {
            // Reached the end of what we hold: did the path move on?
            match std::fs::metadata(&self.path) {
                Ok(md) if md.dev() != self.dev || md.ino() != self.ino => {
                    // Rotated: the old file is drained; a partial last line
                    // in it is flushed as a line (logrotate closes it).
                    self.flush_partial(&mut out);
                    self.file = None;
                    if self.open()? {
                        self.read_some(&mut out)?;
                    }
                }
                Ok(md) if md.len() < self.pos => {
                    // Copy-truncate.
                    self.partial.clear();
                    if let Some(f) = self.file.as_mut() {
                        f.seek(SeekFrom::Start(0))?;
                    }
                    self.pos = 0;
                    self.read_some(&mut out)?;
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // Moved away and not recreated yet: keep the old handle.
                }
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }

    fn flush_partial(&mut self, out: &mut Vec<LogLine>) {
        if !self.partial.is_empty() {
            let text = String::from_utf8_lossy(&self.partial).into_owned();
            self.partial.clear();
            out.push(LogLine {
                time: None,
                text,
                cursor: None,
                file_pos: Some(self.position()),
            });
        }
    }

    /// Read up to READ_CHUNK bytes; true when EOF was reached.
    fn read_some(&mut self, out: &mut Vec<LogLine>) -> std::io::Result<bool> {
        let Some(f) = self.file.as_mut() else {
            return Ok(true);
        };
        let mut buf = vec![0u8; 64 * 1024];
        let mut total = 0;
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                return Ok(true);
            }
            total += n;
            self.pos += n as u64;
            let mut chunk = &buf[..n];
            while let Some(i) = chunk.iter().position(|b| *b == b'\n') {
                self.partial.extend_from_slice(&chunk[..i]);
                chunk = &chunk[i + 1..];
                let consumed_to = self.pos - chunk.len() as u64;
                let text = String::from_utf8_lossy(&self.partial).into_owned();
                self.partial.clear();
                out.push(LogLine {
                    time: None,
                    text,
                    cursor: None,
                    file_pos: Some(FilePos {
                        dev: self.dev,
                        ino: self.ino,
                        pos: consumed_to,
                    }),
                });
            }
            self.partial.extend_from_slice(chunk);
            if self.partial.len() > MAX_PARTIAL {
                self.partial.clear();
            }
            if total >= READ_CHUNK {
                return Ok(false);
            }
        }
    }
}

/// `journalctl --follow --output=json` child process.
pub struct Journal {
    pub child: Child,
    pub reader: BufReader<std::process::ChildStdout>,
}

pub fn journal_args(matches: &[String], cursor: Option<&str>, since: i64) -> Vec<String> {
    let mut a = vec![
        "--follow".to_string(),
        "--output=json".into(),
        "--no-pager".into(),
        "--quiet".into(),
        "--lines=all".into(),
    ];
    match cursor {
        Some(c) => a.push(format!("--after-cursor={c}")),
        None => a.push(format!("--since=@{since}")),
    }
    a.extend(matches.iter().cloned());
    a
}

impl Journal {
    pub fn spawn(bin: &PathBuf, args: &[String]) -> std::io::Result<Journal> {
        let mut child = Command::new(bin)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let out = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("no stdout"))?;
        Ok(Journal {
            child,
            reader: BufReader::new(out),
        })
    }

    /// Next entry; None at EOF (journalctl exited).
    pub fn next_line(&mut self) -> std::io::Result<Option<LogLine>> {
        let mut buf = Vec::new();
        loop {
            buf.clear();
            let n = self.reader.read_until(b'\n', &mut buf)?;
            if n == 0 {
                return Ok(None);
            }
            if buf.len() > 1 << 20 {
                continue;
            }
            if let Some(l) = parse_journal_json(&String::from_utf8_lossy(&buf)) {
                return Ok(Some(l));
            }
        }
    }
}

impl Drop for Journal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn field_str(v: &serde_json::Value, k: &str) -> Option<String> {
    match v.get(k)? {
        serde_json::Value::String(s) => Some(s.clone()),
        // Non-UTF-8 or binary fields are arrays of byte values.
        serde_json::Value::Array(a) => {
            let bytes: Vec<u8> = a
                .iter()
                .filter_map(|x| x.as_u64())
                .map(|x| x as u8)
                .collect();
            Some(String::from_utf8_lossy(&bytes).into_owned())
        }
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// One `journalctl -o json` line into `host ident[pid]: message`.
pub fn parse_journal_json(line: &str) -> Option<LogLine> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let msg = field_str(&v, "MESSAGE")?;
    let t = field_str(&v, "__REALTIME_TIMESTAMP")
        .and_then(|s| s.parse::<i64>().ok())
        .map(|us| us / 1_000_000);
    let host = field_str(&v, "_HOSTNAME").unwrap_or_else(|| "localhost".into());
    let ident = field_str(&v, "SYSLOG_IDENTIFIER")
        .or_else(|| field_str(&v, "_COMM"))
        .unwrap_or_default();
    let pid = field_str(&v, "SYSLOG_PID").or_else(|| field_str(&v, "_PID"));
    let host: String = host.chars().filter(|c| !c.is_whitespace()).collect();
    let ident: String = ident.chars().filter(|c| !c.is_whitespace()).collect();
    let msg = msg.replace(['\n', '\r'], " ");
    let text = match pid {
        Some(p) => format!("{host} {ident}[{p}]: {msg}"),
        None => format!("{host} {ident}: {msg}"),
    };
    Some(LogLine {
        time: t,
        text,
        cursor: field_str(&v, "__CURSOR"),
        file_pos: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("galion-guard-src-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn texts(v: Vec<LogLine>) -> Vec<String> {
        v.into_iter().map(|l| l.text).collect()
    }
    fn append(p: &PathBuf, s: &str) {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .unwrap();
        f.write_all(s.as_bytes()).unwrap();
    }

    #[test]
    fn tail_partial_rename_rotation_and_truncate() {
        let d = tmp("tail");
        let p = d.join("auth.log");
        let mut t = FileTail::new(p.clone(), None);
        assert!(t.poll().unwrap().is_empty()); // missing file is fine
        append(&p, "a\nb");
        assert_eq!(texts(t.poll().unwrap()), vec!["a"]);
        append(&p, "c\nd\n");
        assert_eq!(texts(t.poll().unwrap()), vec!["bc", "d"]);
        // Rename rotation with a line written to the old file after the move.
        std::fs::rename(&p, d.join("auth.log.1")).unwrap();
        append(&d.join("auth.log.1"), "late\n");
        append(&p, "new1\n");
        assert_eq!(texts(t.poll().unwrap()), vec!["late", "new1"]);
        // Copy-truncate.
        std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&p)
            .unwrap();
        append(&p, "x\n");
        assert_eq!(texts(t.poll().unwrap()), vec!["x"]);
        let pos = t.position();
        // Resume after restart from the saved position.
        append(&p, "y\n");
        let mut t2 = FileTail::new(p.clone(), Some(pos.clone()));
        assert_eq!(texts(t2.poll().unwrap()), vec!["y"]);
        // A stale position (other inode) restarts at 0.
        let stale = FilePos {
            ino: pos.ino + 999,
            ..pos
        };
        let mut t3 = FileTail::new(p.clone(), Some(stale));
        assert_eq!(texts(t3.poll().unwrap()), vec!["x", "y"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn journal_json() {
        let l = parse_journal_json(r#"{"__CURSOR":"s=1;i=2","__REALTIME_TIMESTAMP":"1792522154028938","_HOSTNAME":"ip50","SYSLOG_IDENTIFIER":"sshd","_PID":"77","MESSAGE":"Invalid user a from 203.0.113.1 port 1"}"#).unwrap();
        assert_eq!(
            l.text,
            "ip50 sshd[77]: Invalid user a from 203.0.113.1 port 1"
        );
        assert_eq!(l.time, Some(1_792_522_154));
        assert_eq!(l.cursor.as_deref(), Some("s=1;i=2"));
        let b = parse_journal_json(r#"{"_HOSTNAME":"h","_COMM":"sshd","MESSAGE":[104,105,255]}"#)
            .unwrap();
        assert_eq!(b.text, "h sshd: hi\u{fffd}");
        assert!(parse_journal_json("not json").is_none());
        assert!(parse_journal_json(r#"{"_HOSTNAME":"h"}"#).is_none());
        let a = journal_args(&["_COMM=sshd".into()], None, 100);
        assert!(a.contains(&"--since=@100".to_string()));
        let a = journal_args(&[], Some("s=1"), 100);
        assert!(a.contains(&"--after-cursor=s=1".to_string()));
    }
}
