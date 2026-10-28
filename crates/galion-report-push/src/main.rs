// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! galion-report-push CLI.
//!
//! ```text
//! galion-report-push publish --remote URL --branch swarm/X --file SRC:DEST [--file ...]
//!                            --message MSG [--clone DIR] [--base REF] [--push]
//! galion-report-push check PATH...      exit 0 all allowed, 3 any protected
//! galion-report-push protected          list protected roots
//! ```
//! Exit: 0 ok, 2 usage, 3 refused by the openmind write guard, 4 git/io error.

use galion_report_push::guard::Guard;
use galion_report_push::{default_clone, parse_file_map, publish, Error, Plan};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage:
  galion-report-push publish --remote URL --branch swarm/X --file SRC:DEST [--file ...]
                             --message MSG [--clone DIR] [--base REF] [--push]
  galion-report-push check PATH...
  galion-report-push protected
exit: 0 ok, 2 usage, 3 refused (openmind write guard, D-391-2), 4 git/io error";

fn parse_publish(args: &[String]) -> Result<Plan, Error> {
    let mut clone = None;
    let mut remote = None;
    let mut branch = None;
    let mut base = None;
    let mut files = Vec::new();
    let mut message = None;
    let mut push = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = |n: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| Error::Usage(format!("{n} needs a value")))
        };
        match a.as_str() {
            "--clone" => clone = Some(PathBuf::from(val("--clone")?)),
            "--remote" => remote = Some(val("--remote")?),
            "--branch" => branch = Some(val("--branch")?),
            "--base" => base = Some(val("--base")?),
            "--file" => files.push(parse_file_map(&val("--file")?)?),
            "--message" | "-m" => message = Some(val("--message")?),
            "--push" => push = true,
            other => return Err(Error::Usage(format!("unknown argument {other:?}"))),
        }
    }
    Ok(Plan {
        clone: clone.unwrap_or_else(default_clone),
        remote: remote.ok_or_else(|| Error::Usage("--remote is required".into()))?,
        branch: branch.ok_or_else(|| Error::Usage("--branch is required".into()))?,
        base,
        files,
        message: message.ok_or_else(|| Error::Usage("--message is required".into()))?,
        push,
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let guard = Guard::from_env();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let res = match args.first().map(String::as_str) {
        Some("publish") => parse_publish(&args[1..]).and_then(|plan| {
            let o = publish(&guard, &plan, &cwd)?;
            match &o.commit {
                Some(sha) => println!(
                    "committed {} {} in {}{}",
                    plan.branch,
                    sha,
                    plan.clone.display(),
                    if o.pushed {
                        " (pushed)"
                    } else {
                        " (not pushed)"
                    }
                ),
                None => println!("nothing to commit on {}", plan.branch),
            }
            Ok(())
        }),
        Some("check") if args.len() > 1 => {
            let mut res = Ok(());
            for p in &args[1..] {
                match guard.check_path("path", Path::new(p), &cwd) {
                    Ok(()) => println!("ok       {p}"),
                    Err(r) => {
                        println!("refused  {p}");
                        if res.is_ok() {
                            res = Err(Error::Refused(r));
                        }
                    }
                }
            }
            res
        }
        Some("protected") => {
            for p in guard.protected() {
                println!("{}", p.display());
            }
            Ok(())
        }
        Some("-h") | Some("--help") => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(Error::Usage("see --help".into())),
    };
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("galion-report-push: {e}");
            if matches!(e, Error::Usage(_)) {
                eprintln!("{USAGE}");
            }
            ExitCode::from(e.exit_code())
        }
    }
}
