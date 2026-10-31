// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Shared fixture: a queue entry signed end to end with throwaway keys.
#![allow(dead_code)]

use ed25519_dalek::SigningKey;
use galion_self_modify::deploy::{DeployCfg, Keys, System};
use galion_self_modify::owner::{self, ApprovalFacts, OwnerKey};
use galion_self_modify::records::{self, Entry};
use galion_self_modify::util::{b64url, write_atomic};
use hermes_rs::self_modify::{sha256_hex, PROPOSAL_V};
use p256::ecdsa::signature::Signer as _;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

pub const BASE: &str = "6da98fdcdc81330900fc1b8fe71b7d3f79704e59";
pub const NOW: i64 = 1_793_437_200; // fixed clock for tests
pub const RP: &str = "galion.app";
pub const ORIGIN: &str = "https://galion.app";

static N: AtomicU32 = AtomicU32::new(0);

pub fn tmpdir(tag: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "gsm-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn key(b: u8) -> SigningKey {
    SigningKey::from_bytes(&[b; 32])
}

pub fn p256_key(b: u8) -> p256::ecdsa::SigningKey {
    p256::ecdsa::SigningKey::from_slice(&[b; 32]).unwrap()
}

pub fn p256_owner_pub(sk: &p256::ecdsa::SigningKey, cred: &[u8]) -> String {
    let pt = sk.verifying_key().to_encoded_point(false);
    json!({"version": owner::OWNER_KEY_V, "scheme": "webauthn-es256", "rp_id": RP,
        "origins": [ORIGIN, "https://one.galion.app"],
        "public_key_sec1_hex": hermes_rs::self_modify::hex(pt.as_bytes()),
        "credential_id": b64url(cred)})
    .to_string()
}

pub struct Fix {
    pub root: PathBuf,
    pub entry: PathBuf,
    pub stack: SigningKey,
    pub sandbox: SigningKey,
    pub owner: SigningKey,
}

impl Fix {
    pub fn e(&self) -> Entry {
        Entry::new(&self.entry)
    }
    pub fn keys(&self) -> Keys {
        Keys {
            stack: self.stack.verifying_key(),
            sandbox: self.sandbox.verifying_key(),
            owner: OwnerKey::Ed25519(self.owner.verifying_key()),
        }
    }
    pub fn ledger(&self) -> PathBuf {
        self.root.join("state/used-nonces")
    }
    pub fn cfg(&self) -> DeployCfg {
        DeployCfg {
            bin_dir: self.root.join("bin"),
            dropin_dir: self.root.join("hermes-rs.service.d"),
            dropin_name: "99j-self-modify-approved.conf".into(),
            unit: "hermes-rs.service".into(),
            ledger: self.ledger(),
            backup_dir: self.root.join("state/backups"),
            extra_lines: vec![],
        }
    }
    pub fn records(&self) -> (Value, String, Value, String) {
        let (t, ts) = records::verify_test(
            &self.entry,
            &self.stack.verifying_key(),
            &self.sandbox.verifying_key(),
        )
        .unwrap();
        let (d, ds) =
            records::verify_dryrun(&self.entry, &self.sandbox.verifying_key(), &t, &ts).unwrap();
        (t, ts, d, ds)
    }
    /// Approval body for this entry with overrides.
    pub fn body(&self, nonce: &str, issued: i64, ttl: i64) -> String {
        let (t, ts, _, ds) = self.records();
        owner::approval_body(&ApprovalFacts {
            id: t["id"].as_str().unwrap(),
            artifact_sha256: t["artifact"]["sha256"].as_str().unwrap(),
            base_commit: BASE,
            test_record_sha256: &ts,
            dryrun_record_sha256: &ds,
            nonce,
            issued_at: issued,
            ttl_s: ttl,
        })
        .unwrap()
    }
    pub fn put_approval(&self, env: &Value) {
        let p = self.e().owner_approval();
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        write_atomic(&p, format!("{env}\n").as_bytes(), 0o644).unwrap();
    }
    pub fn approve_ed25519(&self, nonce: &str) {
        let b = self.body(nonce, NOW - 60, 1800);
        self.put_approval(&owner::sign_ed25519(&b, &self.owner));
    }
}

pub fn write(p: &Path, b: &[u8]) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b).unwrap();
}

/// proposal.json + change.patch as hermes-rs writes them.
pub fn proposal(entry: &Path, id: &str) {
    let patch = b"--- a/docs/x.md\n+++ b/docs/x.md\n@@ -1 +1 @@\n-a\n+b\n";
    write(&entry.join("change.patch"), patch);
    let p = json!({"version": PROPOSAL_V, "id": id, "base_commit": BASE,
        "proposed_by": "hermes", "patch_sha256": sha256_hex(patch), "files": ["docs/x.md"]});
    write(&entry.join("proposal.json"), format!("{p}\n").as_bytes());
}

pub fn test_result(entry: &Path, id: &str, exits: &[(&str, i64)]) {
    let e = Entry::new(entry);
    write(&e.artifact(), b"\x7fELF fake hermes-gateway build");
    let stages: Vec<Value> = exits
        .iter()
        .map(|(n, x)| {
            write(
                &e.test_dir().join(format!("logs/{n}.log")),
                format!("{n} exit {x}\n").as_bytes(),
            );
            json!({"name": n, "exit": x, "log": format!("logs/{n}.log")})
        })
        .collect();
    let r = json!({"version": records::TEST_RESULT_V, "id": id, "base_commit": BASE,
        "stages": stages, "artifact": "artifact"});
    write(&e.test_result(), r.to_string().as_bytes());
}

pub fn dryrun_result(entry: &Path, id: &str, art: &str, port: i64, calls: i64, ok: bool) {
    let e = Entry::new(entry);
    write(
        &e.dryrun_dir().join("logs/health.log"),
        b"GET /health 200\n",
    );
    let r = json!({"version": records::DRYRUN_RESULT_V, "id": id, "artifact_sha256": art,
        "port": port, "model_calls_real": calls,
        "checks": [{"name": "health", "ok": ok, "detail": "200", "log": "logs/health.log"},
                   {"name": "chat-fake-model", "ok": true, "detail": "fake model answered"}]});
    write(&e.dryrun_result(), r.to_string().as_bytes());
}

/// Entry signed through stack, test and dry run (no owner approval yet).
pub fn signed(tag: &str) -> Fix {
    let root = tmpdir(tag);
    let id = format!("sm-20261031-{tag}");
    let entry = root.join("queue").join(&id);
    proposal(&entry, &id);
    let f = Fix {
        root,
        entry,
        stack: key(1),
        sandbox: key(2),
        owner: key(3),
    };
    records::stack_sign(&f.entry, &f.stack, NOW - 600).unwrap();
    test_result(&f.entry, &id, &[("build", 0), ("test", 0), ("clippy", 0)]);
    let t = records::sign_test(&f.entry, &f.stack.verifying_key(), &f.sandbox, NOW - 500).unwrap();
    assert_eq!(t["verdict"], "pass", "{t}");
    let art = t["artifact"]["sha256"].as_str().unwrap().to_string();
    dryrun_result(&f.entry, &id, &art, 19_070, 0, true);
    let d =
        records::sign_dryrun(&f.entry, &f.stack.verifying_key(), &f.sandbox, NOW - 400).unwrap();
    assert_eq!(d["verdict"], "pass", "{d}");
    f
}

/// A WebAuthn assertion as a browser + platform authenticator would make.
pub struct Assertion<'a> {
    pub sk: &'a p256::ecdsa::SigningKey,
    pub body: &'a str,
    pub rp: &'a str,
    pub origin: &'a str,
    pub typ: &'a str,
    pub flags: u8,
    pub cred: &'a [u8],
    pub challenge_body: Option<&'a str>,
}

impl<'a> Assertion<'a> {
    pub fn new(sk: &'a p256::ecdsa::SigningKey, body: &'a str) -> Self {
        Assertion {
            sk,
            body,
            rp: RP,
            origin: ORIGIN,
            typ: "webauthn.get",
            flags: 0x05,
            cred: b"cred-1",
            challenge_body: None,
        }
    }
    pub fn env(&self) -> Value {
        let mut auth = Sha256::digest(self.rp.as_bytes()).to_vec();
        auth.push(self.flags);
        auth.extend_from_slice(&7u32.to_be_bytes());
        let chal = owner::webauthn_challenge(self.challenge_body.unwrap_or(self.body));
        let cdj = json!({"type": self.typ, "challenge": b64url(&chal), "origin": self.origin,
            "crossOrigin": false})
        .to_string();
        let mut msg = auth.clone();
        msg.extend_from_slice(&Sha256::digest(cdj.as_bytes()));
        let sig: p256::ecdsa::Signature = self.sk.sign(&msg);
        json!({"scheme": "webauthn-es256", "approval_json": self.body,
            "credential_id": b64url(self.cred), "authenticator_data": b64url(&auth),
            "client_data_json": b64url(cdj.as_bytes()),
            "signature": b64url(sig.to_der().as_bytes()), "key_id": "x"})
    }
}

/// Mock systemctl / health / ntfy.
#[derive(Default)]
pub struct Mock {
    pub calls: Vec<String>,
    pub alerts: Vec<(String, String, String)>,
    /// health results, in order; empty = healthy
    pub health: Vec<Result<String, String>>,
}

impl System for Mock {
    fn systemctl(&mut self, args: &[&str]) -> Result<(), String> {
        self.calls.push(args.join(" "));
        Ok(())
    }
    fn health(&mut self) -> Result<String, String> {
        self.calls.push("health".into());
        if self.health.is_empty() {
            Ok("/health 200".into())
        } else {
            self.health.remove(0)
        }
    }
    fn alert(&mut self, title: &str, prio: &str, _tags: &str, body: &str) -> Result<(), String> {
        self.alerts.push((title.into(), prio.into(), body.into()));
        Ok(())
    }
}

pub const NONCE: &str = "0123456789abcdef0123456789abcdef";
