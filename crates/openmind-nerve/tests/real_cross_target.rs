// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Nerve T3.5 cross-target real test (gated: `GALION_REAL=1`, box only).
//!
//! Repeats slot 217's measurement end to end:
//! 1. builds `openmind-nerve` release for x86_64 and aarch64 musl
//!    (`-p openmind-nerve` alone), plus the live recipe
//!    (`-p openmind-nerve -p openmind-memory`, x86_64 musl) that produced
//!    the VPS binary `openmind-nerve-aa77864`;
//! 2. checks every binary is a static ELF of the right machine, under the
//!    2 MiB budget, and records its size and SHA-256 (in-test SHA-256, no
//!    extra tool or crate);
//! 3. checks the pin chosen by `NERVE_XT_PIN` (slot 376, D-363-2):
//!    - `budget` (default): no binary pin; every binary must fit its row of
//!      the size-budget table [`SIZE_BUDGET`], and the solo and live-recipe
//!      x86_64 builds (separate target dirs, different package sets) must be
//!      bit-identical (tree-level reproducibility);
//!    - `live`: everything in `budget`, plus a `git archive` of the live
//!      nerve commit (`NERVE_XT_LIVE_REV`, default 9d1a4f3) is rebuilt with the
//!      deploy recipe and must equal the VPS binary bit for bit
//!      (`NERVE_XT_LIVE_SHA`/`NERVE_XT_LIVE_SIZE`, default = 9d1a4f3 as read on
//!      the VPS 2026-10-28 08:10 +01:00); the rebuilt binary is smoked too;
//!      slot 386: the x86_64 linker comes from this tree's COMMITTED
//!      `.cargo/config.toml` overlaid on the archive, linker env removed
//!      (`NERVE_XT_LIVE_CONFIG` = `committed` default | `env` slot-376 way |
//!      `archive` negative control without the line);
//!    - `aa77864`: the old slot-217 pin (only passes on the aa77864 tree);
//! 4. runs each binary (`aarch64` under qemu user mode) on slot ports
//!    20870-20872 against one live service thread (:20873) and one dead
//!    port (:20874) (block movable with `NERVE_XT_PORT_BASE`, slot 363):
//!    `GET /health`, `POST /signal`, `GET /state`,
//!    `GET /api/v1/signals`, and compares the answers across targets;
//! 5. `cargo check` for aarch64-linux-android and armv7-unknown-linux-musleabihf.
//!
//! Knobs: `NERVE_XT_DIR` (nested target dir, default `<workspace>/target/xt`),
//! `NERVE_XT_JOBS` (default 2), `NERVE_XT_QEMU` (qemu-aarch64 path),
//! `NERVE_XT_REPORT` (JSON report path, default `<NERVE_XT_DIR>/report.json`).
//! Slot 363: `NERVE_XT_PORT_BASE` (5-port block, default 20870). Slot 376:
//! `NERVE_XT_PIN` = `budget` | `live` | `aa77864`; the slot-363 knob
//! `NERVE_XT_LIVE_PIN` still works when `NERVE_XT_PIN` is unset (`0` = budget,
//! `1` = aa77864).

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// VPS `/opt/galion/rust/bin/openmind-nerve-aa77864`, read-only sha256sum/stat
/// 2026-10-21 21:02:28 +02:00 (docs/real-tests/nerve-cross-target.md).
/// Used only by `NERVE_XT_PIN=aa77864` (the slot-217 pin).
const AA77864_SHA: &str = "f0994b6db1fd8ed87c16bd72c9583c2894ec4dd5ba89d407016877322aed6a68";
const AA77864_SIZE: u64 = 1_213_216;
/// Live nerve since 2026-10-27 18:15 +01:00 (deploy t273u):
/// `/opt/galion/rust/bin/openmind-nerve-9d1a4f3`, read-only sha256sum/ls on the
/// VPS 2026-10-28 08:10:06 +01:00 (docs/real-tests/nerve-xt-pin-refresh.md).
/// Commit is the tip of swarm/nerve-deploy-t273u.
const LIVE_REV: &str = "9d1a4f3";
const LIVE_SHA: &str = "d0c70b879e5866bb105707d881686757b5732168da2c016f791316ec3f521d45";
const LIVE_SIZE: u64 = 1_458_760;
/// Deploy recipe of the live binary (reproduced bit for bit on the box
/// 2026-10-28 ~08:12 +01:00): `-p openmind-nerve` alone, x86_64 musl, release,
/// `--locked`, linker rust-lld. The default x86_64 linker gives 1 463 072 B.
const LIVE_RECIPE_LINKER: &str = "rust-lld";
/// Env var that overrides the x86_64 musl linker from `.cargo/config.toml`.
const X86_LINKER_ENV: &str = "CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER";
/// Cross triples this test builds or checks (slot 409, xt-linker-pins-arm).
const XT_TRIPLES: [&str; 5] = [
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "armv7-unknown-linux-musleabihf",
    "aarch64-linux-android",
    "armv7-linux-androideabi",
];

/// `aarch64-unknown-linux-musl` -> `AARCH64_UNKNOWN_LINUX_MUSL` (cargo env form).
fn triple_env(triple: &str) -> String {
    triple.to_ascii_uppercase().replace('-', "_")
}

/// Every env var that would beat the committed `.cargo/config.toml` for a
/// build (slot 409): per-target `LINKER`/`RUSTFLAGS` for [`XT_TRIPLES`] plus
/// the global rustflags/target knobs. `run_cargo_env` removes ALL of them, so
/// every build and check in this test picks its linker from the committed
/// config alone (before slot 409 the test itself exported
/// `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld`, which hid a
/// missing aarch64 line). The live-pin `env` mode adds its var back on purpose.
fn ambient_env_vars() -> Vec<String> {
    let mut v: Vec<String> = XT_TRIPLES
        .iter()
        .flat_map(|t| {
            let u = triple_env(t);
            [
                format!("CARGO_TARGET_{u}_LINKER"),
                format!("CARGO_TARGET_{u}_RUSTFLAGS"),
            ]
        })
        .collect();
    for k in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_RUSTFLAGS",
        "CARGO_BUILD_TARGET",
    ] {
        v.push(k.to_string());
    }
    v
}
/// Default-linker (host `cc`, GNU ld) rebuild of 9d1a4f3, measured 2026-10-28
/// ~08:12 +01:00 (slot 376) and again by slot 386 (`NERVE_XT_LIVE_CONFIG=archive`).
const LIVE_DEFAULT_LINKER_SHA: &str =
    "64ce3aa03bd55a9c698f5c42c4616c6987163d2523fa1ac89aff0187571213f8";
const LIVE_DEFAULT_LINKER_SIZE: u64 = 1_463_072;

/// Where the live-pin rebuild gets its x86_64 linker (slot 386, D-376-2):
/// `NERVE_XT_LIVE_CONFIG`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveConfig {
    /// Default: this tree's committed `.cargo/config.toml` (`git show
    /// HEAD:.cargo/config.toml`) overlaid on the archive, linker env removed.
    Committed,
    /// Slot 376: the archive's own config plus the linker env var.
    Env,
    /// Negative control: the archive's own config, linker env removed (what a
    /// deploy gets without the committed line; must NOT reproduce).
    Archive,
}

fn live_config_from(v: Option<&str>) -> Result<LiveConfig, String> {
    match v {
        None | Some("committed") => Ok(LiveConfig::Committed),
        Some("env") => Ok(LiveConfig::Env),
        Some("archive") => Ok(LiveConfig::Archive),
        Some(v) => Err(format!(
            "NERVE_XT_LIVE_CONFIG must be committed, env or archive, got {v:?}"
        )),
    }
}

fn live_config_mode() -> LiveConfig {
    let v = std::env::var("NERVE_XT_LIVE_CONFIG").ok();
    live_config_from(v.as_deref()).unwrap_or_else(|e| panic!("{e}"))
}

/// Linker set for `[target.<triple>]` in a cargo config text (tiny line
/// parser: section headers and `linker = "..."`; enough for our own file).
fn config_linker(text: &str, triple: &str) -> Option<String> {
    let want = format!("[target.{triple}]");
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == want;
            continue;
        }
        if !inside {
            continue;
        }
        if let Some(rest) = line.strip_prefix("linker") {
            let v = rest.trim_start().strip_prefix('=')?.trim();
            return Some(v.trim_matches('"').to_string());
        }
    }
    None
}

/// `git show HEAD:.cargo/config.toml` of this repo (the COMMITTED config,
/// not the working copy).
fn committed_config() -> String {
    let out = Command::new("git")
        .args(["show", "HEAD:.cargo/config.toml"])
        .current_dir(root())
        .output()
        .expect("git show HEAD:.cargo/config.toml");
    assert!(
        out.status.success(),
        "no committed .cargo/config.toml at HEAD"
    );
    String::from_utf8(out.stdout).expect("config utf-8")
}

/// Replace the exported tree's `.cargo/config.toml` with this tree's
/// committed one.
fn overlay_committed_config(src: &Path) {
    let text = committed_config();
    assert_eq!(
        config_linker(&text, "x86_64-unknown-linux-musl").as_deref(),
        Some(LIVE_RECIPE_LINKER),
        "committed .cargo/config.toml has no x86_64 musl rust-lld linker line (D-376-2)"
    );
    std::fs::create_dir_all(src.join(".cargo")).unwrap();
    std::fs::write(src.join(".cargo/config.toml"), text).unwrap();
}

#[test]
fn live_config_parsing() {
    assert_eq!(live_config_from(None), Ok(LiveConfig::Committed));
    assert_eq!(
        live_config_from(Some("committed")),
        Ok(LiveConfig::Committed)
    );
    assert_eq!(live_config_from(Some("env")), Ok(LiveConfig::Env));
    assert_eq!(live_config_from(Some("archive")), Ok(LiveConfig::Archive));
    assert!(live_config_from(Some("")).is_err());
    assert!(live_config_from(Some("yes")).is_err());
}

#[test]
fn config_linker_parser() {
    let t = "[env]\nX = \"1\"\n[target.aarch64-unknown-linux-musl]\nlinker = \"rust-lld\"\n\n[target.x86_64-unknown-linux-musl]\n# c\nlinker=\"cc\"\n";
    assert_eq!(
        config_linker(t, "aarch64-unknown-linux-musl").as_deref(),
        Some("rust-lld")
    );
    assert_eq!(
        config_linker(t, "x86_64-unknown-linux-musl").as_deref(),
        Some("cc")
    );
    assert_eq!(config_linker(t, "armv7-unknown-linux-musleabihf"), None);
    assert_eq!(
        config_linker("[env]\nlinker = \"x\"\n", "x86_64-unknown-linux-musl"),
        None
    );
}

/// Always on (slot 386, D-376-2): the repo's own config (working copy, so a
/// local edit that drops the line fails before commit) links every musl
/// target with rust-lld, and the 9d1a4f3 config did not (the reason the
/// live recipe needed an uncommitted edit).
#[test]
fn repo_config_pins_rust_lld() {
    let text = std::fs::read_to_string(root().join(".cargo/config.toml")).unwrap();
    for t in [
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-musl",
        "armv7-unknown-linux-musleabihf",
    ] {
        assert_eq!(config_linker(&text, t).as_deref(), Some("rust-lld"), "{t}");
    }
    // Default-linker rebuild of the live commit is a different binary.
    assert!(pin_verdict(
        LIVE_REV,
        (LIVE_SHA, LIVE_SIZE),
        (LIVE_DEFAULT_LINKER_SHA, LIVE_DEFAULT_LINKER_SIZE)
    )
    .is_err());
}
/// Hard ceiling from the low-end profile (every Rust service binary < 2 MiB).
const BUDGET: u64 = 2 * 1024 * 1024;
/// Size-budget table (slot 376, `NERVE_XT_PIN=budget`/`live`): label ->
/// (budget bytes, warn bytes). Over budget fails; over warn only prints.
/// Warn = 90 % of the ceiling so growth shows up before the hard stop.
const SIZE_BUDGET: [(&str, u64, u64); 5] = [
    ("x86_64", BUDGET, BUDGET / 10 * 9),
    ("aarch64", BUDGET, BUDGET / 10 * 9),
    ("armv7", BUDGET, BUDGET / 10 * 9),
    ("x86_64-live-recipe", BUDGET, BUDGET / 10 * 9),
    ("live-pin", BUDGET, BUDGET / 10 * 9),
];
/// Slot 363 (real-test-port-env): 5-port block, first port from
/// `NERVE_XT_PORT_BASE` (unset = 20870, slot 217): nerves on +0..+2, live
/// service thread on +3, dead component on +4.
const PORT_BASE_ENV: &str = "NERVE_XT_PORT_BASE";
const DEFAULT_PORT_BASE: u16 = 20870;

fn port_base() -> u16 {
    galion_realtest::ports::base_from_env(PORT_BASE_ENV, DEFAULT_PORT_BASE, 5)
}
fn port_svc() -> u16 {
    port_base() + 3
}
fn port_dead() -> u16 {
    port_base() + 4
}

/// Which pin the test enforces (slot 376, D-363-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PinMode {
    /// No binary pin: size-budget table + solo == recipe (default).
    Budget,
    /// Budget, plus rebuild the live commit and match the VPS binary.
    Live,
    /// Slot-217 aa77864 pin (passes only on the aa77864 tree).
    Aa77864,
}

/// `NERVE_XT_PIN` wins; else the slot-363 knob `NERVE_XT_LIVE_PIN`
/// (`0` = budget, `1` = aa77864); both unset = budget.
fn pin_mode_from(pin: Option<&str>, legacy: Option<&str>) -> Result<PinMode, String> {
    match pin {
        Some("budget") => return Ok(PinMode::Budget),
        Some("live") => return Ok(PinMode::Live),
        Some("aa77864") => return Ok(PinMode::Aa77864),
        Some(v) => {
            return Err(format!(
                "NERVE_XT_PIN must be budget, live or aa77864, got {v:?}"
            ))
        }
        None => {}
    }
    match legacy {
        None => Ok(PinMode::Budget),
        Some("0") => Ok(PinMode::Budget),
        Some("1") => Ok(PinMode::Aa77864),
        Some(v) => Err(format!("NERVE_XT_LIVE_PIN must be 0 or 1, got {v:?}")),
    }
}

fn pin_mode() -> PinMode {
    let pin = std::env::var("NERVE_XT_PIN").ok();
    let legacy = std::env::var("NERVE_XT_LIVE_PIN").ok();
    pin_mode_from(pin.as_deref(), legacy.as_deref()).unwrap_or_else(|e| panic!("{e}"))
}

/// Size-budget verdict for one binary: `Err` = over budget or unknown label,
/// `Ok(Some(_))` = warning, `Ok(None)` = fine.
fn budget_verdict(label: &str, size: u64) -> Result<Option<String>, String> {
    let Some(&(_, budget, warn)) = SIZE_BUDGET.iter().find(|r| r.0 == label) else {
        return Err(format!("{label}: no row in SIZE_BUDGET"));
    };
    if size > budget {
        return Err(format!(
            "{label}: {size} B over budget {budget} B by {} B",
            size - budget
        ));
    }
    if size > warn {
        return Ok(Some(format!(
            "{label}: {size} B above warn {warn} B, headroom {} B to {budget} B",
            budget - size
        )));
    }
    Ok(None)
}

/// Pin verdict: the binary rebuilt from the pinned commit must equal the
/// pinned VPS binary (SHA-256 and size).
fn pin_verdict(rev: &str, want: (&str, u64), got: (&str, u64)) -> Result<(), String> {
    if got.0 == want.0 && got.1 == want.1 {
        return Ok(());
    }
    Err(format!(
        "rebuild of {rev} = {} {} B, pinned VPS binary = {} {} B: either the live nerve was redeployed (bump NERVE_XT_LIVE_REV/SHA/SIZE after a read-only sha256sum on the VPS) or the toolchain/recipe changed",
        got.0, got.1, want.0, want.1
    ))
}

#[test]
fn pin_mode_parsing() {
    use PinMode::*;
    assert_eq!(pin_mode_from(None, None), Ok(Budget));
    assert_eq!(pin_mode_from(Some("budget"), None), Ok(Budget));
    assert_eq!(pin_mode_from(Some("live"), None), Ok(Live));
    assert_eq!(pin_mode_from(Some("aa77864"), None), Ok(Aa77864));
    assert_eq!(pin_mode_from(None, Some("0")), Ok(Budget));
    assert_eq!(pin_mode_from(None, Some("1")), Ok(Aa77864));
    // NERVE_XT_PIN wins over the slot-363 knob.
    assert_eq!(pin_mode_from(Some("live"), Some("1")), Ok(Live));
    assert!(pin_mode_from(Some("yes"), None).is_err());
    assert!(pin_mode_from(Some(""), None).is_err());
    assert!(pin_mode_from(None, Some("2")).is_err());
}

#[test]
fn size_budget_table() {
    // Every label the test builds has a row; budgets never exceed the ceiling.
    for l in [
        "x86_64",
        "aarch64",
        "armv7",
        "x86_64-live-recipe",
        "live-pin",
    ] {
        assert!(SIZE_BUDGET.iter().any(|r| r.0 == l), "{l}");
    }
    for (l, b, w) in SIZE_BUDGET {
        assert!(b <= BUDGET && w < b, "{l}");
    }
    // Real sizes seen 2026-10-28: live 9d1a4f3 fine, tree 0d545118 warns,
    // one byte over the ceiling fails, unknown label fails.
    assert_eq!(budget_verdict("x86_64", LIVE_SIZE), Ok(None));
    assert!(budget_verdict("x86_64", 1_930_048).unwrap().is_some());
    assert_eq!(
        budget_verdict("aarch64", BUDGET).map(|w| w.is_some()),
        Ok(true)
    );
    assert!(budget_verdict("aarch64", BUDGET + 1).is_err());
    assert!(budget_verdict("riscv64", 1).is_err());
}

#[test]
fn live_pin_verdict() {
    assert!(pin_verdict(LIVE_REV, (LIVE_SHA, LIVE_SIZE), (LIVE_SHA, LIVE_SIZE)).is_ok());
    // Default-linker rebuild of 9d1a4f3 (measured): different bytes -> fail.
    let e = pin_verdict(
        LIVE_REV,
        (LIVE_SHA, LIVE_SIZE),
        (
            "64ce3aa03bd55a9c698f5c42c4616c6987163d2523fa1ac89aff0187571213f8",
            1_463_072,
        ),
    )
    .unwrap_err();
    assert!(e.contains("redeployed"), "{e}");
    // Same size, different hash still fails.
    assert!(pin_verdict(LIVE_REV, (LIVE_SHA, LIVE_SIZE), (AA77864_SHA, LIVE_SIZE)).is_err());
}

/// `git archive <rev>` of the workspace repo into `dst` (fresh), so the live
/// commit is rebuilt from its own tree no matter what is checked out.
fn export_rev(rev: &str, dst: &Path) {
    let _ = std::fs::remove_dir_all(dst);
    std::fs::create_dir_all(dst).unwrap();
    let ok = Command::new("git")
        .args(["cat-file", "-e", &format!("{rev}^{{commit}}")])
        .current_dir(root())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(
        ok,
        "commit {rev} not in this repo: git fetch <remote> swarm/nerve-deploy-t273u (or set NERVE_XT_LIVE_REV)"
    );
    let mut archive = Command::new("git")
        .args(["archive", "--format=tar", rev])
        .current_dir(root())
        .stdout(Stdio::piped())
        .spawn()
        .expect("git archive");
    let st = Command::new("tar")
        .args(["-x", "-C"])
        .arg(dst)
        .stdin(archive.stdout.take().unwrap())
        .status()
        .expect("tar -x");
    let gs = archive.wait().expect("git archive wait");
    assert!(
        gs.success() && st.success(),
        "git archive | tar -x of {rev} failed"
    );
}

// ---------------------------------------------------------------- SHA-256
// FIPS 180-4, straight from the spec; used only to fingerprint the binaries.
fn sha256(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for block in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, c) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for (a, b) in h.iter_mut().zip(v) {
            *a = a.wrapping_add(b);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[test]
fn sha256_known_answers() {
    // FIPS 180-4 examples; always on (cheap), so the fingerprint code is trusted.
    assert_eq!(
        sha256(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

// ---------------------------------------------------------------- ELF
#[derive(Debug, PartialEq)]
struct Elf {
    machine: u16,
    has_interp: bool,
    pie: bool,
}

fn elf(b: &[u8]) -> Elf {
    assert!(b.len() > 64 && &b[..4] == b"\x7fELF", "not an ELF");
    assert_eq!(b[5], 1, "not little-endian");
    let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let u64_at = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    // Slot 409: ELF32 too (armv7 musl); same e_type/e_machine offsets.
    let (phoff, phentsize, phnum) = match b[4] {
        2 => (
            u64_at(32) as usize,
            u16_at(54) as usize,
            u16_at(56) as usize,
        ),
        1 => (
            u32_at(28) as usize,
            u16_at(42) as usize,
            u16_at(44) as usize,
        ),
        c => panic!("unknown ELF class {c}"),
    };
    let has_interp = (0..phnum).any(|i| u32_at(phoff + i * phentsize) == 3); // PT_INTERP
    Elf {
        machine: u16_at(18),
        has_interp,
        pie: u16_at(16) == 3, // ET_DYN
    }
}

// ---------------------------------------------------------------- HTTP
struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn http(port: u16, method: &str, path: &str, body: &str) -> Option<(u16, String)> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    write!(
        s,
        "{method} {path} HTTP/1.0\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (h, body) = b.split_once("\r\n\r\n")?;
    Some((h.split_whitespace().nth(1)?.parse().ok()?, body.to_string()))
}

fn keys(v: &Value) -> Vec<String> {
    v.as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// Key order as sent on the wire (the test's own serde_json may sort keys on
/// parse, so order is read from the raw body). Flat objects only.
fn wire_order(raw: &str, v: &Value) -> Vec<String> {
    let mut k: Vec<(usize, String)> = keys(v)
        .into_iter()
        .map(|k| (raw.find(&format!("\"{k}\":")).unwrap_or(usize::MAX), k))
        .collect();
    k.sort();
    k.into_iter().map(|x| x.1).collect()
}

fn sorted(mut k: Vec<String>) -> Vec<String> {
    k.sort();
    k
}

// ---------------------------------------------------------------- build
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

fn run_cargo(args: &[&str], dir: &Path) -> (bool, f64, String) {
    run_cargo_env(args, dir, &root(), &[], &[])
}

/// `run_cargo` in another source tree (`cwd`) with extra env (slot 376: the
/// live-pin rebuild of an exported commit) and env vars removed (slot 386:
/// prove the committed config alone picks the linker).
fn run_cargo_env(
    args: &[&str],
    dir: &Path,
    cwd: &Path,
    extra_env: &[(&str, &str)],
    remove_env: &[&str],
) -> (bool, f64, String) {
    let mut cmd = Command::new(cargo());
    for k in ambient_env_vars() {
        cmd.env_remove(k);
    }
    for k in remove_env {
        cmd.env_remove(k);
    }
    let jobs = std::env::var("NERVE_XT_JOBS").unwrap_or_else(|_| "2".into());
    let t = Instant::now();
    let out = cmd
        .args(args)
        .args(["-j", &jobs, "--target-dir"])
        .arg(dir)
        .current_dir(cwd)
        .envs(extra_env.iter().copied())
        .env("CARGO_INCREMENTAL", "0")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn cargo");
    let err = String::from_utf8_lossy(&out.stderr);
    let tail: String = err.lines().rev().take(8).collect::<Vec<_>>().join(" | ");
    (out.status.success(), t.elapsed().as_secs_f64(), tail)
}

/// qemu user-mode binary: env knob first, else the first of `names` on PATH
/// (`NERVE_XT_QEMU` for aarch64, slot 409 `NERVE_XT_QEMU_ARM` for armv7).
fn find_qemu(knob: &str, names: &[&str]) -> Option<PathBuf> {
    if let Ok(p) = std::env::var(knob) {
        return Some(PathBuf::from(p));
    }
    let path = std::env::var("PATH").unwrap_or_default();
    for name in names {
        for d in path.split(':') {
            let p = Path::new(d).join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

// ---------------------------------------------------------------- smoke
struct Smoke {
    health: Value,
    health_raw: String,
    signal_code: u16,
    bad_signal_code: u16,
    state: Value,
    signals: Value,
    start_s: f64,
}

fn smoke(label: &str, prog: &Path, args: &[&Path], port: u16, dir: &Path) -> Smoke {
    let data = dir.join(format!("data-{label}"));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let comps = dir.join(format!("components-{label}.json"));
    std::fs::write(
        &comps,
        format!(
            r#"[{{"name":"xt-live","port":{svc}}},{{"name":"xt-dead","port":{dead},"gaia":"APOLLO"}}]"#,
            svc = port_svc(),
            dead = port_dead()
        ),
    )
    .unwrap();
    let mut cmd = Command::new(prog);
    cmd.args(args)
        .arg("serve")
        .env("NERVE_ADDR", format!("127.0.0.1:{port}"))
        .env("NERVE_INTERVAL", "5")
        .env("NERVE_DATA_DIR", &data)
        .env("NERVE_COMPONENTS", &comps)
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_DRY_RUN", "1")
        .env("NERVE_ROLLBACK", "0")
        .env("NERVE_DISK_PATH", &data)
        .env(
            "NERVE_LISTEN_SCOPE",
            format!("{}-{}", port_base(), port_base() + 4),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let t = Instant::now();
    let _nerve = Kill(cmd.spawn().expect("spawn nerve"));
    // First cycle must be done (cycle >= 1) so /state has components.
    let (health, health_raw) = loop {
        if let Some((200, b)) = http(port, "GET", "/health", "") {
            let v: Value = serde_json::from_str(&b).unwrap_or_default();
            if v["cycle"].as_u64().unwrap_or(0) >= 1 {
                break (v, b);
            }
        }
        assert!(
            t.elapsed() < Duration::from_secs(60),
            "{label}: no /health with cycle>=1 within 60 s"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let start_s = t.elapsed().as_secs_f64();
    let sig =
        format!(r#"{{"source":"xt-{label}","kind":"error","name":"cross","msg":"slot 217"}}"#);
    let signal_code = http(port, "POST", "/signal", &sig)
        .map(|r| r.0)
        .unwrap_or(0);
    let bad_signal_code = http(port, "POST", "/signal", r#"{"source":"x"}"#)
        .map(|r| r.0)
        .unwrap_or(0);
    let state = http(port, "GET", "/state", "")
        .filter(|r| r.0 == 200)
        .and_then(|r| serde_json::from_str(&r.1).ok())
        .unwrap_or_default();
    let signals = http(port, "GET", "/api/v1/signals", "")
        .filter(|r| r.0 == 200)
        .and_then(|r| serde_json::from_str(&r.1).ok())
        .unwrap_or_default();
    drop(_nerve);
    let _ = std::fs::remove_dir_all(&data);
    let _ = std::fs::remove_file(&comps);
    Smoke {
        health,
        health_raw,
        signal_code,
        bad_signal_code,
        state,
        signals,
        start_s,
    }
}

fn comp_status(st: &Value, name: &str) -> String {
    st["components"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["name"] == name))
        .map(|c| c["status"].as_str().unwrap_or("").to_string())
        .unwrap_or_default()
}

#[test]
fn real_cross_target() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let started = zagreb_time::now_rfc3339();
    let xt = std::env::var("NERVE_XT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root().join("target/xt"));
    std::fs::create_dir_all(&xt).unwrap();
    let mode = pin_mode();
    // Pinned VPS binary for the chosen mode (budget mode only reports it).
    let (def_rev, def_sha, def_size) = match mode {
        PinMode::Aa77864 => ("aa77864", AA77864_SHA, AA77864_SIZE),
        _ => (LIVE_REV, LIVE_SHA, LIVE_SIZE),
    };
    let live_rev = std::env::var("NERVE_XT_LIVE_REV").unwrap_or_else(|_| def_rev.into());
    let live_sha = std::env::var("NERVE_XT_LIVE_SHA").unwrap_or_else(|_| def_sha.into());
    let live_size: u64 = std::env::var("NERVE_XT_LIVE_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(def_size);
    eprintln!("[xt] pin mode {mode:?}, pinned VPS binary {live_rev} {live_sha} {live_size} B");
    // Slot 409: the COMMITTED config (HEAD) must carry every musl linker
    // line, since run_cargo_env strips all linker env vars.
    let head_cfg = committed_config();
    for t in &XT_TRIPLES[..3] {
        assert_eq!(
            config_linker(&head_cfg, t).as_deref(),
            Some("rust-lld"),
            "committed .cargo/config.toml: no rust-lld linker for {t}"
        );
    }
    eprintln!(
        "[xt] linker env removed for every cargo run: {:?}",
        ambient_env_vars()
    );

    // 1. builds -------------------------------------------------------
    let x86 = "x86_64-unknown-linux-musl";
    let a64 = "aarch64-unknown-linux-musl";
    let arm = "armv7-unknown-linux-musleabihf";
    let builds: [(&str, &str, &[&str]); 4] = [
        ("x86_64", x86, &["-p", "openmind-nerve"]),
        ("aarch64", a64, &["-p", "openmind-nerve"]),
        (
            "x86_64-live-recipe",
            x86,
            &["-p", "openmind-nerve", "-p", "openmind-memory"],
        ),
        // Slot 409: armv7 musl is linked and smoked too (was cargo check only).
        ("armv7", arm, &["-p", "openmind-nerve"]),
    ];
    let mut bins = Vec::new();
    let mut report_bins = Vec::new();
    let mut warnings = Vec::new();
    let mut record = |label: &str,
                      target: &str,
                      pkgs: &[&str],
                      bin: PathBuf,
                      secs: f64,
                      bins: &mut Vec<(String, PathBuf, String, u64)>| {
        let bytes = std::fs::read(&bin).expect("binary");
        let e = elf(&bytes);
        let want_machine = match target {
            t if t == a64 => 0xB7,
            t if t == arm => 0x28,
            _ => 0x3E,
        };
        assert_eq!(e.machine, want_machine, "{label}: wrong e_machine");
        assert!(!e.has_interp, "{label}: has PT_INTERP (not static)");
        let size = bytes.len() as u64;
        assert!(size < BUDGET, "{label}: {size} B over 2 MiB budget");
        let budget = if mode == PinMode::Aa77864 {
            None
        } else {
            match budget_verdict(label, size) {
                Err(e) => panic!("size budget: {e}"),
                Ok(w) => {
                    if let Some(w) = &w {
                        eprintln!("[xt] WARN size budget: {w}");
                        warnings.push(w.clone());
                    }
                    w
                }
            }
        };
        let sha = sha256(&bytes);
        eprintln!(
            "[xt] {label:<20} {size:>9} B  sha256 {sha}  pie={} build {secs:.1} s",
            e.pie
        );
        let row = SIZE_BUDGET.iter().find(|r| r.0 == label);
        report_bins.push(json!({
            "label": label, "target": target, "packages": pkgs, "size": size,
            "sha256": sha, "pie": e.pie, "static": !e.has_interp,
            "e_machine": format!("{:#x}", e.machine), "build_s": (secs * 10.0).round() / 10.0,
            "size_vs_live": size as i64 - live_size as i64, "equals_live": sha == live_sha,
            "budget": row.map(|r| r.1), "warn": row.map(|r| r.2),
            "headroom": row.map(|r| r.1 as i64 - size as i64), "budget_warning": budget,
        }));
        bins.push((label.to_string(), bin, sha, size));
    };
    for (label, target, pkgs) in builds {
        let dir = xt.join(label);
        let mut args = vec!["build", "--release", "--locked", "--target", target];
        args.extend_from_slice(pkgs);
        let (ok, secs, tail) = run_cargo(&args, &dir);
        assert!(ok, "{label}: cargo build failed: {tail}");
        let bin = dir.join(target).join("release/openmind-nerve");
        record(label, target, pkgs, bin, secs, &mut bins);
    }
    // Live pin (slot 376, option A): rebuild the live commit from its own
    // tree with the deploy recipe; must equal the VPS binary bit for bit.
    if mode == PinMode::Live {
        let src = xt.join(format!("pin-src-{live_rev}"));
        export_rev(&live_rev, &src);
        let dir = xt.join(format!("pin-{live_rev}"));
        let pkgs: &[&str] = &["-p", "openmind-nerve"];
        let mut args = vec!["build", "--release", "--locked", "--target", x86];
        args.extend_from_slice(pkgs);
        // Slot 386 (D-376-2): by default the linker comes from the committed
        // `.cargo/config.toml` of THIS tree (HEAD, not the working copy),
        // overlaid on the archive, with every linker env var removed; `env`
        // is the slot-376 way (archive config + linker env var).
        let cfg = live_config_mode();
        let (env, rm): (Vec<(&str, &str)>, Vec<&str>) = match cfg {
            LiveConfig::Committed => {
                overlay_committed_config(&src);
                (Vec::new(), vec![X86_LINKER_ENV])
            }
            LiveConfig::Env => (vec![(X86_LINKER_ENV, LIVE_RECIPE_LINKER)], Vec::new()),
            LiveConfig::Archive => (Vec::new(), vec![X86_LINKER_ENV]),
        };
        eprintln!("[xt] live-pin config {cfg:?}");
        let (ok, secs, tail) = run_cargo_env(&args, &dir, &src, &env, &rm);
        assert!(ok, "live-pin {live_rev}: cargo build failed: {tail}");
        let bin = dir.join(x86).join("release/openmind-nerve");
        record("live-pin", x86, pkgs, bin, secs, &mut bins);
        let (_, _, sha, size) = bins.last().expect("live-pin binary");
        if let Err(e) = pin_verdict(&live_rev, (&live_sha, live_size), (sha, *size)) {
            panic!("live pin: {e}");
        }
        eprintln!("[xt] live pin OK: rebuild of {live_rev} == VPS binary {live_sha} {live_size} B");
        let _ = std::fs::remove_dir_all(&src);
    }
    let (_, _, recipe_sha, recipe_size) = &bins[2];
    match mode {
        PinMode::Aa77864 => {
            // Slot 217: the live recipe must reproduce aa77864 bit for bit;
            // the solo build differs only by serde_json preserve_order.
            assert_eq!(
                recipe_sha, &live_sha,
                "live recipe no longer reproduces the VPS binary"
            );
            assert_eq!(*recipe_size, live_size);
            assert_ne!(
                bins[0].2, live_sha,
                "solo -p openmind-nerve build now equals live (preserve_order no longer differs?)"
            );
        }
        PinMode::Budget | PinMode::Live => {
            // Tree-level reproducibility: two target dirs, two package sets,
            // one binary (serde_json preserve_order is unified since 8555a8f7).
            assert_eq!(
                bins[0].2, *recipe_sha,
                "solo and live-recipe x86_64 builds differ: the nerve binary depends on what is co-built (feature unification)"
            );
            assert_eq!(bins[0].3, *recipe_size);
            eprintln!(
                "[xt] solo == recipe ({recipe_sha} {recipe_size} B); vs pinned VPS {live_rev}: {:+} B",
                *recipe_size as i64 - live_size as i64
            );
        }
    }

    // 2. qemu smoke -----------------------------------------------------
    let qemu = find_qemu("NERVE_XT_QEMU", &["qemu-aarch64-static", "qemu-aarch64"])
        .expect("qemu-aarch64 not found (set NERVE_XT_QEMU)");
    let qemu_arm = find_qemu("NERVE_XT_QEMU_ARM", &["qemu-arm-static", "qemu-arm"])
        .expect("qemu-arm not found (set NERVE_XT_QEMU_ARM)");
    let svc = TcpListener::bind(("127.0.0.1", port_svc()))
        .unwrap_or_else(|e| panic!("live-service port {} not free: {e}", port_svc()));
    std::thread::spawn(move || {
        for mut c in svc.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\nok\n");
        }
    });
    assert!(
        TcpStream::connect(("127.0.0.1", port_dead())).is_err(),
        "{} must be free (dead component)",
        port_dead()
    );
    // --version under qemu (sanity: the aarch64 / armv7 binaries really execute).
    let qemu_version = |q: &Path, bin: &Path| {
        let v = Command::new(q)
            .arg(bin)
            .arg("--version")
            .output()
            .expect("qemu --version");
        let ver = String::from_utf8_lossy(&v.stdout).trim().to_string();
        assert!(
            ver.starts_with("openmind-nerve "),
            "qemu --version {}: {ver:?}",
            bin.display()
        );
        ver
    };
    let ver = qemu_version(&qemu, &bins[1].1);
    let ver_arm = qemu_version(&qemu_arm, &bins[3].1);

    let mut runs = vec![
        ("x86_64", bins[0].1.clone(), None, port_base()),
        (
            "aarch64",
            bins[1].1.clone(),
            Some(qemu.clone()),
            port_base() + 1,
        ),
        (
            "x86_64-live-recipe",
            bins[2].1.clone(),
            None,
            port_base() + 2,
        ),
    ];
    // Slot 409: armv7 under qemu-arm reuses +1 after the aarch64 run.
    runs.push((
        "armv7",
        bins[3].1.clone(),
        Some(qemu_arm.clone()),
        port_base() + 1,
    ));
    // The rebuilt live binary reuses +2 after the recipe run (runs are
    // sequential and every port is checked closed after its run).
    if mode == PinMode::Live {
        runs.push(("live-pin", bins[4].1.clone(), None, port_base() + 2));
    }
    let mut smokes = Vec::new();
    let mut report_smoke = Vec::new();
    for (label, bin, emu, port) in runs {
        let s = match &emu {
            Some(q) => smoke(label, q, &[bin.as_path()], port, &xt),
            None => smoke(label, &bin, &[], port, &xt),
        };
        assert_eq!(
            s.health["service"], "openmind-nerve",
            "{label}: {}",
            s.health
        );
        assert_eq!(s.health["status"], "ok", "{label}");
        assert_eq!(s.signal_code, 202, "{label}: POST /signal");
        assert_eq!(s.bad_signal_code, 400, "{label}: POST /signal without kind");
        assert_eq!(comp_status(&s.state, "xt-live"), "healthy", "{label}");
        assert_eq!(comp_status(&s.state, "xt-dead"), "failing", "{label}");
        let src = format!("xt-{label}");
        let seen = s.signals.to_string().contains(&src);
        assert!(seen, "{label}: signal source {src} not in /api/v1/signals");
        assert!(
            TcpStream::connect(("127.0.0.1", port)).is_err(),
            "{label}: port {port} still open after kill"
        );
        eprintln!(
            "[xt] smoke {label:<20} :{port} first cycle {:.2} s  /health wire order {:?}  state keys {}  system {}",
            s.start_s,
            wire_order(&s.health_raw, &s.health),
            keys(&s.state).len(),
            s.state["system"]["status"]
        );
        report_smoke.push(json!({
            "label": label, "port": port, "emulated": emu.is_some(),
            "first_cycle_s": (s.start_s * 100.0).round() / 100.0,
            "health": s.health, "health_wire_order": wire_order(&s.health_raw, &s.health),
            "signal_post": s.signal_code, "bad_signal_post": s.bad_signal_code,
            "state_keys": keys(&s.state), "system": s.state["system"]["status"],
            "xt_live": comp_status(&s.state, "xt-live"), "xt_dead": comp_status(&s.state, "xt-dead"),
            "signal_seen": seen,
        }));
        smokes.push(s);
    }
    // Same answers on both architectures (shape, statuses, system verdict).
    let (x, a, r) = (&smokes[0], &smokes[1], &smokes[2]);
    assert_eq!(
        keys(&x.health),
        keys(&a.health),
        "health keys x86 vs aarch64"
    );
    assert_eq!(keys(&x.state), keys(&a.state), "state keys x86 vs aarch64");
    assert_eq!(
        x.state["system"]["status"], a.state["system"]["status"],
        "system verdict x86 vs aarch64"
    );
    assert_eq!(sorted(keys(&r.health)), sorted(keys(&x.health)));
    assert_eq!(sorted(keys(&r.state)), sorted(keys(&x.state)));
    let xo = wire_order(&x.health_raw, &x.health);
    let ao = wire_order(&a.health_raw, &a.health);
    let ro = wire_order(&r.health_raw, &r.health);
    assert_eq!(ao, xo, "/health wire order x86_64 vs aarch64");
    // Slot 409: armv7 (32-bit, qemu-arm) answers like x86_64 too.
    let v7 = &smokes[3];
    assert_eq!(
        keys(&x.health),
        keys(&v7.health),
        "health keys x86 vs armv7"
    );
    assert_eq!(keys(&x.state), keys(&v7.state), "state keys x86 vs armv7");
    assert_eq!(
        x.state["system"]["status"], v7.state["system"]["status"],
        "system verdict x86 vs armv7"
    );
    assert_eq!(
        wire_order(&v7.health_raw, &v7.health),
        xo,
        "/health wire order x86_64 vs armv7"
    );
    if mode == PinMode::Aa77864 {
        // Live recipe (preserve_order on): insertion order on the wire; solo
        // builds (BTreeMap) send keys sorted.
        assert_eq!(
            xo,
            sorted(xo.clone()),
            "solo x86_64 /health keys not sorted"
        );
        assert_eq!(
            ro.first().map(String::as_str),
            Some("status"),
            "live recipe order {ro:?}"
        );
    } else {
        // serde_json preserve_order is unified, so solo and recipe must agree
        // on the wire order too.
        assert_eq!(ro, xo, "/health wire order recipe vs solo");
        eprintln!("[xt] /health wire order (all 3 equal): {ro:?}");
    }
    let mut live_pin_report = Value::Null;
    if mode == PinMode::Live {
        // Same /health key set as the tree (no field dropped or added since
        // the live commit); wire order is reported, not asserted (the live
        // 9d1a4f3 build sends keys sorted, the tree sends insertion order).
        let l = &smokes[4];
        assert_eq!(
            sorted(keys(&l.health)),
            sorted(keys(&x.health)),
            "/health key set live-pin vs tree"
        );
        let lo = wire_order(&l.health_raw, &l.health);
        eprintln!(
            "[xt] live-pin /health wire order {lo:?} (sorted: {}), tree order equal: {}",
            lo == sorted(lo.clone()),
            lo == xo
        );
        live_pin_report = json!({
            "rev": live_rev, "wire_order": lo, "wire_sorted": lo == sorted(lo.clone()),
            "wire_equals_tree": lo == xo,
            "state_keys_equal_tree": sorted(keys(&l.state)) == sorted(keys(&x.state)),
        });
    }

    // 3. cargo check for android ----------------------------------------
    // Slot 409: armv7 musl moved to the build + smoke list above. Android
    // links dynamically against bionic (-lc -ldl -lm -llog -lunwind), which
    // only the Android NDK ships; with no NDK on the box neither the default
    // `cc` nor rust-lld can link it (docs/real-tests/xt-linker-pins-arm.md,
    // D-409-1), so android stays `cargo check` (RULES.md hard rule 1) and is
    // deliberately NOT pinned in `.cargo/config.toml`.
    let mut report_check = Vec::new();
    for target in ["aarch64-linux-android", "armv7-linux-androideabi"] {
        let (ok, secs, tail) = run_cargo(
            &[
                "check",
                "--locked",
                "--target",
                target,
                "-p",
                "openmind-nerve",
            ],
            &xt.join("check"),
        );
        assert!(ok, "{target}: cargo check failed: {tail}");
        eprintln!("[xt] check {target} ok {secs:.1} s");
        report_check.push(
            json!({"target": target, "ok": ok, "secs": (secs * 10.0).round() / 10.0,
                "link": "not attempted: needs the Android NDK bionic sysroot (D-409-1)"}),
        );
    }

    let report = json!({
        "slot": 217, "pin_slot": 376, "started": started, "finished": zagreb_time::now_rfc3339(),
        "pin_mode": format!("{mode:?}").to_lowercase(),
        "live": {"rev": live_rev, "sha256": live_sha, "size": live_size, "pinned": mode != PinMode::Budget},
        "live_pin": live_pin_report,
        "size_budget_warnings": warnings,
        "port_base": port_base(),
        "binaries": report_bins, "smoke": report_smoke, "check": report_check,
        "qemu": qemu.display().to_string(), "qemu_version_out": ver,
        "qemu_arm": qemu_arm.display().to_string(), "qemu_arm_version_out": ver_arm,
        "linker_slot": 409, "linker_env_removed": ambient_env_vars(),
        "config_linkers": XT_TRIPLES.iter().map(|t| json!({"target": t,
            "linker": config_linker(&committed_config(), t)})).collect::<Vec<_>>(),
    });
    let out = std::env::var("NERVE_XT_REPORT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| xt.join("report.json"));
    std::fs::write(&out, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    eprintln!("[xt] report {}", out.display());
}

/// Slot 409: every `[target.<triple>]` table in the repo config has its
/// linker env var in the strip list (so a future pin is always tested from
/// the committed config), the musl triples are pinned and android is not
/// (no NDK: a rust-lld pin would only trade one link error for another).
#[test]
fn ambient_env_covers_config_targets() {
    let text = std::fs::read_to_string(root().join(".cargo/config.toml")).unwrap();
    let strip = ambient_env_vars();
    for line in text.lines().map(str::trim) {
        if let Some(t) = line
            .strip_prefix("[target.")
            .and_then(|r| r.strip_suffix(']'))
        {
            let k = format!("CARGO_TARGET_{}_LINKER", triple_env(t));
            assert!(strip.contains(&k), "{k} not stripped by run_cargo_env");
        }
    }
    for t in &XT_TRIPLES[..3] {
        assert_eq!(config_linker(&text, t).as_deref(), Some("rust-lld"), "{t}");
    }
    for t in &XT_TRIPLES[3..] {
        assert_eq!(config_linker(&text, t), None, "{t} pinned without an NDK");
    }
    assert_eq!(
        triple_env("armv7-unknown-linux-musleabihf"),
        "ARMV7_UNKNOWN_LINUX_MUSLEABIHF"
    );
    for k in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_TARGET"] {
        assert!(strip.iter().any(|s| s == k), "{k}");
    }
}

/// Slot 409: the ELF reader handles ELF32 (armv7) and ELF64 headers.
#[test]
fn elf_reader_32_and_64() {
    // ELF32 ARM, ET_DYN, one PT_LOAD program header at 52 (no PT_INTERP).
    let mut e32 = vec![0u8; 52 + 32 + 16];
    e32[..4].copy_from_slice(b"\x7fELF");
    e32[4] = 1;
    e32[5] = 1;
    e32[16] = 3;
    e32[18] = 0x28;
    e32[28..32].copy_from_slice(&52u32.to_le_bytes());
    e32[42] = 32;
    e32[44] = 1;
    e32[52] = 1; // PT_LOAD
    assert_eq!(
        elf(&e32),
        Elf {
            machine: 0x28,
            has_interp: false,
            pie: true
        }
    );
    e32[52] = 3; // PT_INTERP
    assert!(elf(&e32).has_interp);
    // ELF64 x86_64, ET_EXEC, one PT_INTERP header at 64.
    let mut e64 = vec![0u8; 64 + 56];
    e64[..4].copy_from_slice(b"\x7fELF");
    e64[4] = 2;
    e64[5] = 1;
    e64[16] = 2;
    e64[18] = 0x3E;
    e64[32..40].copy_from_slice(&64u64.to_le_bytes());
    e64[54] = 56;
    e64[56] = 1;
    e64[64] = 3;
    assert_eq!(
        elf(&e64),
        Elf {
            machine: 0x3E,
            has_interp: true,
            pie: false
        }
    );
}
