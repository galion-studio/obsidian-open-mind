// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/mesh` (console-ide `src/routes/_console/mesh.tsx`, live tree
//! 2026-10-20 21:25 CEST): the "Scope" room, one live score line from
//! `GET /api/mesh` (`score[]`, `phase`).
//!
//! Same text as the TSX. Differences: no 4 s poll (a Refresh link); when the
//! mesh status does not load the page says why instead of only "No live
//! line yet." (the TSX left the board empty on any error).

use crate::html::esc;
use crate::json::Value;

/// Most score lines shown, and longest line.
pub const MAX_LINES: usize = 50;
pub const MAX_LINE: usize = 2_000;

/// The TSX footer: `phase === "idle" ? "Cold. Nothing is spending." : phase || "Cold."`.
pub fn phase_text(board: Option<&Value>) -> String {
    match board.and_then(|b| b.get("phase").as_str()) {
        Some("idle") => "Cold. Nothing is spending.".into(),
        Some(p) if !p.is_empty() => p.chars().take(200).collect(),
        _ => "Cold.".into(),
    }
}

/// Score lines (strings only, capped).
pub fn lines(board: Option<&Value>) -> Vec<String> {
    board
        .map(|b| b.get("score").as_array())
        .unwrap_or(&[])
        .iter()
        .filter_map(Value::as_str)
        .take(MAX_LINES)
        .map(|l| l.chars().take(MAX_LINE).collect())
        .collect()
}

/// `miss` is the reason the board did not load (empty when it did).
pub fn body(board: Option<&Value>, miss: &str) -> String {
    let mut out = String::from(
        "<div class=\"page w42\"><header><p class=\"k\">One live line</p>\
<p class=\"lede\">This is the current measurement. Older score lines are not shown.</p></header>\
<section class=\"p4-sec\"><ul class=\"rows\">",
    );
    let ls = lines(board);
    if ls.is_empty() {
        out.push_str("<li><span>No live line yet.</span></li>");
    }
    for l in &ls {
        out.push_str(&format!(
            "<li><span class=\"p4-line\">{}</span></li>",
            esc(l)
        ));
    }
    out.push_str("</ul></section>");
    if !miss.is_empty() {
        out.push_str(&format!(
            "<p class=\"note p4-bad\">Mesh status did not load: {}.</p>",
            esc(miss)
        ));
    }
    out.push_str(&format!(
        "<p class=\"note\">{}</p><p class=\"note\"><a href=\"/mesh\">Refresh</a></p></div>",
        esc(&phase_text(board))
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    #[test]
    fn live_shape() {
        let v = json::parse(
            br#"{"ok":true,"phase":"idle","n":55,"score":["SCOPE 3 <ok> & fine"],"cold":true}"#,
        )
        .unwrap();
        let h = body(Some(&v), "");
        assert!(h.contains("<span class=\"p4-line\">SCOPE 3 &lt;ok&gt; &amp; fine</span>"));
        assert!(h.contains("Cold. Nothing is spending."));
        assert!(!h.contains("No live line yet."));
        assert!(!h.contains("p4-bad"));
    }

    #[test]
    fn phases_and_empty() {
        assert_eq!(phase_text(None), "Cold.");
        let run = json::parse(br#"{"phase":"run","score":[]}"#).unwrap();
        assert_eq!(phase_text(Some(&run)), "run");
        let blank = json::parse(br#"{"phase":"","score":[1,"a"]}"#).unwrap();
        assert_eq!(phase_text(Some(&blank)), "Cold.");
        assert_eq!(lines(Some(&blank)), vec!["a".to_string()]);
        let h = body(None, "HTTP 500 (mesh down)");
        assert!(h.contains("No live line yet."));
        assert!(h.contains("Mesh status did not load: HTTP 500 (mesh down)."));
        assert!(h.contains("<p class=\"note\">Cold.</p>"));
    }

    #[test]
    fn caps() {
        let many: Vec<String> = (0..80).map(|i| format!("\"l{i}\"")).collect();
        let v = json::parse(format!("{{\"score\":[{}]}}", many.join(",")).as_bytes()).unwrap();
        assert_eq!(lines(Some(&v)).len(), MAX_LINES);
        let long = format!("{{\"score\":[\"{}\"]}}", "x".repeat(5000));
        let v = json::parse(long.as_bytes()).unwrap();
        assert_eq!(lines(Some(&v))[0].len(), MAX_LINE);
    }
}
