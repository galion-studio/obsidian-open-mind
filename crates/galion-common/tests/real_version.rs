// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life check (GALION_REAL=1): build every migrated binary in release
//! with a stamped commit/time, run `<bin> --version`, and require exit 0,
//! the stamped line, and a wall time under 100 ms.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

const BINS: &[&str] = &[
    "desk-hermes",
    "galion-boot",
    "galion-desk",
    "galion-one",
    "galion-origin",
    "link-watch",
    "one-talk",
    "openmind-memory",
    "room",
    "tuner",
];

#[test]
fn version_is_instant_and_stamped() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root.join("target"));
    let mut build = Command::new(env!("CARGO"));
    build
        .current_dir(&root)
        .args(["build", "--release", "-j", "2"])
        .env("GALION_BUILD_COMMIT", "realtest0")
        .env("GALION_BUILD_TIME", "2026-10-20T23:00:00+02:00");
    for b in BINS {
        build.args(["-p", b]);
    }
    assert!(build.status().expect("cargo build").success());
    for b in BINS {
        let start = Instant::now();
        let out = Command::new(target.join("release").join(b))
            .arg("--version")
            .env_clear()
            .output()
            .expect("spawn");
        let took = start.elapsed();
        let line = String::from_utf8_lossy(&out.stdout);
        eprintln!("{took:?} {}", line.trim_end());
        assert!(out.status.success(), "{b} exit {:?}", out.status);
        assert_eq!(
            line.trim_end(),
            format!(
                "{b} {} (commit realtest0, built 2026-10-20T23:00:00+02:00)",
                env!("CARGO_PKG_VERSION")
            ),
            "{b}"
        );
        assert!(took < Duration::from_millis(100), "{b} took {took:?}");
    }
}
