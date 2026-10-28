// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Unit tests of the `deny` check (slot 428 publish-gates-main). Values are
//! TEST-NET documentation addresses and made-up names; real operator entries
//! live only in a file outside the repository.

use repo_check::*;
use std::path::PathBuf;

fn list(entries: &[&str]) -> Vec<String> {
    entries.iter().map(|s| s.to_string()).collect()
}

#[test]
fn parse_skips_comments_blanks_and_short_entries() {
    let d = parse_deny_list("# operator list\n\n  203.0.113.77  \nabc\nhost-a.example\n#x\n");
    assert_eq!(d, list(&["203.0.113.77", "host-a.example"]));
}

#[test]
fn text_hit_names_line_and_entry_never_value() {
    let deny = list(&["203.0.113.77", "box-a.example"]);
    let text = "ok\nssh root@203.0.113.77 -p 22\nnothing\nsee box-a.example and 203.0.113.77\n";
    let f = deny_hits("docs/x.md", text.as_bytes(), &deny);
    let got: Vec<_> = f.iter().map(|f| (f.line, f.msg.clone())).collect();
    assert_eq!(got.len(), 3, "{got:?}");
    assert_eq!(got[0].0, 2);
    assert_eq!(got[1].0, 4);
    assert!(got[0].1.contains("entry #1"));
    assert!(got[2].1.contains("entry #2"));
    for x in &f {
        assert_eq!(x.check, Check::Deny);
        assert_eq!(x.level, Level::Error);
        assert!(!x.msg.contains("203.0.113"), "value leaked: {}", x.msg);
        assert!(!x.msg.contains("box-a"), "value leaked: {}", x.msg);
    }
}

#[test]
fn one_finding_per_line_even_with_repeats() {
    let deny = list(&["203.0.113.77"]);
    let f = deny_hits("a.md", b"203.0.113.77 203.0.113.77\n", &deny);
    assert_eq!(f.len(), 1);
}

#[test]
fn ip_boundaries_do_not_match_longer_numbers() {
    let deny = list(&["192.0.2.7"]);
    for s in [
        "192.0.2.77",
        "1192.0.2.7",
        "192.0.2.7.5",
        "v192.0.2.7",
        "10.192.0.2.7",
    ] {
        assert!(
            deny_hits("a.md", s.as_bytes(), &deny).is_empty(),
            "{s} matched"
        );
    }
    for s in [
        "192.0.2.7",
        "(192.0.2.7)",
        "ip=192.0.2.7.",
        "http://192.0.2.7:80/",
        "`192.0.2.7`",
    ] {
        assert_eq!(
            deny_hits("a.md", s.as_bytes(), &deny).len(),
            1,
            "{s} missed"
        );
    }
}

#[test]
fn host_boundaries() {
    let deny = list(&["vps-a.example"]);
    assert_eq!(deny_hits("a", b"https://vps-a.example/x", &deny).len(), 1);
    assert_eq!(deny_hits("a", b"sub.vps-a.example", &deny).len(), 1);
    assert!(deny_hits("a", b"myvps-a.example", &deny).is_empty());
    assert!(deny_hits("a", b"vps-a.examples", &deny).is_empty());
}

#[test]
fn allow_marker_does_not_silence_deny() {
    let deny = list(&["203.0.113.77"]);
    let line = format!("x 203.0.113.77 // {}\n", ALLOW_MARKER);
    assert_eq!(
        deny_hits("crates/a/src/x.rs", line.as_bytes(), &deny).len(),
        1
    );
}

#[test]
fn binary_exact_match_without_boundaries_reports_line_zero() {
    let deny = list(&["203.0.113.77"]);
    let mut bin = vec![0x7f, b'E', b'L', b'F', 0, 1];
    bin.extend_from_slice(b"http://203.0.113.77abc/console");
    bin.extend_from_slice(&[0, 0xff, b'\n', b'\n']);
    bin.extend_from_slice(b"203.0.113.77");
    let f = deny_hits("dist/bin/tool", &bin, &deny);
    assert_eq!(f.len(), 1, "one finding per entry per binary");
    assert_eq!(f[0].line, 0);
    assert!(f[0].msg.contains("binary"));
    assert!(!f[0].msg.contains("203.0.113"));
}

#[test]
fn empty_list_is_a_no_op() {
    assert!(deny_hits("a", b"203.0.113.77", &[]).is_empty());
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("repo-check-deny-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("docs")).unwrap();
    std::fs::create_dir_all(d.join("dist")).unwrap();
    std::fs::create_dir_all(d.join("target")).unwrap();
    d
}

#[test]
fn scan_tree_with_finds_text_and_binary_and_matches_scan_tree_when_empty() {
    let d = tmp("tree");
    std::fs::write(d.join("docs/a.md"), "host 203.0.113.77\n").unwrap();
    std::fs::write(d.join("dist/b.bin"), b"\0\x01203.0.113.77\xff").unwrap();
    std::fs::write(d.join("target/skip.md"), "203.0.113.77\n").unwrap();
    let base = scan_tree(&d, None).unwrap();
    let same = scan_tree_with(&d, None, &[]).unwrap();
    assert_eq!(base.findings, same.findings);
    let r = scan_tree_with(&d, None, &list(&["203.0.113.77"])).unwrap();
    let mut got: Vec<_> = r
        .findings
        .iter()
        .filter(|f| f.check == Check::Deny)
        .map(|f| (f.file.as_str(), f.line))
        .collect();
    got.sort();
    assert_eq!(got, vec![("dist/b.bin", 0), ("docs/a.md", 1)]);
    let _ = std::fs::remove_dir_all(&d);
}
