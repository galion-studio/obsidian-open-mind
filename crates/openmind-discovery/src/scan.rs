// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Active scan gate.
//!
//! [`crate::Scan::Off`] never calls this module. The CLI calls [`active_scan`]
//! only after `--scan`. This stub records the request and sends no packets.

/// Result of the opt-in scanner. `packets_sent` is always 0 in this stub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanOutcome {
    pub packets_sent: u32,
    pub note: &'static str,
}

/// Honor `--scan` without opening a socket.
pub fn active_scan() -> ScanOutcome {
    ScanOutcome {
        packets_sent: 0,
        note: "stub: --scan was set; no packets were sent",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_sends_no_packets() {
        let outcome = active_scan();
        assert_eq!(outcome.packets_sent, 0);
        assert!(!outcome.note.is_empty());
    }
}
