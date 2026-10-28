// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! The legacy `--scan` gate.
//!
//! [`crate::Scan::Off`] never calls this module. `--scan` (or
//! [`crate::Scan::Requested`]) runs one real mDNS/DNS-SD browse with the
//! default service list and the default 2 s timeout, and reports exactly how
//! many packets it sent. Results are pending candidates only.

use crate::mdns::{browse, BrowseOpts};
use crate::{fold_records, Candidate};

/// Result of the opt-in scanner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOutcome {
    pub packets_sent: u32,
    pub note: String,
    pub candidates: Vec<Candidate>,
}

/// One mDNS browse with default settings.
pub fn active_scan() -> ScanOutcome {
    match browse(&BrowseOpts::default()) {
        Ok(out) => ScanOutcome {
            packets_sent: out.packets_sent,
            note: format!(
                "mdns browse via {}: {} responses from {} responders",
                out.iface,
                out.packets_received,
                out.responders.len()
            ),
            candidates: fold_records(&out.records),
        },
        Err(err) => ScanOutcome {
            packets_sent: 0,
            note: format!("mdns browse failed: {err}"),
            candidates: Vec::new(),
        },
    }
}
