// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Gaia scenarios for the box stack (`--with-gaia`, slot 230, Gaia S8.1).
//! Every check runs the real release `gaia`, `hades-gate` and
//! `openmind-queue` binaries against the stack's own nerve and queue:
//!
//! * `gaia-heal-tick`: `gaia heal-plan` / `heal-tick` in plan-only mode (no
//!   approval token, so HADES releases the dry-run plan only and nothing is
//!   ever restarted), the scope guard, and every step read back from the
//!   nerve's `/events`.
//! * `gaia-hades-refusals`: every hades-gate token refusal path through
//!   `gaia heal-tick` (forged, wrong scope, expired, malformed, replayed,
//!   disabled gate, gate unavailable, admin safety), each recorded on the
//!   nerve with its code, no token ever consumed by a refusal, no token text
//!   in any output, and the never-bound target still closed at the end.
//! * `gaia-poseidon`: `gaia poseidon` submit / inspect / spend-cap and quota
//!   refusals / VRAM against the supervised `openmind-queue`.
//!
//! No valid, unconsumed token ever reaches `heal-tick`, so the apply path
//! (a restart) cannot run: the heal scenarios are plan-only by construction.

use crate::http;
use crate::report::Suite;
use crate::stack::{off, Stack};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Never-bound heal target name (its port is `base + off::GAIA_HEAL`).
pub const HEAL_SVC: &str = "rt-heal";

struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(bin: &Path, args: &[String]) -> Result<Ran, String> {
    let out = Command::new(bin)
        .args(args)
        .env("TZ", "Europe/Zagreb")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{}: {e}", bin.display()))?;
    Ok(Ran {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn short(t: &str) -> String {
    t.chars().take(200).collect()
}

/// `gaia ARGS --compact` -> (exit, `result` JSON, raw output).
fn gaia(st: &Stack, args: &[String]) -> Result<(i32, Value, Ran), String> {
    let mut a = args.to_vec();
    a.push("--compact".into());
    let r = run(&st.bins.path("gaia"), &a)?;
    let v: Value = serde_json::from_str(r.stdout.trim()).unwrap_or(Value::Null);
    Ok((r.code, v["result"].clone(), r))
}

/// The nerve's event log (`GET /events?n=N`).
fn events(st: &Stack) -> Result<Vec<Value>, String> {
    let r = http::get(&st.addr(off::NERVE), "/events?n=1000")?;
    match r.json()? {
        Value::Array(a) => Ok(a),
        v => Err(format!("/events not an array: {}", short(&v.to_string()))),
    }
}

/// The probe id of a heal-tick report (its `probe` or any step).
fn probe_id(report: &Value) -> Option<String> {
    report["steps"].as_array()?.iter().find_map(|st| {
        st["detail"]["probe_id"]
            .as_str()
            .map(str::to_string)
            .filter(|p| !p.is_empty())
    })
}

/// Events with `kind` whose `data.probe_id` is `pid`.
fn by_probe<'a>(ev: &'a [Value], kind: &str, pid: &str) -> Vec<&'a Value> {
    ev.iter()
        .filter(|e| e["kind"] == kind && e["data"]["probe_id"] == pid)
        .collect()
}

fn approval_code(report: &Value) -> String {
    report["steps"]
        .as_array()
        .and_then(|a| a.iter().find(|s| s["step"] == "approval"))
        .and_then(|s| s["detail"]["code"].as_str())
        .unwrap_or("")
        .to_string()
}

/// Shared fixture for the heal scenarios: the target, its argv and paths.
struct Heal {
    port: u16,
    allow: String,
    nerve: String,
    state: PathBuf,
    dir: PathBuf,
    argv: Vec<String>,
}

impl Heal {
    fn new(st: &Stack) -> Heal {
        let port = st.base + off::GAIA_HEAL;
        Heal {
            port,
            allow: format!("{port}-{port}"),
            nerve: st.addr(off::NERVE),
            state: st.path("gaia/heal-state"),
            dir: st.path("gaia"),
            // A real, absolute, allowed program: the stack's own tuner
            // binary. It is never started (no token is ever valid here).
            argv: vec![st.bins.path("tuner").to_string_lossy().into_owned()],
        }
    }

    /// `heal-tick` args for `svc` on `port`, with optional extra flags.
    fn tick_args(&self, svc: &str, port: u16, allow: &str, extra: &[String]) -> Vec<String> {
        let mut a = s(&["heal-tick", "--service", svc, "--port"]);
        a.push(port.to_string());
        a.extend(s(&["--allow-ports", allow, "--nerve", &self.nerve]));
        a.push("--state".into());
        a.push(self.state.to_string_lossy().into_owned());
        a.extend_from_slice(extra);
        a.extend(s(&["--wait-ms", "500"]));
        a.push("--".into());
        a.extend(self.argv.iter().cloned());
        a
    }

    fn tick(&self, st: &Stack, extra: &[String]) -> Result<(i32, Value, Ran), String> {
        gaia(st, &self.tick_args(HEAL_SVC, self.port, &self.allow, extra))
    }

    fn cfg(&self) -> PathBuf {
        self.dir.join("gate/hades.toml")
    }
    fn path(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }
}

fn p(x: &Path) -> String {
    x.to_string_lossy().into_owned()
}

fn skip_reason(st: &Stack, need_nerve: bool) -> Option<String> {
    if !st.gaia {
        let miss = st.bins.gaia_missing();
        return Some(if miss.is_empty() {
            "gaia, hades-gate and openmind-queue present but not enabled (pass --with-gaia)"
                .to_string()
        } else {
            format!(
                "missing release binaries in {}: {} (build them, pass --with-gaia)",
                st.bins.dir.display(),
                miss.join(", ")
            )
        });
    }
    if need_nerve && !st.nerve {
        return Some("heal-tick reads its input from the nerve: pass --with-nerve".into());
    }
    let port = st.base + off::GAIA_HEAL;
    if need_nerve && !(19000..=19999).contains(&port) {
        return Some(format!(
            "heal target 127.0.0.1:{port} is outside Gaia's test ports 19000-19999 (use a base port in 19000-19946)"
        ));
    }
    None
}

/// Plan-only heal tick: the plan is deterministic and matches hades-gate's
/// hash, a healthy service is left alone, a down one gets the dry-run plan
/// only (no token), and the scope guard refuses anything that is not a box
/// test service. Every step is read back from the nerve.
pub fn heal_tick(st: &Stack) -> Suite {
    let mut su = Suite::new("gaia-heal-tick", "gaia");
    if let Some(why) = skip_reason(st, true) {
        su.skip("heal-tick", &why);
        return su;
    }
    let h = Heal::new(st);
    let _ = fs::create_dir_all(&h.dir);
    let plan_file = h.path("plan-heal.txt");
    let _ = fs::remove_file(&plan_file);

    // 1. heal-plan: exact plan + SHA-256, admin safety ok, file == hash.
    let mut plan_sha = String::new();
    su.check("heal-plan-sha-matches-hades-gate", || {
        let mut a = s(&["heal-plan", "--service", HEAL_SVC, "--port"]);
        a.push(h.port.to_string());
        a.extend(s(&[
            "--allow-ports",
            &h.allow,
            "--out",
            &p(&plan_file),
            "--",
        ]));
        a.extend(h.argv.iter().cloned());
        let (c, r, raw) = gaia(st, &a)?;
        if c != 0 {
            return Err(format!("exit {c}: {}", short(&raw.stderr)));
        }
        let sha = r["plan_sha256"].as_str().unwrap_or("").to_string();
        if sha.len() != 64 || r["admin_safety"] != "ok" || r["action"] != "heal-apply" {
            return Err(format!("unexpected plan: {}", short(&r.to_string())));
        }
        let hg = run(
            &st.bins.path("hades-gate"),
            &s(&["plan-hash", "--plan-file", &p(&plan_file)]),
        )?;
        if hg.code != 0 || hg.stdout.trim() != sha {
            return Err(format!(
                "hades-gate plan-hash {} (exit {}) != gaia {sha}",
                hg.stdout.trim(),
                hg.code
            ));
        }
        let lines = r["plan"].as_array().map_or(0, Vec::len);
        plan_sha = sha.clone();
        Ok(format!(
            "{lines}-line plan, sha256 {}..., admin_safety ok, hades-gate plan-hash equal",
            &sha[..16]
        ))
    });
    su.check("heal-plan-deterministic", || {
        let mut a = s(&["heal-plan", "--service", HEAL_SVC, "--port"]);
        a.push(h.port.to_string());
        a.extend(s(&["--allow-ports", &h.allow, "--"]));
        a.extend(h.argv.iter().cloned());
        let (c, r, _) = gaia(st, &a)?;
        (c == 0 && !plan_sha.is_empty() && r["plan_sha256"] == plan_sha.as_str())
            .then(|| "second run gives the same sha256".to_string())
            .ok_or(format!("exit {c}, {}", short(&r.to_string())))
    });

    // 2. Healthy service (the stack's tuner): nothing to do, pid unchanged.
    su.check("healthy-target-left-alone", || {
        let port = st.base + off::TUNER;
        let before = st.pid("tuner");
        let (c, r, raw) = gaia(
            st,
            &h.tick_args("tuner", port, &format!("{port}-{port}"), &[]),
        )?;
        let after = st.pid("tuner");
        if c != 0 || r["outcome"] != "healthy" {
            return Err(format!(
                "want exit 0 healthy, got {c} {} {}",
                short(&r.to_string()),
                short(&raw.stderr)
            ));
        }
        if before.is_none() || before != after {
            return Err(format!("tuner pid changed: {before:?} -> {after:?}"));
        }
        let pid = probe_id(&r).ok_or("no probe_id in report")?;
        let ev = events(st)?;
        let probes = by_probe(&ev, "service.probe", &pid);
        if probes.len() != 1 || probes[0]["data"]["state"] != "up" {
            return Err(format!("nerve service.probe for {pid}: {probes:?}"));
        }
        Ok(format!(
            "exit 0 healthy, tuner pid {} unchanged, nerve service.probe state up",
            before.unwrap_or(0)
        ))
    });

    // 3. Down target, no token: HADES releases the dry-run plan only.
    su.check("no-gate-config-fails-closed", || {
        let (c, r, raw) = h.tick(st, &s(&["--hades-config", &p(&h.path("no-gate.toml"))]))?;
        // No gate configured at all is refused as gate-unavailable; the real
        // no-token path is checked with a gate below.
        if c != 4 || r["outcome"] != "refused" {
            return Err(format!(
                "want exit 4 refused, got {c} {} {}",
                short(&r.to_string()),
                short(&raw.stderr)
            ));
        }
        let code = r["steps"]
            .as_array()
            .and_then(|a| a.iter().find(|s| s["step"] == "approval"))
            .map(|s| s["detail"]["reason"].to_string())
            .unwrap_or_default();
        Ok(format!("exit 4, no gate -> fail closed ({})", short(&code)))
    });
    let gate = setup_gate(st, &h);
    su.check("hades-gate-ready", || gate.clone());
    su.check("down-target-no-token-dry-run", || {
        gate.clone()?;
        let (c, r, raw) = h.tick(st, &s(&["--hades-config", &p(&h.cfg())]))?;
        let code = approval_code(&r);
        if c != 4 || r["outcome"] != "refused" || code != "no-token" {
            return Err(format!(
                "want exit 4 refused/no-token, got {c} {code:?} {} {}",
                short(&r.to_string()),
                short(&raw.stderr)
            ));
        }
        let steps: Vec<String> = r["steps"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|s| s["step"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let want = ["scope", "probe", "nerve-in", "demeter", "hades-plan", "approval"];
        if steps != want {
            return Err(format!("steps {steps:?}, want {want:?} (no apply)"));
        }
        let hp = r["steps"][4]["detail"]["plan_sha256"].as_str().unwrap_or("");
        if hp != plan_sha {
            return Err(format!("tick plan sha {hp} != heal-plan {plan_sha}"));
        }
        let pid = probe_id(&r).ok_or("no probe_id")?;
        let ev = events(st)?;
        let pr = by_probe(&ev, "service.probe", &pid);
        let rf = by_probe(&ev, "gaia.heal.refused", &pid);
        if pr.len() != 1 || pr[0]["data"]["state"] == "up" || pr[0]["severity"] != "warn" {
            return Err(format!("nerve service.probe: {pr:?}"));
        }
        if rf.len() != 1 || rf[0]["data"]["code"] != "no-token" || rf[0]["source"] != "gaia-live"
        {
            return Err(format!("nerve gaia.heal.refused: {rf:?}"));
        }
        if http::port_open(&format!("127.0.0.1:{}", h.port)) {
            return Err("target port is open: something was started".into());
        }
        Ok(format!(
            "exit 4 no-token; steps {}; nerve has service.probe (warn, {}) + gaia.heal.refused no-token for {pid}; target still closed",
            steps.join(">"),
            pr[0]["data"]["state"].as_str().unwrap_or("?")
        ))
    });

    // 4. Scope guard: live ports, ranges outside 19000-19999, forbidden programs.
    let scope_cases: Vec<(&str, Vec<String>)> = vec![
        (
            "scope-live-port-refused",
            h.tick_args(HEAL_SVC, 8420, "8420-8420", &[]),
        ),
        (
            "scope-port-outside-allow-refused",
            h.tick_args(HEAL_SVC, h.port - 1, &h.allow, &[]),
        ),
        ("scope-forbidden-program-refused", {
            let mut a = h.tick_args(HEAL_SVC, h.port, &h.allow, &[]);
            let n = a.len();
            a[n - 1] = "/usr/bin/systemctl".into();
            a
        }),
        ("scope-relative-program-refused", {
            let mut a = h.tick_args(HEAL_SVC, h.port, &h.allow, &[]);
            let n = a.len();
            a[n - 1] = "tuner".into();
            a
        }),
    ];
    for (name, mut args) in scope_cases {
        // heal-tick needs --hades-config even when the scope guard refuses.
        let at = args.iter().position(|x| x == "--wait-ms").unwrap_or(0);
        args.insert(at, p(&h.cfg()));
        args.insert(at, "--hades-config".into());
        su.check(name, || {
            let (c, r, raw) = gaia(st, &args)?;
            let first = r["steps"][0]["step"].as_str().unwrap_or("");
            let n = r["steps"].as_array().map_or(0, Vec::len);
            if c != 4 || r["outcome"] != "refused" || first != "scope" || n != 1 {
                return Err(format!(
                    "want exit 4 refused at scope only, got {c} {} {}",
                    short(&r.to_string()),
                    short(&raw.stderr)
                ));
            }
            Ok(format!(
                "exit 4 at scope: {}",
                short(r["detail"].as_str().unwrap_or(""))
            ))
        });
    }
    su.check("no-restart-recorded", || {
        let led = h.state.join("restarts.jsonl");
        match fs::read_to_string(&led) {
            Err(_) => Ok("no restarts.jsonl: nothing was ever restarted".into()),
            Ok(t) if t.trim().is_empty() => Ok("restarts.jsonl empty".into()),
            Ok(t) => Err(format!("restart ledger has entries: {}", short(&t))),
        }
    });
    su.log_tail = crate::stack::tail_file(&st.path("logs/nerve.log"), 5);
    su
}

/// Admin side for the box gate: keygen (admin + an attacker key) and the
/// hades-gate config. Returns the admin fingerprint.
fn setup_gate(st: &Stack, h: &Heal) -> Result<String, String> {
    let hg = st.bins.path("hades-gate");
    let gate = h.path("gate");
    let _ = fs::remove_dir_all(&gate);
    fs::create_dir_all(&gate).map_err(|e| e.to_string())?;
    let mut fp = String::new();
    for who in ["admin", "evil"] {
        let r = run(
            &hg,
            &s(&[
                "keygen",
                "--seed",
                &p(&h.path(&format!("{who}.seed"))),
                "--pub",
                &p(&gate.join(format!("{who}.pub"))),
            ]),
        )?;
        if r.code != 0 {
            return Err(format!(
                "keygen {who}: exit {} {}",
                r.code,
                short(&r.stderr)
            ));
        }
        if who == "admin" {
            let v: Value = serde_json::from_str(r.stdout.trim()).unwrap_or(Value::Null);
            fp = v["admin_fingerprint"].as_str().unwrap_or("").to_string();
        }
    }
    let _ = fs::remove_file(h.path("evil.pub"));
    fs::write(
        h.cfg(),
        format!(
            "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\n",
            p(&gate.join("admin.pub")),
            p(&gate.join("ledger.jsonl")),
            p(&gate.join("DISABLED"))
        ),
    )
    .map_err(|e| e.to_string())?;
    let r = run(&hg, &s(&["status", "--config", &p(&h.cfg())]))?;
    let v: Value = serde_json::from_str(r.stdout.trim()).unwrap_or(Value::Null);
    if r.code != 0 || v["disabled"] != false || v["admin_fingerprint"] != fp.as_str() {
        return Err(format!(
            "hades-gate status: exit {} {}",
            r.code,
            short(&r.stdout)
        ));
    }
    Ok(format!(
        "admin key {fp}, gate enabled, ledger + disable file in the fixture"
    ))
}

/// Mint one token with the real `hades-gate mint` into a new 0600 file.
fn mint(
    st: &Stack,
    h: &Heal,
    seed: &str,
    action: &str,
    target: &str,
    plan: &Path,
    timing: &[&str],
    out: &str,
) -> Result<PathBuf, String> {
    let file = h.path(out);
    let _ = fs::remove_file(&file);
    let mut a = s(&[
        "mint",
        "--seed",
        &p(&h.path(seed)),
        "--action",
        action,
        "--target",
        target,
        "--plan-file",
        &p(plan),
        "--out",
        &p(&file),
    ]);
    a.extend(s(timing));
    let r = run(&st.bins.path("hades-gate"), &a)?;
    if r.code != 0 {
        return Err(format!("mint {out}: exit {} {}", r.code, short(&r.stderr)));
    }
    Ok(file)
}

fn hades_check(st: &Stack, h: &Heal, plan: &Path, token: &Path) -> Result<(i32, Value), String> {
    let r = run(
        &st.bins.path("hades-gate"),
        &s(&[
            "check",
            "--config",
            &p(&h.cfg()),
            "--plan-file",
            &p(plan),
            "--action",
            "heal-apply",
            "--target",
            HEAL_SVC,
            "--token-file",
            &p(token),
        ]),
    )?;
    let v: Value = serde_json::from_str(r.stdout.trim()).unwrap_or(Value::Null);
    Ok((r.code, v))
}

/// Every hades-gate refusal path through `gaia heal-tick`.
pub fn hades_refusals(st: &Stack) -> Suite {
    let mut su = Suite::new("gaia-hades-refusals", "hades-gate");
    if let Some(why) = skip_reason(st, true) {
        su.skip("token-refusals", &why);
        return su;
    }
    let h = Heal::new(st);
    // The gaia-heal-tick suite set up the gate; this suite stands alone.
    if let Err(e) = setup_gate(st, &h) {
        su.check("hades-gate-ready", || Err(e));
        return su;
    }
    let plan = h.path("plan-refusals.txt");
    let other_plan = h.path("plan-other.txt");
    for f in [&plan, &other_plan] {
        let _ = fs::remove_file(f);
    }
    let mk_plan = |out: &Path, extra_arg: Option<&str>| -> Result<(), String> {
        let mut a = s(&["heal-plan", "--service", HEAL_SVC, "--port"]);
        a.push(h.port.to_string());
        a.extend(s(&["--allow-ports", &h.allow, "--out", &p(out), "--"]));
        a.extend(h.argv.iter().cloned());
        if let Some(x) = extra_arg {
            a.push(x.into());
        }
        let (c, _, raw) = gaia(st, &a)?;
        (c == 0)
            .then_some(())
            .ok_or(format!("heal-plan exit {c}: {}", short(&raw.stderr)))
    };
    if let Err(e) = mk_plan(&plan, None).and_then(|()| mk_plan(&other_plan, Some("--other"))) {
        su.check("plans-ready", || Err(e));
        return su;
    }
    let cfg = p(&h.cfg());
    let mut tokens_seen: Vec<String> = Vec::new();
    let mut outputs = String::new();
    let mut refused_codes: Vec<(String, String)> = Vec::new();

    // (check name, how to make the token file, expected code)
    type Make<'a> = Box<dyn Fn() -> Result<Option<PathBuf>, String> + 'a>;
    let cases: Vec<(&str, Make, &str)> = vec![
        (
            "forged-token-refused",
            Box::new(|| {
                mint(
                    st,
                    &h,
                    "evil.seed",
                    "heal-apply",
                    HEAL_SVC,
                    &plan,
                    &[],
                    "t-forged",
                )
                .map(Some)
            }),
            "forged",
        ),
        (
            "wrong-target-refused",
            Box::new(|| {
                mint(
                    st,
                    &h,
                    "admin.seed",
                    "heal-apply",
                    "rt-other",
                    &plan,
                    &[],
                    "t-target",
                )
                .map(Some)
            }),
            "wrong-scope",
        ),
        (
            "wrong-plan-refused",
            Box::new(|| {
                mint(
                    st,
                    &h,
                    "admin.seed",
                    "heal-apply",
                    HEAL_SVC,
                    &other_plan,
                    &[],
                    "t-plan",
                )
                .map(Some)
            }),
            "wrong-scope",
        ),
        (
            "wrong-action-refused",
            Box::new(|| {
                mint(
                    st,
                    &h,
                    "admin.seed",
                    "deploy",
                    HEAL_SVC,
                    &plan,
                    &[],
                    "t-action",
                )
                .map(Some)
            }),
            "wrong-scope",
        ),
        (
            "expired-token-refused",
            Box::new(|| {
                let iat = (std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
                    - 600)
                    .to_string();
                mint(
                    st,
                    &h,
                    "admin.seed",
                    "heal-apply",
                    HEAL_SVC,
                    &plan,
                    &["--iat", &iat, "--ttl", "60"],
                    "t-expired",
                )
                .map(Some)
            }),
            "expired",
        ),
        (
            "malformed-token-refused",
            Box::new(|| {
                let f = h.path("t-malformed");
                fs::write(&f, "not-a-token\n").map_err(|e| e.to_string())?;
                Ok(Some(f))
            }),
            "malformed",
        ),
        (
            "empty-token-file-refused",
            Box::new(|| {
                let f = h.path("t-empty");
                fs::write(&f, "\n").map_err(|e| e.to_string())?;
                Ok(Some(f))
            }),
            "token-file",
        ),
    ];
    for (name, make, want) in cases {
        let tok = make();
        su.check(name, || {
            let tok = tok?.ok_or("no token file")?;
            if let Ok(t) = fs::read_to_string(&tok) {
                if t.trim().len() > 20 {
                    tokens_seen.push(t.trim().to_string());
                }
            }
            let (c, r, raw) =
                h.tick(st, &s(&["--hades-config", &cfg, "--token-file", &p(&tok)]))?;
            outputs.push_str(&raw.stdout);
            outputs.push_str(&raw.stderr);
            let code = if want == "token-file" {
                r["steps"]
                    .as_array()
                    .and_then(|a| a.last())
                    .map(|_| "token-file".to_string())
                    .filter(|_| r["detail"].as_str().is_some_and(|d| d.contains("empty")))
                    .unwrap_or_default()
            } else {
                approval_code(&r)
            };
            if c != 4 || r["outcome"] != "refused" || code != want {
                return Err(format!(
                    "want exit 4 {want}, got {c} {code:?}: {} {}",
                    short(r["detail"].as_str().unwrap_or("")),
                    short(&raw.stderr)
                ));
            }
            let pid = probe_id(&r).ok_or("no probe_id")?;
            refused_codes.push((pid, want.to_string()));
            Ok(format!(
                "exit 4 {want}: {}",
                short(r["detail"].as_str().unwrap_or(""))
            ))
        });
    }

    // Replayed: the admin's own check consumes a valid token first.
    su.check("replayed-token-refused", || {
        let tok = mint(
            st,
            &h,
            "admin.seed",
            "heal-apply",
            HEAL_SVC,
            &plan,
            &[],
            "t-replay",
        )?;
        tokens_seen.push(fs::read_to_string(&tok).unwrap_or_default().trim().into());
        let (hc, hv) = hades_check(st, &h, &plan, &tok)?;
        if hc != 0 || hv["decision"] != "Approved" {
            return Err(format!("hades-gate check did not approve: {hc} {hv}"));
        }
        let (c, r, raw) = h.tick(st, &s(&["--hades-config", &cfg, "--token-file", &p(&tok)]))?;
        outputs.push_str(&raw.stdout);
        outputs.push_str(&raw.stderr);
        let code = approval_code(&r);
        if c != 4 || code != "replayed" {
            return Err(format!(
                "want exit 4 replayed, got {c} {code:?}: {}",
                short(&r.to_string())
            ));
        }
        refused_codes.push((probe_id(&r).unwrap_or_default(), "replayed".into()));
        Ok("hades-gate check approved+consumed it, then heal-tick: exit 4 replayed".into())
    });

    // Disabled gate: refused first, and the valid token is NOT consumed.
    su.check("disabled-gate-refused-token-not-burned", || {
        let tok = mint(st, &h, "admin.seed", "heal-apply", HEAL_SVC, &plan, &[], "t-disabled")?;
        tokens_seen.push(fs::read_to_string(&tok).unwrap_or_default().trim().into());
        let hg = st.bins.path("hades-gate");
        let d = run(&hg, &s(&["disable", "--config", &cfg]))?;
        if d.code != 0 {
            return Err(format!("disable: exit {}", d.code));
        }
        let ticked = h.tick(st, &s(&["--hades-config", &cfg, "--token-file", &p(&tok)]));
        let e = run(&hg, &s(&["enable", "--config", &cfg]))?;
        let (c, r, raw) = ticked?;
        outputs.push_str(&raw.stdout);
        outputs.push_str(&raw.stderr);
        let code = approval_code(&r);
        if e.code != 0 {
            return Err(format!("enable: exit {}", e.code));
        }
        if c != 4 || code != "disabled" {
            return Err(format!("want exit 4 disabled, got {c} {code:?}: {}", short(&r.to_string())));
        }
        refused_codes.push((probe_id(&r).unwrap_or_default(), "disabled".into()));
        // Proof the refusal did not consume it: the admin's own check (not
        // heal-tick, so nothing restarts) approves and consumes it now.
        let (hc, hv) = hades_check(st, &h, &plan, &tok)?;
        if hc != 0 || hv["decision"] != "Approved" {
            return Err(format!("token was burned by the refusal: {hc} {hv}"));
        }
        Ok("exit 4 disabled; after enable the same token still approves in hades-gate check (not burned), then consumed".into())
    });

    // Admin safety: a plan with a shell metacharacter is unapprovable.
    su.check("admin-safety-refused", || {
        let mut a = h.tick_args(HEAL_SVC, h.port, &h.allow, &[]);
        let at = a.iter().position(|x| x == "--wait-ms").unwrap_or(0);
        a.insert(at, cfg.clone());
        a.insert(at, "--hades-config".into());
        a.push("x;id".into());
        let (c, r, raw) = gaia(st, &a)?;
        outputs.push_str(&raw.stdout);
        let code = approval_code(&r);
        if c != 4 || code != "admin-safety" {
            return Err(format!(
                "want exit 4 admin-safety, got {c} {code:?}: {}",
                short(&r.to_string())
            ));
        }
        refused_codes.push((probe_id(&r).unwrap_or_default(), "admin-safety".into()));
        Ok(format!(
            "exit 4 admin-safety: {}",
            short(r["detail"].as_str().unwrap_or(""))
        ))
    });

    su.check("every-refusal-on-nerve", || {
        let ev = events(st)?;
        let mut missing = Vec::new();
        for (pid, code) in &refused_codes {
            let rf = by_probe(&ev, "gaia.heal.refused", pid);
            let want_code = if code == "token-file" {
                "token-file"
            } else {
                code.as_str()
            };
            if rf.len() != 1 || rf[0]["data"]["code"] != want_code {
                missing.push(format!("{code}@{pid}"));
            }
        }
        let approved = ev
            .iter()
            .filter(|e| e["kind"] == "gaia.heal.approved")
            .count();
        let applied = ev
            .iter()
            .filter(|e| e["kind"] == "gaia.heal.applied" || e["kind"] == "gaia.heal.failed")
            .count();
        if !missing.is_empty() || approved != 0 || applied != 0 {
            return Err(format!(
                "missing {missing:?}; approved {approved}, applied/failed {applied} (want 0)"
            ));
        }
        Ok(format!(
            "{} refusals each on the nerve with its code; 0 gaia.heal.approved/applied/failed",
            refused_codes.len()
        ))
    });
    su.check("hades-ledger-audits-refusals", || {
        let t = fs::read_to_string(h.path("gate/ledger.jsonl")).map_err(|e| e.to_string())?;
        let refused = t.lines().filter(|l| l.contains("\"refused\"")).count();
        let leaked = tokens_seen
            .iter()
            .filter(|k| !k.is_empty() && t.contains(k.as_str()))
            .count();
        if refused < refused_codes.len().saturating_sub(1) || leaked != 0 {
            return Err(format!(
                "ledger: {refused} refused lines, {leaked} raw tokens"
            ));
        }
        Ok(format!(
            "{} ledger lines, {refused} refused, no raw token text",
            t.lines().count()
        ))
    });
    su.check("no-token-text-in-output-or-nerve", || {
        let ev = events(st)?;
        let evs = serde_json::to_string(&ev).unwrap_or_default();
        let leaked: usize = tokens_seen
            .iter()
            .filter(|k| !k.is_empty() && (outputs.contains(k.as_str()) || evs.contains(k.as_str())))
            .count();
        (leaked == 0 && !tokens_seen.is_empty())
            .then(|| {
                format!(
                    "{} minted tokens, none in gaia output or nerve events",
                    tokens_seen.len()
                )
            })
            .ok_or(format!("{leaked} token(s) leaked"))
    });
    su.check("target-never-started", || {
        if http::port_open(&format!("127.0.0.1:{}", h.port)) {
            return Err("target port open".into());
        }
        match fs::read_to_string(h.state.join("restarts.jsonl")) {
            Ok(t) if !t.trim().is_empty() => Err(format!("restarts: {}", short(&t))),
            _ => Ok(format!("127.0.0.1:{} closed, no restart recorded", h.port)),
        }
    });
    for f in ["admin.seed", "evil.seed"] {
        let _ = fs::remove_file(h.path(f));
    }
    su
}

fn metric(v: &Value, k: &str) -> i64 {
    v["report"]["metrics"][k].as_i64().unwrap_or(i64::MIN)
}

/// POSEIDON through Gaia: submit, inspect, spend-cap and quota refusals,
/// VRAM, and an unreachable queue.
pub fn poseidon(st: &Stack) -> Suite {
    let mut su = Suite::new("gaia-poseidon", "openmind-queue");
    if let Some(why) = skip_reason(st, false) {
        su.skip("poseidon", &why);
        return su;
    }
    let q = st.addr(off::QUEUE);
    let gp = |args: &[&str]| -> Result<(i32, Value), String> {
        let mut a = s(&["poseidon"]);
        a.extend(s(args));
        a.extend(s(&["--queue", &q]));
        gaia(st, &a).map(|(c, v, _)| (c, v))
    };
    let op = |args: &[&str]| -> Result<(), String> {
        let mut a = s(args);
        a.extend(s(&["--addr", &q]));
        let r = run(&st.bins.path("openmind-queue"), &a)?;
        (r.code == 0).then_some(()).ok_or(format!(
            "openmind-queue {args:?}: exit {} {}",
            r.code,
            short(&r.stderr)
        ))
    };
    su.check("queue-healthz", || {
        let r = http::get(&q, "/healthz")?;
        (r.status == 200)
            .then(|| format!("200 {}", short(r.text().trim())))
            .ok_or(format!("HTTP {}", r.status))
    });
    su.check("operator-setup", || {
        op(&["post", "/caps", r#"{"scope":"rt-a","cap":50}"#])?;
        op(&[
            "post",
            "/quotas",
            r#"{"agent":"rt-b","max_pending":1,"max_inflight":1}"#,
        ])?;
        op(&[
            "post",
            "/vram/devices",
            r#"{"id":"gpu0","kind":"gpu","vram_gb":24,"state":"approved"}"#,
        ])?;
        Ok("cap rt-a 50, quota rt-b 1/1, gpu0 24 GB approved (operator CLI)".into())
    });
    su.check("status", || {
        let (c, v) = gp(&["status"])?;
        (c == 0)
            .then(|| format!("exit 0, state {}", v["state"]))
            .ok_or(format!("exit {c} {}", short(&v.to_string())))
    });
    let mut first = 0i64;
    su.check("submit-ok-and-inspect", || {
        let (c, v) = gp(&[
            "submit", "--agent", "rt-a", "--kind", "echo", "--cost", "30",
        ])?;
        let id = metric(&v, "job_id");
        if c != 0 || id <= 0 {
            return Err(format!("submit exit {c}: {}", short(&v.to_string())));
        }
        let (c2, j) = gp(&["job", &id.to_string()])?;
        if c2 != 0 || metric(&j, "live") != 1 {
            return Err(format!("job {id}: exit {c2} {}", short(&j.to_string())));
        }
        first = id;
        Ok(format!(
            "job {id} submitted (exit 0) and live via `gaia poseidon job`"
        ))
    });
    su.check("spend-cap-refused-402", || {
        op(&["worker", "--worker", "rt-w", "--max-jobs", "1", "--idle-exit-ms", "1000"])?;
        let (c, v) = gp(&["submit", "--agent", "rt-a", "--kind", "echo", "--cost", "30"])?;
        if (c, metric(&v, "http_status")) != (4, 402) {
            return Err(format!("want exit 4 / 402, got {c}: {}", short(&v.to_string())));
        }
        let (c2, v2) = gp(&["submit", "--agent", "rt-a", "--kind", "echo", "--cost", "10"])?;
        let id2 = metric(&v2, "job_id");
        if c2 != 0 || id2 <= first {
            return Err(format!("cost 10 within cap: exit {c2} {}", short(&v2.to_string())));
        }
        let (_, caps) = gp(&["caps"])?;
        let spent = metric(&caps, "spent:rt-a");
        (spent == 30)
            .then(|| format!("after the worker spent 30: cost 30 -> exit 4 HTTP 402, cost 10 -> job {id2}; caps spent:rt-a 30/50"))
            .ok_or(format!("caps spent:rt-a {spent}: {}", short(&caps.to_string())))
    });
    su.check("quota-refused-429", || {
        let (c, _) = gp(&["submit", "--agent", "rt-b", "--kind", "echo"])?;
        let (c2, v2) = gp(&["submit", "--agent", "rt-b", "--kind", "echo"])?;
        (c == 0 && (c2, metric(&v2, "http_status")) == (4, 429))
            .then(|| "rt-b: first exit 0, second exit 4 HTTP 429".to_string())
            .ok_or(format!("exits {c}/{c2}: {}", short(&v2.to_string())))
    });
    su.check("vram", || {
        let (c, v) = gp(&["vram"])?;
        let (c2, p2) = gp(&["plan-vram", "8"])?;
        (c == 0 && metric(&v, "approved_gb") == 24 && c2 == 0)
            .then(|| {
                format!(
                    "approved_gb 24; plan-vram 8 exit 0 ({})",
                    p2["report"]["outcome"]
                )
            })
            .ok_or(format!(
                "vram exit {c} {}, plan exit {c2}",
                short(&v.to_string())
            ))
    });
    su.check("queue-unreachable-exit-3", || {
        let down = format!("127.0.0.1:{}", st.base + off::GAIA_HEAL);
        let (c, v, _) = gaia(st, &s(&["poseidon", "status", "--queue", &down]))?;
        (c == 3)
            .then(|| format!("{down} (never bound): exit 3"))
            .ok_or(format!("exit {c}: {}", short(&v.to_string())))
    });
    su.log_tail = st.log_tail("openmind-queue", 5);
    su
}
