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
  galion-purge sec3-perms [--apply] [--fix-world-writable] [--rollback FILE]
                          [--hermes-home DIR] [--state-dir DIR] [--no-chown]

  scan     read-only: list every listed path that exists in any commit of any ref
  rewrite  git fast-export --all --no-data | filter | git fast-import --force, then
           reflog expire + gc --prune=now; then re-scans and checks the old blob ids
           are gone. Bare repos only (git clone --mirror). Never pushes.
  verify   read-only: exit 1 if any listed path is in any commit of any ref
  sec3-perms  SEC-3 Hermes home permissions (owner runs it as root on the VPS).
           Dry run by default: prints current -> target, changes nothing.
           --apply writes a rollback file (modes, numeric uid:gid) to the state dir
           first, then sets config.yaml 600, SOUL.md 644, vault/ 700,
           vault/.vault.lock 600, USER.md 600, all root:root.
           --fix-world-writable also clears o+w on the other world-writable entries.
           --rollback FILE restores exactly what FILE recorded.
           Never reads contents, never follows symlinks, stays on one filesystem.
           Env defaults: HERMES_HOME (/opt/galion/hermes), SEC3_STATE_DIR
           (/root/sec3-perms), SEC3_NO_CHOWN=1 (= --no-chown, fixture tests).

PATTERNS (one per line in FILE, # comments)
  dir/        directory at the root and everything under it
  a/b*.md     anchored glob (also matches when a parent directory matches)
  *.redacted  no slash: glob on the file name at any depth
  * and ? stop at /, ** crosses it

Output lists paths, counts and object ids only; file contents are never read or printed.
Exit codes: 0 ok, 1 listed paths found (verify) or failure, 2 usage error,
3 refused (sec3-perms: symlinked or wrong-type target).
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
    if argv[0] == "sec3-perms" {
        return sec3_main(&argv[1..]);
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

fn sec3_main(argv: &[String]) -> ExitCode {
    use history_purge::sec3::{self, Mode, Opts, Sec3Error};
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    let mut o = Opts {
        home: env("HERMES_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/opt/galion/hermes")),
        state_dir: env("SEC3_STATE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/root/sec3-perms")),
        mode: Mode::Dry,
        fix_world_writable: false,
        no_chown: std::env::var("SEC3_NO_CHOWN").as_deref() == Ok("1"),
    };
    let mut apply = false;
    let mut rollback: Option<PathBuf> = None;
    let mut it = argv.iter();
    while let Some(x) = it.next() {
        let mut val = |n: &str| {
            it.next()
                .map(PathBuf::from)
                .ok_or_else(|| format!("{n} needs a value"))
        };
        let r = match x.as_str() {
            "--apply" => {
                apply = true;
                Ok(())
            }
            "--fix-world-writable" => {
                o.fix_world_writable = true;
                Ok(())
            }
            "--no-chown" => {
                o.no_chown = true;
                Ok(())
            }
            "--rollback" => val("--rollback").map(|v| rollback = Some(v)),
            "--hermes-home" => val("--hermes-home").map(|v| o.home = v),
            "--state-dir" => val("--state-dir").map(|v| o.state_dir = v),
            other => Err(format!("unknown argument {other}")),
        };
        if let Err(e) = r {
            eprintln!("galion-purge sec3-perms: {e}\n(see --help)");
            return ExitCode::from(2);
        }
    }
    o.mode = match (apply, rollback) {
        (true, Some(_)) => {
            eprintln!("galion-purge sec3-perms: --apply and --rollback are exclusive");
            return ExitCode::from(2);
        }
        (true, None) => Mode::Apply,
        (false, Some(f)) => Mode::Rollback(f),
        (false, None) => Mode::Dry,
    };
    let mut out = std::io::stdout();
    match sec3::run(&o, &mut out) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e @ Sec3Error::Refuse(_)) => {
            eprintln!("galion-purge sec3-perms: {e}");
            ExitCode::from(3)
        }
        Err(e) => {
            eprintln!("galion-purge sec3-perms: {e}");
            ExitCode::from(2)
        }
    }
}
