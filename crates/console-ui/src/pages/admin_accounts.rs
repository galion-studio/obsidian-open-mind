// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/admin/accounts`: who is on which plan, display only.
//!
//! The live route passes [`crate::admin_catalog::LIVE_ACCOUNTS`] (empty).
//! A non-empty slice is rendered for tests and for a future directory that
//! is not a payment service. Names are escaped. Unknown plan ids are not
//! printed back.

use crate::admin_catalog::{self, Account};
use crate::html::esc;
use crate::icons::icon;

fn plain(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .take(80)
        .collect::<String>()
        .trim()
        .to_string()
}

fn label(account: &Account) -> String {
    let name = plain(&account.name);
    if name.is_empty() {
        plain(&account.id)
    } else {
        name
    }
}

fn plan_label(plan_id: &str) -> String {
    match admin_catalog::plan_by_id(plan_id) {
        Some(plan) => format!(
            "{} · {} · {}",
            plan.name,
            plan.product,
            admin_catalog::availability(plan).to_ascii_lowercase()
        ),
        None => "Not on a listed plan".into(),
    }
}

fn empty() -> &'static str {
    "<ul class=\"cards\"><li><div><p class=\"name\"><span>No users yet</span></p>\
<p class=\"muted\">This account list is empty. Nothing here creates an account or charges a plan.</p></div></li></ul>"
}

/// Render the account list. An empty slice, or a slice whose rows have no
/// visible name, is the empty state.
pub fn body(accounts: &[Account]) -> String {
    let rows: Vec<&Account> = accounts.iter().filter(|a| !label(a).is_empty()).collect();
    let list = if rows.is_empty() {
        empty().to_string()
    } else {
        let mut ul = String::from("<ul class=\"rows\">");
        for account in rows {
            ul.push_str(&format!(
                "<li><span class=\"ell\">{}</span><span class=\"muted\">{}</span></li>",
                esc(&label(account)),
                esc(&plan_label(&account.plan_id))
            ));
        }
        ul.push_str("</ul>");
        ul
    };
    format!(
        "<div class=\"page w46 adm\"><header><p class=\"k\">Admin</p>\
<h1>{}<span class=\"ell\">Accounts</span></h1>\
<p class=\"lede\">Users on a plan. Display only: this page does not create an account and does not take a payment.</p></header>\
{}\
{}\
{list}</div>",
        icon("agents", "i i-md"),
        super::admin_plans::hold(),
        super::admin_plans::subnav("/admin/accounts")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acc(id: &str, name: &str, plan: &str) -> Account {
        Account {
            id: id.into(),
            name: name.into(),
            plan_id: plan.into(),
        }
    }

    #[test]
    fn live_list_is_the_empty_state() {
        let html = body(admin_catalog::LIVE_ACCOUNTS);
        assert!(html.contains("No users yet"));
        assert!(html.contains("This account list is empty"));
        assert!(html.contains("does not create an account"));
        assert!(html.contains("href=\"/admin/plans\""));
        assert!(html.contains("aria-current=\"page\">Accounts"));
        assert!(!html.contains("<form"));
        assert!(!html.contains("<script"));
        assert!(!html.contains("$20"));
    }

    #[test]
    fn rows_are_escaped_and_unknown_plans_are_not_echoed() {
        let html = body(&[
            acc("a", "<script>", "plus"),
            acc("b", "Ada", "no-such-plan"),
            acc("", "  ", ""),
        ]);
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<script>"));
        assert!(html.contains("Plus · Galion One · not open"));
        assert!(html.contains("Paid plans are not open"));
        assert!(html.contains("Not on a listed plan"));
        assert!(!html.contains("no-such-plan"));
        assert_eq!(html.matches("<li>").count(), 2);
    }
}
