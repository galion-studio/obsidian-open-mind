// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Rust port of `scripts/real/gaia-hades-tokens.sh` (slot 23) and of its
//! wrapper `scripts/real/gaia-live.sh` (both deleted on
//! swarm/realscript-port-1). REAL-life test of `gaia heal-tick` with
//! hades-gate approvals, ON THE BOX ONLY (no VPS, no systemd, no live ports):
//!
//!   probe -> nerve -> GAIA -> DEMETER -> HADES plan -> hades-gate check_gaia
//!   (disable switch, admin-safety rule, Ed25519 signature, expiry, scope
//!   heal-apply + service + plan SHA-256, single use via the fsynced replay
//!   ledger) -> restart (max 3 per hour) -> nerve event.
//!
//! The admin key pair is made with `hades-gate keygen` in a 0700 dir; tokens
//! are minted with `hades-gate mint` over the exact plan bytes that
//! `gaia heal-plan --out` writes. No seed or token is ever printed: outputs
//! show only the admin fingerprint, nonces and SHA-256 prefixes, and a final
//! scan checks that no token string reached any output, log or the nerve.
//!
//! Ports: block 19230 (`GALION_REAL_PORT_BASE`): +0 openmind-nerve,
//! +1 gaia-dummy, +2 "other port" plan; the heal allow-range is +0..+9.
//! Run: `GALION_REAL=1 cargo test -p galion-realtest --test real_sh_gaia_hades_tokens -- --nocapture`

use galion_realtest::http;
use galion_realtest::script::*;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

struct T {
    out: PathBuf,
    gaia: PathBuf,
    dummy: PathBuf,
    hg: PathBuf,
    nerve: String,
    dport: u16,
    allow: String,
}

impl T {
    fn p(&self, rel: &str) -> PathBuf {
        self.out.join(rel)
    }
    fn s(&self, rel: &str) -> String {
        self.p(rel).to_string_lossy().into_owned()
    }
    fn dummy_pid(&self) -> Option<u32> {
        http::get(&format!("127.0.0.1:{}", self.dport), "/health")
            .ok()
            .and_then(|r| r.json().ok())
            .and_then(|v| v["pid"].as_u64())
            .map(|p| p as u32)
    }
    fn consumed(&self) -> usize {
        read(&self.p("gate/ledger.jsonl"))
            .matches("\"event\":\"consumed\"")
            .count()
    }
    fn hades(&self, args: &[&str]) -> std::process::Output {
        let mut c = Command::new(&self.hg);
        c.args(args);
        run(c)
    }
    /// `gaia heal-plan` for a port; returns the JSON.
    fn plan(&self, port: u16, file: &str, extra: &[&str]) -> Value {
        let mut c = Command::new(&self.gaia);
        c.args([
            "heal-plan",
            "--compact",
            "--service",
            "gaia-dummy",
            "--port",
            &port.to_string(),
        ])
        .args(["--allow-ports", &self.allow, "--out", &self.s(file), "--"])
        .arg(&self.dummy)
        .args(["--port", &port.to_string()])
        .args(extra);
        let o = run(c);
        assert!(o.status.success(), "heal-plan: {}", err(&o));
        let v = json(&out(&o));
        let r = &v["result"];
        println!(
            "{}",
            serde_json::json!({"target": r["target"], "plan_sha256": r["plan_sha256"], "admin_safety": r["admin_safety"]})
        );
        v
    }
    /// mint NAME SEED ACTION TARGET PLAN [extra]; prints only non-secret claims.
    fn mint(
        &self,
        name: &str,
        seed: &str,
        action: &str,
        target: &str,
        plan: &str,
        extra: &[&str],
    ) -> Value {
        let (seed, plan, outf) = (self.s(seed), self.s(plan), self.s(&format!("tok/{name}")));
        let mut args = vec![
            "mint",
            "--seed",
            &seed,
            "--action",
            action,
            "--target",
            target,
            "--plan-file",
            &plan,
            "--out",
            &outf,
        ];
        args.extend_from_slice(extra);
        let o = self.hades(&args);
        assert!(o.status.success(), "mint {name}: {}", err(&o));
        let v = json(&out(&o));
        let line = serde_json::json!({
            "token": name, "action": v["action"], "target": v["target"],
            "plan_sha256": jt(&v, "plan_sha256").chars().take(16).collect::<String>(),
            "nonce": v["nonce"], "iat": v["iat"], "exp": v["exp"]
        });
        println!("{line}");
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.p("minted.jsonl"))
            .unwrap();
        use std::io::Write;
        writeln!(f, "{line}").unwrap();
        v
    }
    /// One `gaia heal-tick`; prints the row like the script and returns
    /// (exit, outcome, approval code, consumed count, detail).
    fn tick(
        &self,
        label: &str,
        tok: Option<&str>,
        port: u16,
        unsafe_start: bool,
    ) -> (i32, String, String, usize, String) {
        let mut c = Command::new(&self.gaia);
        c.args([
            "heal-tick",
            "--compact",
            "--service",
            "gaia-dummy",
            "--port",
            &port.to_string(),
        ])
        .args(["--allow-ports", &self.allow, "--nerve", &self.nerve])
        .args([
            "--state",
            &self.s("state"),
            "--hades-config",
            &self.s("gate/hades.toml"),
        ]);
        if let Some(t) = tok {
            c.args(["--token-file", &self.s(&format!("tok/{t}"))]);
        }
        c.arg("--")
            .arg(&self.dummy)
            .args(["--port", &port.to_string()]);
        if unsafe_start {
            c.arg("--note=secret");
        }
        // The restarted dummy must outlive this call: no pipes to it.
        c.stdin(Stdio::null())
            .stdout(fs::File::create(self.p(&format!("tick-{label}.json"))).unwrap())
            .stderr(Stdio::null());
        let st = c.status().expect("run gaia heal-tick");
        let rc = st.code().unwrap_or(-1);
        let v = json(&read(&self.p(&format!("tick-{label}.json"))));
        let outcome = jt(&v, "result.outcome");
        let approval = v["result"]["steps"]
            .as_array()
            .and_then(|s| s.iter().find(|s| s["step"] == "approval"))
            .map(|s| jt(s, "detail.code"))
            .filter(|c| c != "null")
            .unwrap_or_else(|| "-".into());
        let detail = jt(&v, "result.detail");
        let consumed = self.consumed();
        println!("{label:<16} exit={rc} outcome={outcome:<9} code={approval:<13} consumed={consumed} detail={detail}");
        (rc, outcome, approval, consumed, detail)
    }
}

fn row(
    ck: &mut Checks,
    label: &str,
    got: (i32, String, String, usize, String),
    want: (i32, &str, &str, usize),
) {
    ck.eq(
        &format!("{label}: exit/outcome/code/consumed"),
        format!("{} {} {} {}", want.0, want.1, want.2, want.3),
        format!("{} {} {} {}", got.0, got.1, got.2, got.3),
    );
}

fn files_under(p: &Path, acc: &mut Vec<PathBuf>) {
    if p.is_dir() {
        for e in fs::read_dir(p).into_iter().flatten().flatten() {
            files_under(&e.path(), acc);
        }
    } else if p.is_file() {
        acc.push(p.to_path_buf());
    }
}

#[test]
fn real_sh_gaia_hades_tokens() {
    if !enabled("gaia-hades-tokens") {
        return;
    }
    let base = port_base(19230);
    let (nport, dport, oport) = (base, base + 1, base + 2);
    require_free(&[nport, dport, oport]);
    build(&["-p", "gaia", "-p", "openmind-nerve", "-p", "hades-gate"]);
    let dir = TempDir(scratch("gaia-hades-tokens-real"));
    for d in ["state", "gate", "admin", "tok", "plan"] {
        fs::create_dir_all(dir.0.join(d)).unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.0.join("admin"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(dir.0.join("tok"), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let t = T {
        out: dir.0.clone(),
        gaia: need_bin(&release_bin("gaia")),
        dummy: need_bin(&release_bin("gaia-dummy")),
        hg: need_bin(&release_bin("hades-gate")),
        nerve: format!("127.0.0.1:{nport}"),
        dport,
        allow: format!("{}-{}", base, base + 9),
    };
    let nerve_bin = need_bin(&release_bin("openmind-nerve"));
    let mut ck = Checks::new();
    let mut procs = Procs::new();

    say("admin side: hades-gate keygen (seed in admin/, mode 600, never printed); a second 'attacker' key");
    assert!(t
        .hades(&[
            "keygen",
            "--seed",
            &t.s("admin/admin.seed"),
            "--pub",
            &t.s("gate/admin.pub")
        ])
        .status
        .success());
    assert!(t
        .hades(&[
            "keygen",
            "--seed",
            &t.s("admin/evil.seed"),
            "--pub",
            &t.s("admin/evil.pub")
        ])
        .status
        .success());
    ck.eq("admin seed mode", "600", mode(&t.p("admin/admin.seed")));
    fs::write(
        t.p("gate/hades.toml"),
        format!(
            "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\nmax_ttl_secs = 900\n",
            t.s("gate/admin.pub"),
            t.s("gate/ledger.jsonl"),
            t.s("gate/DISABLED")
        ),
    )
    .unwrap();
    println!(
        "gate status: {}",
        out(&t.hades(&["status", "--config", &t.s("gate/hades.toml")])).trim()
    );

    say("gaia heal-plan: the exact plan bytes the admin signs");
    let good = t.plan(dport, "plan/good.txt", &[]);
    println!("{}", read(&t.p("plan/good.txt")));
    let ph = out(&t.hades(&["plan-hash", "--plan-file", &t.s("plan/good.txt")]));
    println!("hades-gate plan-hash: {}", ph.trim());
    ck.has(
        "gaia plan_sha256 == hades-gate plan-hash",
        &jt(&good, "result.plan_sha256"),
        &ph,
    );
    t.plan(oport, "plan/other-port.txt", &[]);
    t.plan(dport, "plan/unsafe.txt", &["--note=secret"]);

    say("admin side: hades-gate mint (tokens written mode 600 to tok/, never printed)");
    let now = std::time::SystemTime::now() // repo-check: allow (unix secs for mint --iat)
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let iat = (now - 400).to_string();
    t.mint(
        "forged",
        "admin/evil.seed",
        "heal-apply",
        "gaia-dummy",
        "plan/good.txt",
        &[],
    );
    t.mint(
        "wrong-svc",
        "admin/admin.seed",
        "heal-apply",
        "gaia-other",
        "plan/good.txt",
        &[],
    );
    t.mint(
        "wrong-plan",
        "admin/admin.seed",
        "heal-apply",
        "gaia-dummy",
        "plan/other-port.txt",
        &[],
    );
    t.mint(
        "expired",
        "admin/admin.seed",
        "heal-apply",
        "gaia-dummy",
        "plan/good.txt",
        &["--iat", &iat, "--ttl", "300"],
    );
    t.mint(
        "unsafe",
        "admin/admin.seed",
        "heal-apply",
        "gaia-dummy",
        "plan/unsafe.txt",
        &[],
    );
    let mut nonces = Vec::new();
    for n in 1..=4 {
        let v = t.mint(
            &format!("valid-{n}"),
            "admin/admin.seed",
            "heal-apply",
            "gaia-dummy",
            "plan/good.txt",
            &[],
        );
        nonces.push(jt(&v, "nonce"));
    }
    t.mint(
        "wrong-action",
        "admin/admin.seed",
        "rollback",
        "gaia-dummy",
        "plan/good.txt",
        &[],
    );
    {
        // tampered: valid-4's signature with the last hex digit flipped
        let tok = read(&t.p("tok/valid-4"));
        let tok = tok.trim_end();
        let flip = if tok.ends_with('0') { '1' } else { '0' };
        fs::write(
            t.p("tok/tampered"),
            format!("{}{flip}", &tok[..tok.len() - 1]),
        )
        .unwrap();
    }
    let mut modes: Vec<String> = fs::read_dir(t.p("tok"))
        .unwrap()
        .flatten()
        .map(|e| format!("{} {}", mode(&e.path()), e.file_name().to_string_lossy()))
        .collect();
    modes.sort();
    println!("{}", modes.join("  "));

    say(&format!(
        "start the real openmind-nerve on {} and gaia-dummy on 127.0.0.1:{dport}",
        t.nerve
    ));
    let mut c = Command::new(&nerve_bin);
    c.args(["serve", "--listen", &t.nerve, "--log", &t.s("nerve.jsonl")])
        .stdout(Stdio::null())
        .stderr(fs::File::create(t.p("nerve.stderr")).unwrap());
    procs.spawn("openmind-nerve", c);
    let mut c = Command::new(&t.dummy);
    c.args(["--port", &dport.to_string()])
        .stdout(Stdio::null())
        .stderr(fs::File::create(t.p("state/gaia-dummy.log")).unwrap());
    let first = procs.spawn("gaia-dummy (first)", c);
    assert!(
        wait_port(nport, Duration::from_secs(5)) && wait_port(dport, Duration::from_secs(5)),
        "nerve/dummy did not come up"
    );
    println!(
        "nerve GET /health: {}",
        http::get(&t.nerve, "/health")
            .map(|r| r.text())
            .unwrap_or_default()
    );
    println!(
        "dummy GET /health: {}",
        http::get(&format!("127.0.0.1:{dport}"), "/health")
            .map(|r| r.text())
            .unwrap_or_default()
    );

    say("1. dummy up: healthy, no token needed or touched");
    row(
        &mut ck,
        "01-healthy",
        t.tick("01-healthy", None, dport, false),
        (0, "healthy", "-", 0),
    );
    say("2. scope guard: live stack port 8420 refused before any probe or token");
    let r = t.tick("02-live-port", Some("valid-1"), 8420, false);
    ck.has(
        "02-live-port detail names the allowed range",
        &t.allow,
        &r.4,
    );
    row(&mut ck, "02-live-port", r, (4, "refused", "-", 0));

    say(&format!("3. kill the dummy (pid {first}, our own child)"));
    procs.stop("gaia-dummy (first)");
    ck.ok(
        "dummy down",
        http::get(&format!("127.0.0.1:{dport}"), "/health").is_err(),
        "connection refused",
    );

    say("4. refusals (service down, nothing restarted, no token consumed)");
    for (label, tok, code) in [
        ("04-no-token", None, "no-token"),
        ("05-forged", Some("forged"), "forged"),
        ("06-tampered", Some("tampered"), "forged"),
        ("07-wrong-service", Some("wrong-svc"), "wrong-scope"),
        ("08-wrong-action", Some("wrong-action"), "wrong-scope"),
        ("09-wrong-plan", Some("wrong-plan"), "wrong-scope"),
        ("10-expired", Some("expired"), "expired"),
    ] {
        row(
            &mut ck,
            label,
            t.tick(label, tok, dport, false),
            (4, "refused", code, 0),
        );
    }
    say("5. admin safety first: a valid signature over an unsafe plan is still refused");
    row(
        &mut ck,
        "11-admin-safety",
        t.tick("11-admin-safety", Some("unsafe"), dport, true),
        (4, "refused", "admin-safety", 0),
    );

    say("6. hades-gate disable (no token, no key) beats a valid token; enable");
    assert!(t
        .hades(&["disable", "--config", &t.s("gate/hades.toml")])
        .status
        .success());
    row(
        &mut ck,
        "12-disabled",
        t.tick("12-disabled", Some("valid-1"), dport, false),
        (4, "refused", "disabled", 0),
    );
    assert!(t
        .hades(&["enable", "--config", &t.s("gate/hades.toml")])
        .status
        .success());

    for n in 1..=3usize {
        say(&format!("7.{n} valid token valid-{n}: restart {n} of 3"));
        let label = format!("13.{n}-valid");
        row(
            &mut ck,
            &label,
            t.tick(&label, Some(&format!("valid-{n}")), dport, false),
            (0, "restarted", "approved", n),
        );
        let p = t.dummy_pid();
        println!(
            "dummy GET /health: {}",
            http::get(&format!("127.0.0.1:{dport}"), "/health")
                .map(|r| r.text())
                .unwrap_or_default()
        );
        if let Some(p) = p {
            procs.pids.push(p);
            say(&format!("kill the restarted dummy (pid {p})"));
            signal(p, "TERM");
        }
        ck.ok(&format!("restart {n}: dummy pid reported"), p.is_some(), "");
        std::thread::sleep(Duration::from_millis(300));
        if n == 1 {
            say("7.1b replay valid-1");
            row(
                &mut ck,
                "14-replayed",
                t.tick("14-replayed", Some("valid-1"), dport, false),
                (4, "refused", "replayed", 1),
            );
        }
    }

    say("8. fourth valid token within the hour: capped, token NOT consumed");
    row(
        &mut ck,
        "15-capped",
        t.tick("15-capped", Some("valid-4"), dport, false),
        (5, "capped", "-", 3),
    );
    let ledger = read(&t.p("gate/ledger.jsonl"));
    let n4_consumed = ledger
        .lines()
        .filter(|l| l.contains("\"event\":\"consumed\"") && l.contains(nonces[3].as_str()))
        .count();
    ck.eq("valid-4 nonce not consumed", "0", n4_consumed.to_string());
    ck.eq("ledger consumed nonces", "3", t.consumed().to_string());
    println!(
        "refusal/dry-run audit lines = {}",
        ledger
            .lines()
            .filter(|l| !l.contains("\"event\":\"consumed\""))
            .count()
    );

    say("approval step of restart 1");
    let v = json(&read(&t.p("tick-13.1-valid.json")));
    for s in v["result"]["steps"].as_array().into_iter().flatten() {
        if s["step"] == "approval" || s["step"] == "hades-plan" {
            let d = &s["detail"];
            let short = if d.get("gate").is_some() {
                serde_json::json!({"admin_fingerprint": d["admin_fingerprint"], "token_sha256_16": d["token_sha256_16"],
                    "decision": d["decision"], "code": d["code"], "approved": d["approved"], "alpha": d["alpha"], "released": d["released"]})
            } else {
                serde_json::json!({"plan_sha256": d["plan_sha256"], "outcome": d["outcome"], "dry_run": d["dry_run"]})
            };
            println!(
                "{}",
                serde_json::json!({"step": s["step"], "at": s["at"], "d": short})
            );
        }
    }

    say("hades-gate ledger (no token strings; consumed nonces and refusal reasons)");
    for l in ledger.lines() {
        let e = json(l);
        let mut m = serde_json::Map::new();
        for k in [
            "ts",
            "event",
            "target",
            "nonce",
            "reason",
            "token_sha256_16",
        ] {
            if !e[k].is_null() {
                m.insert(k.into(), e[k].clone());
            }
        }
        println!("{}", Value::Object(m));
    }

    say("nerve GET /events?n=200 (ts, severity, kind, short data)");
    let ev_text = http::get(&t.nerve, "/events?n=200")
        .map(|r| r.text())
        .unwrap_or_default();
    fs::write(t.p("nerve-events.json"), &ev_text).unwrap();
    let ev = json(&ev_text);
    let evs = ev.as_array().cloned().unwrap_or_default();
    for e in &evs {
        let mut m = serde_json::Map::new();
        for k in [
            "state",
            "stage",
            "code",
            "restarts_last_hour",
            "pid",
            "health_verified",
            "nonce",
            "retry_after_s",
            "token_consumed",
        ] {
            if !e["data"][k].is_null() {
                m.insert(k.into(), e["data"][k].clone());
            }
        }
        println!(
            "{}\t{}\t{}\t{}",
            jt(e, "ts"),
            jt(e, "severity"),
            jt(e, "kind"),
            Value::Object(m)
        );
    }
    let lines = read(&t.p("nerve.jsonl")).lines().count();
    println!("nerve events: {}; JSONL lines on disk: {lines}", evs.len());
    ck.ok(
        "nerve saw events",
        !evs.is_empty() && evs.len() == lines,
        &format!("{} events, {lines} lines", evs.len()),
    );
    let bad = evs
        .iter()
        .filter(|e| {
            let ts = jt(e, "ts");
            !(ts.ends_with("+02:00") || ts.ends_with("+01:00"))
        })
        .count();
    ck.eq("nerve stamps without Zagreb offset", "0", bad.to_string());
    let restarts = read(&t.p("state/restarts.jsonl"));
    ck.eq(
        "restart ledger lines",
        "3",
        restarts.lines().count().to_string(),
    );

    say("token / seed leak check");
    let mut outputs = Vec::new();
    for e in fs::read_dir(&t.out).unwrap().flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.starts_with("tick-") || n == "nerve.jsonl" || n == "nerve-events.json" {
            outputs.push(e.path());
        }
    }
    files_under(&t.p("state"), &mut outputs);
    files_under(&t.p("gate"), &mut outputs);
    let blobs: Vec<String> = outputs.iter().map(|p| read(p)).collect();
    let mut leak = false;
    for e in fs::read_dir(t.p("tok")).unwrap().flatten() {
        let tok = read(&e.path());
        let tok = tok.trim();
        if !tok.is_empty() && blobs.iter().any(|b| b.contains(tok)) {
            println!(
                "LEAK: token {} found in output",
                e.file_name().to_string_lossy()
            );
            leak = true;
        }
    }
    let seed = read(&t.p("admin/admin.seed"));
    let seedhex = seed.split_whitespace().nth(1).unwrap_or("").to_string();
    if !seedhex.is_empty() && blobs.iter().any(|b| b.contains(&seedhex)) {
        println!("LEAK: seed");
        leak = true;
    }
    ck.ok(
        "no token string or seed in tick output, nerve log, gate ledger or state",
        !leak && !seedhex.is_empty(),
        "",
    );

    say("teardown");
    if let Some(p) = t.dummy_pid() {
        procs.pids.push(p);
    }
    drop(procs);
    std::thread::sleep(Duration::from_millis(300));
    for p in [nport, dport, oport] {
        ck.ok(&format!("port {p} closed"), !port_open(p), "");
    }
    ck.finish("gaia-hades-tokens");
}
