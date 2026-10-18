// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `gaia` CLI. Prints JSON. Simulation only: no network, no processes, no
//! file writes.

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
