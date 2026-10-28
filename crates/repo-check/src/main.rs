// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! repo-check CLI. Read-only: scans the tree, prints findings, exits 1 when
//! any finding is at error level, 2 on usage or I/O errors.

use repo_check::{json_escape, scan_tree, Check, Level};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str =
    "usage: repo-check [--root DIR] [--json] [--only a,b] [--warn a,b] [--error a,b]
                  [--copyright TEXT] [--list] [--stat-list FILE|-]
                  [--chmod-plan [--plan-tree DIR]... [--plan-reasons R] [--rollback] [--sh]] [--plan-verify PLAN]
  --root DIR        tree to scan (default .)
  --json            JSON report on stdout (Europe/Zagreb timestamp)
  --only a,b        run only these checks
  --warn a,b        downgrade these checks to warnings
  --error a,b       upgrade these checks to errors
  --copyright TEXT  the LICENSE copyright line must contain TEXT
  --list            list the checks and exit
  --stat-list F     lint a metadata list instead of the tree: lines `MODE TYPE PATH`
                    from `find ROOT -printf '%m %y %p\\n'` (F or - for stdin); perms check only
                    (`%m %y %u %g %p` adds owners, used by the plan)
  --chmod-plan      with --stat-list: print a STAGED chmod plan (TSV) for the perms
                    findings; nothing is changed
  --plan-tree DIR   also normalize group/other-writable entries under DIR to
                    755 dirs / 755 executables / 644 files (repeatable)
  --plan-reasons R  keep only rows with these reasons (dir-o+w-no-sticky,
                    secret-o+w, tree-normalize)
  --rollback        print the reverse plan (old and new swapped)
  --sh              print guarded sh commands instead of TSV (each path only
                    when its mode and owner still match the plan)
  --plan-verify P   with --stat-list: state of every row of plan P
                    (exit 0 all applied, 3 some pending, 1 drift/missing/owner changed)
exit: 0 clean, 1 error findings, 2 usage/io error, 3 plan pending";

struct Opts {
    root: PathBuf,
    json: bool,
    only: Option<Vec<Check>>,
    levels: HashMap<Check, Level>,
    copyright: Option<String>,
    list: bool,
    stat_list: Option<String>,
    chmod_plan: bool,
    plan_trees: Vec<String>,
    rollback: bool,
    sh: bool,
    plan_verify: Option<String>,
    plan_reasons: Option<Vec<repo_check::perms::Reason>>,
}

fn parse_checks(s: &str) -> Result<Vec<Check>, String> {
    s.split(',')
        .filter(|p| !p.is_empty())
        .map(|p| Check::parse(p.trim()).ok_or_else(|| format!("unknown check {p:?}")))
        .collect()
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Opts, String> {
    let mut o = Opts {
        root: PathBuf::from("."),
        json: false,
        only: None,
        levels: HashMap::new(),
        copyright: None,
        list: false,
        stat_list: None,
        chmod_plan: false,
        plan_trees: Vec::new(),
        rollback: false,
        sh: false,
        plan_verify: None,
        plan_reasons: None,
    };
    let mut it = args;
    while let Some(a) = it.next() {
        let mut val = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "--root" => o.root = PathBuf::from(val("--root")?),
            "--json" => o.json = true,
            "--only" => o.only = Some(parse_checks(&val("--only")?)?),
            "--warn" => {
                for c in parse_checks(&val("--warn")?)? {
                    o.levels.insert(c, Level::Warn);
                }
            }
            "--error" => {
                for c in parse_checks(&val("--error")?)? {
                    o.levels.insert(c, Level::Error);
                }
            }
            "--copyright" => o.copyright = Some(val("--copyright")?),
            "--list" => o.list = true,
            "--stat-list" => o.stat_list = Some(val("--stat-list")?),
            "--chmod-plan" => o.chmod_plan = true,
            "--plan-tree" => o.plan_trees.push(val("--plan-tree")?),
            "--rollback" => o.rollback = true,
            "--sh" => o.sh = true,
            "--plan-verify" => o.plan_verify = Some(val("--plan-verify")?),
            "--plan-reasons" => {
                let v = val("--plan-reasons")?;
                let mut rs = Vec::new();
                for p in v.split(',').filter(|p| !p.is_empty()) {
                    rs.push(
                        repo_check::perms::Reason::parse(p.trim())
                            .ok_or_else(|| format!("unknown plan reason {p:?}"))?,
                    );
                }
                o.plan_reasons = Some(rs);
            }
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    if (o.chmod_plan || o.plan_verify.is_some()) && o.stat_list.is_none() {
        return Err("--chmod-plan and --plan-verify need --stat-list".into());
    }
    if o.chmod_plan && o.plan_verify.is_some() {
        return Err("use either --chmod-plan or --plan-verify".into());
    }
    if !o.chmod_plan && (o.rollback || o.sh || !o.plan_trees.is_empty() || o.plan_reasons.is_some())
    {
        return Err("--plan-tree, --plan-reasons, --rollback and --sh need --chmod-plan".into());
    }
    Ok(o)
}

fn read_stat_list(src: &str) -> std::io::Result<String> {
    if src == "-" {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)?;
        Ok(s)
    } else {
        std::fs::read_to_string(src)
    }
}

fn main() -> ExitCode {
    let opts = match parse_args(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("repo-check: {e}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    if opts.list {
        for c in Check::ALL {
            println!("{:<9} {:<5} {}", c.name(), c.default_level(), c.describe());
        }
        return ExitCode::SUCCESS;
    }
    if opts.chmod_plan || opts.plan_verify.is_some() {
        return plan_main(&opts);
    }
    let report = if let Some(src) = opts.stat_list.as_deref() {
        match read_stat_list(src) {
            Ok(text) => {
                let (findings, n) = repo_check::perms::scan_stat_list(&text);
                repo_check::Report {
                    files_scanned: n,
                    findings,
                }
            }
            Err(e) => {
                eprintln!("repo-check: cannot read stat list {src}: {e}");
                return ExitCode::from(2);
            }
        }
    } else {
        match scan_tree(&opts.root, opts.copyright.as_deref()) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("repo-check: cannot scan {}: {e}", opts.root.display());
                return ExitCode::from(2);
            }
        }
    };
    let mut findings: Vec<_> = report
        .findings
        .into_iter()
        .filter(|f| opts.only.as_ref().is_none_or(|o| o.contains(&f.check)))
        .map(|mut f| {
            if let Some(l) = opts.levels.get(&f.check) {
                f.level = *l;
            }
            f
        })
        .collect();
    findings.sort_by(|a, b| {
        (std::cmp::Reverse(a.level), a.check, &a.file, a.line).cmp(&(
            std::cmp::Reverse(b.level),
            b.check,
            &b.file,
            b.line,
        ))
    });
    let errors = findings.iter().filter(|f| f.level == Level::Error).count();
    let warnings = findings.len() - errors;
    let ts = zagreb_time::now_rfc3339();

    if opts.json {
        let items: Vec<String> = findings
            .iter()
            .map(|f| {
                format!(
                    "{{\"check\":\"{}\",\"level\":\"{}\",\"file\":\"{}\",\"line\":{},\"msg\":\"{}\"}}",
                    f.check.name(),
                    f.level,
                    json_escape(&f.file),
                    f.line,
                    json_escape(&f.msg)
                )
            })
            .collect();
        println!(
            "{{\"tool\":\"repo-check\",\"version\":\"{}\",\"ts\":\"{}\",\"files_scanned\":{},\"errors\":{},\"warnings\":{},\"pass\":{},\"findings\":[{}]}}",
            env!("CARGO_PKG_VERSION"),
            ts,
            report.files_scanned,
            errors,
            warnings,
            errors == 0,
            items.join(",")
        );
    } else {
        for f in &findings {
            let loc = if f.line == 0 {
                f.file.clone()
            } else {
                format!("{}:{}", f.file, f.line)
            };
            println!("{:<5} {:<8} {loc}: {}", f.level, f.check.name(), f.msg);
        }
        println!(
            "repo-check {ts}: {} files, {errors} error(s), {warnings} warning(s) -> {}",
            report.files_scanned,
            if errors == 0 { "PASS" } else { "FAIL" }
        );
    }
    if errors == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// `--chmod-plan` / `--plan-verify`: staged plan output, never changes a file.
fn plan_main(opts: &Opts) -> ExitCode {
    use repo_check::perms::{self, RowState};
    let src = opts.stat_list.as_deref().unwrap_or("-");
    let text = match read_stat_list(src) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("repo-check: cannot read stat list {src}: {e}");
            return ExitCode::from(2);
        }
    };
    let entries = perms::parse_stat_list(&text);
    let ts = zagreb_time::now_rfc3339();
    if let Some(pf) = opts.plan_verify.as_deref() {
        let rows = match std::fs::read_to_string(pf)
            .map_err(|e| e.to_string())
            .and_then(|t| perms::parse_plan_tsv(&t))
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("repo-check: cannot read plan {pf}: {e}");
                return ExitCode::from(2);
            }
        };
        let (mut applied, mut pending, mut bad) = (0, 0, 0);
        for (r, st) in perms::verify_plan(&rows, &entries) {
            let s = match st {
                RowState::Applied => {
                    applied += 1;
                    "applied".to_string()
                }
                RowState::Pending => {
                    pending += 1;
                    "pending".to_string()
                }
                RowState::Missing => {
                    bad += 1;
                    "MISSING".to_string()
                }
                RowState::Drift(m) => {
                    bad += 1;
                    format!("DRIFT now {m:o}")
                }
                RowState::OwnerChanged(o) => {
                    bad += 1;
                    format!("OWNER now {o}")
                }
            };
            println!("{s:<16} {:o}->{:o} {}", r.old, r.new, r.path);
        }
        println!(
            "plan-verify {ts}: {} rows, {applied} applied, {pending} pending, {bad} drift/missing/owner",
            rows.len()
        );
        return if bad > 0 {
            ExitCode::from(1)
        } else if pending > 0 {
            ExitCode::from(3)
        } else {
            ExitCode::SUCCESS
        };
    }
    let mut plan = perms::build_plan(&entries, &opts.plan_trees);
    if let Some(rs) = &opts.plan_reasons {
        plan.rows.retain(|r| rs.contains(&r.reason));
    }
    let rows = if opts.rollback {
        perms::rollback(&plan.rows)
    } else {
        plan.rows.clone()
    };
    let title = format!(
        "{} plan, built {ts} by repo-check {} from {} stat entries{}\nhidden (not planned, parent dir has no o+x): {}\nowner review (not root:root): {}",
        if opts.rollback { "ROLLBACK" } else { "chmod" },
        env!("CARGO_PKG_VERSION"),
        entries.len(),
        if opts.plan_trees.is_empty() {
            String::new()
        } else {
            format!("; trees {}", opts.plan_trees.join(", "))
        },
        plan.hidden.len(),
        if plan.review.is_empty() {
            "none".to_string()
        } else {
            plan.review.join(", ")
        }
    );
    if opts.sh {
        print!("{}", perms::render_plan_sh(&rows, &title));
    } else {
        print!("{}", perms::render_plan_tsv(&rows, &title));
    }
    eprintln!(
        "repo-check: plan {} rows, {} hidden, {} owner review",
        rows.len(),
        plan.hidden.len(),
        plan.review.len()
    );
    ExitCode::SUCCESS
}
