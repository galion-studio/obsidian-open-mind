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
    // Text-safe status colours and link/bar colour (overview); WCAG AA on
    // bg, surface and elevated in both themes.
    ("ok", "#89d185"),
    ("warn", "#e2c04a"),
    ("bad", "#f48771"),
    ("link", "#75beff"),
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
    ("ok", "#1f7a34"),
    ("warn", "#835c00"),
    ("bad", "#b42318"),
    ("link", "#005fb8"),
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
/* System-theme toggle (layout.rs theme_link). */
.th-l{display:none}
@media (prefers-color-scheme:light){.th-d{display:none}.th-l{display:inline-flex}}
"#;

/// Rules for the Desk landing (`pages/overview.rs`, classes `ov-*`).
/// Served as `/_ui/overview.<hash>.css` and linked only on `/console` (R1).
const LANDING: &str = r#"
.ov{max-width:76rem;margin:0 auto;padding:.5rem .5rem 6rem}
.ov a{color:var(--c-link);text-underline-offset:2px}
.ov-hero{padding:1.25rem 0 .5rem}
.ov-kicker{font-family:var(--mono);font-size:12px;letter-spacing:.14em;text-transform:uppercase;color:var(--c-muted)}
.ov-hero h1{font-size:clamp(2rem,6vw,3.5rem);line-height:1.08;letter-spacing:-.035em;margin-top:.75rem;max-width:18ch}
.ov-lede{margin-top:1rem;max-width:44rem;font-size:1.0625rem;line-height:1.65;color:var(--c-muted)}
.ov-rule{margin:1.75rem 0 0;padding:1.25rem 1.25rem 1.25rem 1.5rem;border-radius:var(--r-lg);border:1px solid var(--c-line);border-left:3px solid var(--c-link);background:var(--c-surface);max-width:48rem}
.ov-rule blockquote{margin:0}
.ov-rule blockquote p{font-size:1.25rem;line-height:1.45;letter-spacing:-.01em}
.ov-rule figcaption{margin-top:.75rem;color:var(--c-muted);font-size:.9375rem;line-height:1.6}
.ov-ref{margin-top:.5rem;font-family:var(--mono);font-size:12px;color:var(--c-muted);overflow-wrap:anywhere}
.ov-stats{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:.5rem;margin-top:1.75rem}
@media (min-width:720px){.ov-stats{grid-template-columns:repeat(6,minmax(0,1fr))}}
.ov-stats li{display:flex;flex-direction:column;gap:.25rem;min-width:0;border:1px solid var(--c-line);border-radius:var(--r-md);background:var(--c-surface);padding:.875rem 1rem}
.ov-big{font-size:1.75rem;font-weight:500;letter-spacing:-.03em;font-variant-numeric:tabular-nums}
.ov-label{font-size:.8125rem;line-height:1.35;color:var(--c-muted)}
.ov-h2{font-size:1.375rem;letter-spacing:-.02em;margin-top:3rem}
.ov-sub{margin-top:.375rem;max-width:44rem;color:var(--c-muted)}
.ov-map{display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,21rem),1fr));gap:.75rem;margin-top:1.25rem}
.ov-card,.ov-todo{display:flex;flex-direction:column;gap:.5rem;min-width:0;border:1px solid var(--c-line);border-radius:var(--r-lg);background:var(--c-surface);padding:1.125rem 1.25rem}
.ov-card-top{display:flex;align-items:flex-start;justify-content:space-between;gap:.75rem}
.ov-card h3,.ov-todo h3{font-size:1.0625rem;font-weight:600;letter-spacing:-.01em}
.ov-role{font-size:.9375rem;line-height:1.5}
.ov-status{font-family:var(--mono);font-size:12px;color:var(--c-muted)}
.ov-facts{display:flex;flex-direction:column;gap:.375rem;margin-top:.25rem;font-size:.875rem;line-height:1.5;color:var(--c-muted)}
.ov-facts li{position:relative;padding-left:.875rem;overflow-wrap:anywhere}
.ov-facts li::before{content:"";position:absolute;left:0;top:.62em;width:.3rem;height:.3rem;border-radius:999px;background:var(--c-muted)}
.ov-refs{margin-top:auto;padding-top:.625rem;border-top:1px solid var(--c-line);display:flex;flex-direction:column;gap:.125rem;font-size:12px;color:var(--c-muted);overflow-wrap:anywhere}
.ov-sha{color:var(--c-link)}
.ov-badge{flex-shrink:0;border:1px solid currentColor;border-radius:999px;padding:.125rem .625rem;font-family:var(--mono);font-size:11px;letter-spacing:.03em;text-transform:uppercase;white-space:nowrap}
.ov-live{color:var(--c-ok)}
.ov-built{color:var(--c-link)}
.ov-building{color:var(--c-muted)}
.ov-blocked{color:var(--c-warn)}
.ov-flow{display:grid;grid-template-columns:minmax(0,1fr);gap:1rem;margin-top:1.25rem;align-items:start}
@media (min-width:960px){.ov-flow{grid-template-columns:minmax(0,26rem) minmax(0,1fr)}}
.ov-fig{margin:0;border:1px solid var(--c-line);border-radius:var(--r-lg);background:var(--c-surface);padding:1rem}
.ov-fig figcaption{margin-top:.75rem;font-size:.875rem;line-height:1.55;color:var(--c-muted)}
.ov-side{display:flex;flex-direction:column;gap:1rem;min-width:0}
.ov-svg{display:block;width:100%;height:auto;max-width:28rem;margin:0 auto}
.ov-side .ov-svg{max-width:32rem;margin:0}
.ov-node{fill:var(--c-elevated);stroke:var(--c-line);stroke-width:1}
.ov-hi{stroke:var(--c-link);stroke-width:1.5}
.ov-group{fill:none;stroke:var(--c-line);stroke-dasharray:4 4}
.ov-t{fill:var(--c-fg);font-family:var(--sans);font-size:14px;font-weight:600}
.ov-ts{fill:var(--c-muted);font-family:var(--sans);font-size:12px}
.ov-mono,.ov-num{font-family:var(--mono)}
.ov-num{font-variant-numeric:tabular-nums}
.ov-on{fill:var(--c-accent-fg)}
.ov-arrow{fill:none;stroke:var(--c-muted);stroke-width:1.5}
.ov-stop{stroke:var(--c-warn);stroke-dasharray:4 3}
.ov-head{fill:var(--c-muted)}
.ov-track{fill:var(--c-elevated)}
.ov-bar-u{fill:var(--c-link)}
.ov-bar-t{fill:color-mix(in srgb,var(--c-link) 32%,transparent)}
.ov-bar-c{fill:var(--c-accent)}
.ov-notes{display:flex;flex-direction:column;gap:.25rem;padding:0 .25rem;font-family:var(--mono);font-size:12px;line-height:1.5;color:var(--c-muted)}
.ov-tablewrap{overflow-x:auto;border:1px solid var(--c-line);border-radius:var(--r-lg);background:var(--c-surface)}
.ov-table{width:100%;border-collapse:collapse;font-size:.875rem;font-variant-numeric:tabular-nums}
.ov-table caption{text-align:left;padding:.75rem 1rem .25rem;font-weight:600}
.ov-table th,.ov-table td{padding:.5rem 1rem;text-align:right;border-bottom:1px solid var(--c-line)}
.ov-table th:first-child,.ov-table td:first-child{text-align:left}
.ov-table thead th{font-weight:500;font-size:12px;color:var(--c-muted)}
.ov-table tfoot th,.ov-table tfoot td{font-weight:600;border-bottom:0}
@media (max-width:480px){.ov-table th,.ov-table td{padding:.5rem}}
.ov-todos{display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,20rem),1fr));gap:.75rem;margin-top:1.25rem}
.ov-todo p{font-size:.9375rem;line-height:1.55;color:var(--c-muted)}
.ov-todo p.ov-ref{font-size:12px}
@media (max-width:480px){.ov-fig{padding:.5rem}.ov-rule{padding:1rem 1rem 1rem 1.25rem}}
.ov-foot{display:flex;flex-direction:column;gap:.5rem;margin-top:3rem;padding-top:1.25rem;border-top:1px solid var(--c-line);font-size:.8125rem;color:var(--c-muted)}
.ov-foot ul{display:flex;flex-direction:column;gap:.125rem;font-family:var(--mono);font-size:12px}
.ov-foot a{display:inline-block;min-height:1.5rem}
"#;

/// Rules for the console-port-1 rooms (`/analytics`, `/arena`, `/bank`,
/// `/cache`). Kept apart so the other port slots append their own blocks.
const PORT1: &str = r#"
.tabs{display:flex;flex-wrap:wrap;gap:0 1rem;margin-top:1rem}
.tabs a{display:inline-flex;align-items:center;min-height:2.75rem;font-size:.875rem;color:var(--c-muted);text-decoration:none}
.tabs a[aria-current=page]{color:var(--c-fg)}
.arena .who{display:flex;flex-direction:column;min-width:0;flex:1}
.arena .ans{margin-top:.25rem;white-space:pre-wrap;font-size:13px;line-height:1.4}
.score{color:var(--c-accent);font-variant-numeric:tabular-nums}
.rows li.cash{justify-content:flex-start;gap:.25rem .75rem}
.amt{width:7rem;flex-shrink:0;text-align:right;font-size:12px;color:var(--c-muted)}
.rows li.cash>span:last-child:not(:first-child){text-align:left}
.rows li.stack{display:block}
.rows li.disk{flex-wrap:nowrap;justify-content:flex-start}
.disk meter{flex:1;min-width:2rem;height:.375rem}
.fg{color:var(--c-fg)}
.page header .k a{gap:.375rem;font-size:11px;color:var(--c-muted)}
"#;

/// console-port-2 rooms (`/context`, `/gateway`, `/guardian`, `/hermes`). Kept
/// apart so the five console-port branches do not edit the same lines.
const PORT2: &str = r#"
.sec{margin-top:1.5rem;min-width:0}
.sec h2{font-size:13px;color:var(--c-subtle)}
.sec h2 a{color:inherit}
.small{margin-top:.5rem;font-size:13px;color:var(--c-muted);overflow-wrap:anywhere}
.t-bad{color:var(--c-bad)}
.road{list-style:none;margin:.75rem 0 0;padding:0;border-left:1px solid var(--c-line)}
.road li{position:relative;padding:.75rem 0 .75rem 1rem;min-width:0}
.road li::before{content:"";position:absolute;left:-3px;top:1rem;width:6px;height:6px;background:var(--c-fg)}
.road p{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:12px;color:var(--c-muted)}
.road p:first-child{font-size:13px;color:var(--c-fg)}
.bars{margin-top:.75rem}
.bars li{display:flex;align-items:center;gap:.75rem;min-height:2.75rem;border-bottom:1px solid var(--c-line);padding:.375rem 0;min-width:0}
.bars .n{width:4.5rem;flex-shrink:0;text-align:right;font-size:12px;color:var(--c-muted)}
.bars .ell{flex:1 1 8rem;font-size:13px}
.bar{flex:1;min-width:0;height:.375rem;-webkit-appearance:none;appearance:none;border:0;background:var(--c-line);color:var(--c-fg)}
.bar::-webkit-progress-bar{background:var(--c-line)}
.bar::-webkit-progress-value{background:var(--c-fg)}
.bar::-moz-progress-bar{background:var(--c-fg)}
.eps{display:flex;flex-direction:column;margin-top:1rem}
.eps a{display:flex;align-items:center;justify-content:space-between;gap:1rem;min-height:2.75rem;border-bottom:1px solid var(--c-line);text-decoration:none;color:var(--c-muted)}
.eps a[aria-current=true]{color:var(--c-fg)}
.json{margin:1rem 0 0;max-height:28rem;overflow:auto;white-space:pre-wrap;overflow-wrap:anywhere;font-family:var(--mono);font-size:12px;line-height:1.6;color:var(--c-fg)}
.pin{align-items:center}
.pin label{font-size:13px;color:var(--c-muted)}
.pin input{min-height:2.75rem;min-width:0;width:10rem;border-radius:var(--r-md);border:1px solid var(--c-line);background:var(--c-surface);padding:0 .75rem}
.scroll{max-height:16rem;overflow-y:auto}
.rows a[aria-current=page]{font-weight:600}
.msgs{margin-top:.75rem}
.msgs li{border-bottom:1px solid var(--c-line);padding:.5rem 0;min-width:0;font-size:12px}
.say{margin:.25rem 0 0;white-space:pre-wrap;overflow-wrap:anywhere;font-family:var(--sans);font-size:13px;line-height:1.5;color:var(--c-fg)}
.tool{color:var(--c-muted);overflow-wrap:anywhere}
"#;

/// console-port-3 rooms (`/instances`, `/keys`, `/markers`). Kept apart (and
/// prefixed `p3-`) so the five console-port branches do not edit the same lines.
const PORT3: &str = r#"
.p3-sec{margin-top:1.5rem;min-width:0}
.p3-sec h2{font-size:13px;font-weight:400;color:var(--c-subtle);margin:0}
.p3-h{border-bottom:1px solid var(--c-line);padding-bottom:.5rem}
.p3-refresh a{font-size:13px;color:var(--c-link)}
.p3-bad{color:var(--c-offline)}
.p3-list{display:flex;flex-direction:column;margin-top:.5rem;min-width:0}
.p3-card{border-bottom:1px solid var(--c-line);padding:.75rem 0;min-width:0}
.p3-top{display:flex;align-items:center;justify-content:space-between;gap:1rem;min-width:0;min-height:2.75rem}
.p3-name{color:var(--c-fg);font-size:15px;min-width:0}
.p3-sub{margin-top:.25rem;font-size:13px;color:var(--c-muted);overflow-wrap:anywhere}
.p3-pct{font-family:var(--mono);font-size:13px;color:var(--c-fg)}
.p3-bar{display:block;width:100%;height:.375rem;margin-top:.5rem;-webkit-appearance:none;appearance:none;border:0;background:var(--c-line);color:var(--c-fg)}
.p3-bar::-webkit-progress-bar{background:var(--c-line)}
.p3-bar::-webkit-progress-value{background:var(--c-fg)}
.p3-bar::-moz-progress-bar{background:var(--c-fg)}
.p3-term{margin:0;max-height:16rem;overflow:auto;white-space:pre-wrap;word-break:break-all;border-bottom:1px solid var(--c-line);padding:.75rem 0;font-family:var(--mono);font-size:13px;line-height:1.6;color:var(--c-muted)}
.p3-copy{margin-top:1.25rem;border-bottom:1px solid var(--c-line);padding-bottom:1rem;min-width:0}
.p3-pre{margin:.5rem 0 0;overflow:auto;white-space:pre-wrap;word-break:break-all;font-family:var(--mono);font-size:13px;line-height:1.6;color:var(--c-muted);user-select:all;-webkit-user-select:all}
.p3-mint{display:flex;flex-direction:column;margin-top:1rem}
.p3-field{display:flex;align-items:center;justify-content:space-between;gap:1rem;min-height:2.75rem;border-bottom:1px solid var(--c-line);font-size:15px}
.p3-field input{flex:1;min-width:0;height:2.75rem;border:0;background:transparent;color:var(--c-fg);text-align:right;font:inherit}
.p3-scopes{display:flex;flex-wrap:wrap;gap:.25rem 1.25rem;border:0;border-bottom:1px solid var(--c-line);margin:0;padding:.5rem 0}
.p3-scopes label{display:inline-flex;align-items:center;gap:.375rem;min-height:2.5rem;font-size:14px;color:var(--c-fg)}
.p3-fresh{border-bottom:1px solid var(--c-line);padding:.75rem 0;min-width:0}
.p3-key{display:flex;align-items:center;justify-content:space-between;gap:1rem;min-height:2.75rem;border-bottom:1px solid var(--c-line);padding:.5rem 0;min-width:0}
.p3-kn{display:flex;flex-direction:column;min-width:0}
.p3-mk{display:flex;flex-wrap:wrap;align-items:baseline;justify-content:space-between;gap:.75rem;border-bottom:1px solid var(--c-line);padding:.75rem 0;min-width:0}
.p3-mkl{display:flex;flex-wrap:wrap;align-items:center;gap:.5rem;min-width:0}
.p3-note{font-size:14px;overflow-wrap:anywhere;min-width:0}
"#;

/// console-port-4 rooms (`/md/$name`, `/memory`, `/mesh`). Kept apart (own
/// `p4-` names) so the five console-port branches do not edit the same lines.
const PORT4: &str = r#"
.p4-sec{margin-top:1.5rem;min-width:0}
.p4-sec h2{font-size:13px;color:var(--c-subtle)}
.p4-sec h2 a{color:inherit}
.p4-small{margin-top:.5rem;font-size:13px;color:var(--c-fg);overflow-wrap:anywhere}
.p4-bad{color:var(--c-bad)}
.p4-n{width:4.5rem;flex-shrink:0;text-align:right;font-size:12px;color:var(--c-muted)}
.p4-line{font-size:15px;color:var(--c-fg);overflow-wrap:anywhere}
.p4-pre{margin:.5rem 0 0;max-width:100%;overflow:auto;white-space:pre-wrap;overflow-wrap:anywhere;font-family:var(--mono);font-size:12px;line-height:1.6;color:var(--c-muted)}
.p4-md{font-size:14px;color:var(--c-fg);padding-bottom:3rem}
.p4-tabs{position:sticky;top:0;display:flex;gap:.5rem;padding:.75rem 0;background:var(--c-bg)}
.p4-tabs a{display:inline-flex;align-items:center;min-height:2.75rem;border:1px solid var(--c-line);border-radius:999px;padding:0 .875rem;color:var(--c-fg);text-decoration:none;background:var(--c-surface)}
.p4-tabs a[aria-current=page]{border-color:var(--c-fg)}
"#;

/// Base sheet: tokens + RULES + PORT1-4. Linked on every page. Budget 20 KiB.
pub fn sheet() -> String {
    build_sheet(
        "/* Galion console UI (base). Generated by crates/console-ui/src/css.rs. SPDX-License-Identifier: MIT */\n",
        &[RULES, PORT1, PORT2, PORT3, PORT4],
        true,
    )
}

/// Overview sheet: `ov-*` landing rules. Linked only on `/console`. Budget 8 KiB.
pub fn overview_sheet() -> String {
    build_sheet(
        "/* Galion console UI (overview). Generated by crates/console-ui/src/css.rs. SPDX-License-Identifier: MIT */\n",
        &[LANDING],
        false,
    )
}

/// `/hermes` room rules (`hm-*`), linked only on `/hermes`. Budget 10 KiB.
pub const HERMES: &str = include_str!("assets/hermes.css");

/// The `/hermes` sheet (no tokens: the base sheet defines them).
pub fn hermes_sheet() -> String {
    build_sheet(
        "/* Galion console UI (hermes). Source: crates/console-ui/src/assets/hermes.css. SPDX-License-Identifier: MIT */\n",
        &[HERMES],
        false,
    )
}

fn build_sheet(header: &str, blocks: &[&str], with_tokens: bool) -> String {
    let mut out = String::with_capacity(16 * 1024);
    out.push_str(header);
    if with_tokens {
        let light = vars(LIGHT, "c-");
        out.push_str(&format!(
            ":root{{color-scheme:dark;{}{}--sans:{FONT_SANS};--mono:{FONT_MONO}}}\n",
            vars(DARK, "c-"),
            vars(RADII, "r-")
        ));
        // Light palette: forced-light outside media; system-light via :where()
        // so the media query does not raise specificity (R1).
        out.push_str(&format!(
            "html[data-theme=light]{{color-scheme:light;{light}}}\n"
        ));
        out.push_str(&format!(
            "@media (prefers-color-scheme:light){{:where(html:not([data-theme])){{color-scheme:light;{light}}}}}\n"
        ));
    }
    for block in blocks {
        for line in block.lines().filter(|l| !l.trim().is_empty()) {
            out.push_str(line.trim());
            out.push('\n');
        }
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

/// Class names defined in the base + overview sheets (for the class-coverage test).
pub fn defined_classes() -> Vec<String> {
    let all = format!("{RULES}{LANDING}{PORT1}{PORT2}{PORT3}{PORT4}{HERMES}");
    let mut out = Vec::new();
    let b = all.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'.' && i + 1 < b.len() && (b[i + 1].is_ascii_alphabetic()) {
            let s = i + 1;
            let mut e = s;
            while e < b.len() && (b[e].is_ascii_alphanumeric() || b[e] == b'-' || b[e] == b'_') {
                e += 1;
            }
            out.push(all[s..e].to_string());
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
        // R1: base sheet <= 20 KiB (overview is a separate cached file).
        assert!(
            s.len() <= 20 * 1024,
            "base sheet {} B exceeds 20 KiB",
            s.len()
        );
        assert!(s.contains(
            "@media (prefers-color-scheme:light){:where(html:not([data-theme])){color-scheme:light;--c-bg:#f6f6f4"
        ));
        assert!(s.contains("--c-bg:#1e1e1e"));
        assert!(s.contains("html[data-theme=light]{color-scheme:light;--c-bg:#f6f6f4"));
        assert!(!s.contains("@import"));
        assert!(
            !s.contains(".ov-card"),
            "overview rules must not ship in the base sheet"
        );
        assert!(defined_classes().contains(&"dock".to_string()));
        assert!(defined_classes().contains(&"ov-card".to_string()));
        assert!(defined_classes().contains(&"th-l".to_string()));
        assert_eq!(s.matches('{').count(), s.matches('}').count());
    }

    #[test]
    fn overview_sheet_is_small_and_balanced() {
        let s = overview_sheet();
        assert!(
            s.len() <= 8 * 1024,
            "overview sheet {} B exceeds 8 KiB",
            s.len()
        );
        assert!(s.contains(".ov-card,"));
        assert!(
            !s.contains(":root{"),
            "overview sheet has no tokens (base provides them)"
        );
        assert!(!s.contains(".dock{"), "room chrome stays in the base sheet");
        assert_eq!(s.matches('{').count(), s.matches('}').count());
    }
}
