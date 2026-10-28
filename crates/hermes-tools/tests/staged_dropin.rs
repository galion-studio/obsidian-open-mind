// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 355 regression: the staged coding drop-in must pass the same path
//! rules the gateway enforces at start-up. Before slot 355 the staged
//! worktree (/var/lib/hermes-rs/work/openmind) sat under the denied /var/lib,
//! so it could never start; the same mistake crash-looped live hermes-rs on
//! 2026-10-27 19:45 CET. Pure path checks, nothing is created.

use hermes_tools::config::{branch_ok, worktree_path_ok};
use std::path::{Path, PathBuf};

const DROPIN: &str = include_str!("../../../deploy/staged/hermes-rs.service.d/coding-tools.conf");
const NOLOOP: &str =
    include_str!("../../../deploy/staged/hermes-rs.service.d/40-config-no-loop.conf");
const UNIT: &str = include_str!("../../../deploy/staged/hermes-rs.service");

fn env_of(text: &str, key: &str) -> Option<String> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("Environment="))
        .filter_map(|kv| kv.split_once('='))
        .filter(|(k, _)| *k == key)
        .map(|(_, v)| v.trim().to_string())
        .next_back()
}

fn rw_paths(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("ReadWritePaths="))
        .flat_map(|v| v.split_whitespace())
        .map(|p| PathBuf::from(p.trim_start_matches(['-', '+'])))
        .collect()
}

fn writable(p: &Path, rw: &[PathBuf]) -> bool {
    rw.iter().any(|r| p.starts_with(r))
}

#[test]
fn staged_worktree_passes_the_deny_list() {
    let wt = env_of(DROPIN, "HERMES_RS_CODING_WORKTREE").expect("worktree set");
    worktree_path_ok(Path::new(&wt), &[]).unwrap_or_else(|e| panic!("staged worktree: {e}"));
    let br = env_of(DROPIN, "HERMES_RS_CODING_BRANCH").expect("branch set");
    branch_ok(&br).unwrap();
}

#[test]
fn staged_paths_are_writable_inside_the_unit() {
    let mut rw = rw_paths(UNIT);
    rw.extend(rw_paths(DROPIN));
    let wt = PathBuf::from(env_of(DROPIN, "HERMES_RS_CODING_WORKTREE").unwrap());
    let st = PathBuf::from(env_of(DROPIN, "HERMES_RS_CODING_STATE").unwrap());
    assert!(
        writable(&wt, &rw),
        "worktree {wt:?} not in ReadWritePaths {rw:?}"
    );
    assert!(
        writable(&st, &rw),
        "state {st:?} not in ReadWritePaths {rw:?}"
    );
    assert!(!st.starts_with(&wt), "state must live outside the worktree");
}

#[test]
fn old_var_lib_values_are_refused() {
    for p in [
        "/var/lib/hermes-rs/work/openmind",
        "/var/lib/hermes-rs/coding-worktree",
    ] {
        let e = worktree_path_ok(Path::new(p), &[]).unwrap_err();
        assert!(e.contains("/var/lib"), "{e}");
    }
}

#[test]
fn no_loop_dropin_stops_on_exit_2_only() {
    let v: Vec<&str> = NOLOOP
        .lines()
        .filter_map(|l| l.trim().strip_prefix("RestartPreventExitStatus="))
        .collect();
    assert_eq!(v, ["2"]);
    assert!(NOLOOP.contains("[Service]"));
}
