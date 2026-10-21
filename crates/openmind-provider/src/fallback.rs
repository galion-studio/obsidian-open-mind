// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Provider fallback policy.
//!
//! [`select_provider`] picks one row for a need. It does not open a socket,
//! read a registry file, or change approval. Pending, rejected, empty-endpoint,
//! and down rows are skipped. A remote row is never chosen.

#![forbid(unsafe_code)]

/// Where the compute sits. Local to this crate, not the registry type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// External GPU host reached through an already approved path.
    Gpu,
    /// A machine on the local network.
    Lan,
    /// A process on this CPU-light box.
    CpuLocal,
    /// A hosted API. Invisible to this policy.
    Remote,
}

/// Approval of one row. Only [`Approval::Approved`] can be selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    /// Offered, not accepted.
    Pending,
    /// Explicitly accepted.
    Approved,
    /// Explicitly refused.
    Rejected,
}

/// Health snapshot the caller already holds. This crate does not probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// No snapshot yet. Not a failure.
    Unknown,
    /// Last snapshot said the row was up.
    Up,
    /// Last snapshot failed. Selection falls through.
    Down,
}

/// What the caller needs, and what a row can cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// Chat or completion.
    Llm,
    /// Speech to text.
    Stt,
    /// Text to speech.
    Tts,
    /// Image or video input.
    Vision,
}

/// One provider row used only for fallback selection.
///
/// Fields are public so a test or a caller can build a row without a registry.
/// Nothing here is dialed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRow {
    /// Stable name.
    pub id: String,
    /// Where the compute sits.
    pub kind: Kind,
    /// Stored endpoint. Empty means the row is invisible. Never dialed.
    pub endpoint: String,
    /// Approval state.
    pub approval: Approval,
    /// Caller-supplied health snapshot.
    pub health: Health,
    /// Capabilities this row covers, in any order.
    pub capabilities: Vec<Capability>,
}

/// Pick a provider for `need`.
///
/// A row is usable when `approval` is [`Approval::Approved`], `endpoint` is
/// non-empty, `health` is not [`Health::Down`], and `capabilities` contains
/// `need`.
///
/// Among usable rows the order is fixed:
///
/// 1. the first [`Kind::Gpu`]
/// 2. else the first [`Kind::Lan`] (an approved LAN row)
/// 3. else the first [`Kind::CpuLocal`]
///
/// [`Kind::Remote`] is never selected. Input order is kept inside a tier.
/// The slice is not reordered. Returns `None` when no tier has a usable row.
#[must_use]
pub fn select_provider(rows: &[ProviderRow], need: Capability) -> Option<&ProviderRow> {
    for kind in [Kind::Gpu, Kind::Lan, Kind::CpuLocal] {
        if let Some(row) = rows
            .iter()
            .find(|row| row.kind == kind && usable(row, need))
        {
            return Some(row);
        }
    }
    None
}

/// Every usable row for `need` in fallback order: all usable GPU rows, then
/// LAN, then CPU-local, input order kept inside a tier. The first entry is
/// what [`select_provider`] returns; a caller that fails to reach it tries
/// the next one. Same rules as [`select_provider`]; remote is never listed.
#[must_use]
pub fn ordered(rows: &[ProviderRow], need: Capability) -> Vec<&ProviderRow> {
    let mut out = Vec::new();
    for kind in [Kind::Gpu, Kind::Lan, Kind::CpuLocal] {
        out.extend(
            rows.iter()
                .filter(|row| row.kind == kind && usable(row, need)),
        );
    }
    out
}

fn usable(row: &ProviderRow, need: Capability) -> bool {
    row.approval == Approval::Approved
        && row.health != Health::Down
        && !row.endpoint.is_empty()
        && row.capabilities.contains(&need)
}
