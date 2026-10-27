// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! `nerve` check (Nerve T2.4 coverage gate).
//!
//! Every binary target of a workspace member must either reach the nerve
//! through `nerve-client` (a normal, non-optional dependency, directly or via
//! another workspace crate that has one) or be listed as probe-only in the
//! checked-in allowlist [`DEFAULT_ALLOWLIST`] with a reason. Anything else is
//! a coverage gap and an error.
//!
//! The scan is static (manifests + file layout, no `cargo metadata`), std only,
//! and never edits anything: it lists gaps, it does not wire services.

use crate::{Check, Finding, Level};
use std::collections::{BTreeMap, BTreeSet};

/// Repo-relative allowlist path used when the CLI does not override it.
pub const DEFAULT_ALLOWLIST: &str = "crates/repo-check/nerve-probe-only.txt";

/// The crate whose dependency counts as nerve coverage.
pub const NERVE_CLIENT: &str = "nerve-client";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BinDecl {
    pub name: Option<String>,
    pub path: Option<String>,
    /// 1-based line of the `[[bin]]` header.
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dep {
    /// Real package name (`package = "..."` rename resolved).
    pub package: String,
    pub optional: bool,
    pub line: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    pub package: Option<String>,
    pub autobins: bool,
    pub bins: Vec<BinDecl>,
    /// `[dependencies]` and `[target.*.dependencies]` only (no dev/build).
    pub deps: Vec<Dep>,
    /// `[workspace] members` / `exclude` (root manifest only).
    pub members: Vec<String>,
    pub exclude: Vec<String>,
}

fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    let mut prev = '\0';
    for (i, ch) in line.char_indices() {
        match ch {
            '"' if prev != '\\' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
        prev = ch;
    }
    line
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    let s = s
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .or_else(|| s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
        .unwrap_or(s);
    s.to_string()
}

/// `key = value` split; `None` for lines that are not a key/value pair.
fn kv(line: &str) -> Option<(String, &str)> {
    let (k, v) = line.split_once('=')?;
    let k = k.trim();
    let bare = !k.is_empty()
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    let quoted = k.len() >= 2 && k.starts_with('"') && k.ends_with('"');
    (bare || quoted).then(|| (unquote(k), v.trim()))
}

/// Value of `key = "..."` inside an inline table such as `{ path = "..", package = "x" }`.
fn inline_str(table: &str, key: &str) -> Option<String> {
    let body = table.trim().strip_prefix('{')?.strip_suffix('}')?;
    body.split(',').find_map(|part| {
        let (k, v) = kv(part)?;
        (k == key).then(|| unquote(v))
    })
}

fn inline_true(table: &str, key: &str) -> bool {
    table
        .trim()
        .strip_prefix('{')
        .and_then(|b| b.strip_suffix('}'))
        .is_some_and(|body| {
            body.split(',')
                .filter_map(kv)
                .any(|(k, v)| k == key && v == "true")
        })
}

/// Collect a (possibly multi-line) string array starting at `first`.
fn array_items(first: &str, rest: &mut dyn Iterator<Item = &str>) -> Vec<String> {
    let mut buf = first.to_string();
    while !buf.contains(']') {
        match rest.next() {
            Some(l) => {
                buf.push(' ');
                buf.push_str(strip_comment(l));
            }
            None => break,
        }
    }
    let inner = buf
        .trim()
        .trim_start_matches('[')
        .split(']')
        .next()
        .unwrap_or("");
    inner
        .split(',')
        .map(unquote)
        .filter(|s| !s.is_empty())
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Other,
    Package,
    Bin,
    Deps,
    /// `[dependencies.NAME]` table form: index into `deps`.
    DepTable(usize),
    Workspace,
}

/// Minimal Cargo.toml reader: just the keys the coverage gate needs.
pub fn parse_manifest(text: &str) -> Manifest {
    let mut m = Manifest {
        autobins: true,
        ..Manifest::default()
    };
    let mut sec = Section::Other;
    let mut lines = text.lines().enumerate();
    while let Some((i, raw)) = lines.next() {
        let n = i + 1;
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(h) = line.strip_prefix("[[").and_then(|x| x.strip_suffix("]]")) {
            sec = if h.trim() == "bin" {
                m.bins.push(BinDecl {
                    line: n,
                    ..BinDecl::default()
                });
                Section::Bin
            } else {
                Section::Other
            };
            continue;
        }
        if let Some(h) = line.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
            let h = h.trim();
            let parts: Vec<String> = split_header(h);
            let p: Vec<&str> = parts.iter().map(String::as_str).collect();
            sec = match p.as_slice() {
                ["package"] => Section::Package,
                ["workspace"] => Section::Workspace,
                ["dependencies"] | ["target", _, "dependencies"] => Section::Deps,
                ["dependencies", name] | ["target", _, "dependencies", name] => {
                    m.deps.push(Dep {
                        package: name.to_string(),
                        optional: false,
                        line: n,
                    });
                    Section::DepTable(m.deps.len() - 1)
                }
                _ => Section::Other,
            };
            continue;
        }
        let Some((k, v)) = kv(line) else { continue };
        match sec {
            Section::Package => match k.as_str() {
                "name" => m.package = Some(unquote(v)),
                "autobins" => m.autobins = v != "false",
                _ => {}
            },
            Section::Bin => {
                let b = m.bins.last_mut().expect("bin section has a decl");
                match k.as_str() {
                    "name" => b.name = Some(unquote(v)),
                    "path" => b.path = Some(unquote(v)),
                    _ => {}
                }
            }
            Section::Deps => {
                let package = inline_str(v, "package").unwrap_or(k);
                m.deps.push(Dep {
                    package,
                    optional: inline_true(v, "optional"),
                    line: n,
                });
            }
            Section::DepTable(idx) => match k.as_str() {
                "package" => m.deps[idx].package = unquote(v),
                "optional" => m.deps[idx].optional = v == "true",
                _ => {}
            },
            Section::Workspace => match k.as_str() {
                "members" => m.members = array_items(v, &mut lines.by_ref().map(|(_, l)| l)),
                "exclude" => m.exclude = array_items(v, &mut lines.by_ref().map(|(_, l)| l)),
                _ => {}
            },
            Section::Other => {}
        }
    }
    m
}

/// Split a table header on dots outside quotes: `target.'cfg(unix)'.dependencies`.
fn split_header(h: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in h.chars() {
        match (quote, ch) {
            (None, '"' | '\'') => quote = Some(ch),
            (Some(q), c) if c == q => quote = None,
            (None, '.') => out.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(ch),
        }
    }
    out.push(cur.trim().to_string());
    out
}

/// One binary target of one package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinTarget {
    pub package: String,
    pub bin: String,
    /// Repo-relative manifest path.
    pub manifest: String,
    /// `[[bin]]` line, 0 for auto-discovered bins.
    pub line: usize,
}

/// Bin targets of one manifest, following Cargo's rules: explicit `[[bin]]`
/// entries, plus (unless `autobins = false`) `src/main.rs` and
/// `src/bin/*.rs` / `src/bin/*/main.rs` not already claimed by name or path.
/// `files` are paths relative to the crate directory.
pub fn bin_targets(manifest_rel: &str, m: &Manifest, files: &BTreeSet<String>) -> Vec<BinTarget> {
    let Some(pkg) = m.package.clone() else {
        return Vec::new();
    };
    let mut out: Vec<BinTarget> = Vec::new();
    let mut claimed_paths = BTreeSet::new();
    for b in &m.bins {
        let name = match (&b.name, &b.path) {
            (Some(n), _) => n.clone(),
            (None, Some(p)) => stem(p),
            (None, None) => continue,
        };
        let path = b.path.clone().unwrap_or_else(|| {
            if name == pkg {
                "src/main.rs".to_string()
            } else if files.contains(&format!("src/bin/{name}/main.rs")) {
                format!("src/bin/{name}/main.rs")
            } else {
                format!("src/bin/{name}.rs")
            }
        });
        claimed_paths.insert(path.trim_start_matches("./").to_string());
        out.push(BinTarget {
            package: pkg.clone(),
            bin: name,
            manifest: manifest_rel.to_string(),
            line: b.line,
        });
    }
    if m.autobins {
        let mut auto: Vec<(String, String)> = Vec::new();
        if files.contains("src/main.rs") {
            auto.push((pkg.clone(), "src/main.rs".to_string()));
        }
        for f in files {
            let Some(rest) = f.strip_prefix("src/bin/") else {
                continue;
            };
            match rest.split('/').collect::<Vec<_>>().as_slice() {
                [file] if file.ends_with(".rs") => {
                    auto.push((file.trim_end_matches(".rs").to_string(), f.clone()))
                }
                [dir, "main.rs"] => auto.push((dir.to_string(), f.clone())),
                _ => {}
            }
        }
        for (name, path) in auto {
            if claimed_paths.contains(&path) || out.iter().any(|t| t.bin == name) {
                continue;
            }
            out.push(BinTarget {
                package: pkg.clone(),
                bin: name,
                manifest: manifest_rel.to_string(),
                line: 0,
            });
        }
    }
    out
}

fn stem(p: &str) -> String {
    std::path::Path::new(p)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(p)
        .to_string()
}

/// How a package reaches nerve-client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reach {
    Direct,
    /// Through this workspace crate (first hop).
    Via(String),
    None,
}

/// Fixpoint over workspace packages: who reaches nerve-client through normal,
/// non-optional dependencies.
pub fn reach(manifests: &BTreeMap<String, Manifest>) -> BTreeMap<String, Reach> {
    let mut r: BTreeMap<String, Reach> = BTreeMap::new();
    for (pkg, m) in manifests {
        let direct = m
            .deps
            .iter()
            .any(|d| !d.optional && d.package == NERVE_CLIENT);
        r.insert(
            pkg.clone(),
            if direct || pkg == NERVE_CLIENT {
                Reach::Direct
            } else {
                Reach::None
            },
        );
    }
    loop {
        let mut changed = false;
        for (pkg, m) in manifests {
            if r[pkg] != Reach::None {
                continue;
            }
            if let Some(d) = m
                .deps
                .iter()
                .find(|d| !d.optional && r.get(&d.package).is_some_and(|x| *x != Reach::None))
            {
                r.insert(pkg.clone(), Reach::Via(d.package.clone()));
                changed = true;
            }
        }
        if !changed {
            return r;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllowEntry {
    pub package: String,
    pub bin: String,
    pub reason: String,
    pub line: usize,
}

/// Parse the allowlist: `package:bin  reason...` per line, `#` comments.
/// Malformed lines, missing reasons and duplicates are error findings.
pub fn parse_allowlist(path: &str, text: &str) -> (Vec<AllowEntry>, Vec<Finding>) {
    let mut entries: Vec<AllowEntry> = Vec::new();
    let mut bad = Vec::new();
    let err = |n: usize, msg: String| Finding {
        check: Check::Nerve,
        level: Level::Error,
        file: path.to_string(),
        line: n,
        msg,
    };
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (id, reason) = match line.split_once(char::is_whitespace) {
            Some((a, b)) => (a, b.trim()),
            None => (line, ""),
        };
        let Some((package, bin)) = id
            .split_once(':')
            .filter(|(p, b)| !p.is_empty() && !b.is_empty() && !b.contains(':'))
        else {
            bad.push(err(
                n,
                format!("allowlist entry {id:?} is not `package:bin`"),
            ));
            continue;
        };
        if reason.is_empty() {
            bad.push(err(n, format!("allowlist entry {id} has no reason")));
            continue;
        }
        if entries.iter().any(|e| e.package == package && e.bin == bin) {
            bad.push(err(n, format!("allowlist entry {id} is listed twice")));
            continue;
        }
        entries.push(AllowEntry {
            package: package.to_string(),
            bin: bin.to_string(),
            reason: reason.to_string(),
            line: n,
        });
    }
    (entries, bad)
}

/// Per-bin verdict, for the gaps table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Covered(Reach),
    ProbeOnly(String),
    Gap,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinStatus {
    pub target: BinTarget,
    pub status: Status,
}

/// Does a workspace member pattern (`crates/*` or a literal dir) match `dir`?
fn member_match(pat: &str, dir: &str) -> bool {
    let pat = pat.trim_end_matches('/');
    match pat.strip_suffix("/*") {
        Some(base) => dir
            .strip_prefix(base)
            .and_then(|r| r.strip_prefix('/'))
            .is_some_and(|r| !r.is_empty() && !r.contains('/')),
        None => pat == dir,
    }
}

/// The coverage gate over a set of files.
///
/// * `root_manifest`: text of the root `Cargo.toml` (`None`: no workspace, no findings).
/// * `files`: every repo-relative file path (as [`crate::walk`] returns them).
/// * `read`: reads a repo-relative text file.
/// * `allowlist`: `(path, Some(text))`, or `(path, None)` when the file is missing.
pub fn check_coverage(
    root_manifest: Option<&str>,
    files: &[String],
    read: &dyn Fn(&str) -> Option<String>,
    allowlist: (&str, Option<&str>),
) -> (Vec<Finding>, Vec<BinStatus>) {
    let mut findings = Vec::new();
    let Some(root) = root_manifest else {
        return (findings, Vec::new());
    };
    let ws = parse_manifest(root);
    if ws.members.is_empty() {
        return (findings, Vec::new());
    }
    // Member dirs that hold a Cargo.toml.
    let dirs: BTreeSet<String> = files
        .iter()
        .filter_map(|f| f.strip_suffix("/Cargo.toml"))
        .filter(|d| ws.members.iter().any(|p| member_match(p, d)))
        .filter(|d| !ws.exclude.iter().any(|p| member_match(p, d)))
        .map(str::to_string)
        .collect();
    let mut manifests: BTreeMap<String, Manifest> = BTreeMap::new();
    let mut targets: Vec<BinTarget> = Vec::new();
    for dir in &dirs {
        let rel = format!("{dir}/Cargo.toml");
        let Some(text) = read(&rel) else { continue };
        let m = parse_manifest(&text);
        let Some(pkg) = m.package.clone() else {
            continue;
        };
        let prefix = format!("{dir}/");
        let local: BTreeSet<String> = files
            .iter()
            .filter_map(|f| f.strip_prefix(&prefix))
            .map(str::to_string)
            .collect();
        targets.extend(bin_targets(&rel, &m, &local));
        manifests.insert(pkg, m);
    }
    let reach = reach(&manifests);

    let (allow, mut bad) = match allowlist.1 {
        Some(t) => parse_allowlist(allowlist.0, t),
        None => {
            findings.push(Finding {
                check: Check::Nerve,
                level: Level::Warn,
                file: allowlist.0.to_string(),
                line: 0,
                msg: "probe-only allowlist missing; every uncovered bin is a gap".into(),
            });
            (Vec::new(), Vec::new())
        }
    };
    findings.append(&mut bad);

    let mut out = Vec::new();
    for t in targets {
        let r = reach.get(&t.package).cloned().unwrap_or(Reach::None);
        let listed = allow
            .iter()
            .find(|e| e.package == t.package && e.bin == t.bin);
        let status = match (&r, listed) {
            (Reach::None, Some(e)) => Status::ProbeOnly(e.reason.clone()),
            (Reach::None, None) => {
                findings.push(Finding {
                    check: Check::Nerve,
                    level: Level::Error,
                    file: t.manifest.clone(),
                    line: t.line,
                    msg: format!(
                        "bin {}:{} does not depend on nerve-client and is not listed probe-only in {}; wire nerve-client or add it there with a reason",
                        t.package, t.bin, allowlist.0
                    ),
                });
                Status::Gap
            }
            (covered, Some(e)) => {
                findings.push(Finding {
                    check: Check::Nerve,
                    level: Level::Warn,
                    file: allowlist.0.to_string(),
                    line: e.line,
                    msg: format!(
                        "{}:{} reaches nerve-client; drop the redundant probe-only entry",
                        t.package, t.bin
                    ),
                });
                Status::Covered(covered.clone())
            }
            (covered, None) => Status::Covered(covered.clone()),
        };
        out.push(BinStatus { target: t, status });
    }
    for e in &allow {
        if !out
            .iter()
            .any(|s| s.target.package == e.package && s.target.bin == e.bin)
        {
            findings.push(Finding {
                check: Check::Nerve,
                level: Level::Warn,
                file: allowlist.0.to_string(),
                line: e.line,
                msg: format!(
                    "stale probe-only entry {}:{}: no such workspace bin",
                    e.package, e.bin
                ),
            });
        }
    }
    out.sort_by(|a, b| (&a.target.package, &a.target.bin).cmp(&(&b.target.package, &b.target.bin)));
    (findings, out)
}

/// One line per bin: `GAP`, `covered`, `covered via X`, `probe-only (reason)`.
pub fn render_table(rows: &[BinStatus]) -> String {
    let mut s = String::new();
    for r in rows {
        let st = match &r.status {
            Status::Gap => "GAP".to_string(),
            Status::Covered(Reach::Direct) => "covered (direct nerve-client)".to_string(),
            Status::Covered(Reach::Via(v)) => format!("covered (via {v})"),
            Status::Covered(Reach::None) => "covered".to_string(),
            Status::ProbeOnly(why) => format!("probe-only: {why}"),
        };
        s.push_str(&format!(
            "{:<22} {:<22} {st}\n",
            r.target.package, r.target.bin
        ));
    }
    let gaps = rows.iter().filter(|r| r.status == Status::Gap).count();
    let probe = rows
        .iter()
        .filter(|r| matches!(r.status, Status::ProbeOnly(_)))
        .count();
    s.push_str(&format!(
        "nerve coverage: {} bins, {} covered, {probe} probe-only, {gaps} gap(s)\n",
        rows.len(),
        rows.len() - gaps - probe
    ));
    s
}

// ------------------------------------------------------------ wiring depth

/// One wiring rule a directly covered package must meet (slot 327).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Wire {
    /// `nerve_client::event(.., "started", ..)` somewhere in non-test src.
    Started,
    /// Serves HTTP (non-test `TcpListener::bind` / `Server::http`) but never
    /// calls `nerve_client::observe_request`, so 5xx and slow requests are
    /// invisible to the nerve.
    ObserveRequest,
    /// Neither `start_heartbeat` (long-lived) nor `nerve_client::flush`
    /// (one-shot): a one-shot CLI exits before the push worker drains the
    /// queue and its signals are silently lost.
    HeartbeatOrFlush,
}

impl Wire {
    pub const ALL: [Wire; 3] = [Wire::Started, Wire::ObserveRequest, Wire::HeartbeatOrFlush];

    pub fn name(self) -> &'static str {
        match self {
            Wire::Started => "started",
            Wire::ObserveRequest => "observe_request",
            Wire::HeartbeatOrFlush => "heartbeat_or_flush",
        }
    }
}

/// Wiring verdict for one package that depends on nerve-client directly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WiringRow {
    pub package: String,
    pub manifest: String,
    pub serves_http: bool,
    /// Rules not met (after exemptions).
    pub missing: Vec<Wire>,
    /// Rules exempted in source with `// nerve-wiring: exempt <rule> <reason>`.
    pub exempt: Vec<(Wire, String)>,
}

/// The part of a source file before its first `#[cfg(test)]` line (test
/// modules may bind listeners and call anything; they do not count).
pub fn non_test(text: &str) -> &str {
    let mut off = 0;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("#[cfg(test)]") {
            return &text[..off];
        }
        off += line.len();
    }
    text
}

/// Facts the wiring rules look at, over a crate's concatenated non-test src.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WireFacts {
    pub started: bool,
    pub serves_http: bool,
    pub observe: bool,
    pub heartbeat: bool,
    pub flush: bool,
    pub exempt: Vec<(Wire, String)>,
}

pub fn wire_facts(src: &str) -> WireFacts {
    let mut f = WireFacts::default();
    let mut rest = src;
    while let Some(i) = rest.find("nerve_client::event(") {
        let tail = &rest[i..];
        let end = tail.find(';').unwrap_or(tail.len()).min(400);
        if tail[..end].contains("\"started\"") {
            f.started = true;
            break;
        }
        rest = &tail["nerve_client::event(".len()..];
    }
    f.serves_http = src.contains("TcpListener::bind(") || src.contains("Server::http(");
    f.observe = src.contains("observe_request(");
    f.heartbeat = src.contains("start_heartbeat(");
    f.flush = src.contains("nerve_client::flush(");
    for line in src.lines() {
        let Some(m) = line.split("// nerve-wiring: exempt ").nth(1) else {
            continue;
        };
        let (rule, why) = m
            .trim()
            .split_once(char::is_whitespace)
            .unwrap_or((m.trim(), ""));
        if let Some(w) = Wire::ALL.into_iter().find(|w| w.name() == rule) {
            if !why.trim().is_empty() && !f.exempt.iter().any(|e| e.0 == w) {
                f.exempt.push((w, why.trim().to_string()));
            }
        }
    }
    f
}

/// Rules a package with these facts misses (before exemptions).
pub fn wire_missing(f: &WireFacts) -> Vec<Wire> {
    let mut m = Vec::new();
    if !f.started {
        m.push(Wire::Started);
    }
    if f.serves_http && !f.observe {
        m.push(Wire::ObserveRequest);
    }
    if !f.heartbeat && !f.flush {
        m.push(Wire::HeartbeatOrFlush);
    }
    m
}

/// Wiring-depth check (slot 327): every workspace package that has at least
/// one bin and depends on nerve-client DIRECTLY must push `started`, call
/// `observe_request` when it serves HTTP, and either heartbeat or flush.
/// Static text scan of `<crate>/src/**/*.rs` (non-test part). Misses are
/// warnings (the dependency gate above stays the hard error).
pub fn check_wiring(
    root_manifest: Option<&str>,
    files: &[String],
    read: &dyn Fn(&str) -> Option<String>,
) -> (Vec<Finding>, Vec<WiringRow>) {
    let mut findings = Vec::new();
    let mut rows = Vec::new();
    let Some(root) = root_manifest else {
        return (findings, rows);
    };
    let ws = parse_manifest(root);
    let dirs: BTreeSet<String> = files
        .iter()
        .filter_map(|f| f.strip_suffix("/Cargo.toml"))
        .filter(|d| ws.members.iter().any(|p| member_match(p, d)))
        .filter(|d| !ws.exclude.iter().any(|p| member_match(p, d)))
        .map(str::to_string)
        .collect();
    for dir in &dirs {
        let rel = format!("{dir}/Cargo.toml");
        let Some(text) = read(&rel) else { continue };
        let m = parse_manifest(&text);
        let Some(pkg) = m.package.clone() else {
            continue;
        };
        if pkg == NERVE_CLIENT
            || !m
                .deps
                .iter()
                .any(|d| !d.optional && d.package == NERVE_CLIENT)
        {
            continue;
        }
        let prefix = format!("{dir}/");
        let local: BTreeSet<String> = files
            .iter()
            .filter_map(|f| f.strip_prefix(&prefix))
            .map(str::to_string)
            .collect();
        if bin_targets(&rel, &m, &local).is_empty() {
            continue;
        }
        let src_prefix = format!("{dir}/src/");
        let mut src = String::new();
        for f in files
            .iter()
            .filter(|f| f.starts_with(&src_prefix) && f.ends_with(".rs"))
        {
            if let Some(t) = read(f) {
                src.push_str(non_test(&t));
                src.push('\n');
            }
        }
        let facts = wire_facts(&src);
        let missing: Vec<Wire> = wire_missing(&facts)
            .into_iter()
            .filter(|w| !facts.exempt.iter().any(|e| e.0 == *w))
            .collect();
        for w in &missing {
            findings.push(Finding {
                check: Check::Nerve,
                level: Level::Warn,
                file: rel.clone(),
                line: 0,
                msg: format!(
                    "nerve wiring gap {pkg}: {} (wire it, or mark `// nerve-wiring: exempt {} <reason>` in src)",
                    w.name(),
                    w.name()
                ),
            });
        }
        rows.push(WiringRow {
            package: pkg,
            manifest: rel,
            serves_http: facts.serves_http,
            missing,
            exempt: facts.exempt,
        });
    }
    rows.sort_by(|a, b| a.package.cmp(&b.package));
    (findings, rows)
}

/// One line per directly wired package plus a total line.
pub fn render_wiring(rows: &[WiringRow]) -> String {
    let mut s = String::new();
    for r in rows {
        let mut st = if r.missing.is_empty() {
            "wired".to_string()
        } else {
            format!(
                "GAP {}",
                r.missing
                    .iter()
                    .map(|w| w.name())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        for (w, why) in &r.exempt {
            st.push_str(&format!(" [exempt {}: {why}]", w.name()));
        }
        s.push_str(&format!(
            "{:<22} {:<5} {st}\n",
            r.package,
            if r.serves_http { "http" } else { "-" }
        ));
    }
    let gaps: usize = rows.iter().map(|r| r.missing.len()).sum();
    let pk = rows.iter().filter(|r| !r.missing.is_empty()).count();
    s.push_str(&format!(
        "nerve wiring: {} direct packages, {pk} with gaps, {gaps} wiring gap(s)\n",
        rows.len()
    ));
    s
}
