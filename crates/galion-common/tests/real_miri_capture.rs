// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-data check for the Miri pass (slot 190): raw HTTP responses captured
//! from the live VPS services with read-only GETs (`curl -s -i`) are run
//! through the client-side parsers (`parse_response_head`, `dechunk`) and
//! every `"ts":"..."` stamp through `time::require_rfc3339`.
//!
//! Gated by `GALION_MIRI_CAPTURE=<dir of *.http files>`; skipped otherwise.
//! Under Miri it needs the FS pass (`MIRIFLAGS=-Zmiri-disable-isolation`).
//! Captures are not committed (they are live service output).

use galion_common::{http, time};
use std::path::PathBuf;

fn captures() -> Option<Vec<PathBuf>> {
    let dir = std::env::var("GALION_MIRI_CAPTURE").ok()?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "http"))
        .collect();
    files.sort();
    Some(files)
}

/// Every `"ts":"<value>"` in a body.
fn stamps(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find("\"ts\":\"") {
        rest = &rest[i + 6..];
        if let Some(end) = rest.find('"') {
            out.push(&rest[..end]);
            rest = &rest[end..];
        }
    }
    out
}

#[test]
fn live_captures_parse() {
    let Some(files) = captures() else {
        eprintln!("skipped: set GALION_MIRI_CAPTURE=<dir>");
        return;
    };
    assert!(!files.is_empty(), "no *.http captures");
    let mut total_stamps = 0;
    for f in &files {
        let raw = std::fs::read(f).unwrap();
        let (status, _reason, headers, len) =
            http::parse_response_head(&raw).unwrap_or_else(|| panic!("{} head", f.display()));
        assert!(
            (100..600).contains(&status),
            "{} status {status}",
            f.display()
        );
        let body = &raw[len..];
        let body = if http::header(&headers, "transfer-encoding")
            .is_some_and(|v| v.eq_ignore_ascii_case("chunked"))
        {
            http::dechunk(body).unwrap_or_else(|| panic!("{} chunks", f.display()))
        } else {
            let cl: usize = http::header(&headers, "content-length")
                .unwrap_or_else(|| panic!("{} no length", f.display()))
                .parse()
                .unwrap();
            assert_eq!(cl, body.len(), "{} content-length", f.display());
            body.to_vec()
        };
        let text = String::from_utf8_lossy(&body);
        let ts = stamps(&text);
        for s in &ts {
            assert_eq!(time::require_rfc3339(s), Ok(()), "{} ts {s}", f.display());
        }
        total_stamps += ts.len();
        // Truncations of a real response never panic and are never complete
        // with a longer length than the input.
        for cut in (0..len).step_by(7) {
            if let Some((_, _, _, l)) = http::parse_response_head(&raw[..cut]) {
                assert!(l <= cut);
            }
        }
        eprintln!(
            "{}: {status}, head {len} B, body {} B, {} ts",
            f.file_name().unwrap().to_string_lossy(),
            body.len(),
            ts.len()
        );
    }
    eprintln!("{} captures, {total_stamps} stamps ok", files.len());
}
