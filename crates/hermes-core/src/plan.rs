// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Loader for the sector plans (`/workspace/swarm/plans/<sector>.md`).
//!
//! The seven planners wrote Markdown by hand, with four row shapes:
//!
//! * `| S1.1 Finish lan-routing ... | 1 | 1 | ... |` (id and title in one cell)
//! * `| S4.1 | tuner 8650 binary swap ... | 1 | 1 | ... |` (id in its own cell)
//! * `| T1.1 title ... |` and `| T1 title ... |` (T-ids, the latter per session)
//! * header rows that name the columns (`target`, `useful`, `inputs / deps`)
//!
//! A task is a table row whose first cell starts with a task id, inside a
//! `### S<n>` session section. Column positions come from the header row.
//! Dependencies are the task ids (`S1.1`, `T2.1`) and session ids (`S1`)
//! named in the deps column of the same sector; a session id means "every
//! task of that session". "blocks ..." is a forward note, not a dependency.

use crate::action::{classify, ActionClass};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Session {
    /// `S1`, `S0`, ...
    pub id: String,
    /// Position in the file, 0-based.
    pub ord: usize,
    pub title: String,
    /// "Approval ..." lines of the session, verbatim.
    pub approval: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// Unique key, `<sector>:<id>`; per-session T-ids become `<S>.<T>`.
    pub key: String,
    /// The id as written in the plan.
    pub id: String,
    pub sector: String,
    pub session: String,
    pub session_ord: usize,
    pub title: String,
    pub target: Option<u32>,
    pub useful: Option<u32>,
    pub deps_text: String,
    /// Resolved dependency keys (same sector).
    pub deps: Vec<String>,
    pub class: ActionClass,
    /// 1-based line in the plan file.
    pub line: usize,
}

impl Task {
    /// Slots this task occupies when dispatched: its useful parallelism,
    /// at least 1, at most the sector cap.
    pub fn slots(&self, sector_cap: usize) -> usize {
        (self.useful.unwrap_or(1).max(1) as usize).min(sector_cap.max(1))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub sector: String,
    pub sessions: Vec<Session>,
    pub tasks: Vec<Task>,
    /// Dependency cycles found (each as a list of keys). Empty when sound.
    pub cycles: Vec<Vec<String>>,
    /// Rows that restate another task ("(= S1.1)"), skipped.
    pub aliases: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Totals {
    pub sessions: usize,
    pub tasks: usize,
    pub target: u64,
    pub useful: u64,
}

impl Plan {
    pub fn totals(&self) -> Totals {
        Totals {
            sessions: self.sessions.len(),
            tasks: self.tasks.len(),
            target: self
                .tasks
                .iter()
                .map(|t| u64::from(t.target.unwrap_or(0)))
                .sum(),
            useful: self
                .tasks
                .iter()
                .map(|t| u64::from(t.useful.unwrap_or(0)))
                .sum(),
        }
    }

    /// Load a plan file.
    ///
    /// # Errors
    /// I/O errors, or a plan with no sessions.
    pub fn load(sector: &str, path: &Path) -> Result<Plan, String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let plan = parse(sector, &text);
        if plan.sessions.is_empty() {
            return Err(format!("{}: no `### S<n>` sessions found", path.display()));
        }
        Ok(plan)
    }
}

/// `S1`, `S12` (a session id).
fn session_id(tok: &str) -> Option<&str> {
    let t = tok.trim_end_matches(['.', ':', ',']);
    let digits = t.strip_prefix('S')?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then_some(t)
}

/// `S1.1`, `T2.1`, `T1`, `S0.1`, `T1.2b`. Not `S1` (that is a session).
fn task_id(tok: &str) -> Option<String> {
    let t = tok.trim_matches(|c: char| matches!(c, '*' | '`' | ',' | ';' | '(' | ')' | ':' | '.'));
    let (head, rest) = t.split_at(t.char_indices().nth(1).map_or(t.len(), |(i, _)| i));
    if head != "S" && head != "T" {
        return None;
    }
    let mut parts = rest.splitn(2, '.');
    let major = parts.next()?;
    if major.is_empty() || !major.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    match parts.next() {
        None if head == "T" => Some(t.to_string()),
        None => None,
        Some(minor) => {
            let digits = minor.trim_end_matches(|c: char| c.is_ascii_lowercase());
            let ok = !digits.is_empty()
                && digits.bytes().all(|b| b.is_ascii_digit())
                && minor.len() - digits.len() <= 1;
            ok.then(|| t.to_string())
        }
    }
}

fn cells(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    inner.split('|').map(|c| c.trim().to_string()).collect()
}

fn is_separator(line: &str) -> bool {
    let l = line.trim();
    l.starts_with('|') && l.contains('-') && l.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// Leading integer of a cell: `4 (CPU-bound)` -> 4, `0 extra` -> 0, `-` -> None.
fn leading_int(cell: &str) -> Option<u32> {
    let c = cell.trim().trim_start_matches('*');
    let digits: String = c.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

#[derive(Default, Clone, Copy)]
struct Columns {
    title: Option<usize>,
    target: Option<usize>,
    useful: Option<usize>,
    deps: Option<usize>,
}

fn columns(header: &[String]) -> Columns {
    let find =
        |pred: &dyn Fn(&str) -> bool| header.iter().position(|h| pred(&h.to_ascii_lowercase()));
    let target = find(&|h| h.contains("target"));
    let useful = find(&|h| h.contains("useful"));
    let deps = find(&|h| h.contains("dep") || h.contains("input"));
    // A separate title column exists when column 0 is `#` and column 1 names the task.
    let title = if header.first().map(|h| h.trim()) == Some("#") {
        Some(1)
    } else {
        None
    };
    Columns {
        title,
        target,
        useful,
        deps,
    }
}

fn clean_title(s: &str) -> String {
    s.replace("**", "")
        .trim()
        .trim_start_matches([':', '-', ' '])
        .trim()
        .to_string()
}

/// Parse one plan. Never fails; an empty plan has no sessions.
pub fn parse(sector: &str, text: &str) -> Plan {
    let lines: Vec<&str> = text.lines().collect();
    let mut sessions: Vec<Session> = Vec::new();
    let mut tasks: Vec<Task> = Vec::new();
    let mut current: Option<usize> = None;
    let mut cols = Columns::default();
    let mut seen_keys = BTreeSet::new();
    let mut aliases = Vec::new();

    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim_end();
        if line.starts_with('#') {
            cols = Columns::default();
            let hashes = line.chars().take_while(|c| *c == '#').count();
            let rest = line[hashes..].trim();
            let first = rest.split_whitespace().next().unwrap_or("");
            match (hashes, session_id(first)) {
                (3, Some(id)) => {
                    let title = rest[first.len()..]
                        .trim()
                        .trim_start_matches([':', '.', '-'])
                        .trim()
                        .to_string();
                    sessions.push(Session {
                        id: id.to_string(),
                        ord: sessions.len(),
                        title,
                        approval: Vec::new(),
                    });
                    current = Some(sessions.len() - 1);
                }
                _ => current = None,
            }
            continue;
        }
        let Some(si) = current else { continue };
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("approval")
            || lower.starts_with(&format!(
                "{} approval",
                sessions[si].id.to_ascii_lowercase()
            ))
        {
            sessions[si].approval.push(line.trim().to_string());
            continue;
        }
        if !line.trim_start().starts_with('|') || is_separator(line) {
            continue;
        }
        let row = cells(line);
        if lines.get(i + 1).is_some_and(|n| is_separator(n)) {
            cols = columns(&row);
            continue;
        }
        let Some(first) = row.first() else { continue };
        let first_clean = first.replace("**", "");
        let tok = first_clean.split_whitespace().next().unwrap_or("");
        let Some(id) = task_id(tok) else { continue };
        let rest = first_clean.trim_start()[tok.len()..].to_string();
        let title_cell = cols.title.unwrap_or(0);
        let title = if rest.trim().is_empty() || title_cell != 0 {
            row.get(title_cell.max(1)).cloned().unwrap_or_default()
        } else {
            rest
        };
        let title = clean_title(&title);
        let base = if title_cell == 0 { 1 } else { 2 };
        let target = row
            .get(cols.target.unwrap_or(base))
            .and_then(|c| leading_int(c));
        let useful = row
            .get(cols.useful.unwrap_or(base + 1))
            .and_then(|c| leading_int(c));
        let deps_text = cols
            .deps
            .and_then(|d| row.get(d).cloned())
            .unwrap_or_default();
        // "(= S1.1)" / "shared with S1.1, not double-counted" rows with no
        // agent numbers are aliases of another task, not tasks.
        let lt = title.to_ascii_lowercase();
        if target.is_none()
            && useful.is_none()
            && (lt.contains("(= ")
                || lt.contains("not double-counted")
                || lt.contains("shared with"))
        {
            aliases.push(format!("{}: {title}", id));
            continue;
        }
        let session = sessions[si].id.clone();
        let local = if id.contains('.') {
            id.clone()
        } else {
            format!("{session}.{id}")
        };
        let mut key = format!("{sector}:{local}");
        if !seen_keys.insert(key.clone()) {
            // The same id twice in one plan: keep both, mark the second by line.
            key = format!("{key}@{}", i + 1);
            seen_keys.insert(key.clone());
        }
        let class = classify(&title, &sessions[si].title);
        tasks.push(Task {
            key,
            id,
            sector: sector.to_string(),
            session,
            session_ord: si,
            title,
            target,
            useful,
            deps_text,
            deps: Vec::new(),
            class,
            line: i + 1,
        });
    }

    resolve_deps(&mut tasks);
    let cycles = find_cycles(&tasks);
    Plan {
        sector: sector.to_string(),
        sessions,
        tasks,
        cycles,
        aliases,
    }
}

fn resolve_deps(tasks: &mut [Task]) {
    let by_id: BTreeMap<String, String> = tasks
        .iter()
        .filter(|t| t.id.contains('.'))
        .map(|t| (t.id.clone(), t.key.clone()))
        .collect();
    let by_session_id: BTreeMap<(String, String), String> = tasks
        .iter()
        .map(|t| ((t.session.clone(), t.id.clone()), t.key.clone()))
        .collect();
    let mut by_session: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for t in tasks.iter() {
        by_session
            .entry(t.session.clone())
            .or_default()
            .push(t.key.clone());
    }
    for t in tasks.iter_mut() {
        let text = t.deps_text.replace("**", "");
        let cut = text
            .to_ascii_lowercase()
            .find("blocks")
            .unwrap_or(text.len());
        let mut deps = BTreeSet::new();
        for tok in
            text[..cut].split(|c: char| c.is_whitespace() || c == ',' || c == '+' || c == '/')
        {
            let tok =
                tok.trim_matches(|c: char| matches!(c, '(' | ')' | ';' | '`' | '*' | '.' | ':'));
            if let Some(sid) = session_id(tok) {
                if sid != t.session {
                    for k in by_session.get(sid).into_iter().flatten() {
                        deps.insert(k.clone());
                    }
                }
            } else if let Some(id) = task_id(tok) {
                let hit = if id.contains('.') {
                    by_id.get(&id)
                } else {
                    by_session_id.get(&(t.session.clone(), id.clone()))
                };
                if let Some(k) = hit {
                    deps.insert(k.clone());
                }
            }
        }
        deps.remove(&t.key);
        t.deps = deps.into_iter().collect();
    }
}

/// Every cycle reachable in the dependency graph (Tarjan-style DFS colours).
fn find_cycles(tasks: &[Task]) -> Vec<Vec<String>> {
    let idx: BTreeMap<&str, usize> = tasks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.key.as_str(), i))
        .collect();
    let mut colour = vec![0u8; tasks.len()];
    let mut stack: Vec<usize> = Vec::new();
    let mut cycles = Vec::new();
    fn visit(
        n: usize,
        tasks: &[Task],
        idx: &BTreeMap<&str, usize>,
        colour: &mut [u8],
        stack: &mut Vec<usize>,
        cycles: &mut Vec<Vec<String>>,
    ) {
        colour[n] = 1;
        stack.push(n);
        for d in &tasks[n].deps {
            let Some(&m) = idx.get(d.as_str()) else {
                continue;
            };
            match colour[m] {
                0 => visit(m, tasks, idx, colour, stack, cycles),
                1 => {
                    let start = stack.iter().position(|x| *x == m).unwrap_or(0);
                    cycles.push(
                        stack[start..]
                            .iter()
                            .map(|i| tasks[*i].key.clone())
                            .collect(),
                    );
                }
                _ => {}
            }
        }
        stack.pop();
        colour[n] = 2;
    }
    for n in 0..tasks.len() {
        if colour[n] == 0 {
            visit(n, tasks, &idx, &mut colour, &mut stack, &mut cycles);
        }
    }
    cycles
}

#[cfg(test)]
mod tests {
    use super::*;

    // Excerpts copied from the real plans (gaia.md, vps-ops.md, nerve.md,
    // integration-ci.md, cortex.md), shortened.
    const FIXTURE: &str = "\
# Sector plan
## 3. Sessions
### S1: Land everything Gaia on one branch (Oct 20-22)
| Task | Target agents | Useful max | Parallel units | Inputs / deps | Success |
|---|---|---|---|---|---|
| S1.1 Finish lan-routing (already running, slot 31) | 1 | 1 | one branch | running agent. Push needed | x |
| S1.3 Merge agent: integration-1, then gaia-land | 1 | 1 | none | S1.1, S1.2 | x |
| S1.4 Verification fan-out on the merged head | 8 | 4 (CPU-bound: 8 cores) | per gate | S1.3 | x |
Approval: merge only.
### S2. integration-2
| task | target | useful | parallel units | inputs / deps | success |
|---|---|---|---|---|---|
| T1 clean-clone re-verify of 8fd26f6 | 6 | 3 | (a) | fresh clone | ok |
| T3 owner merge pack (draft only) | 1 | 1 | - | T1, T2 | text |
### S3. Sensors
| # | task | target | useful | parallel units | inputs / deps |
|---|---|---|---|---|---|
| T3.1 | Source inventory | 9 | 9 | per function | **blocks all of S2** and S1 |
### S4: Cutovers, one service per approval (Oct 22-29)
| # | Task | Target | Useful | Rollback | Gates |
|---|---|---|---|---|---|
| S4.1 | tuner 8650 binary swap via galion-deploy | 1 | 1 | auto | x |
## 4. Totals
| S1 land | 4 | 20 | 17 |
";

    #[test]
    fn parses_all_four_row_shapes() {
        let p = parse("x", FIXTURE);
        assert_eq!(p.sessions.len(), 4);
        let keys: Vec<_> = p.tasks.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(
            keys,
            ["x:S1.1", "x:S1.3", "x:S1.4", "x:S2.T1", "x:S2.T3", "x:T3.1", "x:S4.1"]
        );
        let t = &p.tasks[2];
        assert_eq!((t.target, t.useful), (Some(8), Some(4)));
        assert_eq!(p.tasks[5].title, "Source inventory");
        assert_eq!((p.tasks[5].target, p.tasks[5].useful), (Some(9), Some(9)));
        assert_eq!(p.tasks[6].title, "tuner 8650 binary swap via galion-deploy");
        assert_eq!(p.tasks[6].class, ActionClass::Deploy);
        assert_eq!(p.sessions[0].approval, ["Approval: merge only."]);
        assert_eq!(p.totals().tasks, 7);
    }

    #[test]
    fn deps_resolve_by_id_session_and_ignore_blocks() {
        let p = parse("x", FIXTURE);
        let get = |k: &str| p.tasks.iter().find(|t| t.key == k).unwrap();
        assert_eq!(get("x:S1.3").deps, ["x:S1.1"]); // S1.2 does not exist
        assert_eq!(get("x:S1.4").deps, ["x:S1.3"]);
        assert_eq!(get("x:S2.T3").deps, ["x:S2.T1"]);
        assert!(
            get("x:T3.1").deps.is_empty(),
            "blocks ... is not a dependency"
        );
        assert!(p.cycles.is_empty());
    }

    #[test]
    fn ids() {
        assert_eq!(task_id("S1.1").as_deref(), Some("S1.1"));
        assert_eq!(task_id("**T2.1**").as_deref(), Some("T2.1"));
        assert_eq!(task_id("T1").as_deref(), Some("T1"));
        assert_eq!(task_id("S1"), None);
        assert_eq!(task_id("S1.2b").as_deref(), Some("S1.2b"));
        assert_eq!(task_id("Sx.1"), None);
        assert_eq!(session_id("S12."), Some("S12"));
        assert_eq!(leading_int("0 (one shell loop)"), Some(0));
        assert_eq!(leading_int("-"), None);
    }

    #[test]
    fn cycles_are_reported() {
        let text = "### S1 a\n| t | target | useful | deps |\n|---|---|---|---|\n| S1.1 a | 1 | 1 | S1.2 |\n| S1.2 b | 1 | 1 | S1.1 |\n";
        let p = parse("c", text);
        assert_eq!(p.cycles.len(), 1);
    }
}
