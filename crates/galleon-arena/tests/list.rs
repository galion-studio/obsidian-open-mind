// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

#![forbid(unsafe_code)]

use std::fs;
use std::process::Command;

use galleon_arena::{
    dispatch, read_zagreb_now, redacted_homes, render_list, CliError, CliOut, ListError, SEED_IDS,
    SEED_TEAM_ID,
};

const STAMP: &str = "2026-10-19T16:21:00+02:00";

fn homes() -> std::path::PathBuf {
    redacted_homes().expect("redacted seed homes")
}

fn arg(text: &str) -> String {
    text.to_string()
}

#[test]
fn list_text_has_seed_ids_and_zagreb_stamp() {
    let dir = homes();
    let soul = fs::read(dir.join("g2-0").join("SOUL.md")).unwrap();
    let text = render_list(&dir, STAMP).unwrap();
    let after = fs::read(dir.join("g2-0").join("SOUL.md")).unwrap();
    assert_eq!(soul, after, "list rewrote a soul");

    assert!(text.starts_with("at 2026-10-19T16:21:00+02:00\n"));
    assert!(!text.contains('Z'));
    assert!(!text.contains("+00:00"));
    let ids: Vec<&str> = text
        .lines()
        .skip(1)
        .map(|line| line.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(ids, vec!["g2-0", "g2-1", "g2-2", "g2-3", SEED_TEAM_ID]);
    assert!(text.lines().any(|line| line == "g2-0 persona"));
    assert!(text.lines().any(|line| line == "g2-1 persona"));
    assert!(text.lines().any(|line| line == "g2-2 persona self-eval"));
    assert!(text.lines().any(|line| line == "g2-3 persona"));
    assert!(text
        .lines()
        .any(|line| line == "arena-compare team g2-0 g2-1 g2-2 g2-3"));
    for id in SEED_IDS {
        assert!(text.contains(id));
    }
}

#[test]
fn zulu_and_utc_offsets_are_refused() {
    let dir = homes();
    let err = render_list(&dir, "2026-10-19T16:21:00Z").unwrap_err();
    assert!(matches!(err, ListError::BadStamp(_)));
    let err = render_list(&dir, "2026-10-19T16:21:00+00:00").unwrap_err();
    assert!(matches!(err, ListError::BadStamp(_)));
    let winter = render_list(&dir, "2026-01-15T08:00:00+01:00").unwrap();
    assert!(winter.contains("g2-0"));
    assert!(winter.contains("+01:00"));
}

#[test]
fn missing_homes_are_not_created() {
    let missing = std::env::temp_dir().join(format!(
        "galleon-arena-list-absent-{}-b50",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&missing);
    assert!(!missing.exists());
    let err = render_list(&missing, STAMP).unwrap_err();
    assert!(matches!(err, ListError::Load(_)));
    assert!(!missing.exists());
    let args = vec![arg("list"), arg("--homes"), missing.display().to_string()];
    let err = dispatch(&args, STAMP).unwrap_err();
    assert!(matches!(err, CliError::Failed(_)));
    assert!(!missing.exists());
}

#[test]
fn help_and_unknown_commands() {
    let help = dispatch(&[arg("--help")], STAMP).unwrap();
    assert_eq!(help, CliOut::Help);
    let help = dispatch(&[arg("list"), arg("--help")], STAMP).unwrap();
    assert_eq!(help, CliOut::Help);
    let err = dispatch(&[arg("score")], STAMP).unwrap_err();
    assert!(matches!(err, CliError::Usage(_)));
    let err = dispatch(&[], STAMP).unwrap_err();
    assert!(matches!(err, CliError::Usage(_)));
    let err = dispatch(&[arg("list"), arg("--nope")], STAMP).unwrap_err();
    assert!(matches!(err, CliError::Usage(_)));
}

#[test]
fn default_list_uses_the_redacted_homes() {
    let out = dispatch(&[arg("list")], STAMP).unwrap();
    match out {
        CliOut::List(text) => {
            for id in SEED_IDS {
                assert!(text.contains(id), "{text}");
            }
            assert!(text.contains(STAMP));
        }
        CliOut::Help => panic!("list was treated as help"),
    }
}

#[test]
fn clock_is_zagreb_rfc3339_from_date() {
    let stamp = read_zagreb_now().unwrap();
    assert_eq!(stamp.len(), 25, "{stamp}");
    assert!(
        stamp.ends_with("+01:00") || stamp.ends_with("+02:00"),
        "{stamp}"
    );
    assert!(!stamp.ends_with('Z'), "{stamp}");
    assert!(!stamp.contains("+00:00"), "{stamp}");
    assert!(stamp.as_bytes()[10] == b'T', "{stamp}");
}

#[test]
fn binary_list_prints_ids_and_zagreb_timestamp() {
    let bin = env!("CARGO_BIN_EXE_galleon-arena");
    let out = Command::new(bin).arg("list").output().unwrap();
    assert!(
        out.status.success(),
        "stderr {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let stamp = text
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("at "))
        .unwrap_or("");
    assert!(
        stamp.ends_with("+01:00") || stamp.ends_with("+02:00"),
        "{text}"
    );
    assert!(!stamp.contains('Z'), "{stamp}");
    assert!(!text.contains('Z'), "{text}");
    assert!(!text.contains("+00:00"), "{text}");
    for id in ["g2-0", "g2-1", "g2-2", "g2-3"] {
        assert!(text.contains(id), "{text}");
    }
}
