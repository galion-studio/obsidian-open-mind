// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! hermes-dryrun — the dry-run step (step 4) of the Galion approval flow.
//!
//! Starts a candidate hermes-gateway against a COPY of the live layout on a
//! spare port in 19000-23999, with dummy secrets, a deterministic fake model
//! and loopback stubs, inside its own user+network+pid namespace with no
//! route off the machine. Runs health checks and scripted tool tasks,
//! proves no non-loopback connection happened, records fingerprints, tears
//! everything down and writes the unsigned `galion-sm/dryrun-result/v1`
//! record for the signer.
//!
//! ```text
//! hermes-dryrun snapshot --ssh slavik-vps --out LAYOUT_DIR
//! hermes-dryrun run --candidate BIN --layout LAYOUT_DIR --entry ENTRY_DIR --id ID
//!                   [--stack-bin openmind-mcp-stack] [--deadline-s 300]
//! ```

mod layout;
mod netwatch;
mod run;
mod snapshot;
mod stubs;
mod util;

use std::path::PathBuf;

const USAGE: &str = "usage:
  hermes-dryrun snapshot --ssh HOST --out LAYOUT_DIR
  hermes-dryrun run --candidate BIN --layout LAYOUT_DIR --entry ENTRY_DIR --id ID [--stack-bin BIN] [--deadline-s N]
  hermes-dryrun stub-mcp";

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // copied in place of an MCP binary that is not part of the dry run
    let argv0 = args
        .first()
        .and_then(|a| a.rsplit('/').next())
        .unwrap_or("");
    if !argv0.starts_with("hermes-dryrun") || args.get(1).map(String::as_str) == Some("stub-mcp") {
        std::process::exit(stubs::stub_mcp());
    }
    let code = match args.get(1).map(String::as_str) {
        Some("snapshot") => match (flag(&args, "--ssh"), flag(&args, "--out")) {
            (Some(h), Some(o)) => match snapshot::run(&h, &PathBuf::from(o)) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("snapshot: {e}");
                    2
                }
            },
            _ => {
                eprintln!("{USAGE}");
                2
            }
        },
        Some("run") => {
            let need = |n: &str| flag(&args, n).ok_or(format!("{n} is required"));
            let a = (|| -> Result<run::RunArgs, String> {
                Ok(run::RunArgs {
                    candidate: PathBuf::from(need("--candidate")?),
                    layout: PathBuf::from(need("--layout")?),
                    entry: PathBuf::from(need("--entry")?),
                    id: need("--id")?,
                    stack_bin: flag(&args, "--stack-bin").map(PathBuf::from),
                    deadline_s: flag(&args, "--deadline-s")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(300),
                })
            })();
            match a.and_then(|a| {
                let abs = |p: PathBuf| {
                    std::fs::canonicalize(&p).map_err(|e| format!("{}: {e}", p.display()))
                };
                std::fs::create_dir_all(&a.entry).map_err(|e| e.to_string())?;
                Ok(run::RunArgs {
                    candidate: abs(a.candidate)?,
                    layout: abs(a.layout)?,
                    entry: abs(a.entry)?,
                    stack_bin: a.stack_bin.map(abs).transpose()?,
                    ..a
                })
            }) {
                Ok(a) => match run::run(a) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("run: {e}");
                        2
                    }
                },
                Err(e) => {
                    eprintln!("run: {e}\n{USAGE}");
                    2
                }
            }
        }
        Some("inner") => match args.get(2) {
            Some(p) => run::inner(&PathBuf::from(p)),
            None => 2,
        },
        _ => {
            eprintln!("{USAGE}");
            2
        }
    };
    std::process::exit(code);
}
