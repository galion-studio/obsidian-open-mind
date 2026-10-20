// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio

//! SEC-3 on a fixture Hermes home (no chown, so it runs as any user):
//! dry run changes nothing, apply gives the target modes and a rollback
//! file, fix-world-writable clears o+w, rollback restores every mode,
//! symlinks and wrong types are refused.

use history_purge::sec3::{run, world_writable, Mode, Opts, Sec3Error};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sec3-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn set(p: &Path, m: u32) {
    fs::set_permissions(p, fs::Permissions::from_mode(m)).unwrap();
}

fn fixture(root: &Path) -> PathBuf {
    let h = root.join("hermes");
    fs::create_dir_all(h.join("vault")).unwrap();
    fs::create_dir_all(h.join("sessions")).unwrap();
    for f in [
        "config.yaml",
        "SOUL.md",
        "vault/.vault.lock",
        "USER.md",
        "notes.md",
    ] {
        fs::write(h.join(f), b"").unwrap();
    }
    set(&h, 0o700);
    for f in ["config.yaml", "SOUL.md", "vault/.vault.lock", "notes.md"] {
        set(&h.join(f), 0o666);
    }
    set(&h.join("vault"), 0o777);
    set(&h.join("USER.md"), 0o664);
    h
}

const NAMES: [&str; 6] = [
    "config.yaml",
    "SOUL.md",
    "vault",
    "vault/.vault.lock",
    "USER.md",
    "notes.md",
];

fn modes(h: &Path) -> Vec<u32> {
    NAMES
        .iter()
        .map(|n| {
            fs::symlink_metadata(h.join(n))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777
        })
        .collect()
}

fn opts(h: &Path, state: &Path, mode: Mode, fix: bool) -> Opts {
    Opts {
        home: h.to_path_buf(),
        state_dir: state.to_path_buf(),
        mode,
        fix_world_writable: fix,
        no_chown: true,
    }
}

#[test]
fn dry_apply_fix_rollback() {
    let root = tmp("flow");
    let h = fixture(&root);
    let st = root.join("state");
    let orig = vec![0o666, 0o666, 0o777, 0o666, 0o664, 0o666];
    assert_eq!(modes(&h), orig);
    assert_eq!(world_writable(&h), vec!["notes.md".to_string()]);

    let mut out = Vec::new();
    assert_eq!(
        run(&opts(&h, &st, Mode::Dry, true), &mut out).unwrap(),
        None
    );
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("dry run: nothing changed"), "{text}");
    assert!(text.contains("notes.md"), "{text}");
    assert_eq!(modes(&h), orig);
    assert!(!st.exists(), "dry run must not create the state dir");

    let first = run(&opts(&h, &st, Mode::Apply, false), &mut Vec::new())
        .unwrap()
        .unwrap();
    assert_eq!(modes(&h), vec![0o600, 0o644, 0o700, 0o600, 0o600, 0o666]);
    assert_eq!(
        fs::metadata(&first).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&st).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let rb = fs::read_to_string(&first).unwrap();
    assert!(rb.contains("config.yaml\t666\t"), "{rb}");
    assert!(rb.contains("notes.md\t666\t"), "{rb}");

    run(&opts(&h, &st, Mode::Apply, true), &mut Vec::new()).unwrap();
    assert_eq!(modes(&h), vec![0o600, 0o644, 0o700, 0o600, 0o600, 0o664]);
    assert!(world_writable(&h).is_empty());

    let mut out = Vec::new();
    run(&opts(&h, &st, Mode::Rollback(first), false), &mut out).unwrap();
    assert!(String::from_utf8(out).unwrap().contains("rollback from"));
    assert_eq!(modes(&h), orig);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn refusals() {
    let root = tmp("refuse");
    let h = fixture(&root);
    let st = root.join("state");
    // symlinked target
    fs::remove_file(h.join("SOUL.md")).unwrap();
    std::os::unix::fs::symlink(root.join("elsewhere"), h.join("SOUL.md")).unwrap();
    let e = run(&opts(&h, &st, Mode::Apply, false), &mut Vec::new()).unwrap_err();
    assert!(
        matches!(e, Sec3Error::Refuse(ref m) if m.contains("symlink")),
        "{e}"
    );
    assert_eq!(
        fs::symlink_metadata(h.join("config.yaml"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o666
    );
    assert!(!st.exists());
    // wrong type
    fs::remove_file(h.join("SOUL.md")).unwrap();
    fs::create_dir(h.join("SOUL.md")).unwrap();
    let e = run(&opts(&h, &st, Mode::Dry, false), &mut Vec::new()).unwrap_err();
    assert!(
        matches!(e, Sec3Error::Refuse(ref m) if m.contains("not type f")),
        "{e}"
    );
    // forbidden homes
    for bad in [root.join("x/secrets/persist"), root.join("galion-control")] {
        fs::create_dir_all(&bad).unwrap();
        let e = run(&opts(&bad, &st, Mode::Dry, false), &mut Vec::new()).unwrap_err();
        assert!(
            matches!(e, Sec3Error::Usage(ref m) if m.contains("refusing")),
            "{e}"
        );
    }
    // rollback file escaping the home
    fs::write(root.join("evil.tsv"), "../outside\t777\t0:0\n").unwrap();
    let e = run(
        &opts(&h, &st, Mode::Rollback(root.join("evil.tsv")), false),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(matches!(e, Sec3Error::Refuse(_)), "{e}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn missing_targets_are_skipped() {
    let root = tmp("missing");
    let h = root.join("hermes");
    fs::create_dir_all(&h).unwrap();
    fs::write(h.join("config.yaml"), b"").unwrap();
    set(&h.join("config.yaml"), 0o666);
    let mut out = Vec::new();
    run(&opts(&h, &root.join("st"), Mode::Apply, false), &mut out).unwrap();
    let t = String::from_utf8(out).unwrap();
    assert!(t.contains("SOUL.md") && t.contains("missing"), "{t}");
    assert_eq!(
        fs::metadata(h.join("config.yaml"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let _ = fs::remove_dir_all(&root);
}
