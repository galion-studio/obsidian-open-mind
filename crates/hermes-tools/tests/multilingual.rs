// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Browser HTML helpers and cut helpers on any language and script; each
//! sample is cut at every char boundary and mixed with HTML delimiters.

use hermes_tools::agent::html;

const SAMPLES: &[&str] = &[
    "Črnomerec Šibenik Đakovo Łódź Straße",
    "Москва Київ Београд &ё; &Ж",
    "数学的証明 中华人民共和国 한국어",
    "مرحبا بالعالم",
    "שלום עולם",
    "नमस्ते दुनिया",
    "👩‍👩‍👧‍👦 🏳️‍🌈 ❤️ 🇭🇷 👍🏽",
    "&č; &数; &👍; &#x1F44D; &#128077; &#xZZ; &amp;ž",
];
const DELIMS: &[&str] = &["&", ";", "&#", "&#x", "<", ">", "</", "<!--", "-->", "\"", "'", "=", "<a href=", "<title>", "<script>"];

fn variants() -> Vec<String> {
    let mut v = vec![];
    for s in SAMPLES {
        let idx: Vec<usize> = s.char_indices().map(|(i, _)| i).chain([s.len()]).collect();
        for &i in &idx {
            v.push(s[..i].to_string());
            v.push(s[i..].to_string());
            for d in DELIMS {
                v.push(format!("{}{d}{}", &s[..i], &s[i..]));
            }
        }
    }
    v
}

#[test]
fn html_helpers_take_any_text() {
    for t in variants() {
        let _ = html::decode_entities(&t);
        let _ = html::absolutize("https://例え.jp/パス/", &t);
        let _ = html::absolutize(&format!("https://x.org/{t}"), "a");
        let p = html::to_text(&format!("<html><head><title>{t}</title></head><body><p>{t}</p><a href=\"/{t}\">{t}</a></body></html>"), "https://x.org/", 5);
        assert!(p.text.is_char_boundary(p.text.len()));
        let _ = html::to_text(&t, "https://x.org/", 5);
    }
    let p = html::to_text("<p>Привет &amp; 数学 &č;</p>", "https://x.org/", 0);
    assert!(p.text.contains("Привет & 数学"), "{}", p.text);
}

#[test]
fn cut_helpers_take_any_text() {
    for t in variants() {
        for n in 0..14 {
            let a = hermes_tools::agent::cut(&t, n);
            let b = hermes_tools::audit::cut(&t, n);
            assert!(a.is_char_boundary(a.len()) && b.is_char_boundary(b.len()));
        }
    }
}
