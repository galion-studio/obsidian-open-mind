// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Ops S7.3: the LIVE binary table in `docs/ops/live-binary-table.md`.
//!
//! Offline (always): the table between the `live-table` markers parses, every
//! sha256 is 64 lowercase hex, units are unique and well-formed, paths are
//! absolute, and the document carries no public IPv4 address and no UTC/`Z`
//! timestamps.
//!
//! Live (`GALION_REAL=1`): one READ-ONLY ssh batch to the VPS (host alias from
//! `GALION_VPS_SSH`, default `slavik-vps`) running only `systemctl show -p`,
//! `sha256sum` and `stat -L` per row. Fails on any state, exe sha, file sha or
//! size drift. Nothing is written, started or POSTed; stderr of ssh is never
//! printed (it could name the host).

use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
struct Row {
    unit: String,
    state: String,
    path: String,
    size: u64,
    sha: String,
}

fn doc() -> String {
    // GALION_LIVE_TABLE_DOC lets a negative run point at a tampered copy.
    let p = std::env::var_os("GALION_LIVE_TABLE_DOC")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/ops/live-binary-table.md")
        });
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn safe(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '/' | '-'))
}

fn parse(doc: &str) -> Vec<Row> {
    let start = doc.find("<!-- live-table:begin -->").expect("begin marker");
    let end = doc.find("<!-- live-table:end -->").expect("end marker");
    let mut rows = Vec::new();
    for line in doc[start..end].lines() {
        let line = line.trim();
        if !line.starts_with('|') || line.starts_with("| Unit") || line.starts_with("|---") {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        assert_eq!(cells.len(), 9, "9 columns: {line}");
        let state = cells[1].replace("**", "");
        let state = state.rsplit('/').next().unwrap_or("").to_string();
        let path = cells[2].split_whitespace().next().unwrap_or("").to_string();
        let size = cells[3]
            .parse::<u64>()
            .unwrap_or_else(|_| panic!("size not a number: {line}"));
        let sha = cells[5].trim_matches('`').to_string();
        rows.push(Row {
            unit: cells[0].to_string(),
            state,
            path,
            size,
            sha,
        });
    }
    rows
}

fn is_sha(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Dotted quads that are not loopback / unspecified.
fn public_ipv4(doc: &str) -> Vec<String> {
    let mut out = Vec::new();
    for tok in doc.split(|c: char| !(c.is_ascii_digit() || c == '.')) {
        let parts: Vec<&str> = tok.trim_matches('.').split('.').collect();
        if parts.len() == 4
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.len() <= 3 && p.parse::<u16>().is_ok_and(|n| n < 256))
            && parts[0] != "127"
            && parts[0] != "0"
        {
            out.push(tok.to_string());
        }
    }
    out
}

/// `…T12:34:56Z` style stamps or the word UTC.
fn utc_stamps(doc: &str) -> usize {
    let b = doc.as_bytes();
    let mut n = doc.matches("UTC").count();
    for i in 8..b.len() {
        if b[i] == b'Z'
            && b[i - 1].is_ascii_digit()
            && b[i - 2].is_ascii_digit()
            && b[i - 3] == b':'
        {
            n += 1;
        }
    }
    n
}

#[test]
fn live_table_offline() {
    let d = doc();
    let rows = parse(&d);
    assert!(rows.len() >= 12, "expected >= 12 rows, got {}", rows.len());
    let mut seen = std::collections::BTreeSet::new();
    for r in &rows {
        assert!(safe(&r.unit) && !r.unit.contains('/'), "unit {:?}", r.unit);
        assert!(seen.insert(r.unit.clone()), "duplicate unit {}", r.unit);
        assert!(
            r.path.starts_with('/') && safe(&r.path),
            "path {:?}",
            r.path
        );
        assert!(is_sha(&r.sha), "sha for {}: {:?}", r.unit, r.sha);
        assert_eq!(r.state, "active", "{} state", r.unit);
        assert!(r.size > 0);
    }
    for must in [
        "galion-nerve",
        "gaia-cortex",
        "hermes-rs",
        "openmind-memory",
    ] {
        assert!(seen.contains(must), "row {must} missing");
    }
    assert!(public_ipv4(&d).is_empty(), "public IPv4 in doc");
    assert_eq!(utc_stamps(&d), 0, "UTC/Z timestamps in doc");
}

#[test]
fn helpers_catch_bad_input() {
    assert!(!public_ipv4("x 203.0.113.7 y").is_empty());
    assert!(public_ipv4("127.0.0.1:8670 0.0.0.0 v1.2.3").is_empty());
    assert_eq!(utc_stamps(&format!("2026-10-21T21:00:00{}", 'Z')), 1);
    assert_eq!(utc_stamps("2026-10-21T23:00:00+02:00"), 0);
    assert!(!is_sha("ABC"));
    assert!(!safe("a;rm -rf /"));
    assert!(!safe("$(x)"));
}

#[test]
fn real_live_table_vps() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 (read-only ssh to the VPS)");
        return;
    }
    let host = std::env::var("GALION_VPS_SSH").unwrap_or_else(|_| "slavik-vps".into());
    assert!(safe(&host), "bad host alias");
    let rows = parse(&doc());
    let mut script = String::from("export LC_ALL=C\n");
    for r in &rows {
        // Both values were checked by `safe`: no quoting/metacharacters.
        assert!(safe(&r.unit) && safe(&r.path));
        script.push_str(&format!(
            "u={u}; p=$(systemctl show -p MainPID --value $u); s=$(systemctl show -p ActiveState --value $u); \
             e=$(sha256sum /proc/$p/exe 2>/dev/null | cut -d' ' -f1); f=$(sha256sum {f} | cut -d' ' -f1); \
             z=$(stat -L -c %s {f}); echo \"ROW $u $s $p $e $f $z\"\n",
            u = r.unit,
            f = r.path
        ));
    }
    let out = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=15",
            &host,
            "nice -n 10 bash -s",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .and_then(|mut c| {
            use std::io::Write;
            c.stdin
                .take()
                .expect("stdin")
                .write_all(script.as_bytes())?;
            c.wait_with_output()
        })
        .expect("ssh");
    assert!(out.status.success(), "ssh exit {:?}", out.status.code());
    let text = String::from_utf8_lossy(&out.stdout);
    let mut drift = Vec::new();
    let mut checked = 0;
    for r in &rows {
        let line = text
            .lines()
            .find(|l| l.split_whitespace().nth(1) == Some(r.unit.as_str()))
            .unwrap_or_else(|| panic!("no ROW for {}", r.unit));
        let f: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(f.len(), 7, "short ROW for {}", r.unit);
        let (state, pid, exe, file, size) = (f[2], f[3], f[4], f[5], f[6]);
        let ok = state == r.state
            && pid != "0"
            && exe == r.sha
            && file == r.sha
            && size.parse::<u64>().ok() == Some(r.size);
        println!(
            "{:<20} state={state:<8} pid={pid:<8} exe={} file={} size={size} {}",
            r.unit,
            exe.get(..16).unwrap_or(exe),
            file.get(..16).unwrap_or(file),
            if ok { "OK" } else { "DRIFT" }
        );
        if !ok {
            drift.push(r.unit.clone());
        }
        checked += 1;
    }
    println!("checked {checked} rows, drift {}", drift.len());
    assert!(drift.is_empty(), "drift: {drift:?}");
}
