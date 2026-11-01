// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The /account page (HTML + CSS + JS, no libraries), compiled in.

use galion_common::http::Response;

pub const HTML: &str = include_str!("../page/account.html");
pub const CSS: &str = include_str!("../page/account.css");
pub const JS: &str = include_str!("../page/account.js");

/// Strict CSP: only our own files, no inline script, no framing.
pub const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'";

fn secure(r: Response) -> Response {
    r.with_header("content-security-policy", CSP)
        .with_header("x-content-type-options", "nosniff")
        .with_header("referrer-policy", "no-referrer")
        .with_header("cache-control", "no-store")
}

pub fn html() -> Response {
    secure(Response::new(200, "text/html; charset=utf-8", HTML))
}

pub fn css() -> Response {
    secure(Response::new(200, "text/css; charset=utf-8", CSS))
}

pub fn js() -> Response {
    secure(Response::new(200, "text/javascript; charset=utf-8", JS))
}
