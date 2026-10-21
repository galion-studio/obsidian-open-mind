// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! D-FI4 guard (wave-4 slot 199): Gaia's test ports never include a port
//! that is live on the VPS, and every Gaia test keeps its hardcoded ports in
//! its own slot band.
//!
//! * [`LIVE_PORTS`] is sorted, has no duplicates, and holds the live ports
//!   from `docs/ops/vps-baseline.md` plus the 2026-10-21 17:57 +02:00 `ss`.
//! * No port that [`is_test_port`] accepts is live, and every live port
//!   inside [`TEST_PORTS`] (POSEIDON 19120, cloudflared 20241/20242) is
//!   refused by [`check_target`].
//! * Every `.rs` file under `crates/gaia/{src,tests}` and
//!   `crates/gaia-live/{src,tests}` is registered in [`REGISTRY`] with its
//!   slot band(s). Each 5-digit number 18000-21999 in its code (comments
//!   skipped, string literals included) must sit in one of those bands, or be
//!   listed as a deliberate value (a port a test proves is refused, a
//!   non-port number). A live port is never allowed inside a band. A new
//!   file fails this test until it is registered.

use gaia_live::heal::{check_target, is_live_port, is_test_port, TestTarget, LIVE_PORTS, TEST_PORTS};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

/// Slot port band: wave 1-3 slot NN -> 19NN0-19NN9; wave-4 slot N (>= 100)
/// -> 19700+10*(N-100) .. +9 (common rules in plans/wave4-tasks.md).
fn band(slot: u16) -> RangeInclusive<u16> {
    let lo = if slot < 100 {
        19000 + 10 * slot
    } else {
        19700 + 10 * (slot - 100)
    };
    lo..=lo + 9
}

struct Entry {
    /// Path relative to `crates/`.
    file: &'static str,
    /// Slots whose bands this file may use.
    slots: &'static [u16],
    /// Numbers outside the bands that are deliberate (refused ports, the
    /// live list itself, non-port values), with no band meaning.
    values: &'static [u16],
}

/// Live ports in [`TEST_PORTS`] that refusal tests name on purpose.
const LIVE_IN_BAND: &[u16] = &[19120, 20241, 20242];

const REGISTRY: &[Entry] = &[
    // gaia tests
    Entry { file: "gaia/tests/commands.rs", slots: &[], values: &[] },
    Entry { file: "gaia/tests/heal_decisions.rs", slots: &[175], values: &[] },
    Entry { file: "gaia/tests/heal_loop.rs", slots: &[160], values: LIVE_IN_BAND },
    Entry { file: "gaia/tests/heal_tick.rs", slots: &[23], values: &[] },
    Entry { file: "gaia/tests/live.rs", slots: &[], values: &[] },
    Entry { file: "gaia/tests/real_cli.rs", slots: &[1], values: &[] },
    Entry { file: "gaia/tests/real_gaia_decisions.rs", slots: &[175], values: &[] },
    Entry { file: "gaia/tests/real_gaia_faults.rs", slots: &[189, 199], values: &[] },
    Entry { file: "gaia/tests/real_gaia_live.rs", slots: &[23], values: &[] },
    Entry { file: "gaia/tests/real_gaia_loop.rs", slots: &[160], values: &[] },
    Entry { file: "gaia/tests/real_gaia_poseidon.rs", slots: &[24], values: &[] },
    Entry { file: "gaia/tests/scenario.rs", slots: &[], values: &[] },
    Entry { file: "gaia/tests/tokens.rs", slots: &[175], values: &[] },
    // gaia-live tests
    Entry { file: "gaia-live/tests/decisions.rs", slots: &[], values: &[] },
    // 19240 / 21000: just outside the allowed / test range, proven refused;
    // 19000/19999: the old whole band, proven refused (holds 19120).
    Entry { file: "gaia-live/tests/heal.rs", slots: &[23, 199], values: &[19000, 19120, 19240, 19999, 20241, 20242, 21000, 21009] },
    Entry { file: "gaia-live/tests/json_strict.rs", slots: &[], values: &[] },
    Entry { file: "gaia-live/tests/live.rs", slots: &[], values: &[] },
    Entry { file: "gaia-live/tests/port_bands.rs", slots: &[], values: &[18000, 19000, 19120, 19700, 20241, 20242, 21999] },
    // gaia src (unit tests and defaults)
    Entry { file: "gaia/src/heal_loop.rs", slots: &[], values: &[] },
    Entry { file: "gaia/src/lib.rs", slots: &[], values: &[] },
    Entry { file: "gaia/src/live_heal.rs", slots: &[], values: &[] },
    Entry { file: "gaia/src/main.rs", slots: &[], values: &[19000, 20999] },
    Entry { file: "gaia/src/poseidon.rs", slots: &[24], values: &[] },
    Entry { file: "gaia/src/tokens.rs", slots: &[], values: &[] },
    // view.rs names the live cloudflared metrics ports to describe them.
    Entry { file: "gaia/src/view.rs", slots: &[], values: &[20241, 20242] },
    Entry { file: "gaia/src/bin/gaia-dummy.rs", slots: &[], values: &[19000, 20999] },
    // gaia-live src: the band and the live list themselves.
    Entry { file: "gaia-live/src/heal.rs", slots: &[], values: &[18800, 19000, 19120, 20241, 20242, 20999] },
    Entry { file: "gaia-live/src/lib.rs", slots: &[], values: &[19000, 20999] },
    Entry { file: "gaia-live/src/nerve.rs", slots: &[], values: &[] },
];

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n != "fixtures") {
                rs_files(&p, out);
            }
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// 5-digit numbers 18000..=21999 in code lines (comment lines skipped),
/// with their line numbers. Digits glued to letters/digits/`-` dates are
/// not numbers (`2026-10-21`, `0x19120`).
fn numbers(src: &str) -> Vec<(usize, u16)> {
    let mut v = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with("//") {
            continue;
        }
        let b = line.as_bytes();
        let mut j = 0;
        while j < b.len() {
            if b[j].is_ascii_digit() {
                let s = j;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
                let before = if s > 0 { b[s - 1] } else { b' ' };
                let glued_before = before.is_ascii_alphanumeric() || before == b'_' || before == b'.';
                if !glued_before && j - s == 5 {
                    let n: u32 = line[s..j].parse().unwrap();
                    if (18000..=21999).contains(&n) {
                        v.push((i + 1, n as u16));
                    }
                }
            } else {
                j += 1;
            }
        }
    }
    v
}

#[test]
fn live_list_is_sorted_unique_and_complete() {
    assert!(LIVE_PORTS.windows(2).all(|w| w[0] < w[1]), "LIVE_PORTS must be sorted and unique");
    // vps-baseline.md (2026-10-21 11:23 +02:00) listeners and tunnel origins,
    // plus the slot-199 read-only `ss -ltn` (17:57 +02:00).
    for p in [
        22u16, 25, 53, 3004, 3005, 3010, 3011, 3020, 4001, 4003, 4004, 4020, 4042, 4060, 4444, 8002,
        8080, 8091, 8420, 8642, 8646, 8650, 8660, 8670, 8671, 8672, 18800, 19120, 20241, 20242,
    ] {
        assert!(is_live_port(p), "{p} missing from LIVE_PORTS");
    }
}

#[test]
fn no_test_port_is_live() {
    let mut test_ports = 0u32;
    for p in TEST_PORTS {
        if is_test_port(p) {
            test_ports += 1;
            assert!(!LIVE_PORTS.contains(&p), "test port {p} is in the live list");
        }
    }
    let live_inside: Vec<u16> = LIVE_PORTS.iter().copied().filter(|p| TEST_PORTS.contains(p)).collect();
    assert_eq!(live_inside, LIVE_IN_BAND, "live ports inside the test band");
    assert_eq!(test_ports, 2000 - LIVE_IN_BAND.len() as u32);
    let prog = ["/bin/true", "/usr/bin/true"]
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .unwrap();
    for p in LIVE_PORTS.iter().copied() {
        let t = TestTarget {
            name: "gaia-dummy".into(),
            port: p,
            health_path: "/health".into(),
            argv: vec![prog.into(), "--port".into(), p.to_string()],
        };
        assert!(check_target(&t, &(p..=p)).is_err(), "live port {p} accepted as a target");
    }
}

#[test]
fn registered_bands_hold_no_live_port() {
    for e in REGISTRY {
        for s in e.slots {
            let b = band(*s);
            assert!(
                b.clone().all(is_test_port),
                "{}: slot {s} band {}-{} holds a live or out-of-band port",
                e.file,
                b.start(),
                b.end()
            );
        }
    }
    // Known collisions, so they are never handed to a Gaia test: slot 12's
    // band holds POSEIDON, wave-4 slot 154's holds cloudflared's metrics.
    assert!(band(12).contains(&19120));
    assert!(band(154).contains(&20241) && band(154).contains(&20242));
}

#[test]
fn every_gaia_test_port_is_in_its_slot_band_and_never_live() {
    let root = crates_dir();
    let mut files = Vec::new();
    for d in ["gaia/src", "gaia/tests", "gaia-live/src", "gaia-live/tests"] {
        rs_files(&root.join(d), &mut files);
    }
    files.sort();
    assert!(files.len() >= REGISTRY.len(), "scanned {} files", files.len());
    let mut bad = Vec::new();
    let mut checked = 0usize;
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let Some(e) = REGISTRY.iter().find(|e| e.file == rel) else {
            bad.push(format!("{rel}: not in REGISTRY (add it with its slot band)"));
            continue;
        };
        let src = std::fs::read_to_string(f).unwrap();
        for (line, n) in numbers(&src) {
            checked += 1;
            let in_band = e.slots.iter().any(|s| band(*s).contains(&n));
            if in_band && is_live_port(n) {
                bad.push(format!("{rel}:{line}: {n} is a live VPS port inside a slot band"));
            } else if !in_band && !e.values.contains(&n) {
                bad.push(format!(
                    "{rel}:{line}: {n} is outside slot band(s) {:?} and not a listed value",
                    e.slots
                ));
            } else if is_live_port(n) && !e.values.contains(&n) {
                bad.push(format!("{rel}:{line}: live VPS port {n} used without being listed"));
            }
        }
    }
    for e in REGISTRY {
        assert!(root.join(e.file).is_file(), "REGISTRY names missing file {}", e.file);
    }
    assert!(bad.is_empty(), "{} port problems:\n{}", bad.len(), bad.join("\n"));
    assert!(checked > 50, "only {checked} numbers checked; scanner broken?");
}

#[test]
fn scanner_skips_comments_and_dates() {
    let src = "// 19120 in a comment\nlet a = \"127.0.0.1:20300\"; // 19999\nlet d = \"2026-10-21\"; let x = 0x19120; let y = 20301u16;\n";
    assert_eq!(numbers(src), vec![(2, 20300), (3, 20301)]);
}
