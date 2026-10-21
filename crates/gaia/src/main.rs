// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `gaia` CLI. Prints JSON. Simulation and observation only (no network, no
//! processes, no file writes; `view` only reads the capture files it is
//! given), except `status --live`: read-only HTTP GETs to loopback health
//! endpoints, `systemctl is-active`, and state-file mtimes; `poseidon` and
//! `dispatch --queue`: loopback HTTP to openmind-queue (budget read-only, never
//! leases or acks); and `heal-tick`: the hades-gate-approved (signed,
//! single-use token), capped restart of a box-local TEST service
//! (127.0.0.1:19000-19999 only) with nerve events over loopback.

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
                                 exit 0 ok, 4 refused, 5 capped, 6 apply failed, 8 hung (reported, never killed)
                                 D-L2: the nerve's restarts (GET /state restarts_1h / reflex live_1h) count
                                 toward the 3/h cap; an unreadable /state fails closed
                                 D-L4: a service that holds its port but never answers -> gaia.heal.hung
                                 event on the nerve, no restart, no kill, no token used
  heal-tick --loop [--interval-ms 30000] [--jitter-ms 0] [--max-ticks N] [--max-runtime-s S]
            [--max-restarts R] [--max-errors 5] [--stop-file F] [--token-dir D] [--seed N] ...heal-tick flags... -- CMD
                                 S3.1 supervised loop: one heal-tick per interval + random jitter (monotonic
                                 clock), budget stops (ticks, runtime, restarts, consecutive errors -> exit 7,
                                 stop file); setup errors and panics back off instead of ending the loop;
                                 --token-dir: pre-minted single-use tokens (*.tok), used ones renamed .spent;
                                 the 3/h cap is GLOBAL (gaia-restarts.jsonl next to hades-gate's replay
                                 ledger), a new --state dir does not reset it; one JSON line per tick
                                 expired *.tok files are renamed .tok.expired before use
  nerve-restarts (--nerve 127.0.0.1:N | --state-file F) --service S [--service S2 ...]
                                 READ-ONLY D-L2 view: nerve restarts in the last hour per service and how
                                 many of the combined 3/h that leaves for Gaia
  tokens request --token-dir D [--count 3] [--ttl-s 900] --service NAME --port P --allow-ports A-B -- CMD
                                 D-L3: ask the admin for a small batch (1-3) of short-lived (<=3600 s)
                                 single-use heal-apply tokens; refuses past 3 live tokens in D;
                                 no standing grant, no HADES mode change
  tokens mint --request R --seed S
                                 ADMIN SIDE ONLY (needs the admin seed): mint the batch as 0600 *.tok
  tokens status --token-dir D [--hades-config C] [--prune]
                                 state, expiry (Zagreb), signature and ledger state per token file;
                                 never prints a token; --prune renames expired ones .tok.expired
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
  plan SYSTEM [--payload TEXT]   ask the owning function for a plan only (never executes)
  dry-run [--function F]         plan every system; destructive systems also get an unapproved
                                 execute, which must come back refused with a dry-run plan
  ownership                      crate and unit -> system -> function registry (52-system design)
  view --units FILE [--listen FILE]
                                 Gaia's view of a real host from captured `systemctl list-units`
                                 and `ss -ltn[p]` text (FILE may be - for stdin); observe only
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
        Some("heal-tick") if args.contains(&"--loop") => {
            match gaia::heal_loop::parse_cli(&args[1..]) {
                Err(e) => Err(e),
                Ok(cfg) => {
                    let run = gaia::heal_loop::run(&cfg, &mut std::io::stdout());
                    live_exit = run.exit;
                    Ok(run.summary)
                }
            }
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
        Some("nerve-restarts") => gaia::live_heal::nerve_restarts_cli(&args[1..]),
        Some("tokens") => gaia::tokens::cli(&args[1..]).map(|v| gaia_live::nerve::to_gaia_json(&v)),
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
