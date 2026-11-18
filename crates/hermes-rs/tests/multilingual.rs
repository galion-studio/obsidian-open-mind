// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Every text helper must take any language: Cyrillic, CJK, Arabic and
//! Hebrew (RTL), Devanagari, Latin diacritics, emoji with ZWJ and variation
//! selectors. Each sample is also cut at every char boundary and mixed with
//! the ASCII delimiters the parsers look for, so a byte-index slice that
//! lands inside a multi-byte character would panic here.

use hermes_rs::denylist::{self, Policy};
use hermes_rs::tools::{Event, Tool, ToolCtx, ToolOutput};
use serde_json::json;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const SAMPLES: &[&str] = &[
    "Zagreb, Črnomerec, Šibenik, Đakovo — Łódź, Kraków, Gdańsk, Ærø, Straße",
    "Москва, Київ, Београд — Привет, мир! Ёжик &ё; <б>",
    "数学的証明と東京大学。中华人民共和国。한국어 문장입니다。",
    "مرحبا بالعالم — العربية من اليمين إلى اليسار",
    "שלום עולם, עברית מימין לשמאל",
    "नमस्ते दुनिया, हिन्दी देवनागरी लिपि",
    "emoji 👩‍👩‍👧‍👦 🏳️‍🌈 ❤️ 🇭🇷 👍🏽 & more",
    "mixed &č; &Ж &数; &👍; <a href=\"/ž\">ž</a> [ж](https://例え.jp/パス) ${č} `ш` -ž --ж",
    "e\u{301} combining, ZWJ\u{200d}, RTL mark \u{200f}, BOM \u{feff}",
];

const DELIMS: &[&str] = &[
    "&", ";", "<", ">", "</", "[", "]", "(", ")", "](", "`", "${", "}", "-", "--", "\"", "'", "\n", "---\n", "://", "/", "#", "=", ":",
];

fn variants() -> Vec<String> {
    let mut v = vec![];
    for s in SAMPLES {
        v.push(s.to_string());
        let idx: Vec<usize> = s.char_indices().map(|(i, _)| i).chain([s.len()]).collect();
        for &i in &idx {
            v.push(s[..i].to_string());
            v.push(s[i..].to_string());
        }
        for d in DELIMS {
            for &i in idx.iter().step_by(3) {
                v.push(format!("{}{d}{}", &s[..i], &s[i..]));
                v.push(format!("{d}{}", &s[i..]));
                v.push(format!("{}{d}", &s[..i]));
            }
        }
    }
    v
}

#[test]
fn redact_and_header_helpers_take_any_text() {
    for t in variants() {
        let _ = hermes_rs::redact(&t);
        for n in [0, 1, 2, 3, 5, 7, 11, 200] {
            let h = hermes_rs::hdr_value(&t, n);
            assert!(h.chars().count() <= n);
            let c = hermes_rs::mcp::clean_text(&t, n);
            assert!(c.is_char_boundary(c.len()));
        }
    }
}

#[test]
fn memory_citations_yaml_arxiv_take_any_text() {
    for t in variants() {
        let _ = hermes_rs::memory::sanitize_context(&t);
        let _ = hermes_rs::memory::context_block(&t);
        let _ = hermes_rs::citations::normalize_url(&t);
        let _ = hermes_rs::citations::match_key(&t);
        let _ = hermes_rs::citations::quote_in_evidence(&t, &t);
        let _ = hermes_rs::citations::cites(&t);
        let _ = hermes_rs::yaml::parse(&t);
        let _ = hermes_rs::yaml::parse(&format!("key: {t}\nlist:\n  - {t}\n  - \"{t}\"\n"));
        let _ = hermes_rs::arxiv::parse(&format!(
            "<feed><entry><id>http://arxiv.org/abs/2402.03300v3</id><title>{t}</title><summary>{t}</summary><author><name>{t}</name></author></entry></feed>"
        ));
        let _ = hermes_rs::arxiv::parse(&t);
        let _ = hermes_rs::mcp::sanitize(&t);
        let _ = hermes_rs::mcp::parse_env_text(&t);
        let _ = hermes_rs::time::parse_rfc3339(&t);
        let _ = hermes_rs::time::parse_rfc3339(&format!("2026-11-19T00:00:00{t}"));
        let _ = hermes_rs::delegate::user_asked_for_helper(&t);
    }
}

#[test]
fn source_check_helpers_take_any_text() {
    use hermes_rs::source_check as sc;
    for t in variants() {
        let _ = sc::html_text(&t);
        let _ = sc::html_title(&format!("<title>{t}</title>"));
        let _ = sc::html_text(&format!("<html><script>{t}</script><p>{t}</p></html>"));
        let _ = sc::squash(&t);
        let _ = sc::check_url(&format!("https://{t}"));
        let _ = sc::check_url(&format!("https://example.org/{t}"));
        let _ = sc::norm_doi(&t);
        let _ = sc::norm_doi(&format!("10.1000/{t}"));
    }
    assert!(sc::check_url("https://ru.wikipedia.org/wiki/Математика").is_err());
    assert!(sc::squash(&sc::html_text("<p>Привет,\n  МИР</p>")).contains("привет, мир"));
}

#[test]
fn deny_list_takes_any_text() {
    let dir = std::env::temp_dir().join(format!("hermes-ml-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pol = Policy::workspace(dir.clone());
    for t in variants() {
        let _ = denylist::check_command(&t, &pol);
        let _ = denylist::check_command(&format!("echo {t} > out.txt"), &pol);
        let _ = denylist::check_path(&t, denylist::Access::Read, &dir, &pol);
        let _ = denylist::check_url(&t, &pol, true);
        let _ = denylist::delegate_task_check(&t, &t, &pol);
        let _ = denylist::check_tool_call("write_file", &json!({"path": "a.txt", "content": t}), &pol);
        let _ = denylist::check_tool_call("terminal", &json!({"command": t}), &pol);
        let _ = denylist::check_tool_call("delegate_task", &json!({"goal": t, "context": t}), &pol);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

struct Boom;
impl Tool for Boom {
    fn name(&self) -> String {
        "boom".into()
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object"})
    }
    fn description(&self) -> String {
        "panics on purpose".into()
    }
    fn call(&self, _a: &serde_json::Value, _c: &ToolCtx) -> ToolOutput {
        let s = "Ž";
        let n = 1;
        ToolOutput::ok(&s[..n])
    }
}

#[test]
fn a_panicking_tool_is_an_error_not_a_crash() {
    let cancel = Arc::new(AtomicBool::new(false));
    let emit = |_e: Event| {};
    let ctx = ToolCtx {
        session_id: "ml",
        spend_session: "ml",
        depth: 0,
        user_text: "",
        cancel: &cancel,
        emit: &emit,
    };
    let o = hermes_rs::tools::call_isolated(&Boom, "boom", &json!({}), &ctx);
    assert!(o.is_error);
    assert!(o.content.contains("failed internally"), "{}", o.content);
}
