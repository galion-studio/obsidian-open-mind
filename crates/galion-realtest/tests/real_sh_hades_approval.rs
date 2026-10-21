// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Rust port of `scripts/real/hades-approval.sh` (slot 02; deleted on
//! swarm/realscript-port-1). Real-life test for hades-gate admin approvals.
//!
//! Builds the release binary (x86_64 musl by default, `GALION_REAL_HADES_TARGET`
//! to change), makes a THROWAWAY admin key in a temp dir (never a real secret),
//! starts `hades-gate serve` on 127.0.0.1, approves a dry-run action once, and
//! shows that replay, tampering, expiry, wrong scope, the admin-safety rule,
//! the disable switch and `/v1/stop` all behave. Tears down. Token strings
//! and key material are never printed.
//!
//! Ports: block 19020 (`GALION_REAL_PORT_BASE`), offset 0.
//! Run: `GALION_REAL=1 cargo test -p galion-realtest --test real_sh_hades_approval -- --nocapture`

use galion_realtest::http;
use galion_realtest::script::*;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

fn post(addr: &str, tok: Option<&Path>, action: &str, target: &str, plan: &Path) -> String {
    let token = tok.map(read).unwrap_or_default();
    let body = serde_json::json!({
        "action": action, "target": target, "plan": read(plan), "token": token
    })
    .to_string();
    match http::post_json(addr, "/v1/approve", &[], &body) {
        Ok(r) => format!("{} http={}", r.text(), r.status),
        Err(e) => format!("error {e}"),
    }
}

#[test]
fn real_sh_hades_approval() {
    if !enabled("hades-approval") {
        return;
    }
    let port = port_base(19020);
    let addr = format!("127.0.0.1:{port}");
    let triple = std::env::var("GALION_REAL_HADES_TARGET")
        .unwrap_or_else(|_| "x86_64-unknown-linux-musl".into());
    build(&["-p", "hades-gate", "--target", &triple]);
    let bin = need_bin(&target_dir().join(&triple).join("release/hades-gate"));
    require_free(&[port]);

    let tmp = TempDir(scratch("hades-real"));
    let t = |n: &str| tmp.0.join(n);
    let hg = |args: &[&str]| -> std::process::Output {
        let mut c = Command::new(&bin);
        c.args(args);
        run(c)
    };
    let mut ck = Checks::new();

    say(&format!("binary: {}", bin.display()));
    let o = hg(&[
        "keygen",
        "--seed",
        t("admin.seed").to_str().unwrap(),
        "--pub",
        t("admin.pub").to_str().unwrap(),
    ]);
    assert!(o.status.success(), "keygen: {}", err(&o));
    fs::write(
        t("hades.toml"),
        format!(
            "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\nmax_ttl_secs = 900\n",
            t("admin.pub").display(),
            t("ledger.jsonl").display(),
            t("DISABLED").display()
        ),
    )
    .unwrap();
    fs::write(
        t("plan.txt"),
        "restart galion-tuner\nverify /health on 127.0.0.1:8420",
    )
    .unwrap();
    let cfg = t("hades.toml");
    let cfg = cfg.to_str().unwrap();
    let plan = t("plan.txt");

    let mut procs = Procs::new();
    let mut c = Command::new(&bin);
    c.args(["serve", "--config", cfg, "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(fs::File::create(t("serve.log")).unwrap());
    let pid = procs.spawn("hades-gate serve", c);
    assert!(
        wait_port(port, Duration::from_secs(5)),
        "serve did not come up: {}",
        tail(&t("serve.log"), 20)
    );

    let mint = |name: &str, seed: &str, action: &str, target: &str, plan: &Path, ttl: &str| {
        let o = hg(&[
            "mint",
            "--seed",
            t(seed).to_str().unwrap(),
            "--action",
            action,
            "--target",
            target,
            "--plan-file",
            plan.to_str().unwrap(),
            "--ttl",
            ttl,
            "--out",
            t(name).to_str().unwrap(),
        ]);
        assert!(o.status.success(), "mint {name}: {}", err(&o));
        t(name)
    };

    say("GET /health");
    let h = http::get(&addr, "/health")
        .map(|r| r.text())
        .unwrap_or_default();
    println!("{h}");
    ck.has("health", "\"mode\":\"dry-run\"", &h);

    say("1. no token -> DryRun (default)");
    let o = post(&addr, None, "heal-apply", "galion-tuner", &plan);
    println!("{o}");
    ck.has("dry-run", "\"decision\":\"DryRun\"", &o);

    say("2. old public token approved-test -> refused");
    fs::write(t("old.tok"), "approved-test").unwrap();
    let o = post(
        &addr,
        Some(&t("old.tok")),
        "heal-apply",
        "galion-tuner",
        &plan,
    );
    println!("{o}");
    ck.has("old-token", "\"code\":\"malformed\"", &o);

    say("3. mint a token (throwaway key) and approve the dry-run action once");
    let good = mint(
        "good.tok",
        "admin.seed",
        "heal-apply",
        "galion-tuner",
        &plan,
        "120",
    );
    let o = post(&addr, Some(&good), "heal-apply", "galion-tuner", &plan);
    println!("{o}");
    ck.has("approved", "\"decision\":\"WouldApply\"", &o);
    ck.has("not-executed", "\"executed\":false", &o);

    say("4. replay the same token -> refused");
    let o = post(&addr, Some(&good), "heal-apply", "galion-tuner", &plan);
    println!("{o}");
    ck.has("replay", "\"code\":\"replayed\"", &o);

    say("5. tamper: one payload hex digit changed (fresh token) -> refused");
    let t2 = mint(
        "t2.tok",
        "admin.seed",
        "heal-apply",
        "galion-tuner",
        &plan,
        "120",
    );
    {
        let tok = read(&t2);
        let rest = tok.strip_prefix("hades1.").expect("hades1. token");
        let (p, s) = rest.split_once('.').expect("payload.sig");
        // The last payload digit is the last digit of exp: '3' <-> '2'.
        let new = if p.ends_with('3') { '2' } else { '3' };
        fs::write(
            t("tampered.tok"),
            format!("hades1.{}{new}.{s}", &p[..p.len() - 1]),
        )
        .unwrap();
    }
    let o = post(
        &addr,
        Some(&t("tampered.tok")),
        "heal-apply",
        "galion-tuner",
        &plan,
    );
    println!("{o}");
    ck.has("tamper", "\"code\":\"forged\"", &o);

    say("6. forged: token signed by a different throwaway key -> refused");
    let o = hg(&[
        "keygen",
        "--seed",
        t("mallory.seed").to_str().unwrap(),
        "--pub",
        t("mallory.pub").to_str().unwrap(),
    ]);
    assert!(o.status.success());
    let mal = mint(
        "mallory.tok",
        "mallory.seed",
        "heal-apply",
        "galion-tuner",
        &plan,
        "120",
    );
    let o = post(&addr, Some(&mal), "heal-apply", "galion-tuner", &plan);
    println!("{o}");
    ck.has("forged", "\"code\":\"forged\"", &o);

    say("7. wrong scope: valid token used for another target -> refused");
    let o = post(&addr, Some(&t2), "heal-apply", "galion-one", &plan);
    println!("{o}");
    ck.has("wrong-scope", "\"code\":\"wrong-scope\"", &o);

    say("8. expiry: token with ttl 2 s, used after 3 s -> refused");
    let short = mint(
        "short.tok",
        "admin.seed",
        "heal-apply",
        "galion-tuner",
        &plan,
        "2",
    );
    std::thread::sleep(Duration::from_secs(3));
    let o = post(&addr, Some(&short), "heal-apply", "galion-tuner", &plan);
    println!("{o}");
    ck.has("expired", "\"code\":\"expired\"", &o);

    say("9. admin-safety: validly signed token for 'rm -rf' -> refused");
    fs::write(t("evil.txt"), "rm -rf /var/lib/galion").unwrap();
    let evil = mint(
        "evil.tok",
        "admin.seed",
        "heal-apply",
        "galion-tuner",
        &t("evil.txt"),
        "120",
    );
    let o = post(
        &addr,
        Some(&evil),
        "heal-apply",
        "galion-tuner",
        &t("evil.txt"),
    );
    println!("{o}");
    ck.has("safety-destructive", "\"code\":\"admin-safety\"", &o);
    fs::write(t("lock.txt"), "usermod -L slavik").unwrap();
    let lock = mint(
        "lock.tok",
        "admin.seed",
        "heal-apply",
        "galion-tuner",
        &t("lock.txt"),
        "120",
    );
    let o = post(
        &addr,
        Some(&lock),
        "heal-apply",
        "galion-tuner",
        &t("lock.txt"),
    );
    println!("{o}");
    ck.has("safety-admin", "\"code\":\"admin-safety\"", &o);

    say("10. kill with a valid signature -> refused");
    let kill = mint(
        "kill.tok",
        "admin.seed",
        "kill",
        "galion-tuner",
        &plan,
        "120",
    );
    let o = post(&addr, Some(&kill), "kill", "galion-tuner", &plan);
    println!("{o}");
    ck.has("kill", "\"code\":\"action\"", &o);

    say("11. admin disables HADES (no token needed) -> everything refused");
    assert!(hg(&["disable", "--config", cfg]).status.success());
    let t3 = mint(
        "t3.tok",
        "admin.seed",
        "heal-apply",
        "galion-tuner",
        &plan,
        "120",
    );
    let o = post(&addr, Some(&t3), "heal-apply", "galion-tuner", &plan);
    println!("{o}");
    ck.has("disabled", "\"code\":\"disabled\"", &o);
    assert!(hg(&["enable", "--config", cfg]).status.success());
    let o = post(&addr, Some(&t3), "heal-apply", "galion-tuner", &plan);
    println!("{o}");
    ck.has("re-enabled", "\"decision\":\"WouldApply\"", &o);

    say("12. CLI check path: replay of t3 via the CLI -> exit 2");
    let o = hg(&[
        "check",
        "--config",
        cfg,
        "--action",
        "heal-apply",
        "--target",
        "galion-tuner",
        "--plan-file",
        plan.to_str().unwrap(),
        "--token-file",
        t3.to_str().unwrap(),
    ]);
    println!("{} exit={}", out(&o).trim(), code(&o));
    ck.has("cli-replay", "\"code\":\"replayed\"", &out(&o));
    ck.eq("cli-exit-2", "2", code(&o).to_string());

    say("13. ledger (append-only, fsynced): event counts; no token strings stored");
    let ledger = read(&t("ledger.jsonl"));
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for l in ledger.lines() {
        *counts.entry(jt(&json(l), "event")).or_default() += 1;
    }
    for (k, v) in &counts {
        println!("  {v:>4} {k}");
    }
    ck.ok("no-token-in-ledger", !ledger.contains(read(&t3).trim()), "");
    println!(
        "  ledger mode {}  seed mode {}",
        mode(&t("ledger.jsonl")),
        mode(&t("admin.seed"))
    );

    say("14. admin stop: POST /v1/stop");
    let o = http::request(&addr, "POST", "/v1/stop", &[], b"", Duration::from_secs(5));
    println!("{}", o.map(|r| r.text()).unwrap_or_else(|e| e));
    let st = procs
        .get("hades-gate serve")
        .and_then(|c| wait_exit(c, Duration::from_secs(3)));
    ck.ok("stopped", st.is_some() && !alive(pid), &format!("{st:?}"));
    ck.ok(&format!("port {port} closed"), !port_open(port), "");

    drop(procs);
    ck.finish("hades-approval");
}
