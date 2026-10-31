// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! galion-sm: stack signing (openmind-hermes) and owner helpers (Slavik's
//! device, plain Ed25519 key file; galion.app does the passkey version).
//!
//!   galion-sm stack-keygen      --seed-out SEED              (prints public key)
//!   galion-sm stack-sign        --entry E --key SEED         (openmind-hermes)
//!   galion-sm owner-keygen      --seed-out SEED              (prints owner.pub JSON)
//!   galion-sm owner-challenge   --entry E --stack-pub P --sandbox-pub P [--ttl-s 1800]
//!                               (prints the approval body string + WebAuthn challenge)
//!   galion-sm owner-sign        --body-file F --key SEED --out E/owner/approval.json
//!   galion-sm owner-fingerprint --owner-pub F
//!
//! Exit: 0 ok, 1 refused, 2 usage/I-O.

use galion_self_modify::{owner, records, util};
use serde_json::json;
use std::path::PathBuf;

fn die(code: i32, m: &str) -> ! {
    eprintln!("galion-sm: {m}");
    std::process::exit(code)
}

fn opt(a: &[String], k: &str) -> Option<String> {
    a.iter()
        .position(|x| x == k)
        .and_then(|i| a.get(i + 1))
        .cloned()
}

fn req(a: &[String], k: &str) -> PathBuf {
    PathBuf::from(opt(a, k).unwrap_or_else(|| die(2, &format!("missing {k}"))))
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let r: Result<String, String> = match a.first().map(String::as_str).unwrap_or("") {
        "stack-keygen" => util::keygen_to(&req(&a, "--seed-out")),
        "stack-sign" => util::load_seed(&req(&a, "--key")).and_then(|k| {
            records::stack_sign(&req(&a, "--entry"), &k, util::now_secs()).map(|b| b.to_string())
        }),
        "owner-keygen" => util::keygen_to(&req(&a, "--seed-out")).map(|p| {
            owner::ed25519_owner_pub(&hermes_rs::self_modify::verifying_key(&p).expect("own key"))
        }),
        "owner-challenge" => (|| {
            let e = req(&a, "--entry");
            let stack = util::load_pub(&req(&a, "--stack-pub"))?;
            let sandbox = util::load_pub(&req(&a, "--sandbox-pub"))?;
            let ttl = opt(&a, "--ttl-s")
                .map(|x| x.parse().unwrap_or(0))
                .unwrap_or(owner::MAX_TTL_S);
            let (trec, trsha) = records::verify_test(&e, &stack, &sandbox)?;
            let (drec, drsha) = records::verify_dryrun(&e, &sandbox, &trec, &trsha)?;
            if trec["verdict"] != "pass" || drec["verdict"] != "pass" {
                return Err("refused: test and dry run must both pass".to_string());
            }
            let nonce = owner::new_nonce()?;
            let body = owner::approval_body(&owner::ApprovalFacts {
                id: trec["id"].as_str().unwrap_or(""),
                artifact_sha256: trec["artifact"]["sha256"].as_str().unwrap_or(""),
                base_commit: trec["base_commit"].as_str().unwrap_or(""),
                test_record_sha256: &trsha,
                dryrun_record_sha256: &drsha,
                nonce: &nonce,
                issued_at: util::now_secs(),
                ttl_s: ttl,
            })?;
            Ok(json!({"approval_json": body,
                "webauthn_challenge_b64url": util::b64url(&owner::webauthn_challenge(&body))})
            .to_string())
        })(),
        "owner-sign" => (|| {
            let body = String::from_utf8(util::read_file(&req(&a, "--body-file"), 64 * 1024)?)
                .map_err(|_| "body is not UTF-8".to_string())?;
            let body = body.trim_end_matches('\n');
            let v: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
            if v["version"] != owner::APPROVAL_V {
                return Err("not an owner-approval body".into());
            }
            let k = util::load_seed(&req(&a, "--key"))?;
            let env = owner::sign_ed25519(body, &k);
            let out = req(&a, "--out");
            if let Some(d) = out.parent() {
                std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
            }
            util::write_new(&out, format!("{env}\n").as_bytes(), 0o644)?;
            Ok(format!("signed {}", out.display()))
        })(),
        "owner-fingerprint" => util::read_file(&req(&a, "--owner-pub"), 64 * 1024).and_then(|b| {
            let k = owner::OwnerKey::parse(&String::from_utf8_lossy(&b))?;
            let f = k.fingerprint();
            Ok(format!(
                "{} {}\nshort: {}",
                k.scheme(),
                util::grouped(&f),
                &f[..16]
            ))
        }),
        _ => Err("usage: see source header".into()),
    };
    match r {
        Ok(s) => println!("{s}"),
        Err(e) if e.starts_with("usage") || e.starts_with("missing") => die(2, &e),
        Err(e) => die(1, &e),
    }
}
