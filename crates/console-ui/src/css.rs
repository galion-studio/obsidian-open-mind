// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The console stylesheet, generated from Rust.
//!
//! Replaces Tailwind v4 (a JS engine) with a hand-written sheet: the design
//! tokens from console-ide `src/styles.css` (`@theme`) are Rust constants,
//! the component rules are written once here and only cover the classes the
//! Rust pages emit. The sheet is built once at start and served under a
//! content-hashed name with `immutable` caching.

/// Dark tokens (console-ide `@theme`).
pub const DARK: &[(&str, &str)] = &[
    ("bg", "#1e1e1e"),
    ("surface", "#252526"),
    ("elevated", "#2d2d30"),
    ("fg", "#d4d4d4"),
    ("muted", "#9d9d9d"),
    ("subtle", "#6a6a6a"),
    ("line", "#3c3c3c"),
    ("accent", "#007acc"),
    ("accent-fg", "#ffffff"),
    ("online", "#89d185"),
    ("offline", "#f14c4c"),
    ("held", "#cca700"),
    ("ring", "#007acc"),
    ("frost", "#007acc"),
    ("ice", "#569cd6"),
    ("winter", "#1e1e1e"),
];

/// Light overrides (console-ide `html[data-theme="light"]` and `LIGHT`).
pub const LIGHT: &[(&str, &str)] = &[
    ("bg", "#f6f6f4"),
    ("surface", "#ffffff"),
    ("elevated", "#efefec"),
    ("fg", "#1a1a1a"),
    ("muted", "#5e5e5e"),
    ("subtle", "#8a8a86"),
    ("line", "#e2e2de"),
    ("winter", "#f6f6f4"),
];

pub const RADII: &[(&str, &str)] = &[
    ("xs", "4px"),
    ("sm", "8px"),
    ("md", "12px"),
    ("lg", "20px"),
    ("xl", "28px"),
];

pub const FONT_SANS: &str = "\"IBM Plex Sans\",\"Segoe UI\",system-ui,sans-serif";
pub const FONT_MONO: &str =
    "\"IBM Plex Mono\",\"Cascadia Code\",\"JetBrains Mono\",ui-monospace,Menlo,Consolas,monospace";

/// Theme colour for `<meta name="theme-color">`.
pub fn theme_color(light: bool) -> &'static str {
    if light {
        "#f6f6f4"
    } else {
        "#1e1e1e"
    }
}

fn vars(pairs: &[(&str, &str)], prefix: &str) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("--{prefix}{k}:{v};"))
        .collect()
}

/// Component rules. Class names are short and semantic; each one is used by
/// a page in `pages/` or by `layout.rs` (checked by a test).
const RULES: &str = r#"
*,*::before,*::after{box-sizing:border-box}
html,body{margin:0;background:var(--c-bg);color:var(--c-fg);font-family:var(--sans);-webkit-font-smoothing:antialiased;overflow-x:clip;max-width:100%}
body{min-height:100dvh;font-size:1rem;line-height:1.5}
a{color:inherit}
h1,h2,h3{text-wrap:balance;margin:0;font-weight:500}
p{text-wrap:pretty;margin:0}
ul{list-style:none;margin:0;padding:0}
button,input{font:inherit;color:inherit}
:focus-visible{outline:2px solid var(--c-accent);outline-offset:2px}
.sr-only{position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0}
.sr-only:focus{position:static;width:auto;height:auto;margin:0;clip:auto;white-space:normal}
.i{display:inline-block;flex-shrink:0;width:1.25rem;height:1.25rem}
.i-sm{width:.875rem;height:.875rem}
.i-md{width:1.5rem;height:1.5rem}
.i-lg{width:1.75rem;height:1.75rem}
.mono{font-family:var(--mono)}
.k{font-family:var(--mono);font-size:11px;letter-spacing:.18em;text-transform:uppercase;color:var(--c-subtle)}
.muted{color:var(--c-muted)}
.subtle{color:var(--c-subtle)}
.ell{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.app{display:flex;flex-direction:column;height:100dvh;max-height:100dvh;overflow:hidden}
.top{flex-shrink:0;border-bottom:1px solid var(--c-line);background:var(--c-bg);padding-top:env(safe-area-inset-top);z-index:40}
.top-row{display:flex;align-items:center;gap:.5rem;min-height:3rem;padding:.25rem .5rem .25rem .75rem}
.top-title{min-width:0;flex:1}
.top-title p:first-child{display:flex;align-items:center;gap:.25rem;font-size:.875rem;font-weight:500;letter-spacing:-.01em}
.top-title p+p{font-size:.75rem;line-height:1.35;color:var(--c-subtle);display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden}
.origin{display:flex;align-items:center;gap:.375rem;flex-shrink:0;font-family:var(--mono);font-size:10px;text-transform:uppercase;color:var(--c-muted)}
.iconbtn{display:inline-flex;align-items:center;justify-content:center;width:2.75rem;height:2.75rem;flex-shrink:0;color:var(--c-fg);border-radius:var(--r-md);text-decoration:none;cursor:pointer;list-style:none}
.iconbtn::-webkit-details-marker{display:none}
.body{position:relative;min-height:0;flex:1;display:flex}
.main{flex:1;min-height:0;overflow-y:auto;overscroll-behavior-y:contain;padding:.75rem}
.menu>summary{list-style:none}
.menu .i-close{display:none}
.menu[open] .i-close{display:inline-block}
.menu[open] .i-open{display:none}
.sheet{position:fixed;left:0;right:0;top:calc(3.6rem + env(safe-area-inset-top));bottom:calc(3.6rem + env(safe-area-inset-bottom));z-index:30;overflow-y:auto;background:var(--c-bg);padding:1rem}
.rooms{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));row-gap:1rem;margin-top:.75rem}
@media (min-width:640px){.rooms{grid-template-columns:repeat(6,minmax(0,1fr))}}
.room{display:flex;flex-direction:column;align-items:center;min-width:0;min-height:2.75rem;padding:.25rem;text-decoration:none;color:var(--c-muted);font-size:.75rem}
.room .tile{display:flex;align-items:center;justify-content:center;width:3rem;height:3rem;border-radius:1rem;border:1px solid var(--c-line);background:var(--c-surface);color:var(--c-fg);margin-bottom:.375rem}
.room[aria-current=page]{color:var(--c-fg)}
.room[aria-current=page] .tile{background:var(--c-elevated)}
.cold{margin-top:1.5rem}
.dock{flex-shrink:0;border-top:1px solid var(--c-line);background:var(--c-bg);padding-bottom:env(safe-area-inset-bottom);z-index:40}
.dock ul{display:grid;grid-template-columns:repeat(7,minmax(0,1fr));max-width:32rem;margin:0 auto}
.dock a{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:.25rem;min-height:3.5rem;font-size:10px;color:var(--c-muted);text-decoration:none}
.dock a[aria-current=page]{color:var(--c-fg)}
.dot{display:inline-block;width:.375rem;height:.375rem;border-radius:999px;flex-shrink:0;background:var(--c-subtle)}
.dot.online{background:var(--c-online)}
.dot.offline{background:var(--c-offline)}
.dot.held{background:var(--c-held)}
@media (prefers-reduced-motion:no-preference){.dot.online{animation:pulse 1.618s cubic-bezier(.23,1,.32,1) infinite}}
@keyframes pulse{0%,100%{opacity:1;transform:scale(1)}50%{opacity:.35;transform:scale(.72)}}
.badge{display:inline-flex;align-items:center;gap:.375rem;max-width:100%;min-width:0;border-radius:999px;padding:.125rem .625rem;font-family:var(--mono);font-size:11px;letter-spacing:.025em;text-transform:uppercase}
.badge.online{background:color-mix(in srgb,var(--c-online) 12%,transparent);color:var(--c-online)}
.badge.held{background:color-mix(in srgb,var(--c-held) 12%,transparent);color:var(--c-held)}
.badge.offline{background:color-mix(in srgb,var(--c-offline) 12%,transparent);color:var(--c-offline)}
.btn{display:inline-flex;align-items:center;justify-content:center;gap:.5rem;height:2.75rem;min-height:2.75rem;padding:0 1rem;border-radius:var(--r-md);font-size:.875rem;font-weight:500;white-space:nowrap;cursor:pointer;border:1px solid transparent;text-decoration:none;transition:opacity .15s,transform .15s}
.btn:active{transform:scale(.96)}
.btn.primary{background:var(--c-accent);color:var(--c-accent-fg)}
.btn.ghost{background:transparent;color:var(--c-fg);border-color:var(--c-line)}
.btn.ghost:hover{background:var(--c-elevated)}
.btn:disabled{opacity:.4;cursor:default}
.page{margin:0 auto;width:100%;min-width:0;overflow:hidden;padding:.25rem .25rem 6rem}
.w42{max-width:42rem}
.w46{max-width:46rem}
.page h1{display:flex;align-items:center;gap:.5rem;min-width:0;font-size:1.5rem;letter-spacing:-.04em;margin-top:.5rem}
.page h2{font-size:13px;font-weight:400;color:var(--c-subtle);margin:1rem 0 .5rem}
.lede{font-size:15px;line-height:1.6;color:var(--c-muted);margin-top:.5rem}
.page header a{display:inline-flex;align-items:center;min-height:2.75rem;color:var(--c-fg);text-decoration:none;font-size:15px}
.facts{margin-top:1.5rem;display:flex;flex-direction:column;gap:.5rem}
.fact{display:flex;flex-wrap:wrap;align-items:center;justify-content:space-between;gap:.25rem .75rem;min-height:2.75rem;min-width:0;overflow:hidden;border-radius:var(--r-md);border:1px solid var(--c-line);background:var(--c-surface);padding:.25rem .75rem}
.fact>span:first-child{font-size:.875rem}
.fact>span:last-child{display:flex;align-items:center;gap:.5rem;min-width:0;max-width:100%;overflow:hidden;font-family:var(--mono);font-size:11px;color:var(--c-muted)}
.actions{display:flex;flex-wrap:wrap;gap:.5rem;margin-top:1.5rem}
.out{margin:1rem 0 0;max-height:12rem;overflow:auto;white-space:pre-wrap;word-break:break-all;border-radius:var(--r-lg);border:1px solid var(--c-line);background:var(--c-bg);padding:.75rem;font-family:var(--mono);font-size:11px;color:var(--c-muted)}
.svc{min-width:0}
.svc>div{display:flex;flex-wrap:wrap;align-items:center;justify-content:space-between;gap:.25rem .75rem;min-height:2.75rem;min-width:0;overflow:hidden;border-bottom:1px solid var(--c-line);padding:.25rem 0}
.svc .meta{display:flex;flex-wrap:wrap;gap:.25rem .75rem;color:var(--c-muted)}
.svc .where{color:var(--c-muted);text-align:right}
.note{margin-top:.75rem;font-size:13px;color:var(--c-subtle)}
.board{max-width:48rem;margin:0 auto;padding:1.5rem 1rem 4rem}
.board-top{display:flex;align-items:center;justify-content:space-between;margin-bottom:1rem}
.board-top>div{display:flex;align-items:center;gap:.75rem}
.board-top a{min-height:2.75rem;font-size:.875rem;color:var(--c-muted);text-decoration:none}
.board h1{font-size:2rem;letter-spacing:-.03em;margin-top:.5rem}
.board .lede{font-size:1rem}
.hosts{margin-top:2.5rem;border:1px solid var(--c-line);border-radius:var(--r-xl);overflow:hidden}
.hosts li{display:flex;align-items:center;justify-content:space-between;gap:.75rem;background:var(--c-surface);padding:1rem 1.25rem}
.hosts li+li{border-top:1px solid var(--c-line)}
.hosts .state{display:flex;align-items:center;gap:.5rem;font-family:var(--mono);font-size:11px;text-transform:uppercase;letter-spacing:.05em;color:var(--c-muted)}
.hosts p+p{font-size:.75rem;color:var(--c-muted)}
.gate{max-width:24rem;margin:0 auto;min-height:100dvh;display:flex;flex-direction:column;justify-content:center;padding:1.25rem}
.gate h1{font-size:1.9rem;letter-spacing:-.03em;margin-top:.5rem}
.gate input{width:100%;height:3rem;margin-top:1.5rem;border:1px solid var(--c-line);background:var(--c-surface);border-radius:var(--r-md);padding:0 .75rem;font-family:var(--mono)}
.gate .btn{width:100%;margin-top:.75rem;height:3rem}
.err{color:var(--c-offline);margin-top:.5rem;font-size:.875rem}
.w68{max-width:67.86rem}
.tiny{font-size:11px}
.badge.neutral{background:var(--c-elevated);color:var(--c-muted);border:1px solid var(--c-line)}
.badge.fork{background:var(--c-elevated);color:var(--c-subtle);border:1px solid var(--c-line)}
.badge.chill{background:color-mix(in srgb,var(--c-frost) 10%,transparent);color:var(--c-frost)}
.rows{display:flex;flex-direction:column;margin-top:1rem}
.rows li{display:flex;flex-wrap:wrap;align-items:center;justify-content:space-between;gap:.25rem 1rem;min-height:2.75rem;border-bottom:1px solid var(--c-line);padding:.375rem 0;min-width:0;overflow-wrap:anywhere}
.rows li>span:last-child:not(:first-child){text-align:right}
.rows .dns>span{flex:0 1 auto}
.cards{margin-top:1.5rem;border:1px solid var(--c-line);border-radius:var(--r-xl);overflow:hidden}
.cards li{display:flex;flex-wrap:wrap;align-items:flex-start;justify-content:space-between;gap:.5rem 1rem;background:var(--c-surface);padding:1rem 1.25rem;min-width:0}
.cards li+li{border-top:1px solid var(--c-line)}
.cards li.row{align-items:baseline}
.cards p.muted{margin-top:.25rem;font-size:.875rem}
.name{display:flex;flex-wrap:wrap;align-items:center;gap:.5rem;min-width:0;overflow-wrap:anywhere}
.name .mono{font-size:.875rem}
.filter{display:flex;gap:.5rem;margin-top:1.5rem;max-width:28rem}
.filter input{flex:1;min-width:0;height:2.75rem;border:1px solid var(--c-line);background:var(--c-surface);border-radius:var(--r-md);padding:0 .75rem;font-family:var(--mono);font-size:.875rem}
.steps{margin:1rem 0 0;padding:0;list-style:none}
.steps li{display:flex;gap:1rem;border-bottom:1px solid var(--c-line);padding:.75rem 0;font-size:15px;line-height:1.6}
.page h2.split{display:flex;justify-content:space-between;gap:.75rem;margin-top:1.5rem}
.plain{margin-top:.5rem}
.plain summary{cursor:pointer;min-height:2.75rem;display:flex;align-items:center;font-size:.875rem;color:var(--c-muted)}
.code{margin:.75rem 0 0;overflow:auto;border-radius:var(--r-lg);border:1px solid color-mix(in srgb,var(--c-frost) 15%,transparent);background:var(--c-winter);padding:1rem;font-family:var(--mono);font-size:12px;line-height:1.6;color:color-mix(in srgb,var(--c-frost) 90%,transparent)}
.cold-file{margin-top:1.5rem;opacity:.8}
.cold-file h2{font-size:1rem;font-weight:500;color:var(--c-fg)}
.frost{color:var(--c-frost)}
.ice{color:var(--c-ice)}
.page header a.frost{color:var(--c-frost)}
.ice-cards{background:var(--c-winter);border-color:color-mix(in srgb,var(--c-frost) 15%,transparent)}
.ice-cards li{background:transparent}
.ice-cards li+li{border-color:color-mix(in srgb,var(--c-frost) 10%,transparent)}
.ice-cards .name span:first-child{font-weight:500}
.hib{position:relative;display:flex;flex-direction:column;min-height:100dvh;overflow:hidden;background:radial-gradient(ellipse 70% 50% at 80% -10%,color-mix(in oklab,var(--c-frost) 18%,transparent),transparent 55%)}
.hib header{display:flex;align-items:center;justify-content:space-between;padding:max(1rem,env(safe-area-inset-top)) 1.25rem 1rem}
.hib-mark{width:1.75rem;height:1.75rem;border-radius:8px;border:1px solid var(--c-line);background:linear-gradient(160deg,var(--c-frost),var(--c-fg) 55%,var(--c-bg))}
.hib main{margin:0 auto;display:flex;flex:1;flex-direction:column;justify-content:center;width:100%;max-width:36rem;padding:0 1.25rem 4rem}
.hib h1{margin-top:.5rem;font-size:1.875rem;letter-spacing:-.03em}
@media (min-width:640px){.hib h1{font-size:2.25rem}}
"#;

/// The whole sheet.
pub fn sheet() -> String {
    let mut out = String::with_capacity(RULES.len() + 1024);
    out.push_str("/* Galion console UI. Generated by crates/console-ui/src/css.rs. SPDX-License-Identifier: MIT */\n");
    out.push_str(&format!(
        ":root{{color-scheme:dark;{}{}--sans:{FONT_SANS};--mono:{FONT_MONO}}}\n",
        vars(DARK, "c-"),
        vars(RADII, "r-")
    ));
    out.push_str(&format!(
        "html[data-theme=light]{{color-scheme:light;{}}}\n",
        vars(LIGHT, "c-")
    ));
    for line in RULES.lines().filter(|l| !l.trim().is_empty()) {
        out.push_str(line.trim());
        out.push('\n');
    }
    out
}

/// FNV-1a 64; used for the cache-busting file name and the ETag.
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Class names defined in [`RULES`] (for the "every emitted class exists" test).
pub fn defined_classes() -> Vec<String> {
    let mut out = Vec::new();
    let b = RULES.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'.' && i + 1 < b.len() && (b[i + 1].is_ascii_alphabetic()) {
            let s = i + 1;
            let mut e = s;
            while e < b.len() && (b[e].is_ascii_alphanumeric() || b[e] == b'-' || b[e] == b'_') {
                e += 1;
            }
            out.push(RULES[s..e].to_string());
            i = e;
        } else {
            i += 1;
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_is_small_and_balanced() {
        let s = sheet();
        assert!(s.len() < 16 * 1024, "{}", s.len());
        assert_eq!(s.matches('{').count(), s.matches('}').count());
        assert!(s.contains("--c-bg:#1e1e1e"));
        assert!(s.contains("html[data-theme=light]{color-scheme:light;--c-bg:#f6f6f4"));
        assert!(!s.contains("@import"));
        assert!(defined_classes().contains(&"dock".to_string()));
    }
}
