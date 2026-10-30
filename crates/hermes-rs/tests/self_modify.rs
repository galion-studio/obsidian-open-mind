// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Hermes self-modify through the sandbox gate, against a fake Hermes repo
//! (a tiny cargo workspace with the same crates/hermes-rs layout). The gate
//! runner is the real `hermes-self-modify` binary with its own key file.

use hermes_rs::self_modify as sm;
use hermes_rs::tools::{Registry, Tool, ToolCtx};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

static N: AtomicUsize = AtomicUsize::new(0);

const LIB_OK: &str = "pub fn greet() -> &'static str {\n    \"hi\"\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn greets() {\n        assert_eq!(super::greet(), \"hi\");\n    }\n}\n";

fn sh(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn put(p: &Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

struct Fx {
    root: PathBuf,
    repo: PathBuf,
    queue: PathBuf,
    sandbox_seed: PathBuf,
    sandbox_pub: PathBuf,
    owner_seed: PathBuf,
    owner_pub: PathBuf,
}

impl Fx {
    fn new(tag: &str) -> Fx {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join("self_modify")
            .join(format!(
                "{tag}-{}-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst),
                hermes_rs::time::now_secs()
            ));
        let repo = root.join("hermes-src");
        put(
            &repo.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/hermes-rs\"]\nresolver = \"2\"\n",
        );
        put(
            &repo.join("crates/hermes-rs/Cargo.toml"),
            "[package]\nname = \"fake-hermes\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[[bin]]\nname = \"hermes-gateway\"\npath = \"src/main.rs\"\n",
        );
        put(&repo.join("crates/hermes-rs/src/lib.rs"), LIB_OK);
        put(
            &repo.join("crates/hermes-rs/src/main.rs"),
            "fn main() {\n    println!(\"{}\", fake_hermes::greet());\n}\n",
        );
        put(&repo.join("crates/hermes-rs/SOUL.md"), "persona\n");
        sh(&repo, &["init", "-q"]);
        sh(&repo, &["add", "-A"]);
        sh(&repo, &["commit", "-q", "-m", "base"]);
        let keys = |n: &str| {
            let (s, p) = sm::keygen().unwrap();
            let (sp, pp) = (
                root.join(format!("{n}.seed")),
                root.join(format!("{n}.pubkey")),
            );
            std::fs::write(&sp, s).unwrap();
            std::fs::write(&pp, p).unwrap();
            (sp, pp)
        };
        let (sandbox_seed, sandbox_pub) = keys("sandbox");
        let (owner_seed, owner_pub) = keys("owner");
        Fx {
            queue: root.join("queue"),
            root,
            repo,
            sandbox_seed,
            sandbox_pub,
            owner_seed,
            owner_pub,
        }
    }

    fn vk(p: &Path) -> ed25519_dalek::VerifyingKey {
        sm::verifying_key(&std::fs::read_to_string(p).unwrap()).unwrap()
    }
    fn spub(&self) -> ed25519_dalek::VerifyingKey {
        Self::vk(&self.sandbox_pub)
    }
    fn opub(&self) -> ed25519_dalek::VerifyingKey {
        Self::vk(&self.owner_pub)
    }
    fn owner_key(&self) -> ed25519_dalek::SigningKey {
        sm::signing_key(&std::fs::read_to_string(&self.owner_seed).unwrap()).unwrap()
    }
    fn sandbox_key(&self) -> ed25519_dalek::SigningKey {
        sm::signing_key(&std::fs::read_to_string(&self.sandbox_seed).unwrap()).unwrap()
    }

    /// The cargo plan (build, tests, clippy when the toolchain has it).
    fn cargo_plan(&self) -> PathBuf {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let mut stages = vec![
            json!({"name": "build", "argv": [cargo, "build", "--offline"], "timeout_s": 600}),
            json!({"name": "test", "argv": [cargo, "test", "--offline", "--all-targets"], "timeout_s": 600}),
        ];
        let clippy = Command::new(&cargo)
            .args(["clippy", "--version"])
            .output()
            .is_ok_and(|o| o.status.success());
        if clippy {
            stages.push(json!({"name": "clippy", "argv": [cargo, "clippy", "--offline", "--all-targets", "--", "-D", "warnings"], "timeout_s": 600}));
        }
        let shared = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join("self_modify")
            .join("fake-target");
        self.plan(json!({"stages": stages, "artifact": "target/debug/hermes-gateway",
            "target_dir": shared.join(format!("{}", N.fetch_add(1, Ordering::SeqCst))),
            "pass_env": ["PATH", "HOME", "CARGO_HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN", "LD_LIBRARY_PATH", "RUSTC", "RUSTFLAGS", "RUSTDOCFLAGS", "CARGO"]}))
    }

    /// A fast plan (no cargo) for the signature tests.
    fn quick_plan(&self) -> PathBuf {
        self.plan(
            json!({"stages": [{"name": "noop", "argv": ["git", "--version"]}],
            "artifact": "crates/hermes-rs/src/main.rs", "pass_env": ["PATH", "HOME"]}),
        )
    }

    fn plan(&self, v: Value) -> PathBuf {
        let p = self
            .root
            .join(format!("plan-{}.json", N.fetch_add(1, Ordering::SeqCst)));
        std::fs::write(&p, v.to_string()).unwrap();
        p
    }

    fn tool(&self, plan: Option<&Path>) -> sm::SelfModifyTool {
        let gate_cmd = plan.map(|p| {
            [
                env!("CARGO_BIN_EXE_hermes-self-modify"),
                "gate",
                "--repo",
                self.repo.to_str().unwrap(),
                "--scratch",
                self.root.join("gate-scratch").to_str().unwrap(),
                "--plan",
                p.to_str().unwrap(),
                "--key",
                self.sandbox_seed.to_str().unwrap(),
            ]
            .iter()
            .map(|s| s.to_string())
            .collect()
        });
        sm::SelfModifyTool::new(sm::Cfg {
            repo: self.repo.clone(),
            queue: self.queue.clone(),
            base: "HEAD".into(),
            sandbox_pub: self.spub(),
            owner_pub: Some(self.opub()),
            sandbox_key_path: self.root.join("not-for-hermes.seed"),
            gate_cmd,
            run_as: None,
            cli: None,
            child_env: vec![],
        })
    }
}

type Edit = Box<dyn Fn(String) -> String>;

fn call(t: &dyn Tool, args: Value) -> (bool, Value) {
    let cancel = Arc::new(AtomicBool::new(false));
    let emit = |_e: hermes_rs::tools::Event| {};
    let ctx = ToolCtx {
        session_id: "s",
        spend_session: "s",
        depth: 0,
        user_text: "",
        cancel: &cancel,
        emit: &emit,
    };
    let o = t.call(&args, &ctx);
    (
        o.is_error,
        serde_json::from_str(&o.content).unwrap_or(json!(o.content)),
    )
}

fn propose(t: &dyn Tool, path: &str, content: &str) -> (bool, Value) {
    call(
        t,
        json!({"action": "propose", "summary": "greet with hello", "reason": "user prefers hello",
        "files": [{"path": path, "content": content}]}),
    )
}

fn entry(fx: &Fx, out: &Value) -> PathBuf {
    fx.queue.join(out["id"].as_str().expect("id"))
}

fn deploy_cfg(fx: &Fx, tag: &str) -> sm::DeployCfg {
    sm::DeployCfg {
        bin_dir: fx.root.join(format!("live-{tag}/bin")),
        dropin_dir: fx.root.join(format!("live-{tag}/unit.d")),
        dropin_name: "99s-self-modify.conf".into(),
        extra_lines: vec!["Environment=HERMES_RS_SELF_MODIFY_TEST=1".into()],
    }
}

/// A real signed PASS without cargo (for the signature tests).
fn quick_pass(fx: &Fx) -> PathBuf {
    let t = fx.tool(Some(&fx.quick_plan()));
    let (err, out) = propose(
        &t,
        "crates/hermes-rs/src/lib.rs",
        &LIB_OK.replace("\"hi\"", "\"hi\" // tweak"),
    );
    assert!(!err, "{out}");
    let e = entry(fx, &out);
    assert!(
        out["state"].as_str().unwrap().contains("pending Slavik"),
        "{out}"
    );
    e
}

#[test]
fn full_flow_build_sign_pending_approve_install_rollback() {
    let fx = Fx::new("flow");
    let t = fx.tool(Some(&fx.cargo_plan()));
    let new_lib = LIB_OK.replace("\"hi\"", "\"hello\"");
    let (err, out) = propose(&t, "crates/hermes-rs/src/lib.rs", &new_lib);
    assert!(!err, "{out}");
    let e = entry(&fx, &out);
    // Hermes's worktree copy has the change; the source repo does not.
    assert!(
        std::fs::read_to_string(e.join("worktree/crates/hermes-rs/src/lib.rs"))
            .unwrap()
            .contains("hello")
    );
    assert!(
        std::fs::read_to_string(fx.repo.join("crates/hermes-rs/src/lib.rs"))
            .unwrap()
            .contains("\"hi\"")
    );
    // Signed sandbox pass, waiting for Slavik.
    let en = sm::Entry::open(&e);
    let (rec, _) = en
        .verified_record(&fx.spub())
        .expect("record verifies with the gate key");
    assert_eq!(rec["verdict"], "pass", "{rec}\n{}", stage_logs(&e));
    let names: Vec<&str> = rec["stages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(names.starts_with(&["build", "test"]), "{names:?}");
    assert!(rec["checks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c["ok"] == true));
    assert!(en.state(&fx.spub(), &fx.opub()).contains("pending Slavik"));
    let (_, st) = call(&t, json!({"action": "status", "id": out["id"]}));
    assert_eq!(st["gate"]["verdict"], "pass", "{st}");
    let (_, list) = call(&t, json!({"action": "list"}));
    assert_eq!(list["changes"].as_array().unwrap().len(), 1);
    // Not deployable before approval.
    let d = deploy_cfg(&fx, "a");
    assert!(sm::deploy(&e, &fx.spub(), &fx.opub(), &d)
        .unwrap_err()
        .contains("no owner approval"));
    // Slavik approves; deploy installs a versioned binary + drop-in.
    sm::approve(&e, &fx.owner_key(), &fx.spub(), "slavik", 3600).unwrap();
    assert!(en
        .state(&fx.spub(), &fx.opub())
        .contains("approved by the owner"));
    put(
        &d.dropin_dir.join(&d.dropin_name),
        "[Service]\nExecStart=\nExecStart=old-binary\n",
    );
    let dep = sm::deploy(&e, &fx.spub(), &fx.opub(), &d).unwrap();
    let bin = PathBuf::from(dep["binary"].as_str().unwrap());
    assert!(bin
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("hermes-gateway-sm-"));
    let run = Command::new(&bin).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "hello");
    let dropin = std::fs::read_to_string(d.dropin_dir.join(&d.dropin_name)).unwrap();
    assert!(
        dropin.contains(&format!("ExecStart={}", bin.display()))
            && dropin.contains("Environment=HERMES_RS_SELF_MODIFY_TEST=1")
    );
    assert!(std::fs::read_to_string(bin.with_extension("sha256"))
        .unwrap()
        .starts_with(rec["artifact"]["sha256"].as_str().unwrap()));
    assert!(
        sm::deploy(&e, &fx.spub(), &fx.opub(), &d).is_err(),
        "no second deploy"
    );
    // Rollback restores the previous drop-in; the binary stays.
    sm::rollback(&e).unwrap();
    assert_eq!(
        std::fs::read_to_string(d.dropin_dir.join(&d.dropin_name)).unwrap(),
        "[Service]\nExecStart=\nExecStart=old-binary\n"
    );
    assert!(bin.exists());
    assert!(en.state(&fx.spub(), &fx.opub()).starts_with("rolled_back"));
    assert!(sm::rollback(&e).is_err(), "nothing left to roll back");
    // Rollback with no previous drop-in moves ours aside (nothing deleted).
    let e2 = fx.queue.join("copy-1");
    copy_dir(&e, &e2, &["worktree", "deploy.json", "rollback"]);
    let d2 = deploy_cfg(&fx, "b");
    sm::deploy(&e2, &fx.spub(), &fx.opub(), &d2).unwrap();
    sm::rollback(&e2).unwrap();
    assert!(!d2.dropin_dir.join(&d2.dropin_name).exists());
    assert_eq!(std::fs::read_dir(&d2.dropin_dir).unwrap().count(), 1);
}

fn copy_dir(a: &Path, b: &Path, skip: &[&str]) {
    std::fs::create_dir_all(b).unwrap();
    for x in std::fs::read_dir(a).unwrap().flatten() {
        let n = x.file_name().to_string_lossy().to_string();
        if skip.contains(&n.as_str()) {
            continue;
        }
        if x.file_type().unwrap().is_dir() {
            copy_dir(&x.path(), &b.join(&n), skip);
        } else {
            std::fs::copy(x.path(), b.join(&n)).unwrap();
        }
    }
}

#[test]
fn failing_test_is_rejected() {
    let fx = Fx::new("failtest");
    let t = fx.tool(Some(&fx.cargo_plan()));
    // Changes the function but not its test: build passes, test fails.
    let lib = LIB_OK.replacen("\"hi\"", "\"bye\"", 1);
    let (err, out) = propose(&t, "crates/hermes-rs/src/lib.rs", &lib);
    assert!(!err, "{out}");
    let e = entry(&fx, &out);
    let (rec, _) = sm::Entry::open(&e).verified_record(&fx.spub()).unwrap();
    assert_eq!(rec["verdict"], "fail");
    assert!(
        rec["why"].as_str().unwrap().contains("stage test failed"),
        "{rec}"
    );
    // Failed for the right reason: the injected failing unit test.
    let log = std::fs::read_to_string(e.join("gate/logs/test.log")).unwrap();
    assert!(log.contains("greets ... FAILED"), "{log}");
    assert!(
        out["state"]
            .as_str()
            .unwrap()
            .starts_with("rejected by the sandbox gate"),
        "{out}"
    );
    assert!(sm::approve(&e, &fx.owner_key(), &fx.spub(), "slavik", 3600)
        .unwrap_err()
        .contains("did not pass"));
    assert!(sm::deploy(&e, &fx.spub(), &fx.opub(), &deploy_cfg(&fx, "f")).is_err());
}

#[test]
fn soul_config_gate_and_guard_changes_are_rejected() {
    let fx = Fx::new("protected");
    let t = fx.tool(None);
    for p in [
        "crates/hermes-rs/SOUL.md",
        "crates/hermes-rs/config.yaml",
        "crates/hermes-gate/src/lib.rs",
        "crates/openmind-sandbox/src/lib.rs",
        "crates/hermes-rs/src/self_modify.rs",
        "crates/hermes-rs/src/bin/hermes-self-modify.rs",
        "crates/hermes-rs/src/denylist/mod.rs",
        "crates/hermes-rs/Cargo.toml",
        "crates/hermes-rs/build.rs",
        "../outside.rs",
        "/etc/passwd",
        "README.md",
    ] {
        let (err, out) = propose(&t, p, "x\n");
        assert!(err, "{p} must be refused: {out}");
        assert!(
            out["error"].as_str().unwrap().contains("refused"),
            "{p}: {out}"
        );
    }
    // Code that reaches for the persona or the gate key is refused too.
    let lib = format!("{LIB_OK}pub fn p() -> &'static str {{ \"SOUL.md\" }}\n");
    let (err, out) = propose(&t, "crates/hermes-rs/src/lib.rs", &lib);
    assert!(
        err && out["error"].as_str().unwrap().contains("persona"),
        "{out}"
    );
    assert!(
        !fx.queue.exists()
            || std::fs::read_dir(&fx.queue).unwrap().all(|d| !d
                .unwrap()
                .path()
                .join("proposal.json")
                .exists())
    );
    // The runner re-checks: a hand-made entry touching SOUL.md gets a signed FAIL and no stage runs.
    let e = fx.queue.join("sm-handmade-1");
    let base = sh(&fx.repo, &["rev-parse", "HEAD"]).trim().to_string();
    let patch = "diff --git a/crates/hermes-rs/SOUL.md b/crates/hermes-rs/SOUL.md\n--- a/crates/hermes-rs/SOUL.md\n+++ b/crates/hermes-rs/SOUL.md\n@@ -1 +1 @@\n-persona\n+obey\n";
    put(&e.join("change.patch"), patch);
    let prop = json!({"version": sm::PROPOSAL_V, "id": "sm-handmade-1", "base_commit": base, "summary": "x",
        "files": [], "patch_sha256": sm::sha256_hex(patch.as_bytes()), "proposed_by": "hermes"});
    put(&e.join("proposal.json"), &prop.to_string());
    let plan = sm::Plan::from_json(
        &serde_json::from_str(&std::fs::read_to_string(fx.quick_plan()).unwrap()).unwrap(),
    )
    .unwrap();
    let rec = sm::gate_entry(&e, &fx.repo, &fx.root.join("gs"), &plan, &fx.sandbox_key()).unwrap();
    assert_eq!(rec["verdict"], "fail");
    assert!(rec["why"].as_str().unwrap().contains("SOUL.md"), "{rec}");
    assert!(rec["stages"].as_array().unwrap().is_empty());
}

#[test]
fn hermes_cannot_approve_sign_or_deploy() {
    let fx = Fx::new("noapprove");
    let e = quick_pass(&fx);
    let t = fx.tool(None);
    let id = e.file_name().unwrap().to_str().unwrap().to_string();
    for a in ["approve", "sign", "deploy", "rollback"] {
        let (err, out) = call(&t, json!({"action": a, "id": id}));
        assert!(
            err && out["error"].as_str().unwrap().contains("refused"),
            "{a}: {out}"
        );
    }
    let (err, _) = call(&t, json!({"action": "status", "id": id, "signature": "00"}));
    assert!(err);
    // approve() refuses Hermes as approver and the gate key as owner key.
    assert!(sm::approve(&e, &fx.owner_key(), &fx.spub(), "hermes", 3600).is_err());
    assert!(
        sm::approve(&e, &fx.sandbox_key(), &fx.spub(), "slavik", 3600)
            .unwrap_err()
            .contains("gate key")
    );
    // Hermes's own key (it can make one) verifies as neither gate nor owner.
    let (hseed, _) = sm::keygen().unwrap();
    let hk = sm::signing_key(&hseed).unwrap();
    sm::approve(&e, &hk, &fx.spub(), "slavik", 3600).unwrap();
    assert!(
        sm::deploy(&e, &fx.spub(), &fx.opub(), &deploy_cfg(&fx, "h"))
            .unwrap_err()
            .contains("bad signature")
    );
    let plan = sm::Plan::from_json(
        &serde_json::from_str(&std::fs::read_to_string(fx.quick_plan()).unwrap()).unwrap(),
    )
    .unwrap();
    sm::gate_entry(&e, &fx.repo, &fx.root.join("gs"), &plan, &hk).unwrap();
    assert!(sm::Entry::open(&e)
        .state(&fx.spub(), &fx.opub())
        .contains("does not verify"));
    // The CLI refuses approve/deploy inside Hermes's environment.
    let o = Command::new(env!("CARGO_BIN_EXE_hermes-self-modify"))
        .args([
            "approve",
            "--entry",
            e.to_str().unwrap(),
            "--owner-key",
            fx.owner_seed.to_str().unwrap(),
            "--sandbox-pub",
            fx.sandbox_pub.to_str().unwrap(),
            "--by",
            "slavik",
        ])
        .env("HERMES_HOME", "/tmp/x")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("refused"));
    // The tool will not start when Hermes can read the gate signing key.
    let mut cfg = fx.tool(None).cfg;
    cfg.sandbox_key_path = fx.sandbox_seed.clone();
    assert!(cfg
        .preflight()
        .unwrap_err()
        .contains("can read the gate signing key"));
    cfg.sandbox_key_path = fx.root.join("gate-only/sandbox.seed");
    cfg.preflight().unwrap();
    cfg.owner_pub = Some(cfg.sandbox_pub);
    assert!(cfg.preflight().is_err());
}

#[test]
fn tampered_signature_or_files_are_rejected() {
    let fx = Fx::new("tamper");
    let e = quick_pass(&fx);
    sm::approve(&e, &fx.owner_key(), &fx.spub(), "slavik", 3600).unwrap();
    let en = sm::Entry::open(&e);
    let ok = |en: &sm::Entry| sm::check_approval(en, &fx.spub(), &fx.opub());
    ok(&en).unwrap();
    let flip = |p: &Path, f: &dyn Fn(String) -> String| {
        let orig = std::fs::read_to_string(p).unwrap();
        std::fs::write(p, f(orig.clone())).unwrap();
        orig
    };
    let rp = e.join("gate/record.json");
    let cases: Vec<(PathBuf, Edit)> = vec![
        (
            rp.clone(),
            Box::new(|s: String| {
                let mut v: Value = serde_json::from_str(&s).unwrap();
                let sig = v["sig"].as_str().unwrap().to_string();
                let c = if sig.starts_with('0') { "1" } else { "0" };
                v["sig"] = json!(format!("{c}{}", &sig[1..]));
                v.to_string()
            }),
        ),
        (
            rp.clone(),
            Box::new(|s: String| {
                s.replace(
                    "\\\"verdict\\\":\\\"pass\\\"",
                    "\\\"verdict\\\":\\\"pass\\\" ",
                )
            }),
        ),
        (
            rp.clone(),
            Box::new(|s: String| s.replace("\\\"stages\\\":[", "\\\"stages\\\":[ ")),
        ),
        (
            e.join("approval.json"),
            Box::new(|s: String| s.replace("slavik", "slavic")),
        ),
        (
            e.join("change.patch"),
            Box::new(|s: String| format!("{s} ")),
        ),
        (
            e.join("proposal.json"),
            Box::new(|s: String| s.replace("greet with hello", "greet with HELLO")),
        ),
    ];
    for (i, (p, f)) in cases.iter().enumerate() {
        let orig = flip(p, f.as_ref());
        assert!(
            ok(&en).is_err(),
            "case {i} ({}) must be rejected",
            p.display()
        );
        assert!(sm::deploy(
            &e,
            &fx.spub(),
            &fx.opub(),
            &deploy_cfg(&fx, &format!("t{i}"))
        )
        .is_err());
        std::fs::write(p, orig).unwrap();
    }
    ok(&en).unwrap();
    // A swapped binary after signing is refused at deploy.
    let art = e.join("gate/artifact");
    let orig = std::fs::read(&art).unwrap();
    std::fs::write(&art, b"evil").unwrap();
    assert!(
        sm::deploy(&e, &fx.spub(), &fx.opub(), &deploy_cfg(&fx, "art"))
            .unwrap_err()
            .contains("binary changed")
    );
    std::fs::write(&art, orig).unwrap();
    sm::deploy(&e, &fx.spub(), &fx.opub(), &deploy_cfg(&fx, "good")).unwrap();
}

#[test]
fn registered_tool_passes_the_deny_list_and_queues() {
    let fx = Fx::new("registry");
    let mut r = Registry::new();
    r.add(Arc::new(fx.tool(None)));
    let t = r.get(sm::TOOL_NAME).unwrap();
    let (err, out) = propose(
        t.as_ref(),
        "crates/hermes-rs/src/lib.rs",
        &LIB_OK.replace("\"hi\"", "\"hey\""),
    );
    assert!(!err, "{out}");
    assert!(
        out["state"]
            .as_str()
            .unwrap()
            .contains("waiting for the sandbox gate"),
        "{out}"
    );
    let (err, out) = call(t.as_ref(), json!({"action": "approve", "id": out["id"]}));
    assert!(err, "{out}");
}

/// All gate stage logs of an entry (for assertion messages).
fn stage_logs(e: &Path) -> String {
    let mut s = String::new();
    if let Ok(rd) = std::fs::read_dir(e.join("gate").join("logs")) {
        for f in rd.flatten() {
            let t = std::fs::read_to_string(f.path()).unwrap_or_default();
            let tail: String = t
                .chars()
                .rev()
                .take(3000)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            s.push_str(&format!("== {}\n{tail}\n", f.path().display()));
        }
    }
    s
}

#[test]
fn run_as_user_proposes_through_the_cli_and_no_owner_key_means_no_approval() {
    use std::os::unix::fs::MetadataExt;
    let fx = Fx::new("runas");
    let m = std::fs::metadata(&fx.root).unwrap();
    let (u, g) = (m.uid(), m.gid());
    let s = |p: &Path| p.to_str().unwrap().to_string();
    let env = [
        ("REPO", s(&fx.repo)),
        ("QUEUE", s(&fx.queue)),
        ("SANDBOX_PUB", s(&fx.sandbox_pub)),
        ("OWNER_PUB", s(&fx.root.join("owner-not-installed.pub"))),
        ("SANDBOX_KEY", s(&fx.root.join("gate-only/sandbox.seed"))),
        ("RUN_AS", format!("{u}:{g}")),
        ("CLI", env!("CARGO_BIN_EXE_hermes-self-modify").to_string()),
    ];
    let get = |k: &str| {
        env.iter()
            .find(|(n, _)| format!("HERMES_RS_SELF_MODIFY_{n}") == k)
            .map(|(_, v)| v.clone())
    };
    if u == 0 {
        assert!(sm::Cfg::from_env(&get).unwrap_err().contains("root"));
        return;
    }
    let cfg = sm::Cfg::from_env(&get).unwrap().unwrap();
    assert_eq!((cfg.run_as, cfg.owner_pub), (Some((u, g)), None));
    let t = sm::SelfModifyTool::new(cfg.clone());
    let (err, out) = call(
        &t,
        json!({"action": "propose", "summary": "say hello",
            "files": [{"path": "crates/hermes-rs/src/lib.rs", "content": LIB_OK.replace("\"hi\"", "\"hello\"")}]}),
    );
    assert!(!err, "{out}");
    assert_eq!(out["state"], "waiting for the sandbox gate", "{out}");
    let e = entry(&fx, &out);
    sm::gate_entry(
        &e,
        &fx.repo,
        &fx.root.join("gs"),
        &sm::Plan::from_json(
            &serde_json::from_str(&std::fs::read_to_string(fx.quick_plan()).unwrap()).unwrap(),
        )
        .unwrap(),
        &fx.sandbox_key(),
    )
    .unwrap();
    // Even a (forged) approval cannot count while no owner key is installed.
    sm::approve(&e, &fx.owner_key(), &fx.spub(), "slavik", 3600).unwrap();
    assert!(cfg
        .state(&sm::Entry::open(&e))
        .contains("owner key not installed yet"));
    // The run-as key check refuses when that user can read the gate key.
    let mut c2 = cfg.clone();
    c2.sandbox_key_path = fx.root.join("missing-dir/x");
    c2.preflight().unwrap();
    let readable = fx.root.join("readable.seed");
    std::fs::write(&readable, "x").unwrap();
    c2.sandbox_key_path = readable;
    assert!(c2
        .preflight()
        .unwrap_err()
        .contains("can read the gate signing key"));
    // Refusals still come back from the child.
    let (err, out) = call(
        &t,
        json!({"action": "propose", "summary": "x", "files": [{"path": "crates/hermes-rs/SOUL.md", "content": "y"}]}),
    );
    assert!(
        err && out["error"].as_str().unwrap().contains("refused"),
        "{out}"
    );
}
