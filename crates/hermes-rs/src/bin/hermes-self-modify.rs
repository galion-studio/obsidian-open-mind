// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! hermes-self-modify: the non-Hermes side of Hermes self-modify.
//!
//!   hermes-self-modify keygen SEED_OUT PUB_OUT
//!   hermes-self-modify gate     --entry E --repo R --scratch S --plan PLAN.json --key SEED   (gate user)
//!   hermes-self-modify status   --entry E --sandbox-pub PUB --owner-pub PUB
//!   hermes-self-modify approve  --entry E --owner-key SEED --sandbox-pub PUB --by NAME [--ttl-s N]  (owner)
//!   hermes-self-modify deploy   --entry E --sandbox-pub PUB --owner-pub PUB --bin-dir D --dropin-dir D
//!                               [--dropin-name N.conf] [--line 'Environment=K=V']...   (root, by a person)
//!   hermes-self-modify rollback --entry E
//!
//! approve, deploy and rollback refuse to run with a Hermes runtime
//! environment (HERMES_HOME / HERMES_RS_* set), as one more layer on top of
//! the keys. Exit: 0 ok/pass, 1 fail/refused, 2 usage or I/O error.

use hermes_rs::self_modify as sm;
use std::path::{Path, PathBuf};

fn die(code: i32, m: &str) -> ! {
    eprintln!("hermes-self-modify: {m}");
    std::process::exit(code)
}

fn opt(a: &[String], k: &str) -> Option<String> {
    a.iter()
        .position(|x| x == k)
        .and_then(|i| a.get(i + 1))
        .cloned()
}

fn req(a: &[String], k: &str) -> String {
    opt(a, k).unwrap_or_else(|| die(2, &format!("missing {k}")))
}

fn text(p: &str) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| die(2, &format!("{p}: {e}")))
}

fn vk(a: &[String], k: &str) -> ed25519_dalek::VerifyingKey {
    sm::verifying_key(&text(&req(a, k))).unwrap_or_else(|e| die(2, &e))
}

fn not_hermes(what: &str) {
    if std::env::vars().any(|(k, _)| k == "HERMES_HOME" || k.starts_with("HERMES_RS_")) {
        die(
            1,
            &format!(
                "refused: {what} cannot run inside Hermes's environment; only the owner does this"
            ),
        );
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let entry = || PathBuf::from(req(&a, "--entry"));
    let out = match a.first().map(String::as_str) {
        Some("keygen") => {
            let (seed, pubk) = sm::keygen().unwrap_or_else(|e| die(2, &e));
            let s = a.get(1).unwrap_or_else(|| die(2, "SEED_OUT"));
            {
                use std::io::Write;
                use std::os::unix::fs::OpenOptionsExt;
                let mut f = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(s)
                    .unwrap_or_else(|e| die(2, &format!("{s}: {e}")));
                f.write_all(seed.as_bytes()).unwrap_or_else(|e| die(2, &e.to_string()));
            }
            let p = a.get(2).unwrap_or_else(|| die(2, "PUB_OUT"));
            std::fs::write(p, format!("{pubk}\n")).unwrap_or_else(|e| die(2, &e.to_string()));
            Ok(serde_json::json!({"public": pubk}))
        }
        Some("gate") => {
            let plan = serde_json::from_str(&text(&req(&a, "--plan")))
                .map_err(|e| e.to_string())
                .and_then(|v| sm::Plan::from_json(&v))
                .unwrap_or_else(|e| die(2, &e));
            let key = sm::signing_key(&text(&req(&a, "--key"))).unwrap_or_else(|e| die(2, &e));
            sm::gate_entry(
                &entry(),
                Path::new(&req(&a, "--repo")),
                Path::new(&req(&a, "--scratch")),
                &plan,
                &key,
            )
        }
        Some("status") => {
            let e = sm::Entry::open(&entry());
            Ok(serde_json::json!({"state": e.state(&vk(&a, "--sandbox-pub"), &vk(&a, "--owner-pub"))}))
        }
        Some("approve") => {
            not_hermes("approve");
            let key = sm::signing_key(&text(&req(&a, "--owner-key"))).unwrap_or_else(|e| die(2, &e));
            let ttl = opt(&a, "--ttl-s").and_then(|v| v.parse().ok()).unwrap_or(86_400);
            sm::approve(&entry(), &key, &vk(&a, "--sandbox-pub"), &req(&a, "--by"), ttl)
        }
        Some("deploy") => {
            not_hermes("deploy");
            let lines = a
                .iter()
                .enumerate()
                .filter(|(_, x)| *x == "--line")
                .filter_map(|(i, _)| a.get(i + 1).cloned())
                .collect();
            let d = sm::DeployCfg {
                bin_dir: PathBuf::from(req(&a, "--bin-dir")),
                dropin_dir: PathBuf::from(req(&a, "--dropin-dir")),
                dropin_name: opt(&a, "--dropin-name").unwrap_or_else(|| "99s-self-modify.conf".into()),
                extra_lines: lines,
            };
            sm::deploy(&entry(), &vk(&a, "--sandbox-pub"), &vk(&a, "--owner-pub"), &d)
        }
        Some("rollback") => {
            not_hermes("rollback");
            sm::rollback(&entry())
        }
        _ => die(2, "usage: keygen | gate | status | approve | deploy | rollback (see --help in the source header)"),
    };
    match out {
        Ok(v) => {
            println!("{v}");
            let fail = v.get("verdict").is_some_and(|x| x != "pass");
            std::process::exit(if fail { 1 } else { 0 });
        }
        Err(e) => die(1, &e),
    }
}
