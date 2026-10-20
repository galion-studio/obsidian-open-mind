// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio

//! Running git: scan, rewrite and verify.

use crate::pattern::PathSet;
use crate::stream::{self, Stats};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Error type: a message for the operator.
pub type Res<T> = Result<T, String>;

fn git(repo: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C").arg(repo);
    // never let a pager, prompt or user config hook interfere
    c.env("GIT_PAGER", "cat")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C");
    c
}

/// Run git and return stdout bytes; error with stderr on failure.
pub fn git_out(repo: &Path, args: &[&str]) -> Res<Vec<u8>> {
    let o = git(repo)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !o.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    Ok(o.stdout)
}

fn git_str(repo: &Path, args: &[&str]) -> Res<String> {
    Ok(String::from_utf8_lossy(&git_out(repo, args)?)
        .trim()
        .to_string())
}

/// One path found in history.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Hit {
    /// Repo-relative path (raw bytes, lossy-printed).
    pub path: Vec<u8>,
    /// Blob (or gitlink) object id stored at that path in some tree.
    pub oid: String,
}

/// Result of walking every tree reachable from every ref.
#[derive(Debug, Default, Clone)]
pub struct Scan {
    /// Commits reachable from all refs.
    pub commits: u64,
    /// Distinct root trees walked.
    pub trees: u64,
    /// Matching (path, oid) pairs.
    pub hits: BTreeSet<Hit>,
    /// Object ids referenced from paths that do NOT match.
    pub kept_oids: BTreeSet<String>,
}

impl Scan {
    /// Distinct matching paths.
    pub fn paths(&self) -> BTreeSet<Vec<u8>> {
        self.hits.iter().map(|h| h.path.clone()).collect()
    }
}

/// Walk every distinct root tree of every commit reachable from any ref
/// (`--all`, which includes tags, notes and stash) and collect paths that
/// match `set`. Reads tree listings only, never blob contents.
pub fn scan(repo: &Path, set: &PathSet) -> Res<Scan> {
    let mut s = Scan::default();
    let log = git_out(repo, &["log", "--all", "--format=%T"])?;
    let mut trees = BTreeSet::new();
    for t in log.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
        s.commits += 1;
        trees.insert(String::from_utf8_lossy(t).to_string());
    }
    // annotated tags that point straight at a tree or blob
    let tags = git_out(
        repo,
        &[
            "for-each-ref",
            "--format=%(*objecttype) %(*objectname) %(objecttype) %(objectname)",
            "refs/tags",
        ],
    )?;
    for l in String::from_utf8_lossy(&tags).lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() == 4 && f[0] == "tree" {
            trees.insert(f[1].to_string());
        }
        if f.len() == 2 && f[0] == "tree" {
            trees.insert(f[1].to_string());
        }
    }
    s.trees = trees.len() as u64;
    for t in &trees {
        let out = git_out(repo, &["ls-tree", "-r", "-z", "--full-tree", t])?;
        for rec in out.split(|&b| b == 0).filter(|r| !r.is_empty()) {
            // "<mode> SP <type> SP <oid> TAB <path>"
            let tab = match rec.iter().position(|&b| b == b'\t') {
                Some(i) => i,
                None => continue,
            };
            let meta = String::from_utf8_lossy(&rec[..tab]);
            let oid = meta.split(' ').nth(2).unwrap_or("").to_string();
            let path = rec[tab + 1..].to_vec();
            if set.matches(&path) {
                s.hits.insert(Hit { path, oid });
            } else {
                s.kept_oids.insert(oid);
            }
        }
    }
    Ok(s)
}

/// Options for [`rewrite`].
#[derive(Debug, Clone, Default)]
pub struct RewriteOpts {
    /// Delete `remote.*` config sections before rewriting.
    pub strip_remotes: bool,
    /// Skip `reflog expire` + `gc --prune=now` (tests only).
    pub skip_gc: bool,
}

/// Outcome of [`rewrite`].
#[derive(Debug, Clone)]
pub struct Report {
    /// Scan before.
    pub before: Scan,
    /// Filter stats.
    pub stats: Stats,
    /// Scan after (must have no hits).
    pub after: Scan,
    /// Old blob ids of dropped paths that are gone from the object store.
    pub oids_gone: u64,
    /// Old blob ids still present because a kept path has identical content.
    pub oids_shared: Vec<String>,
    /// Old blob ids still present and NOT referenced by a kept path (bad).
    pub oids_orphan_present: Vec<String>,
    /// Remotes removed.
    pub remotes_removed: Vec<String>,
    /// old-sha TAB new-sha for every rewritten commit, written into the repo dir.
    pub commit_map: PathBuf,
    /// Lines in the commit map.
    pub mapped: u64,
}

/// Checks before touching anything. Returns the canonical repo path.
pub fn preflight(repo: &Path, opts: &RewriteOpts) -> Res<(PathBuf, Vec<String>)> {
    let canon = repo
        .canonicalize()
        .map_err(|e| format!("{}: {e}", repo.display()))?;
    if canon.starts_with("/opt/galion") {
        return Err("refusing to run under /opt/galion (live VPS trees)".into());
    }
    if git_str(&canon, &["rev-parse", "--is-bare-repository"])? != "true" {
        return Err(
            "not a bare repository: make a scratch copy with `git clone --mirror SRC scratch.git`"
                .into(),
        );
    }
    if git_str(&canon, &["rev-parse", "--is-shallow-repository"])? == "true" {
        return Err("shallow clone: history is incomplete, clone without --depth".into());
    }
    let remotes: Vec<String> = git_str(&canon, &["remote"])?
        .lines()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .collect();
    if !remotes.is_empty() && !opts.strip_remotes {
        return Err(format!(
            "repository has remote(s) {remotes:?}; pass --strip-remotes so the rewritten copy cannot be pushed by accident"
        ));
    }
    Ok((canon, remotes))
}

/// Rewrite all refs of a bare scratch repository, dropping `set`.
pub fn rewrite(repo: &Path, set: &PathSet, opts: &RewriteOpts) -> Res<Report> {
    if set.is_empty() {
        return Err("empty purge list".into());
    }
    let (repo, remotes) = preflight(repo, opts)?;
    for r in &remotes {
        // config only: `git remote remove` would also delete refs that a
        // mirror refspec (+refs/*:refs/*) maps, i.e. every branch
        git_out(
            &repo,
            &["config", "--remove-section", &format!("remote.{r}")],
        )?;
    }
    let before = scan(&repo, set)?;
    let exp_marks = repo.join("galion-purge-export.marks");
    let imp_marks = repo.join("galion-purge-import.marks");

    let mut exp = git(&repo)
        .args([
            "fast-export",
            "--all",
            "--no-data",
            "--signed-tags=strip",
            "--tag-of-filtered-object=rewrite",
            "--reencode=no",
            "--fake-missing-tagger",
            "--use-done-feature",
        ])
        .arg(format!("--export-marks={}", exp_marks.display()))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("spawn fast-export: {e}"))?;
    let mut imp = git(&repo)
        .args(["fast-import", "--force", "--quiet"])
        .arg(format!("--export-marks={}", imp_marks.display()))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("spawn fast-import: {e}"))?;
    let exp_out = exp.stdout.take().ok_or("no fast-export stdout")?;
    let imp_in = imp.stdin.take().ok_or("no fast-import stdin")?;
    let mut w = BufWriter::new(imp_in);
    let fres: io::Result<Stats> = stream::filter(BufReader::new(exp_out), &mut w, set);
    let flush = w.flush();
    drop(w);
    let eo = exp
        .wait_with_output()
        .map_err(|e| format!("fast-export: {e}"))?;
    let io = imp
        .wait_with_output()
        .map_err(|e| format!("fast-import: {e}"))?;
    let stats = fres.map_err(|e| format!("filter: {e}"))?;
    flush.map_err(|e| format!("write to fast-import: {e}"))?;
    if !eo.status.success() {
        return Err(format!(
            "fast-export failed ({}), see stderr above",
            eo.status
        ));
    }
    if !io.status.success() {
        return Err(format!(
            "fast-import failed ({}), see stderr above",
            io.status
        ));
    }

    let commit_map = repo.join("galion-purge-commit-map.tsv");
    let mapped = write_commit_map(&exp_marks, &imp_marks, &commit_map)?;
    let _ = std::fs::remove_file(&exp_marks);
    let _ = std::fs::remove_file(&imp_marks);

    if !opts.skip_gc {
        git_out(&repo, &["reflog", "expire", "--expire=now", "--all"])?;
        git_out(
            &repo,
            &[
                "-c",
                "gc.reflogExpireUnreachable=now",
                "gc",
                "--prune=now",
                "--quiet",
            ],
        )?;
    }
    let after = scan(&repo, set)?;

    // Are the old blobs of dropped paths gone from the object store?
    let old: BTreeSet<String> = before.hits.iter().map(|h| h.oid.clone()).collect();
    let present = present_objects(&repo, &old)?;
    let mut oids_gone = 0;
    let mut oids_shared = Vec::new();
    let mut oids_orphan_present = Vec::new();
    for o in &old {
        if !present.contains(o) {
            oids_gone += 1;
        } else if after.kept_oids.contains(o) {
            oids_shared.push(o.clone());
        } else if !opts.skip_gc {
            oids_orphan_present.push(o.clone());
        }
    }
    Ok(Report {
        before,
        stats,
        after,
        oids_gone,
        oids_shared,
        oids_orphan_present,
        remotes_removed: remotes,
        commit_map,
        mapped,
    })
}

/// Which of `oids` exist in the object store (`cat-file --batch-check`).
pub fn present_objects(repo: &Path, oids: &BTreeSet<String>) -> Res<BTreeSet<String>> {
    let mut c = git(repo)
        .args(["cat-file", "--batch-check=%(objectname) %(objecttype)"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn cat-file: {e}"))?;
    let mut input = Vec::new();
    for o in oids {
        input.extend_from_slice(o.as_bytes());
        input.push(b'\n');
    }
    let mut stdin = c.stdin.take().ok_or("no stdin")?;
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let out = c.wait_with_output().map_err(|e| format!("cat-file: {e}"))?;
    writer
        .join()
        .map_err(|_| "cat-file writer panicked".to_string())?
        .map_err(|e| format!("cat-file stdin: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.ends_with(" missing"))
        .filter_map(|l| l.split(' ').next().map(str::to_string))
        .collect())
}

/// Count matching paths per pattern-free top-level directory, for reports.
pub fn by_top_dir(paths: &BTreeSet<Vec<u8>>) -> BTreeMap<String, u64> {
    let mut m = BTreeMap::new();
    for p in paths {
        let s = String::from_utf8_lossy(p);
        let top = match s.find('/') {
            Some(i) => format!("{}/", &s[..i]),
            None => s.to_string(),
        };
        *m.entry(top).or_default() += 1;
    }
    m
}

fn read_marks(p: &Path) -> Res<BTreeMap<String, String>> {
    let t = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(t.lines()
        .filter_map(|l| {
            let (m, o) = l.split_once(' ')?;
            Some((m.to_string(), o.trim().to_string()))
        })
        .collect())
}

/// Join fast-export marks (mark -> old id) with fast-import marks
/// (mark -> new id) into `old<TAB>new` lines. Returns the line count.
pub fn write_commit_map(exp: &Path, imp: &Path, out: &Path) -> Res<u64> {
    let old = read_marks(exp)?;
    let new = read_marks(imp)?;
    let mut text = String::from("# galion-purge commit map: old-id<TAB>new-id\n");
    let mut n = 0;
    let mut pairs: Vec<(&String, &String)> = old
        .iter()
        .filter_map(|(m, o)| new.get(m).map(|nw| (o, nw)))
        .collect();
    pairs.sort();
    for (o, nw) in pairs {
        text.push_str(o);
        text.push('\t');
        text.push_str(nw);
        text.push('\n');
        n += 1;
    }
    std::fs::write(out, text).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(n)
}
