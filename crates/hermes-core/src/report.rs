// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! How Hermes speaks to the operator: one spoken sentence, then detail.
//!
//! SOUL.md: "The console reads your first sentence aloud and stops. That
//! sentence is one breath: plain, finished, no list, no path, no tool name.
//! The screen holds the rest." [`spoken_ok`] enforces that.

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Read aloud. One sentence.
    pub spoken: String,
    /// Shown on screen.
    pub detail: Vec<String>,
}

impl Report {
    pub fn new(spoken: String, detail: Vec<String>) -> Report {
        Report { spoken, detail }
    }

    pub fn render(&self) -> String {
        let mut out = self.spoken.clone();
        out.push('\n');
        for d in &self.detail {
            out.push_str("  ");
            out.push_str(d);
            out.push('\n');
        }
        out
    }
}

/// Longest spoken sentence, in words (one breath).
pub const MAX_SPOKEN_WORDS: usize = 32;

/// Check the spoken-sentence rule.
///
/// # Errors
/// The first rule the sentence breaks.
pub fn spoken_ok(s: &str) -> Result<(), String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("empty".into());
    }
    if !t.ends_with('.') {
        return Err("must end with a full stop".into());
    }
    let body = &t[..t.len() - 1];
    if body.contains(['.', '!', '?']) {
        return Err("more than one sentence".into());
    }
    if t.contains(['\n', '/', '\\', '`', '|', '*', '#', '_', '{', '[', '<']) {
        return Err("contains a path, list or markup character".into());
    }
    if t.contains("://") || t.contains(".rs") || t.contains(".md") || t.contains("--") {
        return Err("contains a path, URL or tool flag".into());
    }
    let words = t.split_whitespace().count();
    if words > MAX_SPOKEN_WORDS {
        return Err(format!(
            "{words} words; one breath is at most {MAX_SPOKEN_WORDS}"
        ));
    }
    if !t.is_ascii() {
        return Err("non-ASCII characters".into());
    }
    Ok(())
}

/// Small numbers as words, so the voice reads them naturally.
pub fn num(n: usize) -> String {
    const W: [&str; 21] = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
        "twenty",
    ];
    W.get(n).map_or_else(|| n.to_string(), |w| (*w).to_string())
}

/// "a", "a and b", "a, b and c".
pub fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [a] => a.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_rule() {
        assert!(spoken_ok("Hermes is not ready yet, because the model credit is missing.").is_ok());
        assert!(spoken_ok("Two things. Then more.").is_err());
        assert!(spoken_ok("See docs/hermes-core.md for detail.").is_err());
        assert!(spoken_ok("Run hermes --help now.").is_err());
        assert!(spoken_ok("no stop").is_err());
        let long = format!("{}.", vec!["word"; 40].join(" "));
        assert!(spoken_ok(&long).is_err());
    }

    #[test]
    fn words() {
        assert_eq!(num(7), "seven");
        assert_eq!(num(42), "42");
        assert_eq!(
            and_list(&["a".into(), "b".into(), "c".into()]),
            "a, b and c"
        );
    }
}
