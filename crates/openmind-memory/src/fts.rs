// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio
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
    "的", "了", "在", "是", "我", "有", "和", "就", "不", "人", "都", "一", "一个", "上", "也",
    "很", "到", "说", "要", "去", "你", "会", "着", "没有", "看", "好", "自己", "这", "他", "她",
    "它", "们", "那", "吗", "吧", "呢", "啊", "呀", "哦", "嗯",
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
        let q = build_fts_query("旅行计划API").unwrap();
        assert!(
            q.contains("\"旅行\"")
                && q.contains("\"计划\"")
                && q.contains("\"API\"")
                && q.contains("\"旅行计划\"")
        );
        assert_eq!(build_fts_query("的"), None);
        assert_eq!(tokenize_for_fts("plain text"), "plain text");
        assert!(tokenize_for_fts("人工智能").starts_with("人工智能 人工 工智 智能"));
    }
    #[test]
    fn score() {
        assert!((bm25_rank_to_score(-1.0) - 0.5).abs() < 1e-12);
        assert!((bm25_rank_to_score(1.0) - 0.5).abs() < 1e-12);
    }
}
