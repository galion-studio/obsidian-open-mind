// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio

//! `galion-purge`: CLI for the SEC-1 history purge. See `--help`.

use history_purge::repo::{self, RewriteOpts};
use history_purge::PathSet;
use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "galion-purge: drop listed paths from all git history (scratch mirror clones only)

USAGE
  galion-purge scan    --repo DIR (--paths FILE | --path PAT)...
  galion-purge rewrite --repo SCRATCH.git (--paths FILE | --path PAT)... --confirm-scratch-copy [--strip-remotes]
  galion-purge verify  --repo DIR (--paths FILE | --path PAT)...

  scan     read-only: list every listed path that exists in any commit of any ref
  rewrite  git fast-export --all --no-data | filter | git fast-import --force, then
           reflog expire + gc --prune=now; then re-scans and checks the old blob ids
           are gone. Bare repos only (git clone --mirror). Never pushes.
  verify   read-only: exit 1 if any listed path is in any commit of any ref

PATTERNS (one per line in FILE, # comments)
  dir/        directory at the root and everything under it
  a/b*.md     anchored glob (also matches when a parent directory matches)
  *.redacted  no slash: glob on the file name at any depth
  * and ? stop at /, ** crosses it

Output lists paths, counts and object ids only; file contents are never read or printed.
Exit codes: 0 ok, 1 listed paths found (verify) or failure, 2 usage error.
";

struct Args {
    cmd: String,
    repo: Option<PathBuf>,
    set: PathSet,
    confirm: bool,
    strip_remotes: bool,
    quiet: bool,
}

fn parse(argv: &[String]) -> Result<Args, String> {
    let mut it = argv.iter();
    let cmd = it.next().cloned().ok_or("missing command")?;
    let mut a = Args {
        cmd,
        repo: None,
        set: PathSet::default(),
        confirm: false,
        strip_remotes: false,
        quiet: false,
    };
    while let Some(x) = it.next() {
        match x.as_str() {
            "--repo" => a.repo = Some(PathBuf::from(it.next().ok_or("--repo needs a value")?)),
            "--paths" => {
                let f = it.next().ok_or("--paths needs a value")?;
                let text = std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
                let s = PathSet::parse(&text).map_err(|e| format!("{f}: {e}"))?;
                for p in s.patterns() {
                    a.set.add(p)?;
                }
            }
            "--path" => a.set.add(it.next().ok_or("--path needs a value")?)?,
            "--confirm-scratch-copy" => a.confirm = true,
            "--strip-remotes" => a.strip_remotes = true,
            "--quiet" => a.quiet = true,
            o => return Err(format!("unknown argument {o}")),
        }
    }
    Ok(a)
}

fn show_paths(paths: &std::collections::BTreeSet<Vec<u8>>, quiet: bool) {
    for (top, n) in repo::by_top_dir(paths) {
        println!("  {n:>5}  {top}");
    }
    if !quiet {
        for p in paths {
            println!("    {}", String::from_utf8_lossy(p));
        }
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.is_empty() || argv.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return if argv.is_empty() {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        };
    }
    let a = match parse(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("galion-purge: {e}\n(see --help)");
            return ExitCode::from(2);
        }
    };
    let Some(repo_dir) = a.repo.clone() else {
        eprintln!("galion-purge: --repo is required");
        return ExitCode::from(2);
    };
    if a.set.is_empty() {
        eprintln!("galion-purge: no patterns (use --paths FILE or --path PAT)");
        return ExitCode::from(2);
    }
    match a.cmd.as_str() {
        "scan" | "verify" => match repo::scan(&repo_dir, &a.set) {
            Ok(s) => {
                let paths = s.paths();
                println!(
                    "{}: {} commits, {} distinct trees, {} patterns",
                    a.cmd,
                    s.commits,
                    s.trees,
                    a.set.len()
                );
                println!(
                    "listed paths present in history: {} ({} blob ids)",
                    paths.len(),
                    s.hits
                        .iter()
                        .map(|h| &h.oid)
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                );
                show_paths(&paths, a.quiet);
                if a.cmd == "verify" && !paths.is_empty() {
                    println!("VERIFY FAIL");
                    return ExitCode::from(1);
                }
                if a.cmd == "verify" {
                    println!("VERIFY OK: no listed path in any commit of any ref");
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("galion-purge: {e}");
                ExitCode::from(1)
            }
        },
        "rewrite" => {
            if !a.confirm {
                eprintln!(
                    "galion-purge: rewrite needs --confirm-scratch-copy (it rewrites every ref of --repo)"
                );
                return ExitCode::from(2);
            }
            let opts = RewriteOpts {
                strip_remotes: a.strip_remotes,
                skip_gc: false,
            };
            match repo::rewrite(&repo_dir, &a.set, &opts) {
                Ok(r) => {
                    if !r.remotes_removed.is_empty() {
                        println!("remote config removed: {:?}", r.remotes_removed);
                    }
                    println!(
                        "before: {} commits, {} listed paths",
                        r.before.commits,
                        r.before.paths().len()
                    );
                    show_paths(&r.before.paths(), a.quiet);
                    println!(
                        "filter: {} commits, dropped {} M + {} D lines, kept {} changes, {} commits left empty (kept)",
                        r.stats.commits,
                        r.stats.dropped_modify,
                        r.stats.dropped_delete,
                        r.stats.kept_changes,
                        r.stats.emptied_commits
                    );
                    println!(
                        "after: {} commits, {} listed paths",
                        r.after.commits,
                        r.after.paths().len()
                    );
                    println!(
                        "old blob ids of dropped paths: {} gone, {} still present only because a kept path has identical content, {} orphaned-but-present",
                        r.oids_gone,
                        r.oids_shared.len(),
                        r.oids_orphan_present.len()
                    );
                    for o in &r.oids_shared {
                        println!("    shared: {o}");
                    }
                    if !r.after.hits.is_empty() || !r.oids_orphan_present.is_empty() {
                        println!("REWRITE FAIL");
                        return ExitCode::from(1);
                    }
                    println!(
                        "commit map ({} commits, old<TAB>new): {}",
                        r.mapped,
                        r.commit_map.display()
                    );
                    println!("REWRITE OK (nothing pushed; the copy has no remote)");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("galion-purge: {e}");
                    ExitCode::from(1)
                }
            }
        }
        o => {
            eprintln!("galion-purge: unknown command {o} (see --help)");
            ExitCode::from(2)
        }
    }
}
