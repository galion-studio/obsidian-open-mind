// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The Bookworm install script, the emulator job, and a files-only second run.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    let path = root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

const EMULATOR_LINE: &str =
    "pi-bookworm: emulator result, not a Raspberry Pi 5 result and not a Raspberry Pi OS boot";

#[test]
fn license_line_is_unchanged() {
    let lic = read("LICENSE");
    assert!(
        lic.starts_with("MIT License\n\nCopyright (c) 3000 Galion / galion-studio\n"),
        "LICENSE opening changed"
    );
}

#[test]
fn install_and_emulator_name_the_same_non_pi_result() {
    let install = read("scripts/linux/pi-bookworm-install.sh");
    let emulator = read("scripts/linux/pi-bookworm-emulator.sh");
    let workflow = read(".github/workflows/pi-seed.yml");
    let doc = read("docs/PI-BOOKWORM.md");
    for (name, text) in [
        ("install", install.as_str()),
        ("emulator", emulator.as_str()),
        ("doc", doc.as_str()),
    ] {
        assert!(
            text.contains(EMULATOR_LINE),
            "{name} missing the emulator line"
        );
    }
    assert!(workflow.contains("bookworm arm64 emulator"));
    assert!(workflow.contains("scripts/linux/pi-bookworm-emulator.sh"));
    assert!(workflow.contains("contents: read"));
    assert!(!workflow.contains("contents: write"));
    assert!(emulator.contains("debootstrap"));
    assert!(emulator.contains("--arch=arm64"));
    assert!(emulator.contains("bookworm"));
    assert!(emulator.contains("PI_BOOKWORM_EMULATOR=1"));
    assert!(emulator.contains("run_in --seed /opt/obsidian/seed"));
    assert_eq!(
        emulator.matches("run_in --seed /opt/obsidian/seed").count(),
        2,
        "the install must run twice"
    );
    assert!(emulator.contains("check-report"));
    assert!(install.contains("WantedBy=multi-user.target"));
    assert!(!install.contains("WantedBy=default.target"));
    assert!(doc.contains("not a Raspberry Pi 5 result"));
    assert!(read("docs/PI5-SEED.md").contains("not been run on Raspberry Pi 5"));
}

#[cfg(unix)]
fn aarch64_elf() -> Vec<u8> {
    let mut bytes = vec![0u8; 64];
    bytes[0] = 0x7f;
    bytes[1] = b'E';
    bytes[2] = b'L';
    bytes[3] = b'F';
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[18] = 0xb7;
    bytes
}

#[cfg(unix)]
fn run(args: &[&str]) -> std::process::Output {
    Command::new(root().join("scripts/linux/pi-bookworm-install.sh"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run install script")
}

#[cfg(unix)]
#[test]
fn files_only_second_run_is_unchanged_and_a_bad_elf_is_refused() {
    let base = std::env::temp_dir().join(format!("pi-bookworm-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let seed = base.join("seed");
    let dest = base.join("root");
    fs::create_dir_all(&seed).unwrap();
    let names = [
        "openmind-desktop",
        "openmind-memory",
        "openmind-nerve",
        "gaia-cortex",
        "galion-console-serve",
        "hermes-gateway",
        "galion-desk",
        "galion-one",
    ];
    for name in names {
        fs::write(seed.join(name), aarch64_elf()).unwrap();
    }
    let seed_s = seed.to_str().unwrap();
    let dest_s = dest.to_str().unwrap();
    let first = run(&["--files-only", "--root", dest_s, "--seed", seed_s]);
    assert!(
        first.status.success(),
        "first install failed\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_out = String::from_utf8_lossy(&first.stdout);
    assert!(first_out.contains("pi-bookworm: installed"));
    assert!(!first_out.contains("pi-bookworm: unchanged"));

    let unit =
        fs::read_to_string(dest.join("etc/systemd/system/obsidian-desktop.service")).unwrap();
    assert!(unit.contains("Copyright (c) 3000 Galion / galion-studio"));
    assert!(unit.contains("--listen 127.0.0.1:18181 --stack off"));
    assert!(unit.contains("WantedBy=multi-user.target"));
    assert!(!unit.contains("WantedBy=default.target"));
    let voice = fs::read_to_string(dest.join("etc/systemd/system/obsidian-one.service")).unwrap();
    assert!(voice.contains("ONE_MOUTH_BIND=127.0.0.1:8002"));
    assert!(voice.contains("ONE_BIND=127.0.0.1:8091"));

    let second = run(&["--files-only", "--root", dest_s, "--seed", seed_s]);
    assert!(second.status.success(), "second install failed");
    let second_out = String::from_utf8_lossy(&second.stdout);
    assert!(second_out.contains("pi-bookworm: unchanged"));
    assert!(!second_out.contains("pi-bookworm: installed"));

    fs::write(seed.join("openmind-desktop"), b"not an elf").unwrap();
    let bad = run(&["--files-only", "--root", dest_s, "--seed", seed_s]);
    assert!(!bad.status.success(), "a non-aarch64 seed was accepted");

    let host = run(&["--seed", seed_s]);
    assert!(
        !host.status.success(),
        "install without --files-only ran on this host"
    );

    let _ = fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn check_report_accepts_the_stub_voice_and_rejects_a_missing_service() {
    let base = std::env::temp_dir().join(format!("pi-bookworm-report-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let good = base.join("good.json");
    fs::write(
        &good,
        r#"{"read_only":true,"writes":0,"timeout_ms":2000,"probes":[{"service":"nerve","enabled":true,"state":"up","host":"localhost","port":8670,"path":"/health","http_status":200,"detail":"200","observed_mode":null},{"service":"cortex","enabled":true,"state":"up","host":"localhost","port":8674,"path":"/health","http_status":200,"detail":"200","observed_mode":"dry-run"},{"service":"console","enabled":true,"state":"up","host":"localhost","port":18080,"path":"/api/v1/health","http_status":200,"detail":"200","observed_mode":null},{"service":"hermes","enabled":true,"state":"up","host":"localhost","port":8642,"path":"/health","http_status":200,"detail":"200","observed_mode":null},{"service":"voice","enabled":true,"state":"down","host":"localhost","port":8002,"path":"/health","http_status":503,"detail":"http 503","observed_mode":null},{"service":"desk","enabled":true,"state":"up","host":"localhost","port":8646,"path":"/health","http_status":200,"detail":"200","observed_mode":null},{"service":"memory","enabled":true,"state":"up","host":"localhost","port":8420,"path":"/health","http_status":200,"detail":"200","observed_mode":null},{"service":"galion-one","enabled":true,"state":"up","host":"localhost","port":8091,"path":"/health","http_status":200,"detail":"200","observed_mode":null}]}"#,
    )
    .unwrap();
    let ok = run(&["check-report", good.to_str().unwrap()]);
    assert!(
        ok.status.success(),
        "good report rejected\n{}",
        String::from_utf8_lossy(&ok.stderr)
    );

    let bad = base.join("bad.json");
    fs::write(
        &bad,
        r#"{"read_only":true,"writes":0,"timeout_ms":2000,"probes":[{"service":"voice","enabled":true,"state":"up","host":"localhost","port":8002,"path":"/health","http_status":200,"detail":"200","observed_mode":null}]}"#,
    )
    .unwrap();
    let no = run(&["check-report", bad.to_str().unwrap()]);
    assert!(!no.status.success(), "a report missing nerve was accepted");
    let _ = fs::remove_dir_all(&base);
}
