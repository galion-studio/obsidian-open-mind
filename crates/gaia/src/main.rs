// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `gaia` CLI. Prints JSON. Simulation only (no network, no processes, no
//! file writes), except `status --live`: read-only HTTP GETs to loopback
//! health endpoints, `systemctl is-active`, and state-file mtimes; and
//! `heal-tick`: the hades-gate-approved (signed, single-use token), capped
//! restart of a box-local TEST service (127.0.0.1:19000-19999 only) with
//! nerve events over loopback.

use std::process::ExitCode;

const USAGE_HEAD: &str = "usage: gaia <command> [options]
  status                         boot GAIA and print every function, the hierarchy and the system table
  status --live [--host H] [--no-systemd] [--timeout-ms MS] [--strict]
                                 READ-ONLY live health of the real services (8420, 8642, link-watch,
                                 tuner 8650, one 8091, console 8080): HTTP GET, systemctl is-active,
                                 state-file age; HADES proposes plans only (never executes);
                                 --strict exits 3 unless everything is up
  heal-tick --service NAME --port P --allow-ports A-B --nerve 127.0.0.1:N --state DIR
            --hades-config C [--token-file F] [--health /health] [--wait-ms MS] -- /abs/start/cmd ARGS...
                                 M2 live heal, BOX TEST SERVICES ONLY (127.0.0.1, ports 19000-19999):
                                 probe -> nerve event -> GAIA -> DEMETER heal-dry -> HADES plan ->
                                 hades-gate check (disable, admin safety, Ed25519 token scoped to
                                 heal-apply + service + plan hash, expiry, single use) ->
                                 restart (max 3 per service per hour) -> nerve event;
                                 exit 0 ok, 4 refused, 5 capped, 6 apply failed
  heal-plan --service NAME --port P --allow-ports A-B [--health /health] [--out F] -- /abs/start/cmd ARGS...
                                 print the exact heal-apply plan and its SHA-256 for `hades-gate mint`;
                                 --out writes the plan bytes to a new file
  tick [--n N]                   boot and run N ticks (default 10, max 1000)
  simulate-faro [--full-power] [--approve ALPHA]
                                 run the Faro scenario; --full-power drives every system at intensity 100;
                                 --approve gives an Alpha's approval token to the HADES directives
  systems                        print the system -> function ownership table
  dispatch SYSTEM VERB [--payload TEXT] [--approve ALPHA]
                                 route one directive (verbs: status, run, plan, execute)
  --compact                      one-line JSON (any command)
{POSEIDON}";

fn usage() -> String {
    USAGE_HEAD.replace("{POSEIDON}", gaia::poseidon::USAGE)
}

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
    let mut live_exit = 0u8;
    let result = match args.first().copied() {
        Some("status") if args.contains(&"--live") => {
            let mut o = gaia_live::Opts::default();
            if let Some(h) = opt("--host") {
                o.host = h.to_string();
            }
            if args.contains(&"--no-systemd") {
                o.systemd = false;
            }
            if let Some(ms) = opt("--timeout-ms").and_then(|v| v.parse::<u64>().ok()) {
                o.connect_timeout = std::time::Duration::from_millis(ms.max(50));
                o.read_timeout = std::time::Duration::from_millis(ms.max(50) * 2);
            }
            let results = gaia_live::probe_all(&gaia_live::default_probes(), &o);
            let j = gaia::status_live(&results);
            if args.contains(&"--strict")
                && j.get("overall").and_then(gaia_core::Json::as_str) != Some("healthy")
            {
                live_exit = 3;
            }
            Ok(j)
        }
        Some("heal-tick") => match gaia::live_heal::parse_cli(&args[1..]) {
            Err(e) => Err(e),
            Ok(cfg) => match gaia::live_heal::heal_tick(&cfg) {
                Ok(t) => {
                    live_exit = t.exit;
                    Ok(t.report)
                }
                Err(e) => Err(e),
            },
        },
        Some("heal-plan") => gaia::live_heal::heal_plan_cli(&args[1..]),
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
        Some("poseidon") => match gaia::poseidon::run(&args[1..]) {
            Ok((j, code)) => {
                live_exit = code;
                Ok(j)
            }
            Err(e) => Err(e),
        },
        Some("dispatch") if opt("--queue").is_some() => match (args.get(1), args.get(2)) {
            (Some(s), Some(v)) => gaia::poseidon::queue_config(opt("--queue")).and_then(|cfg| {
                gaia::poseidon::dispatch(
                    cfg,
                    s,
                    v,
                    opt("--payload").unwrap_or(""),
                    opt("--approve"),
                )
                .map(|(j, code)| {
                    live_exit = code;
                    j
                })
            }),
            _ => Err("dispatch needs SYSTEM and VERB".to_string()),
        },
        Some("dispatch") => match (args.get(1), args.get(2)) {
            (Some(s), Some(v)) => {
                gaia::dispatch(s, v, opt("--payload").unwrap_or(""), opt("--approve"))
            }
            _ => Err("dispatch needs SYSTEM and VERB".to_string()),
        },
        Some("-h" | "--help") | None => {
            println!("{}", usage());
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
            ExitCode::from(live_exit)
        }
        Err(e) => {
            eprintln!("gaia: {e}\n{}", usage());
            ExitCode::from(2)
        }
    }
}
