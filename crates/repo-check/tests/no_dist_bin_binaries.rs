// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Guard for D-428-2 (slot 445): release binaries are not kept in the source
//! tree. `dist/bin/` held 5 committed musl binaries (+ SHA256SUMS and
//! `.license` companions) until slot 445; they are listed in
//! `docs/release/dist-bin-removed-manifest.txt` and rebuilt from source with
//! `scripts/build-dist-bins.sh`. This test fails when an ELF (or any file
//! with a NUL byte) shows up under `dist/bin/` in a checkout again.
//! Local builds belong in `target/`, never in `dist/bin/`.

use std::path::Path;

fn is_binary(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x7fELF") || bytes.contains(&0)
}

#[test]
fn dist_bin_holds_no_binaries() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = root.join("dist/bin");
    let mut bad = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() && std::fs::read(&p).is_ok_and(|b| is_binary(&b)) {
                bad.push(p.file_name().unwrap_or_default().to_string_lossy().into_owned());
            }
        }
    }
    assert!(
        bad.is_empty(),
        "binaries under dist/bin/ (D-428-2: build with scripts/build-dist-bins.sh into target/): {bad:?}"
    );
}

#[test]
fn removed_manifest_lists_the_five_binaries() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let m = std::fs::read_to_string(root.join("docs/release/dist-bin-removed-manifest.txt"))
        .expect("manifest");
    for b in ["desk-hermes", "galion-desk", "galion-one", "link-watch", "room"] {
        let line = m
            .lines()
            .find(|l| l.split_whitespace().next() == Some(&format!("dist/bin/{b}")))
            .unwrap_or_else(|| panic!("{b} missing from manifest"));
        let cols: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(cols.len(), 4, "{line}");
        assert!(cols[1].parse::<u64>().is_ok_and(|n| n > 100_000), "{line}");
        assert_eq!(cols[2].len(), 64, "{line}");
    }
}

#[test]
fn detector_sees_elf_and_nul() {
    assert!(is_binary(b"\x7fELF\x02\x01"));
    assert!(is_binary(b"abc\0def"));
    assert!(!is_binary(b"# just text\n"));
}
