// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 397: the staged tmpfiles.d file must create every folder the
//! staged coding drop-in needs, with the owner/mode the live unit expects,
//! before the drop-in lands. Slot 381 found /srv/hermes-coding missing on the
//! VPS, so `--check-config` with the staged drop-in exits 2.
//!
//! Static checks always run. `sim_root_*` needs GALION_REAL=1 and the box's
//! `systemd-tmpfiles`: it applies the staged file to a throwaway sim root
//! (`--root=`), then runs the real `CodingConfig::from_env` +
//! `write_problems` + `prepare_dirs` on the remapped paths and proves
//! prepare_dirs had nothing left to create. No sandbox, no namespaces.

use hermes_tools::config::worktree_path_ok;
use hermes_tools::CodingConfig;
use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const TMPFILES: &str = include_str!("../../../deploy/staged/tmpfiles.d/hermes-coding.conf");
const DROPIN: &str = include_str!("../../../deploy/staged/hermes-rs.service.d/coding-tools.conf");
const UNIT: &str = include_str!("../../../deploy/staged/hermes-rs.service");

#[derive(Debug)]
struct Line {
    kind: String,
    path: PathBuf,
    mode: String,
    user: String,
    group: String,
    age: String,
}

fn lines(text: &str) -> Vec<Line> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            assert_eq!(f.len(), 6, "tmpfiles line needs 6 fields: {l}");
            Line {
                kind: f[0].into(),
                path: f[1].into(),
                mode: f[2].into(),
                user: f[3].into(),
                group: f[4].into(),
                age: f[5].into(),
            }
        })
        .collect()
}

fn env_of(text: &str, key: &str) -> Option<String> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("Environment="))
        .filter_map(|kv| kv.split_once('='))
        .filter(|(k, _)| *k == key)
        .map(|(_, v)| v.trim().to_string())
        .next_back()
}

fn rw_paths(text: &str) -> Vec<(bool, PathBuf)> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("ReadWritePaths="))
        .flat_map(|v| v.split_whitespace())
        .map(|p| {
            (
                p.starts_with('-'),
                PathBuf::from(p.trim_start_matches(['-', '+'])),
            )
        })
        .collect()
}

/// Folders CodingConfig needs for the staged env (same defaults as from_env
/// and prepare_dirs: target = state/target, audit = state/audit.jsonl).
fn needed_dirs() -> BTreeSet<PathBuf> {
    let st = PathBuf::from(env_of(DROPIN, "HERMES_RS_CODING_STATE").unwrap());
    let target = env_of(DROPIN, "HERMES_RS_CODING_TARGET")
        .map(PathBuf::from)
        .unwrap_or_else(|| st.join("target"));
    let audit = env_of(DROPIN, "HERMES_RS_CODING_AUDIT")
        .map(PathBuf::from)
        .unwrap_or_else(|| st.join("audit.jsonl"));
    let mut s: BTreeSet<PathBuf> = ["", "stage", "tmp", "home", "cargo-home"]
        .iter()
        .map(|d| if d.is_empty() { st.clone() } else { st.join(d) })
        .collect();
    s.insert(target);
    s.insert(audit.parent().unwrap().to_path_buf());
    s
}

#[test]
fn only_safe_create_lines_root_0700_no_age() {
    let ls = lines(TMPFILES);
    assert!(!ls.is_empty());
    for l in &ls {
        assert_eq!(l.kind, "d", "only plain create (no D/R/r/x/e/z): {l:?}");
        assert_eq!(l.mode, "0700", "{l:?}");
        assert_eq!(
            (l.user.as_str(), l.group.as_str()),
            ("root", "root"),
            "{l:?}"
        );
        assert_eq!(l.age, "-", "no time-based cleanup: {l:?}");
        assert!(l.path.is_absolute(), "{l:?}");
    }
    let set: BTreeSet<_> = ls.iter().map(|l| &l.path).collect();
    assert_eq!(set.len(), ls.len(), "duplicate path lines");
}

#[test]
fn covers_every_dir_the_dropin_needs_and_the_rw_root() {
    let made: BTreeSet<PathBuf> = lines(TMPFILES).into_iter().map(|l| l.path).collect();
    for d in needed_dirs() {
        assert!(made.contains(&d), "tmpfiles does not create {d:?}");
    }
    // The optional ("-") ReadWritePaths root must exist BEFORE the unit
    // starts, otherwise systemd skips it and it stays read-only.
    for (optional, p) in rw_paths(DROPIN) {
        if optional {
            assert!(made.contains(&p), "optional RW path {p:?} not created");
        }
    }
    let wt = PathBuf::from(env_of(DROPIN, "HERMES_RS_CODING_WORKTREE").unwrap());
    assert!(made.contains(wt.parent().unwrap()), "worktree parent");
}

#[test]
fn nothing_inside_the_worktree_or_denied_and_all_writable() {
    let wt = PathBuf::from(env_of(DROPIN, "HERMES_RS_CODING_WORKTREE").unwrap());
    let mut rw: Vec<PathBuf> = rw_paths(UNIT).into_iter().map(|x| x.1).collect();
    rw.extend(rw_paths(DROPIN).into_iter().map(|x| x.1));
    for l in lines(TMPFILES) {
        assert!(!l.path.starts_with(&wt), "{:?} is in the worktree", l.path);
        assert!(
            rw.iter().any(|r| l.path.starts_with(r)),
            "{:?} not in ReadWritePaths {rw:?}",
            l.path
        );
        // Same deny list the gateway uses for the worktree: the coding folder
        // tree must not sit in or over a denied root either.
        worktree_path_ok(&l.path, &[]).unwrap_or_else(|e| panic!("{e}"));
    }
}

#[test]
fn dropin_stays_execstartpre_free() {
    for t in [DROPIN, UNIT] {
        assert!(
            !t.lines()
                .any(|l| l.trim_start().starts_with("ExecStartPre")),
            "no ExecStartPre: folders come from tmpfiles.d"
        );
        assert!(
            !t.lines().any(|l| l.trim() == "DefaultDependencies=no"),
            "DefaultDependencies=no would drop the order after systemd-tmpfiles-setup"
        );
    }
}

// ---------------------------------------------------------------- real

fn on() -> bool {
    std::env::var("GALION_REAL").ok().as_deref() == Some("1")
}

fn id(a: &str) -> String {
    let o = Command::new("id").arg(a).output().expect("id");
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

fn me() -> (String, String) {
    (id("-un"), id("-gn"))
}

/// `--root=` resolves user/group names from the sim root's own /etc, like
/// an offline image: give it the test user (and root) there.
fn sim_etc(root: &Path) {
    let (u, g) = me();
    let (uid, gid) = (id("-u"), id("-g"));
    std::fs::create_dir_all(root.join("etc")).unwrap();
    std::fs::write(
        root.join("etc/passwd"),
        format!("root:x:0:0::/root:/bin/sh\n{u}:x:{uid}:{gid}::/:/bin/sh\n"),
    )
    .unwrap();
    std::fs::write(root.join("etc/group"), format!("root:x:0:\n{g}:x:{gid}:\n")).unwrap();
}

/// The staged file with root:root swapped for the test user when not root
/// (an unprivileged run cannot chown to root); everything else byte-identical.
fn staged_for_me(dir: &Path) -> PathBuf {
    let (u, g) = me();
    let txt = if u == "root" {
        TMPFILES.to_string()
    } else {
        TMPFILES
            .lines()
            .map(|l| {
                if l.trim_start().starts_with('d') {
                    l.replace(" root root ", &format!(" {u} {g} "))
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let p = dir.join("hermes-coding.conf");
    std::fs::write(&p, txt).unwrap();
    p
}

fn tmpfiles(root: &Path, conf: &Path, mode: &str) -> i32 {
    Command::new("systemd-tmpfiles")
        .arg(mode)
        .arg(format!("--root={}", root.display()))
        .arg(conf)
        .status()
        .expect("systemd-tmpfiles")
        .code()
        .unwrap_or(-1)
}

fn under(root: &Path, p: &str) -> PathBuf {
    root.join(p.trim_start_matches('/'))
}

fn tree(root: &Path) -> BTreeSet<PathBuf> {
    fn walk(p: &Path, out: &mut BTreeSet<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                out.insert(e.path());
                if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    walk(&e.path(), out);
                }
            }
        }
    }
    let mut s = BTreeSet::new();
    walk(root, &mut s);
    s
}

#[test]
fn sim_root_tmpfiles_then_coding_config_needs_nothing_more() {
    if !on() {
        eprintln!("skip: GALION_REAL=1 not set");
        return;
    }
    let base = std::env::temp_dir().join(format!("slot397-tmpfiles-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("root");
    std::fs::create_dir_all(root.join("srv")).unwrap();
    std::fs::set_permissions(root.join("srv"), std::fs::Permissions::from_mode(0o755)).unwrap();
    sim_etc(&root);
    let conf = staged_for_me(&base);

    // 1. create (boot-equivalent), twice: idempotent, rc 0.
    assert_eq!(tmpfiles(&root, &conf, "--create"), 0);
    assert_eq!(tmpfiles(&root, &conf, "--create"), 0);
    for l in lines(TMPFILES) {
        let p = under(&root, l.path.to_str().unwrap());
        let m = std::fs::metadata(&p).unwrap_or_else(|e| panic!("{p:?}: {e}"));
        assert!(m.is_dir());
        assert_eq!(m.permissions().mode() & 0o7777, 0o700, "{p:?}");
    }

    // 2. A wrong mode (e.g. the 19:45 leftovers were 0755) is fixed.
    let state = under(&root, &env_of(DROPIN, "HERMES_RS_CODING_STATE").unwrap());
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(tmpfiles(&root, &conf, "--create"), 0);
    assert_eq!(
        std::fs::metadata(&state).unwrap().permissions().mode() & 0o7777,
        0o700
    );

    // 3. --clean and --remove never delete work (age "-", type d only).
    let keep = state.join("stage/keep.txt");
    std::fs::write(&keep, b"work").unwrap();
    assert_eq!(tmpfiles(&root, &conf, "--clean"), 0);
    assert_eq!(tmpfiles(&root, &conf, "--remove"), 0);
    assert_eq!(std::fs::read(&keep).unwrap(), b"work");
    std::fs::remove_file(&keep).unwrap();

    // 4. Runbook step 1b: the worktree is a git clone (here: git init).
    let wt = under(&root, &env_of(DROPIN, "HERMES_RS_CODING_WORKTREE").unwrap());
    let st = Command::new("git")
        .args(["init", "-q"])
        .arg(&wt)
        .status()
        .unwrap();
    assert!(st.success());

    // 5. The real CodingConfig on the remapped staged env.
    let remap = |k: &str| -> Option<String> {
        match k {
            "HERMES_RS_CODING_WORKTREE" | "HERMES_RS_CODING_STATE" => {
                env_of(DROPIN, k).map(|v| under(&root, &v).to_string_lossy().into_owned())
            }
            "HERMES_RS_CODING_BRANCH" => env_of(DROPIN, k),
            _ => None, // toolchain/cgroup: not part of this check (F-397-1)
        }
    };
    let home = base.join("hermes-home");
    std::fs::create_dir_all(&home).unwrap();
    let cc = CodingConfig::from_env(&remap, &home)
        .expect("from_env ok")
        .expect("coding on");
    assert!(cc.write_problems().is_empty(), "{:?}", cc.write_problems());
    let before = tree(&root);
    cc.prepare_dirs().expect("prepare_dirs");
    let after = tree(&root);
    let new: Vec<_> = after.difference(&before).collect();
    assert!(new.is_empty(), "prepare_dirs had to create {new:?}");

    // 6. Without the tmpfiles step the same env is refused (slot 381 case).
    let bare = base.join("bare");
    std::fs::create_dir_all(bare.join("srv")).unwrap();
    let remap_bare = |k: &str| -> Option<String> {
        match k {
            "HERMES_RS_CODING_WORKTREE" | "HERMES_RS_CODING_STATE" => {
                env_of(DROPIN, k).map(|v| under(&bare, &v).to_string_lossy().into_owned())
            }
            "HERMES_RS_CODING_BRANCH" => env_of(DROPIN, k),
            _ => None,
        }
    };
    let e = CodingConfig::from_env(&remap_bare, &home).unwrap_err();
    assert!(e.contains("No such file"), "{e}");

    std::fs::remove_dir_all(&base).unwrap();
}
