// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! FTS5 query building and write-side tokenisation.
//!
//! The Node gateway uses `@node-rs/jieba` (a native C++/Rust addon with a
//! ~5 MB dictionary) when present and a `[\p{L}\p{N}_]+` split otherwise.
//! We stay dictionary-free so the binary is small and works offline on any
//! CPU: Latin/Cyrillic/etc. words split exactly like the fallback (and like
//! jieba, which also splits non-CJK text on word boundaries), and runs of CJK
//! ideographs expand to overlapping 2- and 3-grams plus the whole run. Those
//! n-grams hit jieba's 2-4 character words already indexed by Node, and our
//! own write side indexes the same n-grams. See docs/ws-memory.md.

/// Common Chinese stop words (same list as `ZH_STOP_WORDS`).
const ZH_STOP_WORDS: [&str; 40] = [
    "\u{7684}",
    "\u{4E86}",
    "\u{5728}",
    "\u{662F}",
    "\u{6211}",
    "\u{6709}",
    "\u{548C}",
    "\u{5C31}",
    "\u{4E0D}",
    "\u{4EBA}",
    "\u{90FD}",
    "\u{4E00}",
    "\u{4E00}\u{4E2A}",
    "\u{4E0A}",
    "\u{4E5F}",
    "\u{5F88}",
    "\u{5230}",
    "\u{8BF4}",
    "\u{8981}",
    "\u{53BB}",
    "\u{4F60}",
    "\u{4F1A}",
    "\u{7740}",
    "\u{6CA1}\u{6709}",
    "\u{770B}",
    "\u{597D}",
    "\u{81EA}\u{5DF1}",
    "\u{8FD9}",
    "\u{4ED6}",
    "\u{5979}",
    "\u{5B83}",
    "\u{4EEC}",
    "\u{90A3}",
    "\u{5417}",
    "\u{5427}",
    "\u{5462}",
    "\u{554A}",
    "\u{5440}",
    "\u{54E6}",
    "\u{55EF}",
];

/// CJK ideographs, kana and hangul: scripts written without spaces.
pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF |
        0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Split into word runs; a run never mixes CJK and non-CJK characters.
fn runs(raw: &str) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut cur = String::new();
    let mut cur_cjk = false;
    for c in raw.chars() {
        if is_word_char(c) {
            let cjk = is_cjk(c);
            if !cur.is_empty() && cjk != cur_cjk {
                out.push((std::mem::take(&mut cur), cur_cjk));
            }
            cur_cjk = cjk;
            cur.push(c);
        } else if !cur.is_empty() {
            out.push((std::mem::take(&mut cur), cur_cjk));
        }
    }
    if !cur.is_empty() {
        out.push((cur, cur_cjk));
    }
    out
}

fn ngrams(run: &str, n: usize, out: &mut Vec<String>) {
    let cs: Vec<char> = run.chars().collect();
    if cs.len() < n {
        return;
    }
    for w in cs.windows(n) {
        out.push(w.iter().collect());
    }
}

const MAX_QUERY_TERMS: usize = 64;

/// Port of `buildFtsQuery`: OR-joined quoted terms, or None when no tokens.
pub fn build_fts_query(raw: &str) -> Option<String> {
    let mut tokens: Vec<String> = Vec::new();
    for (run, cjk) in runs(raw) {
        if !cjk {
            tokens.push(run);
            continue;
        }
        let n = run.chars().count();
        if n <= 1 {
            tokens.push(run);
            continue;
        }
        tokens.push(run.clone());
        ngrams(&run, 2, &mut tokens);
        ngrams(&run, 3, &mut tokens);
    }
    let mut seen = std::collections::HashSet::new();
    let tokens: Vec<String> = tokens
        .into_iter()
        .filter(|t| !ZH_STOP_WORDS.contains(&t.as_str()))
        .filter(|t| seen.insert(t.clone()))
        .take(MAX_QUERY_TERMS)
        .collect();
    if tokens.is_empty() {
        return None;
    }
    Some(
        tokens
            .iter()
            .map(|t| format!("\"{}\"", t.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

/// Write-side text for the indexed FTS column (`content` / `message_text`).
/// Non-CJK text is kept verbatim (unicode61 splits it); CJK runs gain their
/// 2- and 3-grams so CJK queries match without a dictionary.
pub fn tokenize_for_fts(raw: &str) -> String {
    let extra: Vec<String> = runs(raw)
        .into_iter()
        .filter(|(r, cjk)| *cjk && r.chars().count() > 1)
        .flat_map(|(r, _)| {
            let mut v = Vec::new();
            ngrams(&r, 2, &mut v);
            ngrams(&r, 3, &mut v);
            v
        })
        .collect();
    if extra.is_empty() {
        raw.to_string()
    } else {
        format!("{raw} {}", extra.join(" "))
    }
}

/// Port of `bm25RankToScore`.
pub fn bm25_rank_to_score(rank: f64) -> f64 {
    if !rank.is_finite() {
        return 1.0 / (1.0 + 999.0);
    }
    if rank < 0.0 {
        let r = -rank;
        return r / (1.0 + r);
    }
    1.0 / (1.0 + rank)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn latin_query_matches_fallback() {
        assert_eq!(
            build_fts_query("Hello, world_1 \"x\"").as_deref(),
            Some("\"Hello\" OR \"world_1\" OR \"x\"")
        );
        assert_eq!(build_fts_query("  ,,, "), None);
        assert_eq!(
            build_fts_query("Šibenik čevapi").as_deref(),
            Some("\"Šibenik\" OR \"čevapi\"")
        );
    }
    #[test]
    fn cjk_ngrams() {
        let q = build_fts_query("\u{65C5}\u{884C}\u{8BA1}\u{5212}API").unwrap();
        assert!(
            q.contains("\"\u{65C5}\u{884C}\"")
                && q.contains("\"\u{8BA1}\u{5212}\"")
                && q.contains("\"API\"")
                && q.contains("\"\u{65C5}\u{884C}\u{8BA1}\u{5212}\"")
        );
        assert_eq!(build_fts_query("\u{7684}"), None);
        assert_eq!(tokenize_for_fts("plain text"), "plain text");
        assert!(tokenize_for_fts("\u{4EBA}\u{5DE5}\u{667A}\u{80FD}").starts_with("\u{4EBA}\u{5DE5}\u{667A}\u{80FD} \u{4EBA}\u{5DE5} \u{5DE5}\u{667A} \u{667A}\u{80FD}"));
    }
    #[test]
    fn score() {
        assert!((bm25_rank_to_score(-1.0) - 0.5).abs() < 1e-12);
        assert!((bm25_rank_to_score(1.0) - 0.5).abs() < 1e-12);
    }
}
