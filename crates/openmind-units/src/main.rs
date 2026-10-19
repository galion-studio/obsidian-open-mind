// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Print staged unit text from an openmind.toml fixture.
//!
//! Writes files only when `--out` is set. Does not install units and does
//! not start them.

#![forbid(unsafe_code)]

use openmind_units::{render_fixture, render_fixture_now, write_rendered, RenderedUnit};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

struct Args {
    fixture: String,
    out: Option<String>,
    stamp: Option<String>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("openmind-units: {message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let fixture_path = &args.fixture;
    let text = fs::read_to_string(fixture_path)
        .map_err(|err| format!("cannot read {fixture_path}: {err}"))?;
    let units = match &args.stamp {
        Some(stamp) => render_fixture(&text, stamp).map_err(|err| err.to_string())?,
        None => render_fixture_now(&text).map_err(|err| err.to_string())?,
    };
    if let Some(dir) = &args.out {
        write_rendered(Path::new(dir), &units).map_err(|err| err.to_string())?;
    }
    print_units(&units)
}

fn print_units(units: &[RenderedUnit]) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    for unit in units {
        let file_name = &unit.file_name;
        writeln!(stdout, "# file: {file_name}").map_err(|err| err.to_string())?;
        stdout
            .write_all(unit.text.as_bytes())
            .map_err(|err| err.to_string())?;
        if !unit.text.ends_with('\n') {
            writeln!(stdout).map_err(|err| err.to_string())?;
        }
    }
    stdout.flush().map_err(|err| err.to_string())
}

fn parse_args() -> Result<Args, String> {
    let mut fixture = None;
    let mut out = None;
    let mut stamp = None;
    let mut raw = env::args().skip(1);
    while let Some(arg) = raw.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            "--fixture" => fixture = Some(need(&mut raw, "--fixture")?),
            "--out" => out = Some(need(&mut raw, "--out")?),
            "--stamp" => stamp = Some(need(&mut raw, "--stamp")?),
            other if other.starts_with('-') => {
                return Err(format!("unknown argument {other}"));
            }
            other if fixture.is_none() => fixture = Some(other.to_string()),
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    let fixture = fixture.ok_or_else(|| "missing fixture path (see --help)".to_string())?;
    Ok(Args {
        fixture,
        out,
        stamp,
    })
}

fn need(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{flag} needs a value"))
}

fn print_help() {
    println!(
        "\
openmind-units — print staged unit text from an openmind.toml fixture

Usage:
  openmind-units --fixture <openmind.toml> [--stamp <zagreb-rfc3339>] [--out <dir>]

Reads the fixture and prints unit text on stdout.
--stamp is a Europe/Zagreb RFC 3339 time with an explicit +01:00 or +02:00 offset.
Without --stamp the clock is read in Europe/Zagreb.
--out also writes <name>.service files into that directory.
This program does not install units and does not start them."
    );
}
