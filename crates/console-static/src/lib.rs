// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Static front for the Galion console (backlog B61).
//!
//! The live console on 127.0.0.1:8080 is a Node srvx process serving
//! `/opt/galion/console-dist` (static assets + nitro API). This crate is the
//! Rust replacement path for that front:
//!
//! - [`site::Site`] indexes the built tree once at startup. Only regular files
//!   whose every path segment is safe (no dotfiles, no symlinks) are listed.
//!   Requests never touch the filesystem by name.
//! - [`routes::Routes`] lists the SPA page routes that get the HTML shell and
//!   the few redirects. Anything else is 404.
//! - [`proxy`] forwards an allowlisted prefix set (default `/api/` and
//!   `/_serverFn/`) to a loopback upstream, streaming the answer (SSE works).
//! - [`api`] (`--local-api` / `--dist`) answers honest `/api/v1/health` and
//!   unpaired session phone-gate locally so console-dist cutover prep does not
//!   need the Node board for those paths.
//!
//! std only, no dependencies, `#![forbid(unsafe_code)]`. Prefer this crate over
//! adding axum/tower-http for ServeDir (licence-ok but not needed).

#![forbid(unsafe_code)]

pub mod api;
pub mod config;
pub mod entry;
pub mod http;
pub mod mime;
pub mod path;
pub mod proxy;
pub mod routes;
pub mod server;
pub mod site;
