// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Guard for `.github/workflows/miri.yml` (owner decision D-M1): a short Miri
//! run on every push and the full run on a nightly schedule, both on the same
//! dated nightly, and the short run's skip list names exactly the slow tests
//! (galion-common `nist` + `time_vs_chrono`), nothing else.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn workflow() -> String {
    std::fs::read_to_string(root().join(".github/workflows/miri.yml")).expect("miri.yml")
}

/// Value of a top-level `env:` key (`  KEY: value`).
fn env(w: &str, key: &str) -> String {
    let pre = format!("  {key}: ");
    w.lines()
        .find_map(|l| l.strip_prefix(&pre))
        .unwrap_or_else(|| panic!("env {key} missing"))
        .trim()
        .to_string()
}

/// The text of one job: from `  <name>:` up to the next line indented by
/// exactly two spaces (the next job) or the end of the file.
fn job(w: &str, name: &str) -> String {
    let head = format!("  {name}:");
    let mut out = vec![];
    let mut inside = false;
    for l in w.lines() {
        let two = l.starts_with("  ") && !l.starts_with("   ");
        if inside && two {
            break;
        }
        if l == head {
            inside = true;
        }
        if inside {
            out.push(l);
        }
    }
    assert!(!out.is_empty(), "job {name} missing");
    out.join("\n") + "\n"
}

fn skips(w: &str) -> Vec<String> {
    let s = env(w, "MIRI_SHORT_SKIP");
    let t: Vec<&str> = s.split_whitespace().collect();
    assert_eq!(t.first(), Some(&"--exact"), "{s}");
    t.windows(2)
        .filter(|p| p[0] == "--skip")
        .map(|p| p[1].to_string())
        .collect()
}

fn test_fns(path: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let mut out = vec![];
    for (i, l) in lines.iter().enumerate() {
        if l.trim() == "#[test]" {
            for n in lines.iter().skip(i + 1).take(3) {
                if let Some(r) = n.trim().strip_prefix("fn ") {
                    out.push(r.split('(').next().unwrap_or("").to_string());
                    break;
                }
            }
        }
    }
    out
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn spdx_and_triggers() {
    let w = workflow();
    assert!(w.starts_with("# SPDX-License-Identifier: MIT\n"));
    assert!(w.contains("Copyright (c) 3000 Galion / galion-studio"));
    let on = &w[w.find("\non:\n").expect("on:")..w.find("\npermissions:").expect("permissions")];
    // Every push, any branch: no branch filter under `push:`.
    assert!(on.contains("\n  push:\n  schedule:"), "{on}");
    assert!(on.contains("- cron: '17 0 * * *'"), "{on}");
    assert!(on.contains("workflow_dispatch:"));
    assert!(w.contains("permissions:\n  contents: read"));
    assert!(!w.contains("secrets."), "no secrets");
}

#[test]
fn short_on_push_full_on_schedule_same_toolchain() {
    let w = workflow();
    let tc = env(&w, "MIRI_TOOLCHAIN");
    assert!(tc.starts_with("nightly-20"), "dated nightly, got {tc}");
    let crates = env(&w, "MIRI_CRATES");
    assert_eq!(crates, "-p galion-common -p hades-gate -p openmind-schema");
    let short = job(&w, "miri-short");
    let full = job(&w, "miri-full");
    let (short, full) = (short.as_str(), full.as_str());
    assert!(short.contains("if: github.event_name == 'push'"), "{short}");
    assert!(!short.contains("'schedule'"), "{short}");
    assert!(
        full.contains("if: github.event_name == 'schedule'"),
        "{full}"
    );
    assert!(!full.contains("'push'"), "{full}");
    for (j, max) in [(short, 15), (full, 60)] {
        assert!(j.contains(&format!("toolchain: {tc}")), "{j}");
        assert!(j.contains("components: miri, rust-src"), "{j}");
        assert!(j.contains("cargo +\"$MIRI_TOOLCHAIN\" miri setup"), "{j}");
        let t: u32 = j
            .lines()
            .find_map(|l| l.trim().strip_prefix("timeout-minutes: "))
            .and_then(|v| v.parse().ok())
            .expect("timeout-minutes");
        assert!(t <= max, "{t} > {max}");
    }
    assert!(short.contains(
        "run: set -f; cargo +\"$MIRI_TOOLCHAIN\" miri test --locked $MIRI_TARGETS -- $MIRI_SHORT_SKIP"
    ));
    // The three shards together cover exactly MIRI_CRATES (galion-common split
    // into its lib and its integration tests).
    let shards: Vec<&str> = short
        .lines()
        .filter_map(|l| l.trim().strip_prefix("targets: "))
        .collect();
    assert_eq!(
        shards,
        [
            "-p galion-common --lib",
            "-p galion-common --test *",
            "-p hades-gate -p openmind-schema",
        ]
    );
    assert!(short.contains("fail-fast: false"));
    assert!(short.contains("MIRI_TARGETS: ${{ matrix.targets }}"));
    assert!(full.contains("run: cargo +\"$MIRI_TOOLCHAIN\" miri test --locked $MIRI_CRATES\n"));
    assert!(!full.contains("MIRI_SHORT_SKIP"), "full run skips nothing");
}

#[test]
fn short_skip_list_is_exactly_the_slow_targets() {
    let w = workflow();
    let mut want: Vec<String> = ["nist.rs", "time_vs_chrono.rs"]
        .iter()
        .flat_map(|f| test_fns(&root().join("crates/galion-common/tests").join(f)))
        .collect();
    want.sort();
    let mut got = skips(&w);
    got.sort();
    assert_eq!(got.len(), 6);
    assert_eq!(got, want, "skip list != tests of nist + time_vs_chrono");
    // `--exact` matches whole names: each skipped name must be unique across
    // the three crates, so the skip never hides another test.
    let mut files = vec![];
    for c in ["galion-common", "hades-gate", "openmind-schema"] {
        rs_files(&root().join("crates").join(c), &mut files);
    }
    for name in &got {
        let n: usize = files
            .iter()
            .map(|f| test_fns(f).iter().filter(|t| *t == name).count())
            .sum();
        assert_eq!(n, 1, "{name} defined {n} times");
    }
}

#[test]
fn negative_controls() {
    let w = workflow();
    // A dropped skip or an extra one is caught by the skip-list check.
    let dropped = w.replace(" --skip stamp_checks_agree", "");
    assert_eq!(skips(&dropped).len(), 5);
    // The job splitter stops at the next job.
    let short = job(&w, "miri-short");
    assert!(!short.contains("miri-full"));
    assert!(job(&w, "miri-full").contains("timeout-minutes: 60"));
    // A schedule trigger on the short job would be visible to the check.
    let bad = w.replace(
        "if: github.event_name == 'push' ||",
        "if: github.event_name == 'schedule' || github.event_name == 'push' ||",
    );
    assert!(job(&bad, "miri-short").contains("'schedule'"));
}
