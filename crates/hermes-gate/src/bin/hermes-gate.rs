// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! hermes-gate CLI. Never pushes; never applies in place or to a live path.
//!
//!   hermes-gate keygen  SEED_OUT PUB_OUT
//!   hermes-gate propose --id ID --summary S [--tier T] --out P.json
//!                       (--worktree W | --base-commit SHA --patch FILE)
//!   hermes-gate gate    --repo R --scratch D --proposal P.json --plan PLAN.json
//!                       --key SEED --out REC.json [--allow-tree a/,b/] [--allow net,spawn,unsafe]
//!   hermes-gate verify  --record REC.json --sandbox-pub HEX|FILE --proposal P.json
//!   hermes-gate approve --owner-key SEED --record REC.json --proposal P.json
//!                       --target-head SHA [--ttl-s N] [--label TEXT] --out A.json
//!   hermes-gate apply   --source R --staging-root D [--ledger F] --record REC.json
//!                       --sandbox-pub HEX|FILE --owner-pub HEX|FILE --proposal P.json
//!                       --approval A.json
//!
//! Exit: 0 ok/pass, 1 fail/refused, 2 usage or I/O error.

use hermes_gate::{apply, checks, proposal::Proposal, record, stage};
use serde_json::json;
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

/// A public key given as hex or as a file holding the hex.
fn pubkey(v: &str) -> ed25519_dalek::VerifyingKey {
    let hex = if Path::new(v).is_file() {
        read_text(v)
    } else {
        v.to_string()
    };
    record::verifying_key(&hex).unwrap_or_else(|e| die(2, &e))
}

fn list(a: &[String], k: &str) -> Option<Vec<String>> {
    opt(a, k).map(|v| {
        v.split(',')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(str::to_string)
            .collect()
    })
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
            let tier = opt(&a, "--tier").unwrap_or_else(|| "high".into());
            let p = if let Some(w) = opt(&a, "--worktree") {
                Proposal::from_worktree(
                    Path::new(&w),
                    &req(&a, "--id"),
                    &req(&a, "--summary"),
                    &tier,
                )
            } else {
                Proposal::from_patch(
                    &req(&a, "--base-commit"),
                    &read_text(&req(&a, "--patch")),
                    &req(&a, "--id"),
                    &req(&a, "--summary"),
                    &tier,
                )
            }
            .unwrap_or_else(|e| die(1, &e));
            write_out(&req(&a, "--out"), &p.to_json());
            println!("{}", p.sha256());
        }
        Some("gate") => {
            let p = proposal(&req(&a, "--proposal"));
            let plan = stage::Plan::from_json(&read_json(&req(&a, "--plan")))
                .unwrap_or_else(|e| die(2, &e));
            let key_path = PathBuf::from(req(&a, "--key"));
            let sk =
                record::signing_key(&read_text(&req(&a, "--key"))).unwrap_or_else(|e| die(2, &e));
            let repo = PathBuf::from(req(&a, "--repo"));
            let scratch = PathBuf::from(req(&a, "--scratch"));
            if stage::under_live_root(&repo) || stage::under_live_root(&scratch) {
                die(1, "gate refuses live paths for --repo and --scratch");
            }
            let mut pol = checks::CheckPolicy::default();
            if let Some(t) = list(&a, "--allow-tree") {
                pol.allow_tree = t;
            }
            if let Some(c) = list(&a, "--allow") {
                if let Some(bad) = c
                    .iter()
                    .find(|x| !["net", "spawn", "unsafe"].contains(&x.as_str()))
                {
                    die(2, &format!("--allow {bad}: only net, spawn, unsafe"));
                }
                pol.allow_caps = c;
            }
            let t0 = std::time::Instant::now();
            let cs = checks::run(&p, &repo, &pol);
            for c in &cs {
                eprintln!(
                    "check {:<20} {}  {}",
                    c.name,
                    if c.ok { "PASS" } else { "FAIL" },
                    c.detail
                );
            }
            // The sandbox deploy only runs when every static check passes.
            // The sandbox key's dir is hidden from every stage.
            let hide: Vec<PathBuf> = key_path
                .canonicalize()
                .ok()
                .and_then(|k| k.parent().map(Path::to_path_buf))
                .into_iter()
                .collect();
            let rep = if checks::all_ok(&cs) {
                Some(
                    stage::sandbox_deploy_hiding(&repo, &scratch, &p, &plan, &hide)
                        .unwrap_or_else(|e| die(2, &e)),
                )
            } else {
                None
            };
            if let Some(r) = &rep {
                if let Some(e) = &r.apply_error {
                    eprintln!("sandbox apply FAIL {e}");
                }
                for st in &r.stages {
                    eprintln!(
                        "stage {:<8} {} exit={:?} wall_ms={} missing_layers={:?}",
                        st.name,
                        if st.ok { "PASS" } else { "FAIL" },
                        st.exit,
                        st.wall_ms,
                        st.missing_layers
                    );
                }
            }
            let rec = record::build(&p, &plan, &cs, rep.as_ref());
            let verdict = rec["verdict"].as_str().unwrap_or("fail").to_string();
            let signed = record::sign(&rec, &sk);
            write_out(&req(&a, "--out"), &signed);
            println!(
                "{}",
                json!({"verdict": verdict, "checks_passed": rec["checks_passed"], "checks_total": rec["checks_total"],
                       "record_sha256": record::record_sha256(&signed), "result_tree": rec["result_tree"],
                       "wall_ms": t0.elapsed().as_millis() as u64})
            );
            std::process::exit(if verdict == "pass" { 0 } else { 1 });
        }
        Some("verify") => {
            let vk = pubkey(&req(&a, "--sandbox-pub"));
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
            let target = req(&a, "--target-head");
            if !hermes_gate::proposal::commit_ok(&target) {
                die(2, "--target-head must be a full lowercase commit sha");
            }
            let v = apply::approve_labeled(
                &sk,
                &p.sha256(),
                &record::record_sha256(&signed),
                &target,
                hermes_rs::time::now_secs() + ttl.clamp(60, 86_400),
                &opt(&a, "--label").unwrap_or_default(),
            );
            write_out(&req(&a, "--out"), &v);
            println!(
                "approval {} expires {}",
                apply::approval_id(&v),
                v["expires"]
            );
        }
        Some("apply") => {
            let t = apply::ApplyTarget {
                source: PathBuf::from(req(&a, "--source")),
                staging_root: PathBuf::from(req(&a, "--staging-root")),
                ledger: PathBuf::from(opt(&a, "--ledger").unwrap_or_else(|| {
                    format!(
                        "{}/applied.ledger",
                        req(&a, "--staging-root").trim_end_matches('/')
                    )
                })),
            };
            // Live paths are refused here, before any file is read or any
            // command (git included) is started.
            apply::refuse_live(&t).unwrap_or_else(|e| die(1, &e));
            let p = proposal(&req(&a, "--proposal"));
            let svk = pubkey(&req(&a, "--sandbox-pub"));
            let ovk = pubkey(&req(&a, "--owner-pub"));
            let approval = match opt(&a, "--approval") {
                Some(f) => read_json(&f),
                None => serde_json::Value::Null,
            };
            let r = apply::apply_staged(
                &t,
                &read_json(&req(&a, "--record")),
                &svk,
                &p,
                &approval,
                &ovk,
                hermes_rs::time::now_secs(),
            )
            .unwrap_or_else(|e| die(1, &e));
            println!(
                "{}",
                json!({"applied": "staging copy (index, not committed)", "staging_copy": r.staging_copy.display().to_string(), "tree": r.tree})
            );
        }
        _ => die(
            2,
            "usage: keygen | propose | gate | verify | approve | apply (see --help in source)",
        ),
    }
}
