// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Unit tests of the `nerve` coverage gate (Nerve T2.4), all in memory.

use repo_check::nerve::*;
use repo_check::{Check, Level};
use std::collections::{BTreeMap, BTreeSet};

const ROOT: &str =
    "[workspace]\nresolver = \"2\"\nmembers = [\"crates/*\"]\nexclude = [\"crates/skip\"]\n";
const AL: &str = "crates/repo-check/nerve-probe-only.txt";

fn pkg(name: &str, extra: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n{extra}")
}

struct Tree(BTreeMap<String, String>);

impl Tree {
    fn new() -> Self {
        let mut m = BTreeMap::new();
        m.insert("Cargo.toml".to_string(), ROOT.to_string());
        Tree(m)
    }
    fn add(mut self, path: &str, text: &str) -> Self {
        self.0.insert(path.to_string(), text.to_string());
        self
    }
    fn run(&self, allow: Option<&str>) -> (Vec<repo_check::Finding>, Vec<BinStatus>) {
        let files: Vec<String> = self.0.keys().cloned().collect();
        let read = |p: &str| self.0.get(p).cloned();
        check_coverage(
            self.0.get("Cargo.toml").map(String::as_str),
            &files,
            &read,
            (AL, allow),
        )
    }
}

fn status(rows: &[BinStatus], id: &str) -> Status {
    rows.iter()
        .find(|r| format!("{}:{}", r.target.package, r.target.bin) == id)
        .unwrap_or_else(|| panic!("no bin {id} in {rows:?}"))
        .status
        .clone()
}

#[test]
fn parse_manifest_keys() {
    let m = parse_manifest(
        "[package]\nname = \"svc\" # c\nautobins = false\n\n[[bin]]\nname = \"svc\"\npath = \"src/main.rs\"\n\n[[bin]]\nname = \"tool\"\n\n[dependencies]\nnerve-client = { path = \"../nerve-client\" }\nrenamed = { package = \"other\", path = \"../o\" }\nopt = { path = \"../opt\", optional = true }\nserde = \"1\"\n\n[dependencies.tabled]\npath = \"../t\"\noptional = true\n\n[target.'cfg(unix)'.dependencies]\nunixdep = \"1\"\n\n[dev-dependencies]\ndevonly = \"1\"\n\n[build-dependencies]\nbuildonly = \"1\"\n",
    );
    assert_eq!(m.package.as_deref(), Some("svc"));
    assert!(!m.autobins);
    assert_eq!(m.bins.len(), 2);
    assert_eq!(m.bins[0].line, 5);
    assert_eq!(m.bins[1].name.as_deref(), Some("tool"));
    let deps: Vec<(&str, bool)> = m
        .deps
        .iter()
        .map(|d| (d.package.as_str(), d.optional))
        .collect();
    assert_eq!(
        deps,
        vec![
            ("nerve-client", false),
            ("other", false),
            ("opt", true),
            ("serde", false),
            ("tabled", true),
            ("unixdep", false),
        ]
    );
}

#[test]
fn parse_workspace_members_multiline() {
    let m = parse_manifest("[workspace]\nmembers = [\n  \"crates/*\", # all\n  \"tools/x\",\n]\nexclude = [\"crates/old\"]\n");
    assert_eq!(m.members, vec!["crates/*", "tools/x"]);
    assert_eq!(m.exclude, vec!["crates/old"]);
}

#[test]
fn bin_discovery_follows_cargo_rules() {
    let files: BTreeSet<String> = [
        "src/main.rs",
        "src/bin/a.rs",
        "src/bin/b/main.rs",
        "src/bin/b/util.rs",
        "src/lib.rs",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let names = |m: &Manifest| -> Vec<String> {
        let mut v: Vec<String> = bin_targets("crates/p/Cargo.toml", m, &files)
            .into_iter()
            .map(|t| t.bin)
            .collect();
        v.sort();
        v
    };
    assert_eq!(names(&parse_manifest(&pkg("p", ""))), vec!["a", "b", "p"]);
    // An explicit [[bin]] for the main path renames it; autobins still adds the rest.
    assert_eq!(
        names(&parse_manifest(&pkg(
            "p",
            "[[bin]]\nname = \"pd\"\npath = \"src/main.rs\"\n"
        ))),
        vec!["a", "b", "pd"]
    );
    // autobins = false keeps only the explicit ones.
    assert_eq!(
        names(&parse_manifest(&pkg(
            "p",
            "autobins = false\n[[bin]]\nname = \"a\"\n"
        ))),
        vec!["a"]
    );
    // Library-only crate: no bins.
    let lib: BTreeSet<String> = ["src/lib.rs".to_string()].into();
    assert!(bin_targets("x", &parse_manifest(&pkg("p", "")), &lib).is_empty());
}

#[test]
fn direct_transitive_optional_and_dev_deps() {
    let t = Tree::new()
        .add("crates/nerve-client/Cargo.toml", &pkg("nerve-client", ""))
        .add("crates/nerve-client/src/lib.rs", "")
        .add(
            "crates/lib1/Cargo.toml",
            &pkg(
                "lib1",
                "[dependencies]\nnerve-client = { path = \"../nerve-client\" }\n",
            ),
        )
        .add("crates/lib1/src/lib.rs", "")
        .add(
            "crates/direct/Cargo.toml",
            &pkg(
                "direct",
                "[dependencies]\nnerve-client = { path = \"../nerve-client\" }\n",
            ),
        )
        .add("crates/direct/src/main.rs", "")
        .add(
            "crates/via/Cargo.toml",
            &pkg("via", "[dependencies]\nlib1 = { path = \"../lib1\" }\n"),
        )
        .add("crates/via/src/main.rs", "")
        .add(
            "crates/opt/Cargo.toml",
            &pkg(
                "opt",
                "[dependencies]\nnerve-client = { path = \"../nerve-client\", optional = true }\n",
            ),
        )
        .add("crates/opt/src/main.rs", "")
        .add(
            "crates/dev/Cargo.toml",
            &pkg(
                "dev",
                "[dev-dependencies]\nnerve-client = { path = \"../nerve-client\" }\n",
            ),
        )
        .add("crates/dev/src/main.rs", "")
        .add("crates/skip/Cargo.toml", &pkg("skip", ""))
        .add("crates/skip/src/main.rs", "");
    let (f, rows) = t.run(Some(""));
    assert_eq!(
        status(&rows, "direct:direct"),
        Status::Covered(Reach::Direct)
    );
    assert_eq!(
        status(&rows, "via:via"),
        Status::Covered(Reach::Via("lib1".into()))
    );
    assert_eq!(status(&rows, "opt:opt"), Status::Gap);
    assert_eq!(status(&rows, "dev:dev"), Status::Gap);
    assert!(
        rows.iter().all(|r| r.target.package != "skip"),
        "excluded member scanned"
    );
    let errs: Vec<&str> = f
        .iter()
        .filter(|x| x.level == Level::Error)
        .map(|x| x.file.as_str())
        .collect();
    assert_eq!(errs, vec!["crates/dev/Cargo.toml", "crates/opt/Cargo.toml"]);
    assert!(f.iter().all(|x| x.check == Check::Nerve));
}

#[test]
fn allowlist_probe_only_stale_redundant_and_malformed() {
    let t = Tree::new()
        .add("crates/nerve-client/Cargo.toml", &pkg("nerve-client", ""))
        .add(
            "crates/svc/Cargo.toml",
            &pkg("svc", "[dependencies]\nnerve-client = \"*\"\n"),
        )
        .add("crates/svc/src/main.rs", "")
        .add("crates/tool/Cargo.toml", &pkg("tool", ""))
        .add("crates/tool/src/main.rs", "")
        .add("crates/tool/src/bin/mock.rs", "");
    let allow = "# header\ntool:tool   a linter\nsvc:svc  already wired\ngone:gone  removed crate\nnocolon reason\ntool:mock\ntool:tool  dup\n";
    let (f, rows) = t.run(Some(allow));
    assert_eq!(
        status(&rows, "tool:tool"),
        Status::ProbeOnly("a linter".into())
    );
    assert_eq!(status(&rows, "svc:svc"), Status::Covered(Reach::Direct));
    // tool:mock's entry has no reason, so it is malformed AND still a gap.
    assert_eq!(status(&rows, "tool:mock"), Status::Gap);
    let mut got: Vec<(Level, usize, &str)> = f
        .iter()
        .map(|x| {
            (
                x.level,
                x.line,
                if x.file == AL {
                    "allow"
                } else {
                    x.file.as_str()
                },
            )
        })
        .collect();
    got.sort();
    assert_eq!(
        got,
        vec![
            (Level::Warn, 3, "allow"),                   // redundant svc:svc
            (Level::Warn, 4, "allow"),                   // stale gone:gone
            (Level::Error, 0, "crates/tool/Cargo.toml"), // gap tool:mock
            (Level::Error, 5, "allow"),                  // not package:bin
            (Level::Error, 6, "allow"),                  // no reason
            (Level::Error, 7, "allow"),                  // duplicate
        ]
    );
}

#[test]
fn missing_allowlist_and_no_workspace() {
    let t = Tree::new()
        .add("crates/tool/Cargo.toml", &pkg("tool", ""))
        .add("crates/tool/src/main.rs", "");
    let (f, rows) = t.run(None);
    assert_eq!(rows.len(), 1);
    assert_eq!(f.len(), 2);
    assert!(f.iter().any(|x| x.level == Level::Warn && x.file == AL));
    // A tree without a root manifest (or without [workspace]) yields nothing.
    let files = vec!["crates/tool/Cargo.toml".to_string()];
    assert_eq!(
        check_coverage(None, &files, &|_| None, (AL, None)).0.len(),
        0
    );
    let lone = pkg("x", "");
    assert_eq!(
        check_coverage(Some(&lone), &files, &|_| None, (AL, None))
            .0
            .len(),
        0
    );
}

#[test]
fn table_counts() {
    let t = Tree::new()
        .add("crates/nerve-client/Cargo.toml", &pkg("nerve-client", ""))
        .add(
            "crates/a/Cargo.toml",
            &pkg("a", "[dependencies]\nnerve-client = \"*\"\n"),
        )
        .add("crates/a/src/main.rs", "")
        .add("crates/b/Cargo.toml", &pkg("b", ""))
        .add("crates/b/src/main.rs", "")
        .add("crates/c/Cargo.toml", &pkg("c", ""))
        .add("crates/c/src/main.rs", "");
    let (_, rows) = t.run(Some("b:b bench\n"));
    let s = render_table(&rows);
    assert!(
        s.contains("a                      a                      covered (direct nerve-client)"),
        "{s}"
    );
    assert!(s.contains("probe-only: bench"));
    assert!(
        s.ends_with("nerve coverage: 3 bins, 1 covered, 1 probe-only, 1 gap(s)\n"),
        "{s}"
    );
}

#[test]
fn checked_in_allowlist_is_well_formed() {
    let text = include_str!("../nerve-probe-only.txt");
    let (entries, bad) = parse_allowlist(AL, text);
    assert!(bad.is_empty(), "{bad:?}");
    assert!(entries.len() >= 10);
    assert!(
        entries.iter().all(|e| e.reason.len() >= 10),
        "reasons must say why"
    );
    assert!(text
        .lines()
        .take(3)
        .any(|l| l.contains("SPDX-License-Identifier: MIT")));
}
