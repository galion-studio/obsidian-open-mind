// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Unit-level tests of every check. Strings that the scanner would flag in
//! this very file are built at runtime, so `repo-check` passes on itself.

use repo_check::*;
use std::path::PathBuf;

const HDR: &str =
    "// SPDX-License-Identifier: MIT\n// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio\n";

fn checks(path: &str, text: &str) -> Vec<Check> {
    scan_text(path, text).into_iter().map(|f| f.check).collect()
}

fn mit(holder: &str) -> String {
    format!("MIT License\n\nCopyright (c) {holder}\n\nPermission is hereby granted, free of charge, to any person\n")
}

#[test]
fn license_ok_missing_and_pinned() {
    assert!(check_license(Some(&mit("2026 Slavik / Galion Studio")), None).is_empty());
    assert!(check_license(
        Some(&mit("2026 Slavik / Galion Studio")),
        Some("Galion Studio")
    )
    .is_empty());
    let pinned = check_license(
        Some(&mit("2026 Slavik / Galion Studio")),
        Some("3000 Galion"),
    );
    assert_eq!(pinned.len(), 1);
    assert_eq!(pinned[0].line, 3);
    assert_eq!(check_license(None, None).len(), 1);
    let apache = check_license(Some("Apache License\nVersion 2.0\n"), None);
    assert_eq!(apache.len(), 3);
}

#[test]
fn crate_license_must_match_root() {
    let root = mit("2026 X");
    assert!(check_crate_license("crates/a/LICENSE", &root, Some(&root)).is_empty());
    assert_eq!(
        check_crate_license("crates/a/LICENSE", &mit("2025 Y"), Some(&root)).len(),
        1
    );
}

#[test]
fn spdx_scope() {
    assert_eq!(
        checks("crates/a/src/lib.rs", "fn main() {}\n"),
        vec![Check::Spdx]
    );
    assert!(checks("crates/a/src/lib.rs", &format!("{HDR}fn main() {{}}\n")).is_empty());
    assert_eq!(
        checks("crates/a/Cargo.toml", "[package]\n"),
        vec![Check::Spdx]
    );
    assert_eq!(
        checks("deploy/x.sh", "#!/bin/sh\necho hi\n"),
        vec![Check::Spdx]
    );
    // Docs and files outside crates/ and deploy/ are not in scope.
    assert!(checks("docs/a.md", "# hi\n").is_empty());
    assert!(checks("tools/a.rs", "fn x() {}\n").is_empty());
    // Header too late does not count.
    let late = format!("\n\n\n\n\n{HDR}");
    assert_eq!(checks("crates/a/src/lib.rs", &late), vec![Check::Spdx]);
}

#[test]
fn cjk_detected_latin_and_croatian_ok() {
    let han = char::from_u32(0x4E2D).unwrap();
    let hangul = char::from_u32(0xD55C).unwrap();
    let kana = char::from_u32(0x30AB).unwrap();
    for ch in [han, hangul, kana] {
        assert!(is_cjk(ch));
        let f = scan_text("docs/a.md", &format!("ok\nbad {ch}\n"));
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].check, f[0].line), (Check::Cjk, 2));
    }
    assert!(checks(
        "docs/a.md",
        "Zagreb, \u{010d}\u{0107}\u{0161}\u{0111}\u{017e} - \u{2014} \u{00e9}\n"
    )
    .is_empty());
}

#[test]
fn python_files_and_shebangs() {
    assert_eq!(scan_path("tools/x.py")[0].check, Check::Python);
    assert_eq!(scan_path("a/b.pyc").len(), 1);
    assert!(scan_path("a/b.rs").is_empty());
    let shebang = format!("#!/usr/bin/env {}3\nprint(1)\n", "pyth".to_owned() + "on");
    assert_eq!(checks("bin/tool", &shebang), vec![Check::Python]);
    assert!(checks("bin/tool", "#!/bin/sh\n").is_empty());
}

#[test]
fn utc_reads_and_z_timestamps() {
    let now = format!("{HDR}let t = Utc::{}();\n", "now");
    let f = scan_text("crates/a/src/lib.rs", &now);
    assert_eq!(f.len(), 1);
    assert_eq!(
        (f[0].check, f[0].level, f[0].line),
        (Check::Utc, Level::Warn, 3)
    );
    // zagreb-time itself is the one allowed place.
    assert!(scan_text("crates/zagreb-time/src/lib.rs", &now)
        .iter()
        .all(|f| f.check != Check::Utc));
    assert!(has_z_timestamp(&format!("\"2026-10-18T19:00:00{}\"", "Z")));
    assert!(has_z_timestamp(&format!(
        "at 2026-10-18T19:00:00.123{}",
        "Z"
    )));
    assert!(!has_z_timestamp("2026-10-18T21:00:00+02:00"));
    assert!(!has_z_timestamp("2026-10-18 Z"));
    assert!(!has_z_timestamp("Z"));
}

#[test]
fn secrets_never_echo_value() {
    let tok = format!("{}{}", "gh", "p_") + &"A1b2".repeat(9);
    let line = format!("token = \"{tok}\"\n");
    let f = scan_text("docs/notes.md", &line);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].check, Check::Secrets);
    assert!(!f[0].msg.contains(&tok[4..]), "value must not be printed");

    let key = format!("-----BEGIN OPENSSH {} KEY-----\n", "PRIVATE");
    assert_eq!(checks("x/id", &key), vec![Check::Secrets]);
    let aws = format!("{}{}", "AK", "IA") + "ABCDEFGHIJ234567";
    assert_eq!(checks("x/env", &aws), vec![Check::Secrets]);
    let sk = format!("KEY={}{}", "s", "k-") + &"x9".repeat(12);
    assert_eq!(checks("x/env", &sk), vec![Check::Secrets]);
    // The allow marker does not silence secrets.
    let allowed = format!("{tok} // {ALLOW_MARKER}");
    assert_eq!(checks("x/env", &allowed), vec![Check::Secrets]);
}

#[test]
fn secrets_no_false_positives() {
    for ok in [
        "desk-hermes-and-a-very-long-crate-name-here",
        "task-0123456789abcdefghijkl",
        "ghp_short",
        "the prefix gh p_ alone",
        "-----BEGIN PUBLIC KEY-----",
    ] {
        assert!(checks("docs/a.md", ok).is_empty(), "{ok}");
    }
}

#[test]
fn hosts_public_only() {
    let public = format!("{}.{}.{}.{}", 51, 38, 137, 193);
    let f = scan_text("crates/a/src/x.txt", &format!("bind {public}:443\n"));
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].check, Check::Hosts);
    assert!(public_ipv4s(&format!("x={public}.")).len() == 1);
    for ok in [
        "127.0.0.1:8091",
        "0.0.0.0",
        "10.1.2.3 192.168.1.10 172.20.0.1",
        "169.254.1.1",
        "100.101.102.103",
        "192.0.2.10 198.51.100.7 203.0.113.9",
        "255.255.255.255",
        "version 1.2.3",
        "v1.2.3.4",
        "1.2.3.4.5",
        "999.1.1.1",
        "chrono 0.4.45",
    ] {
        assert!(public_ipv4s(ok).is_empty(), "{ok}");
    }
    // Out of scope dirs (docs) are not checked.
    assert!(checks("docs/a.md", &public).is_empty());
    // Allow marker works for hosts.
    let allowed = format!("{public} // {ALLOW_MARKER}");
    assert!(checks("deploy/a.conf", &allowed).is_empty());
}

#[test]
fn portable_rust_and_config() {
    let asm = format!("{HDR}unsafe {{ {}!(\"nop\") }}\n", "asm");
    assert_eq!(checks("crates/a/src/lib.rs", &asm), vec![Check::Portable]);
    let simd = format!("{HDR}use std::{}::x86_64::*;\n", "arch");
    assert_eq!(checks("crates/a/src/lib.rs", &simd), vec![Check::Portable]);
    let tf = format!("{HDR}#[target_{}(enable = \"avx2\")]\n", "feature");
    assert_eq!(checks("crates/a/src/lib.rs", &tf), vec![Check::Portable]);
    let gated = format!("{HDR}use core::{}::aarch64; // {ALLOW_MARKER}\n", "arch");
    assert!(checks("crates/a/src/lib.rs", &gated).is_empty());

    let cfg = format!(
        "[build]\nrustflags = [\"-C\", \"target-{}=native\"]\n",
        "cpu"
    );
    assert_eq!(checks(".cargo/config.toml", &cfg), vec![Check::Portable]);
    let lock = format!("[[package]]\nname = \"{}-sys\"\n", "openssl");
    assert_eq!(checks("Cargo.lock", &lock), vec![Check::Portable]);
    assert!(checks("Cargo.lock", "[[package]]\nname = \"rustls\"\n").is_empty());
}

#[test]
fn json_escape_works() {
    assert_eq!(json_escape("a\"b\\c\nd\u{1}"), "a\\\"b\\\\c\\nd\\u0001");
}

fn tmp_tree(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("repo-check-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("crates/a/src")).unwrap();
    std::fs::create_dir_all(d.join("target")).unwrap();
    std::fs::create_dir_all(d.join("contrib")).unwrap();
    std::fs::write(d.join("LICENSE"), mit("2026 Slavik / Galion Studio")).unwrap();
    std::fs::write(
        d.join("crates/a/src/lib.rs"),
        format!("{HDR}pub fn x() {{}}\n"),
    )
    .unwrap();
    // Skipped dirs and binaries never produce findings.
    std::fs::write(d.join("target/junk.py"), "x").unwrap();
    std::fs::write(d.join("contrib/vendored.py"), "x").unwrap();
    std::fs::write(d.join("crates/a/blob.bin"), [0u8, 1, 2, 0xff]).unwrap();
    d
}

#[test]
fn scan_tree_clean_then_dirty() {
    let d = tmp_tree("tree");
    let r = scan_tree(&d, Some("Galion Studio")).unwrap();
    assert!(r.findings.is_empty(), "{:?}", r.findings);
    assert_eq!(r.files_scanned, 2);

    std::fs::write(d.join("crates/a/src/bad.rs"), "fn y() {}\n").unwrap();
    std::fs::write(d.join("tool.py"), "").unwrap();
    std::fs::write(d.join("crates/a/LICENSE"), mit("1999 Other")).unwrap();
    let r = scan_tree(&d, None).unwrap();
    let mut got: Vec<_> = r
        .findings
        .iter()
        .map(|f| (f.check, f.file.as_str()))
        .collect();
    got.sort();
    assert_eq!(
        got,
        vec![
            (Check::License, "crates/a/LICENSE"),
            (Check::Spdx, "crates/a/src/bad.rs"),
            (Check::Python, "tool.py"),
        ]
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn check_names_roundtrip() {
    for c in Check::ALL {
        assert_eq!(Check::parse(c.name()), Some(c));
        assert!(!c.describe().is_empty());
    }
    assert_eq!(Check::parse("nope"), None);
}
