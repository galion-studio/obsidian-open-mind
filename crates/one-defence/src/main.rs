// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! `one-defence` binary. Replaces `galion-light/one/defence.py`.
//!
//! Default stdout is a JSON array of `{service, status, ms}`. `--text` prints
//! the old `service status ms` lines. The database is opened read-only.

use one_defence::{query_path, to_json, to_text};
use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

/// Live guardian database on this host. defence.py hard-coded the laptop path
/// `/workspace/galion-light/guardian/guardian.db`; the file here lives under
/// `/opt/galion/galion-light`. Override with a positional path or `ONE_DEFENCE_DB`.
const DEFAULT_DB: &str = "/opt/galion/galion-light/guardian/guardian.db";

const USAGE: &str = "\
one-defence [--json|--text] [DB]

Read the newest health_checks row per service (defence.py) and print it.
Default output is a JSON array of {service, status, ms}. --text prints the
old \"service status ms\" lines.

The database is opened read-only (SQLite URI mode=ro). This program never writes.

DB is the positional path if given, otherwise ONE_DEFENCE_DB, otherwise
/opt/galion/galion-light/guardian/guardian.db.

Exit 0 on success, 1 if the database cannot be read, 2 on usage errors.
";

struct Exit {
    code: u8,
    message: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Json,
    Text,
}

fn default_db() -> PathBuf {
    match env::var("ONE_DEFENCE_DB") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(DEFAULT_DB),
    }
}

fn parse(args: &[String]) -> Result<(Mode, PathBuf), String> {
    let mut mode = Mode::Json;
    let mut saw_mode = false;
    let mut path: Option<PathBuf> = None;
    for arg in args {
        match arg.as_str() {
            "--text" => {
                if saw_mode && mode != Mode::Text {
                    return Err("choose only one of --json or --text".into());
                }
                mode = Mode::Text;
                saw_mode = true;
            }
            "--json" => {
                if saw_mode && mode != Mode::Json {
                    return Err("choose only one of --json or --text".into());
                }
                mode = Mode::Json;
                saw_mode = true;
            }
            "-h" | "--help" => return Err("use --help alone to print usage".into()),
            s if s.starts_with('-') => return Err(format!("unknown flag {s}")),
            s => {
                if path.is_some() {
                    return Err("too many arguments".into());
                }
                path = Some(PathBuf::from(s));
            }
        }
    }
    Ok((mode, path.unwrap_or_else(default_db)))
}

fn try_main() -> Result<(), Exit> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return Ok(());
    }
    let (mode, path) = parse(&args).map_err(|message| Exit { code: 2, message })?;
    let rows = query_path(&path).map_err(|e| Exit {
        code: 1,
        message: e.to_string(),
    })?;
    let mut stdout = io::stdout().lock();
    match mode {
        Mode::Json => {
            let body = to_json(&rows).map_err(|e| Exit {
                code: 1,
                message: e.to_string(),
            })?;
            writeln!(stdout, "{body}").map_err(|e| Exit {
                code: 1,
                message: e.to_string(),
            })?;
        }
        Mode::Text => {
            let body = to_text(&rows);
            write!(stdout, "{body}").map_err(|e| Exit {
                code: 1,
                message: e.to_string(),
            })?;
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match try_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("one-defence: {}", err.message);
            ExitCode::from(err.code)
        }
    }
}
