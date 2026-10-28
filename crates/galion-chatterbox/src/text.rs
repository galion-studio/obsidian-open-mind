// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Chatterbox English text front end: the punctuation clean-up applied before
//! tokenising (`punc_norm`) and the character-level BPE of `tokenizer.json`
//! (Hugging Face tokenizers format: added tokens, a `Whitespace`
//! pre-tokenizer, merges ranked by order), implemented here directly.

use std::collections::HashMap;

/// Text clean-up from the reference implementation: capitalise the first
/// letter, collapse spaces, replace uncommon punctuation, end with a stop.
pub fn punc_norm(text: &str) -> String {
    if text.is_empty() {
        return "You need to add some text for me to talk.".into();
    }
    let mut t = String::with_capacity(text.len() + 1);
    let mut chars = text.chars();
    if let Some(c) = chars.next() {
        if c.is_lowercase() {
            t.extend(c.to_uppercase());
        } else {
            t.push(c);
        }
    }
    t.extend(chars);
    let mut t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    for (a, b) in [
        ("...", ", "),
        ("\u{2026}", ", "),
        (":", ","),
        (" - ", ", "),
        (";", ", "),
        ("\u{2014}", "-"),
        ("\u{2013}", "-"),
        (" ,", ","),
        ("\u{201c}", "\""),
        ("\u{201d}", "\""),
        ("\u{2018}", "'"),
        ("\u{2019}", "'"),
    ] {
        t = t.replace(a, b);
    }
    let t = t.trim_end_matches(' ').to_string();
    if [".", "!", "?", "-", ","].iter().any(|p| t.ends_with(p)) {
        t
    } else {
        t + "."
    }
}

pub struct EnTokenizer {
    vocab: HashMap<String, u32>,
    ranks: HashMap<(String, String), usize>,
    /// Added tokens, longest first, matched literally before pre-tokenising.
    added: Vec<(String, u32)>,
    unk: u32,
}

pub const SOT: u32 = 255;
pub const EOT: u32 = 0;

impl EnTokenizer {
    pub fn from_json(json: &str) -> crate::Result<Self> {
        let v: serde_json::Value = serde_json::from_str(json).map_err(crate::e)?;
        let model = &v["model"];
        if model["type"].as_str() != Some("BPE") {
            return Err("tokenizer.json: model type is not BPE".into());
        }
        let vocab: HashMap<String, u32> = model["vocab"]
            .as_object()
            .ok_or("tokenizer.json: no vocab")?
            .iter()
            .filter_map(|(k, id)| id.as_u64().map(|i| (k.clone(), i as u32)))
            .collect();
        let mut ranks = HashMap::new();
        for (i, m) in model["merges"]
            .as_array()
            .ok_or("tokenizer.json: no merges")?
            .iter()
            .enumerate()
        {
            let pair = match m {
                serde_json::Value::String(s) => {
                    let mut it = s.splitn(2, ' ');
                    (
                        it.next().unwrap_or("").to_string(),
                        it.next().unwrap_or("").to_string(),
                    )
                }
                serde_json::Value::Array(a) if a.len() == 2 => (
                    a[0].as_str().unwrap_or("").to_string(),
                    a[1].as_str().unwrap_or("").to_string(),
                ),
                _ => continue,
            };
            ranks.insert(pair, i);
        }
        let mut added: Vec<(String, u32)> = v["added_tokens"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| Some((t["content"].as_str()?.to_string(), t["id"].as_u64()? as u32)))
            .collect();
        added.sort_by_key(|(s, _)| std::cmp::Reverse(s.len()));
        let unk = model["unk_token"]
            .as_str()
            .and_then(|u| vocab.get(u).copied())
            .unwrap_or(1);
        for need in ["[START]", "[STOP]"] {
            if !vocab.contains_key(need) && !added.iter().any(|(s, _)| s == need) {
                return Err(format!("tokenizer.json: missing {need}"));
            }
        }
        Ok(EnTokenizer {
            vocab,
            ranks,
            added,
            unk,
        })
    }

    /// `EnTokenizer.encode`: spaces become `[SPACE]`, then tokenise.
    pub fn encode(&self, text: &str) -> Vec<u32> {
        let text = text.replace(' ', "[SPACE]");
        let mut out = Vec::new();
        let mut rest = text.as_str();
        while !rest.is_empty() {
            // Earliest added-token match (longest first at the same offset).
            let mut best: Option<(usize, &str, u32)> = None;
            for (s, id) in &self.added {
                if let Some(p) = rest.find(s.as_str()) {
                    if best.is_none_or(|(bp, bs, _)| p < bp || (p == bp && s.len() > bs.len())) {
                        best = Some((p, s.as_str(), *id));
                    }
                }
            }
            match best {
                Some((p, s, id)) => {
                    self.encode_plain(&rest[..p], &mut out);
                    out.push(id);
                    rest = &rest[p + s.len()..];
                }
                None => {
                    self.encode_plain(rest, &mut out);
                    rest = "";
                }
            }
        }
        out
    }

    /// Whitespace pre-tokenizer (`\w+|[^\w\s]+`) then BPE per piece.
    fn encode_plain(&self, s: &str, out: &mut Vec<u32>) {
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let mut piece = String::new();
        let mut kind = 0u8; // 1 word, 2 punctuation
        for c in s.chars() {
            let k = if c.is_whitespace() {
                0
            } else if is_word(c) {
                1
            } else {
                2
            };
            if k != kind && !piece.is_empty() {
                self.bpe(&piece, out);
                piece.clear();
            }
            kind = k;
            if k != 0 {
                piece.push(c);
            }
        }
        if !piece.is_empty() {
            self.bpe(&piece, out);
        }
    }

    fn bpe(&self, word: &str, out: &mut Vec<u32>) {
        let mut parts: Vec<String> = word.chars().map(String::from).collect();
        loop {
            let mut best: Option<(usize, usize)> = None;
            for i in 0..parts.len().saturating_sub(1) {
                if let Some(&r) = self.ranks.get(&(parts[i].clone(), parts[i + 1].clone())) {
                    if best.is_none_or(|(br, _)| r < br) {
                        best = Some((r, i));
                    }
                }
            }
            let Some((_, i)) = best else { break };
            let merged = format!("{}{}", parts[i], parts[i + 1]);
            parts[i] = merged;
            parts.remove(i + 1);
        }
        // fuse_unk is false in the shipped tokenizer: one [UNK] per unknown symbol.
        for p in parts {
            out.push(self.vocab.get(&p).copied().unwrap_or(self.unk));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punc_norm_matches_reference_rules() {
        assert_eq!(punc_norm("hello  world"), "Hello world.");
        // Like the reference, spaces are collapsed before the replacements.
        assert_eq!(punc_norm("Wait... what: yes; no"), "Wait,  what, yes,  no.");
        assert_eq!(
            punc_norm("\u{201c}Hi\u{201d} she said!"),
            "\"Hi\" she said!"
        );
        assert_eq!(punc_norm(""), "You need to add some text for me to talk.");
        assert_eq!(punc_norm("ok,"), "Ok,");
    }

    #[test]
    fn bpe_with_added_tokens() {
        let j = r#"{"added_tokens":[{"id":0,"content":"[STOP]"},{"id":2,"content":"[SPACE]"},{"id":255,"content":"[START]"}],
          "model":{"type":"BPE","unk_token":"[UNK]","vocab":{"[STOP]":0,"[UNK]":1,"[SPACE]":2,"h":3,"i":4,"hi":5,".":6,"H":7,"[START]":255},
          "merges":["h i"]}}"#;
        let t = EnTokenizer::from_json(j).unwrap();
        assert_eq!(t.encode("hi hi."), vec![5, 2, 5, 6]);
        assert_eq!(t.encode("Hi"), vec![7, 4]);
        assert_eq!(t.encode("hz"), vec![3, 1]);
    }
}
