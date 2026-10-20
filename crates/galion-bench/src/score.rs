// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Equal-weight scorecards. Every axis is 0-100; the card's percentage is the
//! plain mean of its axes, rounded half up. An empty card scores 0, never 100:
//! "no evidence" must not read as "done".

/// A benchmark scorecard: named axes, each scored 0-100, equal weight.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scorecard {
    /// `(axis name, score 0-100)`. Scores above 100 are clamped when averaged.
    pub axes: Vec<(String, u8)>,
}

impl Scorecard {
    /// An empty card.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add (or replace) an axis. Scores above 100 are stored clamped to 100.
    pub fn set(&mut self, name: impl Into<String>, score: u8) -> &mut Self {
        let name = name.into();
        let score = score.min(100);
        if let Some(slot) = self.axes.iter_mut().find(|(n, _)| *n == name) {
            slot.1 = score;
        } else {
            self.axes.push((name, score));
        }
        self
    }

    /// Pass/fail axis: 100 if `ok`, else 0.
    pub fn pass(&mut self, name: impl Into<String>, ok: bool) -> &mut Self {
        self.set(name, if ok { 100 } else { 0 })
    }

    /// Axis from `passed` of `total` checks, as a 0-100 ratio (rounded half up).
    /// `total == 0` scores 0.
    pub fn ratio(&mut self, name: impl Into<String>, passed: u32, total: u32) -> &mut Self {
        let s = if total == 0 {
            0
        } else {
            let p = u64::from(passed.min(total));
            ((p * 200 + u64::from(total)) / (2 * u64::from(total))) as u8
        };
        self.set(name, s)
    }

    /// The equal-weight mean of all axes, 0-100, rounded half up. Empty -> 0.
    pub fn pct(&self) -> u8 {
        if self.axes.is_empty() {
            return 0;
        }
        let n = self.axes.len() as u64;
        let sum: u64 = self
            .axes
            .iter()
            .map(|(_, s)| u64::from((*s).min(100)))
            .sum();
        ((sum * 2 + n) / (2 * n)) as u8
    }

    /// The unrounded mean, for reports that want a decimal. Empty -> 0.0.
    pub fn pct_exact(&self) -> f64 {
        if self.axes.is_empty() {
            return 0.0;
        }
        let sum: u64 = self
            .axes
            .iter()
            .map(|(_, s)| u64::from((*s).min(100)))
            .sum();
        sum as f64 / self.axes.len() as f64
    }

    /// Axes below 100, in insertion order. The scoreboard lists these.
    pub fn failing(&self) -> Vec<&(String, u8)> {
        self.axes.iter().filter(|(_, s)| *s < 100).collect()
    }

    /// Markdown table for `docs/bench/SCOREBOARD.md`, stamped with `stamp`
    /// (pass [`crate::now_zagreb`]; taken as a parameter so output is testable).
    pub fn to_markdown(&self, title: &str, stamp: &str) -> String {
        let mut out = format!(
            "## {title}: {}%\n\nScored {stamp} (Europe/Zagreb).\n\n| Axis | Score |\n|---|---|\n",
            self.pct()
        );
        for (n, s) in &self.axes {
            out.push_str(&format!("| {n} | {s} |\n"));
        }
        let failing = self.failing();
        if !failing.is_empty() {
            out.push_str("\nBelow 100: ");
            let names: Vec<String> = failing.iter().map(|(n, s)| format!("{n} ({s})")).collect();
            out.push_str(&names.join(", "));
            out.push('\n');
        }
        out
    }

    /// JSON form: `{"pct":N,"axes":[{"name":..,"score":..}]}`.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "pct": self.pct(),
            "axes": self.axes.iter().map(|(n, s)| serde_json::json!({"name": n, "score": s})).collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_zero() {
        assert_eq!(Scorecard::new().pct(), 0);
        assert_eq!(Scorecard::new().pct_exact(), 0.0);
    }

    #[test]
    fn averages_and_rounds_half_up() {
        let card = Scorecard {
            axes: vec![("a".into(), 100), ("b".into(), 0), ("c".into(), 50)],
        };
        assert_eq!(card.pct(), 50);
        let card = Scorecard {
            axes: vec![("a".into(), 100), ("b".into(), 99)],
        };
        assert_eq!(card.pct(), 100); // 99.5 -> 100
        let card = Scorecard {
            axes: vec![("a".into(), 100), ("b".into(), 100), ("c".into(), 99)],
        };
        assert_eq!(card.pct(), 100); // 99.67
        let card = Scorecard {
            axes: vec![("a".into(), 100), ("b".into(), 98)],
        };
        assert_eq!(card.pct(), 99);
    }

    #[test]
    fn clamps_over_100() {
        let card = Scorecard {
            axes: vec![("a".into(), 255), ("b".into(), 0)],
        };
        assert_eq!(card.pct(), 50);
        let mut c = Scorecard::new();
        c.set("x", 200);
        assert_eq!(c.axes[0].1, 100);
    }

    #[test]
    fn set_replaces_ratio_and_failing() {
        let mut c = Scorecard::new();
        c.pass("auth", true).pass("audit", false).ratio("fx", 2, 3);
        c.set("audit", 100);
        assert_eq!(c.axes.len(), 3);
        assert_eq!(c.axes[2], ("fx".to_string(), 67));
        assert_eq!(c.failing().len(), 1);
        assert_eq!(c.pct(), 89); // (100+100+67)/3 = 89.0
        let mut z = Scorecard::new();
        z.ratio("none", 5, 0);
        assert_eq!(z.pct(), 0);
    }

    #[test]
    fn markdown_lists_failing() {
        let mut c = Scorecard::new();
        c.pass("ok", true).set("half", 50);
        let md = c.to_markdown("Banking", "2026-10-20T23:10:00+02:00");
        assert!(md.contains("## Banking: 75%"));
        assert!(md.contains("Below 100: half (50)"));
        assert_eq!(c.to_json()["pct"], 75);
    }
}
