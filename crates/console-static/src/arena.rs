// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `GET /api/arena` reader for `--local-api` (`--arena-root DIR`).
//!
//! Rust port of the read-only `status()` of the retired arena script
//! (`/workspace/galion-light/arena/arena.py status`, read as text on the VPS
//! 2026-10-28 01:20 CET +01:00; the Node console ran it with `python3`). It
//! only reads files under the arena root and never runs anything:
//!
//! * `board.jsonl`: the last 16 lines (blank and bad lines skipped);
//! * `champions/current.json`: the champion, `null` when missing or bad;
//! * `spend.json`: spend, or the script's default when missing or bad;
//! * `STOP`: present = stopped.
//!
//! Reply shape (key order as the script): `ok, stopped, spend, champion,
//! rows, hermes, note`. Breed / Promote / Stop (the script's `generation`,
//! `promote`, `kill`) are NOT ported: Breed is a paid run, so the API answers
//! 405 to every non-GET (owner decision, see docs/console-port-1.md).
//!
//! Differences from the script, on purpose: a board file larger than
//! [`MAX_BOARD`] is read from its tail only (the script read it all); a file
//! that cannot be read (permissions, not UTF-8) gives 500
//! `{"ok":false,"error":"arena read failed"}` without the path or OS error
//! (the Node route sent the Python exception text).

use console_ui::json::{self, Value};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// Board lines returned (`splitlines()[-16:]`).
pub const BOARD_ROWS: usize = 16;
/// Largest board tail read.
pub const MAX_BOARD: u64 = 1024 * 1024;
/// Largest `spend.json` / `champions/current.json` read.
pub const MAX_SMALL: u64 = 256 * 1024;
/// The script's `CAP_USD` / `MAX_GENS` defaults.
pub const CAP_USD: f64 = 2.0;
pub const MAX_GENS: f64 = 3.0;

#[derive(Debug)]
pub struct ReadFailed;

impl From<io::Error> for ReadFailed {
    fn from(_: io::Error) -> Self {
        ReadFailed
    }
}

/// Whole file as UTF-8 if it exists (`None` = missing). Larger than `cap`
/// counts as unreadable.
fn read_small(path: &Path, cap: u64) -> Result<Option<String>, ReadFailed> {
    let f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ReadFailed),
    };
    let mut buf = Vec::new();
    f.take(cap + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > cap {
        return Err(ReadFailed);
    }
    String::from_utf8(buf).map(Some).map_err(|_| ReadFailed)
}

/// Board text: whole file, or its last [`MAX_BOARD`] bytes starting after
/// the first newline (so no half line is parsed).
fn read_board(path: &Path) -> Result<Option<String>, ReadFailed> {
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ReadFailed),
    };
    let len = f.metadata()?.len();
    let cut = len > MAX_BOARD;
    if cut {
        f.seek(SeekFrom::Start(len - MAX_BOARD))?;
    }
    let mut buf = Vec::new();
    f.take(MAX_BOARD).read_to_end(&mut buf)?;
    if cut {
        match buf.iter().position(|b| *b == b'\n') {
            Some(i) => {
                buf.drain(..=i);
            }
            None => buf.clear(),
        }
    }
    String::from_utf8(buf).map(Some).map_err(|_| ReadFailed)
}

/// The script's default spend row.
pub fn default_spend() -> Value {
    json::obj(&[
        ("cap_usd", Value::Num(CAP_USD)),
        ("spent_usd", Value::Num(0.0)),
        ("generations", Value::Num(0.0)),
        ("max_generations", Value::Num(MAX_GENS)),
    ])
}

/// Last [`BOARD_ROWS`] lines, then blank / non-JSON lines dropped (the
/// script's order: slice first, then filter).
pub fn board_rows(text: &str) -> Vec<Value> {
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(BOARD_ROWS);
    lines[from..]
        .iter()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| json::parse(l.as_bytes()).ok())
        .collect()
}

/// `arena.py status` as JSON.
pub fn status(root: &Path) -> Result<Value, ReadFailed> {
    let rows = match read_board(&root.join("board.jsonl"))? {
        Some(t) => board_rows(&t),
        None => Vec::new(),
    };
    let champion = match read_small(&root.join("champions").join("current.json"), MAX_SMALL)? {
        Some(t) => json::parse(t.as_bytes()).unwrap_or(Value::Null),
        None => Value::Null,
    };
    let spend = match read_small(&root.join("spend.json"), MAX_SMALL)? {
        Some(t) => json::parse(t.as_bytes()).unwrap_or_else(|_| default_spend()),
        None => default_spend(),
    };
    let stopped = root.join("STOP").exists();
    Ok(json::obj(&[
        ("ok", Value::Bool(true)),
        ("stopped", Value::Bool(stopped)),
        ("spend", spend),
        ("champion", champion),
        ("rows", Value::Arr(rows)),
        ("hermes", Value::Str("intern".into())),
        ("note", Value::Str("Promote does not edit SOUL.md".into())),
    ]))
}

/// `(status, body)` for `GET /api/arena`.
pub fn reply(root: &Path) -> (u16, String) {
    match status(root) {
        Ok(v) => (200, v.to_json()),
        Err(ReadFailed) => (500, r#"{"ok":false,"error":"arena read failed"}"#.into()),
    }
}

/// Body for a non-GET on a read-only data route.
pub const HELD: &str = r#"{"ok":false,"error":"read-only: arena actions (breed, promote, stop) are held until the owner approves the spend"}"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("arena-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn empty_root_is_default_shape() {
        let d = tmp("empty");
        let (c, b) = reply(&d);
        assert_eq!(c, 200);
        assert_eq!(
            b,
            r#"{"ok":true,"stopped":false,"spend":{"cap_usd":2,"spent_usd":0,"generations":0,"max_generations":3},"champion":null,"rows":[],"hermes":"intern","note":"Promote does not edit SOUL.md"}"#
        );
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn last_16_then_filter() {
        let mut t = String::new();
        for i in 0..20 {
            t.push_str(&format!("{{\"id\":\"g{i}\",\"generation\":1}}\n"));
        }
        t.push_str("\n   \nnot json\n");
        let rows = board_rows(&t);
        // 23 lines: last 16 = g7..g19 (13) + 3 junk lines dropped.
        assert_eq!(rows.len(), 13);
        assert_eq!(rows[0].get("id").as_str(), Some("g7"));
        assert_eq!(rows[12].get("id").as_str(), Some("g19"));
        assert_eq!(board_rows("{\"a\":1}\r\n{\"a\":2}").len(), 2);
    }

    #[test]
    fn full_root() {
        let d = tmp("full");
        std::fs::create_dir_all(d.join("champions")).unwrap();
        std::fs::write(d.join("board.jsonl"), "{\"id\":\"g1-0\",\"score\":7}\n").unwrap();
        std::fs::write(
            d.join("champions/current.json"),
            "{\"id\":\"g1-0\",\"voice\":\"short\"}",
        )
        .unwrap();
        std::fs::write(
            d.join("spend.json"),
            "{\"cap_usd\":2.0,\"spent_usd\":0.25}\n",
        )
        .unwrap();
        std::fs::write(d.join("STOP"), "").unwrap();
        let v = status(&d).unwrap();
        assert_eq!(v.get("stopped").as_bool(), Some(true));
        assert_eq!(v.get("spend").get("spent_usd").as_f64(), Some(0.25));
        assert_eq!(v.get("champion").get("voice").as_str(), Some("short"));
        assert_eq!(v.get("rows").as_array().len(), 1);
        // Bad JSON: champion null, spend default (as the script).
        std::fs::write(d.join("champions/current.json"), "{oops").unwrap();
        std::fs::write(d.join("spend.json"), "nope").unwrap();
        let v = status(&d).unwrap();
        assert!(v.get("champion").is_null());
        assert_eq!(v.get("spend"), &default_spend());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn unreadable_is_500_without_path() {
        let d = tmp("bad");
        std::fs::write(d.join("spend.json"), [0xffu8, 0xfe, b'{']).unwrap();
        let (c, b) = reply(&d);
        assert_eq!(c, 500);
        assert!(!b.contains(d.to_str().unwrap()));
        // A directory where a file should be is also a read failure.
        std::fs::remove_file(d.join("spend.json")).unwrap();
        std::fs::create_dir_all(d.join("board.jsonl")).unwrap();
        assert_eq!(reply(&d).0, 500);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn big_board_reads_tail_only() {
        let d = tmp("big");
        let mut t = String::new();
        let mut i = 0;
        while (t.len() as u64) < MAX_BOARD + 4096 {
            t.push_str(&format!(
                "{{\"id\":\"g{i}\",\"pad\":\"{}\"}}\n",
                "x".repeat(200)
            ));
            i += 1;
        }
        std::fs::write(d.join("board.jsonl"), &t).unwrap();
        let v = status(&d).unwrap();
        let rows = v.get("rows").as_array();
        assert_eq!(rows.len(), 16);
        assert_eq!(
            rows[15].get("id").as_str(),
            Some(format!("g{}", i - 1).as_str())
        );
        let _ = std::fs::remove_dir_all(d);
    }
}
