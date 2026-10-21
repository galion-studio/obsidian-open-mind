// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The Desk landing (`/console`): a visual overview of the whole project.
//!
//! Layout only. Every word and number comes from [`crate::overview`]; the
//! diagrams are inline SVG drawn here from those numbers (no JavaScript, no
//! inline `style`, so the strict CSP holds). SVG colours come from classes in
//! the sheet, so dark and light both work.

use crate::html::esc;
use crate::overview::{
    self as ov, OwnerItem, Plan, Sector, State, OWNER_ITEMS, PLANS, PLAN_TOTAL, PROCESS, RULE,
    SECTORS, SOURCES, STATS,
};

fn state_class(s: State) -> &'static str {
    match s {
        State::Live => "ov-live",
        State::Built => "ov-built",
        State::Building => "ov-building",
        State::Blocked => "ov-blocked",
    }
}

fn card(s: &Sector, routes_in_rust: usize) -> String {
    let mut facts: String = s
        .facts
        .iter()
        .map(|f| format!("<li>{}</li>", esc(f)))
        .collect();
    if s.id == "console" {
        facts.push_str(&format!(
            "<li>This build: {routes_in_rust} of 31 routes in Rust, including this overview</li>"
        ));
    }
    let refs: String = s
        .refs
        .iter()
        .map(|(b, sha)| {
            if sha.is_empty() {
                format!("<li><span class=\"mono\">{}</span></li>", esc(b))
            } else {
                format!(
                    "<li><span class=\"mono\">{}</span> <span class=\"mono ov-sha\">{}</span></li>",
                    esc(b),
                    esc(sha)
                )
            }
        })
        .collect();
    format!(
        "<li class=\"ov-card\" id=\"sector-{id}\"><div class=\"ov-card-top\"><h3>{name}</h3>\
<span class=\"ov-badge {cls}\">{word}</span></div>\
<p class=\"ov-role\">{role}</p><p class=\"ov-status\">{status}</p>\
<ul class=\"ov-facts\">{facts}</ul><ul class=\"ov-refs\" aria-label=\"Branches\">{refs}</ul></li>",
        id = esc(s.id),
        name = esc(s.name),
        cls = state_class(s.state),
        word = esc(s.state.word()),
        role = esc(s.role),
        status = esc(s.status),
    )
}

/// Vertical flow: owner -> Hermes -> 7 planners (bars: useful vs target
/// agents) -> agents <-> queue/auto-restart.
pub fn flow_svg() -> String {
    let max = PLANS.iter().map(|p| p.target).max().unwrap_or(1).max(1);
    let bar_x = 132.0_f64;
    let bar_w = 150.0_f64;
    let mut rows = String::new();
    for (i, p) in PLANS.iter().enumerate() {
        let y = 202 + i as u32 * 28;
        let tw = f64::from(p.target) / f64::from(max) * bar_w;
        let uw = f64::from(p.useful) / f64::from(max) * bar_w;
        rows.push_str(&format!(
            "<text class=\"ov-ts\" x=\"22\" y=\"{ty}\">{name}</text>\
<rect class=\"ov-bar-t\" x=\"{bar_x}\" y=\"{by}\" width=\"{tw:.1}\" height=\"12\" rx=\"3\"/>\
<rect class=\"ov-bar-u\" x=\"{bar_x}\" y=\"{by}\" width=\"{uw:.1}\" height=\"12\" rx=\"3\"/>\
<text class=\"ov-ts ov-num\" x=\"340\" y=\"{ty}\" text-anchor=\"end\">{u}/{t}</text>",
            ty = y + 14,
            by = y + 4,
            name = esc(p.sector),
            u = p.useful,
            t = p.target,
        ));
    }
    let hermes = SECTORS.iter().find(|s| s.id == "hermes");
    let (hb, hs) = hermes
        .and_then(|s| s.refs.first())
        .map(|(b, s)| (*b, *s))
        .unwrap_or(("", ""));
    format!(
        "<svg class=\"ov-svg\" viewBox=\"0 0 360 580\" role=\"img\" aria-labelledby=\"flow-t flow-d\">\
<title id=\"flow-t\">How the swarm runs</title>\
<desc id=\"flow-d\">Slavik decides. Hermes, the coordinator, hands work to 7 sector planners, which plan {tasks} tasks for {target} target agents ({useful} useful). \
About {lo} to {hi} agents hold at once. Agents stopped by the request limit go back to the queue and restart automatically.</desc>\
<defs><marker id=\"ov-ah\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" markerHeight=\"7\" orient=\"auto-start-reverse\">\
<path class=\"ov-head\" d=\"M0 0L10 5L0 10z\"/></marker></defs>\
<rect class=\"ov-node\" x=\"70\" y=\"8\" width=\"220\" height=\"48\" rx=\"12\"/>\
<text class=\"ov-t\" x=\"180\" y=\"29\" text-anchor=\"middle\">Slavik · owner</text>\
<text class=\"ov-ts\" x=\"180\" y=\"46\" text-anchor=\"middle\">decides, signs HADES approvals</text>\
<line class=\"ov-arrow\" x1=\"180\" y1=\"56\" x2=\"180\" y2=\"80\" marker-end=\"url(#ov-ah)\"/>\
<rect class=\"ov-node ov-hi\" x=\"40\" y=\"82\" width=\"280\" height=\"64\" rx=\"12\"/>\
<text class=\"ov-t\" x=\"180\" y=\"104\" text-anchor=\"middle\">Hermes · coordinator</text>\
<text class=\"ov-ts\" x=\"180\" y=\"121\" text-anchor=\"middle\">governor cap {gov} · readiness 11/15</text>\
<text class=\"ov-ts ov-mono\" x=\"180\" y=\"137\" text-anchor=\"middle\">{hb} {hs}</text>\
<line class=\"ov-arrow\" x1=\"180\" y1=\"146\" x2=\"180\" y2=\"168\" marker-end=\"url(#ov-ah)\"/>\
<rect class=\"ov-group\" x=\"8\" y=\"170\" width=\"344\" height=\"232\" rx=\"14\"/>\
<text class=\"ov-t\" x=\"22\" y=\"192\">7 sector planners</text>\
<text class=\"ov-ts\" x=\"340\" y=\"192\" text-anchor=\"end\">useful/target agents</text>\
{rows}\
<line class=\"ov-arrow\" x1=\"180\" y1=\"402\" x2=\"180\" y2=\"426\" marker-end=\"url(#ov-ah)\"/>\
<text class=\"ov-ts\" x=\"190\" y=\"419\">{sessions} sessions · {tasks} tasks</text>\
<rect class=\"ov-node\" x=\"40\" y=\"428\" width=\"280\" height=\"56\" rx=\"12\"/>\
<text class=\"ov-t\" x=\"180\" y=\"450\" text-anchor=\"middle\">Agents · coding and planning</text>\
<text class=\"ov-ts\" x=\"180\" y=\"468\" text-anchor=\"middle\">~{lo}–{hi} hold at once · ceiling {ceiling}</text>\
<line class=\"ov-arrow ov-stop\" x1=\"120\" y1=\"484\" x2=\"120\" y2=\"520\" marker-end=\"url(#ov-ah)\"/>\
<text class=\"ov-ts\" x=\"112\" y=\"506\" text-anchor=\"end\">limit stop</text>\
<line class=\"ov-arrow\" x1=\"240\" y1=\"520\" x2=\"240\" y2=\"486\" marker-end=\"url(#ov-ah)\"/>\
<text class=\"ov-ts\" x=\"248\" y=\"506\">restart</text>\
<rect class=\"ov-node\" x=\"40\" y=\"522\" width=\"280\" height=\"50\" rx=\"12\"/>\
<text class=\"ov-t\" x=\"180\" y=\"543\" text-anchor=\"middle\">Queue + auto-restart</text>\
<text class=\"ov-ts\" x=\"180\" y=\"561\" text-anchor=\"middle\">retry until done · 30 → 600 s backoff</text>\
</svg>",
        sessions = PLAN_TOTAL.sessions,
        tasks = PLAN_TOTAL.tasks,
        target = PLAN_TOTAL.target,
        useful = PLAN_TOTAL.useful,
        lo = PROCESS.measured_lo,
        hi = PROCESS.measured_hi,
        gov = PROCESS.governor,
        ceiling = PROCESS.ceiling,
        hb = esc(hb),
        hs = esc(hs),
    )
}

/// Horizontal bars on one linear scale: the owner ceiling against what
/// actually holds at once.
pub fn capacity_svg() -> String {
    let scale = 336.0 / f64::from(PROCESS.ceiling.max(1));
    let w = |v: u32| f64::from(v) * scale;
    let rows: [(&str, u32, u32, String, &str); 4] = [
        (
            if PROCESS.auto_accept {
                "Owner ceiling (auto-accept on)"
            } else {
                "Owner ceiling"
            },
            0,
            PROCESS.ceiling,
            PROCESS.ceiling.to_string(),
            "ov-bar-c",
        ),
        (
            "Measured: agents that hold at once",
            PROCESS.measured_lo,
            PROCESS.measured_hi,
            format!("{}–{}", PROCESS.measured_lo, PROCESS.measured_hi),
            "ov-bar-u",
        ),
        (
            "Wave 3 launched at",
            0,
            PROCESS.wave3_running,
            PROCESS.wave3_running.to_string(),
            "ov-bar-u",
        ),
        (
            "Hermes governor cap",
            0,
            PROCESS.governor,
            PROCESS.governor.to_string(),
            "ov-bar-u",
        ),
    ];
    let mut out = String::new();
    for (i, (label, lo, hi, txt, cls)) in rows.iter().enumerate() {
        let y = 20 + i as u32 * 42;
        let end = 12.0 + w(*hi);
        out.push_str(&format!(
            "<text class=\"ov-ts\" x=\"12\" y=\"{y}\">{label}</text>\
<rect class=\"ov-track\" x=\"12\" y=\"{by}\" width=\"336\" height=\"14\" rx=\"4\"/>",
            by = y + 7,
            label = esc(label),
        ));
        if *lo > 0 {
            // Solid to the low end, lighter band up to the high end.
            out.push_str(&format!(
                "<rect class=\"{cls}\" x=\"12\" y=\"{by}\" width=\"{lw:.1}\" height=\"14\" rx=\"4\"/>\
<rect class=\"ov-bar-t\" x=\"{bx:.1}\" y=\"{by}\" width=\"{bw:.1}\" height=\"14\"/>",
                by = y + 7,
                lw = w(*lo),
                bx = 12.0 + w(*lo),
                bw = w(*hi) - w(*lo),
            ));
        } else {
            out.push_str(&format!(
                "<rect class=\"{cls}\" x=\"12\" y=\"{by}\" width=\"{bw:.1}\" height=\"14\" rx=\"4\"/>",
                by = y + 7,
                bw = w(*hi),
            ));
        }
        let (tx, anchor, tcls) = if end > 300.0 {
            (end - 6.0, "end", "ov-ts ov-num ov-on")
        } else {
            (end + 6.0, "start", "ov-ts ov-num")
        };
        out.push_str(&format!(
            "<text class=\"{tcls}\" x=\"{tx:.1}\" y=\"{ty}\" text-anchor=\"{anchor}\">{txt}</text>",
            ty = y + 18,
        ));
    }
    format!(
        "<svg class=\"ov-svg\" viewBox=\"0 0 360 176\" role=\"img\" aria-labelledby=\"cap-t cap-d\">\
<title id=\"cap-t\">Ceiling against measured concurrency</title>\
<desc id=\"cap-d\">The owner ceiling is {c} agents with auto-accept on. Measured, about {lo} to {hi} agents hold at once; wave 3 launched at {w3}; the Hermes governor caps at {g}.</desc>\
{out}</svg>",
        c = PROCESS.ceiling,
        lo = PROCESS.measured_lo,
        hi = PROCESS.measured_hi,
        w3 = PROCESS.wave3_running,
        g = PROCESS.governor,
    )
}

fn plan_row(p: &Plan, total: bool) -> String {
    let (o, c) = if total {
        ("<th scope=\"row\">", "</th>")
    } else {
        ("<td>", "</td>")
    };
    format!(
        "<tr>{o}{}{c}<td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
        esc(p.sector),
        p.sessions,
        p.tasks,
        p.target,
        p.useful
    )
}

fn owner_item(it: &OwnerItem) -> String {
    let (cls, word) = if it.done {
        ("ov-badge ov-live", "done")
    } else {
        ("ov-badge ov-blocked", "waiting for you")
    };
    format!(
        "<li class=\"ov-todo\"><div class=\"ov-card-top\"><h3>{t}</h3><span class=\"{cls}\">{word}</span></div>\
<p>{d}</p><p class=\"ov-ref mono\">{r}</p></li>",
        t = esc(it.title),
        d = esc(it.detail),
        r = esc(it.refs),
    )
}

/// The page body. `routes_in_rust` is the live count from `PORTED`.
pub fn body(routes_in_rust: usize) -> String {
    let stats: String = STATS
        .iter()
        .map(|s| {
            format!(
                "<li><span class=\"ov-big\">{}</span><span class=\"ov-label\">{}</span></li>",
                esc(s.value),
                esc(s.label)
            )
        })
        .collect();
    let cards: String = SECTORS.iter().map(|s| card(s, routes_in_rust)).collect();
    let plans: String = PLANS.iter().map(|p| plan_row(p, false)).collect();
    let notes: String = PROCESS
        .notes
        .iter()
        .map(|n| format!("<li>{}</li>", esc(n)))
        .collect();
    let todo: String = OWNER_ITEMS.iter().map(owner_item).collect();
    let open = OWNER_ITEMS.iter().filter(|i| !i.done).count();
    let sources: String = SOURCES
        .iter()
        .map(|s| format!("<li>{}</li>", esc(s)))
        .collect();
    format!(
        "<div class=\"ov\">\
<section class=\"ov-hero\" aria-labelledby=\"ov-h\"><p class=\"ov-kicker\">{kicker}</p>\
<h1 id=\"ov-h\">{headline}</h1><p class=\"ov-lede\">{lede}</p>\
<figure class=\"ov-rule\"><blockquote><p>{rule}</p></blockquote>\
<figcaption><p>{meaning}</p><p class=\"ov-ref\">{set}</p></figcaption></figure>\
<ul class=\"ov-stats\" aria-label=\"At a glance\">{stats}</ul></section>\
<section id=\"sectors\" aria-labelledby=\"ov-map-h\"><h2 id=\"ov-map-h\" class=\"ov-h2\">Sector map</h2>\
<p class=\"ov-sub\">Nine parts of one project. Status, the facts behind it, and the branch it lives on.</p>\
<ul class=\"ov-map\">{cards}</ul></section>\
<section id=\"process\" aria-labelledby=\"ov-proc-h\"><h2 id=\"ov-proc-h\" class=\"ov-h2\">The automated process</h2>\
<p class=\"ov-sub\">Hermes hands work to one planner per sector; planners size the work; agents run until done, and a stop is only a pause.</p>\
<div class=\"ov-flow\"><figure class=\"ov-fig\">{flow}</figure>\
<div class=\"ov-side\"><figure class=\"ov-fig\">{cap}<figcaption>The ceiling is far above what the request limit lets hold at once, so the queue does the rest: {retry}.</figcaption></figure>\
<ul class=\"ov-notes\" aria-label=\"Measurements\">{notes}</ul>\
<div class=\"ov-tablewrap\"><table class=\"ov-table\"><caption>Plan totals by sector</caption>\
<thead><tr><th scope=\"col\">Sector</th><th scope=\"col\">Sessions</th><th scope=\"col\">Tasks</th><th scope=\"col\">Target</th><th scope=\"col\">Useful</th></tr></thead>\
<tbody>{plans}</tbody><tfoot>{total}</tfoot></table></div></div></div></section>\
<section id=\"owner\" aria-labelledby=\"ov-own-h\"><h2 id=\"ov-own-h\" class=\"ov-h2\">Waiting for Slavik</h2>\
<p class=\"ov-sub\">{open} of {all} open. These unblock Hermes; nothing here is decided by the swarm.</p>\
<ul class=\"ov-todos\">{todo}</ul></section>\
<footer class=\"ov-foot\"><p>Facts as of {as_of}. Sources:</p><ul>{sources}</ul>\
<p>Live state: <a href=\"/status\">Status</a> · <a href=\"/services\">Services</a> · <a href=\"/agents\">Agents</a></p></footer>\
</div>",
        kicker = esc(ov::KICKER),
        headline = esc(ov::HEADLINE),
        lede = esc(ov::LEDE),
        rule = esc(RULE.text),
        meaning = esc(RULE.meaning),
        set = esc(RULE.set),
        flow = flow_svg(),
        cap = capacity_svg(),
        retry = esc(PROCESS.retry),
        total = plan_row(&PLAN_TOTAL, true),
        all = OWNER_ITEMS.len(),
        as_of = esc(ov::AS_OF),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_has_every_section_and_no_script() {
        let b = body(14);
        for needle in [
            "Discovering the universe together with humans.",
            "leave with more than you came in with",
            "id=\"sectors\"",
            "id=\"process\"",
            "id=\"owner\"",
            "swarm/hermes-core",
            "45b7980",
            "swarm/voice-rust",
            "03d21a9",
            "swarm/common-migrate-2 123eab2",
            "HADES admin key",
            "OpenRouter credit",
            "Strict HTTP handling approval",
            "14 of 31 routes in Rust",
            "<td>645</td>",
            "<td>407</td>",
        ] {
            assert!(b.contains(needle), "missing {needle}");
        }
        assert!(!b.to_ascii_lowercase().contains("<script"));
        assert!(!b.contains("style=\""));
        // No event-handler attributes (` onclick="` and friends).
        for (i, _) in b.match_indices(" on") {
            let rest = &b[i + 3..];
            let word: String = rest.chars().take_while(char::is_ascii_lowercase).collect();
            assert!(
                !rest[word.len()..].starts_with("=\""),
                "handler attribute on{word}"
            );
        }
        assert_eq!(b.matches("class=\"ov-card\"").count(), SECTORS.len());
        assert_eq!(b.matches("<svg class=\"ov-svg\"").count(), 2);
        assert_eq!(b.matches("<div").count(), b.matches("</div>").count());
        assert_eq!(b.matches("<ul").count(), b.matches("</ul>").count());
        let li = b.matches("<li>").count() + b.matches("<li ").count();
        assert_eq!(li, b.matches("</li>").count());
        assert_eq!(b.matches("<svg").count(), b.matches("</svg>").count());
        assert_eq!(
            b.matches("<section").count(),
            b.matches("</section>").count()
        );
    }

    #[test]
    fn bars_scale_to_the_data() {
        let s = flow_svg();
        // The widest target (security, 166) gets the full 150 px.
        assert!(s.contains("width=\"150.0\""));
        let c = capacity_svg();
        // Ceiling 250 spans the track (336 px); 20 is 26.9 px of it.
        assert!(c.contains("width=\"336.0\""));
        assert!(c.contains(">18–20<"));
    }
}
