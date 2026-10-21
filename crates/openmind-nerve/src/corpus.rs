// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Replay regression corpus (cortex plan T5.3).
//!
//! Every saved capture (the synthetic fixtures in `tests/fixtures/` and the
//! read-only live VPS captures under `/workspace/swarm/nerve-capture/`) is
//! listed in `tests/replay-corpus/corpus.json` with an expected verdict file
//! in `tests/replay-corpus/expected/<name>.json`. A run replays the capture
//! through the live pipeline ([`crate::replay::replay_dir_with`]), then
//! holds the last snapshot for [`HOLD_CYCLES`] more cycles, and records:
//!
//! - `final`: the verdict after the last captured snapshot (score, grade,
//!   pass, system, incidents with their correlated members, cortex findings,
//!   self-eval checks, deductions);
//! - `held`: the verdict after the hold cycles;
//! - `timeline`: `score grade` per cycle;
//! - `flaps`: every key (incident + severity, finding + severity, check,
//!   grade) that comes back after it cleared (more than one episode) over
//!   all cycles. A quiet host must not flap; a flap fails the corpus test.
//!
//! Text is normalised before it is stored: every run of digits becomes `#`,
//! so expected files carry no addresses, counts or ages, only structure.
//! The capture itself is fingerprinted (file name, size, FNV-1a 64) so a
//! changed capture is reported as such, not as code drift.

use crate::config::Config;
use crate::replay::replay_dir_with;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Hold cycles after the last captured snapshot (the no-flap window).
pub const HOLD_CYCLES: usize = 20;

/// Every run of ASCII digits replaced by one `#`, after [`canon_text`].
pub fn norm(s: &str) -> String {
    let s = canon_text(s);
    let mut out = String::with_capacity(s.len());
    let mut in_digits = false;
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            if !in_digits {
                out.push('#');
            }
            in_digits = true;
        } else {
            out.push(ch);
            in_digits = false;
        }
    }
    out
}

fn sorted(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut kv: Vec<(String, Value)> = m.into_iter().collect();
            kv.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(kv.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(sorted).collect()),
        x => x,
    }
}

/// `s` with every embedded JSON object (health bodies) re-serialised with
/// sorted keys: serde_json's `preserve_order` (on in workspace-wide builds
/// through feature unification, off in `-p openmind-nerve` builds) must not
/// look like drift. Same rule as tests/support/mod.rs.
pub fn canon_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let mut it = serde_json::Deserializer::from_str(tail).into_iter::<Value>();
        match it.next() {
            Some(Ok(v @ Value::Object(_))) => {
                let n = it.byte_offset();
                out.push_str(&sorted(v).to_string());
                rest = &tail[n..];
            }
            _ => {
                out.push('{');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.clone(),
        Value::Null => "-".into(),
        x => x.to_string(),
    }
}

/// Stable, text-normalised summary of a self-eval report.
pub fn verdict(r: &Value) -> Value {
    let mut incidents: Vec<Value> = r["incidents"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|i| {
            let mut members: Vec<String> = i["correlation"]["findings"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|f| format!("{} {}", s(&f["finding"]), s(&f["severity"])))
                .collect();
            members.sort();
            json!({"cause": norm(&s(&i["cause"])), "severity": s(&i["severity"]), "members": members})
        })
        .collect();
    incidents.sort_by_key(|i| (s(&i["cause"]), s(&i["severity"])));
    let mut findings: Vec<String> = vec![];
    if let Some(m) = r["cortex"].as_object() {
        for (cat, list) in m {
            for f in list.as_array().into_iter().flatten() {
                findings.push(format!(
                    "{cat}:{} {}",
                    norm(&s(&f["id"])),
                    s(&f["severity"])
                ));
            }
        }
    }
    findings.sort();
    let checks: BTreeSet<String> = r["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| {
            format!(
                "{} {} {}",
                s(&f["level"]),
                s(&f["check"]),
                norm(&s(&f["component"]))
            )
        })
        .collect();
    let mut deductions: Vec<String> = r["deductions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| format!("{} {}", s(&d["points"]), norm(&s(&d["what"]))))
        .collect();
    deductions.sort();
    json!({
        "score": r["score"],
        "grade": r["grade"],
        "pass": r["pass"],
        "system": r["system"],
        "incidents": incidents,
        "findings": findings,
        "checks": checks,
        "deductions": deductions,
    })
}

/// Flap keys of one verdict: what must not come and go repeatedly.
pub fn keys(v: &Value) -> BTreeSet<String> {
    let mut k = BTreeSet::new();
    k.insert(format!("grade:{}", s(&v["grade"])));
    for i in v["incidents"].as_array().into_iter().flatten() {
        k.insert(format!("incident:{}={}", s(&i["cause"]), s(&i["severity"])));
    }
    for f in v["findings"].as_array().into_iter().flatten() {
        k.insert(format!("finding:{}", s(f)));
    }
    for c in v["checks"].as_array().into_iter().flatten() {
        k.insert(format!("check:{}", s(c)));
    }
    k
}

/// Keys present in more than one separate episode over `seq` (one verdict
/// per cycle: present, gone, back again), each with its presence pattern
/// (`1` present, `0` absent).
pub fn flaps(seq: &[Value]) -> Vec<String> {
    let sets: Vec<BTreeSet<String>> = seq.iter().map(keys).collect();
    let all: BTreeSet<&String> = sets.iter().flatten().collect();
    let mut out = vec![];
    for key in all {
        let pat: String = sets
            .iter()
            .map(|x| if x.contains(key) { '1' } else { '0' })
            .collect();
        // Episodes: runs of presence. One onset and one clear is fine (a
        // flood that stops); coming back after clearing is a flap.
        let episodes = pat.split('0').filter(|r| !r.is_empty()).count();
        if episodes > 1 {
            out.push(format!("{key} {pat}"));
        }
    }
    out
}

fn fnv64(b: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for x in b {
        h ^= u64::from(*x);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// File name, size and FNV-1a 64 of every regular file in `dir`, sorted.
pub fn fingerprint(dir: &Path) -> Result<Value, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut files: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    let mut out = vec![];
    for p in files {
        let b = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let name = p
            .file_name()
            .map(|x| x.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.push(json!({"file": name, "bytes": b.len(), "fnv64": format!("{:016x}", fnv64(&b))}));
    }
    Ok(Value::Array(out))
}

/// Replay `dir` with `cfg`, hold [`HOLD_CYCLES`] (or `hold`) cycles and
/// return `{capture_cycles, hold_cycles, final, held, timeline, flaps}`.
pub fn run(
    dir: &Path,
    cfg: Config,
    env: crate::sensors::Env,
    hold: usize,
) -> Result<Value, String> {
    let mut seq: Vec<Value> = vec![];
    let mut timeline = vec![];
    let out = replay_dir_with(dir, cfg, env, hold, &mut |_, _, e| {
        let sh = e.shared.lock().unwrap_or_else(|x| x.into_inner());
        let v = verdict(&sh.report);
        timeline.push(format!("{} {}", s(&v["score"]), s(&v["grade"])));
        seq.push(v);
    })?;
    let n = out.cycles - hold;
    Ok(json!({
        "capture_cycles": n,
        "hold_cycles": hold,
        "final": seq[n - 1],
        "held": seq[seq.len() - 1],
        "timeline": timeline,
        "flaps": flaps(&seq),
    }))
}

/// One corpus entry.
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    /// `fixture:<dir>` (under tests/fixtures) or `live:<dir>` (under the
    /// live capture root).
    pub source: String,
    pub dir: PathBuf,
    pub live: bool,
    pub env: BTreeMap<String, String>,
    pub note: String,
}

/// Read `corpus.json`. `fixtures` is `tests/fixtures`, `live_root` the live
/// capture root (`NERVE_CAPTURE_ROOT`, default `/workspace/swarm/nerve-capture`).
pub fn load_manifest(path: &Path, fixtures: &Path, live_root: &Path) -> Result<Vec<Entry>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = vec![];
    let mut seen = BTreeSet::new();
    for c in v["captures"]
        .as_array()
        .ok_or("corpus.json: no captures array")?
    {
        let name = c["name"]
            .as_str()
            .ok_or("capture without name")?
            .to_string();
        if !seen.insert(name.clone()) {
            return Err(format!("duplicate corpus entry {name}"));
        }
        let source = c["source"]
            .as_str()
            .ok_or(format!("{name}: no source"))?
            .to_string();
        let (live, dir) = if let Some(d) = source.strip_prefix("fixture:") {
            (false, fixtures.join(d))
        } else if let Some(d) = source.strip_prefix("live:") {
            (true, live_root.join(d))
        } else {
            return Err(format!(
                "{name}: source must be fixture:<dir> or live:<dir>"
            ));
        };
        let env = c["env"]
            .as_object()
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), s(v)))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        out.push(Entry {
            name,
            source,
            dir,
            live,
            env,
            note: c["note"].as_str().unwrap_or("").to_string(),
        });
    }
    Ok(out)
}

/// Directories directly under `root` that hold a capture (a `state*.json`).
pub fn capture_dirs(root: &Path) -> Vec<String> {
    let mut out = vec![];
    for e in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let has_state = std::fs::read_dir(&p)
            .into_iter()
            .flatten()
            .flatten()
            .any(|f| {
                let n = f.file_name().to_string_lossy().into_owned();
                n.starts_with("state") && n.ends_with(".json")
            });
        if has_state {
            out.push(e.file_name().to_string_lossy().into_owned());
        }
    }
    out.sort();
    out
}

/// The config a corpus entry replays with: defaults plus the entry's own
/// env, never the process environment (so a run is the same everywhere).
pub fn entry_config(e: &Entry, data_dir: &Path) -> Result<Config, String> {
    let dd = data_dir.display().to_string();
    Config::from_env(&|k| {
        if k == "NERVE_DATA_DIR" {
            return Some(dd.clone());
        }
        e.env.get(k).cloned()
    })
}

/// Replay one entry into the expected-file shape.
pub fn run_entry(e: &Entry, data_dir: &Path) -> Result<Value, String> {
    let _ = std::fs::remove_dir_all(data_dir);
    std::fs::create_dir_all(data_dir).map_err(|x| format!("{}: {x}", data_dir.display()))?;
    let cfg = entry_config(e, data_dir)?;
    let env = |k: &str| e.env.get(k).cloned();
    let result = run(&e.dir, cfg, &env, HOLD_CYCLES)?;
    let _ = std::fs::remove_dir_all(data_dir);
    let mut m = Map::new();
    m.insert("name".into(), json!(e.name));
    m.insert("source".into(), json!(e.source));
    m.insert("env".into(), json!(e.env));
    m.insert("capture".into(), fingerprint(&e.dir)?);
    m.insert("result".into(), result);
    Ok(Value::Object(m))
}

/// Paths where `want` and `got` differ (`$.result.final.score: 56 -> 61`).
pub fn diff(want: &Value, got: &Value) -> Vec<String> {
    let mut out = vec![];
    walk("$", want, got, &mut out);
    out
}

fn walk(path: &str, a: &Value, b: &Value, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let ks: BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            for k in ks {
                walk(
                    &format!("{path}.{k}"),
                    x.get(k).unwrap_or(&Value::Null),
                    y.get(k).unwrap_or(&Value::Null),
                    out,
                );
            }
        }
        (Value::Array(x), Value::Array(y))
            if x.iter().all(Value::is_string) && y.iter().all(Value::is_string) =>
        {
            let xs: BTreeSet<&str> = x.iter().filter_map(Value::as_str).collect();
            let ys: BTreeSet<&str> = y.iter().filter_map(Value::as_str).collect();
            for gone in xs.difference(&ys) {
                out.push(format!("{path}: lost {gone:?}"));
            }
            for new in ys.difference(&xs) {
                out.push(format!("{path}: new {new:?}"));
            }
            if xs == ys && x != y {
                out.push(format!("{path}: order changed"));
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                out.push(format!("{path}: {} item(s) -> {}", x.len(), y.len()));
            }
            for (i, (p, q)) in x.iter().zip(y.iter()).enumerate() {
                walk(&format!("{path}[{i}]"), p, q, out);
            }
        }
        _ if a != b => out.push(format!("{path}: {a} -> {b}")),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_hides_numbers_and_addresses() {
        assert_eq!(
            norm(r#"HTTP 503 [health body {"ok":false,"loaded":false}]"#),
            r#"HTTP # [health body {"loaded":false,"ok":false}]"#
        );
        assert_eq!(
            norm("4.0 failed/min over 25 min"),
            "#.# failed/min over # min"
        );
        assert_eq!(norm("from 198.51.100.7:22"), "from #.#.#.#:#");
        assert_eq!(norm("no digits"), "no digits");
    }

    #[test]
    fn flaps_counts_episodes() {
        let v = |g: &str, inc: &[&str]| json!({"grade": g, "incidents": inc.iter().map(|c| json!({"cause": c, "severity": "warn"})).collect::<Vec<_>>()});
        // edge resolves once: fine. scan comes, goes, comes: flap.
        let seq = vec![
            v("D", &["edge"]),
            v("D", &["edge", "scan"]),
            v("C", &["scan"]),
            v("C", &[]),
            v("C", &["scan"]),
        ];
        let f = flaps(&seq);
        assert_eq!(f, vec!["incident:scan=warn 01101".to_string()], "{f:?}");
        assert!(flaps(&vec![v("D", &["edge"]); 21]).is_empty());
        // Severity flap: warn -> error -> warn.
        let sv =
            |sev: &str| json!({"grade": "D", "incidents": [{"cause": "edge", "severity": sev}]});
        assert_eq!(
            flaps(&[sv("warn"), sv("error"), sv("warn")]),
            vec!["incident:edge=warn 101".to_string()]
        );
        // Onset then clear (a flood that stops) is one episode, not a flap.
        assert!(flaps(&[v("D", &[]), v("D", &["edge"]), v("D", &[])]).is_empty());
    }

    #[test]
    fn diff_names_paths() {
        let a = json!({"result": {"final": {"score": 56, "findings": ["a x", "b y"]}}});
        let b = json!({"result": {"final": {"score": 61, "findings": ["a x", "c z"]}}});
        assert_eq!(
            diff(&a, &b),
            vec![
                "$.result.final.findings: lost \"b y\"",
                "$.result.final.findings: new \"c z\"",
                "$.result.final.score: 56 -> 61",
            ]
        );
        assert!(diff(&a, &a).is_empty());
    }

    #[test]
    fn fingerprint_is_stable() {
        let d = std::env::temp_dir().join(format!("nerve-corpus-fp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("b.txt"), "hello").unwrap();
        std::fs::write(d.join("a.txt"), "").unwrap();
        let f = fingerprint(&d).unwrap();
        assert_eq!(f[0]["file"], "a.txt");
        assert_eq!(f[0]["fnv64"], "cbf29ce484222325");
        assert_eq!(f[1]["fnv64"], "a430d84680aabd0b");
        let _ = std::fs::remove_dir_all(&d);
    }
}
