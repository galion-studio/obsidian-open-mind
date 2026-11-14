// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Routes for the admin plan and account screens.
//!
//! GET and HEAD only. The plan list is the static catalog. The account
//! list is [`crate::admin_catalog::LIVE_ACCOUNTS`]. The only upstream call
//! is `GET /api/v1/health` for the header origin dot, the same call every
//! other room makes. No billing path is requested.

use crate::admin_catalog::{LIVE_ACCOUNTS, PLANS};
use crate::layout::{self, Origin};
use crate::pages::{admin_accounts, admin_plans};
use crate::{Reply, Request, Ui};

/// Pages this module answers.
pub const ROUTES: &[&str] = &["/admin/plans", "/admin/accounts"];

pub fn owns(path: &str) -> bool {
    ROUTES.contains(&path)
}

impl Ui {
    pub(crate) fn admin_room(&self, req: &Request<'_>) -> Reply {
        let fwd = self.forward(req);
        let origin = match self.cached_health(&fwd) {
            Some(h) if h.get("status").as_str() == Some("healthy") => Origin::Up,
            _ => Origin::Down,
        };
        let (label, body) = match req.path {
            "/admin/plans" => ("Plans", admin_plans::body(PLANS)),
            _ => ("Accounts", admin_accounts::body(LIVE_ACCOUNTS)),
        };
        let title = format!("Galion Console · {label}");
        let f = self.frame(req, &title);
        Reply::html(200, layout::console(&f, origin, &body))
    }
}
