// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! galion-pyarchive: dry-run planner for Phase A of docs/py-archive-plan.md.
//! Reads only. Never moves, deletes or chmods anything on the scanned tree.

use galion_pyarchive::{
    list, manifest, parse_manifest, proposed_commands, real, scan_refs, sha256_file, Class,
    Listing, Scan, ARCHIVE_DIR, EARLIEST, PHASE_A,
};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage:
  galion-pyarchive plan   [--root DIR] [--out NEWDIR] [--json] [--no-procs]
  galion-pyarchive verify --manifest FILE [--root DIR] [--json] [--no-procs]
plan    list the Phase A set, hash it, prove 0 live systemd unit/timer/cron/process
        references, print the tar + SHA-256 manifest plan. --out writes
        phase-a.{plan.txt,sha256,files,json} into a NEW directory (never overwrites).
verify  re-hash every manifest entry and re-run the reference scan.
Read-only: nothing on the scanned tree is moved, deleted or changed.
exit: 0 proof holds, 1 proof fails, 2 usage/io error";

struct Opts {
    cmd: String,
    root: PathBuf,
    out: Option<PathBuf>,
    manifest: Option<PathBuf>,
    json: bool,
    procs: bool,
}

fn parse(args: Vec<String>) -> Result<Opts, String> {
    let mut it = args.into_iter();
    let cmd = it.next().ok_or("missing command")?;
    if cmd != "plan" && cmd != "verify" {
        return Err(format!("unknown command {cmd:?}"));
    }
    let mut o = Opts {
        cmd,
        root: PathBuf::from("/"),
        out: None,
        manifest: None,
        json: false,
        procs: true,
    };
    while let Some(a) = it.next() {
        let mut val = |n: &str| it.next().ok_or_else(|| format!("{n} needs a value"));
        match a.as_str() {
            "--root" => o.root = PathBuf::from(val("--root")?),
            "--out" => o.out = Some(PathBuf::from(val("--out")?)),
            "--manifest" => o.manifest = Some(PathBuf::from(val("--manifest")?)),
            "--json" => o.json = true,
            "--no-procs" => o.procs = false,
            _ => return Err(format!("unknown flag {a:?}")),
        }
    }
    if o.cmd == "verify" && o.manifest.is_none() {
        return Err("verify needs --manifest FILE".into());
    }
    if o.cmd == "verify" && o.out.is_some() {
        return Err("verify takes no --out".into());
    }
    Ok(o)
}

fn host(root: &Path) -> String {
    fs::read_to_string(real(root, "/etc/hostname"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

fn verdict(l: &Listing, s: &Scan) -> (bool, Vec<String>) {
    let mut why = Vec::new();
    if l.entries.is_empty() {
        why.push("no files found".to_string());
    }
    if !l.missing.is_empty() {
        why.push(format!("{} path(s) missing", l.missing.len()));
    }
    if !l.refused.is_empty() {
        why.push(format!("{} non-regular path(s)", l.refused.len()));
    }
    let a = s.count(Class::Active);
    if a > 0 {
        why.push(format!("{a} ACTIVE reference(s)"));
    }
    if !s.unreadable.is_empty() {
        why.push(format!(
            "{} unreadable file(s): proof incomplete",
            s.unreadable.len()
        ));
    }
    (why.is_empty(), why)
}

fn report_json(cmd: &str, root: &Path, l: &Listing, s: &Scan, extra: serde_json::Value) -> String {
    let (ok, why) = verdict(l, s);
    let v = json!({
        "tool": "galion-pyarchive",
        "command": cmd,
        "generated": zagreb_time::now_rfc3339(),
        "host": host(root),
        "root": root.display().to_string(),
        "phase": "A",
        "earliest": EARLIEST,
        "archive_dir": ARCHIVE_DIR,
        "verdict": if ok { "PASS" } else { "FAIL" },
        "why": why,
        "files": l.entries.iter().map(|e| json!({
            "path": e.path, "size": e.size, "mode": format!("{:04o}", e.mode),
            "mtime": zagreb_time::from_unix(e.mtime), "sha256": e.sha256,
        })).collect::<Vec<_>>(),
        "total_bytes": l.entries.iter().map(|e| e.size).sum::<u64>(),
        "missing": l.missing,
        "refused": l.refused,
        "scanned": {"units": s.units_scanned, "cron_files": s.cron_files_scanned, "processes": s.procs_scanned},
        "counts": {
            "active": s.count(Class::Active), "overridden": s.count(Class::Overridden),
            "shadowed": s.count(Class::Shadowed), "masked": s.count(Class::Masked),
            "comment": s.count(Class::Comment),
        },
        "refs": s.refs.iter().map(|r| json!({
            "class": r.class.as_str(), "kind": r.kind, "source": r.source,
            "line": r.line, "target": r.target, "text": r.text,
        })).collect::<Vec<_>>(),
        "unreadable": s.unreadable,
        "extra": extra,
    });
    serde_json::to_string_pretty(&v).unwrap_or_default() + "\n"
}

fn report_text(cmd: &str, root: &Path, l: &Listing, s: &Scan, extra: &str) -> String {
    let (ok, why) = verdict(l, s);
    let mut o = String::new();
    o.push_str(&format!(
        "galion-pyarchive {cmd} (dry run, read-only)\ngenerated {}  host {}  root {}\n",
        zagreb_time::now_rfc3339(),
        host(root),
        root.display()
    ));
    o.push_str(&format!(
        "phase A  earliest {EARLIEST}  archive dir {ARCHIVE_DIR}\n\n"
    ));
    o.push_str(&format!("files ({}):\n", l.entries.len()));
    for e in &l.entries {
        o.push_str(&format!(
            "  {:>9} B  {:04o}  {}  {}  sha256 {}\n",
            e.size,
            e.mode,
            zagreb_time::label(e.mtime),
            e.path,
            e.sha256
        ));
    }
    o.push_str(&format!(
        "  total {} B\n",
        l.entries.iter().map(|e| e.size).sum::<u64>()
    ));
    for m in &l.missing {
        o.push_str(&format!("  MISSING {m}\n"));
    }
    for m in &l.refused {
        o.push_str(&format!("  REFUSED (not a regular file) {m}\n"));
    }
    o.push_str(&format!(
        "\nscanned: {} units (system+user, timers included), {} cron files, {} processes\n",
        s.units_scanned, s.cron_files_scanned, s.procs_scanned
    ));
    o.push_str(&format!(
        "references: active {}  overridden {}  shadowed {}  masked {}  comment {}\n",
        s.count(Class::Active),
        s.count(Class::Overridden),
        s.count(Class::Shadowed),
        s.count(Class::Masked),
        s.count(Class::Comment)
    ));
    for r in &s.refs {
        o.push_str(&format!(
            "  [{}] {} {}:{} -> {}  | {}\n",
            r.class.as_str(),
            r.kind,
            r.source,
            r.line,
            r.target,
            r.text
        ));
    }
    for u in &s.unreadable {
        o.push_str(&format!("  UNREADABLE {u}\n"));
    }
    if s.count(Class::Overridden) > 0 {
        o.push_str(
            "\nWARNING rollback dependency: an overridden unit line still names a Phase A file.\n\
             Removing the overriding drop-in (the documented rollback) would start the old path again.\n\
             After archiving, rollback must restore from the tarball first.\n",
        );
    }
    o.push_str(extra);
    o.push_str(&format!(
        "\nverdict: {}{}\n",
        if ok { "PASS" } else { "FAIL" },
        if why.is_empty() {
            String::new()
        } else {
            format!(" ({})", why.join("; "))
        }
    ));
    o
}

fn targets_of(l: &Listing) -> Vec<String> {
    let mut t: Vec<String> = l.entries.iter().map(|e| e.path.clone()).collect();
    for p in PHASE_A {
        if !t.iter().any(|x| x == p.path) {
            t.push(p.path.to_string());
        }
    }
    t
}

fn plan(o: &Opts) -> Result<bool, String> {
    let paths: Vec<&str> = PHASE_A.iter().map(|i| i.path).collect();
    let l = list(&o.root, &paths).map_err(|e| format!("listing: {e}"))?;
    let s = scan_refs(&o.root, &targets_of(&l), o.procs);
    let man = manifest(&l.entries);
    let files: String = l.entries.iter().map(|e| format!("{}\n", e.path)).collect();
    let cmds = proposed_commands(&l.entries);
    let extra = format!("\nmanifest (sha256sum -c format):\n{man}\nproposed commands:\n{cmds}");
    let text = report_text("plan", &o.root, &l, &s, &extra);
    let js = report_json(
        "plan",
        &o.root,
        &l,
        &s,
        json!({"manifest": man, "proposed_commands": cmds}),
    );
    if let Some(dir) = &o.out {
        let d = dir.to_string_lossy();
        if d.starts_with("/opt/galion") || d.starts_with("/etc") {
            return Err(format!("refusing --out under a live tree: {d}"));
        }
        // create_dir fails if it exists: never overwrite an earlier plan.
        fs::create_dir(dir).map_err(|e| format!("--out {d}: {e} (must be a new directory)"))?;
        for (n, body) in [
            ("phase-a.plan.txt", &text),
            ("phase-a.sha256", &man),
            ("phase-a.files", &files),
            ("phase-a.json", &js),
        ] {
            fs::write(dir.join(n), body).map_err(|e| format!("write {n}: {e}"))?;
        }
    }
    print!("{}", if o.json { &js } else { &text });
    Ok(verdict(&l, &s).0)
}

fn verify(o: &Opts) -> Result<bool, String> {
    let mp = o.manifest.as_ref().ok_or("verify needs --manifest")?;
    let text = fs::read_to_string(mp).map_err(|e| format!("{}: {e}", mp.display()))?;
    let want = parse_manifest(&text)?;
    let paths: Vec<&str> = want.iter().map(|(_, p)| p.as_str()).collect();
    let l = list(&o.root, &paths).map_err(|e| format!("listing: {e}"))?;
    let mut mismatch = Vec::new();
    for (h, p) in &want {
        match l.entries.iter().find(|e| &e.path == p) {
            Some(e) if &e.sha256 == h => {}
            Some(_) => mismatch.push(format!("{p}: sha256 changed")),
            None => {}
        }
    }
    // A manifest path must still be a file, not grown into a dir.
    for e in &l.entries {
        if !want.iter().any(|(_, p)| p == &e.path) {
            mismatch.push(format!("{}: unexpected extra file", e.path));
        }
    }
    let targets: Vec<String> = want.iter().map(|(_, p)| p.clone()).collect();
    let s = scan_refs(&o.root, &targets, o.procs);
    let mut extra = format!("\nmanifest {}: {} entries\n", mp.display(), want.len());
    for m in &mismatch {
        extra.push_str(&format!("  MISMATCH {m}\n"));
    }
    if mismatch.is_empty() {
        extra.push_str("  all sha256 match\n");
    }
    // sanity: re-hash one file directly (independent of list()).
    if let Some((h, p)) = want.first() {
        if let Ok(x) = sha256_file(&real(&o.root, p)) {
            extra.push_str(&format!(
                "  spot-check {p}: {}\n",
                if &x == h { "ok" } else { "DIFFERS" }
            ));
        }
    }
    let ok = verdict(&l, &s).0 && mismatch.is_empty();
    if o.json {
        print!(
            "{}",
            report_json("verify", &o.root, &l, &s, json!({"mismatch": mismatch}))
        );
    } else {
        let mut t = report_text("verify", &o.root, &l, &s, &extra);
        if !mismatch.is_empty() {
            t.push_str(&format!("verify: FAIL ({} mismatch)\n", mismatch.len()));
        }
        print!("{t}");
    }
    Ok(ok)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return if args.is_empty() {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        };
    }
    let o = match parse(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("galion-pyarchive: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let r = if o.cmd == "plan" {
        plan(&o)
    } else {
        verify(&o)
    };
    match r {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("galion-pyarchive: {e}");
            ExitCode::from(2)
        }
    }
}
