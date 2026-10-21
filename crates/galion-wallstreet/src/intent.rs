// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Intent layer of the compliance screen: catches reworded insider tips,
//! pump-and-dump asks and other manipulation that a fixed phrase list misses.
//!
//! Two parts, both deterministic and dependency-free:
//!
//! 1. [`Text::new`] de-obfuscates before anything is matched: zero-width and
//!    soft-hyphen removal, Cyrillic/Greek/full-width homoglyphs to Latin,
//!    Croatian and Latin diacritics stripped, leetspeak inside words
//!    (`1nsid3r` -> `insider`), letter-spaced words (`p.u.m.p`, `i n s i d e r`)
//!    rejoined, stretched letters squeezed (`puuuump`), emoji named
//!    (rocket, moon, money) and harmless idioms neutralised (`pump the brakes`,
//!    `friend at the gym`).
//! 2. [`detect`] looks for *combinations of concepts*, not single phrases:
//!    a private source plus a corporate event plus a trade; promotion plus a
//!    planned exit; a causative price move plus a trade; orders meant to be
//!    cancelled; trades with oneself; trading ahead of a client. A public-source
//!    marker ("on the earnings call", "press release", "newswire") clears the
//!    weak insider rule, never the strong one.
//!
//! [`educational`] recognises questions *about* the rules ("explain what a
//! pump and dump is"). They are allowed only when they name no symbol and carry
//! no operational cue ("for me", "how would I", "without getting caught").
//!
//! Patterns are written already normalised (lower case, no apostrophes);
//! a trailing `*` on a word is a prefix match. A unit test checks every pattern
//! survives [`Text::new`] unchanged so none of them can silently never match.

use crate::compliance::Category;

/// The normalised, tokenised form of one screened text.
#[derive(Debug, Clone)]
pub struct Text {
    pub norm: String,
    toks: Vec<String>,
    /// A ticker-like symbol is named (universe symbol, company name, $cashtag
    /// or an upper-case 2-5 letter token that is not a common acronym).
    pub names_symbol: bool,
}

const ZERO_WIDTH: &[char] = &[
    '\u{200b}', '\u{200c}', '\u{200d}', '\u{200e}', '\u{200f}', '\u{2060}', '\u{feff}', '\u{00ad}',
];

fn fold(c: char) -> Option<&'static str> {
    Some(match c {
        // Cyrillic look-alikes
        'а' => "a",
        'е' | 'ё' => "e",
        'о' => "o",
        'р' => "p",
        'с' => "c",
        'у' => "y",
        'х' => "x",
        'і' | 'ї' => "i",
        'ј' => "j",
        'ѕ' => "s",
        'к' => "k",
        'м' => "m",
        'ԁ' => "d",
        'һ' => "h",
        // Greek look-alikes
        'α' => "a",
        'ε' => "e",
        'ι' => "i",
        'κ' => "k",
        'ν' => "v",
        'ο' => "o",
        'ρ' => "p",
        'τ' => "t",
        'υ' => "u",
        // Croatian and common Latin diacritics
        'č' | 'ć' | 'ç' => "c",
        'š' => "s",
        'ž' => "z",
        'đ' => "d",
        'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => "a",
        'é' | 'è' | 'ë' | 'ê' => "e",
        'í' | 'ì' | 'ï' | 'î' => "i",
        'ó' | 'ò' | 'ö' | 'ô' | 'õ' => "o",
        'ú' | 'ù' | 'ü' | 'û' => "u",
        'ñ' => "n",
        'ß' => "ss",
        // emoji that carry meaning in pump posts
        '🚀' => " rocket ",
        '🌕' | '🌙' => " moon ",
        '📈' => " chart up ",
        '💰' | '🤑' | '💵' | '💸' => " money ",
        '💎' => " diamond ",
        '%' => " percent ",
        '&' => " and ",
        '+' => " and ",
        _ => return None,
    })
}

fn fullwidth(c: char) -> char {
    let u = c as u32;
    if (0xff01..=0xff5e).contains(&u) {
        char::from_u32(u - 0xfee0).unwrap_or(c)
    } else {
        c
    }
}

/// Re-join letters written one by one with a single separator
/// (`p.u.m.p`, `i n s i d e r`, `d-u-m-p`). Needs at least 3 letters.
fn rejoin_spaced(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let single = |k: usize| {
        c[k].is_alphabetic()
            && (k == 0 || !c[k - 1].is_alphanumeric())
            && (k + 1 >= c.len() || !c[k + 1].is_alphanumeric())
    };
    while i < c.len() {
        if single(i) && i + 2 < c.len() && !c[i + 1].is_alphanumeric() && c[i + 1] != '\n' {
            let sep = c[i + 1];
            let mut letters = vec![c[i]];
            let mut j = i;
            while j + 2 < c.len() && c[j + 1] == sep && single(j + 2) {
                letters.push(c[j + 2]);
                j += 2;
            }
            if letters.len() >= 3 {
                out.extend(letters);
                i = j + 1;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// Suffixes after a number that are units, not leetspeak.
const UNIT_SUFFIXES: &[&str] = &[
    "x", "k", "m", "b", "bn", "mm", "pm", "am", "st", "nd", "rd", "th", "h", "d", "w", "y", "yr",
    "yrs", "bp", "bps", "kg", "km", "eur", "usd", "gb", "mb", "ms", "s", "min", "hr", "hrs", "day",
    "days",
];

fn leet(t: &str) -> String {
    let letters = t.chars().filter(|c| c.is_ascii_alphabetic()).count();
    let digits = t.chars().filter(|c| c.is_ascii_digit()).count();
    if digits == 0 || letters < 2 {
        return t.to_string();
    }
    // Numbers with a known unit suffix (10x, 401k, 5pm, 2nd, 3m, 50bp) stay as
    // written; other digit-led words (`0ut`, `3arnings`) are leet.
    let lead_digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if lead_digits > 0 && UNIT_SUFFIXES.contains(&&t[lead_digits..]) {
        return t.to_string();
    }
    // Letters followed only by a number (q3, fy26, trading212) stay too.
    let lead_letters = t.chars().take_while(|c| c.is_ascii_alphabetic()).count();
    if lead_letters > 0 && t[lead_letters..].chars().all(|c| c.is_ascii_digit()) {
        return t.to_string();
    }
    t.chars()
        .map(|c| match c {
            '0' => 'o',
            '1' => 'i',
            '3' => 'e',
            '4' => 'a',
            '5' => 's',
            '7' => 't',
            '8' => 'b',
            o => o,
        })
        .collect()
}

fn squeeze(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let c: Vec<char> = t.chars().collect();
    let mut i = 0;
    while i < c.len() {
        let mut j = i;
        while j < c.len() && c[j] == c[i] {
            j += 1;
        }
        let n = j - i;
        let keep = if n >= 3 && c[i].is_alphabetic() { 1 } else { n };
        for _ in 0..keep {
            out.push(c[i]);
        }
        i = j;
    }
    out
}

/// Harmless idioms that share words with attack concepts.
const IDIOMS: &[(&str, &str)] = &[
    ("pump the brakes", "slow down"),
    ("pump the brake", "slow down"),
    ("gas pump", "fuel station"),
    ("fuel pump", "fuel station"),
    ("petrol pump", "fuel station"),
    ("heat pump", "heating unit"),
    ("water pump", "water unit"),
    ("pump prices", "fuel prices"),
    ("friend at the gym", "friend"),
    ("friend at school", "friend"),
    ("friend at university", "friend"),
    ("friend at uni", "friend"),
    ("friend at work", "friend"),
    ("friend at church", "friend"),
    ("friend at the club", "friend"),
];

const ACRONYMS: &[&str] = &[
    "SEC", "MNPI", "FDA", "CEO", "CFO", "COO", "IPO", "ETF", "SMA", "USD", "EUR", "API", "FINRA",
    "ESMA", "HANFA", "NDA", "PNL", "OK", "AI", "IT", "FAQ", "EU", "US", "UK", "USA", "CSV", "JSON",
    "HTTP", "Q1", "Q2", "Q3", "Q4", "FY", "MA", "PR", "TV", "VWAP", "OTC", "ATH", "IRA", "LLC",
    "INC", "PLC", "AG", "DD", "RSI", "EPS", "PE", "IBKR", "WSB", "DM", "NYSE", "LSE", "ZSE", "OMG",
    "LOL", "AND", "THE", "BUY", "SELL", "NOW", "NOT", "ALL", "YES", "NO", "PAPER", "LIVE", "REAL",
];

const SYMBOLS: &[&str] = &[
    "glxa",
    "omnd",
    "posd",
    "hrms",
    "rstr",
    "zgrb",
    "hltd",
    "galaxia",
    "restricta",
    "haltwell",
    "poseidon",
    "aapl",
    "tsla",
    "gme",
    "amc",
    "nvda",
    "msft",
];

impl Text {
    pub fn new(raw: &str) -> Text {
        let names_upper = raw.split(|c: char| !c.is_alphanumeric()).any(|w| {
            (2..=5).contains(&w.len())
                && w.chars().all(|c| c.is_ascii_uppercase())
                && !ACRONYMS.contains(&w)
        });
        let cashtag = raw
            .split_whitespace()
            .any(|w| w.starts_with('$') && w[1..].starts_with(|c: char| c.is_ascii_alphabetic()));
        let mut s = String::with_capacity(raw.len());
        for ch in raw.chars() {
            if ZERO_WIDTH.contains(&ch) {
                continue;
            }
            let ch = fullwidth(ch);
            for lc in ch.to_lowercase() {
                match fold(lc) {
                    Some(r) => s.push_str(r),
                    None => s.push(lc),
                }
            }
        }
        // apostrophes join (can't -> cant); `$`/`@` inside words are leet
        let mut t = String::with_capacity(s.len());
        let cs: Vec<char> = s.chars().collect();
        for (i, &ch) in cs.iter().enumerate() {
            let prev_alpha = i > 0 && cs[i - 1].is_alphabetic();
            let next_alpha = i + 1 < cs.len() && cs[i + 1].is_alphabetic();
            match ch {
                '\'' | '\u{2019}' | '\u{2018}' | '`' => {}
                '$' if prev_alpha => t.push('s'),
                '$' => {}
                '@' if prev_alpha && next_alpha => t.push('a'),
                o => t.push(o),
            }
        }
        let t = rejoin_spaced(&t);
        let toks: Vec<String> = t
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(|w| squeeze(&leet(w)))
            .collect();
        let mut norm = toks.join(" ");
        for (from, to) in IDIOMS {
            let padded = format!(" {norm} ");
            if padded.contains(&format!(" {from} ")) {
                norm = padded.replace(&format!(" {from} "), &format!(" {to} "));
                norm = norm.trim().to_string();
            }
        }
        let toks: Vec<String> = norm.split(' ').map(str::to_string).collect();
        let names_symbol = names_upper
            || cashtag
            || toks.iter().any(|w| SYMBOLS.contains(&w.as_str()))
            || norm.contains("open mind")
            || norm.contains("hermes freight")
            || norm.contains("zagreb savings")
            || norm.contains("zagrebacke banke");
        Text {
            norm,
            toks,
            names_symbol,
        }
    }

    fn tok_eq(tok: &str, pat: &str) -> bool {
        match pat.strip_suffix('*') {
            Some(p) => tok.starts_with(p),
            None => tok == pat,
        }
    }

    /// Start positions of `pat` (space-separated words, `*` = prefix).
    fn find(&self, pat: &str) -> Vec<usize> {
        let p: Vec<&str> = pat.split(' ').collect();
        if p.is_empty() || p.len() > self.toks.len() {
            return Vec::new();
        }
        (0..=self.toks.len() - p.len())
            .filter(|&i| {
                p.iter()
                    .enumerate()
                    .all(|(k, w)| Self::tok_eq(&self.toks[i + k], w))
            })
            .collect()
    }

    /// First pattern of `pats` present in the text.
    pub fn any(&self, pats: &[&'static str]) -> Option<&'static str> {
        pats.iter().copied().find(|p| !self.find(p).is_empty())
    }

    /// Phrase as the v1 list wrote it: word-start aligned, the last word may
    /// continue (`spoof` matches `spoofing`).
    pub fn phrase(&self, phrase: &str) -> bool {
        let n = Text::new(phrase);
        if n.norm.is_empty() {
            return false;
        }
        let mut p = n.norm.clone();
        p.push('*');
        let mut words: Vec<String> = p.split(' ').map(str::to_string).collect();
        // only the last word is a prefix; earlier words exact
        let last = words.len() - 1;
        for w in words.iter_mut().take(last) {
            w.retain(|c| c != '*');
        }
        let pat = words.join(" ");
        !self.find(&pat).is_empty()
    }

    /// `a` and `b` both present with their starts at most `w` words apart.
    fn near(
        &self,
        a: &[&'static str],
        b: &[&'static str],
        w: usize,
    ) -> Option<(&'static str, &'static str)> {
        for pa in a {
            let xs = self.find(pa);
            if xs.is_empty() {
                continue;
            }
            for pb in b {
                for y in self.find(pb) {
                    if xs.iter().any(|&x| x.abs_diff(y) <= w) {
                        return Some((pa, pb));
                    }
                }
            }
        }
        None
    }
}

// ------------------------------------------------------------------ lexicon

pub const TRADE: &[&str] = &[
    "buy*",
    "bought",
    "purchas*",
    "load up",
    "loading up",
    "load the boat",
    "get in",
    "get into",
    "get me in",
    "get me into",
    "put me in",
    "grab",
    "accumulat*",
    "position*",
    "go long",
    "long",
    "short",
    "shorting",
    "sell*",
    "calls",
    "puts",
    "option*",
    "trade*",
    "trading",
    "bet",
    "bets",
    "betting",
    "wager*",
    "invest*",
    "order*",
    "execut*",
    "scoop*",
    "snap up",
    "back up the truck",
    "all in",
    "shares",
    "stake",
    "kupi*",
    "kupuj*",
    "prodaj*",
    "ulozi*",
    "ulaganj*",
    "dionic*",
    "hit the bid",
    "lift the offer",
];

pub const EVENT: &[&str] = &[
    "merger*",
    "merge",
    "merging",
    "acqui*",
    "takeover*",
    "take over",
    "buyout*",
    "bought out",
    "buy out",
    "deal",
    "deals",
    "earnings",
    "results",
    "numbers",
    "figures",
    "guidance",
    "quarter*",
    "q1",
    "q2",
    "q3",
    "q4",
    "fda",
    "approv*",
    "contract*",
    "lawsuit*",
    "settlement*",
    "settle*",
    "ruling",
    "verdict",
    "downgrad*",
    "upgrad*",
    "bid",
    "tender",
    "ipo",
    "spin off",
    "spinoff",
    "split",
    "dividend*",
    "recall*",
    "restat*",
    "bankrupt*",
    "insolven*",
    "layoff*",
    "resign*",
    "profit warning",
    "outlook",
    "forecast*",
    "beat",
    "beats",
    "miss",
    "misses",
    "crush*",
    "announc*",
    "preuzim*",
    "spajanj*",
    "rezultat*",
    "dobit*",
    "ugovor*",
    "objav*",
];

pub const STRONG_PRIVATE: &[&str] = &[
    "leak*",
    "let slip",
    "wont be disclosed",
    "not be disclosed",
    "not disclosed",
    "undisclosed",
    "wont be announced",
    "wont be public",
    "copied on an email",
    "copied on the email",
    "email by mistake",
    "email by accident",
    "sent to me by mistake",
    "forwarded to me by mistake",
    "not meant to see",
    "wasnt meant to see",
    "not supposed to see",
    "wasnt supposed to see",
    "nothing is out",
    "not out yet",
    "isnt out yet",
    "cant tell you how",
    "cannot tell you how",
    "dont ask how",
    "unannounced",
    "unreleased",
    "unpublished",
    "not public",
    "not yet public",
    "isnt public",
    "is not public",
    "not been announced",
    "hasnt been announced",
    "has not been announced",
    "not yet announced",
    "not announced yet",
    "isnt announced",
    "under wraps",
    "hush hush",
    "keep this quiet",
    "keep it quiet",
    "keep this between",
    "keep it between",
    "between us",
    "between you and me",
    "confidential*",
    "privileged",
    "secret*",
    "embargo*",
    "nda",
    "deal team",
    "internal numbers",
    "internal memo*",
    "internal email*",
    "internal figures",
    "internal data",
    "draft press release",
    "draft earnings",
    "draft release",
    "advance copy",
    "early look",
    "a week early",
    "days early",
    "got the numbers early",
    "before the market knows",
    "before anyone knows",
    "before everyone knows",
    "before it gets out",
    "before word gets out",
    "before the news breaks",
    "before the news drops",
    "before news breaks",
    "before the news hits",
    "hit the wire",
    "hits the wire",
    "hasnt hit",
    "nobody knows yet",
    "no one knows yet",
    "nobody outside knows",
    "only i know",
    "cant say more",
    "cannot say more",
    "dont ask how i know",
    "not saying where",
    "cant say where",
    "wont say where",
    "trust me i know",
    "source inside",
    "sources inside",
    "from the inside",
    "inside source",
    "inside scoop",
    "off the record",
    "told me privately",
    "privately told",
    "not supposed to know",
    "shouldnt know",
    "shouldnt be telling",
    "nije javno",
    "jos nije objav*",
    "povjerljiv*",
    "tajn*",
    "insajd*",
    "procur*",
    "unutarnj* informacij*",
];

/// "before ... announcement/disclosure" within a few words.
const BEFORE: &[&str] = &["before", "ahead", "prije", "prior"];
const DISCLOSURE: &[&str] = &[
    "announc*",
    "disclos*",
    "public",
    "publish*",
    "press release",
    "wire",
    "objav*",
    "release",
    "released",
    "it comes out",
    "it is out",
    "it is official",
    "official",
];

pub const WEAK_PRIVATE: &[&str] = &[
    "friend*",
    "buddy",
    "pal",
    "cousin*",
    "brother*",
    "sister*",
    "uncle",
    "aunt",
    "wife",
    "husband",
    "girlfriend",
    "boyfriend",
    "neighbo*",
    "roommate",
    "colleague*",
    "coworker*",
    "co worker*",
    "my contact",
    "a contact",
    "my source",
    "a source",
    "someone at",
    "someone in",
    "someone on",
    "somebody at",
    "somebody in",
    "a guy at",
    "a guy in",
    "guy who works",
    "works at",
    "works in",
    "who works",
    "exec",
    "execs",
    "executive*",
    "director*",
    "board member*",
    "on the board",
    "cfo",
    "ceo",
    "coo",
    "chairman",
    "lawyer*",
    "banker*",
    "analyst at",
    "insider*",
    "heard*",
    "overheard",
    "whisper*",
    "a little bird",
    "birdie",
    "grapevine",
    "told me",
    "tells me",
    "telling me",
    "says",
    "said",
    "mentioned",
    "tipped me",
    "tip off",
    "tipped off",
    "prijatelj*",
    "kolega",
    "kolegica",
    "rodak",
    "iz uprave",
    "u upravi",
    "rekao",
    "rekla",
    "kaze",
];

pub const PUBLIC: &[&str] = &[
    "public earnings",
    "public call",
    "earnings call",
    "conference call",
    "press release says",
    "press release said",
    "announced today",
    "announced yesterday",
    "announced this morning",
    "newswire",
    "on the wire",
    "filing",
    "filed",
    "8 k",
    "10 q",
    "10 k",
    "publicly",
    "public news",
    "public headline*",
    "public source*",
    "in the fixture",
    "headline*",
    "published",
    "on the record",
    "official statement",
    "javno objav*",
];

pub const PROMOTE: &[&str] = &[
    "hype*",
    "fake press release",
    "fake release",
    "fake news",
    "false news",
    "fake announcement*",
    "false announcement*",
    "fabricat*",
    "made up news",
    "shill*",
    "promot*",
    "pump*",
    "napump*",
    "ramp*",
    "talk it up",
    "talk up",
    "talking up",
    "talk the stock",
    "spread the word",
    "spread*",
    "rumo*",
    "buzz",
    "blast*",
    "spam*",
    "flood*",
    "brigad*",
    "raid*",
    "campaign*",
    "telegram",
    "discord",
    "whatsapp",
    "signal group",
    "reddit",
    "wallstreetbets",
    "wsb",
    "stocktwits",
    "twitter",
    "tweet*",
    "forum*",
    "chat room",
    "chatroom",
    "influencer*",
    "newsletter*",
    "paid promotion",
    "sock puppet*",
    "sockpuppet*",
    "bots",
    "bot army",
    "bot accounts",
    "fake account*",
    "burner account*",
    "posts",
    "post about",
    "rocket*",
    "moon",
    "mooning",
    "to the moon",
    "fomo",
    "get everyone",
    "get people",
    "get them to",
    "convinc*",
    "make people",
    "followers",
    "subscribers",
    "my channel",
    "our channel",
    "group chat",
];

pub const EXIT: &[&str] = &[
    "sell into",
    "sell to them",
    "sell to the",
    "sell it to",
    "sell when they",
    "sell when it",
    "sell once",
    "sell after",
    "sell at the top",
    "sell higher",
    "sell high",
    "then sell",
    "then i sell",
    "then we sell",
    "then i will sell",
    "then ill sell",
    "so i can sell",
    "so we can sell",
    "dump*",
    "offload*",
    "unload*",
    "cash out",
    "cash in",
    "take profit*",
    "exit at",
    "exit when",
    "exit before",
    "exit once",
    "we exit",
    "i exit",
    "get out at",
    "get out before",
    "out at the top",
    "at the top",
    "at the peak",
    "at the high",
    "bail*",
    "before they realis*",
    "before they realiz*",
    "before they notice",
    "before people notice",
    "before the followers",
    "bag holder*",
    "bagholder*",
    "holding the bag",
    "leave them holding",
    "exit liquidit*",
    "latecomer*",
    "pa je prodaj*",
    "pa prodaj*",
    "rasprodaj*",
];

pub const PRICE_MOVE: &[&str] = &[
    "push the price",
    "push it up",
    "push up the",
    "push the stock",
    "push price",
    "push prices",
    "drive the price",
    "drive it up",
    "drive up the",
    "prop up",
    "prop it up",
    "inflate the price",
    "inflate the stock",
    "artificially",
    "move the market",
    "move the stock",
    "move the price",
    "run it up",
    "run the price",
    "get it to run",
    "make it run",
    "make it pop",
    "make the price",
    "juice the price",
    "spike the price",
    "spike it",
    "pop the price",
    "so it pops",
    "so it rockets",
    "so it jumps",
    "so the price jumps",
    "so the price rises",
    "so the price goes up",
    "so the price spikes",
    "the price jumps",
    "lift the price",
    "jack up",
    "jack the price",
    "pump up",
    "pump the price",
    "ramp it",
    "ramp up the price",
    "napumpaj*",
    "digni cijenu",
    "podigni cijenu",
    "gurni cijenu",
    "dignuti cijenu",
];

pub const COORD: &[&str] = &[
    "coordinat*",
    "lets all",
    "we all",
    "you all",
    "everyone",
    "everybody",
    "all of us",
    "all at once",
    "at the same time",
    "same time",
    "at exactly",
    "group buy*",
    "group",
    "members",
    "followers",
    "subscribers",
    "the channel",
    "my channel",
    "our channel",
    "the chat",
    "the server",
    "together",
    "in unison",
    "organis*",
    "organiz*",
    "synchroni*",
    "svi zajedno",
];

pub const SPOOF: &[&str] = &[
    "never intend to fill",
    "dont intend to fill",
    "do not intend to fill",
    "no intention of filling",
    "not intending to fill",
    "never meant to fill",
    "cancel before they fill",
    "cancel them before",
    "cancel before fill",
    "cancel before it fills",
    "pull them before",
    "pull them once",
    "pull them when",
    "pull the orders",
    "fake order*",
    "fake bid*",
    "fake offer*",
    "phantom order*",
    "phantom bid*",
    "flash orders",
    "think there is demand",
    "think theres demand",
    "fake demand",
    "fake supply",
];
const LAYER: &[&str] = &["layer*", "stack*", "wall of"];
const ORDERS: &[&str] = &["bid*", "order*", "offer*", "ask*"];

pub const WASH: &[&str] = &[
    "with myself",
    "to myself",
    "from myself",
    "between my own accounts",
    "between my accounts",
    "between two accounts",
    "between both my accounts",
    "between both accounts",
    "my other account",
    "my second account",
    "wifes account",
    "husbands account",
    "both sides of the trade",
    "both sides",
    "self trade*",
    "selftrad*",
    "fake volume",
    "create volume",
    "boost volume",
    "inflate volume",
    "show volume",
    "volume look",
    "look active",
    "look busy",
    "round trip*",
    "matched order*",
    "prearranged",
    "pre arranged",
    "back and forth",
];

pub const FRONT: &[&str] = &[
    "ahead of the client*",
    "ahead of our client*",
    "ahead of client*",
    "ahead of the fund",
    "ahead of our fund",
    "before the client*",
    "before our client*",
    "before the fund*",
    "before our fund",
    "before the block",
    "ahead of the block",
    "personal account first",
    "my own account first",
    "my account first",
    "our fund will buy",
    "our fund is buying",
    "the fund is about to",
    "client is about to",
    "clients order",
    "client order*",
];

pub const MARK_CLOSE: &[&str] = &[
    "mark the close",
    "marking the close",
    "set the closing",
    "set a higher closing",
    "push the close",
    "inflate the close",
    "bang the close",
    "paint the close",
];

pub const SQUEEZE_COORD: &[&str] = &["squeeze*"];

pub const TIP_STRONG: &[&str] = &[
    "no brainer",
    "nobrainer",
    "cant miss",
    "cannot miss",
    "sure bet",
    "safe bet",
    "free money",
    "easy money",
    "money printer",
    "zero risk",
    "zero risk play",
    "no risk",
    "risk free",
    "riskless",
    "without risk",
    "100 percent",
    "guarantee*",
    "garantira*",
    "zajamcen*",
    "will definitely",
    "definitely go up",
    "definitely rise",
    "certain to",
    "bound to go up",
    "will surely",
    "surely go up",
    "10x",
    "100x",
    "10 bagger",
    "ten bagger",
    "tenbagger",
    "multibagger",
    "multi bagger",
    "next big winner",
    "explode",
    "will explode",
    "skyrocket*",
    "to the moon",
    "moonshot",
    "moon shot",
    "cant go wrong",
    "cant fail",
    "never goes down",
    "only goes up",
    "promise me",
    "promise it",
    "you promise",
    "sigurica",
    "sigurn* zarad*",
];
const TIP_CONTEXT: &[&str] = &[
    "stock*", "share*", "ticker*", "play", "pick*", "winner*", "return*", "profit*", "gain*",
    "money", "go up", "goes up", "rise", "dionic*",
];
pub const TIP_ASK: &[&str] = &[
    "pick me",
    "pick a winner",
    "give me a winner",
    "your best pick",
    "top pick",
    "best stock",
    "what should i buy",
    "what do i buy",
    "what to buy",
    "which stock should",
    "which ticker should",
    "which one should i buy",
    "which share should",
    "which shares should",
    "tell me the one",
    "the one stock",
    "whats going to pop",
    "going to pop",
    "next to pop",
    "will pop",
    "what will go up",
    "which will go up",
    "what goes up",
    "sto da kupim",
    "koju dionicu da",
];

pub const LIVE: &[&str] = &[
    "real funds",
    "real cash",
    "real capital",
    "real euros",
    "real dollars",
    "actual funds",
    "actual cash",
    "my own money",
    "own money",
    "my funds",
    "interactive brokers",
    "ibkr",
    "robinhood",
    "etrade",
    "e trade",
    "schwab",
    "fidelity",
    "trading 212",
    "trading212",
    "degiro",
    "saxo",
    "etoro",
    "revolut",
    "coinbase",
    "binance",
    "my brokerage",
    "brokerage account",
    "broker account",
    "brokerage login",
    "broker login",
    "my login",
    "my password",
    "password*",
    "credential*",
    "account number",
    "account id",
    "iban",
    "api secret",
    "secret key",
    "access token",
    "oauth",
    "connect to my broker",
    "link my broker",
    "hook it up to my",
    "route it to",
    "route the order to",
    "send it to my",
    "for real",
    "not paper",
    "no paper",
    "skip the paper",
    "bypass the paper",
    "bypass paper",
    "turn off paper",
    "disable paper",
    "switch off paper",
    "leave paper",
    "real mode",
    "live mode",
    "live trading",
    "go live",
    "production account",
    "prod account",
    "wire*",
    "transfer*",
    "deposit*",
    "withdraw*",
    "top up",
    "pravi novac",
    "prave novce",
    "pravim novcem",
    "stvarni novac",
    "moj racun",
];

pub const PERSONAL: &[&str] = &[
    "life savings",
    "my savings",
    "savings",
    "nest egg",
    "college fund",
    "university fund",
    "kids fund",
    "401k",
    "401 k",
    "ira",
    "roth",
    "pension*",
    "retirement*",
    "mirovin*",
    "ustedevin*",
    "inheritance",
    "emergency fund",
    "house deposit",
    "house fund",
    "remortgag*",
    "mortgage*",
    "home equity",
    "take out a loan",
    "take a loan",
    "borrow*",
    "loan to buy",
    "credit card",
    "my salary",
    "my paycheck",
    "my paycheque",
    "everything i have",
    "everything i own",
    "all i have",
    "all my",
    "put everything in",
    "kredit*",
];
const ADVICE_ASK: &[&str] = &[
    "should i",
    "should we",
    "is it smart",
    "is it wise",
    "good idea",
    "yes or no",
    "worth it",
    "do you recommend",
    "would you",
    "trebam li",
    "isplati li",
];

pub const EDU_START: &[&str] = &[
    "what is",
    "what are",
    "whats",
    "what does",
    "what counts",
    "what makes",
    "what happens",
    "explain",
    "define",
    "definition",
    "describe how",
    "how do regulators",
    "how does the sec",
    "how do exchanges",
    "how are",
    "how is",
    "how does",
    "why is",
    "why are",
    "is it illegal",
    "is it legal",
    "history of",
    "tell me about",
    "teach me",
    "kako",
    "sto je",
    "objasni",
];
pub const EDU_ANY: &[&str] = &[
    "warning signs",
    "red flags",
    "how to spot",
    "how to recognise",
    "how to recognize",
    "how to detect",
    "protect myself",
    "avoid falling",
    "report suspected",
    "how regulators",
    "is illegal",
    "illegal",
    "for new investors",
];
pub const OP: &[&str] = &[
    "help me",
    "for me",
    "lets",
    "let us",
    "i want",
    "id like",
    "i would like",
    "i will",
    "ill",
    "we will",
    "we should",
    "can you buy",
    "buy",
    "sell",
    "go short",
    "short it",
    "short the",
    "short my",
    "place",
    "execute",
    "load",
    "plan",
    "step by step",
    "steps to",
    "exact",
    "how can i",
    "how could i",
    "how would i",
    "how do i",
    "how should i",
    "how can we",
    "how do we",
    "without getting caught",
    "not get caught",
    "avoid detection",
    "under the radar",
    "evade",
    "so i can",
    "then i",
    "then we",
    "run one",
    "do one",
    "set up",
    "set it up",
    "organise",
    "organize",
    "coordinate",
    "draft",
    "write posts",
    "write the posts",
    "write hype",
    "my character",
    "demonstrate it",
    "pomozi mi",
    "zelim",
];

/// One intent-layer hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub category: Category,
    pub rule: &'static str,
    pub evidence: String,
}

fn hit(category: Category, rule: &'static str, ev: &[&str]) -> Option<Hit> {
    Some(Hit {
        category,
        rule,
        evidence: ev.join("+"),
    })
}

/// A question *about* the rules, with no symbol and no operational cue.
pub fn educational(t: &Text) -> bool {
    let starts = EDU_START.iter().any(|p| {
        let n = p.split(' ').count();
        t.toks.len() >= n
            && p.split(' ')
                .enumerate()
                .all(|(k, w)| Text::tok_eq(&t.toks[k], w))
    });
    let anywhere = t.any(EDU_ANY).is_some();
    (starts || anywhere) && !t.names_symbol && t.any(OP).is_none()
}

/// The intent rules, in category precedence order.
pub fn detect(t: &Text) -> Option<Hit> {
    use Category::*;
    let trade = t.any(TRADE);
    let event = t.any(EVENT);
    // --- insider information
    if let Some(p) = t.any(STRONG_PRIVATE) {
        if let Some(x) = trade.or(event) {
            return hit(InsiderInformation, "strong_private_source", &[p, x]);
        }
    }
    if let Some((a, b)) = t.near(BEFORE, DISCLOSURE, 4) {
        if let Some(x) = trade {
            return hit(InsiderInformation, "trade_before_disclosure", &[a, b, x]);
        }
    }
    if let (Some(p), Some(e), Some(x)) = (t.any(WEAK_PRIVATE), event, trade) {
        if t.any(PUBLIC).is_none() {
            return hit(InsiderInformation, "private_source_event_trade", &[p, e, x]);
        }
    }
    // --- market manipulation
    let promote = t.any(PROMOTE);
    let exit = t.any(EXIT);
    let price = t.any(PRICE_MOVE);
    let coord = t.any(COORD);
    if let (Some(p), Some(e)) = (promote, exit) {
        return hit(MarketManipulation, "promote_then_exit", &[p, e]);
    }
    if let (Some(p), Some(m)) = (promote, price) {
        return hit(MarketManipulation, "promote_to_move_price", &[p, m]);
    }
    if let Some(m) = price {
        if let Some(x) = trade.or(exit).or(coord) {
            return hit(MarketManipulation, "trade_to_move_price", &[m, x]);
        }
    }
    if let (Some(p), Some(c), Some(x)) = (promote, coord, trade) {
        return hit(MarketManipulation, "coordinated_promotion", &[p, c, x]);
    }
    if let (Some(c), Some(e), Some(x)) = (coord, exit, trade) {
        return hit(MarketManipulation, "coordinated_buy_then_exit", &[c, e, x]);
    }
    if let (Some(s), Some(c)) = (t.any(SQUEEZE_COORD), coord) {
        return hit(MarketManipulation, "coordinated_squeeze", &[s, c]);
    }
    if let Some(s) = t.any(SPOOF) {
        return hit(MarketManipulation, "spoofing", &[s]);
    }
    if let Some((a, b)) = t.near(LAYER, ORDERS, 4) {
        if t.any(&["pull*", "cancel*", "withdraw*", "remove*"])
            .is_some()
        {
            return hit(MarketManipulation, "layering", &[a, b]);
        }
    }
    if let (Some(w), Some(x)) = (t.any(WASH), trade) {
        return hit(MarketManipulation, "wash_trade", &[w, x]);
    }
    if let (Some(f), Some(x)) = (t.any(FRONT), trade) {
        return hit(MarketManipulation, "front_running", &[f, x]);
    }
    if let Some(m) = t.any(MARK_CLOSE) {
        return hit(MarketManipulation, "marking_the_close", &[m]);
    }
    // --- live money
    if let Some(l) = t.any(LIVE) {
        let ctx = trade.or_else(|| t.any(&["money", "eur", "euro*", "usd", "dollar*", "funds"]));
        if ctx.is_some() || t.names_symbol {
            return hit(LiveMoney, "live_money", &[l, ctx.unwrap_or("symbol")]);
        }
    }
    // --- stock tips
    if let Some(a) = t.any(TIP_ASK) {
        return hit(StockTip, "tip_request", &[a]);
    }
    if let Some(s) = t.any(TIP_STRONG) {
        let ctx = trade.or_else(|| t.any(TIP_CONTEXT));
        if ctx.is_some() || t.names_symbol {
            return hit(StockTip, "promised_return", &[s, ctx.unwrap_or("symbol")]);
        }
    }
    // --- personal advice
    if let Some(p) = t.any(PERSONAL) {
        let ctx = trade.or_else(|| t.any(ADVICE_ASK));
        if ctx.is_some() || t.names_symbol {
            return hit(
                PersonalAdvice,
                "personal_money",
                &[p, ctx.unwrap_or("symbol")],
            );
        }
    }
    None
}

/// Every pattern list, for the self-check test and the docs.
pub fn all_lists() -> Vec<(&'static str, &'static [&'static str])> {
    vec![
        ("TRADE", TRADE),
        ("EVENT", EVENT),
        ("STRONG_PRIVATE", STRONG_PRIVATE),
        ("BEFORE", BEFORE),
        ("DISCLOSURE", DISCLOSURE),
        ("WEAK_PRIVATE", WEAK_PRIVATE),
        ("PUBLIC", PUBLIC),
        ("PROMOTE", PROMOTE),
        ("EXIT", EXIT),
        ("PRICE_MOVE", PRICE_MOVE),
        ("COORD", COORD),
        ("SPOOF", SPOOF),
        ("LAYER", LAYER),
        ("ORDERS", ORDERS),
        ("WASH", WASH),
        ("FRONT", FRONT),
        ("MARK_CLOSE", MARK_CLOSE),
        ("SQUEEZE_COORD", SQUEEZE_COORD),
        ("TIP_STRONG", TIP_STRONG),
        ("TIP_CONTEXT", TIP_CONTEXT),
        ("TIP_ASK", TIP_ASK),
        ("LIVE", LIVE),
        ("PERSONAL", PERSONAL),
        ("ADVICE_ASK", ADVICE_ASK),
        ("EDU_START", EDU_START),
        ("EDU_ANY", EDU_ANY),
        ("OP", OP),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pattern_is_already_normalised() {
        for (name, list) in all_lists() {
            for p in list {
                let bare = p.replace('*', "");
                let n = Text::new(&bare).norm;
                assert_eq!(n, bare, "{name}: pattern {p:?} normalises to {n:?}");
            }
        }
    }

    #[test]
    fn deobfuscation() {
        let n = |s: &str| Text::new(s).norm;
        assert_eq!(n("p.u.m.p and d.u.m.p"), "pump and dump");
        assert_eq!(n("i n s i d e r   i n f o"), "insider info");
        assert_eq!(n("1nsid3r t1p"), "insider tip");
        assert_eq!(
            n("in\u{200b}side informa\u{00ad}tion"),
            "inside information"
        );
        assert_eq!(n("inѕider"), "insider"); // Cyrillic dze
        assert_eq!(n("ＧＬＸＡ"), "glxa");
        assert_eq!(n("puuuuump it"), "pump it");
        assert_eq!(n("Q3 numbers, 10x, 401k, 5pm"), "q3 numbers 10x 401k 5pm");
        assert_eq!(
            n("prije objave, dionica Zagrebačke"),
            "prije objave dionica zagrebacke"
        );
        assert_eq!(n("ca$h 0ut"), "cash out");
        assert_eq!(n("pump & dump"), "pump and dump");
        assert_eq!(n("🚀🚀 HRMS"), "rocket rocket hrms");
        assert_eq!(n("Pump the brakes, can't"), "slow down cant");
        assert_eq!(n("U.S. stocks, e.g. GLXA"), "u s stocks e g glxa");
        assert_eq!(n("119.60 and 3/8 SMA"), "119 60 and 3 8 sma");
    }

    #[test]
    fn symbols_detected() {
        assert!(Text::new("buy $XYZQ").names_symbol);
        assert!(Text::new("buy ZZTOP now").names_symbol);
        assert!(Text::new("Open Mind contract").names_symbol);
        assert!(!Text::new("What does the SEC say about MNPI?").names_symbol);
    }

    #[test]
    fn near_window() {
        let t = Text::new("buy it before the big announcement");
        assert!(t.near(BEFORE, DISCLOSURE, 4).is_some());
        let t = Text::new("before lunch I will read every single public filing");
        assert!(t.near(BEFORE, DISCLOSURE, 4).is_none());
    }
}
