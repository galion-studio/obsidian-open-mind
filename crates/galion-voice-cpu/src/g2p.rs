// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Licence-clean English grapheme-to-phoneme (G2P) for neural TTS.
//!
//! Produces espeak-ng style US English IPA (the phoneme inventory KittenTTS
//! and other StyleTTS2-family models were trained on) without any GPL code:
//!
//! * a pronouncing dictionary in CMUdict format (ARPAbet with stress digits).
//!   The CMU Pronouncing Dictionary is BSD-2-Clause style licensed
//!   (Copyright (C) 1993-2015 Carnegie Mellon University, see
//!   `NOTICE-cmudict.txt`). The dictionary file is loaded at run time and is
//!   not in git;
//! * our own letter-to-sound rules for out-of-vocabulary words, acronym
//!   spelling and a small number expander;
//! * an ARPAbet -> IPA mapping with the espeak-ng en-us conventions that
//!   matter to the model: the stress mark sits right before the stressed
//!   vowel (`həlˈoʊ`), long vowels carry `ː`, unstressed ER is `ɚ`, `t`/`d`
//!   between a vowel and an unstressed vowel flap to `ɾ`, and function words
//!   are unstressed (`ɐ`, `ðə`, `tə`).
//!
//! Then [`kitten_ids`] maps the IPA string to KittenTTS token ids the way
//! KittenTTS 0.8's `TextCleaner` does (punctuation split off by spaces, start
//! token 0, end tokens 10 and 0).

use std::collections::HashMap;

/// The 39 ARPAbet phonemes of CMUdict.
const ARPA: [&str; 39] = [
    "AA", "AE", "AH", "AO", "AW", "AY", "B", "CH", "D", "DH", "EH", "ER", "EY", "F", "G", "HH",
    "IH", "IY", "JH", "K", "L", "M", "N", "NG", "OW", "OY", "P", "R", "S", "SH", "T", "TH", "UH",
    "UW", "V", "W", "Y", "Z", "ZH",
];

/// One phone: ARPAbet index plus stress (0, 1, 2 for vowels, `None` for
/// consonants). Packed into a byte for a compact dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Phone(u8);

const NO_STRESS: u8 = 3;

impl Phone {
    fn new(code: &str, stress: Option<u8>) -> Option<Phone> {
        let i = ARPA.iter().position(|a| *a == code)? as u8;
        Some(Phone(i | (stress.unwrap_or(NO_STRESS).min(NO_STRESS) << 6)))
    }
    fn parse(tok: &str) -> Option<Phone> {
        let (code, digit) = match tok.as_bytes().last()? {
            d @ b'0'..=b'2' => (&tok[..tok.len() - 1], Some(d - b'0')),
            _ => (tok, None),
        };
        Phone::new(code, digit)
    }
    pub fn code(self) -> &'static str {
        ARPA[(self.0 & 63) as usize]
    }
    pub fn stress(self) -> Option<u8> {
        match self.0 >> 6 {
            NO_STRESS => None,
            s => Some(s),
        }
    }
    pub fn is_vowel(self) -> bool {
        self.stress().is_some()
    }
    fn with_stress(self, s: u8) -> Phone {
        Phone((self.0 & 63) | (s << 6))
    }
}

fn p(s: &str) -> Vec<Phone> {
    s.split_whitespace().filter_map(Phone::parse).collect()
}

/// A CMUdict-format pronouncing dictionary (first pronunciation per word).
#[derive(Default)]
pub struct Lexicon {
    words: HashMap<Box<str>, Box<[Phone]>>,
}

impl Lexicon {
    /// Parse CMUdict text (`word PH1 PH2 ...`, `word(2) ...` variants and
    /// `# comments` are skipped).
    pub fn parse(text: &str) -> Lexicon {
        let mut words = HashMap::with_capacity(140_000);
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let mut it = line.split_whitespace();
            let Some(word) = it.next() else { continue };
            if word.ends_with(')') || line.starts_with(";;;") {
                continue;
            }
            let phones: Vec<Phone> = it.filter_map(Phone::parse).collect();
            if !phones.is_empty() {
                words
                    .entry(word.to_lowercase().into_boxed_str())
                    .or_insert_with(|| phones.into_boxed_slice());
            }
        }
        Lexicon { words }
    }

    pub fn load(path: &std::path::Path) -> Result<Lexicon, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Lexicon::parse(&text))
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }
    pub fn get(&self, word: &str) -> Option<&[Phone]> {
        self.words.get(word).map(|b| &b[..])
    }
}

/// Text -> espeak-style IPA.
pub struct Phonemizer {
    lex: Lexicon,
    /// Words looked up in the lexicon / spelled / guessed by rules (last call).
    pub stats: G2pStats,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct G2pStats {
    pub dict: usize,
    pub spelled: usize,
    pub rules: usize,
}

/// Unstressed (in espeak's sense) English function words.
const FUNCTION_WORDS: &[&str] = &[
    "a", "an", "the", "to", "of", "and", "or", "but", "in", "on", "at", "as", "for", "from",
    "with", "by", "is", "am", "are", "was", "were", "be", "been", "it", "its", "this", "that",
    "than", "then", "he", "she", "we", "they", "you", "i", "me", "him", "her", "us", "them", "my",
    "your", "his", "our", "their", "can", "could", "will", "would", "shall", "should", "has",
    "have", "had", "do", "does", "did", "if", "so", "into", "onto", "upon", "some", "there",
];

/// Fixed espeak-like forms for the commonest reduced words.
fn fixed_form(word: &str, next_starts_with_vowel: bool) -> Option<&'static str> {
    Some(match word {
        "a" => "ɐ",
        "an" => "ɐn",
        "the" if next_starts_with_vowel => "ði",
        "the" => "ðə",
        "to" => "tə",
        "of" => "ʌv",
        "and" => "ænd",
        "or" => "ɔːɹ",
        "for" => "fɔːɹ",
        "from" => "fɹʌm",
        "was" => "wʌz",
        "were" => "wɜː",
        "are" => "ɑːɹ",
        "your" => "jʊɹ",
        _ => return None,
    })
}

impl Phonemizer {
    pub fn new(lex: Lexicon) -> Phonemizer {
        Phonemizer {
            lex,
            stats: G2pStats::default(),
        }
    }

    /// Add or override one word, in CMUdict ARPAbet (`"G AE1 L IY0 AH0 N"`).
    pub fn add_word(&mut self, word: &str, arpabet: &str) {
        let ph = p(arpabet);
        if !ph.is_empty() {
            self.lex
                .words
                .insert(word.to_lowercase().into_boxed_str(), ph.into_boxed_slice());
        }
    }

    pub fn lexicon(&self) -> &Lexicon {
        &self.lex
    }

    /// Whole text to IPA. Words are separated by one space; punctuation
    /// (`, . ! ? ; : — …`) is kept as its own space-separated token, which is
    /// what KittenTTS's tokenizer turns espeak output into.
    pub fn phonemize(&mut self, text: &str) -> String {
        self.stats = G2pStats::default();
        let tokens = tokenize(text);
        let mut out: Vec<String> = Vec::with_capacity(tokens.len());
        for (i, tok) in tokens.iter().enumerate() {
            match tok {
                Tok::Punct(c) => out.push(c.to_string()),
                Tok::Word(w, caps) => {
                    let next_vowel = tokens[i + 1..]
                        .iter()
                        .find_map(|t| match t {
                            Tok::Word(n, _) => Some(n),
                            Tok::Punct(_) => None,
                        })
                        .is_some_and(|n| n.starts_with(['a', 'e', 'i', 'o', 'u']));
                    if let Some(s) = self.word_ipa(w, *caps, next_vowel) {
                        out.push(s);
                    }
                }
            }
        }
        out.join(" ")
    }

    fn word_ipa(&mut self, w: &str, all_caps: bool, next_vowel: bool) -> Option<String> {
        if w.is_empty() {
            return None;
        }
        if let Some(f) = fixed_form(w, next_vowel) {
            self.stats.dict += 1;
            return Some(f.to_string());
        }
        let function = FUNCTION_WORDS.contains(&w);
        // Acronyms written in capitals and not a dictionary word: spell out.
        let acronym = all_caps && w.len() >= 2 && w.len() <= 5 && self.lex.get(w).is_none();
        let phones = if acronym {
            self.stats.spelled += 1;
            spell(w)
        } else if let Some(ph) = self.lookup(w) {
            self.stats.dict += 1;
            ph
        } else if w.len() <= 3 && !w.contains(['a', 'e', 'i', 'o', 'u', 'y']) {
            self.stats.spelled += 1;
            spell(w)
        } else {
            self.stats.rules += 1;
            letter_to_sound(w)
        };
        let s = to_ipa(&phones, function);
        (!s.is_empty()).then_some(s)
    }

    /// Dictionary, then simple morphology (possessive, hyphen/compound split).
    fn lookup(&self, w: &str) -> Option<Vec<Phone>> {
        if let Some(ph) = self.lex.get(w) {
            return Some(ph.to_vec());
        }
        if let Some(base) = w.strip_suffix("'s") {
            let mut ph = self.lookup(base)?;
            let last = *ph.last()?;
            let s = match last.code() {
                "S" | "Z" | "SH" | "ZH" | "CH" | "JH" => p("IH0 Z"),
                "P" | "T" | "K" | "F" | "TH" => p("S"),
                _ => p("Z"),
            };
            ph.extend(s);
            return Some(ph);
        }
        if w.contains('-') {
            let mut all = Vec::new();
            for part in w.split('-').filter(|x| !x.is_empty()) {
                all.extend(self.lookup(part).unwrap_or_else(|| letter_to_sound(part)));
            }
            return (!all.is_empty()).then_some(all);
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    /// Lowercased word, and whether it was written in capitals.
    Word(String, bool),
    Punct(char),
}

fn tokenize(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut caps = true;
    let flush = |cur: &mut String, caps: &mut bool, out: &mut Vec<Tok>| {
        let w = cur.trim_matches(['\'', '-']).to_string();
        if !w.is_empty() {
            if w.chars().all(|c| c.is_ascii_digit()) {
                for n in number_words(&w) {
                    out.push(Tok::Word(n.to_string(), false));
                }
            } else {
                let c = *caps && w.chars().filter(|c| c.is_alphabetic()).count() >= 2;
                out.push(Tok::Word(w.to_lowercase(), c));
            }
        }
        cur.clear();
        *caps = true;
    };
    for ch in text.chars() {
        let ch = match ch {
            '\u{2019}' | '\u{2018}' => '\'',
            c => c,
        };
        if ch.is_alphanumeric() || ((ch == '\'' || ch == '-') && !cur.is_empty()) {
            // Split letter/digit boundaries ("mp3" -> "mp" "3").
            let boundary = cur.chars().last().is_some_and(|l| {
                l.is_ascii_digit() != ch.is_ascii_digit()
                    && ch.is_alphanumeric()
                    && l.is_alphanumeric()
            });
            if boundary {
                flush(&mut cur, &mut caps, &mut out);
            }
            if ch.is_lowercase() {
                caps = false;
            }
            cur.push(ch);
            continue;
        }
        flush(&mut cur, &mut caps, &mut out);
        match ch {
            ',' | '.' | '!' | '?' | ';' | ':' | '—' | '…' => out.push(Tok::Punct(ch)),
            '&' => out.push(Tok::Word("and".into(), false)),
            '%' => out.push(Tok::Word("percent".into(), false)),
            '+' => out.push(Tok::Word("plus".into(), false)),
            '=' => out.push(Tok::Word("equals".into(), false)),
            '@' => out.push(Tok::Word("at".into(), false)),
            _ => {}
        }
    }
    flush(&mut cur, &mut caps, &mut out);
    out
}

/// Cardinal number words for up to 12 digits; longer strings are read digit
/// by digit.
pub fn number_words(digits: &str) -> Vec<&'static str> {
    const ONES: [&str; 20] = [
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
    ];
    const TENS: [&str; 10] = [
        "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];
    let Ok(n) = digits.parse::<u64>() else {
        return Vec::new();
    };
    if digits.len() > 12 || (digits.len() > 1 && digits.starts_with('0')) {
        return digits.bytes().map(|b| ONES[(b - b'0') as usize]).collect();
    }
    fn below_1000(n: u64, out: &mut Vec<&'static str>) {
        if n >= 100 {
            out.push(ONES[(n / 100) as usize]);
            out.push("hundred");
        }
        let r = n % 100;
        if r >= 20 {
            out.push(TENS[(r / 10) as usize]);
            if !r.is_multiple_of(10) {
                out.push(ONES[(r % 10) as usize]);
            }
        } else if r > 0 || n == 0 {
            out.push(ONES[r as usize]);
        }
    }
    let mut out = Vec::new();
    if n == 0 {
        return vec!["zero"];
    }
    for (scale, name) in [
        (1_000_000_000, "billion"),
        (1_000_000, "million"),
        (1_000, "thousand"),
    ] {
        if n / scale % 1000 > 0 {
            below_1000(n / scale % 1000, &mut out);
            out.push(name);
        }
    }
    if n % 1000 > 0 {
        below_1000(n % 1000, &mut out);
    }
    out
}

/// Letter names, for acronyms ("GPU" -> G P U).
fn spell(w: &str) -> Vec<Phone> {
    let mut out = Vec::new();
    let letters: Vec<char> = w.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    for (i, c) in letters.iter().enumerate() {
        let name = match c {
            'a' => "EY1",
            'b' => "B IY1",
            'c' => "S IY1",
            'd' => "D IY1",
            'e' => "IY1",
            'f' => "EH1 F",
            'g' => "JH IY1",
            'h' => "EY1 CH",
            'i' => "AY1",
            'j' => "JH EY1",
            'k' => "K EY1",
            'l' => "EH1 L",
            'm' => "EH1 M",
            'n' => "EH1 N",
            'o' => "OW1",
            'p' => "P IY1",
            'q' => "K Y UW1",
            'r' => "AA1 R",
            's' => "EH1 S",
            't' => "T IY1",
            'u' => "Y UW1",
            'v' => "V IY1",
            'w' => "D AH1 B AH0 L Y UW0",
            'x' => "EH1 K S",
            'y' => "W AY1",
            _ => "Z IY1",
        };
        // espeak: secondary stress on every letter but the last.
        let last = i + 1 == letters.len();
        out.extend(p(name).into_iter().map(|ph| match ph.stress() {
            Some(1) if !last => ph.with_stress(2),
            _ => ph,
        }));
    }
    out
}

fn is_v(c: u8) -> bool {
    matches!(c, b'a' | b'e' | b'i' | b'o' | b'u')
}

/// Rule-based guess for words not in the dictionary. Our own simplified
/// English rules (digraphs, magic e, soft c/g, open vs closed syllables);
/// first syllable stressed, unstressed short vowels reduced to schwa.
pub fn letter_to_sound(word: &str) -> Vec<Phone> {
    let w: Vec<u8> = word
        .bytes()
        .filter(|b| b.is_ascii_alphabetic())
        .map(|b| b.to_ascii_lowercase())
        .collect();
    let n = w.len();
    let at = |i: usize| -> u8 { w.get(i).copied().unwrap_or(0) };
    let starts = |i: usize, s: &str| w[i..].starts_with(s.as_bytes());
    let mut out: Vec<Phone> = Vec::new();
    let mut i = 0;
    // Final silent e (not in "be", "the"-like short words).
    let silent_e = n > 2 && at(n - 1) == b'e' && !is_v(at(n - 2)) && at(n - 2) != b'l';
    while i < n {
        let c = at(i);
        // (pattern, phones, condition)
        let multi: &[(&str, &str, bool)] = &[
            ("tion", "SH AH0 N", true),
            (
                "sion",
                if i > 0 && is_v(at(i - 1)) {
                    "ZH AH0 N"
                } else {
                    "SH AH0 N"
                },
                true,
            ),
            ("ture", "CH ER0", true),
            ("ough", "AO1", true),
            ("augh", "AO1", true),
            ("eigh", "EY1", true),
            ("igh", "AY1", true),
            ("tch", "CH", true),
            ("dge", "JH", true),
            ("sch", "S K", true),
            ("ph", "F", true),
            ("sh", "SH", true),
            ("ch", "CH", true),
            ("th", "TH", true),
            ("wh", "W", true),
            ("ck", "K", true),
            ("ng", "NG", i + 2 >= n || !is_v(at(i + 2))),
            ("qu", "K W", true),
            ("kn", "N", i == 0),
            ("wr", "R", i == 0),
            ("gn", "N", i == 0 || i + 2 == n),
            ("mb", "M", i + 2 == n),
            ("gh", "", i > 0),
            ("ee", "IY1", true),
            ("ea", "IY1", true),
            ("oo", "UW1", true),
            ("ou", "AW1", true),
            ("ow", if i + 2 == n { "OW1" } else { "AW1" }, true),
            ("oi", "OY1", true),
            ("oy", "OY1", true),
            ("ai", "EY1", true),
            ("ay", "EY1", true),
            ("au", "AO1", true),
            ("aw", "AO1", true),
            ("ey", if i + 2 == n { "IY0" } else { "EY1" }, true),
            ("ei", "EY1", true),
            ("ie", if i + 2 == n && n <= 4 { "AY1" } else { "IY1" }, true),
            ("ue", "UW1", i + 2 == n),
            ("ui", "UW1", true),
            ("oa", "OW1", true),
            ("ew", "UW1", true),
            ("eu", "UW1", true),
            ("ar", "AA1 R", true),
            ("er", "ER1", true),
            ("ir", "ER1", true),
            ("ur", "ER1", true),
            ("or", "AO1 R", true),
            ("le", "AH0 L", i + 2 == n && i > 0),
        ];
        if let Some((pat, ph, _)) = multi.iter().find(|(pat, _, ok)| *ok && starts(i, pat)) {
            out.extend(p(ph));
            i += pat.len();
            continue;
        }
        if c == b'e' && i + 1 == n && silent_e {
            i += 1;
            continue;
        }
        // Doubled consonant: one sound.
        if i > 0 && c == at(i - 1) && !is_v(c) && c != b'c' {
            i += 1;
            continue;
        }
        let next = at(i + 1);
        let ph: &str = match c {
            b'a' | b'e' | b'i' | b'o' | b'u' => {
                // Long if open syllable (V C V) or magic e (V C e$).
                let open = !is_v(next)
                    && next != 0
                    && next != b'x'
                    && (is_v(at(i + 2)) || at(i + 2) == b'y')
                    && !(at(i + 2) == b'e' && i + 3 < n && at(i + 3) == b'r');
                let magic = silent_e && i + 3 == n && !is_v(next);
                let long = (open && i + 3 <= n && (magic || i + 3 < n || at(i + 2) == b'e'))
                    || magic
                    || (next == 0 && n <= 3 && c != b'e');
                if matches!(c, b'i' | b'e') && is_v(next) {
                    out.extend(p("IY0"));
                    i += 1;
                    continue;
                }
                match (c, long) {
                    (b'a', true) => "EY1",
                    (b'a', false) => "AE1",
                    (b'e', true) => "IY1",
                    (b'e', false) => "EH1",
                    (b'i', true) => "AY1",
                    (b'i', false) => "IH1",
                    (b'o', true) => "OW1",
                    (b'o', false) => "AA1",
                    (b'u', true) => "UW1",
                    _ => "AH1",
                }
            }
            b'y' if i == 0 => "Y",
            b'y' if i + 1 == n => {
                if w.iter().take(n - 1).any(|&b| is_v(b)) {
                    "IY0"
                } else {
                    "AY1"
                }
            }
            b'y' => "IH1",
            b'c' => {
                if matches!(next, b'e' | b'i' | b'y') {
                    "S"
                } else if next == b'c' {
                    i += 1;
                    if matches!(at(i + 1), b'e' | b'i') {
                        "K S"
                    } else {
                        "K"
                    }
                } else {
                    "K"
                }
            }
            b'g' => {
                if matches!(next, b'e' | b'i' | b'y') && i > 0 {
                    "JH"
                } else {
                    "G"
                }
            }
            b'x' if i == 0 => "Z",
            b'x' => "K S",
            b's' if i > 0 && is_v(at(i - 1)) && (is_v(next) || next == b'm') => "Z",
            b's' if i + 1 == n && i > 0 && !matches!(at(i - 1), b'p' | b't' | b'k' | b'f') => "Z",
            b'j' => "JH",
            b'q' => "K",
            b'h' => "HH",
            b'b' => "B",
            b'd' => "D",
            b'f' => "F",
            b'k' => "K",
            b'l' => "L",
            b'm' => "M",
            b'n' => "N",
            b'p' => "P",
            b'r' => "R",
            b's' => "S",
            b't' => "T",
            b'v' => "V",
            b'w' => "W",
            b'z' => "Z",
            _ => "",
        };
        out.extend(p(ph));
        i += 1;
    }
    // Stress: first vowel primary, the rest unstressed (short ones reduced).
    let mut seen = false;
    for ph in out.iter_mut() {
        if !ph.is_vowel() {
            continue;
        }
        if !seen {
            *ph = ph.with_stress(1);
            seen = true;
        } else if ph.stress() == Some(1) {
            *ph = match ph.code() {
                "AE" | "EH" | "AA" | "AH" => Phone::new("AH", Some(0)).unwrap_or(*ph),
                "ER" => ph.with_stress(0),
                _ => ph.with_stress(if ph.code() == "IH" { 0 } else { 2 }),
            };
        }
    }
    out
}

/// ARPAbet to espeak-style en-us IPA. `function`: drop stress marks.
pub fn to_ipa(phones: &[Phone], function: bool) -> String {
    let mut s = String::new();
    for (i, &ph) in phones.iter().enumerate() {
        let stress = ph.stress();
        let prev = i.checked_sub(1).map(|j| phones[j]);
        let next = phones.get(i + 1).copied();
        if let (Some(st @ 1..=2), false) = (stress, function) {
            s.push(if st == 1 { 'ˈ' } else { 'ˌ' });
        }
        let stressed = matches!(stress, Some(1 | 2));
        let ipa = match ph.code() {
            "AA" => "ɑː",
            "AE" => "æ",
            "AH" if stressed => "ʌ",
            "AH" => "ə",
            "AO" => "ɔː",
            "AW" => "aʊ",
            "AY" => "aɪ",
            "EH" => "ɛ",
            "ER" if stressed => "ɜː",
            "ER" => "ɚ",
            "EY" => "eɪ",
            "IH" => "ɪ",
            "IY" if !stressed => "i",
            "IY" => "iː",
            "OW" => "oʊ",
            "OY" => "ɔɪ",
            "UH" => "ʊ",
            "UW" => "uː",
            "B" => "b",
            "CH" => "tʃ",
            "T" if prev.is_some_and(|p| p.is_vowel() || p.code() == "R")
                && next.is_some_and(|q| q.stress() == Some(0))
                && !prev.is_some_and(|p| p.code() == "R" && i < 2) =>
            {
                "ɾ"
            }
            "D" => "d",
            "DH" => "ð",
            "F" => "f",
            "G" => "ɡ",
            "HH" => "h",
            "JH" => "dʒ",
            "K" => "k",
            "L" => "l",
            "M" => "m",
            "N" => "n",
            "NG" => "ŋ",
            "P" => "p",
            "R" => "ɹ",
            "S" => "s",
            "SH" => "ʃ",
            "T" => "t",
            "TH" => "θ",
            "V" => "v",
            "W" => "w",
            "Y" => "j",
            "Z" => "z",
            _ => "ʒ",
        };
        s.push_str(ipa);
    }
    s
}

/// The KittenTTS 0.8 / StyleTTS2 symbol table, in order (index = token id):
/// pad, 16 punctuation marks incl. space, A-Z, a-z, then 109 IPA symbols.
/// Taken from KittenTTS `TextCleaner` (Apache-2.0), where the two curly
/// quotes are written as straight quotes.
const KITTEN_PUNCT: &str = ";:,.!?¡¿—…\"«»\"\" ";
const KITTEN_IPA: &str = "ɑɐɒæɓʙβɔɕçɗɖðʤəɘɚɛɜɝɞɟʄɡɠɢʛɦɧħɥʜɨɪʝɭɬɫɮʟɱɯɰŋɳɲɴøɵɸθœɶʘɹɺɾɻʀʁɽʂʃʈʧʉʊʋⱱʌɣɤʍχʎʏʑʐʒʔʡʕʢǀǁǂǃˈˌːˑʼʴʰʱʲʷˠˤ˞↓↑→↗↘'̩'ᵻ";

/// char -> token id, KittenTTS `TextCleaner` semantics (a symbol listed twice
/// keeps its last index).
pub fn kitten_symbol_table() -> HashMap<char, u32> {
    let mut m = HashMap::new();
    let all = std::iter::once('$')
        .chain(KITTEN_PUNCT.chars())
        .chain('A'..='Z')
        .chain('a'..='z')
        .chain(KITTEN_IPA.chars());
    for (i, c) in all.enumerate() {
        m.insert(c, i as u32);
    }
    m
}

/// IPA string to model token ids: punctuation split off with spaces (as
/// KittenTTS's `basic_english_tokenize` does), unknown characters dropped,
/// then `[0] + ids + [10, 0]`.
pub fn kitten_ids(ipa: &str, table: &HashMap<char, u32>) -> Vec<u32> {
    let mut spaced = String::with_capacity(ipa.len() + 8);
    let mut word = false;
    for c in ipa.chars() {
        // Python `\w`: letters (incl. IPA and modifier letters), digits, `_`.
        let is_w = c.is_alphanumeric() || c == '_';
        if c.is_whitespace() {
            word = false;
            continue;
        }
        if !spaced.is_empty() && (!is_w || !word) {
            spaced.push(' ');
        }
        spaced.push(c);
        word = is_w;
    }
    let mut ids = vec![0];
    ids.extend(spaced.chars().filter_map(|c| table.get(&c).copied()));
    ids.extend([10, 0]);
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    // A few CMUdict 0.7b lines (Copyright (C) 1993-2015 Carnegie Mellon
    // University, BSD-2-Clause style, see NOTICE-cmudict.txt).
    const MINI: &str = "\
a AH0
and AH0 N D
hello HH AH0 L OW1
hello(2) HH EH0 L OW1
world W ER1 L D
quality K W AA1 L AH0 T IY0
water W AO1 T ER0
this DH IH1 S
high HH AY1
model M AA1 D AH0 L
works W ER1 K S
without W IH0 TH AW1 T
city S IH1 T IY0
the DH AH0
mind M AY1 N D
open OW1 P AH0 N
made M EY1 D
by B AY1
aalborg AO1 L B AO0 R G # place, danish
";

    fn ph() -> Phonemizer {
        Phonemizer::new(Lexicon::parse(MINI))
    }

    #[test]
    fn lexicon_parses_and_skips_variants_and_comments() {
        let l = Lexicon::parse(MINI);
        assert_eq!(l.len(), 18);
        let h: Vec<_> = l.get("hello").unwrap().iter().map(|p| p.code()).collect();
        assert_eq!(h, ["HH", "AH", "L", "OW"]);
        assert_eq!(l.get("hello").unwrap()[3].stress(), Some(1));
        assert_eq!(l.get("aalborg").unwrap().len(), 6);
    }

    #[test]
    fn espeak_style_ipa_for_dictionary_words() {
        let mut g = ph();
        assert_eq!(g.phonemize("Hello, world."), "həlˈoʊ , wˈɜːld .");
        assert_eq!(g.phonemize("water"), "wˈɔːɾɚ");
        assert_eq!(g.phonemize("quality"), "kwˈɑːləɾi");
        assert_eq!(g.phonemize("city"), "sˈɪɾi");
        // Function words lose stress; "a" is ɐ; "the" before a vowel is ði.
        assert_eq!(g.phonemize("this model"), "ðɪs mˈɑːdəl");
        assert_eq!(
            g.phonemize("a model and the open mind"),
            "ɐ mˈɑːdəl ænd ði ˈoʊpən mˈaɪnd"
        );
        assert_eq!(g.stats.dict, 6);
    }

    #[test]
    fn acronyms_numbers_and_oov() {
        let mut g = ph();
        assert_eq!(g.phonemize("GPU"), "dʒˌiːpˌiːjˈuː");
        assert_eq!(g.stats.spelled, 1);
        assert_eq!(number_words("2026"), ["two", "thousand", "twenty", "six"]);
        assert_eq!(number_words("115"), ["one", "hundred", "fifteen"]);
        assert_eq!(number_words("0"), ["zero"]);
        assert_eq!(number_words("007"), ["zero", "zero", "seven"]);
        // OOV words go through the letter-to-sound rules.
        let mut g = ph();
        assert_eq!(g.phonemize("Galion"), "ɡˈeɪliən");
        assert_eq!(g.stats.rules, 1);
        // Names can be pinned in ARPAbet.
        g.add_word("galion", "G AE1 L IY0 AH0 N");
        assert_eq!(g.phonemize("Galion"), "ɡˈæliən");
        assert_eq!(g.phonemize("cake"), "kˈeɪk");
        assert_eq!(g.phonemize("ship"), "ʃˈɪp");
        assert_eq!(g.phonemize("nation"), "nˈeɪʃən");
        assert_eq!(g.phonemize("phone"), "fˈoʊn");
        assert_eq!(g.phonemize("knight"), "nˈaɪt");
    }

    #[test]
    fn kitten_ids_follow_the_text_cleaner() {
        let t = kitten_symbol_table();
        assert_eq!(t.len(), 175); // 178 slots; `"` is listed 3 times, `'` twice
        assert_eq!(t[&'$'], 0);
        assert_eq!(t[&';'], 1);
        assert_eq!(t[&'…'], 10);
        assert_eq!(t[&'"'], 15); // last of the three quote slots
        assert_eq!(t[&' '], 16);
        assert_eq!(t[&'A'], 17);
        assert_eq!(t[&'a'], 43);
        assert_eq!(t[&'ɑ'], 69);
        assert_eq!(t[&'ˈ'], 156);
        assert_eq!(t[&'ː'], 158);
        assert_eq!(t[&'ᵻ'], 177);
        let ids = kitten_ids("həlˈoʊ, wˈɜːld.", &t);
        // h ə l ˈ o ʊ ␠ , ␠ w ˈ ɜ ː l d ␠ .
        assert_eq!(
            ids,
            [0, 50, 83, 54, 156, 57, 135, 16, 3, 16, 65, 156, 87, 158, 54, 46, 16, 4, 10, 0]
        );
    }
}
