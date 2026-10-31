// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! gaia-cortex: the event-driven decision layer above the event-driven nerve.
//!
//! See `docs/cortex/event-driven.md` and `docs/cortex/event-contract.md`.
//! Pipeline: [`engine`]; rules: [`rules`]; never-act list: [`guard`];
//! advisor (OFF by default): [`llm`]; nerve link: [`nerve`]; API: [`server`] (reads from [`snapshot`] while the decider is busy).

pub mod approvals;
pub mod build_info;
pub mod config;
pub mod contact;
pub mod dispatch;
pub mod engine;
pub mod freeze;
pub mod guard;
pub mod handoff;
pub mod handoff_exclude;
pub mod llm;
pub mod nerve;
pub mod phone;
pub mod repeat;
pub mod rules;
pub mod server;
pub mod sink;
pub mod snapshot;
pub mod verify;

use std::path::Path;

/// Build a cortex from config (real guard, real nerve link if configured).
pub fn build(cfg: config::Config) -> engine::Cortex {
    let guard = guard::Guard::new(&cfg.protected_units);
    let contact = std::sync::Arc::new(contact::NerveContact::default());
    let link: Box<dyn nerve::NerveLink> =
        match cfg.nerve_url.as_deref().and_then(nerve::HttpNerve::new) {
            Some(n) => Box::new(n.with_contact(contact.clone())),
            None => Box::new(nerve::NoNerve),
        };
    let mut cx = engine::Cortex::new(cfg, guard, link);
    cx.contact = contact;
    cx.publish();
    cx
}

/// Read an NDJSON / JSON-array event file.
pub fn read_events(path: &Path) -> Result<Vec<nerve_cortex_contract::Event>, String> {
    let t = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(nerve_cortex_contract::parse_events(&t)
        .into_iter()
        .filter_map(Result::ok)
        .collect())
}
