// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

#![forbid(unsafe_code)]

use openmind_nerve::{read_all, rotation_path, Event, Writer, MAX_KEEP};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("openmind-nerve-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sample(source: &str) -> Event {
    Event::new(source, "beat", "info", r#"{"ok":true}"#).unwrap()
}

fn assert_zagreb(ts: &str) {
    assert_eq!(ts.len(), 25, "{ts}");
    assert_eq!(ts.as_bytes().get(10), Some(&b'T'), "{ts}");
    assert!(
        ts.ends_with("+01:00") || ts.ends_with("+02:00"),
        "missing Zagreb offset: {ts}"
    );
    assert!(!ts.ends_with('Z'), "{ts}");
    assert!(!ts.contains("+00:00"), "{ts}");
    assert!(!ts.contains('Z'), "{ts}");
}

#[test]
fn open_creates_empty_file_at_caller_path() {
    let dir = Scratch::new();
    let path = dir.path("nerve.jsonl");
    let writer = Writer::open(&path, 4096, 3).unwrap();
    assert_eq!(writer.path(), path.as_path());
    assert_eq!(writer.max_bytes(), 4096);
    assert_eq!(writer.keep(), 3);
    assert_eq!(fs::metadata(&path).unwrap().len(), 0);
    assert!(read_all(&path).unwrap().is_empty());
}

#[test]
fn append_stamps_zagreb_and_does_not_rewrite_older_bytes() {
    let dir = Scratch::new();
    let path = dir.path("nerve.jsonl");
    let prior = "{\"ts\":\"2026-10-18T18:56:00+02:00\",\"source\":\"old\",\"kind\":\"note\",\"severity\":\"info\",\"data\":null}\n";
    fs::write(&path, prior).unwrap();

    let before = zagreb_time::now_rfc3339();
    let mut writer = Writer::open(&path, 0, 0).unwrap();
    let event = Event::new("new", "note", "warn", "null").unwrap();
    writer.append(&event).unwrap();
    let after = zagreb_time::now_rfc3339();

    let raw = fs::read(&path).unwrap();
    assert!(
        raw.starts_with(prior.as_bytes()),
        "existing bytes were rewritten"
    );
    let events = read_all(&path).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].ts(), "2026-10-18T18:56:00+02:00");
    assert_eq!(events[0].source(), "old");
    assert_eq!(events[0].data_json(), "null");
    assert_eq!(events[1].source(), "new");
    assert_eq!(events[1].kind(), "note");
    assert_eq!(events[1].severity(), "warn");
    assert_eq!(events[1].data_json(), "null");
    assert_ne!(events[1].ts(), events[0].ts());
    let ts = events[1].ts();
    assert!(
        ts == before || ts == after,
        "{ts} not between {before} and {after}"
    );
    assert_zagreb(ts);

    let line = std::str::from_utf8(&raw[prior.len()..]).unwrap();
    assert_eq!(line.matches('\n').count(), 1);
    assert!(!line.contains(": "));
    let ts_at = line.find("\"ts\":").unwrap();
    let source_at = line.find("\"source\":").unwrap();
    let kind_at = line.find("\"kind\":").unwrap();
    let severity_at = line.find("\"severity\":").unwrap();
    let data_at = line.find("\"data\":").unwrap();
    assert!(
        ts_at < source_at && source_at < kind_at && kind_at < severity_at && severity_at < data_at
    );
}

#[test]
fn caller_paths_stay_separate() {
    let dir = Scratch::new();
    let left = dir.path("a.jsonl");
    let right = dir.path("b.jsonl");
    let mut a = Writer::open(&left, 0, 1).unwrap();
    let mut b = Writer::open(&right, 0, 1).unwrap();
    a.append(&sample("alpha")).unwrap();
    b.append(&sample("beta")).unwrap();
    assert_eq!(read_all(&left).unwrap()[0].source(), "alpha");
    assert_eq!(read_all(&right).unwrap()[0].source(), "beta");
    assert!(!rotation_path(&left, 1).exists());
}

#[test]
fn size_rotation_keeps_newest_siblings_and_preserves_bytes() {
    let dir = Scratch::new();
    let path = dir.path("nerve.jsonl");
    let mut writer = Writer::open(&path, 1, 2).unwrap();

    writer.append(&sample("one")).unwrap();
    let first = fs::read(&path).unwrap();
    let first_ts = read_all(&path).unwrap()[0].ts().to_string();
    assert_eq!(first.iter().filter(|byte| **byte == b'\n').count(), 1);
    assert!(!rotation_path(&path, 1).exists());

    writer.append(&sample("two")).unwrap();
    assert_eq!(fs::read(rotation_path(&path, 1)).unwrap(), first);
    assert_eq!(
        read_all(&rotation_path(&path, 1)).unwrap()[0].ts(),
        first_ts
    );
    let second = fs::read(&path).unwrap();
    assert_eq!(read_all(&path).unwrap()[0].source(), "two");
    assert_ne!(second, first);

    writer.append(&sample("three")).unwrap();
    assert_eq!(fs::read(rotation_path(&path, 2)).unwrap(), first);
    assert_eq!(fs::read(rotation_path(&path, 1)).unwrap(), second);
    assert_eq!(read_all(&path).unwrap()[0].source(), "three");

    writer.append(&sample("four")).unwrap();
    assert!(!rotation_path(&path, 3).exists());
    assert_eq!(fs::read(rotation_path(&path, 2)).unwrap(), second);
    assert_eq!(
        read_all(&rotation_path(&path, 1)).unwrap()[0].source(),
        "three"
    );
    assert_eq!(read_all(&path).unwrap()[0].source(), "four");
}

#[test]
fn exact_max_bytes_rotates_one_byte_more_appends() {
    let dir = Scratch::new();
    let path = dir.path("exact.jsonl");
    let mut writer = Writer::open(&path, u64::MAX, 2).unwrap();
    writer.append(&sample("one")).unwrap();
    drop(writer);
    let prior = fs::read(&path).unwrap();
    let size = prior.len() as u64;

    let mut at_limit = Writer::open(&path, size, 2).unwrap();
    at_limit.append(&sample("two")).unwrap();
    drop(at_limit);
    assert_eq!(fs::read(rotation_path(&path, 1)).unwrap(), prior);
    assert_eq!(read_all(&path).unwrap().len(), 1);
    assert_eq!(read_all(&path).unwrap()[0].source(), "two");

    let under = dir.path("under.jsonl");
    fs::write(&under, &prior).unwrap();
    let mut still = Writer::open(&under, size + 1, 2).unwrap();
    still.append(&sample("two")).unwrap();
    assert!(!rotation_path(&under, 1).exists());
    let raw = fs::read(&under).unwrap();
    assert!(raw.starts_with(&prior));
    let events = read_all(&under).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].source(), "one");
    assert_eq!(events[1].source(), "two");
}

#[test]
fn keep_zero_unlinks_instead_of_renaming() {
    let dir = Scratch::new();
    let path = dir.path("nerve.jsonl");
    let mut writer = Writer::open(&path, 1, 0).unwrap();
    writer.append(&sample("one")).unwrap();
    writer.append(&sample("two")).unwrap();
    assert!(!rotation_path(&path, 1).exists());
    let events = read_all(&path).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].source(), "two");
    assert_zagreb(events[0].ts());
}

#[test]
fn zero_max_bytes_never_rotates() {
    let dir = Scratch::new();
    let path = dir.path("nerve.jsonl");
    let mut writer = Writer::open(&path, 0, 4).unwrap();
    for name in ["a", "b", "c", "d", "e"] {
        writer.append(&sample(name)).unwrap();
    }
    assert!(!rotation_path(&path, 1).exists());
    let events = read_all(&path).unwrap();
    assert_eq!(events.len(), 5);
    assert_eq!(events[0].source(), "a");
    assert_eq!(events[4].source(), "e");
    assert_eq!(events[0].data_json(), r#"{"ok":true}"#);
}

#[test]
fn open_does_not_rotate_an_existing_file() {
    let dir = Scratch::new();
    let path = dir.path("nerve.jsonl");
    let body = "{\"ts\":\"2026-12-01T13:00:00+01:00\",\"source\":\"kept\",\"kind\":\"beat\",\"severity\":\"info\",\"data\":{}}\n";
    fs::write(&path, body).unwrap();
    let _writer = Writer::open(&path, 1, 2).unwrap();
    assert!(!rotation_path(&path, 1).exists());
    assert_eq!(fs::read(&path).unwrap(), body.as_bytes());
}

#[test]
fn rejected_inputs_and_bad_lines() {
    let dir = Scratch::new();
    let path = dir.path("nerve.jsonl");
    let missing = dir.path("no-such-dir").join("nerve.jsonl");
    let err = Writer::open(&missing, 64, 1).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::NotFound);

    let err = Writer::open(&path, 64, MAX_KEEP + 1).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);

    let err = Event::new("s", "k", "info", "{").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidData);

    fs::write(&path, "{not json\n").unwrap();
    let err = read_all(&path).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidData);

    fs::write(&path, "\n  \n").unwrap();
    assert!(read_all(&path).unwrap().is_empty());
}

#[test]
fn newline_in_source_stays_one_physical_line() {
    let dir = Scratch::new();
    let path = dir.path("nerve.jsonl");
    let mut writer = Writer::open(&path, 0, 1).unwrap();
    let event = Event::new("src\"\n", "kind", "sev", r#"{"note":"a\nb"}"#).unwrap();
    writer.append(&event).unwrap();
    let raw = fs::read(&path).unwrap();
    assert_eq!(raw.iter().filter(|byte| **byte == b'\n').count(), 1);
    let got = read_all(&path).unwrap();
    assert_eq!(got[0].source(), "src\"\n");
    assert_eq!(got[0].data_json(), r#"{"note":"a\nb"}"#);
    assert_zagreb(got[0].ts());
}

#[test]
fn rotation_path_numbering() {
    let path = Path::new("/tmp/nerve.jsonl");
    assert_eq!(rotation_path(path, 0), PathBuf::from("/tmp/nerve.jsonl"));
    assert_eq!(rotation_path(path, 1), PathBuf::from("/tmp/nerve.jsonl.1"));
    assert_eq!(rotation_path(path, 2), PathBuf::from("/tmp/nerve.jsonl.2"));
}

#[test]
fn reopening_appends() {
    let dir = Scratch::new();
    let path = dir.path("nerve.jsonl");
    let mut writer = Writer::open(&path, 1_000_000, 2).unwrap();
    writer.append(&sample("one")).unwrap();
    drop(writer);
    let prior = fs::read(&path).unwrap();
    let mut writer = Writer::open(&path, 1_000_000, 2).unwrap();
    writer.append(&sample("two")).unwrap();
    let raw = fs::read(&path).unwrap();
    assert!(raw.starts_with(&prior));
    assert_eq!(read_all(&path).unwrap().len(), 2);
    assert!(!rotation_path(&path, 1).exists());
}
