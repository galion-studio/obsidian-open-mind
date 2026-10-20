// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! News / event fixtures and deterministic tagging.
//!
//! Tags describe *what kind* of event a headline is, and two risk flags say how
//! far to trust it: `unverified` (rumour, social, forum) and `mnpi_risk`
//! (leak or pre-announcement framing). A `mnpi_risk` item is never usable as a
//! trade reason; the report shows it only as a warning.

use serde::{Deserialize, Serialize};

pub const NEWS_FIXTURE: &str = include_str!("../fixtures/news.jsonl");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewsItem {
    pub date: String,
    /// Ticker, or `*` for market-wide.
    pub symbol: String,
    pub headline: String,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tag {
    Earnings,
    Guidance,
    MergersAcquisitions,
    Macro,
    Regulatory,
    Management,
    TradingHalt,
    Contract,
    Unverified,
    MnpiRisk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tagged {
    #[serde(flatten)]
    pub item: NewsItem,
    pub tags: Vec<Tag>,
    pub usable_as_reason: bool,
}

const RULES: &[(Tag, &[&str])] = &[
    (
        Tag::Earnings,
        &[
            "earnings", "eps", "revenue", "quarter", "q1", "q2", "q3", "q4",
        ],
    ),
    (Tag::Guidance, &["guidance", "outlook", "forecast"]),
    (
        Tag::MergersAcquisitions,
        &["acquire", "acquisition", "merger", "deal", "takeover"],
    ),
    (
        Tag::Macro,
        &["central bank", "rates", "inflation", "jobs report", "gdp"],
    ),
    (
        Tag::Regulatory,
        &["regulator", "fine", "fines", "probe", "sanction"],
    ),
    (
        Tag::Management,
        &["ceo", "cfo", "departs", "appoints", "resigns"],
    ),
    (Tag::TradingHalt, &["halt", "halts", "suspended"]),
    (Tag::Contract, &["contract", "order book", "wins bid"]),
    (
        Tag::Unverified,
        &[
            "rumor",
            "rumour",
            "reportedly",
            "sources say",
            "unconfirmed",
        ],
    ),
    (
        Tag::MnpiRisk,
        &[
            "leaked",
            "leak",
            "before announcement",
            "before the announcement",
            "insider",
        ],
    ),
];

pub fn parse(jsonl: &str) -> Result<Vec<NewsItem>, String> {
    let mut out = Vec::new();
    for (i, line) in jsonl.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let it: NewsItem =
            serde_json::from_str(line).map_err(|e| format!("line {}: {e}", i + 1))?;
        out.push(it);
    }
    Ok(out)
}

pub fn fixture() -> Vec<NewsItem> {
    parse(NEWS_FIXTURE).expect("committed news fixture is valid")
}

fn has_word(hay: &str, needle: &str) -> bool {
    if needle.contains(' ') {
        return hay.contains(needle);
    }
    hay.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| w == needle)
}

pub fn tag(item: &NewsItem) -> Tagged {
    let h = item.headline.to_lowercase();
    let mut tags: Vec<Tag> = RULES
        .iter()
        .filter(|(_, ws)| ws.iter().any(|w| has_word(&h, w)))
        .map(|(t, _)| *t)
        .collect();
    if matches!(item.source.as_str(), "social" | "forum") && !tags.contains(&Tag::Unverified) {
        tags.push(Tag::Unverified);
    }
    tags.sort();
    let usable = !tags.contains(&Tag::Unverified) && !tags.contains(&Tag::MnpiRisk);
    Tagged {
        item: item.clone(),
        tags,
        usable_as_reason: usable,
    }
}

/// Items for `symbol` (plus market-wide) dated on or before `as_of`.
pub fn context(items: &[NewsItem], symbol: &str, as_of: &str) -> Vec<Tagged> {
    items
        .iter()
        .filter(|n| (n.symbol == symbol || n.symbol == "*") && n.date.as_str() <= as_of)
        .map(tag)
        .collect()
}

/// Expected tags for every fixture line, in file order (the scoreboard's key).
pub fn expected_fixture_tags() -> Vec<Vec<Tag>> {
    use Tag::*;
    vec![
        vec![Earnings, Guidance],
        vec![Contract],
        vec![Guidance],
        vec![Macro],
        vec![MergersAcquisitions, Unverified],
        vec![MergersAcquisitions, Unverified, MnpiRisk],
        vec![Regulatory],
        vec![Unverified],
        vec![TradingHalt],
        vec![Guidance, Management],
        vec![Macro],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_tags_match_key() {
        let items = fixture();
        let want = expected_fixture_tags();
        assert_eq!(items.len(), want.len());
        for (it, w) in items.iter().zip(want) {
            assert_eq!(tag(it).tags, w, "{}", it.headline);
        }
    }

    #[test]
    fn context_respects_date_and_flags() {
        let items = fixture();
        let c = context(&items, "GLXA", "2026-09-20");
        assert_eq!(c.len(), 3);
        assert!(c.iter().any(|t| !t.usable_as_reason));
        assert!(context(&items, "GLXA", "2026-08-11").is_empty());
    }
}
