// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Replay regression corpus (cortex plan T5.3). No `GALION_REAL` needed:
//! replay only, nothing on the host is read except the captures.
//!
//! - Every saved capture is listed in `tests/replay-corpus/corpus.json`
//!   (fixture dirs with a `state*.json`, and every capture dir under the
//!   live capture root when that root exists) and has an expected file.
//! - Every entry replays to its expected verdict (capture cycles, then 20
//!   quiet hold cycles): any drift in score, grade, incidents, correlated
//!   members, cortex findings, self-eval checks or deductions fails, with a
//!   path-by-path diff. A replay runs twice and must be identical.
//! - No key may flap (come back after it cleared) over the capture plus 20
//!   hold cycles.
//!
//! Env: `NERVE_CAPTURE_ROOT` (live captures, default
//! `/workspace/swarm/nerve-capture`); `REPLAY_CORPUS_REQUIRE_LIVE=1` fails
//! when a live capture is missing (the box run; CI has only the fixtures);
//! `REPLAY_CORPUS_BLESS=1` rewrites the expected files (intended changes
//! only; the diff is printed first).

use openmind_nerve::corpus::{capture_dirs, diff, load_manifest, run_entry, Entry};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn here() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn live_root() -> PathBuf {
    std::env::var("NERVE_CAPTURE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/workspace/swarm/nerve-capture"))
}

fn flag(k: &str) -> bool {
    std::env::var(k).is_ok_and(|v| v == "1")
}

fn entries() -> Vec<Entry> {
    load_manifest(
        &here().join("replay-corpus/corpus.json"),
        &here().join("fixtures"),
        &live_root(),
    )
    .unwrap()
}

fn expected_path(name: &str) -> PathBuf {
    here().join(format!("replay-corpus/expected/{name}.json"))
}

fn scratch(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("nerve-corpus-{tag}-{}", std::process::id()))
}

#[test]
fn every_saved_capture_is_in_the_corpus() {
    let es = entries();
    let listed: BTreeSet<String> = es.iter().map(|e| e.source.clone()).collect();
    let mut missing = vec![];
    for d in capture_dirs(&here().join("fixtures")) {
        if !listed.contains(&format!("fixture:{d}")) {
            missing.push(format!("fixture:{d}"));
        }
    }
    let root = live_root();
    if root.is_dir() {
        for d in capture_dirs(&root) {
            if !listed.contains(&format!("live:{d}")) {
                missing.push(format!("live:{d}"));
            }
        }
    } else {
        eprintln!(
            "live capture root {} not here: fixtures only",
            root.display()
        );
    }
    assert!(
        missing.is_empty(),
        "saved capture(s) not in tests/replay-corpus/corpus.json: {missing:?}"
    );
    let names: BTreeSet<String> = es.iter().map(|e| e.name.clone()).collect();
    let mut orphans = vec![];
    for f in std::fs::read_dir(here().join("replay-corpus/expected"))
        .unwrap()
        .flatten()
    {
        let n = f.file_name().to_string_lossy().into_owned();
        let stem = n.strip_suffix(".json").unwrap_or(&n).to_string();
        if !names.contains(&stem) {
            orphans.push(n);
        }
    }
    assert!(
        orphans.is_empty(),
        "expected file(s) without a corpus entry: {orphans:?}"
    );
    if !flag("REPLAY_CORPUS_BLESS") {
        let no_expected: Vec<&String> = names
            .iter()
            .filter(|n| !expected_path(n).is_file())
            .collect();
        assert!(
            no_expected.is_empty(),
            "corpus entries without an expected file: {no_expected:?}"
        );
    }
}

fn check(e: &Entry, bless: bool) -> Result<String, Vec<String>> {
    let got = run_entry(e, &scratch(&format!("{}-a", e.name))).map_err(|x| vec![x])?;
    let again = run_entry(e, &scratch(&format!("{}-b", e.name))).map_err(|x| vec![x])?;
    let mut errs = vec![];
    let nondet = diff(&got, &again);
    if !nondet.is_empty() {
        errs.push(format!(
            "{}: replay is not deterministic: {nondet:?}",
            e.name
        ));
    }
    let flaps = got["result"]["flaps"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if !flaps.is_empty() {
        errs.push(format!(
            "{}: verdict flaps over capture + hold cycles: {flaps:?}",
            e.name
        ));
    }
    let path = expected_path(&e.name);
    let want: Option<Value> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let d = want
        .as_ref()
        .map(|w| diff(w, &got))
        .unwrap_or_else(|| vec!["no expected file".into()]);
    let summary =
        format!(
        "{}: final {} {} held {} {} ({} capture + {} hold cycles, {} incident(s), {} finding(s))",
        e.name,
        got["result"]["final"]["score"],
        got["result"]["final"]["grade"],
        got["result"]["held"]["score"],
        got["result"]["held"]["grade"],
        got["result"]["capture_cycles"],
        got["result"]["hold_cycles"],
        got["result"]["final"]["incidents"].as_array().map_or(0, Vec::len),
        got["result"]["final"]["findings"].as_array().map_or(0, Vec::len),
    );
    if !d.is_empty() {
        let (cap, rest): (Vec<&String>, Vec<&String>) =
            d.iter().partition(|x| x.starts_with("$.capture"));
        if bless {
            eprintln!("BLESS {}: {} change(s): {d:#?}", e.name, d.len());
            let mut text = serde_json::to_string_pretty(&got).unwrap();
            text.push('\n');
            std::fs::write(&path, text).unwrap();
        } else {
            if !cap.is_empty() {
                errs.push(format!(
                    "{}: the capture itself changed (not code drift): {cap:?}",
                    e.name
                ));
            }
            if !rest.is_empty() {
                errs.push(format!(
                    "{}: verdict drift ({} change(s)): {rest:#?}",
                    e.name,
                    rest.len()
                ));
            }
        }
    }
    if errs.is_empty() {
        Ok(summary)
    } else {
        Err(errs)
    }
}

#[test]
fn corpus_replays_without_drift_or_flaps() {
    let bless = flag("REPLAY_CORPUS_BLESS");
    let require_live = flag("REPLAY_CORPUS_REQUIRE_LIVE");
    let mut errs = vec![];
    let (mut ran, mut skipped) = (0, vec![]);
    for e in entries() {
        if !Path::new(&e.dir).is_dir() {
            if e.live && !require_live {
                skipped.push(e.name.clone());
                continue;
            }
            errs.push(format!(
                "{}: capture dir {} missing",
                e.name,
                e.dir.display()
            ));
            continue;
        }
        match check(&e, bless) {
            Ok(s) => {
                ran += 1;
                eprintln!("ok {s}");
            }
            Err(mut x) => {
                ran += 1;
                errs.append(&mut x);
            }
        }
    }
    eprintln!(
        "replay corpus: {ran} replayed, {} live skipped (not on this host): {skipped:?}",
        skipped.len()
    );
    assert!(
        errs.is_empty(),
        "replay corpus failures:\n{}",
        errs.join("\n")
    );
    assert!(ran >= 2, "the committed fixtures always replay");
}

/// The drift check itself: a corpus entry whose expected file disagrees
/// fails with a readable diff (here: a doctored score and a lost finding).
#[test]
fn drift_is_detected() {
    let e = entries()
        .into_iter()
        .find(|e| e.name == "fixture-cortex-replay")
        .unwrap();
    let got = run_entry(&e, &scratch("drift")).unwrap();
    let mut doctored = got.clone();
    doctored["result"]["final"]["score"] = Value::from(99);
    doctored["result"]["final"]["findings"]
        .as_array_mut()
        .unwrap()
        .push(Value::from("bleeding:made-up warn"));
    let d = diff(&doctored, &got);
    assert!(
        d.iter()
            .any(|x| x.starts_with("$.result.final.score: 99 -> ")),
        "{d:?}"
    );
    assert!(
        d.iter()
            .any(|x| x.contains("lost \"bleeding:made-up warn\"")),
        "{d:?}"
    );
}
