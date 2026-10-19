// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! The rendered unit contains Restart=always and a Zagreb timestamp comment,
//! and does not contain systemctl.

#![forbid(unsafe_code)]

use openmind_units::{render_fixture, render_fixture_now, write_rendered, RenderedUnit};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const STAMP: &str = "2026-10-19T16:45:00+02:00";
const WINTER: &str = "2026-12-01T13:00:00+01:00";

const MINIMAL: &str = r#"
[[unit]]
name = "demo"
description = "Demo unit"
exec_start = "/bin/true"
"#;

fn fixture() -> &'static str {
    include_str!("fixtures/openmind.toml")
}

fn stamp_of(text: &str) -> &str {
    let line = text
        .lines()
        .find(|line| line.starts_with("# Rendered: "))
        .expect("zagreb timestamp comment");
    line.trim_start_matches("# Rendered: ")
        .trim_end_matches(" Europe/Zagreb")
}

fn assert_policy(text: &str) {
    assert!(
        text.contains("Restart=always"),
        "missing Restart=always:\n{text}"
    );
    let line = text
        .lines()
        .find(|line| line.starts_with("# Rendered: ") && line.ends_with(" Europe/Zagreb"))
        .expect("zagreb timestamp comment");
    let stamp = stamp_of(line);
    assert!(
        stamp.ends_with("+01:00") || stamp.ends_with("+02:00"),
        "stamp {stamp} is not a Zagreb offset"
    );
    assert!(!stamp.contains('Z') && !stamp.contains('z'), "{stamp}");
    assert!(
        !text.to_ascii_lowercase().contains("systemctl"),
        "rendered unit contains systemctl"
    );
    assert!(text.lines().all(|line| line.trim() != "[Install]"));
}

fn staged_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/boot/rendered")
}

#[test]
fn fixture_units_restart_always_zagreb_stamp_no_systemctl() {
    let units = render_fixture(fixture(), STAMP).expect("render");
    assert_eq!(units.len(), 4);
    for unit in &units {
        assert!(unit
            .text
            .contains(&format!("# Rendered: {STAMP} Europe/Zagreb")));
        assert_policy(&unit.text);
        assert!(unit.text.contains("# Staged and not installed."));
    }
}

#[test]
fn fixture_substitutes_ports_hosts_and_paths() {
    let units = render_fixture(fixture(), STAMP).expect("render");
    let one = units.iter().find(|unit| unit.name == "galion-one").unwrap();
    assert!(one
        .text
        .contains("ExecStart=/opt/galion/openmind/dist/bin/galion-one"));
    assert!(one.text.contains("Environment=ONE_BIND=127.0.0.1:8091"));
    assert!(one
        .text
        .contains("Environment=ONE_MOUTH_BIND=127.0.0.1:8002"));
    assert!(one.text.contains("RestartSec=3"));
    let desk = units
        .iter()
        .find(|unit| unit.name == "galion-desk")
        .unwrap();
    assert!(desk.text.contains("Environment=DESK_BIND=127.0.0.1:8646"));
    assert!(desk
        .text
        .contains("Environment=DESK_HERMES_BIN=/opt/galion/openmind/dist/bin/desk-hermes"));
    let link = units.iter().find(|unit| unit.name == "link-watch").unwrap();
    assert!(link
        .text
        .contains("ExecStart=/opt/galion/openmind/dist/bin/link-watch"));
    let tuner = units.iter().find(|unit| unit.name == "tuner").unwrap();
    assert!(tuner.text.contains("Environment=TUNER_ADDR=127.0.0.1:8650"));
    assert!(tuner
        .text
        .contains("ExecStart=/opt/galion/openmind/dist/bin/tuner"));
    assert!(tuner.text.contains("Restart=always"));
    assert!(tuner.text.contains("Europe/Zagreb"));
    assert!(!tuner.text.to_ascii_lowercase().contains("systemctl"));
    assert!(tuner.text.lines().all(|line| line.trim() != "[Install]"));
    assert!(!tuner
        .text
        .lines()
        .any(|line| line.trim().starts_with("WantedBy=")));
}

#[test]
fn winter_offset_is_accepted() {
    let units = render_fixture(MINIMAL, WINTER).expect("winter stamp");
    assert!(units[0]
        .text
        .contains(&format!("# Rendered: {WINTER} Europe/Zagreb")));
    assert_policy(&units[0].text);
}

#[test]
fn now_stamp_is_zagreb_offset_not_zulu() {
    let units = render_fixture_now(MINIMAL).expect("now");
    assert_policy(&units[0].text);
    assert_eq!(stamp_of(&units[0].text).len(), 25);
}

#[test]
fn rejects_zulu_stamp() {
    let err = render_fixture(MINIMAL, "2026-10-19T16:45:00Z").unwrap_err();
    let message = err.to_string();
    assert!(message.contains("Europe/Zagreb"));
    assert!(!message.to_ascii_lowercase().contains("systemctl"));
}

#[test]
fn rejects_utc_offset() {
    let err = render_fixture(MINIMAL, "2026-10-19T14:45:00+00:00").unwrap_err();
    assert!(err.to_string().contains("Europe/Zagreb"));
}

#[test]
fn rejects_non_always_restart() {
    let toml = r#"
[[unit]]
name = "demo"
description = "Demo unit"
exec_start = "/bin/true"
restart = "on-failure"
"#;
    let err = render_fixture(toml, STAMP).unwrap_err();
    assert!(err.to_string().contains("restart must be always"));
}

#[test]
fn rejects_forbidden_token_in_description() {
    let toml = r#"
[[unit]]
name = "demo"
description = "do not call systemctl"
exec_start = "/bin/true"
"#;
    let err = render_fixture(toml, STAMP).unwrap_err();
    assert!(err.to_string().contains("plain line") || err.to_string().contains("forbidden"));
}

#[test]
fn rejects_duplicate_name() {
    let toml = r#"
[[unit]]
name = "demo"
description = "Demo unit"
exec_start = "/bin/true"

[[unit]]
name = "demo"
description = "Demo unit again"
exec_start = "/bin/true"
"#;
    let err = render_fixture(toml, STAMP).unwrap_err();
    assert!(err.to_string().contains("duplicate"));
}

#[test]
fn write_rendered_roundtrip() {
    let units = render_fixture(fixture(), STAMP).expect("render");
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("om-b55-rt-{pid}"));
    let _ = fs::remove_dir_all(&dir);
    write_rendered(&dir, &units).expect("write");
    for unit in &units {
        let got = fs::read_to_string(dir.join(&unit.file_name)).expect("read");
        assert_eq!(got, unit.text);
        assert_policy(&got);
    }
    fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn write_refuses_a_path_separator() {
    let unit = RenderedUnit {
        name: "demo".to_string(),
        file_name: "../x.service".to_string(),
        text: "nope\n".to_string(),
    };
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("om-b55-nope-{pid}"));
    let _ = fs::remove_dir_all(&dir);
    let err = write_rendered(&dir, &[unit]).unwrap_err();
    assert!(err.to_string().contains("refused file name"));
    assert!(!dir.exists());
}

#[test]
fn binary_prints_unit_text() {
    // clippy --all-targets does not set CARGO_BIN_EXE_*. cargo test does.
    let Some(bin) = option_env!("CARGO_BIN_EXE_openmind_units") else {
        return;
    };
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/openmind.toml");
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("om-b55-bin-{pid}"));
    let _ = fs::remove_dir_all(&dir);
    let output = Command::new(bin)
        .arg("--fixture")
        .arg(&fixture)
        .arg("--stamp")
        .arg(STAMP)
        .arg("--out")
        .arg(&dir)
        .output()
        .expect("run renderer");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr {stderr}");
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert!(output.stderr.is_empty());
    assert_eq!(stdout.matches("Restart=always").count(), 4);
    assert!(stdout.contains(&format!("# Rendered: {STAMP} Europe/Zagreb")));
    assert!(!stdout.to_ascii_lowercase().contains("systemctl"));
    assert!(stdout.contains("# file: galion-one.service"));
    assert!(stdout.contains("# file: tuner.service"));
    assert!(stdout.contains("TUNER_ADDR=127.0.0.1:8650"));
    let disk = fs::read_to_string(dir.join("galion-one.service")).expect("disk");
    assert_policy(&disk);
    assert!(!disk.contains("# file:"));
    fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn staged_files_match_renderer_and_are_not_install_sections() {
    let dir = staged_dir();
    let sample = fs::read_to_string(dir.join("galion-one.service")).expect("staged galion-one");
    let stamp = stamp_of(&sample).to_string();
    let units = render_fixture(fixture(), &stamp).expect("re-render");
    let mut found = Vec::new();
    for entry in fs::read_dir(&dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("service") {
            found.push(path);
        }
    }
    found.sort();
    let mut expected: Vec<PathBuf> = units.iter().map(|unit| dir.join(&unit.file_name)).collect();
    expected.sort();
    assert_eq!(found, expected);
    for unit in &units {
        let disk = fs::read_to_string(dir.join(&unit.file_name)).expect("read staged");
        assert_eq!(disk, unit.text);
        assert_policy(&disk);
        assert!(disk.contains("Restart=always"));
        assert!(!disk.to_ascii_lowercase().contains("systemctl"));
    }
}

#[test]
fn docs_say_staged_and_not_installed() {
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/boot-units.md");
    let text = fs::read_to_string(docs).expect("docs");
    let lower = text.to_ascii_lowercase();
    assert!(lower.contains("staged and not installed"));
    assert!(lower.contains("deploy/boot/rendered"));
}
