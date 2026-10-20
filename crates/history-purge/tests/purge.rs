// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio

//! End-to-end: build a small repo with branches, a merge, tags, quoted
//! paths and shared content, mirror-clone it, purge, and check the result.
//! Needs the `git` binary (skipped when absent).

use history_purge::repo::{self, RewriteOpts};
use history_purge::PathSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn have_git() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn tmp(name: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!(
        "history-purge-{}-{}-{}",
        name,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn g(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn write(dir: &Path, rel: &str, body: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn commit(dir: &Path, msg: &str) {
    g(dir, &["add", "-A"]);
    g(dir, &["commit", "-q", "--allow-empty", "-m", msg]);
}

fn build_source() -> PathBuf {
    let src = tmp("src");
    g(&src, &["init", "-q", "-b", "main"]);
    write(&src, "src/lib.rs", "pub fn a() {}\n");
    write(&src, "LICENSE", "MIT\n");
    commit(&src, "root");
    write(&src, "contrib/notes.md", "operator notes\n");
    write(&src, "stack/kms/sp ace é.redacted", "X=<REDACTED:X>\n");
    // same bytes as LICENSE: the blob must survive (kept path shares it)
    write(&src, "stack/kms/LICENSE-copy", "MIT\n");
    write(&src, "deep/x/.env.redacted", "Y=<REDACTED:Y>\n");
    commit(&src, "Record stack: M 100644 inline fake line in message");
    g(&src, &["tag", "-a", "v0", "-m", "tag on dirty commit"]);
    g(&src, &["checkout", "-q", "-b", "side"]);
    write(&src, "contrib/side.md", "side\n");
    write(&src, "src/side.rs", "pub fn s() {}\n");
    commit(&src, "side work");
    g(&src, &["checkout", "-q", "main"]);
    g(&src, &["rm", "-q", "-r", "contrib"]);
    write(&src, "src/main.rs", "fn main() {}\n");
    commit(&src, "remove contrib on main");
    g(
        &src,
        &["merge", "-q", "--no-ff", "-m", "merge side", "side"],
    );
    g(&src, &["tag", "light"]);
    src
}

fn patterns() -> PathSet {
    PathSet::parse("contrib/\nstack/kms/\n*.redacted\n").unwrap()
}

#[test]
fn rewrite_drops_paths_from_all_history_and_keeps_the_rest() {
    if !have_git() {
        eprintln!("git not found; skipped");
        return;
    }
    let src = build_source();
    let commits_before = g(&src, &["rev-list", "--all", "--count"]);
    let head_tree_files_before = g(&src, &["ls-tree", "-r", "--name-only", "main"]);
    let mirror = tmp("mirror").join("m.git");
    let st = Command::new("git")
        .args(["clone", "-q", "--mirror"])
        .arg(&src)
        .arg(&mirror)
        .status()
        .unwrap();
    assert!(st.success());

    let set = patterns();
    let before = repo::scan(&mirror, &set).unwrap();
    assert!(before.paths().len() >= 5, "{:?}", before.paths());

    // a remote is configured: refused without --strip-remotes
    let err = repo::rewrite(&mirror, &set, &RewriteOpts::default()).unwrap_err();
    assert!(err.contains("--strip-remotes"), "{err}");

    let opts = RewriteOpts {
        strip_remotes: true,
        skip_gc: false,
    };
    let r = repo::rewrite(&mirror, &set, &opts).unwrap();
    assert!(r.after.hits.is_empty(), "{:?}", r.after.paths());
    assert_eq!(r.remotes_removed, vec!["origin".to_string()]);
    assert!(r.oids_orphan_present.is_empty());
    assert_eq!(r.oids_shared.len(), 1, "LICENSE blob is shared");
    assert!(r.oids_gone >= 4);
    assert!(g(&mirror, &["remote"]).is_empty());

    // old -> new commit map covers every commit and points at real commits
    let map = std::fs::read_to_string(&r.commit_map).unwrap();
    let pairs: Vec<(&str, &str)> = map
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
        .collect();
    assert_eq!(pairs.len().to_string(), commits_before);
    assert_eq!(r.mapped as usize, pairs.len());
    let old_main = g(&src, &["rev-parse", "main"]);
    let new_main = g(&mirror, &["rev-parse", "main"]);
    assert!(pairs.contains(&(old_main.as_str(), new_main.as_str())));
    assert!(!mirror.join("galion-purge-export.marks").exists());

    // history shape kept: same commit count, branches, tags, messages
    assert_eq!(
        g(&mirror, &["rev-list", "--all", "--count"]),
        commits_before
    );
    let refs = g(&mirror, &["for-each-ref", "--format=%(refname)"]);
    for r in [
        "refs/heads/main",
        "refs/heads/side",
        "refs/tags/v0",
        "refs/tags/light",
    ] {
        assert!(refs.contains(r), "{r} missing in {refs}");
    }
    assert_eq!(g(&mirror, &["cat-file", "-t", "v0"]), "tag");
    let log = g(&mirror, &["log", "--all", "--format=%s"]);
    assert!(log.contains("Record stack: M 100644 inline fake line in message"));
    assert!(log.contains("merge side"));
    assert_eq!(
        g(&mirror, &["rev-list", "--merges", "--count", "main"]),
        "1"
    );

    // the kept tree is identical apart from dropped paths
    let after_files = g(&mirror, &["ls-tree", "-r", "--name-only", "main"]);
    let expect: Vec<&str> = head_tree_files_before
        .lines()
        .filter(|p| !set.matches(p.trim_matches('"').as_bytes()))
        .collect();
    assert_eq!(after_files.lines().collect::<Vec<_>>(), expect);
    assert_eq!(g(&mirror, &["show", "main:src/side.rs"]), "pub fn s() {}");

    // verify through a fresh scan, and the old dirty blobs are unreachable
    assert!(repo::scan(&mirror, &set).unwrap().hits.is_empty());
    let fsck = g(&mirror, &["fsck", "--full", "--no-dangling"]);
    assert!(fsck.is_empty(), "{fsck}");

    let _ = std::fs::remove_dir_all(src);
    let _ = std::fs::remove_dir_all(mirror.parent().unwrap());
}

#[test]
fn refuses_non_bare_and_opt_galion() {
    if !have_git() {
        return;
    }
    let src = build_source();
    let set = patterns();
    let err = repo::rewrite(&src, &set, &RewriteOpts::default()).unwrap_err();
    assert!(err.contains("not a bare repository"), "{err}");
    let err = repo::rewrite(
        Path::new("/opt/galion/openmind"),
        &set,
        &RewriteOpts::default(),
    )
    .unwrap_err();
    // either it does not exist on this machine or it is refused
    assert!(
        err.contains("/opt/galion") || err.contains("No such file"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(src);
}
