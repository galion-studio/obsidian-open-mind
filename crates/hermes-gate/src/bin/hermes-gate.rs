// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! hermes-gate CLI (prototype; never pushes, never touches a live root).
//!
//!   hermes-gate keygen SEED_OUT PUB_OUT
//!   hermes-gate propose --worktree W --id ID --summary S [--tier T] --out P.json
//!   hermes-gate gate    --repo R --scratch D --proposal P.json --plan PLAN.json --key SEED --out REC.json
//!   hermes-gate verify  --record REC.json --sandbox-pub HEX --proposal P.json
//!   hermes-gate approve --owner-key SEED --record REC.json --proposal P.json --target-head SHA [--ttl-s N] --out A.json
//!   hermes-gate apply   --target T --record REC.json --sandbox-pub HEX --owner-pub HEX --proposal P.json --approval A.json
//!
//! Exit: 0 ok/pass, 1 fail/refused, 2 usage or I/O error.

use hermes_gate::{apply, checks, proposal::Proposal, record, stage};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn die(code: i32, m: &str) -> ! {
    eprintln!("hermes-gate: {m}");
    std::process::exit(code)
}

fn opt(args: &[String], k: &str) -> Option<String> {
    args.iter()
        .position(|a| a == k)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn req(args: &[String], k: &str) -> String {
    opt(args, k).unwrap_or_else(|| die(2, &format!("missing {k}")))
}

fn read_json(p: &str) -> Value {
    let t = std::fs::read_to_string(p).unwrap_or_else(|e| die(2, &format!("{p}: {e}")));
    serde_json::from_str(&t).unwrap_or_else(|e| die(2, &format!("{p}: {e}")))
}

fn read_text(p: &str) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| die(2, &format!("{p}: {e}")))
}

fn write_private(p: &Path, body: &str) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(p)
        .unwrap_or_else(|e| die(2, &format!("{}: {e}", p.display())));
    f.write_all(body.as_bytes())
        .unwrap_or_else(|e| die(2, &format!("{}: {e}", p.display())));
}

fn write_out(p: &str, v: &Value) {
    let tmp = format!("{p}.tmp");
    std::fs::write(&tmp, format!("{v}\n")).unwrap_or_else(|e| die(2, &format!("{tmp}: {e}")));
    std::fs::rename(&tmp, p).unwrap_or_else(|e| die(2, &format!("{p}: {e}")));
}

fn proposal(p: &str) -> Proposal {
    Proposal::from_json(&read_json(p)).unwrap_or_else(|e| die(2, &e))
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(String::as_str) {
        Some("keygen") => {
            let (seed, pubk) = record::keygen().unwrap_or_else(|e| die(2, &e));
            write_private(
                Path::new(a.get(1).unwrap_or_else(|| die(2, "SEED_OUT"))),
                &seed,
            );
            std::fs::write(
                a.get(2).unwrap_or_else(|| die(2, "PUB_OUT")),
                format!("{pubk}\n"),
            )
            .unwrap_or_else(|e| die(2, &e.to_string()));
            println!("{pubk}");
        }
        Some("propose") => {
            let p = Proposal::from_worktree(
                Path::new(&req(&a, "--worktree")),
                &req(&a, "--id"),
                &req(&a, "--summary"),
                &opt(&a, "--tier").unwrap_or_else(|| "high".into()),
            )
            .unwrap_or_else(|e| die(1, &e));
            write_out(&req(&a, "--out"), &p.to_json());
            println!("{}", p.sha256());
        }
        Some("gate") => {
            let p = proposal(&req(&a, "--proposal"));
            let plan = stage::Plan::from_json(&read_json(&req(&a, "--plan")))
                .unwrap_or_else(|e| die(2, &e));
            let sk =
                record::signing_key(&read_text(&req(&a, "--key"))).unwrap_or_else(|e| die(2, &e));
            let cs = checks::run(&p);
            // The sandbox deploy only runs when the static checks pass.
            let rep = if checks::all_ok(&cs) {
                Some(
                    stage::sandbox_deploy(
                        &PathBuf::from(req(&a, "--repo")),
                        &PathBuf::from(req(&a, "--scratch")),
                        &p,
                        &plan,
                    )
                    .unwrap_or_else(|e| die(2, &e)),
                )
            } else {
                None
            };
            let rec = record::build(&p, &plan, &cs, rep.as_ref());
            let verdict = rec["verdict"].as_str().unwrap_or("fail").to_string();
            write_out(&req(&a, "--out"), &record::sign(&rec, &sk));
            println!("{verdict}");
            std::process::exit(if verdict == "pass" { 0 } else { 1 });
        }
        Some("verify") => {
            let vk =
                record::verifying_key(&req(&a, "--sandbox-pub")).unwrap_or_else(|e| die(2, &e));
            let rec = record::verify(&read_json(&req(&a, "--record")), &vk)
                .unwrap_or_else(|e| die(1, &e));
            let p = proposal(&req(&a, "--proposal"));
            if rec["proposal_sha256"] != p.sha256().as_str() {
                die(1, "record is for a different proposal");
            }
            println!("{}", rec["verdict"].as_str().unwrap_or("fail"));
            std::process::exit(if rec["verdict"] == "pass" { 0 } else { 1 });
        }
        Some("approve") => {
            let sk = record::signing_key(&read_text(&req(&a, "--owner-key")))
                .unwrap_or_else(|e| die(2, &e));
            let signed = read_json(&req(&a, "--record"));
            let p = proposal(&req(&a, "--proposal"));
            let ttl: i64 = opt(&a, "--ttl-s")
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600);
            let v = apply::approve(
                &sk,
                &p.sha256(),
                &record::record_sha256(&signed),
                &req(&a, "--target-head"),
                hermes_rs::time::now_secs() + ttl.clamp(60, 86_400),
            );
            write_out(&req(&a, "--out"), &v);
        }
        Some("apply") => {
            let target = PathBuf::from(req(&a, "--target"));
            if stage::under_live_root(&target) {
                die(1, "live apply is not enabled in this prototype (owner rule: nothing live changes)");
            }
            let p = proposal(&req(&a, "--proposal"));
            let svk =
                record::verifying_key(&req(&a, "--sandbox-pub")).unwrap_or_else(|e| die(2, &e));
            let ovk = record::verifying_key(&req(&a, "--owner-pub")).unwrap_or_else(|e| die(2, &e));
            let head = hermes_gate::git::head(&target).unwrap_or_else(|e| die(2, &e));
            let auth = apply::authorize(
                &read_json(&req(&a, "--record")),
                &svk,
                &p,
                &read_json(&req(&a, "--approval")),
                &ovk,
                &head,
                hermes_rs::time::now_secs(),
            )
            .unwrap_or_else(|e| die(1, &e));
            let tree = apply::apply(&target, &auth, &p).unwrap_or_else(|e| die(1, &e));
            println!("applied (staged, not committed) tree {tree}");
        }
        _ => die(
            2,
            "usage: keygen | propose | gate | verify | approve | apply (see --help in source)",
        ),
    }
}
