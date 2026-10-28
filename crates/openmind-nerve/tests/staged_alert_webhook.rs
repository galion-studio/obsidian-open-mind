// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 382: the staged drop-in
//! `deploy/staged/nerve/galion-nerve.service.d/50-alert-webhook.conf` parses
//! through the real config path, targets loopback only, and carries no
//! secret-looking material. Always runs (no network).

use openmind_nerve::alert_sink::{Format, WebhookConfig};
use openmind_nerve::alerts::Severity;
use std::collections::HashMap;
use std::path::Path;

const STAGED: &str = "../../deploy/staged/nerve/galion-nerve.service.d/50-alert-webhook.conf";

fn staged() -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(STAGED)).unwrap()
}

fn env_lines(s: &str) -> HashMap<String, String> {
    s.lines()
        .filter_map(|l| l.trim().strip_prefix("Environment="))
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn staged_dropin_is_valid_loopback_and_secret_free() {
    let s = staged();
    assert!(s.contains("[Service]"));
    assert!(s.contains("STAGED, NOT LIVE"));
    let env = env_lines(&s);
    assert!(
        env.keys().all(|k| k.starts_with("NERVE_ALERT_WEBHOOK")),
        "{env:?}"
    );
    let c = WebhookConfig::from_env(&|k| env.get(k).cloned())
        .expect("staged config accepted")
        .expect("configured");
    assert!(c.addr.ip().is_loopback());
    assert_eq!(c.format, Format::Ntfy);
    assert_eq!(c.min, Severity::Crit);
    assert_eq!(c.per_hour, 6);
    let low = s.to_ascii_lowercase();
    for bad in [
        "token=",
        "password",
        "secret=",
        "authorization",
        "bearer ",
        "@127",
        "?",
    ] {
        assert!(!low.contains(bad), "staged drop-in contains {bad:?}");
    }
    assert!(
        !s.contains("EnvironmentFile"),
        "no env file (secrets) in the staged drop-in"
    );
}
