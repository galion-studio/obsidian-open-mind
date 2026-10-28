// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 416: static checks of the STAGED gateway deploy candidate
//! drop-in (deploy/staged/hermes-rs.service.d/95-gateway-candidate.conf).
//! It may only swap the binary: one ExecStart reset plus one tagged path,
//! no environment (coding runtime stays OFF, no secrets), sorts after the
//! live 90-deploy-t291u.conf, and documents the rollback to b7fbbc78.
//! Pure text checks, nothing is installed or started.

const DROPIN: &str =
    include_str!("../../../deploy/staged/hermes-rs.service.d/95-gateway-candidate.conf");
/// The live drop-in that sets ExecStart today (VPS read-only, 2026-10-28 18:03 CET).
const LIVE_DROPIN: &str = "90-deploy-t291u.conf";
const LIVE_BIN: &str = "/opt/galion/rust/bin/hermes-gateway-b7fbbc78";

fn directives(text: &str) -> Vec<(&str, &str)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('['))
        .filter_map(|l| l.split_once('='))
        .collect()
}

fn hex(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[test]
fn dropin_only_swaps_the_binary() {
    let d = directives(DROPIN);
    let keys: Vec<&str> = d.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        keys,
        ["ExecStart", "ExecStart", "SyslogIdentifier"],
        "{d:?}"
    );
    assert_eq!(d[0].1, "", "ExecStart must be reset first");
    assert_eq!(d[2].1, "hermes-gateway");
    let exe = d[1].1;
    let tag = exe
        .strip_prefix("/opt/galion/rust/bin/hermes-gateway-")
        .unwrap_or_else(|| panic!("unexpected path {exe}"));
    assert!(hex(tag) && tag.len() >= 7, "commit tag {tag:?}");
    assert_ne!(exe, LIVE_BIN, "candidate must not reuse the live path");
    assert!(!exe.contains(' '), "no arguments (no --mock): {exe}");
    assert!(DROPIN.contains(&format!("hermes-gateway-{tag} = land tree")));
}

#[test]
fn dropin_sets_no_environment_and_no_coding() {
    for (k, v) in directives(DROPIN) {
        assert!(!k.starts_with("Environment"), "{k}={v}");
        assert!(!v.contains("HERMES_RS_CODING"), "{k}={v}");
    }
    let low = DROPIN.to_ascii_lowercase();
    for bad in ["api_server_key=", "token=", "secret=", "password="] {
        assert!(!low.contains(bad), "secret-looking text: {bad}");
    }
}

#[test]
fn dropin_overrides_the_live_one_and_documents_rollback() {
    let name = "95-gateway-candidate.conf";
    assert!(name > LIVE_DROPIN, "systemd applies drop-ins in name order");
    assert!(DROPIN.starts_with("# SPDX-License-Identifier: MIT"));
    assert!(DROPIN.contains("STAGED, NOT INSTALLED"));
    assert!(DROPIN.contains("Rollback to live b7fbbc78"));
    assert!(DROPIN.contains(&format!(
        "rm /etc/systemd/system/hermes-rs.service.d/{name}"
    )));
    assert!(DROPIN.contains(LIVE_DROPIN));
    let sha = DROPIN
        .lines()
        .find_map(|l| l.strip_prefix("# sha256 "))
        .expect("sha256 line");
    assert!(sha.len() == 64 && hex(sha), "{sha}");
}
