// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Slot 398 chmod-plan-refresh: `--plan-ow-tree` (`tree-o-w`, remove o+w
//! only) and a static check that the staged 69-row plan has no row under the
//! legacy Hermes trees removed on 2026-10-28 10:08 CET (F-379-1).

use repo_check::perms::*;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_repo-check");

/// Shape of the VPS venv listing on 2026-10-28 16:42 CET (+01:00): pip
/// installs leave 775/664; a handful of entries are 777/666.
const VENV: &str = "755 d root root /opt/galion
775 d root root /opt/galion/tts
775 d root root /opt/galion/tts/venv
775 d root root /opt/galion/tts/venv/lib
664 f root root /opt/galion/tts/venv/lib/ok.py
777 d root root /opt/galion/tts/venv/lib/vaultgemma
666 f root root /opt/galion/tts/venv/lib/vaultgemma/modeling_vaultgemma.py
666 f root root /opt/galion/tts/venv/lib/_git_credential.py
666 f root root /opt/galion/tts/venv/lib/cacert.pem
777 l root root /opt/galion/tts/venv/bin/python
1777 d root root /opt/galion/tts/venv/tmp
766 f root root /opt/galion/tts/venv/lib/exec_ow.sh
700 d root root /opt/galion/hidden
777 d root root /opt/galion/hidden/venv
666 f root root /opt/galion/hidden/venv/x.py
777 d root root /opt/galion/elsewhere
";

fn row<'a>(p: &'a Plan, path: &str) -> Option<&'a PlanRow> {
    p.rows.iter().find(|r| r.path == path)
}

#[test]
fn ow_tree_removes_only_other_write() {
    let e = parse_stat_list(VENV);
    let p = build_plan_ow(
        &e,
        &[],
        &[
            "/opt/galion/tts/venv".into(),
            "/opt/galion/hidden/venv".into(),
        ],
    );
    let t = "/opt/galion/tts/venv/lib";
    // 775/664 install defaults are not touched
    assert!(row(&p, "/opt/galion/tts/venv").is_none());
    assert!(row(&p, &format!("{t}/ok.py")).is_none());
    // 777 dir -> 775, 666 -> 664, 766 -> 764: group bits and x kept
    let r = row(&p, &format!("{t}/vaultgemma")).unwrap();
    assert_eq!(
        (r.old, r.new, r.reason),
        (0o777, 0o775, Reason::TreeOtherWritable)
    );
    let r = row(&p, &format!("{t}/vaultgemma/modeling_vaultgemma.py")).unwrap();
    assert_eq!((r.old, r.new), (0o666, 0o664));
    let r = row(&p, &format!("{t}/exec_ow.sh")).unwrap();
    assert_eq!((r.old, r.new), (0o766, 0o764));
    // secret-like names inside an o-w tree are NOT forced to 600 (cacert.pem
    // must stay readable for non-root pip users)
    for f in ["_git_credential.py", "cacert.pem"] {
        let r = row(&p, &format!("{t}/{f}")).unwrap();
        assert_eq!((r.new, r.reason), (0o664, Reason::TreeOtherWritable), "{f}");
    }
    // sticky dir: o-w keeps the sticky bit
    let r = row(&p, "/opt/galion/tts/venv/tmp").unwrap();
    assert_eq!((r.old, r.new), (0o1777, 0o1775));
    // symlinks never planned
    assert!(row(&p, "/opt/galion/tts/venv/bin/python").is_none());
    // behind a 700 dir: hidden, not planned
    assert!(row(&p, "/opt/galion/hidden/venv").is_none());
    assert!(p.hidden.iter().any(|h| h == "/opt/galion/hidden/venv/x.py"));
    // outside every tree the normal rule still applies
    let r = row(&p, "/opt/galion/elsewhere").unwrap();
    assert_eq!(r.reason, Reason::DirOtherWritable);
    assert_eq!(p.rows.len(), 7, "{:#?}", p.rows);
    // dirs first
    let first_file = p.rows.iter().position(|r| r.kind == Kind::File).unwrap();
    assert!(p.rows[first_file..].iter().all(|r| r.kind == Kind::File));
}

#[test]
fn normalize_tree_wins_and_no_ow_tree_is_old_behaviour() {
    let e = parse_stat_list(VENV);
    let both = build_plan_ow(
        &e,
        &["/opt/galion/tts/venv".into()],
        &["/opt/galion/tts/venv".into()],
    );
    assert!(both
        .rows
        .iter()
        .all(|r| r.reason != Reason::TreeOtherWritable));
    assert_eq!(
        row(&both, "/opt/galion/tts/venv/lib/ok.py").unwrap().new,
        0o644
    );
    let a = build_plan(&e, &[]);
    let b = build_plan_ow(&e, &[], &[]);
    assert_eq!(a.rows, b.rows);
    assert_eq!(a.hidden, b.hidden);
}

#[test]
fn tree_o_w_round_trips_sh_and_verify() {
    let e = parse_stat_list(VENV);
    let p = build_plan_ow(&e, &[], &["/opt/galion/tts/venv".into()]);
    let tsv = render_plan_tsv(&p.rows, "t");
    assert!(tsv.contains("\t1777\t1775\ttree-o-w\n"), "{tsv}");
    assert_eq!(parse_plan_tsv(&tsv).unwrap(), p.rows);
    let sh = render_plan_sh(&p.rows, "t");
    assert!(sh.contains("g '/opt/galion/tts/venv/lib/cacert.pem' f 666 'root:root' 664"));
    let back = rollback(&p.rows);
    assert!(back
        .iter()
        .all(|r| r.reason == Reason::TreeOtherWritable || r.reason == Reason::DirOtherWritable));
    let st = verify_plan(&p.rows, &e);
    assert!(st.iter().all(|(_, s)| *s == RowState::Pending));
}

#[test]
fn staged_plan_has_no_removed_legacy_paths() {
    let text = include_str!("../../../deploy/staged/world-writable-dirs/chmod-plan.tsv");
    let rows = parse_plan_tsv(text).unwrap();
    assert_eq!(rows.len(), 69);
    for r in &rows {
        for gone in [
            "/opt/galion/tooling/hermes-agent",
            "/opt/galion/galion-light/shadow/",
            "/venv/",
        ] {
            assert!(!r.path.contains(gone), "{} under removed {gone}", r.path);
        }
        assert_eq!(r.owner, "root:root");
    }
    let back = parse_plan_tsv(include_str!(
        "../../../deploy/staged/world-writable-dirs/rollback-plan.tsv"
    ))
    .unwrap();
    assert_eq!(back, rollback(&rows));
}

#[test]
fn cli_plan_ow_tree() {
    let dir = std::env::temp_dir().join(format!("rc398-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let list = dir.join("venv.list");
    std::fs::write(&list, VENV).unwrap();
    let out = Command::new(BIN)
        .args(["--stat-list", list.to_str().unwrap(), "--chmod-plan"])
        .args(["--plan-ow-tree", "/opt/galion/tts/venv"])
        .args(["--plan-reasons", "tree-o-w"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("; o-w trees /opt/galion/tts/venv"), "{s}");
    assert_eq!(s.lines().filter(|l| !l.starts_with('#')).count(), 6, "{s}");
    let bad = Command::new(BIN)
        .args(["--stat-list", list.to_str().unwrap()])
        .args(["--plan-ow-tree", "/x"])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
    std::fs::remove_dir_all(&dir).unwrap();
}
