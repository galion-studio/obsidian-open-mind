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

/// The gate needs user namespaces (for the netns/pidns/mountns) and
/// seccomp; it never needs Landlock.
fn sandbox_usable() -> bool {
    let f = openmind_sandbox::features();
    if !f.userns {
        eprintln!("SKIP: no user namespaces on this host");
    }
    f.userns
}

/// The toy repo keeps its code under src/ (the real default is crates/, docs/).
fn pol() -> checks::CheckPolicy {
    checks::CheckPolicy {
        allow_tree: vec!["src/".into()],
        allow_caps: vec![],
    }
}

fn target(r: &Path) -> apply::ApplyTarget {
    let st = tmpdir("staging");
    apply::ApplyTarget {
        source: r.to_path_buf(),
        ledger: st.join("applied.ledger"),
        staging_root: st,
    }
}

fn gate(
    r: &Path,
    p: &Proposal,
    pl: &stage::Plan,
    sk: &ed25519_dalek::SigningKey,
) -> (Value, Value) {
    let cs = checks::run(p, r, &pol());
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
    assert_eq!(rec["checks_passed"], 7);
    assert_eq!(rec["sandbox"]["landlock"], "not used");
    assert_eq!(rec["stages"][0]["layers"]["seccomp"], "on", "{rec}");
    assert_eq!(
        rec["stages"][0]["layers"]["landlock"],
        "off: disabled by policy"
    );
    // verify, approve, apply to a fresh staging copy (never in place)
    let v = record::verify(&signed, &sbx.verifying_key()).unwrap();
    assert_eq!(v["proposal_sha256"], p.sha256().as_str());
    let t = target(&r);
    let head = hermes_gate::git::head(&r).unwrap();
    let now = hermes_rs::time::now_secs();
    let appr = apply::approve_labeled(
        &owner,
        &p.sha256(),
        &record::record_sha256(&signed),
        &head,
        now + 600,
        "test approval",
    );
    let done = apply::apply_staged(
        &t,
        &signed,
        &sbx.verifying_key(),
        &p,
        &appr,
        &owner.verifying_key(),
        now,
    )
    .unwrap();
    assert_eq!(done.tree, rec["result_tree"].as_str().unwrap());
    assert!(done.staging_copy.starts_with(&t.staging_root));
    assert!(
        std::fs::read_to_string(done.staging_copy.join("src/app.sh"))
            .unwrap()
            .contains("version 2")
    );
    // the source tree is untouched (not applied in place, nothing committed)
    assert_eq!(hermes_gate::git::head(&r).unwrap(), p.base_commit);
    assert!(std::fs::read_to_string(r.join("src/app.sh"))
        .unwrap()
        .contains("version 1"));
    // replay: the same approval a second time is refused
    let e = apply::apply_staged(
        &t,
        &signed,
        &sbx.verifying_key(),
        &p,
        &appr,
        &owner.verifying_key(),
        now,
    )
    .unwrap_err();
    assert!(e.contains("replay refused"), "{e}");
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
         unshare -U true 2>/dev/null && echo UNSHARE-OK\n\
         grep -q '^Seccomp:.*2' /proc/self/status && echo SECCOMP-ON\n\
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
    assert!(!checks::all_ok(&checks::run(&p, &r, &pol())));
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
    assert!(!out.contains("UNSHARE-OK"), "seccomp let unshare through");
    assert!(out.contains("SECCOMP-ON"), "{out}");
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
            "protected_targets",
        ),
        (
            "unit",
            Box::new(|wt: &Path| w(wt, "src/hermes.service", "[Service]\n")),
            "protected_targets",
        ),
        (
            "selfgate",
            Box::new(|wt: &Path| w(wt, "crates/hermes-gate/src/lib.rs", "//\n")),
            "protected_targets",
        ),
        (
            "push",
            Box::new(|wt: &Path| w(wt, "src/app.sh", "git push origin main\n")),
            "protected_targets",
        ),
        (
            "live",
            Box::new(|wt: &Path| w(wt, "src/app.sh", "cp x /opt/galion/rust/bin/\n")),
            "protected_targets",
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
            "no_secrets",
        ),
        (
            "ip",
            Box::new(|wt: &Path| w(wt, "src/app.sh", &format!("curl http://{}/\n", pub_ip(8)))),
            "no_secrets",
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
            "no_new_capabilities",
        ),
        (
            "spawn",
            Box::new(|wt: &Path| {
                w(
                    wt,
                    "src/y.rs",
                    "fn f() { std::process::Command::new(\"sh\"); }\n",
                )
            }),
            "no_new_capabilities",
        ),
        (
            "net",
            Box::new(|wt: &Path| w(wt, "src/z.rs", "use std::net::TcpStream;\n")),
            "no_new_capabilities",
        ),
        (
            "outside",
            Box::new(|wt: &Path| w(wt, "other/readme.md", "hi\n")),
            "allowed_tree",
        ),
        (
            "blob",
            Box::new(|wt: &Path| w(wt, "src/b.txt", &format!("{}\n", "QUJD".repeat(60)))),
            "text_only",
        ),
        (
            "secretfile",
            Box::new(|wt: &Path| w(wt, "src/my_secret.txt", "x\n")),
            "protected_targets",
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
    assert!(
        checks::all_ok(&checks::run(&p, &r, &pol())),
        "{:?}",
        checks::run(&p, &r, &pol())
    );
    // the operator flag (never the proposal) unlocks a capability
    let p = propose(&r, "net-ok", &|wt| {
        w(wt, "src/z.rs", "use std::net::TcpStream;\n")
    });
    let mut allow = pol();
    allow.allow_caps = vec!["net".into()];
    assert!(checks::all_ok(&checks::run(&p, &r, &allow)));
    // a patch that does not apply to its base fails check 1
    let mut bad = p.clone();
    bad.patch = bad.patch.replace("--- /dev/null", "--- a/src/z.rs");
    let c = checks::run(&bad, &r, &allow);
    assert!(!c[0].ok && c[0].name == "applies_to_base", "{c:?}");
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
    let r = repo();
    let fails = |p: &Proposal, n: &str| {
        checks::run(p, &r, &pol())
            .iter()
            .any(|c| c.name == n && !c.ok)
    };
    assert!(fails(
        &mk("diff --git a/a b/a\nGIT binary patch\n"),
        "text_only"
    ));
    assert!(fails(
        &mk("diff --git a/a b/a\nnew file mode 100755\n"),
        "text_only"
    ));
    assert!(fails(
        &mk("diff --git a/l b/l\nnew file mode 120000\n"),
        "text_only"
    ));
    assert!(fails(
        &mk("--- /etc/passwd\n+++ /etc/passwd\n"),
        "allowed_tree"
    ));
    assert!(fails(&mk("diff --git a/../x b/../x\n"), "allowed_tree"));
    assert!(fails(&mk(""), "size_limits"));
    assert!(fails(
        &mk("diff --git a/src/a b/src/a\n"),
        "applies_to_base"
    ));
    assert_eq!(checks::run(&mk(""), &r, &pol()).len(), 7);
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
fn apply_refuses_live_paths_before_running_any_command() {
    let (sbx, owner) = keys();
    let p = Proposal {
        id: "l".into(),
        base_commit: "a".repeat(40),
        patch: "diff --git a/src/f b/src/f\n".into(),
        summary: String::new(),
        model_tier: "high".into(),
        created: String::new(),
    };
    let ok_staging = tmpdir("st");
    for live in [
        "/opt/galion/openmind",
        "/opt/galion/rust/src",
        "/opt/galion/hermes",
        "/root/.hermes",
        "/etc/systemd/system",
        "/var/lib/hermes-rs",
        "/var/lib/galion-gate/../hermes-rs",
    ] {
        for t in [
            apply::ApplyTarget {
                source: PathBuf::from(live),
                staging_root: ok_staging.clone(),
                ledger: ok_staging.join("l"),
            },
            apply::ApplyTarget {
                source: tmpdir("src"),
                staging_root: PathBuf::from(live),
                ledger: ok_staging.join("l"),
            },
        ] {
            let before = hermes_gate::git::spawn_count();
            let e = apply::apply_staged(
                &t,
                &json!({}),
                &sbx.verifying_key(),
                &p,
                &json!({"sig": "00"}),
                &owner.verifying_key(),
                0,
            )
            .unwrap_err();
            assert!(e.contains("live path refused"), "{live}: {e}");
            assert_eq!(
                hermes_gate::git::spawn_count(),
                before,
                "{live}: git was started for a refused path"
            );
        }
    }
    // the gate home itself is not live, but the always-live trees are
    assert!(!stage::under_live_root(Path::new(
        "/var/lib/galion-gate/staging"
    )));
    assert!(stage::under_live_root(Path::new("/opt/galion/openmind/x")));
    // in place (staging inside the source) is refused too
    let r = repo();
    let t = apply::ApplyTarget {
        source: r.clone(),
        staging_root: r.join("staging"),
        ledger: ok_staging.join("l"),
    };
    let e = apply::refuse_live(&t).unwrap_err();
    assert!(e.contains("no in-place apply"), "{e}");
    let e = stage::sandbox_deploy(Path::new("/opt/galion/openmind"), &tmpdir("s"), &p, &plan())
        .unwrap_err();
    assert!(e.contains("refuses live roots"), "{e}");
    for live in ["/opt/galion/openmind", "/opt/galion/openmind/../openmind"] {
        let before = hermes_gate::git::spawn_count();
        let e = hermes_gate::git::head(Path::new(live)).unwrap_err();
        assert!(e.contains("git in a live root is never run"), "{live}: {e}");
        assert_eq!(hermes_gate::git::spawn_count(), before);
    }
}

/// A real record + approval for the refusal tests below (no sandbox run:
/// the record is signed directly, the apply path is what is tested).
fn signed_case() -> (
    PathBuf,
    Proposal,
    Value,
    ed25519_dalek::SigningKey,
    ed25519_dalek::SigningKey,
    String,
) {
    let r = repo();
    let p = propose(&r, "case", &|wt| {
        w(wt, "src/app.sh", "#!/bin/sh\necho \"version 9\"\n")
    });
    // the tree the "sandbox" saw: apply in a scratch copy
    let t = target(&r);
    let tree = {
        let a = apply::Authorized {
            proposal_sha256: p.sha256(),
            base_commit: p.base_commit.clone(),
            result_tree: "0".repeat(40),
            approval_id: String::new(),
            label: String::new(),
        };
        let e = apply::apply_to_copy(&t, &a, &p).unwrap_err();
        assert!(e.contains("differs"), "{e}");
        // read the real tree from the reversed copy by re-applying
        let copy = std::fs::read_dir(&t.staging_root)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| p.is_dir())
            .unwrap();
        git(&copy, &["apply", "--index", "-"], Some(p.patch.as_bytes())).unwrap();
        git(&copy, &["write-tree"], None)
            .unwrap()
            .trim()
            .to_string()
    };
    let (sbx, owner) = keys();
    let rec = json!({"version": record::VERSION, "proposal_sha256": p.sha256(),
                     "patch_sha256": hermes_gate::sha256_hex(p.patch.as_bytes()),
                     "base_commit": p.base_commit, "result_tree": tree, "verdict": "pass"});
    (r, p, record::sign(&rec, &sbx), sbx, owner, tree)
}

#[test]
fn refusal_no_approval() {
    let (r, p, signed, sbx, owner, _) = signed_case();
    let e = apply::apply_staged(
        &target(&r),
        &signed,
        &sbx.verifying_key(),
        &p,
        &Value::Null,
        &owner.verifying_key(),
        0,
    )
    .unwrap_err();
    assert!(e.contains("no owner approval"), "{e}");
}

#[test]
fn refusal_sandbox_key_used_as_owner_key() {
    let (r, p, signed, sbx, _owner, _) = signed_case();
    let head = hermes_gate::git::head(&r).unwrap();
    let now = hermes_rs::time::now_secs();
    let by_sbx = apply::approve(
        &sbx,
        &p.sha256(),
        &record::record_sha256(&signed),
        &head,
        now + 60,
    );
    let e = apply::apply_staged(
        &target(&r),
        &signed,
        &sbx.verifying_key(),
        &p,
        &by_sbx,
        &sbx.verifying_key(),
        now,
    )
    .unwrap_err();
    assert!(e.contains("must differ"), "{e}");
}

fn ctx() -> (
    PathBuf,
    Proposal,
    Value,
    ed25519_dalek::SigningKey,
    ed25519_dalek::SigningKey,
    String,
    String,
    i64,
    String,
) {
    let (r, p, signed, sbx, owner, tree) = signed_case();
    let head = hermes_gate::git::head(&r).unwrap();
    let now = hermes_rs::time::now_secs();
    let rsha = record::record_sha256(&signed);
    (r, p, signed, sbx, owner, tree, head, now, rsha)
}

#[test]
fn refusal_expired_approval() {
    let (r, p, signed, sbx, owner, _, head, now, rsha) = ctx();
    let old = apply::approve(&owner, &p.sha256(), &rsha, &head, now - 1);
    let e = apply::apply_staged(
        &target(&r),
        &signed,
        &sbx.verifying_key(),
        &p,
        &old,
        &owner.verifying_key(),
        now,
    )
    .unwrap_err();
    assert!(e.contains("expired"), "{e}");
}

#[test]
fn refusal_checksum_mismatch() {
    let (r, p, signed, sbx, owner, _, head, now, rsha) = ctx();
    // a proposal file whose patch no longer matches its patch_sha256
    let mut pj = p.to_json();
    pj["patch"] = json!(format!("{}+x\n", p.patch));
    assert!(Proposal::from_json(&pj)
        .unwrap_err()
        .contains("patch_sha256"));
    // a changed patch (with a fresh checksum) against the old record/approval
    let mut p2 = p.clone();
    p2.patch = p.patch.replace("version 9", "version 8");
    let good = apply::approve(&owner, &p.sha256(), &rsha, &head, now + 60);
    let e = apply::apply_staged(
        &target(&r),
        &signed,
        &sbx.verifying_key(),
        &p2,
        &good,
        &owner.verifying_key(),
        now,
    )
    .unwrap_err();
    assert!(
        e.contains("checksum mismatch") || e.contains("different proposal"),
        "{e}"
    );
}

#[test]
fn refusal_replay() {
    let (r, p, signed, sbx, owner, tree, head, now, rsha) = ctx();
    let good = apply::approve(&owner, &p.sha256(), &rsha, &head, now + 60);
    // staging root not yet created: the ledger dir is made on first use
    let mut t = target(&r);
    t.staging_root = t.staging_root.join("new/root");
    t.ledger = t.staging_root.join("applied.ledger");
    let done = apply::apply_staged(
        &t,
        &signed,
        &sbx.verifying_key(),
        &p,
        &good,
        &owner.verifying_key(),
        now,
    )
    .unwrap();
    assert_eq!(done.tree, tree);
    let e = apply::apply_staged(
        &t,
        &signed,
        &sbx.verifying_key(),
        &p,
        &good,
        &owner.verifying_key(),
        now,
    )
    .unwrap_err();
    assert!(e.contains("replay refused"), "{e}");
}

#[test]
fn refusal_target_moved() {
    let (r, p, signed, sbx, owner, _, head, now, rsha) = ctx();
    w(&r, "src/other.sh", "x\n");
    git(&r, &["add", "-A"], None).unwrap();
    git(&r, &["commit", "-q", "-m", "moved"], None).unwrap();
    let good = apply::approve(&owner, &p.sha256(), &rsha, &head, now + 60);
    let e = apply::apply_staged(
        &target(&r),
        &signed,
        &sbx.verifying_key(),
        &p,
        &good,
        &owner.verifying_key(),
        now,
    )
    .unwrap_err();
    assert!(e.contains("target moved"), "{e}");
}

#[test]
fn refusal_result_differs_from_tested_tree_reverts() {
    let (r, p, _signed, _sbx, _owner, _tree) = signed_case();
    let t = target(&r);
    let a = apply::Authorized {
        proposal_sha256: p.sha256(),
        base_commit: p.base_commit.clone(),
        result_tree: "1".repeat(40),
        approval_id: String::new(),
        label: String::new(),
    };
    let e = apply::apply_to_copy(&t, &a, &p).unwrap_err();
    assert!(e.contains("reversed"), "{e}");
    let copy = std::fs::read_dir(&t.staging_root)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_dir())
        .unwrap();
    assert!(git(&copy, &["status", "--porcelain"], None)
        .unwrap()
        .trim()
        .is_empty());
}

#[test]
fn plan_env_and_loopback_ports_are_allowlisted() {
    let ok = stage::Plan::from_json(&json!({
        "env": {"CARGO_HOME": "/home/x/.cargo"},
        "stages": [{"name": "deploy", "argv": ["true"], "loopback_ports": [18642]}]
    }))
    .unwrap();
    assert_eq!(ok.stages[0].loopback_ports, vec![18642]);
    let env_plan = |path: &str| {
        stage::Plan::from_json(&json!({
            "env": {"PATH": path},
            "stages": [{"name": "a", "argv": ["true"]}]
        }))
    };
    assert!(env_plan("/var/lib/galion-gate/toolchain/cargo/bin:/usr/bin:/bin").is_ok());
    assert!(env_plan("/usr/bin:/opt/galion/openmind/bin").is_err());
    assert!(env_plan("/var/lib/hermes-rs/bin:/usr/bin").is_err());
    assert!(env_plan("/var/lib/galion-gate/../hermes-rs:/usr/bin").is_err());
    let pol = stage::policy(Path::new("/tmp/w"), &ok, &ok.stages[0]);
    assert_eq!(pol.loopback_tcp_ports, vec![18642]);
    assert!(!pol.net && pol.require_net_isolation);
    assert!(
        pol.seccomp && !pol.landlock,
        "seccomp on, Landlock never used"
    );
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
    assert!(stage::Plan::from_json(
        &json!({"stages": [{"name": "a", "argv": ["true"]}], "require_layers": ["landlock", "seccomp", "net", "no_new_privs"]})
    )
    .is_err());
    assert!(stage::Plan::from_json(
        &json!({"stages": [{"name": "a", "argv": ["true"]}], "require_layers": ["net", "no_new_privs"]})
    )
    .is_err(), "seccomp may not be dropped");
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
