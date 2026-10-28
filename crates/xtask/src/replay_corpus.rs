// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//
// `cargo xtask replay-corpus`: the cortex replay regression corpus (T5.3).
// Runs the corpus unit tests (verdict, flap and diff rules) and the corpus
// test (every capture replayed against its expected verdict file, twice for
// determinism, plus the drift and flap positive controls).
//
//   --bless         rewrite the expected files (intended verdict changes only;
//                   the per-capture diff is printed; commit with the reason)
//   --require-live  fail when a live capture is missing (box run; CI has only
//                   the committed fixtures, the live captures stay on the box)
//   --release       release profile (faster replays of large captures)

use std::path::PathBuf;
use std::process::Command;

/// One cargo invocation: args and extra env.
#[derive(Debug, PartialEq)]
pub struct Step {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

pub fn plan(rest: &[String]) -> Result<Vec<Step>, String> {
    let (mut bless, mut live, mut release) = (false, false, false);
    for a in rest {
        match a.as_str() {
            "--bless" => bless = true,
            "--require-live" => live = true,
            "--release" => release = true,
            x => return Err(format!("unknown argument '{x}'")),
        }
    }
    let base = |extra: &[&str]| -> Vec<String> {
        let mut v: Vec<String> = ["test", "--locked", "-p", "openmind-nerve"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        if release {
            v.push("--release".into());
        }
        v.extend(extra.iter().map(|s| s.to_string()));
        v
    };
    let mut env = vec![];
    if bless {
        env.push(("REPLAY_CORPUS_BLESS".to_string(), "1".to_string()));
    }
    if live {
        env.push(("REPLAY_CORPUS_REQUIRE_LIVE".to_string(), "1".to_string()));
    }
    Ok(vec![
        Step {
            args: base(&["--lib", "corpus::"]),
            env: vec![],
        },
        Step {
            args: base(&["--test", "replay_corpus", "--", "--nocapture"]),
            env,
        },
    ])
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("."))
}

pub fn run(rest: &[String]) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    for s in plan(rest)? {
        eprintln!("xtask replay-corpus: cargo {}", s.args.join(" "));
        let st = Command::new(&cargo)
            .args(&s.args)
            .envs(s.env.iter().map(|(k, v)| (k, v)))
            .current_dir(repo_root())
            .status()
            .map_err(|e| format!("{cargo}: {e}"))?;
        if !st.success() {
            return Err(format!("cargo {} failed ({st})", s.args.join(" ")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn default_plan_runs_unit_then_corpus_tests() {
        let p = plan(&[]).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(
            p[0].args,
            a(&[
                "test",
                "--locked",
                "-p",
                "openmind-nerve",
                "--lib",
                "corpus::"
            ])
        );
        assert_eq!(
            p[1].args,
            a(&[
                "test",
                "--locked",
                "-p",
                "openmind-nerve",
                "--test",
                "replay_corpus",
                "--",
                "--nocapture"
            ])
        );
        assert!(p[1].env.is_empty());
    }

    #[test]
    fn flags_map_to_env_and_profile() {
        let p = plan(&a(&["--bless", "--require-live", "--release"])).unwrap();
        assert!(p[1].args.contains(&"--release".to_string()));
        assert_eq!(
            p[1].env,
            vec![
                ("REPLAY_CORPUS_BLESS".to_string(), "1".to_string()),
                ("REPLAY_CORPUS_REQUIRE_LIVE".to_string(), "1".to_string())
            ]
        );
        assert!(plan(&a(&["--frobnicate"])).is_err());
    }
}
