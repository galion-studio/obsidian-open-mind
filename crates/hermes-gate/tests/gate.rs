// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! End-to-end: proposal -> checks -> sandbox deploy (real openmind-sandbox
//! runs, real git) -> signed record -> owner approval -> apply to a local
//! test target. Plus tamper, escape and live-root refusals. No network, no
//! model calls, nothing outside a temp dir.

use hermes_gate::{apply, checks, git::git, proposal::Proposal, record, stage, tier};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// A public test address, built at run time so no literal sits in the tree.
fn pub_ip(a: u8) -> String {
    [a, a, a, a].map(|x| x.to_string()).join(".")
}

fn tmpdir(tag: &str) -> PathBuf {
    let mut r = [0u8; 6];
    getrandom::getrandom(&mut r).unwrap();
    let d = std::env::temp_dir().join(format!("hermes-gate-{tag}-{}", hermes_gate::hex(&r)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn w(dir: &Path, f: &str, s: &str) {
    let p = dir.join(f);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

/// A tiny "platform": a service script, its build, tests and a live-style
/// deploy that boots the built service and probes its health file.
fn repo() -> PathBuf {
    let d = tmpdir("repo");
    git(&d, &["init", "-q", "-b", "main"], None).unwrap();
    w(&d, "src/app.sh", "#!/bin/sh\necho \"version 1\"\n");
    w(
        &d,
        "build.sh",
        "set -e\nmkdir -p out\ncp src/app.sh out/app\necho built\n",
    );
    w(&d, "test.sh", "set -e\nout=$(sh out/app)\ncase \"$out\" in version*) echo tests ok;; *) echo bad; exit 1;; esac\n");
    // Live-style deploy: boot the service in the background inside the
    // sandbox, wait for its health file, check it, stop it.
    w(
        &d,
        "deploy.sh",
        "set -e\nrm -f .gate-tmp/health\n( sh out/app > .gate-tmp/health; sleep 30 ) &\npid=$!\ni=0\nwhile [ ! -s .gate-tmp/health ] && [ $i -lt 50 ]; do sleep 0.1; i=$((i+1)); done\nkill $pid 2>/dev/null || true\ngrep -q version .gate-tmp/health\necho deploy smoke ok: $(cat .gate-tmp/health)\n",
    );
    git(&d, &["add", "-A"], None).unwrap();
    git(&d, &["commit", "-q", "-m", "base"], None).unwrap();
    d
}

fn plan() -> stage::Plan {
    stage::Plan::from_json(&json!({"stages": [
        {"name": "build", "argv": ["/bin/sh", "build.sh"], "timeout_s": 30, "mem_mb": 256},
        {"name": "test", "argv": ["/bin/sh", "test.sh"], "timeout_s": 30, "mem_mb": 256},
        {"name": "deploy", "argv": ["/bin/sh", "deploy.sh"], "timeout_s": 30, "mem_mb": 256}
    ]}))
    .unwrap()
}

/// Hermes's side: edit a copy, turn the diff into a proposal.
fn propose(base: &Path, id: &str, edit: &dyn Fn(&Path)) -> Proposal {
    let wt = tmpdir("hermes-wt");
    std::fs::remove_dir_all(&wt).unwrap();
    git(
        base,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            wt.to_str().unwrap(),
            "HEAD",
        ],
        None,
    )
    .unwrap();
    edit(&wt);
    let p = Proposal::from_worktree(&wt, id, "test change", "high").unwrap();
    git(
        base,
        &["worktree", "remove", "--force", wt.to_str().unwrap()],
        None,
    )
    .unwrap();
    p
}

fn sandbox_usable() -> bool {
    let f = openmind_sandbox::features();
    let ok = f.landlock_abi >= 1 && (f.userns || f.landlock_abi >= 4);
    if !ok {
        eprintln!(
            "SKIP: no Landlock/userns on this host (landlock_abi={}, userns={})",
            f.landlock_abi, f.userns
        );
    }
    ok
}

fn gate(
    r: &Path,
    p: &Proposal,
    pl: &stage::Plan,
    sk: &ed25519_dalek::SigningKey,
) -> (Value, Value) {
    let cs = checks::run(p);
    let rep = if checks::all_ok(&cs) {
        Some(stage::sandbox_deploy(r, &tmpdir("scratch"), p, pl).unwrap())
    } else {
        None
    };
    let rec = record::build(p, pl, &cs, rep.as_ref());
    (rec.clone(), record::sign(&rec, sk))
}

fn keys() -> (ed25519_dalek::SigningKey, ed25519_dalek::SigningKey) {
    let (s1, _) = record::keygen().unwrap();
    let (s2, _) = record::keygen().unwrap();
    (
        record::signing_key(&s1).unwrap(),
        record::signing_key(&s2).unwrap(),
    )
}

#[test]
fn full_flow_sandbox_deploy_sign_approve_apply() {
    if !sandbox_usable() {
        return;
    }
    let r = repo();
    let p = propose(&r, "bump-v2", &|wt| {
        w(wt, "src/app.sh", "#!/bin/sh\necho \"version 2\"\n")
    });
    let (sbx, owner) = keys();
    let (rec, signed) = gate(&r, &p, &plan(), &sbx);
    eprintln!("record: {}", serde_json::to_string_pretty(&rec).unwrap());
    assert_eq!(rec["verdict"], "pass", "{rec}");
    assert_eq!(rec["stages"].as_array().unwrap().len(), 3);
    assert!(rec["stages"][2]["stdout_tail"]
        .as_str()
        .unwrap()
        .contains("deploy smoke ok: version 2"));
    // verify, approve, apply to a separate target clone at the same base
    let v = record::verify(&signed, &sbx.verifying_key()).unwrap();
    assert_eq!(v["proposal_sha256"], p.sha256().as_str());
    let target = tmpdir("target");
    std::fs::remove_dir_all(&target).unwrap();
    git(
        &r,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            target.to_str().unwrap(),
            "HEAD",
        ],
        None,
    )
    .unwrap();
    let head = hermes_gate::git::head(&target).unwrap();
    let now = hermes_rs::time::now_secs();
    let appr = apply::approve(
        &owner,
        &p.sha256(),
        &record::record_sha256(&signed),
        &head,
        now + 600,
    );
    let auth = apply::authorize(
        &signed,
        &sbx.verifying_key(),
        &p,
        &appr,
        &owner.verifying_key(),
        &head,
        now,
    )
    .unwrap();
    let tree = apply::apply(&target, &auth, &p).unwrap();
    assert_eq!(tree, rec["result_tree"].as_str().unwrap());
    assert!(std::fs::read_to_string(target.join("src/app.sh"))
        .unwrap()
        .contains("version 2"));
    // the source repo HEAD is untouched (nothing committed, nothing pushed)
    assert_eq!(hermes_gate::git::head(&r).unwrap(), p.base_commit);
}

#[test]
fn failing_test_stage_gives_fail_record() {
    if !sandbox_usable() {
        return;
    }
    let r = repo();
    let p = propose(&r, "break-it", &|wt| {
        w(wt, "src/app.sh", "#!/bin/sh\necho broken\n")
    });
    let (sbx, _) = keys();
    let (rec, _) = gate(&r, &p, &plan(), &sbx);
    assert_eq!(rec["verdict"], "fail");
    assert_eq!(rec["stages"][1]["name"], "test");
    assert_eq!(rec["stages"][1]["ok"], false);
    assert_eq!(
        rec["stages"].as_array().unwrap().len(),
        2,
        "stops at the first failed stage"
    );
}

#[test]
fn sandbox_blocks_escape_writes_and_network_and_hides_live_roots() {
    if !sandbox_usable() {
        return;
    }
    let outside = tmpdir("outside");
    let marker = outside.join("escaped");
    let r = repo();
    assert!(std::path::Path::new(&std::env::var("HOME").unwrap())
        .read_dir()
        .unwrap()
        .next()
        .is_some());
    let script = format!(
        "echo x > {m} 2>/dev/null && echo WROTE-OUTSIDE\n\
         (exec 3<>/dev/tcp/{ip}/80) 2>/dev/null && echo NET-OPEN\n\
         ls /opt/galion 2>/dev/null | head -1 | grep -q . && echo SAW-LIVE\n\
         ls {h} 2>/dev/null | head -1 | grep -q . && echo SAW-HOME\n\
         cat {r}/.git/HEAD 2>/dev/null && echo SAW-REPO-GIT\n\
         echo probe-done\n",
        m = marker.display(),
        ip = pub_ip(1),
        h = std::env::var("HOME").unwrap_or_else(|_| "/root".into()),
        r = r.display()
    );
    let p = propose(&r, "escape", &|wt| w(wt, "escape.sh", &script));
    let pl = stage::Plan::from_json(&json!({"stages": [
        {"name": "escape", "argv": ["/bin/bash", "escape.sh"], "timeout_s": 20, "mem_mb": 128}
    ]}))
    .unwrap();
    // The static checks already refuse this patch (live root, address);
    // run the sandbox deploy directly to prove the second layer holds too.
    assert!(!checks::all_ok(&checks::run(&p)));
    let rep = stage::sandbox_deploy(&r, &tmpdir("scratch"), &p, &pl).unwrap();
    let rec = json!({"stages": rep.stages.iter().map(|s| s.to_json()).collect::<Vec<_>>()});
    let out = rec["stages"][0]["stdout_tail"].as_str().unwrap_or("");
    eprintln!("escape stage: {}", rec["stages"][0]);
    assert!(out.contains("probe-done"), "{rec}");
    assert!(
        !out.contains("WROTE-OUTSIDE") && !marker.exists(),
        "write escaped the sandbox"
    );
    assert!(
        !out.contains("NET-OPEN"),
        "network reachable from the sandbox"
    );
    assert!(!out.contains("SAW-LIVE"), "live root visible");
    assert!(!out.contains("SAW-HOME"), "the host HOME is visible");
    assert!(
        !out.contains("SAW-REPO-GIT"),
        "the source repo's .git is visible"
    );
    assert!(
        rec["stages"][0]["missing_layers"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{rec}"
    );
}

type Edit = Box<dyn Fn(&Path)>;

#[test]
fn static_checks_refuse_before_any_deploy() {
    let r = repo();
    let cases: Vec<(&str, Edit, &str)> = vec![
        (
            "ci",
            Box::new(|wt: &Path| w(wt, ".github/workflows/x.yml", "on: push\n")),
            "paths",
        ),
        (
            "unit",
            Box::new(|wt: &Path| w(wt, "deploy/hermes.service", "[Service]\n")),
            "paths",
        ),
        (
            "selfgate",
            Box::new(|wt: &Path| w(wt, "crates/hermes-gate/src/lib.rs", "//\n")),
            "paths",
        ),
        (
            "push",
            Box::new(|wt: &Path| w(wt, "src/app.sh", "git push origin main\n")),
            "no_live_or_secrets",
        ),
        (
            "live",
            Box::new(|wt: &Path| w(wt, "src/app.sh", "cp x /opt/galion/rust/bin/\n")),
            "no_live_or_secrets",
        ),
        (
            "key",
            Box::new(|wt: &Path| {
                w(
                    wt,
                    "src/k.txt",
                    &format!("{}BEGIN {} KEY-----\n", "-".repeat(5), "PRIVATE"),
                )
            }),
            "no_live_or_secrets",
        ),
        (
            "ip",
            Box::new(|wt: &Path| w(wt, "src/app.sh", &format!("curl http://{}/\n", pub_ip(8)))),
            "no_host_addresses",
        ),
        (
            "unsafe",
            Box::new(|wt: &Path| {
                w(
                    wt,
                    "src/x.rs",
                    "fn f() { unsafe { g() } }\nunsafe fn g() {}\n",
                )
            }),
            "no_new_unsafe",
        ),
    ];
    let (sbx, _) = keys();
    for (id, edit, check) in cases {
        let p = propose(&r, id, &*edit);
        let (rec, _) = gate(&r, &p, &plan(), &sbx);
        assert_eq!(rec["verdict"], "fail", "{id}");
        assert_eq!(
            rec["applied"], false,
            "{id}: must not reach the sandbox deploy"
        );
        let c = rec["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == check)
            .unwrap();
        assert_eq!(c["ok"], false, "{id}: {rec}");
        assert!(
            !rec.to_string().contains(&pub_ip(8)),
            "the record never echoes an address"
        );
    }
    // a clean change passes every static check
    let p = propose(&r, "clean", &|wt| {
        w(wt, "src/app.sh", "#!/bin/sh\necho \"version 3\"\n")
    });
    assert!(checks::all_ok(&checks::run(&p)), "{:?}", checks::run(&p));
}

#[test]
fn checks_catch_binary_mode_and_absolute_paths() {
    let mk = |patch: &str| Proposal {
        id: "x".into(),
        base_commit: "0".repeat(40),
        patch: patch.into(),
        summary: String::new(),
        model_tier: "low".into(),
        created: String::new(),
    };
    let fails = |p: &Proposal, n: &str| checks::run(p).iter().any(|c| c.name == n && !c.ok);
    assert!(fails(
        &mk("diff --git a/a b/a\nGIT binary patch\n"),
        "text_only"
    ));
    assert!(fails(
        &mk("diff --git a/a b/a\nnew file mode 100755\n"),
        "mode_changes"
    ));
    assert!(fails(
        &mk("diff --git a/l b/l\nnew file mode 120000\n"),
        "mode_changes"
    ));
    assert!(fails(&mk("--- /etc/passwd\n+++ /etc/passwd\n"), "paths"));
    assert!(fails(&mk("diff --git a/../x b/../x\n"), "paths"));
    assert!(fails(&mk(""), "size"));
    assert_eq!(
        checks::public_ipv4("bind 127.0.0.1:8642 and 10.0.0.1"),
        None
    );
    assert!(checks::public_ipv4(&format!("x {} y", pub_ip(8))).is_some());
}

#[test]
fn tampering_and_wrong_approvals_are_refused() {
    let p = Proposal {
        id: "t".into(),
        base_commit: "a".repeat(40),
        patch: "diff --git a/f b/f\n".into(),
        summary: String::new(),
        model_tier: "high".into(),
        created: String::new(),
    };
    let (sbx, owner) = keys();
    let rec = json!({"version": record::VERSION, "proposal_sha256": p.sha256(), "base_commit": p.base_commit,
                     "result_tree": "b".repeat(40), "verdict": "pass"});
    let signed = record::sign(&rec, &sbx);
    let head = p.base_commit.clone();
    let now = 1_000_000;
    let rsha = record::record_sha256(&signed);
    let good = apply::approve(&owner, &p.sha256(), &rsha, &head, now + 60);
    let auth =
        |s: &Value, a: &Value, h: &str, t: i64, o: &ed25519_dalek::SigningKey, pp: &Proposal| {
            apply::authorize(s, &sbx.verifying_key(), pp, a, &o.verifying_key(), h, t)
        };
    assert!(auth(&signed, &good, &head, now, &owner, &p).is_ok());
    // record text edited after signing
    let mut bad = signed.clone();
    bad["record_json"] = Value::String(
        signed["record_json"]
            .as_str()
            .unwrap()
            .replace("\"fail\"", "\"pass\"")
            .replace("bbbb", "cccc"),
    );
    assert!(auth(&bad, &good, &head, now, &owner, &p)
        .unwrap_err()
        .contains("bad signature"));
    // fail verdict, properly signed
    let mut f = rec.clone();
    f["verdict"] = json!("fail");
    let fs = record::sign(&f, &sbx);
    let fa = apply::approve(
        &owner,
        &p.sha256(),
        &record::record_sha256(&fs),
        &head,
        now + 60,
    );
    assert!(auth(&fs, &fa, &head, now, &owner, &p)
        .unwrap_err()
        .contains("not pass"));
    // record signed by someone else (e.g. Hermes signing its own record)
    let forged = record::sign(&rec, &owner);
    assert!(auth(&forged, &good, &head, now, &owner, &p).is_err());
    // approval expired, wrong head, signed by the sandbox key, other proposal
    assert!(auth(&signed, &good, &head, now + 61, &owner, &p)
        .unwrap_err()
        .contains("expired"));
    assert!(auth(&signed, &good, &"c".repeat(40), now, &owner, &p).is_err());
    let by_sbx = apply::approve(&sbx, &p.sha256(), &rsha, &head, now + 60);
    assert!(auth(&signed, &by_sbx, &head, now, &owner, &p)
        .unwrap_err()
        .contains("owner signature"));
    let mut p2 = p.clone();
    p2.patch.push_str("+evil\n");
    assert!(auth(&signed, &good, &head, now, &owner, &p2)
        .unwrap_err()
        .contains("different proposal"));
    // one key for both roles is refused
    assert!(auth(&signed, &good, &head, now, &sbx, &p)
        .unwrap_err()
        .contains("must differ"));
}

#[test]
fn apply_refuses_live_roots_and_moved_heads() {
    let p = Proposal {
        id: "l".into(),
        base_commit: "a".repeat(40),
        patch: String::new(),
        summary: String::new(),
        model_tier: "high".into(),
        created: String::new(),
    };
    let a = apply::Authorized {
        proposal_sha256: p.sha256(),
        base_commit: p.base_commit.clone(),
        result_tree: "b".repeat(40),
    };
    for live in [
        "/opt/galion/openmind",
        "/root/.hermes",
        "/etc/systemd/system",
        "/var/lib/hermes-rs",
    ] {
        let e = apply::apply(Path::new(live), &a, &p).unwrap_err();
        assert!(e.contains("live apply is not enabled"), "{live}: {e}");
    }
    let r = repo();
    let e = apply::apply(&r, &a, &p).unwrap_err();
    assert!(e.contains("HEAD moved"), "{e}");
    let e = stage::sandbox_deploy(Path::new("/opt/galion/openmind"), &tmpdir("s"), &p, &plan())
        .unwrap_err();
    assert!(e.contains("refuses live roots"), "{e}");
    for live in ["/opt/galion/openmind", "/opt/galion/openmind/../openmind"] {
        let e = hermes_gate::git::head(Path::new(live)).unwrap_err();
        assert!(e.contains("git in a live root is never run"), "{live}: {e}");
    }
}

#[test]
fn plan_env_and_loopback_ports_are_allowlisted() {
    let ok = stage::Plan::from_json(&json!({
        "env": {"CARGO_HOME": "/home/x/.cargo"},
        "stages": [{"name": "deploy", "argv": ["true"], "loopback_ports": [18642]}]
    }))
    .unwrap();
    assert_eq!(ok.stages[0].loopback_ports, vec![18642]);
    let pol = stage::policy(Path::new("/tmp/w"), &ok, &ok.stages[0]);
    assert_eq!(pol.loopback_tcp_ports, vec![18642]);
    assert!(!pol.net && pol.require_net_isolation);
    for bad in [
        json!({"env": {"LD_PRELOAD": "/tmp/x.so"}, "stages": [{"name": "a", "argv": ["true"]}]}),
        json!({"env": {"CARGO_HOME": "/opt/galion/x"}, "stages": [{"name": "a", "argv": ["true"]}]}),
        json!({"stages": [{"name": "a", "argv": ["true"], "loopback_ports": [22]}]}),
        json!({"stages": [{"name": "a", "argv": ["true"], "loopback_ports": [70000]}]}),
    ] {
        assert!(stage::Plan::from_json(&bad).is_err(), "{bad}");
    }
    // The ports are part of the plan hash the record signs.
    let other = stage::Plan::from_json(&json!({
        "env": {"CARGO_HOME": "/home/x/.cargo"},
        "stages": [{"name": "deploy", "argv": ["true"], "loopback_ports": [18643]}]
    }))
    .unwrap();
    assert_ne!(ok.sha256(), other.sha256());
}

#[test]
fn plan_refuses_live_reads_and_unknown_layers() {
    assert!(stage::Plan::from_json(
        &json!({"stages": [{"name": "a", "argv": ["true"]}], "extra_read": ["/opt/galion/hermes"]})
    )
    .is_err());
    assert!(stage::Plan::from_json(
        &json!({"stages": [{"name": "a", "argv": ["true"]}], "require_layers": ["vibes"]})
    )
    .is_err());
    assert!(stage::Plan::from_json(&json!({"stages": []})).is_err());
}

#[test]
fn model_tiers_route_deep_high_routine_low_gate_none() {
    use tier::{may_call, tier_for, Step, Tier};
    assert_eq!(tier_for(Step::Propose), Tier::High);
    assert_eq!(tier_for(Step::ReviewFailure), Tier::High);
    assert_eq!(tier_for(Step::Summarize), Tier::Low);
    assert_eq!(may_call(Step::Gate, 0.0, 0.0).unwrap(), None);
    assert_eq!(may_call(Step::Propose, 5.0, 0.30).unwrap(), Some("boss"));
    assert_eq!(
        may_call(Step::Summarize, 5.0, 0.01).unwrap(),
        Some("worker")
    );
    // routine work stops first and leaves the deep reserve
    assert!(may_call(Step::Summarize, 1.50, 0.01).is_err());
    assert!(may_call(Step::Propose, 1.50, 0.30).is_ok());
    assert!(may_call(Step::Propose, 0.60, 0.30).is_err());
    assert!(may_call(Step::Propose, f64::NAN, 0.1).is_err());
}
