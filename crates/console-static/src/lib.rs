// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Static front for the Galion console (backlog B61).
//!
//! The live console on 127.0.0.1:8080 is a Vite dev server: it serves source
//! files, `/@vite/*`, `/@fs/*` and answers scanner probes. This crate is the
//! production replacement for the *front*:
//!
//! - [`site::Site`] indexes the built `dist/client` tree once at startup. Only
//!   regular files whose every path segment is safe (no dotfiles, no symlinks)
//!   are listed. Requests never touch the filesystem by name, so `..`,
//!   encoded separators and directory listing are impossible by construction.
//! - [`routes::Routes`] lists the SPA page routes that get the HTML shell and
//!   the few redirects. Anything else is 404.
//! - [`proxy`] forwards an allowlisted prefix set (default `/api/` and
//!   `/_serverFn/`) to a loopback upstream, streaming the answer (SSE works).
//!
//! std only, no dependencies, `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]

pub mod config;
pub mod http;
pub mod mime;
pub mod path;
pub mod proxy;
pub mod routes;
pub mod server;
pub mod site;
