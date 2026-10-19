// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Defence decisions are notes.
//!
//! [`decide`] copies health rows into a [`DefenceDecision`]. The value is
//! not a command. It does not apply a firewall change, does not start a
//! firewall tool, and does not open a socket.

#![forbid(unsafe_code)]

use crate::HealthRow;

/// One service inside a decision. Copied from a health row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceNote {
    pub service: String,
    pub status: String,
    pub ms: i64,
    /// True only when `status` is exactly `healthy`.
    pub healthy: bool,
}

/// A defence decision. Nothing here is applied to the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefenceDecision {
    notes: Vec<ServiceNote>,
}

impl DefenceDecision {
    /// Notes in the same order as the rows passed to [`decide`].
    #[must_use]
    pub fn notes(&self) -> &[ServiceNote] {
        &self.notes
    }

    /// Always false. A decision does not apply a firewall change.
    #[must_use]
    pub fn applies_firewall_change(&self) -> bool {
        let _notes = &self.notes;
        false
    }

    /// Always false. No firewall tool is started.
    #[must_use]
    pub fn spawns_firewall_tool(&self) -> bool {
        let _notes = &self.notes;
        false
    }

    /// Always false. No socket is opened.
    #[must_use]
    pub fn opens_socket(&self) -> bool {
        let _notes = &self.notes;
        false
    }
}

/// Build a decision from health rows.
///
/// The result is data only. It does not apply a firewall change, spawn a
/// process, or open a socket.
#[must_use]
pub fn decide(rows: &[HealthRow]) -> DefenceDecision {
    let notes = rows
        .iter()
        .map(|row| ServiceNote {
            service: row.service.clone(),
            status: row.status.clone(),
            ms: row.ms,
            healthy: row.status == "healthy",
        })
        .collect();
    DefenceDecision { notes }
}
