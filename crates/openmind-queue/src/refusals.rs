// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Refusal counters that survive `kill -9`: the `refusals.dat` sidecar.
//!
//! The engine's refusal counters (`quota_refusals`, `budget_refusals`,
//! `backpressure_refusals`, `stale_acks`, `charge_clamped`) are not
//! derived from log records: a refused request changes no state, so
//! nothing about it is written to `queue.log`. Before this sidecar they
//! were persisted only at compaction, and a crash reset them.
//!
//! Layout: two fixed slots of [`SLOT`] bytes. Each slot holds one line
//! `<crc32 as 8 hex> {"seq":N,"c":{...}}\n`, zero-padded to the slot size.
//! A save writes the next `seq` into slot `seq % 2` with one positioned
//! write, so a torn write can only damage the slot being written; the
//! other slot still holds the previous value. Opening picks the valid
//! slot with the highest `seq`.
//!
//! Durability: the write lands in the page cache before the client is
//! answered, so a `kill -9` (or any process crash) loses nothing. It is
//! `sync_data`'d by the server's ticker at most ~1 s later, so a power
//! loss can lose at most the last second of refusal counts. Job state is
//! unaffected either way (that lives in the synced log).
//!
//! The file sits in the data directory, which `queue.lock` already guards
//! against a second server. An old binary simply ignores it, so a
//! rollback to a build without the sidecar is safe.

use crate::engine::Stats;
use crate::store::crc32;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

/// File name inside the data directory.
pub const FILE: &str = "refusals.dat";
/// Bytes per slot. The JSON line is ~200 bytes even with every counter at
/// `u64::MAX`, so 512 leaves plenty of room.
pub const SLOT: usize = 512;
/// A restored sequence number above this is rebased to 1/2 on open. One
/// save per changed refusal count never gets near it; only damage or a
/// crafted file does. Without the rebase a slot at `u64::MAX` overflowed
/// the next save (debug panic) or wrapped it to seq 0 in release, so the
/// stale slot won every later open and the counters silently stopped
/// persisting (slot 221 fuzz find).
pub const REBASE_AT: u64 = 1 << 62;

/// The process-local counters, as persisted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Refusals {
    pub quota_refusals: u64,
    pub budget_refusals: u64,
    pub backpressure_refusals: u64,
    pub stale_acks: u64,
    pub charge_clamped: u64,
}

impl Refusals {
    pub fn of(s: &Stats) -> Refusals {
        Refusals {
            quota_refusals: s.quota_refusals,
            budget_refusals: s.budget_refusals,
            backpressure_refusals: s.backpressure_refusals,
            stale_acks: s.stale_acks,
            charge_clamped: s.charge_clamped,
        }
    }

    /// Restore into `s`, field by field taking the larger value. The
    /// counters only grow, so the larger one is the newer one whichever of
    /// the log (`Counters` at compaction) or the sidecar holds it.
    pub fn restore_into(&self, s: &mut Stats) {
        s.quota_refusals = s.quota_refusals.max(self.quota_refusals);
        s.budget_refusals = s.budget_refusals.max(self.budget_refusals);
        s.backpressure_refusals = s.backpressure_refusals.max(self.backpressure_refusals);
        s.stale_acks = s.stale_acks.max(self.stale_acks);
        s.charge_clamped = s.charge_clamped.max(self.charge_clamped);
    }
}

#[derive(Serialize, Deserialize)]
struct Body {
    seq: u64,
    c: Refusals,
}

fn encode_slot(seq: u64, c: &Refusals) -> [u8; SLOT] {
    let json = serde_json::to_string(&Body { seq, c: *c }).expect("counters serialise");
    let line = format!("{:08x} {}\n", crc32(json.as_bytes()), json);
    let mut out = [0u8; SLOT];
    // ~200 bytes at most (see SLOT); the assert documents the bound.
    assert!(line.len() <= SLOT, "refusal slot overflow");
    out[..line.len()].copy_from_slice(line.as_bytes());
    out
}

fn decode_slot(b: &[u8]) -> Option<(u64, Refusals)> {
    let end = b.iter().position(|&x| x == b'\n')?;
    if b[end + 1..].iter().any(|&x| x != 0) {
        return None;
    }
    let line = std::str::from_utf8(&b[..end]).ok()?;
    let (crc, json) = line.split_once(' ')?;
    if crc.len() != 8 || u32::from_str_radix(crc, 16).ok()? != crc32(json.as_bytes()) {
        return None;
    }
    let body: Body = serde_json::from_str(json).ok()?;
    Some((body.seq, body.c))
}

/// What opening the sidecar found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SidecarReport {
    /// The newest valid value, if any slot was valid.
    pub restored: Option<Refusals>,
    /// Its sequence number (0 when nothing was restored).
    pub seq: u64,
    /// Slots that held bytes but failed the checksum or parse (0, 1 or 2).
    /// One bad slot is what a torn write leaves; two means the file was
    /// damaged and the counters restart from the log's `Counters` record.
    pub bad_slots: u8,
    /// The file did not exist (first start, or an older data directory).
    pub fresh: bool,
}

pub struct Sidecar {
    path: PathBuf,
    file: File,
    seq: u64,
    last: Refusals,
    dirty: bool,
    pub writes: u64,
    pub write_errors: u64,
    pub syncs: u64,
}

impl Sidecar {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join(FILE)
    }

    /// Open (or create) the sidecar in `dir`. A damaged file never stops
    /// the server: the report says what was found.
    pub fn open(dir: &Path) -> io::Result<(Sidecar, SidecarReport)> {
        let path = Self::path(dir);
        let fresh = !path.exists();
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        let len = file.metadata()?.len() as usize;
        let mut buf = vec![0u8; len.min(2 * SLOT)];
        file.read_exact_at(&mut buf, 0)?;
        let mut rep = SidecarReport {
            fresh,
            ..SidecarReport::default()
        };
        let mut best: Option<(u64, Refusals)> = None;
        for i in 0..2 {
            let lo = i * SLOT;
            if lo >= buf.len() {
                continue;
            }
            let slot = &buf[lo..buf.len().min(lo + SLOT)];
            if slot.iter().all(|&x| x == 0) {
                continue;
            }
            match decode_slot(slot) {
                Some(v) if best.is_none_or(|b| v.0 > b.0) => best = Some(v),
                Some(_) => {}
                None => rep.bad_slots += 1,
            }
        }
        let (seq, last) = best.unwrap_or_default();
        rep.restored = best.map(|b| b.1);
        rep.seq = seq;
        let mut sc = Sidecar {
            path,
            file,
            seq,
            last,
            dirty: false,
            writes: 0,
            write_errors: 0,
            syncs: 0,
        };
        if seq > REBASE_AT {
            sc.rebase();
        }
        Ok((sc, rep))
    }

    /// Rewrite the restored value as seq 2 (slot 0), then seq 1 (slot 1).
    /// Both writes carry the same value, so a torn write at any point
    /// still opens to that value. On a write error the old seq is kept
    /// (counted in `write_errors`; `save` saturates instead of wrapping).
    fn rebase(&mut self) {
        for (seq, off) in [(2u64, 0u64), (1, SLOT as u64)] {
            if self
                .file
                .write_all_at(&encode_slot(seq, &self.last), off)
                .is_err()
            {
                self.write_errors += 1;
                return;
            }
        }
        self.seq = 2;
        self.dirty = true;
        self.writes += 2;
    }

    pub fn file_path(&self) -> &Path {
        &self.path
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// Write `c` if it differs from the last saved value. Returns whether a
    /// write happened. On error the in-memory value is kept so the next
    /// call retries.
    pub fn save(&mut self, c: &Refusals) -> io::Result<bool> {
        if *c == self.last {
            return Ok(false);
        }
        let seq = self.seq.saturating_add(1);
        let slot = encode_slot(seq, c);
        let off = (seq % 2) * SLOT as u64;
        if let Err(e) = self.file.write_all_at(&slot, off) {
            self.write_errors += 1;
            return Err(e);
        }
        self.seq = seq;
        self.last = *c;
        self.dirty = true;
        self.writes += 1;
        Ok(true)
    }

    /// `sync_data` if anything was written since the last sync.
    pub fn sync(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        self.file.sync_data()?;
        self.dirty = false;
        self.syncs += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "poseidon-refusals-{name}-{}-{}",
            std::process::id(),
            crate::now_ms()
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn r(q: u64, b: u64) -> Refusals {
        Refusals {
            quota_refusals: q,
            budget_refusals: b,
            ..Refusals::default()
        }
    }

    #[test]
    fn max_values_fit_a_slot() {
        let m = Refusals {
            quota_refusals: u64::MAX,
            budget_refusals: u64::MAX,
            backpressure_refusals: u64::MAX,
            stale_acks: u64::MAX,
            charge_clamped: u64::MAX,
        };
        let s = encode_slot(u64::MAX, &m);
        assert_eq!(decode_slot(&s), Some((u64::MAX, m)));
    }

    #[test]
    fn fresh_then_roundtrip_and_alternating_slots() {
        let d = tmpdir("rt");
        {
            let (mut s, rep) = Sidecar::open(&d).unwrap();
            assert!(rep.fresh);
            assert_eq!(rep.restored, None);
            assert!(s.save(&r(1, 0)).unwrap());
            assert!(
                !s.save(&r(1, 0)).unwrap(),
                "unchanged value is not rewritten"
            );
            assert!(s.save(&r(1, 2)).unwrap());
            assert!(s.save(&r(3, 2)).unwrap());
            assert_eq!(s.seq(), 3);
            // dropped without sync: what kill -9 leaves (page cache only)
        }
        assert_eq!(
            fs::metadata(Sidecar::path(&d)).unwrap().len(),
            2 * SLOT as u64
        );
        let (_s, rep) = Sidecar::open(&d).unwrap();
        assert!(!rep.fresh);
        assert_eq!(rep.restored, Some(r(3, 2)));
        assert_eq!(rep.seq, 3);
        assert_eq!(rep.bad_slots, 0);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn torn_write_falls_back_to_the_other_slot() {
        let d = tmpdir("torn");
        {
            let (mut s, _) = Sidecar::open(&d).unwrap();
            s.save(&r(5, 1)).unwrap(); // seq 1 -> slot 1
            s.save(&r(6, 1)).unwrap(); // seq 2 -> slot 0
        }
        // tear slot 0 (the newest) half way, as a crash mid-write would
        let p = Sidecar::path(&d);
        let mut b = fs::read(&p).unwrap();
        for x in &mut b[20..SLOT] {
            *x = 0;
        }
        fs::write(&p, &b).unwrap();
        let (mut s, rep) = Sidecar::open(&d).unwrap();
        assert_eq!(rep.bad_slots, 1);
        assert_eq!(rep.restored, Some(r(5, 1)));
        assert_eq!(rep.seq, 1);
        // the next save goes to slot 0 again and repairs it
        s.save(&r(7, 1)).unwrap();
        drop(s);
        let (_s, rep) = Sidecar::open(&d).unwrap();
        assert_eq!(
            (rep.restored, rep.seq, rep.bad_slots),
            (Some(r(7, 1)), 2, 0)
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn flipped_byte_fails_crc_and_both_bad_restores_nothing() {
        let d = tmpdir("crc");
        {
            let (mut s, _) = Sidecar::open(&d).unwrap();
            s.save(&r(9, 9)).unwrap();
            s.save(&r(10, 9)).unwrap();
        }
        let p = Sidecar::path(&d);
        let mut b = fs::read(&p).unwrap();
        // change a digit inside each JSON body; the crc no longer matches
        for base in [0, SLOT] {
            let i = (base..base + SLOT).find(|&i| b[i] == b':').unwrap() + 1;
            b[i] = if b[i] == b'1' { b'2' } else { b'1' };
        }
        fs::write(&p, &b).unwrap();
        let (_s, rep) = Sidecar::open(&d).unwrap();
        assert_eq!(rep.bad_slots, 2);
        assert_eq!(rep.restored, None);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn restore_takes_the_larger_value_per_field() {
        let mut s = Stats {
            quota_refusals: 10,
            budget_refusals: 1,
            stale_acks: 4,
            ..Stats::default()
        };
        Refusals {
            quota_refusals: 7,
            budget_refusals: 3,
            stale_acks: 4,
            charge_clamped: 2,
            ..Refusals::default()
        }
        .restore_into(&mut s);
        assert_eq!(
            (
                s.quota_refusals,
                s.budget_refusals,
                s.stale_acks,
                s.charge_clamped
            ),
            (10, 3, 4, 2)
        );
    }
}
