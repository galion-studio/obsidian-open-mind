// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `gaia` CLI. Prints JSON. Simulation and observation only: no network, no
//! processes, no file writes. `view` reads the capture files it is given.

use std::process::ExitCode;

const USAGE: &str = "usage: gaia <command> [options]
  status                         boot GAIA and print every function, the hierarchy and the system table
  tick [--n N]                   boot and run N ticks (default 10, max 1000)
  simulate-faro [--full-power] [--approve ALPHA]
                                 run the Faro scenario; --full-power drives every system at intensity 100;
                                 --approve gives an Alpha's approval token to the HADES directives
  systems                        print the system -> function ownership table
  dispatch SYSTEM VERB [--payload TEXT] [--approve ALPHA]
                                 route one directive (verbs: status, run, plan, execute)
  plan SYSTEM [--payload TEXT]   ask the owning function for a plan only (never executes)
  dry-run [--function F]         plan every system; destructive systems also get an unapproved
                                 execute, which must come back refused with a dry-run plan
  ownership                      crate and unit -> system -> function registry (52-system design)
  view --units FILE [--listen FILE]
                                 Gaia's view of a real host from captured `systemctl list-units`
                                 and `ss -ltn[p]` text (FILE may be - for stdin); observe only
  --compact                      one-line JSON (any command)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let compact = args.iter().any(|a| a == "--compact");
    let args: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| *a != "--compact")
        .collect();
    let opt = |name: &str| -> Option<&str> {
        args.iter()
            .position(|a| *a == name)
            .and_then(|i| args.get(i + 1).copied())
    };
    let result = match args.first().copied() {
        Some("status") => Ok(gaia::status()),
        Some("tick") => match opt("--n").map(str::parse::<u64>) {
            None => Ok(gaia::tick(10)),
            Some(Ok(n)) => Ok(gaia::tick(n)),
            Some(Err(_)) => Err("--n needs a number".to_string()),
        },
        Some("simulate-faro") => Ok(gaia::simulate_faro(&gaia::ScenarioOpts {
            full_power: args.contains(&"--full-power"),
            approve: opt("--approve").map(str::to_string),
        })),
        Some("systems") => Ok(gaia::systems_json()),
        Some("dispatch") => match (args.get(1), args.get(2)) {
            (Some(s), Some(v)) => {
                gaia::dispatch(s, v, opt("--payload").unwrap_or(""), opt("--approve"))
            }
            _ => Err("dispatch needs SYSTEM and VERB".to_string()),
        },
        Some("plan") => match args.get(1) {
            Some(s) if !s.starts_with("--") => gaia::plan(s, opt("--payload").unwrap_or("")),
            _ => Err("plan needs SYSTEM".to_string()),
        },
        Some("dry-run") => gaia::dry_run(opt("--function")),
        Some("ownership") => Ok(gaia::ownership_json()),
        Some("view") => match opt("--units") {
            None => Err("view needs --units FILE".to_string()),
            Some(u) => read_capture(u).and_then(|units| {
                let listen = opt("--listen").map(read_capture).transpose()?;
                Ok(gaia::view::view(&units, listen.as_deref()))
            }),
        },
        Some("-h" | "--help") | None => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(other) => Err(format!("unknown command {other:?}")),
    };
    match result {
        Ok(j) => {
            let j = gaia_core::Json::obj()
                .with("generated_at", zagreb_time::now_rfc3339())
                .with("result", j);
            println!("{}", if compact { j.compact() } else { j.pretty() });
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("gaia: {e}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Read one capture file (or stdin for `-`). Capped at 4 MiB.
fn read_capture(path: &str) -> Result<String, String> {
    use std::io::Read;
    const CAP: u64 = 4 << 20;
    let mut s = String::new();
    let r = if path == "-" {
        std::io::stdin().take(CAP).read_to_string(&mut s)
    } else {
        std::fs::File::open(path)
            .map_err(|e| format!("{path}: {e}"))?
            .take(CAP)
            .read_to_string(&mut s)
    };
    r.map_err(|e| format!("{path}: {e}"))?;
    Ok(s)
}
