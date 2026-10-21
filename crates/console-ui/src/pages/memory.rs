// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/memory` (console-ide `src/routes/_console/memory.tsx`, live tree
//! 2026-10-20 21:25 CEST): Hermes's memory pyramid on the host
//! (`/var/lib/memory-tencentdb` live), counted by [`crate::port4::memory_snap`].
//!
//! Same rows and labels as the TSX. Differences: no 4 s poll (a Refresh
//! link); when no memory folder is configured, or it cannot be read, the page
//! says so instead of showing zeros; a capped walk is named.

use super::p4::count;
use crate::html::esc;
use crate::icons::icon;
use crate::port4::MemorySnap;

fn row(label: &str, value: &str) -> String {
    format!(
        "<li><span class=\"p4-n\">{}</span><span class=\"ell\">{}</span></li>",
        esc(value),
        esc(label)
    )
}

/// `snap` is `None` when no memory folder is configured.
pub fn body(snap: Option<&MemorySnap>) -> String {
    let c = |f: fn(&MemorySnap) -> u64| count(snap.filter(|s| s.root_ok).map(f));
    let mut out = format!(
        "<div class=\"page w46\"><header><p class=\"k\"><a href=\"/console\">{} Desk</a></p>\
<h1><span class=\"ell\">Memory</span></h1><p class=\"lede\">Hermes Research on the Obsidian stack</p></header>",
        icon("desk", "i i-sm")
    );
    match snap {
        None => out.push_str(
            "<p class=\"note p4-bad\">No memory folder is configured for this console (<code>--memory-root</code>).</p>",
        ),
        Some(s) if !s.root_ok => out.push_str(&format!(
            "<p class=\"note p4-bad\">The memory folder could not be read: {}.</p>",
            esc(&s.error)
        )),
        Some(s) if s.capped => out.push_str(
            "<p class=\"note\">The folder walk stopped at its safety limit; counts are lower bounds.</p>",
        ),
        Some(_) => {}
    }
    out.push_str(
        "<section class=\"p4-sec\" aria-label=\"Layers\"><h2>Layers</h2><ul class=\"rows\">",
    );
    out.push_str(&row("L0 Conversation", &c(|s| s.l0)));
    out.push_str(&row("L1 Atom", &c(|s| s.l1)));
    out.push_str(&row("L2 Scenario", &c(|s| s.l2)));
    out.push_str(&row("L3 Persona", &c(|s| s.l3_bytes)));
    out.push_str("</ul></section><section class=\"p4-sec\" aria-label=\"Offload\"><h2>Offload</h2><ul class=\"rows\">");
    out.push_str(&row("scenes", &c(|s| s.scenes)));
    out.push_str(&row("raw tool refs", &c(|s| s.refs)));
    out.push_str(&row("mermaid canvases", &c(|s| s.canvases)));
    out.push_str("</ul></section>");
    let model = snap.map(|s| s.model.as_str()).unwrap_or("");
    out.push_str(&format!(
        "<section class=\"p4-sec\" aria-label=\"Model\"><h2>Model</h2><p class=\"p4-small\">{}</p></section>",
        esc(model)
    ));
    let persona = snap.map(|s| s.persona_excerpt.as_str()).unwrap_or("");
    out.push_str(&format!(
        "<section class=\"p4-sec\" aria-label=\"Persona\"><h2>Persona <a href=\"/md/persona\">(whole file)</a></h2><pre class=\"p4-pre\">{}</pre></section>",
        esc(persona)
    ));
    out.push_str("<p class=\"note\"><a href=\"/memory\">Refresh</a></p></div>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> MemorySnap {
        MemorySnap {
            l0: 249,
            l1: 215,
            l2: 249,
            l3_bytes: 34_907,
            scenes: 66,
            refs: 940,
            canvases: 17,
            model: "galion".into(),
            persona_excerpt: "# Persona <b>&\"x\"".into(),
            root_ok: true,
            capped: false,
            error: String::new(),
        }
    }

    #[test]
    fn renders_live_shape() {
        let h = body(Some(&snap()));
        for (label, v) in [
            ("L0 Conversation", "249"),
            ("L1 Atom", "215"),
            ("L2 Scenario", "249"),
            ("L3 Persona", "34907"),
            ("scenes", "66"),
            ("raw tool refs", "940"),
            ("mermaid canvases", "17"),
        ] {
            assert!(
                h.contains(&format!(
                    "<span class=\"p4-n\">{v}</span><span class=\"ell\">{label}</span>"
                )),
                "{label}"
            );
        }
        assert!(h.contains("<p class=\"p4-small\">galion</p>"));
        assert!(h.contains("# Persona &lt;b&gt;&amp;&quot;x&quot;"));
        assert!(!h.contains("<b>"));
        assert!(!h.contains("p4-bad"));
        assert!(!h.contains("<script"));
    }

    #[test]
    fn unconfigured_and_unreadable() {
        let h = body(None);
        assert!(h.contains("No memory folder is configured"));
        assert_eq!(h.matches(">—</span>").count(), 7);
        let mut s = snap();
        s.root_ok = false;
        s.error = "permission denied".into();
        let h = body(Some(&s));
        assert!(h.contains("could not be read: permission denied."));
        assert_eq!(h.matches(">—</span>").count(), 7);
        let mut s = snap();
        s.capped = true;
        assert!(body(Some(&s)).contains("lower bounds"));
    }
}
