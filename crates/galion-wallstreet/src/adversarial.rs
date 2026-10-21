// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! B2 adversarial set: reworded insider tips, pump-and-dump asks, other
//! manipulation, obfuscation, wrappers (role-play, hypotheticals, "explain
//! then do"), Croatian, and benign hard negatives that must still pass.
//!
//! Three committed JSONL sets: `dev` (written first, the rules were tuned on
//! it), `holdout` (written after the rules were frozen) and `blind1` (written
//! after the holdout fixes, first run over real HTTP). Each set's first-run
//! number is recorded in docs/bench/wallstreet-adversarial.md; the misses were
//! then fixed, so only those first-run numbers are blind. Every case runs through
//! four surfaces of the paper broker, and passes only if all four agree:
//! the screen itself, an order rationale, a scratchpad note and a ticket
//! request. The v1 phrase-only screen is scored on the same cases so the
//! scoreboard shows what the intent layer adds.

use crate::broker::{OrderRequest, PaperBroker, Side};
use crate::compliance::{self, Category, Verdict};
use crate::strategy::Scratchpad;
use crate::ticket::{self, Step, Ticket};
use serde::{Deserialize, Serialize};

pub const DEV: &str = include_str!("../fixtures/adversarial/dev.jsonl");
pub const HOLDOUT: &str = include_str!("../fixtures/adversarial/holdout.jsonl");
pub const BLIND1: &str = include_str!("../fixtures/adversarial/blind1.jsonl");

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Case {
    pub id: String,
    pub family: String,
    pub expect: Option<Category>,
    pub text: String,
}

pub fn parse(jsonl: &str) -> Result<Vec<Case>, String> {
    let mut out: Vec<Case> = Vec::new();
    for (i, l) in jsonl.lines().enumerate() {
        if l.trim().is_empty() {
            continue;
        }
        let c: Case = serde_json::from_str(l).map_err(|e| format!("line {}: {e}", i + 1))?;
        if out.iter().any(|o| o.id == c.id) {
            return Err(format!("line {}: duplicate id {}", i + 1, c.id));
        }
        out.push(c);
    }
    Ok(out)
}

/// Both committed sets, tagged with their set name.
pub fn cases() -> Vec<(&'static str, Case)> {
    let mut v = Vec::new();
    for (set, src) in [("dev", DEV), ("holdout", HOLDOUT), ("blind1", BLIND1)] {
        for c in parse(src).expect("committed adversarial fixtures parse") {
            v.push((set, c));
        }
    }
    v
}

fn category(v: &Verdict) -> Option<Category> {
    match v {
        Verdict::Allow => None,
        Verdict::Refuse { category, .. } => Some(*category),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseResult {
    pub set: &'static str,
    pub id: String,
    pub family: String,
    pub expect: Option<Category>,
    pub got: Option<Category>,
    pub matched: String,
    pub v1_got: Option<Category>,
    pub v1_pass: bool,
    pub order_code: String,
    pub note_refused: bool,
    pub ticket_status: String,
    pub ticket_steps: usize,
    pub surfaces_agree: bool,
    pub pass: bool,
}

/// Run one case through the screen, an order rationale, a note and a ticket.
pub fn run_case(set: &'static str, c: &Case) -> CaseResult {
    let v = compliance::screen(&c.text);
    let got = category(&v);
    let matched = match &v {
        Verdict::Refuse { matched, .. } => matched.clone(),
        Verdict::Allow => String::new(),
    };
    let v1_got = category(&compliance::screen_phrases(&c.text));
    let mut b = PaperBroker::fixture();
    let o = b.place(OrderRequest {
        symbol: "GLXA".into(),
        side: Some(Side::Buy),
        qty: 1,
        limit: None,
        rationale: c.text.clone(),
        ticket: format!("ADV-{}", c.id),
    });
    let order_code = o.code().to_string();
    let mut pad = Scratchpad::default();
    let note_refused = pad.add("2026-08-10", &c.id, &c.text, &[]).is_err();
    let t = Ticket {
        id: format!("ADV-{}", c.id),
        title: String::new(),
        request: c.text.clone(),
        start_date: None,
        steps: vec![Step::Quote {
            symbol: "GLXA".into(),
        }],
    };
    let tr = ticket::run_fresh(&t);
    let surfaces_agree = match got {
        Some(cat) => {
            order_code == cat.code()
                && note_refused
                && tr.status == "refused"
                && tr.steps.is_empty()
                && b.account().positions.iter().all(|p| p.qty == 0)
        }
        None => order_code == "filled" && !note_refused && tr.status == "completed",
    };
    CaseResult {
        set,
        id: c.id.clone(),
        family: c.family.clone(),
        expect: c.expect,
        got,
        matched,
        v1_got,
        v1_pass: v1_got == c.expect,
        order_code,
        note_refused,
        ticket_status: tr.status.to_string(),
        ticket_steps: tr.steps.len(),
        surfaces_agree,
        pass: got == c.expect && surfaces_agree,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Group {
    pub name: String,
    pub passed: usize,
    pub total: usize,
    pub percent: u32,
    pub v1_passed: usize,
    pub v1_percent: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Board {
    pub bench: &'static str,
    pub mode: &'static str,
    pub live_broker: bool,
    pub passed: usize,
    pub total: usize,
    pub percent: u32,
    pub v1_passed: usize,
    pub v1_percent: u32,
    pub attacks_total: usize,
    /// Attacks refused under any compliance category.
    pub attacks_refused: usize,
    /// Attacks refused under the expected category.
    pub attacks_category_ok: usize,
    pub v1_attacks_refused: usize,
    pub benign_total: usize,
    pub benign_allowed: usize,
    pub v1_benign_allowed: usize,
    pub surfaces_agree: usize,
    pub sets: Vec<Group>,
    pub families: Vec<Group>,
    pub failing: Vec<String>,
    pub cases: Vec<CaseResult>,
    pub generated_at: String,
}

fn pct(p: usize, t: usize) -> u32 {
    (p * 100).checked_div(t).unwrap_or(0) as u32
}

fn group(name: &str, rs: &[&CaseResult]) -> Group {
    let passed = rs.iter().filter(|r| r.pass).count();
    let v1 = rs.iter().filter(|r| r.v1_pass).count();
    Group {
        name: name.into(),
        passed,
        total: rs.len(),
        percent: pct(passed, rs.len()),
        v1_passed: v1,
        v1_percent: pct(v1, rs.len()),
    }
}

pub fn run() -> Board {
    let rs: Vec<CaseResult> = cases().iter().map(|(s, c)| run_case(s, c)).collect();
    let mut sets = Vec::new();
    for s in ["dev", "holdout", "blind1"] {
        let xs: Vec<&CaseResult> = rs.iter().filter(|r| r.set == s).collect();
        sets.push(group(s, &xs));
    }
    let mut fams: Vec<String> = rs.iter().map(|r| r.family.clone()).collect();
    fams.sort();
    fams.dedup();
    let families = fams
        .iter()
        .map(|f| {
            let xs: Vec<&CaseResult> = rs.iter().filter(|r| &r.family == f).collect();
            group(f, &xs)
        })
        .collect();
    let attacks: Vec<&CaseResult> = rs.iter().filter(|r| r.expect.is_some()).collect();
    let benign: Vec<&CaseResult> = rs.iter().filter(|r| r.expect.is_none()).collect();
    let passed = rs.iter().filter(|r| r.pass).count();
    let v1_passed = rs.iter().filter(|r| r.v1_pass).count();
    let failing = rs
        .iter()
        .filter(|r| !r.pass)
        .map(|r| {
            format!(
                "{}/{} expect {:?} got {:?} ({}) order {} note_refused {} ticket {}",
                r.set,
                r.id,
                r.expect,
                r.got,
                r.matched,
                r.order_code,
                r.note_refused,
                r.ticket_status
            )
        })
        .collect();
    Board {
        bench: "B2 wallstreet adversarial",
        mode: "paper",
        live_broker: false,
        passed,
        total: rs.len(),
        percent: pct(passed, rs.len()),
        v1_passed,
        v1_percent: pct(v1_passed, rs.len()),
        attacks_total: attacks.len(),
        attacks_refused: attacks.iter().filter(|r| r.got.is_some()).count(),
        attacks_category_ok: attacks.iter().filter(|r| r.got == r.expect).count(),
        v1_attacks_refused: attacks.iter().filter(|r| r.v1_got.is_some()).count(),
        benign_total: benign.len(),
        benign_allowed: benign.iter().filter(|r| r.got.is_none()).count(),
        v1_benign_allowed: benign.iter().filter(|r| r.v1_got.is_none()).count(),
        surfaces_agree: rs.iter().filter(|r| r.surfaces_agree).count(),
        sets,
        families,
        failing,
        cases: rs,
        generated_at: zagreb_time::now_rfc3339(),
    }
}

/// Short text view (CLI `adv --md`).
pub fn markdown(b: &Board) -> String {
    let mut o = format!(
        "B2 wallstreet adversarial: {}% ({}/{}), v1 phrase-only {}% ({}/{}); attacks refused {}/{} (right category {}), v1 {}/{}; benign allowed {}/{}, v1 {}/{}; surfaces agree {}/{}; generated {}\n",
        b.percent, b.passed, b.total, b.v1_percent, b.v1_passed, b.total,
        b.attacks_refused, b.attacks_total, b.attacks_category_ok, b.v1_attacks_refused, b.attacks_total,
        b.benign_allowed, b.benign_total, b.v1_benign_allowed, b.benign_total,
        b.surfaces_agree, b.total, b.generated_at
    );
    for g in b.sets.iter().chain(b.families.iter()) {
        o.push_str(&format!(
            "  {:<26} {:>3}% ({}/{})   v1 {:>3}% ({}/{})\n",
            g.name, g.percent, g.passed, g.total, g.v1_percent, g.v1_passed, g.total
        ));
    }
    for f in &b.failing {
        o.push_str(&format!("    FAIL {f}\n"));
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_parse_and_are_balanced() {
        let cs = cases();
        let attacks = cs.iter().filter(|(_, c)| c.expect.is_some()).count();
        let benign = cs.len() - attacks;
        assert!(attacks >= 80, "{attacks} attacks");
        assert!(benign >= 40, "{benign} benign");
        assert!(parse("{\"id\":\"a\",\"family\":\"f\",\"expect\":null,\"text\":\"x\"}\n{\"id\":\"a\",\"family\":\"f\",\"expect\":null,\"text\":\"y\"}").is_err());
    }

    #[test]
    fn adversarial_board_is_100_and_beats_v1() {
        let b = run();
        assert!(b.failing.is_empty(), "{:#?}", b.failing);
        assert_eq!(b.percent, 100);
        assert_eq!(b.attacks_refused, b.attacks_total);
        assert_eq!(b.benign_allowed, b.benign_total);
        assert!(b.v1_percent < b.percent, "v1 {}%", b.v1_percent);
    }

    #[test]
    fn board_is_deterministic() {
        let a = serde_json::to_value(run().cases).unwrap();
        let b = serde_json::to_value(run().cases).unwrap();
        assert_eq!(a, b);
    }
}
