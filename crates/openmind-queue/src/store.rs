// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The durable log behind the engine.
//!
//! One file, `queue.log`, one record per line: `<crc32 as 8 hex> <json>\n`.
//! Every append is flushed with `sync_data` before the client is answered
//! (except [`Record::Tally`] counter lines, see [`Store::append_unsynced`]).
//!
//! Opening the log replays it. A torn tail (the last line cut short or
//! failing its checksum, which is what a crash mid-write leaves) is cut off
//! and reported. A bad line with good lines after it is real corruption:
//! opening fails rather than silently dropping acknowledged work.
//!
//! Compaction writes the engine snapshot to `queue.log.tmp`, syncs it,
//! renames it over the log and syncs the directory.
//!
//! A `queue.lock` file is held with an exclusive `flock` for the life of the
//! store, so two servers cannot share one data directory. The kernel drops
//! the lock when the process dies, so a crash never leaves a stale lock.

use crate::engine::Record;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// CRC-32 (IEEE 802.3, reflected, poly 0xEDB88320). Small table-less
/// version: the log is not a hot path.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Encode one record as a log line (with the trailing newline).
pub fn encode(r: &Record) -> String {
    let json = serde_json::to_string(r).expect("records always serialise");
    format!("{:08x} {}\n", crc32(json.as_bytes()), json)
}

/// Decode one line (without the newline).
pub fn decode(line: &str) -> Option<Record> {
    let (crc, json) = line.split_once(' ')?;
    if crc.len() != 8 || u32::from_str_radix(crc, 16).ok()? != crc32(json.as_bytes()) {
        return None;
    }
    serde_json::from_str(json).ok()
}

/// What opening found.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OpenReport {
    pub records: usize,
    /// Bytes cut off a torn tail (0 when the log was clean).
    pub truncated_bytes: u64,
}

pub struct Store {
    dir: PathBuf,
    file: File,
    lines: u64,
    _lock: File,
}

impl Store {
    pub fn log_path(dir: &Path) -> PathBuf {
        dir.join("queue.log")
    }

    /// Open (or create) the store in `dir` and return every valid record.
    pub fn open(dir: &Path) -> io::Result<(Store, Vec<Record>, OpenReport)> {
        fs::create_dir_all(dir)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("queue.lock"))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    format!("{} is in use by another process", dir.display()),
                ))
            }
            Err(fs::TryLockError::Error(e)) => return Err(e),
        }
        let path = Self::log_path(dir);
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        let mut records = Vec::new();
        let mut good_end = 0usize;
        let mut first_bad: Option<(usize, usize)> = None; // (offset, line no)
        let mut pos = 0usize;
        let mut line_no = 0usize;
        while pos < buf.len() {
            line_no += 1;
            let (end, complete) = match buf[pos..].iter().position(|&b| b == b'\n') {
                Some(i) => (pos + i, true),
                None => (buf.len(), false),
            };
            let rec = if complete {
                std::str::from_utf8(&buf[pos..end]).ok().and_then(decode)
            } else {
                None
            };
            match rec {
                Some(r) => {
                    if let Some((_, bad_line)) = first_bad {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!(
                                "{}: corrupt record at line {bad_line} with valid records after it; refusing to start",
                                path.display()
                            ),
                        ));
                    }
                    records.push(r);
                    good_end = end + 1;
                }
                None => {
                    if first_bad.is_none() {
                        first_bad = Some((pos, line_no));
                    }
                }
            }
            pos = end + 1;
        }
        let truncated = (buf.len() - good_end) as u64;
        if truncated > 0 {
            file.set_len(good_end as u64)?;
            file.sync_data()?;
        }
        file.seek(SeekFrom::End(0))?;
        let report = OpenReport {
            records: records.len(),
            truncated_bytes: truncated,
        };
        let store = Store {
            dir: dir.to_path_buf(),
            file,
            lines: records.len() as u64,
            _lock: lock,
        };
        Ok((store, records, report))
    }

    /// Lines in the log since the last compaction.
    pub fn lines(&self) -> u64 {
        self.lines
    }

    /// Append records and sync. On failure the file is cut back to where it
    /// was, so a failed write never leaves a half line in the middle.
    pub fn append(&mut self, recs: &[Record]) -> io::Result<()> {
        self.append_with(recs, true)
    }

    /// Like [`Store::append`] but without `sync_data`: the line reaches the
    /// page cache, so it survives a process crash (kill -9) but not a power
    /// loss, and the next synced append makes it durable too. Used only for
    /// [`Record::Tally`] counter lines, so a refusal flood costs no fsyncs.
    pub fn append_unsynced(&mut self, recs: &[Record]) -> io::Result<()> {
        self.append_with(recs, false)
    }

    fn append_with(&mut self, recs: &[Record], sync: bool) -> io::Result<()> {
        if recs.is_empty() {
            return Ok(());
        }
        let mut out = String::new();
        for r in recs {
            out.push_str(&encode(r));
        }
        let before = self.file.seek(SeekFrom::End(0))?;
        let res = self.file.write_all(out.as_bytes()).and_then(|_| {
            if sync {
                self.file.sync_data()
            } else {
                Ok(())
            }
        });
        if let Err(e) = res {
            let _ = self.file.set_len(before);
            let _ = self.file.seek(SeekFrom::End(0));
            return Err(e);
        }
        self.lines += recs.len() as u64;
        Ok(())
    }

    /// Replace the log with `snapshot` atomically.
    ///
    /// The handle that wrote the snapshot becomes the log handle, so once the
    /// rename has happened there is no later step that can fail and leave
    /// appends going to the old, unlinked file (slot 206: the previous code
    /// reopened the path after the rename). Any error before the rename
    /// leaves the old log and handle untouched.
    pub fn compact(&mut self, snapshot: &[Record]) -> io::Result<()> {
        let tmp = self.dir.join("queue.log.tmp");
        let mut f = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&tmp)?;
        let mut out = String::new();
        for r in snapshot {
            out.push_str(&encode(r));
        }
        f.write_all(out.as_bytes())?;
        f.sync_all()?;
        f.seek(SeekFrom::End(0))?;
        let path = Self::log_path(&self.dir);
        fs::rename(&tmp, &path)?;
        if let Ok(d) = File::open(&self.dir) {
            let _ = d.sync_all();
        }
        self.file = f;
        self.lines = snapshot.len() as u64;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Config, Engine, JobSpec};

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "poseidon-store-{name}-{}-{}",
            std::process::id(),
            crate::now_ms()
        ));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn crc32_known_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn roundtrip_and_torn_tail_repair() {
        let dir = tmpdir("torn");
        let mut e = Engine::new(Config::default());
        {
            let (mut s, recs, rep) = Store::open(&dir).unwrap();
            assert!(recs.is_empty());
            assert_eq!(rep.truncated_bytes, 0);
            for i in 0..3 {
                let r = e
                    .enqueue(
                        JobSpec {
                            priority: i,
                            ..JobSpec::default()
                        },
                        1,
                    )
                    .unwrap();
                s.append(std::slice::from_ref(&r)).unwrap();
                e.apply(&r);
            }
        }
        // simulate a crash mid-write: half a line at the end
        let path = Store::log_path(&dir);
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"deadbeef {\"t\":\"enq").unwrap();
        drop(f);
        let (_s, recs, rep) = Store::open(&dir).unwrap();
        assert_eq!(recs.len(), 3);
        assert_eq!(rep.truncated_bytes, 18);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corruption_in_the_middle_refuses_to_open() {
        let dir = tmpdir("mid");
        {
            let (mut s, _, _) = Store::open(&dir).unwrap();
            let mut e = Engine::new(Config::default());
            for _ in 0..3 {
                let r = e.enqueue(JobSpec::default(), 1).unwrap();
                s.append(std::slice::from_ref(&r)).unwrap();
                e.apply(&r);
            }
        }
        let path = Store::log_path(&dir);
        let text = fs::read_to_string(&path).unwrap();
        let mut lines: Vec<String> = text.lines().map(String::from).collect();
        lines[1] = lines[1].replacen("default", "DEFAULT", 1);
        fs::write(&path, lines.join("\n") + "\n").unwrap();
        let err = Store::open(&dir).err().expect("must refuse");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn second_open_is_locked_out() {
        let dir = tmpdir("lock");
        let (_a, _, _) = Store::open(&dir).unwrap();
        let err = Store::open(&dir).err().expect("locked");
        assert_eq!(err.kind(), io::ErrorKind::WouldBlock);
        drop(_a);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compaction_keeps_state() {
        let dir = tmpdir("compact");
        let mut e = Engine::new(Config::default());
        {
            let (mut s, _, _) = Store::open(&dir).unwrap();
            for i in 0..50 {
                let r = e
                    .enqueue(
                        JobSpec {
                            priority: i % 4,
                            ..JobSpec::default()
                        },
                        1,
                    )
                    .unwrap();
                s.append(std::slice::from_ref(&r)).unwrap();
                e.apply(&r);
            }
            for _ in 0..20 {
                let r = e.lease("default", "w", 2, 0).unwrap();
                s.append(std::slice::from_ref(&r)).unwrap();
                e.apply(&r);
                if let Record::Lease { id, token, .. } = r {
                    let a = e.ack(id, token, None).unwrap();
                    s.append(std::slice::from_ref(&a)).unwrap();
                    e.apply(&a);
                }
            }
            assert_eq!(s.lines(), 90);
            s.compact(&e.snapshot()).unwrap();
            assert!(s.lines() < 40);
            // appends after compaction land in the new file
            let r = e.enqueue(JobSpec::default(), 3).unwrap();
            s.append(std::slice::from_ref(&r)).unwrap();
            e.apply(&r);
        }
        let (_s, recs, rep) = Store::open(&dir).unwrap();
        assert_eq!(rep.truncated_bytes, 0);
        let mut r = Engine::new(Config::default());
        for x in &recs {
            r.apply(x);
        }
        assert_eq!(r.depth(), e.depth());
        assert_eq!(r.live_jobs(), 31);
        assert_eq!(r.stats, e.stats);
        let _ = fs::remove_dir_all(&dir);
    }
}
