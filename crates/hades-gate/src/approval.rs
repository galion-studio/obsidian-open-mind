// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The real approval path: config, admin public key, replay ledger and the
//! admin-safety rule put together.
//!
//! Order of checks for [`Approver::approve`] (the first failure wins):
//!
//! 1. disable file present → [`Verdict::Refused`] (the admin switched HADES off);
//! 2. `kill` / `rotate` or an unknown action → refused, no token can change it;
//! 3. admin-safety [`crate::safety::review`] on target and plan → refused;
//! 4. no token → [`Verdict::DryRun`] (the default);
//! 5. signature, lifetime, expiry and exact scope ([`crate::token::verify`]);
//! 6. single use: the nonce is consumed in the fsynced ledger.
//!
//! A success is [`Verdict::Approved`], which maps to
//! [`Decision::WouldApply`](crate::Decision::WouldApply): the gate is still in
//! dry-run and executes nothing. It hands an approved, single-use, scoped
//! decision to the caller (operator script or GAIA HADES).

use std::path::{Path, PathBuf};

use ed25519_dalek::VerifyingKey;

use crate::config::Config;
use crate::ledger::{Ledger, LedgerError};
use crate::token::{self, Claims, Scope, TokenError};
use crate::{Action, Decision, Mode};

/// Outcome of [`Approver::approve`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// No token was given: plan only.
    DryRun,
    /// Valid, in scope, unexpired, first use. Still not executed.
    Approved(Claims),
    /// Refused, with a reason that never contains key material.
    Refused(Refusal),
}

impl Verdict {
    /// The matching legacy [`Decision`].
    #[must_use]
    pub fn decision(&self) -> Decision {
        match self {
            Self::DryRun => Decision::DryRun,
            Self::Approved(_) => Decision::WouldApply,
            Self::Refused(_) => Decision::Refused,
        }
    }
}

/// Why an approval was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The admin disabled HADES (disable file present).
    Disabled,
    /// `kill`, `rotate` or unknown action kind.
    ActionNotApprovable(String),
    /// The admin-safety rule.
    Safety(crate::safety::Hazard),
    /// Token problem: forged, tampered, expired, wrong scope, ...
    Token(TokenError),
    /// Replay or ledger failure.
    Ledger(LedgerError),
}

impl Refusal {
    /// Human-readable reason.
    #[must_use]
    pub fn reason(&self) -> String {
        match self {
            Self::Disabled => "HADES is disabled by the admin (disable file present)".into(),
            Self::ActionNotApprovable(a) => format!("action {a:?} can never be approved"),
            Self::Safety(h) => h.reason(),
            Self::Token(t) => t.reason(),
            Self::Ledger(l) => l.reason(),
        }
    }

    /// Short stable code for JSON output and tests.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::ActionNotApprovable(_) => "action",
            Self::Safety(_) => "admin-safety",
            Self::Token(TokenError::Malformed(_)) => "malformed",
            Self::Token(TokenError::BadSignature) => "forged",
            Self::Token(TokenError::Expired) => "expired",
            Self::Token(TokenError::NotYetValid) => "not-yet-valid",
            Self::Token(TokenError::BadLifetime) => "lifetime",
            Self::Token(TokenError::WrongScope(_)) => "wrong-scope",
            Self::Ledger(LedgerError::Replayed) => "replayed",
            Self::Ledger(_) => "ledger",
        }
    }
}

/// Whether the ledger line for a decision was written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Audit {
    /// Written and fsynced.
    Written,
    /// Not written, or the ledger is failing closed. The text of
    /// [`LedgerError::reason`] starts with `ledger unavailable` or
    /// `ledger corrupt`, the strings the cortex `hades` sensor matches.
    Failed(LedgerError),
}

impl Audit {
    /// `true` when the ledger line was written.
    #[must_use]
    pub const fn ok(&self) -> bool {
        matches!(self, Self::Written)
    }
}

/// Outcome of [`Approver::approve_audited`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decided {
    /// The decision.
    pub verdict: Verdict,
    /// Whether it reached the ledger.
    pub audit: Audit,
}

/// The approval gate with a real admin key.
#[derive(Debug, Clone)]
pub struct Approver {
    admin: VerifyingKey,
    ledger: Ledger,
    disable_file: PathBuf,
    max_ttl_secs: u64,
}

/// One approval request.
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    /// Action kind wire name (`heal-apply`, `switch`, `rollback`).
    pub action: &'a str,
    /// Exact target service.
    pub target: &'a str,
    /// Exact plan bytes.
    pub plan: &'a [u8],
    /// Token string; empty means "no token" (dry-run).
    pub token: &'a str,
}

impl Approver {
    /// Build from parts.
    ///
    /// # Errors
    /// When the ledger cannot be opened.
    pub fn new(
        admin: VerifyingKey,
        ledger: impl Into<PathBuf>,
        disable_file: impl Into<PathBuf>,
        max_ttl_secs: u64,
    ) -> Result<Self, String> {
        Ok(Self {
            admin,
            ledger: Ledger::open(ledger).map_err(|e| e.reason())?,
            disable_file: disable_file.into(),
            max_ttl_secs: max_ttl_secs.min(token::HARD_MAX_TTL_SECS),
        })
    }

    /// Build from a [`Config`].
    ///
    /// # Errors
    /// Key or ledger problems.
    pub fn from_config(cfg: &Config) -> Result<Self, String> {
        let admin = crate::keys::load_public(&cfg.admin_pubkey)?;
        Self::new(admin, &cfg.ledger, &cfg.disable_file, cfg.max_ttl_secs)
    }

    /// Always [`Mode::DryRun`]: approval never executes anything here.
    #[must_use]
    pub const fn mode(&self) -> Mode {
        Mode::DryRun
    }

    /// The ledger.
    #[must_use]
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// The disable file path.
    #[must_use]
    pub fn disable_file(&self) -> &Path {
        &self.disable_file
    }

    /// Whether the admin has disabled HADES.
    #[must_use]
    pub fn is_disabled(&self) -> bool {
        // Fail closed: if we cannot tell, treat as disabled.
        !matches!(self.disable_file.try_exists(), Ok(false))
    }

    /// Admin public key fingerprint (safe to print).
    #[must_use]
    pub fn admin_fingerprint(&self) -> String {
        crate::keys::fingerprint(&self.admin)
    }

    /// Decide one request at Unix time `now`. Same as
    /// [`Approver::approve_audited`] without the audit outcome.
    ///
    /// Prefer [`Approver::approve_audited`] in anything that runs as a service:
    /// a refusal whose audit line cannot be written is otherwise invisible
    /// (finding 4 of cortex-s2-d, fixed 2026-10-21).
    #[must_use]
    pub fn approve(&self, req: Request<'_>, now: u64) -> Verdict {
        self.approve_audited(req, now).verdict
    }

    /// Decide one request at Unix time `now` and report whether its ledger
    /// line was written.
    ///
    /// Approvals are written before they are returned (mandatory, fsynced): a
    /// failed write turns them into [`Refusal::Ledger`]. Refusals and dry-runs
    /// are then written as audit lines; if that write fails the verdict stands
    /// (a refusal stays a refusal, a dry-run grants nothing) but
    /// [`Decided::audit`] is [`Audit::Failed`], so the caller can make the
    /// failure loud. A refusal because the ledger itself failed is
    /// [`Audit::Failed`] too, even when its audit line happened to land: the
    /// gate cannot approve anything while that holds.
    #[must_use]
    pub fn approve_audited(&self, req: Request<'_>, now: u64) -> Decided {
        let verdict = self.decide(req, now);
        let noted = match &verdict {
            Verdict::Approved(_) => Ok(()),
            Verdict::DryRun => self
                .ledger
                .note("dry-run", req.action, req.target, "no token", "", now),
            Verdict::Refused(r) => self.ledger.note(
                "refused",
                req.action,
                req.target,
                &r.reason(),
                req.token,
                now,
            ),
        };
        let audit = match (&verdict, noted) {
            (Verdict::Refused(Refusal::Ledger(e)), _) if *e != LedgerError::Replayed => {
                Audit::Failed(e.clone())
            }
            (_, Ok(())) => Audit::Written,
            (_, Err(e)) => Audit::Failed(e),
        };
        Decided { verdict, audit }
    }

    /// Read-only ledger probe (see [`Ledger::health`]). Writes nothing.
    ///
    /// # Errors
    /// The ledger error; while it holds, no approval can pass.
    pub fn ledger_health(&self) -> Result<crate::ledger::LedgerHealth, LedgerError> {
        self.ledger.health()
    }

    fn decide(&self, req: Request<'_>, now: u64) -> Verdict {
        if self.is_disabled() {
            return Verdict::Refused(Refusal::Disabled);
        }
        match Action::parse(req.action) {
            Some(a) if a.is_allow_listed() => {}
            _ => return Verdict::Refused(Refusal::ActionNotApprovable(req.action.to_owned())),
        }
        if !token::valid_target(req.target) {
            return Verdict::Refused(Refusal::Token(TokenError::Malformed("target name")));
        }
        if let Err(h) = crate::safety::review(req.target, req.plan) {
            return Verdict::Refused(Refusal::Safety(h));
        }
        if req.token.is_empty() {
            return Verdict::DryRun;
        }
        let scope = Scope {
            action: req.action,
            target: req.target,
            plan: req.plan,
        };
        let claims = match token::verify(req.token, &self.admin, scope, now, self.max_ttl_secs) {
            Ok(c) => c,
            Err(e) => return Verdict::Refused(Refusal::Token(e)),
        };
        // Re-check the disable switch right before consuming, so a disable
        // that lands mid-request still wins.
        if self.is_disabled() {
            return Verdict::Refused(Refusal::Disabled);
        }
        match self.ledger.consume(&claims, now) {
            Ok(()) => Verdict::Approved(claims),
            Err(e) => Verdict::Refused(Refusal::Ledger(e)),
        }
    }
}

/// Current Unix time in seconds.
#[must_use]
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
