// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! `perms` check (D-318-2): metadata-only rules for secret-like files and
//! world-writable directories.

use repo_check::perms::*;
use repo_check::{scan_tree, Check, Level};

#[test]
fn secret_like_names() {
    for p in [
        "/opt/galion/galion-light/publish/obsidian/link.env",
        ".env",
        "a/.envrc",
        "a/.env.local",
        "a/prod.env",
        "a/server.pem",
        "a/tls.key",
        "a/id_ed25519",
        "a/.git-credentials",
        "a/authorized_keys",
        "a/openrouter_token",
        "a/API_KEY",
        "a/db_password",
        "a/store.p12",
        "a/github_token.json",
        "a/get-app-token",
    ] {
        assert!(is_secret_like(p), "{p}");
    }
    for p in [
        "a/.env.example",
        "a/.env.sample",
        "a/credential_pool.py",
        "a/secret_scope.py",
        "a/tokens.rs",
        "a/secret-storage-policy.ts",
        "a/secrets.md",
        "a/id_ed25519.pub",
        "a/play.sh",
        "a/main.rs",
        "a/Cargo.toml",
        "a/llama-tokenize",
        "a/test-tokenizer-0.cpp",
        "a/added_tokens.json",
        "a/GalionToken.sol",
        "a/008_password_reset_tokens.sql",
    ] {
        assert!(!is_secret_like(p), "{p}");
    }
}

#[test]
fn parse_lines() {
    let e = parse_stat_line("666 f /opt/x/link.env").unwrap();
    assert_eq!(
        (e.mode, e.kind, e.path.as_str()),
        (0o666, Kind::File, "/opt/x/link.env")
    );
    let d = parse_stat_line("1777 d /tmp").unwrap();
    assert_eq!((d.mode, d.kind), (0o1777, Kind::Dir));
    let sp = parse_stat_line("644 f /a dir/with space.env").unwrap();
    assert_eq!(sp.path, "/a dir/with space.env");
    let notype = parse_stat_line("600 /root/.env").unwrap();
    assert_eq!(
        (notype.kind, notype.path.as_str()),
        (Kind::File, "/root/.env")
    );
    assert_eq!(parse_stat_line("777 l /x").unwrap().kind, Kind::Other);
    for bad in ["", "abc f /x", "888 f /x", "66666 f /x", "666"] {
        assert!(parse_stat_line(bad).is_none(), "{bad:?}");
    }
}

#[test]
fn modes() {
    let f = check_mode("x/link.env", 0o666, Kind::File, true).unwrap();
    assert_eq!((f.check, f.level), (Check::Perms, Level::Error));
    assert!(f.msg.contains("666") && f.msg.contains("chmod 600"));
    assert_eq!(
        check_mode("x/link.env", 0o664, Kind::File, true)
            .unwrap()
            .level,
        Level::Warn
    );
    assert_eq!(
        check_mode("x/link.env", 0o644, Kind::File, true)
            .unwrap()
            .level,
        Level::Warn
    );
    assert!(check_mode("x/link.env", 0o644, Kind::File, false).is_none());
    assert!(check_mode("x/link.env", 0o600, Kind::File, true).is_none());
    assert!(check_mode("x/link.env", 0o640, Kind::File, true).is_none());
    // non-secret 666 files are not this check's business
    assert!(check_mode("x/favicon.svg", 0o666, Kind::File, true).is_none());
    let d = check_mode("x/vault", 0o777, Kind::Dir, true).unwrap();
    assert_eq!(d.level, Level::Error);
    assert!(check_mode("/tmp", 0o1777, Kind::Dir, true).is_none());
    assert!(check_mode("x/d", 0o775, Kind::Dir, true).is_none());
    assert!(check_mode("x/l.env", 0o777, Kind::Other, true).is_none());
}

/// Shape of the live VPS listing on 2026-10-28 05:09 CET (metadata only).
const VPS_SHAPE: &str = "755 d /opt/galion
775 d /opt/galion/galion-light
775 d /opt/galion/galion-light/publish
775 d /opt/galion/galion-light/publish/obsidian
666 f /opt/galion/galion-light/publish/obsidian/link.env
775 f /opt/galion/galion-light/publish/obsidian/play.sh
700 d /opt/galion/hermes
600 f /opt/galion/hermes/.env
777 d /opt/galion/hermes/vault
666 f /opt/galion/hermes/vault/.vault.lock
666 f /opt/galion/hermes/config.yaml
755 d /opt/galion/console-dist/static
666 f /opt/galion/console-dist/static/favicon.svg
666 f /opt/galion/tooling/hermes-agent/.env.example
777 d /opt/galion/tooling/hermes-agent/agent/secret_sources
";

#[test]
fn stat_list_vps_shape() {
    let (f, n) = scan_stat_list(VPS_SHAPE);
    assert_eq!(n, 15);
    let got: Vec<(&str, Level)> = f.iter().map(|x| (x.file.as_str(), x.level)).collect();
    assert_eq!(
        got,
        vec![
            (
                "/opt/galion/galion-light/publish/obsidian/link.env",
                Level::Error
            ),
            (
                "/opt/galion/tooling/hermes-agent/agent/secret_sources",
                Level::Error
            ),
        ],
        "hermes/ (700) hides its 777 vault; .env.example and favicon are not secret-like"
    );
    // after the staged fix (chmod 600) link.env is clean
    let fixed = VPS_SHAPE.replace(
        "666 f /opt/galion/galion-light/publish/obsidian/link.env",
        "600 f /opt/galion/galion-light/publish/obsidian/link.env",
    );
    let (f2, _) = scan_stat_list(&fixed);
    assert!(f2.iter().all(|x| !x.file.ends_with("link.env")));
    // and if hermes/ were opened to 755 its 777 vault would show up
    let opened = VPS_SHAPE.replace("700 d /opt/galion/hermes\n", "755 d /opt/galion/hermes\n");
    let (f3, _) = scan_stat_list(&opened);
    assert!(f3.iter().any(|x| x.file == "/opt/galion/hermes/vault"));
}

#[cfg(unix)]
#[test]
fn tree_scan_flags_world_writable_env() {
    use std::os::unix::fs::PermissionsExt;
    let d = std::env::temp_dir().join(format!("rc-perms-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("deploy")).unwrap();
    let lic = "MIT License\n\nCopyright (c) 3000 Galion / galion-studio\n\nPermission is hereby granted, free of charge, to any person\n";
    std::fs::write(d.join("LICENSE"), lic).unwrap();
    let env = d.join("deploy/link.env");
    std::fs::write(&env, "GALION_EAR=127.0.0.1:1\n").unwrap();
    std::fs::set_permissions(&env, std::fs::Permissions::from_mode(0o666)).unwrap();
    let r = scan_tree(&d, None).unwrap();
    let p: Vec<_> = r
        .findings
        .iter()
        .filter(|f| f.check == Check::Perms)
        .collect();
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].file, "deploy/link.env");
    assert!(!p[0].msg.contains("127.0.0.1"), "never prints contents");
    std::fs::set_permissions(&env, std::fs::Permissions::from_mode(0o644)).unwrap();
    let r = scan_tree(&d, None).unwrap();
    assert!(r.findings.iter().all(|f| f.check != Check::Perms));
    let _ = std::fs::remove_dir_all(&d);
}
