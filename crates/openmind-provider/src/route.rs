// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Choose one provider row for a need.
//!
//! [`route`] returns the chosen row, or [`None`]. The order matches
//! [`crate::fallback::select_provider`]: the first usable GPU, else the
//! first usable approved LAN row, else the first usable CPU-local row.
//! Pending rows are not chosen. A remote row is never chosen.
//!
//! `openmind-discovery` exports pending LAN candidates, but its record type
//! is not a [`ProviderRow`] (no kind, health, or capability). That API is
//! left untouched. This crate keeps a tiny local copy of those pending
//! seed ids and endpoints instead. Nothing here opens a socket.

#![forbid(unsafe_code)]

pub use crate::fallback::{Approval, Capability, Health, Kind, ProviderRow};

use crate::fallback::select_provider;

/// Pending seed ids and endpoints copied from discovery. Not dialed.
///
/// Kind is LAN because those seeds are discovery candidates. Capability is
/// a local stand-in: discovery has no capability enum, and [`Capability::Llm`]
/// makes each row eligible for the usual need except that approval stays
/// pending.
const PENDING_SEEDS: &[(&str, &str)] = &[
    ("seed-openmind", "127.0.0.1:7787"),
    ("seed-ollama", "127.0.0.1:11434"),
    ("seed-llama-rpc", "127.0.0.1:50052"),
    ("seed-exo", "127.0.0.1:52415"),
];

fn pending_candidates() -> Vec<ProviderRow> {
    PENDING_SEEDS
        .iter()
        .map(|(id, endpoint)| ProviderRow {
            id: (*id).to_owned(),
            kind: Kind::Lan,
            endpoint: (*endpoint).to_owned(),
            approval: Approval::Pending,
            health: Health::Unknown,
            capabilities: vec![Capability::Llm],
        })
        .collect()
}

/// Pick a row for `need`.
///
/// `rows` are caller fixtures. The built-in pending candidates are placed
/// in front of `rows` and passed to [`crate::fallback::select_provider`].
/// They are never returned. A fixture is chosen only when that policy would
/// choose it: [`Approval::Approved`], a non-empty endpoint, health not
/// [`Health::Down`], a capability list that contains `need`, and a kind of
/// GPU, else LAN, else CPU-local.
///
/// The result is an owned copy so the built-in list can stay private. The
/// input slice is not reordered and is not dialed.
#[must_use]
pub fn route(need: Capability, rows: &[ProviderRow]) -> Option<ProviderRow> {
    let mut candidates = pending_candidates();
    candidates.extend_from_slice(rows);
    let chosen = select_provider(&candidates, need)?;
    if chosen.approval != Approval::Approved {
        return None;
    }
    Some(chosen.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, kind: Kind, approval: Approval) -> ProviderRow {
        ProviderRow {
            id: id.to_owned(),
            kind,
            endpoint: format!("fixture://{id}"),
            approval,
            health: Health::Up,
            capabilities: vec![Capability::Llm],
        }
    }

    #[test]
    fn pending_seeds_match_the_discovery_ids_and_are_not_a_choice() {
        let pending = pending_candidates();
        let ids: Vec<&str> = pending.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(
            ids,
            ["seed-openmind", "seed-ollama", "seed-llama-rpc", "seed-exo"]
        );
        assert!(pending.iter().all(|row| {
            row.approval == Approval::Pending
                && row.kind == Kind::Lan
                && !row.endpoint.is_empty()
                && row.capabilities.contains(&Capability::Llm)
        }));
        assert!(select_provider(&pending, Capability::Llm).is_none());
        assert!(route(Capability::Llm, &[]).is_none());
    }

    #[test]
    fn route_matches_select_provider_on_the_combined_slice() {
        let rows = [row("cpu-local", Kind::CpuLocal, Approval::Approved)];
        let mut combined = pending_candidates();
        combined.extend_from_slice(&rows);
        let via_policy = select_provider(&combined, Capability::Llm).expect("cpu");
        let via_route = route(Capability::Llm, &rows).expect("cpu");
        assert_eq!(via_route, *via_policy);
        assert_eq!(via_route.id, "cpu-local");
        assert_ne!(via_route.endpoint, "127.0.0.1:7787");
    }
}
