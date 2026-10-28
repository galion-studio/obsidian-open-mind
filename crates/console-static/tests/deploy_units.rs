// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Slot 313 (D-303-1 prep): the staged drop-ins
//! deploy/console-rust-serve/{console,console-rust-primary}.service.d/60-admin-store.conf
//! on top of the as-built mirrors of the live units must give the Rust
//! console exactly the live argv plus --admin-store/--admin-mirror, parse
//! with this crate's own `Config::from_args`, keep every live hardening key,
//! and make both store directories writable. Removing the drop-in (the
//! rollback) must give back the live ExecStart byte for byte. Offline,
//! always on.

mod units;

use console_static::config::Config;
use std::path::{Path, PathBuf};
use units::{covered, effective, effective_list, effective_paths, exec_argv, flag, parse};

const LIVE_8080: &str = include_str!("../../../deploy/console-rust-serve/live/console.service");
const LIVE_18080: &str =
    include_str!("../../../deploy/console-rust-serve/live/console-rust-primary.service");
const DROP_8080: &str =
    include_str!("../../../deploy/console-rust-serve/console.service.d/60-admin-store.conf");
const DROP_18080: &str = include_str!(
    "../../../deploy/console-rust-serve/console-rust-primary.service.d/60-admin-store.conf"
);

const LIVE_BIN: &str = "/opt/galion/rust/bin/galion-console-serve-2d846fb8";
const NEW_BIN: &str = "/opt/galion/rust/bin/galion-console-serve-9c0f9d1a";
const STORE: &str = "/root/galion-secrets/admin-device.json";
const MIRROR: &str = "/workspace/galion-light/secrets/persist/admin-device.json";

fn sets() -> [(&'static str, &'static str, &'static str, &'static str); 2] {
    [
        ("console", LIVE_8080, DROP_8080, "127.0.0.1:8080"),
        (
            "console-rust-primary",
            LIVE_18080,
            DROP_18080,
            "127.0.0.1:18080",
        ),
    ]
}

fn cfg_of(argv: &[String]) -> Config {
    Config::from_args(argv[1..].iter().cloned()).unwrap_or_else(|e| panic!("argv {argv:?}: {e}"))
}

#[test]
fn live_mirrors_run_2d846fb8_without_admin_store() {
    for (u, live, _, bind) in sets() {
        let argv = exec_argv(&[live]);
        assert_eq!(argv[0], LIVE_BIN, "{u}");
        assert_eq!(flag(&argv, "--bind"), Some(bind), "{u}");
        assert!(
            flag(&argv, "--admin-store").is_none(),
            "{u}: live has no store"
        );
        // This build parses the live argv too (so the binary swap alone
        // would be safe, it just leaves /api/v1/admin at 503).
        let c = cfg_of(&argv);
        assert!(c.admin_store.is_none() && c.local_api && c.rust_ui, "{u}");
    }
}

#[test]
fn dropin_argv_is_live_argv_plus_admin_flags_on_new_binary() {
    for (u, live, drop, bind) in sets() {
        let old = exec_argv(&[live]);
        let new = exec_argv(&[live, drop]);
        assert_eq!(new[0], NEW_BIN, "{u}");
        // Same flags in the same order, admin pair inserted before --quiet.
        let mut want: Vec<String> = old[1..].to_vec();
        let q = want.iter().position(|a| a == "--quiet").expect("--quiet");
        want.splice(
            q..q,
            ["--admin-store", STORE, "--admin-mirror", MIRROR]
                .iter()
                .map(|s| s.to_string()),
        );
        assert_eq!(new[1..].to_vec(), want, "{u}");
        let c = cfg_of(&new);
        assert_eq!(c.bind.to_string(), bind, "{u}");
        assert_eq!(c.admin_store.as_deref(), Some(Path::new(STORE)), "{u}");
        assert_eq!(c.admin_mirror.as_deref(), Some(Path::new(MIRROR)), "{u}");
        assert_eq!(
            c.pulse.map(|p| p.to_string()).as_deref(),
            Some("127.0.0.1:8091")
        );
        assert_eq!(
            c.desk.map(|p| p.to_string()).as_deref(),
            Some("127.0.0.1:8646")
        );
        assert!(
            c.local_api && c.rust_ui && c.dist_mode && !c.access_log,
            "{u}"
        );
        // Self-upstream (the Rust UI asks itself), nothing proxied: the board
        // and admin gate are answered in-process.
        assert_eq!(c.upstream, Some(c.bind), "{u}: self-upstream");
        assert!(c.proxy_prefixes.is_empty(), "{u}: nothing proxied");
    }
}

#[test]
fn store_dirs_are_writable_and_nothing_else_is() {
    for (u, live, drop, _) in sets() {
        let files = [live, drop];
        let rw = effective_paths(&files, "ReadWritePaths");
        let argv = exec_argv(&files);
        for f in [flag(&argv, "--admin-store"), flag(&argv, "--admin-mirror")] {
            let dir = PathBuf::from(f.unwrap());
            let dir = dir.parent().unwrap().to_str().unwrap().to_string();
            assert!(covered(&dir, &rw), "{u}: {dir} not in {rw:?}");
        }
        assert_eq!(
            rw,
            vec![
                "/root/galion-secrets",
                "/workspace/galion-light/secrets/persist"
            ],
            "{u}: exactly the two store dirs"
        );
        // Binary and dist stay read-only, and no RW path widens to them.
        let ro = effective_paths(&files, "ReadOnlyPaths");
        assert!(covered(NEW_BIN, &ro), "{u}: new binary under ReadOnlyPaths");
        assert!(covered("/opt/galion/console-dist", &ro), "{u}");
        assert!(!covered("/opt/galion/console-dist", &rw) && !covered(NEW_BIN, &rw));
        // Negative control: the base alone leaves the store dirs read-only.
        assert!(effective_paths(&[live], "ReadWritePaths").is_empty(), "{u}");
    }
}

#[test]
fn hardening_is_unchanged_and_root_can_reach_the_store() {
    let keys = [
        "Type",
        "Restart",
        "RestartSec",
        "MemoryMax",
        "TasksMax",
        "Nice",
        "NoNewPrivileges",
        "ProtectSystem",
        "ProtectHome",
        "PrivateTmp",
        "PrivateDevices",
        "ProtectKernelTunables",
        "ProtectKernelModules",
        "ProtectControlGroups",
        "SyslogIdentifier",
    ];
    for (u, live, drop, _) in sets() {
        for k in keys {
            assert_eq!(
                effective(&[live, drop], "Service", k),
                effective(&[live], "Service", k),
                "{u}: {k}"
            );
        }
        let f = [live, drop];
        assert_eq!(
            effective(&f, "Service", "ProtectSystem").as_deref(),
            Some("strict")
        );
        // ProtectHome=yes/tmpfs would hide /root (store missing = unpaired
        // forever); read-only + ReadWritePaths is the only working mode.
        assert_eq!(
            effective(&f, "Service", "ProtectHome").as_deref(),
            Some("read-only")
        );
        // /root/galion-secrets is 0700 root: no User=/DynamicUser=.
        assert!(effective(&f, "Service", "User").is_none(), "{u}");
        assert!(effective(&f, "Service", "DynamicUser").is_none(), "{u}");
    }
}

#[test]
fn dropin_only_touches_execstart_and_readwritepaths() {
    for (_, _, drop, _) in sets() {
        for (s, k, _) in parse(drop) {
            assert_eq!(s, "Service");
            assert!(k == "ExecStart" || k == "ReadWritePaths", "{k}");
        }
        assert_eq!(effective_list(&[drop], "Service", "ExecStart").len(), 1);
    }
}

#[test]
fn rollback_is_removing_the_dropin() {
    for (u, live, drop, _) in sets() {
        let back = effective_list(&[live], "Service", "ExecStart");
        assert_eq!(back.len(), 1, "{u}");
        assert!(back[0].starts_with(LIVE_BIN), "{u}");
        // And the drop-in never edits the base file it overrides.
        assert!(
            !drop.contains("[Unit]") && !drop.contains("[Install]"),
            "{u}"
        );
    }
}

#[test]
fn dropin_without_reset_would_be_rejected_by_systemd() {
    // Negative control: Type=simple refuses more than one ExecStart; without
    // the empty `ExecStart=` the unit would fail to load (console down).
    for (_, live, drop, _) in sets() {
        let bad = drop.replace("ExecStart=\n", "");
        assert_eq!(
            effective_list(&[live, &bad], "Service", "ExecStart").len(),
            2
        );
        assert_eq!(
            effective(&[live], "Service", "Type").as_deref(),
            Some("simple")
        );
    }
}

#[test]
fn both_dropins_differ_only_in_bind_and_unit_name() {
    let a = DROP_8080
        .replace("127.0.0.1:8080", "BIND")
        .replace("console.service", "UNIT");
    let b = DROP_18080
        .replace("127.0.0.1:18080", "BIND")
        .replace("console-rust-primary.service", "UNIT")
        .replace("/console-rust-primary.service", "/UNIT");
    assert_eq!(a, b);
}

#[test]
fn bad_store_argv_is_refused_by_the_binary() {
    // The binary refuses the mistakes a hand edit could make (fail at start,
    // visible in `systemctl status`, instead of a silently open console).
    let base = exec_argv(&[LIVE_8080, DROP_8080]);
    let drop_pair = |name: &str| {
        let mut v = base[1..].to_vec();
        let i = v.iter().position(|a| a == name).unwrap();
        v.drain(i..i + 2);
        v
    };
    assert!(
        Config::from_args(drop_pair("--admin-store")).is_err(),
        "mirror without store"
    );
    let mut same = base[1..].to_vec();
    let i = same.iter().position(|a| a == "--admin-mirror").unwrap();
    same[i + 1] = STORE.into();
    assert!(Config::from_args(same).is_err(), "mirror == store");
    let mut rel = base[1..].to_vec();
    let i = rel.iter().position(|a| a == "--admin-store").unwrap();
    rel[i + 1] = "admin-device.json".into();
    assert!(Config::from_args(rel).is_err(), "relative store path");
    assert!(
        Config::from_args(drop_pair("--admin-mirror")).is_ok(),
        "mirror optional"
    );
}

#[test]
fn spdx_headers_and_mirror_marker() {
    for f in [LIVE_8080, LIVE_18080, DROP_8080, DROP_18080] {
        // Split so REUSE comply does not read this assertion as a second
        // license tag (the unit files themselves carry the real header).
        assert!(f.starts_with(concat!("# SPDX-", "License-Identifier: MIT\n")));
        assert!(f.contains("Copyright (c) 3000 Galion / galion-studio"));
    }
    for f in [LIVE_8080, LIVE_18080] {
        assert!(units::live_body(f).starts_with("# Mate t305u"));
    }
}
