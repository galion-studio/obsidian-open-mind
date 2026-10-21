// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/arena`: port of console-ide `src/routes/_console/arena.tsx` (read
//! side). Live tree /opt/galion/console-ide, read-only 2026-10-21
//! 05:06 CEST (sha256 249169c4...). Data: `GET /api/arena` on the console API (`arena.py status`,
//! read-only: last 16 board rows, spend, champion).
//!
//! Not ported, on purpose (owner decision, see docs/console-port-1.md):
//! Breed (`generation`), Stop (`kill`) and Promote (`promote <id>`). Breed
//! runs a paid generation against the arena's $2 cap; the swarm may not
//! start paid model runs, and a no-JS form here would make that one click
//! from any page. The page shows the buttons' place with a note and the
//! actions stay on the TS console until Slavik decides.

use super::jsfmt::{finite, num, to_fixed};
use crate::html::esc;
use crate::json::Value;

/// JSX `{value}` child text.
fn child(v: &Value) -> String {
    match v {
        Value::Num(n) => num(*n),
        Value::Str(s) => s.clone(),
        _ => String::new(),
    }
}

/// The spend line under the (not ported) buttons.
pub fn spend_line(board: Option<&Value>) -> String {
    let Some(b) = board else {
        return "…".into();
    };
    let sp = b.get("spend");
    let mut out = if matches!(sp, Value::Obj(_)) {
        let f = |k: &str| finite(sp.get(k));
        format!(
            "{}/{} generations · ${} / ${}",
            child(sp.get("generations")),
            child(sp.get("max_generations")),
            f("spent_usd")
                .map(|n| to_fixed(n, 2))
                .unwrap_or_else(|| "NaN".into()),
            f("cap_usd")
                .map(|n| to_fixed(n, 0))
                .unwrap_or_else(|| "NaN".into()),
        )
    } else {
        "…".into()
    };
    if b.get("stopped").as_bool() == Some(true) {
        out.push_str(" · stopped");
    }
    out
}

fn row_kind(r: &Value) -> String {
    match r.get("mutation") {
        Value::Str(m) if !m.is_empty() => format!("mutated {m}"),
        _ if r.get("mode").as_str() == Some("ran") => "ran".into(),
        _ => "elite".into(),
    }
}

/// Page body. `board` is the `/api/arena` JSON, `None` when it did not answer.
pub fn body(board: Option<&Value>) -> String {
    let mut out = String::from("<div class=\"page w42\">");
    out.push_str(
        "<header><p class=\"k\">Private arena · Hermes stays the intern</p>\
<p class=\"lede\">Four throwaway specialists. Slot 0 is the parent. The other three each change one knob. Promote does not edit SOUL.</p></header>",
    );
    out.push_str(
        "<p class=\"note\">Breed, Stop and Promote stay on the TypeScript console for now: Breed starts a paid generation, and this Rust page does not spend. It only reads the board.</p>",
    );
    out.push_str(&format!(
        "<p class=\"note\">{}</p>",
        esc(&spend_line(board))
    ));
    let Some(b) = board else {
        out.push_str("<p class=\"note\">The arena did not answer (GET /api/arena).</p></div>");
        return out;
    };
    if let Some(e) = b.get("error").as_str().filter(|e| !e.is_empty()) {
        out.push_str(&format!("<p class=\"err\">{}</p>", esc(e)));
    }
    let ch = b.get("champion");
    if matches!(ch, Value::Obj(_)) {
        out.push_str(&format!(
            "<h2>Champion</h2><p>{} · {} · {} · {}</p>",
            esc(&child(ch.get("id"))),
            esc(&child(ch.get("voice"))),
            esc(&child(ch.get("skill"))),
            esc(&child(ch.get("score")))
        ));
    }
    out.push_str("<h2>Board</h2><ul class=\"rows arena\">");
    for r in b.get("rows").as_array().iter().rev() {
        let answer = match r.get("answer") {
            Value::Str(a) if !a.is_empty() => format!("<span class=\"ans\">{}</span>", esc(a)),
            _ => String::new(),
        };
        out.push_str(&format!(
            "<li><span class=\"who\">{}<span class=\"tiny subtle ell\">{} · {} · {}</span>{answer}</span><span class=\"score mono\">{}</span></li>",
            esc(&child(r.get("id"))),
            esc(&row_kind(r)),
            esc(&child(r.get("voice"))),
            esc(&child(r.get("skill"))),
            esc(&child(r.get("score")))
        ));
    }
    out.push_str("</ul></div>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    #[test]
    fn board_renders_like_the_tsx() {
        let v = parse(br#"{"ok":true,"stopped":false,"note":"n","spend":{"cap_usd":2,"spent_usd":0,"generations":2,"max_generations":3},
          "champion":{"id":"s0","voice":"calm","skill":"math","score":7},
          "rows":[{"id":"a","generation":1,"voice":"v","skill":"s","tools":[],"mutation":null,"mode":"dry","score":1},
                  {"id":"b<i>","generation":2,"voice":"v2","skill":"s2","tools":[],"mutation":"voice","mode":"ran","score":2.5,"answer":"line1\nline2"},
                  {"id":"c","generation":2,"voice":"v3","skill":"s3","tools":[],"mutation":null,"mode":"ran","score":0}]}"#).unwrap();
        let b = body(Some(&v));
        assert!(b.contains("2/3 generations · $0.00 / $2"));
        assert!(!b.contains("· stopped"));
        assert!(b.contains("<h2>Champion</h2><p>s0 · calm · math · 7</p>"));
        // newest first
        let (ic, ib, ia) = (
            b.find(">c<").unwrap(),
            b.find("b&lt;i&gt;").unwrap(),
            b.find(">a<").unwrap(),
        );
        assert!(ic < ib && ib < ia);
        assert!(b.contains("mutated voice · v2 · s2"));
        assert!(b.contains("ran · v3 · s3"));
        assert!(b.contains("elite · v · s"));
        assert!(b.contains("<span class=\"ans\">line1\nline2</span>"));
        assert!(b.contains(">2.5</span>"));
        assert!(!b.contains("<form") && !b.contains("<script"));
    }

    #[test]
    fn stopped_error_and_down() {
        let v = parse(br#"{"ok":false,"stopped":true,"error":"arena down","spend":{"cap_usd":2.4,"spent_usd":1.005,"generations":3,"max_generations":3}}"#).unwrap();
        let b = body(Some(&v));
        assert!(b.contains("3/3 generations · $1.00 / $2 · stopped"));
        assert!(b.contains("<p class=\"err\">arena down</p>"));
        assert!(!b.contains("Champion"));
        let d = body(None);
        assert!(d.contains("<p class=\"note\">…</p>"));
        assert!(d.contains("did not answer"));
    }
}
