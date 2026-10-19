// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Golden test: CLI text matches the defence.py `service status ms` lines,
//! and the default stdout is the JSON array of {service, status, ms}.

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_one-defence"))
}

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture() -> PathBuf {
    manifest().join("tests/fixtures/health.db")
}

fn golden(name: &str) -> String {
    std::fs::read_to_string(manifest().join("tests/fixtures").join(name)).unwrap()
}

fn output(cmd: &mut Command) -> std::process::Output {
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "status {:?}\nstderr {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "stderr {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

#[test]
fn golden_text_and_json_match_fixture() {
    let fx = fixture();
    let before = std::fs::read(&fx).unwrap();

    let text = output(bin().arg("--text").arg(&fx));
    assert_eq!(
        String::from_utf8(text.stdout).unwrap(),
        golden("health.golden.txt")
    );

    let json_default = output(bin().arg(&fx));
    let json_flag = output(bin().arg("--json").arg(&fx));
    assert_eq!(json_default.stdout, json_flag.stdout);
    assert_eq!(
        String::from_utf8(json_default.stdout).unwrap(),
        golden("health.golden.json")
    );

    // Library output is the same bytes the CLI prints (JSON plus the newline
    // the binary adds).
    let rows = one_defence::query_path(&fx).unwrap();
    assert_eq!(one_defence::to_text(&rows), golden("health.golden.txt"));
    assert_eq!(
        format!("{}\n", one_defence::to_json(&rows).unwrap()),
        golden("health.golden.json")
    );

    let after = std::fs::read(&fx).unwrap();
    assert_eq!(before, after, "fixture sqlite must not be written");
    for side in ["-journal", "-wal", "-shm"] {
        assert!(!Path::new(&format!("{}{side}", fx.display())).exists());
    }
}

#[test]
fn relative_path_matches_golden_json() {
    let out = output(
        bin()
            .current_dir(manifest())
            .arg("--json")
            .arg("tests/fixtures/health.db"),
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        golden("health.golden.json")
    );
}

#[test]
fn env_db_matches_golden_text() {
    let out = output(bin().env("ONE_DEFENCE_DB", fixture()).arg("--text"));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        golden("health.golden.txt")
    );
}

#[test]
fn path_with_space_opens() {
    let dir = std::env::temp_dir().join(format!("one defence {}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("health checks.db");
    std::fs::copy(fixture(), &db).unwrap();
    let before = std::fs::read(&db).unwrap();
    let rows = one_defence::query_path(&db).unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(std::fs::read(&db).unwrap(), before);
}

#[test]
fn missing_db_exits_1_and_creates_nothing() {
    let path = std::env::temp_dir().join(format!(
        "one-defence-missing-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&path);
    let out = bin().arg(&path).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!path.exists(), "mode=ro must not create a database");
    assert!(String::from_utf8_lossy(&out.stderr).contains("one-defence:"));
}

#[test]
fn bad_flag_and_both_modes_exit_2() {
    let bad = bin().arg("--nope").output().unwrap();
    assert_eq!(bad.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("unknown flag"));

    let both = bin().arg("--text").arg("--json").output().unwrap();
    assert_eq!(both.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&both.stderr).contains("only one"));
}

#[test]
fn help_exits_0_without_opening_a_database() {
    let out = bin().arg("--help").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("mode=ro"));
    assert!(stdout.contains("defence.py"));
}
