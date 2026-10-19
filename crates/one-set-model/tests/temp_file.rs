// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Temp-file checks for the modes object `set-model.py` would write.
//! Live modes files are never opened.

use one_set_model::{apply, apply_file, render, validate, SetModelError};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("one-set-model-{pid}-{nanos}"));
        fs::create_dir(&path).expect("temp dir");
        Self { path }
    }

    fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn assert_not_live(path: &Path) {
    let text = path.to_string_lossy();
    assert!(
        !text.contains("/opt/galion/hermes/modes.json"),
        "refusing live modes path {text}"
    );
    assert!(
        !text.contains("/workspace/hermes/modes.json"),
        "refusing live modes path {text}"
    );
    let canon = path
        .parent()
        .and_then(|dir| dir.canonicalize().ok())
        .expect("temp parent");
    let tmp = std::env::temp_dir().canonicalize().expect("temp");
    assert!(canon.starts_with(&tmp), "temp path escaped: {canon:?}");
}

fn write_fixture(dir: &TempDir, body: &str) -> PathBuf {
    let path = dir.join("modes.json");
    assert_not_live(&path);
    fs::write(&path, body).expect("write fixture");
    path
}

#[test]
fn validate_lane_and_slash() {
    assert!(validate("plan", "fixture/plan").is_ok());
    assert!(validate("action", "fixture/action").is_ok());
    assert!(validate("plan", "/").is_ok());
    assert!(validate("action", "a/b/c").is_ok());
    assert!(matches!(
        validate("Plan", "fixture/plan"),
        Err(SetModelError::Lane)
    ));
    assert!(matches!(
        validate("action ", "fixture/plan"),
        Err(SetModelError::Lane)
    ));
    assert!(matches!(
        validate("both", "fixture/plan"),
        Err(SetModelError::Lane)
    ));
    assert!(matches!(validate("", "a/b"), Err(SetModelError::Lane)));
    assert!(matches!(
        validate("plan", "noslash"),
        Err(SetModelError::Model)
    ));
    assert!(matches!(validate("plan", ""), Err(SetModelError::Model)));
    assert!(matches!(
        apply(&json!([]), "nope", "noslash"),
        Err(SetModelError::Lane)
    ));
    assert!(matches!(
        apply(&json!([]), "plan", "noslash"),
        Err(SetModelError::Model)
    ));
}

#[test]
fn apply_keeps_other_keys_and_drops_extra_modes() {
    let doc = json!({
        "modes": {"plan": "old/plan", "action": "old/action", "extra": "drop"},
        "provider": "fixture",
        "saved": [{"name": "kept", "model": "kept/model"}],
        "stars": {"a": true}
    });
    let got = apply(&doc, "plan", "new/plan").expect("apply");
    assert_eq!(got["modes"]["plan"], "new/plan");
    assert_eq!(got["modes"]["action"], "old/action");
    assert!(got["modes"].get("extra").is_none());
    assert_eq!(got["modes"].as_object().expect("modes").len(), 2);
    assert_eq!(got["provider"], "fixture");
    assert_eq!(
        got["saved"],
        json!([{"name": "kept", "model": "kept/model"}])
    );
    assert_eq!(got["stars"], json!({"a": true}));
    assert_eq!(got.as_object().expect("root").len(), 4);

    let again = apply(&got, "plan", "new/plan").expect("stable");
    assert_eq!(got, again);

    let action = apply(&doc, "action", "new/action").expect("action");
    assert_eq!(action["modes"]["plan"], "old/plan");
    assert_eq!(action["modes"]["action"], "new/action");
}

#[test]
fn missing_modes_and_non_string_other_lane() {
    let bare = apply(&json!({"provider": "fixture"}), "action", "fixture/action").expect("bare");
    assert_eq!(bare["modes"]["plan"], "");
    assert_eq!(bare["modes"]["action"], "fixture/action");
    assert_eq!(bare["provider"], "fixture");

    let empty = apply(&json!({"modes": {}}), "plan", "v/m").expect("empty");
    assert_eq!(empty["modes"]["plan"], "v/m");
    assert_eq!(empty["modes"]["action"], "");

    let mixed = json!({"modes": {"plan": {"id": "keep/me"}, "action": null}});
    let got = apply(&mixed, "action", "new/action").expect("mixed");
    assert_eq!(got["modes"]["plan"], json!({"id": "keep/me"}));
    assert_eq!(got["modes"]["action"], "new/action");

    assert!(matches!(
        apply(&json!(null), "plan", "a/b"),
        Err(SetModelError::NotObject)
    ));
    assert!(matches!(
        apply(&json!({"modes": "x"}), "plan", "a/b"),
        Err(SetModelError::ModesNotObject)
    ));
    assert!(matches!(
        apply(&json!({"modes": []}), "plan", "a/b"),
        Err(SetModelError::ModesNotObject)
    ));
}

#[test]
fn temp_file_is_read_and_the_object_is_written_only_there() {
    let dir = TempDir::new();
    let body = "{\n  \"modes\": {\n    \"plan\": \"old/plan\",\n    \"action\": \"old/action\",\n    \"extra\": \"drop\"\n  },\n  \"provider\": \"fixture\"\n}\n";
    let path = write_fixture(&dir, body);
    let before = fs::read(&path).expect("read fixture");

    let missing = dir.join("missing.json");
    assert_not_live(&missing);
    assert!(matches!(
        apply_file(&missing, "plan", "noslash"),
        Err(SetModelError::Model)
    ));
    assert!(!missing.exists());
    assert!(matches!(
        apply_file(&missing, "plan", "fixture/plan"),
        Err(SetModelError::Io(_))
    ));

    let got = apply_file(&path, "plan", "fixture/plan").expect("apply_file");
    assert_eq!(fs::read(&path).expect("reread"), before);
    assert_eq!(got["modes"]["plan"], "fixture/plan");
    assert_eq!(got["modes"]["action"], "old/action");
    assert!(got["modes"].get("extra").is_none());
    assert_eq!(got["provider"], "fixture");

    let out = dir.join("would-write.json");
    assert_not_live(&out);
    let text = render(&got).expect("render");
    assert!(text.ends_with('\n'));
    fs::write(&out, &text).expect("write result");
    let parsed: Value = serde_json::from_str(&fs::read_to_string(&out).expect("read result"))
        .expect("parse result");
    assert_eq!(parsed, got);
    assert_eq!(fs::read(&path).expect("source still"), before);

    let cmd = Command::new(env!("CARGO_BIN_EXE_one-set-model"))
        .args(["action", "fixture/action", path.to_str().expect("utf8")])
        .output()
        .expect("bin");
    assert!(
        cmd.status.success(),
        "stderr {}",
        String::from_utf8_lossy(&cmd.stderr)
    );
    assert!(cmd.stderr.is_empty());
    let printed: Value = serde_json::from_slice(&cmd.stdout).expect("stdout json");
    assert_eq!(printed["modes"]["plan"], "old/plan");
    assert_eq!(printed["modes"]["action"], "fixture/action");
    assert_eq!(printed["provider"], "fixture");
    assert_eq!(fs::read(&path).expect("bin did not write"), before);

    let bad = Command::new(env!("CARGO_BIN_EXE_one-set-model"))
        .args(["plan", "noslash", path.to_str().expect("utf8")])
        .output()
        .expect("bin bad");
    assert_eq!(bad.status.code(), Some(1));
    assert!(bad.stdout.is_empty());
    assert_eq!(fs::read(&path).expect("bad bin did not write"), before);

    let usage = Command::new(env!("CARGO_BIN_EXE_one-set-model"))
        .output()
        .expect("usage");
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty());
}
