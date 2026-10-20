// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Retry policy for agents that stop before finishing.
//!
//! Slavik's rule (2026-10-20 20:41 CEST): restart every agent the request
//! limit stops until it completes. A request-limit stop or a stall is retried
//! with capped exponential backoff up to `max_attempts`; a plain error gets
//! `max_error_attempts`; a refusal is never retried.

use crate::registry::RetryCfg;
use serde::Serialize;

/// Why an agent session ended without finishing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "detail")]
pub enum StopKind {
    /// The platform request/concurrency limit stopped the session.
    RequestLimit,
    /// No progress for too long; stopped and resumed in place.
    Stall,
    /// Any other failure.
    Error(String),
}

impl StopKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            StopKind::RequestLimit => "request-limit",
            StopKind::Stall => "stall",
            StopKind::Error(_) => "error",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    Retry { after_secs: u64 },
    Fail(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryPolicy(pub RetryCfg);

impl RetryPolicy {
    /// Backoff before attempt `attempt + 1`, after `attempt` failed (1-based).
    pub fn backoff_secs(&self, attempt: u32) -> u64 {
        let c = &self.0;
        let mut d = c.backoff_base_secs;
        for _ in 1..attempt.max(1) {
            d = d.saturating_mul(c.backoff_factor);
            if d >= c.backoff_max_secs {
                break;
            }
        }
        d.min(c.backoff_max_secs)
    }

    /// What to do after attempt number `attempt` stopped with `kind`.
    pub fn next(&self, kind: &StopKind, attempt: u32) -> Next {
        let limit = match kind {
            StopKind::RequestLimit | StopKind::Stall => self.0.max_attempts,
            StopKind::Error(_) => self.0.max_error_attempts,
        };
        if attempt >= limit {
            Next::Fail(format!(
                "{} after {attempt} attempt(s); limit {limit}",
                kind.as_str()
            ))
        } else {
            Next::Retry {
                after_secs: self.backoff_secs(attempt),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        let p = RetryPolicy(RetryCfg::default());
        let seq: Vec<u64> = (1..=7).map(|a| p.backoff_secs(a)).collect();
        assert_eq!(seq, [30, 60, 120, 240, 480, 600, 600]);
    }

    #[test]
    fn limit_stops_retry_until_max_errors_fail_sooner() {
        let p = RetryPolicy(RetryCfg::default());
        assert_eq!(
            p.next(&StopKind::RequestLimit, 1),
            Next::Retry { after_secs: 30 }
        );
        assert!(matches!(p.next(&StopKind::RequestLimit, 8), Next::Fail(_)));
        assert_eq!(p.next(&StopKind::Stall, 7), Next::Retry { after_secs: 600 });
        assert!(matches!(
            p.next(&StopKind::Error("x".into()), 2),
            Next::Fail(_)
        ));
        assert_eq!(
            p.next(&StopKind::Error("x".into()), 1),
            Next::Retry { after_secs: 30 }
        );
    }
}
