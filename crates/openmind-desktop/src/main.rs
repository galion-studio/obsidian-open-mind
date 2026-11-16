// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `openmind-desktop` binary. See the library docs for the modes.

use openmind_desktop::{DesktopError, Mode, ProbeConfig, USAGE};
use std::process::ExitCode;

fn run() -> Result<(), DesktopError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = openmind_desktop::parse(&args)?;
    match mode {
        Mode::Help => println!("{USAGE}"),
        Mode::Version => {
            println!(
                "{}",
                galion_common::build_info::version_line(
                    "openmind-desktop",
                    env!("CARGO_PKG_VERSION")
                )
            );
        }
        Mode::Page => print!(
            "{}",
            openmind_desktop::present(openmind_desktop::document())
        ),
        Mode::Listen(opts) => {
            let probes = load_probes(None)?;
            let env_port = listen_port_from_env()?;
            let addr =
                openmind_desktop::resolve_listen_addr(opts.addr.as_deref(), env_port.as_deref())?;
            let files = openmind_desktop::Files::open(
                &opts.grants,
                opts.inbox.as_deref(),
                opts.stack.as_deref(),
            )?;
            openmind_desktop::listen_with(&addr, &probes, &files)?;
        }
        Mode::Probe(spec) => {
            let probes = load_probes(spec.as_deref())?;
            let report = openmind_desktop::probe_all(&probes);
            println!("{}", report.to_json());
            if !report.confirmed() {
                return Err(DesktopError::Probes(report.summary()));
            }
        }
    }
    Ok(())
}

/// [`openmind_desktop::PORT_ENV`] when it is set. Unset is `None`. A value
/// that is not Unicode is a usage error. Blank is handled later as unset.
fn listen_port_from_env() -> Result<Option<String>, DesktopError> {
    match std::env::var(openmind_desktop::PORT_ENV) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(DesktopError::Usage(format!(
            "openmind-desktop: {} is not unicode\n{USAGE}",
            openmind_desktop::PORT_ENV
        ))),
    }
}

/// Spec from the command, otherwise `OPENMIND_DESKTOP_PROBES`, otherwise
/// the localhost defaults. Printing the page never calls this.
fn load_probes(spec: Option<&str>) -> Result<ProbeConfig, DesktopError> {
    let chosen = match spec.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => Some(value.to_string()),
        None => std::env::var("OPENMIND_DESKTOP_PROBES")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    };
    match chosen {
        Some(value) => ProbeConfig::parse(&value)
            .map_err(|err| DesktopError::Usage(format!("openmind-desktop: {err}\n{USAGE}"))),
        None => Ok(ProbeConfig::defaults()),
    }
}

fn main() -> ExitCode {
    galion_common::build_info::exit_if_version("openmind-desktop", env!("CARGO_PKG_VERSION"));
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            let code = u8::try_from(err.exit_code()).unwrap_or(1);
            ExitCode::from(code)
        }
    }
}
