// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The real binaries end to end (no root needed): keygen -> stack-sign ->
//! sign-test -> sign-dryrun -> owner-challenge -> owner-sign -> deploy verify,
//! then forged / wrong-key cases through the same CLIs.

mod common;
use common::*;
use std::path::Path;
use std::process::{Command, Output};

fn run(bin: &str, args: &[&str]) -> Output {
    Command::new(bin)
        .args(args)
        .env_remove("HERMES_HOME")
        .output()
        .unwrap()
}

fn ok(o: &Output) -> String {
    assert!(
        o.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn p(x: &Path) -> &str {
    x.to_str().unwrap()
}

const SM: &str = env!("CARGO_BIN_EXE_galion-sm");
const SIGN: &str = env!("CARGO_BIN_EXE_galion-sm-sign");
const DEPLOY: &str = env!("CARGO_BIN_EXE_galion-sm-deploy");

#[test]
fn full_flow_through_the_binaries() {
    if galion_self_modify::util::euid() == 0 {
        return; // the signer refuses root by design
    }
    let root = tmpdir("cli");
    let k = root.join("keys");
    std::fs::create_dir_all(&k).unwrap();
    let id = "sm-20261031-cli";
    let entry = root.join("queue").join(id);
    proposal(&entry, id);
    // keys (seeds never printed; only public halves)
    let stack_pub = ok(&run(
        SM,
        &["stack-keygen", "--seed-out", p(&k.join("stack.seed"))],
    ));
    std::fs::write(k.join("stack.pub"), stack_pub.trim()).unwrap();
    let sb_pub = ok(&run(
        SIGN,
        &["keygen", "--seed-out", p(&k.join("sandbox.seed"))],
    ));
    std::fs::write(k.join("sandbox.pub"), sb_pub.trim()).unwrap();
    let owner_pub = ok(&run(
        SM,
        &["owner-keygen", "--seed-out", p(&k.join("owner.seed"))],
    ));
    std::fs::write(k.join("owner.pub"), owner_pub.trim()).unwrap();
    assert!(!stack_pub.contains(
        &std::fs::read_to_string(k.join("stack.seed"))
            .unwrap()
            .trim()
            .to_string()
    ));

    ok(&run(
        SM,
        &[
            "stack-sign",
            "--entry",
            p(&entry),
            "--key",
            p(&k.join("stack.seed")),
        ],
    ));
    test_result(&entry, id, &[("build", 0), ("test", 0)]);
    let t = ok(&run(
        SIGN,
        &[
            "sign-test",
            "--entry",
            p(&entry),
            "--stack-pub",
            p(&k.join("stack.pub")),
            "--key",
            p(&k.join("sandbox.seed")),
        ],
    ));
    let t: serde_json::Value = serde_json::from_str(t.trim()).unwrap();
    dryrun_result(
        &entry,
        id,
        t["artifact"]["sha256"].as_str().unwrap(),
        19_070,
        0,
        true,
    );
    ok(&run(
        SIGN,
        &[
            "sign-dryrun",
            "--entry",
            p(&entry),
            "--stack-pub",
            p(&k.join("stack.pub")),
            "--key",
            p(&k.join("sandbox.seed")),
        ],
    ));

    let ch = ok(&run(
        SM,
        &[
            "owner-challenge",
            "--entry",
            p(&entry),
            "--stack-pub",
            p(&k.join("stack.pub")),
            "--sandbox-pub",
            p(&k.join("sandbox.pub")),
            "--ttl-s",
            "600",
        ],
    ));
    let ch: serde_json::Value = serde_json::from_str(ch.trim()).unwrap();
    let body_file = root.join("body.json");
    std::fs::write(&body_file, ch["approval_json"].as_str().unwrap()).unwrap();
    assert_eq!(ch["webauthn_challenge_b64url"].as_str().unwrap().len(), 43);
    ok(&run(
        SM,
        &[
            "owner-sign",
            "--body-file",
            p(&body_file),
            "--key",
            p(&k.join("owner.seed")),
            "--out",
            p(&entry.join("owner/approval.json")),
        ],
    ));
    let fp = ok(&run(
        SM,
        &["owner-fingerprint", "--owner-pub", p(&k.join("owner.pub"))],
    ));
    assert!(fp.starts_with("ed25519 ") && fp.contains("short: "), "{fp}");

    let ledger = root.join("used-nonces");
    let args = |owner: &Path| -> Vec<String> {
        [
            "verify",
            "--entry",
            p(&entry),
            "--stack-pub",
            p(&k.join("stack.pub")),
            "--sandbox-pub",
            p(&k.join("sandbox.pub")),
            "--owner-pub",
            p(owner),
            "--ledger",
            p(&ledger),
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    };
    let v = ok(&run(
        DEPLOY,
        &args(&k.join("owner.pub"))
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    ));
    assert!(v.starts_with("OK: all 4 signatures verify"), "{v}");
    assert!(!ledger.exists(), "verify burns nothing");

    // wrong owner key -> exit 1
    std::fs::write(
        k.join("other.pub"),
        hermes_rs::self_modify::hex(key(42).verifying_key().as_bytes()),
    )
    .unwrap();
    let o = run(
        DEPLOY,
        &args(&k.join("other.pub"))
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("bad signature"));

    // deploy as non-root refused before anything happens
    let mut d = args(&k.join("owner.pub"));
    d[0] = "deploy".into();
    let o = run(DEPLOY, &d.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("runs as root"));

    // Hermes runtime env is refused outright
    let o = Command::new(DEPLOY)
        .args(&d)
        .env("HERMES_RS_PORT", "8642")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("Hermes runtime"));

    // The signer will not sign twice, nor with a world-readable key.
    let o = run(
        SIGN,
        &[
            "sign-test",
            "--entry",
            p(&entry),
            "--stack-pub",
            p(&k.join("stack.pub")),
            "--key",
            p(&k.join("sandbox.seed")),
        ],
    );
    assert_eq!(o.status.code(), Some(1));
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        k.join("sandbox.seed"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let o = run(
        SIGN,
        &[
            "sign-test",
            "--entry",
            p(&entry),
            "--stack-pub",
            p(&k.join("stack.pub")),
            "--key",
            p(&k.join("sandbox.seed")),
        ],
    );
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("chmod 600"));
}
