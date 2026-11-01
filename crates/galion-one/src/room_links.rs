// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Approvals link in the one.galion.app room (2026-11-01 CET).
//!
//! The room page (`one/index-v2.html`) lives outside this binary, so the
//! link is added here, at serve time, the same way the boot screen is:
//! nothing on disk changes. The link stays hidden until the device proves it
//! is paired (`GET /api/approvals/count` answers 200); it then shows
//! "Approvals" with a small badge for requests ready to sign.
//!
//! Skipped when the page already links to `/approvals` (so a later edit of
//! the HTML file cannot produce two links), and when `ONE_ROOM_APPROVALS=off`.
//! The count is read on load, when the tab comes back, and every 5 minutes;
//! it never sends anything but a GET. Pairing in the room (the Phone pill
//! turning "paired") re-checks at once.

/// Marker id; also used to detect a second injection.
pub const ID: &str = "g1-appr";

const SNIPPET: &str = concat!(
    r##"<a class="g1-appr" id="g1-appr" href="/approvals" hidden>"##,
    r##"<span class="g1-appr-t">Approvals</span>"##,
    r##"<span class="g1-appr-n" id="g1-appr-n" hidden></span></a>"##,
    r##"<style id="g1-appr-css">"##,
    ".g1-appr{margin-left:auto;display:inline-flex;align-items:center;gap:.5rem;min-height:2.75rem;",
    "padding:0 1rem;border-radius:999px;border:1px solid rgba(255,255,255,.12);",
    "background:rgba(255,255,255,.06);color:var(--g-fg,#f4f5ff);font-size:.8125rem;",
    "letter-spacing:.02em;text-decoration:none;white-space:nowrap;-webkit-tap-highlight-color:transparent;",
    "transition:background .2s var(--glide,ease)}",
    ".g1-appr:hover{background:rgba(255,255,255,.1)}",
    ".g1-appr:focus-visible{outline:2px solid var(--g-a,#6d7bff);outline-offset:2px}",
    ".g1-appr[hidden],.g1-appr-n[hidden]{display:none}",
    ".g1-appr-n{display:inline-flex;align-items:center;justify-content:center;min-width:1.375rem;",
    "height:1.375rem;padding:0 .375rem;border-radius:999px;background:var(--g-w,#ffb36b);",
    "color:#1a0e00;font-size:.75rem;font-weight:600;line-height:1}",
    ".g1-appr.g1-appr-fixed{position:fixed;z-index:5;top:max(.75rem,env(safe-area-inset-top));",
    "right:max(1rem,env(safe-area-inset-right))}",
    // The room's own "Sound off" button follows the title with margin-left:auto;
    // the link takes that slot, so the button sits next to it on wide screens and
    // keeps its right-hand place when it wraps on a phone.
    ".g1-appr~.g1-snd{margin-left:.25rem}",
    "@media (max-width:520px){.g1-appr~.g1-snd{margin-left:auto}}",
    "</style><script>(function(){",
    "var a=document.getElementById(\"g1-appr\"),n=document.getElementById(\"g1-appr-n\");if(!a||!n)return;",
    "var t=document.querySelector(\".hm-head .hm-title\");",
    "if(t&&t.parentNode){t.parentNode.insertBefore(a,t.nextSibling)}else{a.classList.add(\"g1-appr-fixed\")}",
    "var busy=false;function tick(){if(busy)return;busy=true;",
    "fetch(\"/api/approvals/count\",{credentials:\"same-origin\",cache:\"no-store\"})",
    ".then(function(r){return r.ok?r.json():null})",
    ".then(function(j){busy=false;if(!j||typeof j.ready!==\"number\"){a.hidden=true;return}",
    "var k=j.ready;a.hidden=false;n.hidden=k<1;n.textContent=k>9?\"9+\":String(k);",
    "a.setAttribute(\"aria-label\",k>0?\"Approvals, \"+k+\" waiting for you\":\"Approvals\")})",
    ".catch(function(){busy=false})}",
    "tick();setInterval(function(){if(!document.hidden)tick()},300000);",
    "document.addEventListener(\"visibilitychange\",function(){if(!document.hidden)tick()});",
    // Pairing in the room flips the Phone pill to "paired": re-check then.
    "var pv=document.getElementById(\"st-dev-v\");if(pv&&window.MutationObserver){",
    "new MutationObserver(function(){if(pv.textContent===\"paired\"&&a.hidden)tick()})",
    ".observe(pv,{childList:true,characterData:true,subtree:true})}",
    "})()</script>"
);

/// True unless `ONE_ROOM_APPROVALS` is `off`, `0` or `no`.
pub fn enabled_by(v: Option<&str>) -> bool {
    !matches!(
        v.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("off") | Some("0") | Some("no") | Some("false")
    )
}

pub fn enabled() -> bool {
    enabled_by(std::env::var("ONE_ROOM_APPROVALS").ok().as_deref())
}

/// Add the Approvals link to a room page, right before `</body>`.
/// Unchanged when the page already links to /approvals or has no body end.
pub fn with_approvals_link(html: &str) -> String {
    let has_link = html.contains("href=\"/approvals\"")
        || html.contains("href='/approvals'")
        || html.contains(&format!("id=\"{ID}\""));
    if has_link {
        return html.to_string();
    }
    match html.rfind("</body>") {
        Some(at) => {
            let mut out = String::with_capacity(html.len() + SNIPPET.len());
            out.push_str(&html[..at]);
            out.push_str(SNIPPET);
            out.push_str(&html[at..]);
            out
        }
        None => html.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: &str = "<html><body><main><header class=\"hm-head\"><div class=\"hm-title\"></div></header></main></body></html>";

    #[test]
    fn adds_one_hidden_link_before_body_end() {
        let out = with_approvals_link(ROOM);
        assert_eq!(out.matches("href=\"/approvals\"").count(), 1);
        assert!(out.contains("id=\"g1-appr\" href=\"/approvals\" hidden>"));
        let link = out.find("id=\"g1-appr\"").unwrap();
        assert!(link > out.find("</main>").unwrap());
        assert!(out.ends_with("</body></html>"));
    }

    #[test]
    fn twice_is_once() {
        let once = with_approvals_link(ROOM);
        assert_eq!(with_approvals_link(&once), once);
    }

    #[test]
    fn page_with_its_own_link_is_untouched() {
        let own = "<body><a href=\"/approvals\">Approvals</a></body>";
        assert_eq!(with_approvals_link(own), own);
        let own2 = "<body><a href='/approvals'>A</a></body>";
        assert_eq!(with_approvals_link(own2), own2);
    }

    #[test]
    fn no_body_end_is_untouched() {
        assert_eq!(with_approvals_link("missing"), "missing");
    }

    #[test]
    fn script_only_reads_the_count() {
        assert!(SNIPPET.contains("fetch(\"/api/approvals/count\""));
        assert_eq!(SNIPPET.matches("fetch(").count(), 1);
        assert!(!SNIPPET.contains("POST"));
        assert!(!SNIPPET.contains("method"));
        // No outside hosts, no inline handlers.
        assert!(!SNIPPET.contains("http"));
        assert!(!SNIPPET.contains("onclick"));
    }

    #[test]
    fn switch() {
        assert!(enabled_by(None));
        assert!(enabled_by(Some("on")));
        assert!(enabled_by(Some("")));
        for v in ["off", "OFF", " 0", "no", "false"] {
            assert!(!enabled_by(Some(v)), "{v}");
        }
    }
}
