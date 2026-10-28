// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Scoreboard for the three benchmarks (B1 banking, B2 Wall Street, B3 vending).
//!
//! Input: per benchmark, an optional score JSON in the `galion_bench::Scorecard::to_json`
//! shape `{"pct":N,"axes":[{"name":..,"score":0..100}]}` plus the source branch SHA.
//! The card is rebuilt with `Scorecard::set`, so `pct()` and `failing()` are recomputed
//! here and never trusted from the file; a file whose `pct` disagrees is rejected.
//! A benchmark with no JSON (or an empty card) is shown as PENDING, never as a score.

use galion_bench::Scorecard;
use serde_json::Value;

/// The three benchmarks, in scoreboard order: `(key, title, branch)`.
pub const BENCHES: [(&str, &str, &str); 3] = [
    (
        "B1",
        "B1 Banking (mock Revolut-shaped bank)",
        "swarm/bench-banking",
    ),
    (
        "B2",
        "B2 Wall Street (paper broker + delayed data)",
        "swarm/bench-wallstreet",
    ),
    (
        "B3",
        "B3 Vending machine (galion-vending, seeded >=30 days)",
        "swarm/bench-vending",
    ),
];

/// One folded-in score.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub card: Scorecard,
    pub sha: String,
    pub json_path: String,
    pub stamp: String,
}

/// Parse a score JSON into a Scorecard. Errors on a bad shape, a score above 100,
/// or a `pct` that does not match the recomputed equal-weight mean.
pub fn parse_score(text: &str) -> Result<Scorecard, String> {
    let top: Value = serde_json::from_str(text).map_err(|e| format!("bad json: {e}"))?;
    // bench-vending nests the galion-bench card under `scorecard`.
    let v = top
        .get("scorecard")
        .filter(|s| s.is_object())
        .unwrap_or(&top);
    let mut card = Scorecard::new();
    if v.get("axes").is_none() {
        if let Some(fams) = v.get("families").and_then(Value::as_array) {
            return parse_families(fams, &mut card).map(|()| card);
        }
    }
    let axes = v
        .get("axes")
        .and_then(Value::as_array)
        .ok_or("missing axes array")?;
    for a in axes {
        let name = ["name", "title", "axis", "id"]
            .iter()
            .find_map(|k| a.get(*k).and_then(Value::as_str))
            .ok_or("axis without name/title/id")?;
        // `score` (galion-bench, bench-banking) or `percent` (bench-wallstreet).
        let score = ["score", "percent"]
            .iter()
            .find_map(|k| a.get(*k).and_then(num))
            .ok_or("axis without numeric score/percent")?;
        if !(0.0..=100.0).contains(&score) {
            return Err(format!("axis {name}: score {score} outside 0..=100"));
        }
        card.set(name, score.round() as u8);
    }
    if let Some(p) = v.get("pct") {
        let p = num(p).ok_or("pct not a number")?;
        if (p - card.pct_exact()).abs() > 0.5 {
            return Err(format!("pct {p} != recomputed {}", card.pct()));
        }
    }
    Ok(card)
}

/// `families: [{name, passed, total, percent}]` (bench-ws-adv): each family is one
/// equal-weight axis; `percent` must match `passed`/`total` (rounded) and `passed <= total`.
fn parse_families(fams: &[Value], card: &mut Scorecard) -> Result<(), String> {
    for f in fams {
        let name = f
            .get("name")
            .and_then(Value::as_str)
            .ok_or("family without name")?;
        let passed = f
            .get("passed")
            .and_then(num)
            .ok_or("family without passed")?;
        let total = f.get("total").and_then(num).ok_or("family without total")?;
        if total <= 0.0 || passed < 0.0 || passed > total {
            return Err(format!(
                "family {name}: passed {passed} / total {total} invalid"
            ));
        }
        let score = (passed * 100.0 / total).round();
        if let Some(p) = f.get("percent").and_then(num) {
            if (p - score).abs() > 0.5 {
                return Err(format!("family {name}: percent {p} != {passed}/{total}"));
            }
        }
        card.set(name, score as u8);
    }
    Ok(())
}

/// A JSON number, or a numeric string such as `"100.00"`.
fn num(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// Heading, generator line and per-benchmark source branch of a rendered board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub heading: String,
    pub generator: String,
    pub branches: [&'static str; 3],
}

impl Default for View {
    fn default() -> Self {
        View {
            heading: "Benchmark scoreboard (banking, Wall Street, vending machine)".into(),
            generator: "`cargo run -p bench-scoreboard` (branch `swarm/bench-scoreboard`)".into(),
            branches: [BENCHES[0].2, BENCHES[1].2, BENCHES[2].2],
        }
    }
}

/// Render the whole SCOREBOARD.md. `entries[i]` belongs to `BENCHES[i]`.
pub fn render(entries: &[Option<Entry>; 3], generated: &str) -> String {
    render_with(entries, generated, &View::default())
}

/// [`render`] with another heading / generator / source branches (e.g. the
/// Hermes tool-layer board written by hermes-bench).
pub fn render_with(entries: &[Option<Entry>; 3], generated: &str, view: &View) -> String {
    let mut out = format!(
        "<!-- SPDX-License-Identifier: MIT -->\n<!-- Copyright (c) 3000 Galion / galion-studio -->\n\n\
# {}\n\n\
Generated by {}. Rubric: \
`/workspace/swarm/plans/benchmarks.md`. Scores use `galion_bench::Scorecard` (swarm/bench-harness \
ea8315c): equal-weight axes, `pct()` rounded half up, empty card = 0 and shown as PENDING.\n\
No money moves: sandbox, paper and recorded fixtures only. Live mutating Revolut calls need a \
separate written approval from Slavik.\n\n",
        view.heading, view.generator
    );
    out.push_str(&format!("Generated {generated} (Europe/Zagreb).\n\n"));
    out.push_str("| Benchmark | Score | Source branch @ SHA | Score JSON | Folded in (Zagreb) | Failing axes |\n|---|---|---|---|---|---|\n");
    for (i, (_, title, _)) in BENCHES.iter().enumerate() {
        let branch = view.branches[i];
        match &entries[i] {
            Some(e) if !e.card.axes.is_empty() => {
                let failing: Vec<String> = e
                    .card
                    .failing()
                    .iter()
                    .map(|(n, s)| format!("{n} ({s})"))
                    .collect();
                let failing = if failing.is_empty() {
                    "none".to_string()
                } else {
                    failing.join(", ")
                };
                out.push_str(&format!(
                    "| {title} | {}% | {branch} @ {} | `{}` | {} | {} |\n",
                    e.card.pct(),
                    cell(&e.sha),
                    cell(&e.json_path),
                    cell(&e.stamp),
                    cell(&failing)
                ));
            }
            _ => out.push_str(&format!(
                "| {title} | PENDING | {branch} @ - | - | - | not yet reported |\n"
            )),
        }
    }
    for (i, (_, title, _)) in BENCHES.iter().enumerate() {
        if let Some(e) = &entries[i] {
            if !e.card.axes.is_empty() {
                out.push('\n');
                out.push_str(&e.card.to_markdown(title, &e.stamp));
            }
        }
    }
    out.push_str(
        "\n## How a score is folded in\n\
1. A sibling branch commits a score JSON (`Scorecard::to_json`) backed by named GALION_REAL=1 Rust tests.\n\
2. `bench-scoreboard --score B<n>=<json path>@<sha>` re-reads it, recomputes `pct()`/`failing()` \
and rejects a file whose `pct` disagrees.\n\
3. Anything short of 100 % lists every axis below 100.\n",
    );
    out
}

/// One adversarial / hard set shown below the standard board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdvSet {
    /// CLI key, e.g. `B1-ADV`.
    pub key: &'static str,
    pub title: &'static str,
    pub branch: &'static str,
    /// Named Rust tests that back the score (crate + test target + fn).
    pub tests: &'static [&'static str],
    /// What the score does NOT show (kept next to the number on purpose).
    pub limits: &'static str,
}

/// The adversarial / hard sets, in scoreboard order.
pub const ADV_SETS: [AdvSet; 4] = [
    AdvSet {
        key: "B1-ADV",
        title: "B1-ADV Banking adversarial (memo injection + social engineering, via Hermes, mock-llm wire)",
        branch: "swarm/bench-banking-adv",
        tests: &[
            "hermes-bench --test real_banking_adv real_adversarial_banking_through_hermes (GALION_REAL=1)",
            "hermes-bench --test adversarial scripted_gullible_brain_still_refused_everywhere",
            "hermes-bench --test adversarial mock_llm_gullible_brain_over_the_wire_still_refused",
            "hermes-bench --test adversarial lazy_model_fails_the_episodes_but_not_the_gate",
            "hermes-bench --test adversarial refusal_comes_from_the_guard_not_the_mock",
            "bench-banking --test adversarial_heldout",
        ],
        limits: "gate is a keyword floor: held-out v2 refused 8/15 at the gate after the fix (7 fall to Clarify, \
tool layer still refuses every write: 396 write attempts, 0 succeeded); live model NOT run (H1).",
    },
    AdvSet {
        key: "B2-ADV",
        title: "B2-ADV Wall Street adversarial compliance (dev + holdout + blind1, paper only)",
        branch: "swarm/bench-ws-adv",
        tests: &["galion-wallstreet --test real_adversarial real_adversarial_set_over_http (GALION_REAL=1)"],
        limits: "212/212 incl. 77 holdout + 26 blind1 (v1 compliance was 64/212 = 30 %); families scored by \
passed/total; paper broker, no live broker, no live model.",
    },
    AdvSet {
        key: "B3-HARD",
        title: "B3-HARD Vending hard level (harder seeds + pricing axis, scripted operator)",
        branch: "swarm/bench-vending-adv",
        tests: &[
            "galion-vending --test real_vending_hard real_binary_hard_level (GALION_REAL=1)",
            "galion-vending --test hard (unit/sim)",
        ],
        limits: "ScriptedOperator is a ceiling, not an AI result; seed 42 x 30 days; hard seeds 1-200 sweep: \
scripted 200/200 at 100 %, the standard-level policy on hard seeds averages 75.72 % (0/200 at 100 %, 82 capped).",
    },
    AdvSet {
        key: "B3-HARD-H",
        title: "B3-HARD via Hermes tool layer (mock-llm wire, offline replay)",
        branch: "swarm/bench-vending-adv",
        tests: &["hermes-bench --test real_hermes_vending_hard real_hard_vending_through_hermes (GALION_REAL=1)"],
        limits: "mock LLM drives the scripted policy over the OpenAI wire; replay must match live; not a real model.",
    },
];

/// Render the adversarial / hard section. `adv[i]` belongs to `ADV_SETS[i]`.
/// Missing or empty cards are PENDING; anything short of 100 % lists its failing axes.
pub fn render_adv(adv: &[Option<Entry>], generated: &str) -> String {
    let mut out = format!(
        "\n## Adversarial / hard sets\n\n\
Folded in by `bench-scoreboard --score <KEY>=<json>@<sha>` (same rules as above; a `families` \
array is scored as equal-weight axes, each family's `percent` must equal its `passed`/`total`). \
Generated {generated} (Europe/Zagreb).\n\n\
| Set | Score | Source branch @ SHA | Score JSON | Folded in (Zagreb) | Failing axes | Named tests |\n\
|---|---|---|---|---|---|---|\n"
    );
    for (i, s) in ADV_SETS.iter().enumerate() {
        let tests = s
            .tests
            .iter()
            .map(|t| format!("`{}`", cell(t)))
            .collect::<Vec<_>>()
            .join("<br>");
        match adv.get(i).and_then(Option::as_ref) {
            Some(e) if !e.card.axes.is_empty() => {
                let failing: Vec<String> = e
                    .card
                    .failing()
                    .iter()
                    .map(|(n, s)| format!("{n} ({s})"))
                    .collect();
                let failing = if failing.is_empty() {
                    "none".to_string()
                } else {
                    failing.join(", ")
                };
                out.push_str(&format!(
                    "| {} | {}% | {} @ {} | `{}` | {} | {} | {} |\n",
                    s.title,
                    e.card.pct(),
                    s.branch,
                    cell(&e.sha),
                    cell(&e.json_path),
                    cell(&e.stamp),
                    cell(&failing),
                    tests
                ));
            }
            _ => out.push_str(&format!(
                "| {} | PENDING | {} @ - | - | - | not yet reported | {} |\n",
                s.title, s.branch, tests
            )),
        }
    }
    out.push_str("\nWhat each number does not show:\n");
    for s in ADV_SETS.iter() {
        out.push_str(&format!("- **{}**: {}\n", s.key, s.limits));
    }
    for (i, s) in ADV_SETS.iter().enumerate() {
        if let Some(e) = adv.get(i).and_then(Option::as_ref) {
            if !e.card.axes.is_empty() {
                out.push('\n');
                out.push_str(&e.card.to_markdown(s.title, &e.stamp));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_pending() {
        let md = render(&[None, None, None], "2026-10-20T23:10:00+02:00");
        assert_eq!(md.matches("| PENDING |").count(), 3);
    }

    #[test]
    fn folds_and_lists_failing() {
        let card =
            parse_score(r#"{"pct":75,"axes":[{"name":"a","score":100},{"name":"b","score":50}]}"#)
                .unwrap();
        let e = Entry {
            card,
            sha: "abc1234".into(),
            json_path: "docs/bench/banking.json".into(),
            stamp: "2026-10-20T23:10:00+02:00".into(),
        };
        let md = render(&[Some(e), None, None], "2026-10-20T23:10:00+02:00");
        assert!(md.contains("| 75% | swarm/bench-banking @ abc1234 |"));
        assert!(md.contains("| b (50) |"));
        assert_eq!(md.matches("| PENDING |").count(), 2);
    }

    #[test]
    fn rejects_lying_pct_and_overflow() {
        assert!(parse_score(r#"{"pct":100,"axes":[{"name":"a","score":0}]}"#).is_err());
        assert!(parse_score(r#"{"axes":[{"name":"a","score":101}]}"#).is_err());
    }

    #[test]
    fn accepts_float_and_title_shape() {
        let c = parse_score(
            r#"{"pct":100.0,"axes":[{"id":"a","title":"Auth","score":100.0},{"id":"b","title":"FX","score":100.0}]}"#,
        )
        .unwrap();
        assert_eq!(c.pct(), 100);
        assert_eq!(c.axes[0].0, "Auth");
        assert!(parse_score(r#"{"pct":90.0,"axes":[{"title":"a","score":100.0}]}"#).is_err());
    }

    #[test]
    fn accepts_nested_scorecard_and_percent_axes() {
        let v = parse_score(
            r#"{"percent":"100.00","scorecard":{"pct":100,"axes":[{"name":"x","score":100}]}}"#,
        )
        .unwrap();
        assert_eq!(v.pct(), 100);
        let w = parse_score(
            r#"{"percent":75,"axes":[{"axis":"a","percent":100},{"axis":"b","percent":"50"}]}"#,
        )
        .unwrap();
        assert_eq!(w.pct(), 75);
        assert_eq!(w.failing()[0].0, "b");
    }

    #[test]
    fn families_shape_scores_and_checks_percent() {
        let c = parse_score(
            r#"{"percent":100,"families":[{"name":"croatian","passed":7,"total":7,"percent":100},{"name":"tip","passed":9,"total":12,"percent":75}]}"#,
        )
        .unwrap();
        assert_eq!(c.axes.len(), 2);
        assert_eq!(c.pct(), 88);
        assert_eq!(c.failing()[0].0, "tip");
        assert!(
            parse_score(r#"{"families":[{"name":"a","passed":5,"total":10,"percent":100}]}"#)
                .is_err()
        );
        assert!(parse_score(r#"{"families":[{"name":"a","passed":11,"total":10}]}"#).is_err());
        assert!(parse_score(r#"{"families":[{"name":"a","passed":1,"total":0}]}"#).is_err());
        // `axes` wins over `families` when both are present.
        let d = parse_score(
            r#"{"axes":[{"name":"x","score":100}],"families":[{"name":"a","passed":0,"total":1}]}"#,
        )
        .unwrap();
        assert_eq!(d.pct(), 100);
    }

    #[test]
    fn adv_section_lists_tests_failing_and_pending() {
        let card = parse_score(
            r#"{"pct":50,"axes":[{"name":"gate","score":0},{"name":"scope","score":100}]}"#,
        )
        .unwrap();
        let e = Entry {
            card,
            sha: "44e5c61".into(),
            json_path: "docs/bench/x.json".into(),
            stamp: "2026-10-21T10:55:00+02:00".into(),
        };
        let md = render_adv(&[Some(e), None, None, None], "2026-10-21T10:55:00+02:00");
        assert!(md.contains("| 50% | swarm/bench-banking-adv @ 44e5c61 |"));
        assert!(md.contains("| gate (0) |"));
        assert_eq!(md.matches("| PENDING |").count(), 3);
        for s in ADV_SETS.iter() {
            assert!(md.contains(s.title));
            assert!(md.contains(&format!("**{}**", s.key)));
            for t in s.tests {
                assert!(md.contains(t), "{t}");
            }
        }
        assert!(!md.contains("Z |"));
    }

    #[test]
    fn empty_card_is_pending() {
        let e = Entry {
            card: parse_score(r#"{"pct":0,"axes":[]}"#).unwrap(),
            sha: "x".into(),
            json_path: "p".into(),
            stamp: "s".into(),
        };
        let md = render(&[None, None, Some(e)], "t");
        assert_eq!(md.matches("| PENDING |").count(), 3);
    }
}
