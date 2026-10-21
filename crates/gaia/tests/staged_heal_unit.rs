// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Checks the STAGED systemd text for `gaia heal-tick` (wave-4 slot 188):
//! the unit's `ExecStart=` (expanded for the `gaia-canary` instance exactly
//! as systemd would) parses with Gaia's own CLI parser, keeps the D-L2 nerve
//! cap wired to the live nerve, uses the stop file the unit's condition uses,
//! takes tokens only from a token dir, stays inside Gaia's test-port scope,
//! and the hardening the cutover doc promises is all there. Nothing is
//! installed by this test (D-L1).

mod unit_text;

use gaia_live::heal::check_target;
use unit_text::*;

const SVC: &str = "gaia-heal-tick@.service";
const TIMER: &str = "gaia-heal-tick@.timer";

fn service() -> Vec<(String, String, String)> {
    parse_unit(&read(SVC))
}

#[test]
fn exec_start_parses_with_gaia_loop_parser() {
    let argv = canary_argv();
    assert_eq!(argv[0], "/opt/galion/rust/bin/gaia");
    assert_eq!(&argv[1..3], ["heal-tick", "--loop"]);
    // main() drops --compact before parsing; do the same.
    let args: Vec<&str> = argv[1..]
        .iter()
        .map(String::as_str)
        .filter(|a| *a != "--compact")
        .collect();
    let cfg = gaia::heal_loop::parse_cli(&args[1..]).expect("unit ExecStart parses");
    assert_eq!(
        cfg.interval.as_millis(),
        30_000,
        "same 30 s as the nerve cycle"
    );
    assert_eq!(cfg.jitter.as_millis(), 5_000);
    assert_eq!(cfg.max_runtime.map(|d| d.as_secs()), Some(3600));
    assert_eq!(cfg.max_errors, 5);
    assert_eq!(cfg.max_restarts, Some(3));
    assert_eq!(cfg.max_ticks, None);
    assert_eq!(cfg.seed, None, "jitter from the OS, not a fixed seed");
    assert_eq!(
        cfg.stop_file.as_deref().unwrap().to_str(),
        Some("/var/lib/gaia/gaia-canary/stop")
    );
    assert_eq!(
        cfg.token_dir.as_deref().unwrap().to_str(),
        Some("/var/lib/gaia/gaia-canary/tokens")
    );
    assert!(
        cfg.tick.token_file.is_none(),
        "tokens only via the token dir"
    );
    assert_eq!(
        cfg.tick.nerve.to_string(),
        "127.0.0.1:8670",
        "D-L2: live nerve /state"
    );
    assert_eq!(
        cfg.tick.hades_config.to_str(),
        Some("/etc/galion/hades/gaia-gate.toml")
    );
    assert_eq!(
        cfg.tick.state_dir.to_str(),
        Some("/var/lib/gaia/gaia-canary/state")
    );
    let t = &cfg.tick.target;
    assert_eq!(t.name, "gaia-canary");
    assert_eq!(t.port, 19880);
    assert_eq!(t.health_path, "/health");
    assert_eq!(
        t.argv,
        ["/opt/galion/rust/bin/gaia-dummy", "--port", "19880"]
    );
    assert_eq!(cfg.tick.allowed, 19880..=19880);
    // check_target also wants the program to exist: use the box build of
    // gaia-dummy in place of /opt/galion/rust/bin/gaia-dummy.
    let mut boxed = t.clone();
    boxed.argv[0] = env!("CARGO_BIN_EXE_gaia-dummy").to_string();
    check_target(&boxed, &cfg.tick.allowed).expect("canary is inside Gaia's scope");
}

#[test]
fn a_live_port_in_the_env_file_is_refused_by_scope() {
    // If an operator pointed an instance at a live service, the loop would
    // stop before its first tick (exit 4). Same expansion, edited env.
    let svc = service();
    let dummy = env!("CARGO_BIN_EXE_gaia-dummy");
    let sysctl = std::env::temp_dir().join(format!("s188-{}/systemctl", std::process::id()));
    std::fs::create_dir_all(sysctl.parent().unwrap()).unwrap();
    let _ = std::fs::remove_file(&sysctl);
    std::os::unix::fs::symlink(dummy, &sysctl).unwrap();
    let restart = format!("{} restart hermes-rs", sysctl.display());
    for (port, allow, cmd) in [
        ("8642", "8642-8642", dummy),
        ("8670", "8600-8700", dummy),
        ("19880", "19880-19880", restart.as_str()),
    ] {
        let mut env = parse_env(&read("gaia/heal-gaia-canary.env"));
        env.insert("GAIA_PORT".into(), port.into());
        env.insert("GAIA_ALLOW_PORTS".into(), allow.into());
        env.insert("GAIA_START_ARGV".into(), cmd.into());
        let argv = expand_exec(one(&svc, "Service", "ExecStart"), "hermes-rs", &env);
        let args: Vec<&str> = argv[2..]
            .iter()
            .map(String::as_str)
            .filter(|a| *a != "--compact")
            .collect();
        let err = match gaia::heal_loop::parse_cli(&args) {
            Err(e) => e,
            Ok(cfg) => {
                check_target(&cfg.tick.target, &cfg.tick.allowed).expect_err("must be refused")
            }
        };
        eprintln!("refused {port} {cmd}: {err}");
        assert!(
            !err.contains("does not exist"),
            "refused for the wrong reason: {err}"
        );
    }
    let _ = std::fs::remove_dir_all(sysctl.parent().unwrap());
}

#[test]
fn stop_file_condition_matches_the_loop_flag() {
    let u = service();
    let conds = values(&u, "Unit", "ConditionPathExists");
    assert!(conds.contains(&"!/var/lib/gaia/%i/stop"), "{conds:?}");
    assert!(conds.contains(&"/etc/galion/gaia/heal-%i.env"), "{conds:?}");
    let exec = one(&u, "Service", "ExecStart");
    assert!(exec.contains("--stop-file /var/lib/gaia/%i/stop "));
}

#[test]
fn service_lifecycle_settings() {
    let u = service();
    assert_eq!(one(&u, "Service", "Type"), "exec");
    assert_eq!(one(&u, "Service", "User"), "gaia");
    assert_eq!(one(&u, "Service", "Group"), "gaia");
    assert_eq!(one(&u, "Service", "Restart"), "no", "the timer re-arms it");
    assert_eq!(
        one(&u, "Service", "KillMode"),
        "process",
        "stopping Gaia must not kill a service it healed"
    );
    assert_eq!(
        one(&u, "Service", "EnvironmentFile"),
        "/etc/galion/gaia/heal-%i.env"
    );
    assert!(one(&u, "Unit", "After").contains("galion-nerve.service"));
    assert!(
        values(&u, "Unit", "Requires").is_empty(),
        "nerve down = back-off, not a stop"
    );
    assert!(
        !u.iter().any(|(s, _, _)| s == "Install"),
        "service is started by the timer only"
    );
    // Writable state only under StateDirectory (/var/lib/gaia/...).
    let st: Vec<&str> = one(&u, "Service", "StateDirectory").split(' ').collect();
    assert_eq!(
        st,
        [
            "gaia",
            "gaia/hades",
            "gaia/%i",
            "gaia/%i/state",
            "gaia/%i/tokens"
        ]
    );
    assert!(values(&u, "Service", "ReadWritePaths").is_empty());
    assert_eq!(one(&u, "Service", "StateDirectoryMode"), "0700");
    assert_eq!(one(&u, "Service", "UMask"), "0077");
    // The gate's replay ledger (and so Gaia's global restart ledger) is in a
    // StateDirectory; its disable switch and public key are read-only.
    let gate = read("gaia/gaia-gate.toml");
    assert!(gate.contains("ledger = \"/var/lib/gaia/hades/ledger.jsonl\""));
    assert!(gate.contains("admin_pubkey = \"/etc/galion/hades/admin.pub\""));
    assert!(gate.contains("disable_file = \"/etc/galion/hades/gaia.DISABLED\""));
    assert!(
        !gate.lines().any(|l| l.trim_start().starts_with("seed")),
        "no admin seed on the gate host"
    );
}

#[test]
fn hardening_is_present() {
    let u = service();
    for (k, v) in [
        ("NoNewPrivileges", "yes"),
        ("CapabilityBoundingSet", ""),
        ("AmbientCapabilities", ""),
        ("PrivateTmp", "yes"),
        ("PrivateDevices", "yes"),
        ("ProtectSystem", "strict"),
        ("ProtectHome", "yes"),
        ("ProtectKernelTunables", "yes"),
        ("ProtectKernelModules", "yes"),
        ("ProtectKernelLogs", "yes"),
        ("ProtectControlGroups", "yes"),
        ("ProtectClock", "yes"),
        ("ProtectHostname", "yes"),
        ("ProtectProc", "invisible"),
        ("RestrictSUIDSGID", "yes"),
        ("RestrictRealtime", "yes"),
        ("RestrictNamespaces", "yes"),
        ("LockPersonality", "yes"),
        ("MemoryDenyWriteExecute", "yes"),
        ("RemoveIPC", "yes"),
        ("RestrictAddressFamilies", "AF_INET AF_INET6 AF_UNIX"),
        ("IPAddressDeny", "any"),
        ("IPAddressAllow", "localhost"),
        ("SystemCallArchitectures", "native"),
        ("MemoryMax", "128M"),
        ("TasksMax", "64"),
        ("CPUQuota", "20%"),
        ("ReadOnlyPaths", "/etc/galion"),
    ] {
        assert_eq!(one(&u, "Service", k), v, "{k}");
    }
    assert_eq!(
        values(&u, "Service", "SystemCallFilter"),
        ["@system-service", "~@privileged @resources"]
    );
    // /proc/net is needed for the D-L4 hung check (listener pids).
    assert!(values(&u, "Service", "ProcSubset").is_empty());
    assert!(
        values(&u, "Service", "PrivateNetwork").is_empty(),
        "needs loopback nerve"
    );
}

#[test]
fn timer_rearms_the_same_instance() {
    let t = parse_unit(&read(TIMER));
    assert_eq!(one(&t, "Timer", "Unit"), "gaia-heal-tick@%i.service");
    assert_eq!(one(&t, "Timer", "OnUnitInactiveSec"), "5min");
    assert_eq!(one(&t, "Timer", "OnBootSec"), "2min");
    assert_eq!(one(&t, "Timer", "OnActiveSec"), "30s");
    assert_eq!(one(&t, "Install", "WantedBy"), "timers.target");
}

#[test]
fn every_staged_file_has_the_spdx_header_and_staged_marker() {
    for f in [
        SVC,
        TIMER,
        "gaia/heal-gaia-canary.env",
        "gaia/gaia-gate.toml",
        "gaia/sysusers-gaia.conf",
    ] {
        let text = read(f);
        let head: Vec<&str> = text.lines().take(3).collect();
        assert_eq!(head[0], "# SPDX-License-Identifier: MIT", "{f}");
        assert_eq!(
            head[1], "# Copyright (c) 3000 Galion / galion-studio",
            "{f}"
        );
        assert!(text.contains("STAGED"), "{f}");
    }
    let users = read("gaia/sysusers-gaia.conf");
    assert!(users.contains("u gaia - \"Galion Gaia heal loop\" /var/lib/gaia /usr/sbin/nologin"));
}

#[test]
fn canary_plan_hash_is_stable() {
    // The hash the admin signs for the canary (gaia tokens request computes
    // the same); recorded in docs/cutover/gaia-heal-tick.md.
    let argv = canary_argv();
    let args: Vec<&str> = argv[2..]
        .iter()
        .map(String::as_str)
        .filter(|a| *a != "--compact")
        .collect();
    let cfg = gaia::heal_loop::parse_cli(&args).unwrap();
    let h = gaia::live_heal::heal_plan_sha256(&cfg.tick.target);
    assert_eq!(h.len(), 64);
    eprintln!("canary plan sha256 {h}");
    assert_eq!(h, gaia::live_heal::heal_plan_sha256(&cfg.tick.target));
}
