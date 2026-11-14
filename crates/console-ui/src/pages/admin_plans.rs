// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/admin/plans`: the three agreed plans, display only.
//!
//! Markup uses classes already in the console sheet (`page`, `cards`,
//! `badge`, `tabs`, `note`). No new palette tokens. No form, so nothing
//! on this page can start a payment.

use crate::admin_catalog::{self, Plan};
use crate::html::esc;
use crate::icons::icon;

pub fn subnav(current: &str) -> String {
    let tab = |href: &str, label: &str| {
        let here = if href == current {
            " aria-current=\"page\""
        } else {
            ""
        };
        format!("<a href=\"{href}\"{here}>{label}</a>")
    };
    format!(
        "<nav class=\"tabs\" aria-label=\"Admin\">{}{}</nav>",
        tab("/admin/plans", "Plans"),
        tab("/admin/accounts", "Accounts")
    )
}

fn empty() -> &'static str {
    "<ul class=\"cards\"><li><div><p class=\"name\"><span>No plans to show</span></p>\
<p class=\"muted\">The plan list is empty. This screen does not sell a plan.</p></div></li></ul>"
}

fn card(plan: &Plan) -> String {
    format!(
        "<li><div><p class=\"name\"><span>{}</span><span class=\"badge neutral\">{}</span></p>\
<p class=\"muted\">{}</p></div><span class=\"mono\">{}</span></li>",
        esc(plan.name),
        esc(plan.product),
        esc(plan.summary),
        esc(&admin_catalog::price_label(plan))
    )
}

/// `plans` is the catalog to render. The live route passes
/// [`crate::admin_catalog::PLANS`]. An empty slice is the empty state.
pub fn body(plans: &[Plan]) -> String {
    let list = if plans.is_empty() {
        empty().to_string()
    } else {
        let mut ul = String::from("<ul class=\"cards\">");
        for plan in plans {
            ul.push_str(&card(plan));
        }
        ul.push_str("</ul>");
        ul
    };
    format!(
        "<div class=\"page w46\"><header><p class=\"k\">Admin</p>\
<h1>{}<span class=\"ell\">Plans</span></h1>\
<p class=\"lede\">Three plans. Free is Galion OS, open source. Plus is Galion One. Ultra is Galion Research. This page only displays the prices.</p></header>\
{}\
{list}\
<p class=\"note\">Display only. This screen does not take a payment and does not call a billing service.</p></div>",
        icon("desk", "i i-md"),
        subnav("/admin/plans")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin_catalog::PLANS;

    #[test]
    fn shows_the_three_prices_and_no_form() {
        let html = body(PLANS);
        assert!(html.contains("$0 · EUR 0 · forever"));
        assert!(html.contains("$20 or EUR 20 per month"));
        assert!(html.contains("$220 or EUR 220 one-time"));
        assert!(html.contains("Galion OS"));
        assert!(html.contains("Galion One"));
        assert!(html.contains("Galion Research"));
        assert!(html.contains("Open source. Free forever."));
        assert!(html.contains("href=\"/admin/accounts\""));
        assert!(html.contains("aria-current=\"page\">Plans"));
        assert!(!html.contains("<form"));
        assert!(!html.to_ascii_lowercase().contains("stripe"));
        assert!(!html.contains("<script"));
    }

    #[test]
    fn empty_catalog_says_so() {
        let html = body(&[]);
        assert!(html.contains("No plans to show"));
        assert!(!html.contains("$20"));
        assert!(!html.contains("<form"));
    }
}
