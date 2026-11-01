// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! SQLite storage. What exists per account is deliberately tiny:
//!
//! | table            | columns                                                         |
//! |------------------|-----------------------------------------------------------------|
//! | accounts         | id, lookup (argon2id hash), plan, paid_until, stripe_customer?, stripe_subscription? |
//! | sessions         | sha256(cookie), account_id, expires_at                          |
//! | pay_refs         | opaque reference, account_id (nulled once used), method, plan, expires_at, used |
//! | processed_events | webhook event id, seen_at (replay guard, swept after 30 days)   |
//! | ledger           | day, method, amount, currency (bookkeeping; NO account link)    |
//! | crypto_invoices  | pay_ref, backend, invoice id, due (piconero), price-lock end, settled_at (NO account id; swept 2 days after settling or expiry) |
//! | duplicate_subscriptions | Stripe subscription id of a second checkout on an account that already had one (NO account id; for the operator to refund) |
//!
//! Added columns (migrated in place, see [`MIGRATIONS`]): `accounts.billing`
//! (`ok` / `past_due` / `canceling`), `accounts.cancel_at`,
//! `accounts.banked_secs` + `banked_plan` (cash/crypto time bought while a
//! card subscription runs, released when it ends), `pay_refs.url` +
//! `url_until` (an open Stripe Checkout page, reused instead of a second one).
//!
//! No email, no name, no IP address, no user agent, no creation time.

use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::plans::Plan;

const SCHEMA: &str = "
PRAGMA journal_mode=WAL;
CREATE TABLE IF NOT EXISTS accounts(
  id INTEGER PRIMARY KEY,
  lookup BLOB NOT NULL UNIQUE,
  plan TEXT NOT NULL DEFAULT 'free',
  paid_until INTEGER NOT NULL DEFAULT 0,
  stripe_customer TEXT UNIQUE,
  stripe_subscription TEXT UNIQUE
);
CREATE TABLE IF NOT EXISTS sessions(
  token_hash BLOB PRIMARY KEY,
  account_id INTEGER NOT NULL,
  expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS pay_refs(
  reference TEXT PRIMARY KEY,
  account_id INTEGER,
  method TEXT NOT NULL,
  plan TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  used INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS processed_events(
  id TEXT PRIMARY KEY,
  seen_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ledger(
  id INTEGER PRIMARY KEY,
  day TEXT NOT NULL,
  method TEXT NOT NULL,
  amount_cents INTEGER NOT NULL,
  currency TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS crypto_invoices(
  pay_ref TEXT PRIMARY KEY,
  backend TEXT NOT NULL,
  invoice_id TEXT NOT NULL UNIQUE,
  due_atomic INTEGER NOT NULL DEFAULT 0,
  expires_at INTEGER NOT NULL,
  settled_at INTEGER
);
CREATE TABLE IF NOT EXISTS duplicate_subscriptions(
  subscription TEXT PRIMARY KEY,
  seen_at INTEGER NOT NULL
);
";

/// Columns added after the first live schema (2026-11-01, build 95244f0d).
/// Applied with `ALTER TABLE .. ADD COLUMN` when missing; never dropped.
pub const MIGRATIONS: [(&str, &str, &str); 6] = [
    ("accounts", "billing", "TEXT NOT NULL DEFAULT 'ok'"),
    ("accounts", "cancel_at", "INTEGER"),
    ("accounts", "banked_secs", "INTEGER NOT NULL DEFAULT 0"),
    ("accounts", "banked_plan", "TEXT"),
    ("pay_refs", "url", "TEXT"),
    ("pay_refs", "url_until", "INTEGER"),
];

/// Billing state of a card subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Billing {
    /// Paid, or no subscription at all.
    Ok,
    /// A renewal failed; Stripe is retrying. Access runs until the grace end.
    PastDue,
    /// Cancelled at the period end (`cancel_at`); no more renewals.
    Canceling,
}

impl Billing {
    pub fn id(self) -> &'static str {
        match self {
            Billing::Ok => "ok",
            Billing::PastDue => "past_due",
            Billing::Canceling => "canceling",
        }
    }
    pub fn parse(s: &str) -> Billing {
        match s {
            "past_due" => Billing::PastDue,
            "canceling" => Billing::Canceling,
            _ => Billing::Ok,
        }
    }
}

/// Grace after a failed renewal: access stays on this long past the end of
/// the last paid period while Stripe retries the card.
pub const PAYMENT_GRACE: i64 = 7 * DAY;

/// Seconds in a day.
pub const DAY: i64 = 86_400;
/// Unused online payment references expire after 20 days (Mullvad's figure).
pub const REF_TTL_ONLINE: i64 = 20 * DAY;
/// Cash references live 120 days because the post can be slow (Mullvad's figure).
pub const REF_TTL_CASH: i64 = 120 * DAY;
/// Webhook ids are kept 30 days (Stripe retries for up to 3 days).
pub const EVENT_TTL: i64 = 30 * DAY;

/// Crypto settlement window: an invoice (and its payment reference) lives
/// this long after its price lock ends or after it is credited, so late
/// confirmations and webhook redeliveries still land. Then both rows are
/// deleted by the sweep.
pub const CRYPTO_SETTLE_WINDOW: i64 = 2 * DAY;

/// A crypto invoice: payment reference -> processor invoice id. No account
/// id, no address, no transaction id, no amount paid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CryptoInvoice {
    pub pay_ref: String,
    pub backend: String,
    pub invoice_id: String,
    /// Monero: piconero due at the locked rate. 0 for BTCPay (BTCPay holds the
    /// price itself).
    pub due_atomic: i64,
    /// End of the price lock.
    pub expires_at: i64,
    pub settled_at: Option<i64>,
}

const CRYPTO_COLS: &str = "pay_ref, backend, invoice_id, due_atomic, expires_at, settled_at";

fn row_crypto(r: &rusqlite::Row<'_>) -> rusqlite::Result<CryptoInvoice> {
    Ok(CryptoInvoice {
        pay_ref: r.get(0)?,
        backend: r.get(1)?,
        invoice_id: r.get(2)?,
        due_atomic: r.get(3)?,
        expires_at: r.get(4)?,
        settled_at: r.get(5)?,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: i64,
    pub plan: Plan,
    pub paid_until: i64,
    pub stripe_customer: Option<String>,
    pub stripe_subscription: Option<String>,
    pub billing: Billing,
    pub cancel_at: Option<i64>,
    /// Cash/crypto time bought while a card subscription runs (seconds of
    /// `banked_plan`), added when the subscription ends.
    pub banked_secs: i64,
    pub banked_plan: Option<Plan>,
}

/// What other services get to know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entitlement {
    /// The plan in force right now (`Free` once paid time has run out).
    pub plan: Plan,
    pub active: bool,
    pub paid_until: i64,
    /// A Stripe subscription renews this account.
    pub renewing: bool,
    pub billing: Billing,
}

impl Account {
    pub fn entitlement(&self, now: i64) -> Entitlement {
        let active = self.plan != Plan::Free && self.paid_until > now;
        Entitlement {
            plan: if active { self.plan } else { Plan::Free },
            active,
            paid_until: self.paid_until,
            renewing: self.stripe_subscription.is_some() && self.billing != Billing::Canceling,
            billing: if self.stripe_subscription.is_some() {
                self.billing
            } else {
                Billing::Ok
            },
        }
    }
}

/// A payment reference handed back by [`Store::take_pay_ref`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayRef {
    pub account_id: i64,
    pub method: String,
    pub plan: Plan,
}

#[derive(Debug)]
pub enum StoreError {
    Exists,
    Db(rusqlite::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Exists => f.write_str("already exists"),
            StoreError::Db(e) => write!(f, "database: {e}"),
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Db(e)
    }
}

pub struct Store {
    db: Mutex<Connection>,
}

fn day_of(now: i64) -> String {
    galion_common::time::from_unix(now)[..10].to_string()
}

fn row_account(r: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
    let plan: String = r.get(1)?;
    Ok(Account {
        id: r.get(0)?,
        plan: Plan::parse(&plan).unwrap_or(Plan::Free),
        paid_until: r.get(2)?,
        stripe_customer: r.get(3)?,
        stripe_subscription: r.get(4)?,
        billing: Billing::parse(&r.get::<_, String>(5)?),
        cancel_at: r.get(6)?,
        banked_secs: r.get(7)?,
        banked_plan: r
            .get::<_, Option<String>>(8)?
            .as_deref()
            .and_then(Plan::parse),
    })
}

const ACCOUNT_COLS: &str = "id, plan, paid_until, stripe_customer, stripe_subscription, \
                            billing, cancel_at, banked_secs, banked_plan";

/// Seconds of `from` time worth the same money in `to` (plan switch on a
/// top-up: remaining Pro time becomes 11x as much Plus time, and back).
pub fn convert_secs(secs: i64, from: Plan, to: Plan) -> i64 {
    if secs <= 0 || from == to || from.eur_cents() == 0 || to.eur_cents() == 0 {
        return secs.max(0);
    }
    ((secs as i128) * (from.eur_cents() as i128) / (to.eur_cents() as i128)) as i64
}

/// Counts for the admin view (no contents, no ids).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdminStats {
    pub accounts: i64,
    pub active_plus: i64,
    pub active_pro: i64,
    pub subscriptions: i64,
    pub past_due: i64,
    pub canceling: i64,
    pub sessions: i64,
    pub open_refs: Vec<(String, i64)>,
    pub open_crypto: Vec<(String, i64)>,
    pub duplicate_subscriptions: i64,
    /// (month YYYY-MM, method, currency, cents) for the last 3 months.
    pub ledger: Vec<(String, String, String, i64)>,
}

impl Store {
    pub fn open(path: &std::path::Path) -> Result<Store, StoreError> {
        Self::from_conn(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Store, StoreError> {
        Self::from_conn(Connection::open_in_memory()?)
    }

    fn from_conn(c: Connection) -> Result<Store, StoreError> {
        c.execute_batch(SCHEMA)?;
        for (table, col, ddl) in MIGRATIONS {
            let has: i64 = c.query_row(
                "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2",
                params![table, col],
                |r| r.get(0),
            )?;
            if has == 0 {
                c.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {col} {ddl}"))?;
            }
        }
        c.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(Store { db: Mutex::new(c) })
    }

    /// Open a database file created with an older schema (tests).
    #[doc(hidden)]
    pub fn from_connection(c: Connection) -> Result<Store, StoreError> {
        Self::from_conn(c)
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn create_account(&self, lookup: &[u8; 32]) -> Result<i64, StoreError> {
        let db = self.db();
        match db.execute(
            "INSERT INTO accounts(lookup) VALUES (?1)",
            params![&lookup[..]],
        ) {
            Ok(_) => Ok(db.last_insert_rowid()),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(StoreError::Exists)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn find_by_lookup(&self, lookup: &[u8; 32]) -> Result<Option<Account>, StoreError> {
        Ok(self
            .db()
            .query_row(
                &format!("SELECT {ACCOUNT_COLS} FROM accounts WHERE lookup = ?1"),
                params![&lookup[..]],
                row_account,
            )
            .optional()?)
    }

    pub fn get(&self, id: i64) -> Result<Option<Account>, StoreError> {
        Ok(self
            .db()
            .query_row(
                &format!("SELECT {ACCOUNT_COLS} FROM accounts WHERE id = ?1"),
                params![id],
                row_account,
            )
            .optional()?)
    }

    fn find_by(&self, col: &str, v: &str) -> Result<Option<Account>, StoreError> {
        Ok(self
            .db()
            .query_row(
                &format!("SELECT {ACCOUNT_COLS} FROM accounts WHERE {col} = ?1"),
                params![v],
                row_account,
            )
            .optional()?)
    }

    pub fn find_by_subscription(&self, sub: &str) -> Result<Option<Account>, StoreError> {
        self.find_by("stripe_subscription", sub)
    }

    pub fn find_by_customer(&self, cus: &str) -> Result<Option<Account>, StoreError> {
        self.find_by("stripe_customer", cus)
    }

    // ---------------------------------------------------------- sessions

    pub fn create_session(
        &self,
        token_hash: &[u8; 32],
        account_id: i64,
        expires_at: i64,
    ) -> Result<(), StoreError> {
        self.db().execute(
            "INSERT INTO sessions(token_hash, account_id, expires_at) VALUES (?1, ?2, ?3)",
            params![&token_hash[..], account_id, expires_at],
        )?;
        Ok(())
    }

    pub fn session_account(
        &self,
        token_hash: &[u8; 32],
        now: i64,
    ) -> Result<Option<Account>, StoreError> {
        Ok(self
            .db()
            .query_row(
                &format!(
                    "SELECT {} FROM accounts a JOIN sessions s ON s.account_id = a.id \
                     WHERE s.token_hash = ?1 AND s.expires_at > ?2",
                    ACCOUNT_COLS
                        .split(", ")
                        .map(|c| format!("a.{c}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                params![&token_hash[..], now],
                row_account,
            )
            .optional()?)
    }

    pub fn end_session(&self, token_hash: &[u8; 32]) -> Result<(), StoreError> {
        self.db().execute(
            "DELETE FROM sessions WHERE token_hash = ?1",
            params![&token_hash[..]],
        )?;
        Ok(())
    }

    // ---------------------------------------------------------- payment refs

    /// Store a new opaque reference for `account_id`.
    pub fn put_pay_ref(
        &self,
        reference: &str,
        account_id: i64,
        method: &str,
        plan: Plan,
        expires_at: i64,
    ) -> Result<(), StoreError> {
        match self.db().execute(
            "INSERT INTO pay_refs(reference, account_id, method, plan, expires_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![reference, account_id, method, plan.id(), expires_at],
        ) {
            Ok(_) => Ok(()),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(StoreError::Exists)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Look a reference up without using it.
    pub fn peek_pay_ref(&self, reference: &str, now: i64) -> Result<Option<PayRef>, StoreError> {
        Ok(self
            .db()
            .query_row(
                "SELECT account_id, method, plan FROM pay_refs \
                 WHERE reference = ?1 AND used = 0 AND expires_at > ?2 AND account_id IS NOT NULL",
                params![reference, now],
                |r| {
                    let plan: String = r.get(2)?;
                    Ok(PayRef {
                        account_id: r.get(0)?,
                        method: r.get(1)?,
                        plan: Plan::parse(&plan).unwrap_or(Plan::Plus),
                    })
                },
            )
            .optional()?)
    }

    /// Use a reference once: returns it and cuts its link to the account
    /// (account_id set to NULL, used = 1), so after crediting nothing ties a
    /// processor's payment to an account number any more.
    pub fn take_pay_ref(&self, reference: &str, now: i64) -> Result<Option<PayRef>, StoreError> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let found = tx
            .query_row(
                "SELECT account_id, method, plan FROM pay_refs \
                 WHERE reference = ?1 AND used = 0 AND expires_at > ?2 AND account_id IS NOT NULL",
                params![reference, now],
                |r| {
                    let plan: String = r.get(2)?;
                    Ok(PayRef {
                        account_id: r.get(0)?,
                        method: r.get(1)?,
                        plan: Plan::parse(&plan).unwrap_or(Plan::Plus),
                    })
                },
            )
            .optional()?;
        if found.is_some() {
            tx.execute(
                "UPDATE pay_refs SET used = 1, account_id = NULL WHERE reference = ?1",
                params![reference],
            )?;
        }
        tx.commit()?;
        Ok(found)
    }

    // ---------------------------------------------------------- webhooks

    /// Record a webhook event id. `false` when it was already seen (replay).
    pub fn mark_event(&self, id: &str, now: i64) -> Result<bool, StoreError> {
        let n = self.db().execute(
            "INSERT OR IGNORE INTO processed_events(id, seen_at) VALUES (?1, ?2)",
            params![id, now],
        )?;
        Ok(n == 1)
    }

    /// Forget an event id again (processing failed; let the retry through).
    pub fn unmark_event(&self, id: &str) -> Result<(), StoreError> {
        self.db()
            .execute("DELETE FROM processed_events WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ---------------------------------------------------------- time

    /// Add `days` of `plan` time (cash, crypto, vouchers): paid_until =
    /// max(paid_until, now) + days. Returns the new paid_until (or, while a
    /// card subscription runs, when the banked time would end if the
    /// subscription ended now).
    ///
    /// * Remaining time on another plan is converted at the price ratio, so
    ///   a Plus top-up on running Pro time loses nothing.
    /// * While a card subscription runs, the days are banked and added when
    ///   it ends ([`Store::end_stripe`]), so the subscription's own period
    ///   end cannot swallow them.
    pub fn add_days(
        &self,
        account_id: i64,
        plan: Plan,
        days: i64,
        now: i64,
    ) -> Result<i64, StoreError> {
        let Some(a) = self.get(account_id)? else {
            return Ok(0);
        };
        let db = self.db();
        let add = days.max(0) * DAY;
        if a.stripe_subscription.is_some() {
            let banked_plan = a.banked_plan.unwrap_or(plan);
            let banked = convert_secs(a.banked_secs, banked_plan, plan) + add;
            db.execute(
                "UPDATE accounts SET banked_secs = ?2, banked_plan = ?3 WHERE id = ?1",
                params![account_id, banked, plan.id()],
            )?;
            return Ok(a.paid_until.max(now) + banked);
        }
        let remaining = if a.plan != Plan::Free && a.paid_until > now {
            convert_secs(a.paid_until - now, a.plan, plan)
        } else {
            0
        };
        let until = now + remaining + add;
        db.execute(
            "UPDATE accounts SET plan = ?2, paid_until = ?3 WHERE id = ?1",
            params![account_id, plan.id(), until],
        )?;
        Ok(until)
    }

    /// Billing state of the card subscription (and when it cancels).
    pub fn set_billing(
        &self,
        account_id: i64,
        billing: Billing,
        cancel_at: Option<i64>,
    ) -> Result<(), StoreError> {
        self.db().execute(
            "UPDATE accounts SET billing = ?2, cancel_at = ?3 WHERE id = ?1",
            params![account_id, billing.id(), cancel_at],
        )?;
        Ok(())
    }

    /// A renewal failed: mark past due and keep access until `grace_until`
    /// (never shortens paid time).
    pub fn payment_failed(&self, account_id: i64, grace_until: i64) -> Result<(), StoreError> {
        self.db().execute(
            "UPDATE accounts SET billing = 'past_due', paid_until = MAX(paid_until, ?2) WHERE id = ?1",
            params![account_id, grace_until],
        )?;
        Ok(())
    }

    /// A card payment was refunded in full (or charged back): the time it
    /// bought stops now. Banked cash/crypto time is kept.
    pub fn cut_to(&self, account_id: i64, at: i64) -> Result<(), StoreError> {
        self.db().execute(
            "UPDATE accounts SET paid_until = MIN(paid_until, ?2) WHERE id = ?1",
            params![account_id, at],
        )?;
        Ok(())
    }

    /// Remember a second subscription that checkout created for an account
    /// that already had one (the operator refunds and cancels it).
    pub fn record_duplicate(&self, subscription: &str, now: i64) -> Result<(), StoreError> {
        self.db().execute(
            "INSERT OR IGNORE INTO duplicate_subscriptions(subscription, seen_at) VALUES (?1, ?2)",
            params![subscription, now],
        )?;
        Ok(())
    }

    pub fn duplicates(&self) -> Result<Vec<String>, StoreError> {
        let db = self.db();
        let mut st =
            db.prepare("SELECT subscription FROM duplicate_subscriptions ORDER BY seen_at")?;
        let rows = st.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Remember the Checkout page for a payment reference.
    pub fn set_ref_url(&self, reference: &str, url: &str, until: i64) -> Result<(), StoreError> {
        self.db().execute(
            "UPDATE pay_refs SET url = ?2, url_until = ?3 WHERE reference = ?1",
            params![reference, url, until],
        )?;
        Ok(())
    }

    /// An unused Checkout page of `account_id` for `plan` that is still
    /// open, so a double click does not make a second subscription.
    pub fn open_checkout(
        &self,
        account_id: i64,
        plan: Plan,
        now: i64,
    ) -> Result<Option<String>, StoreError> {
        Ok(self
            .db()
            .query_row(
                "SELECT url FROM pay_refs WHERE account_id = ?1 AND method = 'stripe' AND plan = ?2 \
                 AND used = 0 AND url IS NOT NULL AND url_until > ?3 ORDER BY url_until DESC LIMIT 1",
                params![account_id, plan.id(), now],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// What an operator may see about an open reference: method, plan and
    /// expiry. Never the account.
    pub fn pay_ref_info(
        &self,
        reference: &str,
        now: i64,
    ) -> Result<Option<(String, Plan, i64)>, StoreError> {
        Ok(self
            .db()
            .query_row(
                "SELECT method, plan, expires_at FROM pay_refs \
                 WHERE reference = ?1 AND used = 0 AND expires_at > ?2 AND account_id IS NOT NULL",
                params![reference, now],
                |r| {
                    let p: String = r.get(1)?;
                    Ok((r.get(0)?, Plan::parse(&p).unwrap_or(Plan::Plus), r.get(2)?))
                },
            )
            .optional()?)
    }

    /// Open (unused, unexpired) references of one account and method.
    pub fn open_refs_of(
        &self,
        account_id: i64,
        method: &str,
        now: i64,
    ) -> Result<Vec<(String, Plan, i64)>, StoreError> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT reference, plan, expires_at FROM pay_refs WHERE account_id = ?1 \
             AND method = ?2 AND used = 0 AND expires_at > ?3 ORDER BY expires_at",
        )?;
        let rows = st.query_map(params![account_id, method, now], |r| {
            let p: String = r.get(1)?;
            Ok((r.get(0)?, Plan::parse(&p).unwrap_or(Plan::Plus), r.get(2)?))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Counts for the admin status view.
    pub fn admin_stats(&self, now: i64) -> Result<AdminStats, StoreError> {
        let db = self.db();
        let one = |sql: &str| -> Result<i64, StoreError> {
            Ok(db.query_row(sql, params![now], |r| r.get(0))?)
        };
        let mut s = AdminStats {
            accounts: one("SELECT COUNT(*) FROM accounts WHERE ?1 = ?1")?,
            active_plus: one("SELECT COUNT(*) FROM accounts WHERE plan = 'plus' AND paid_until > ?1")?,
            active_pro: one("SELECT COUNT(*) FROM accounts WHERE plan = 'pro' AND paid_until > ?1")?,
            subscriptions: one(
                "SELECT COUNT(*) FROM accounts WHERE stripe_subscription IS NOT NULL AND ?1 = ?1",
            )?,
            past_due: one(
                "SELECT COUNT(*) FROM accounts WHERE stripe_subscription IS NOT NULL AND billing = 'past_due' AND ?1 = ?1",
            )?,
            canceling: one(
                "SELECT COUNT(*) FROM accounts WHERE stripe_subscription IS NOT NULL AND billing = 'canceling' AND ?1 = ?1",
            )?,
            sessions: one("SELECT COUNT(*) FROM sessions WHERE expires_at > ?1")?,
            duplicate_subscriptions: one(
                "SELECT COUNT(*) FROM duplicate_subscriptions WHERE ?1 = ?1",
            )?,
            ..AdminStats::default()
        };
        let mut st = db.prepare(
            "SELECT method, COUNT(*) FROM pay_refs WHERE used = 0 AND expires_at > ?1 GROUP BY method ORDER BY method",
        )?;
        s.open_refs = st
            .query_map(params![now], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let mut st = db.prepare(
            "SELECT backend, COUNT(*) FROM crypto_invoices WHERE settled_at IS NULL AND expires_at + ?2 > ?1 GROUP BY backend ORDER BY backend",
        )?;
        s.open_crypto = st
            .query_map(params![now, CRYPTO_SETTLE_WINDOW], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<Result<_, _>>()?;
        let since = &day_of(now - 92 * DAY)[..7];
        let mut st = db.prepare(
            "SELECT substr(day, 1, 7) AS m, method, currency, SUM(amount_cents) FROM ledger \
             WHERE substr(day, 1, 7) >= ?1 GROUP BY m, method, currency ORDER BY m, method, currency",
        )?;
        s.ledger = st
            .query_map(params![since], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<Result<_, _>>()?;
        Ok(s)
    }

    /// Attach a Stripe customer and subscription after checkout.
    pub fn link_stripe(
        &self,
        account_id: i64,
        customer: Option<&str>,
        subscription: Option<&str>,
        plan: Plan,
        now: i64,
    ) -> Result<(), StoreError> {
        // Prepaid (cash/crypto) time on a different plan is banked in its own
        // plan and comes back when the subscription ends. Same-plan prepaid
        // time is covered by the checkout's trial_end instead.
        if let Some(a) = self.get(account_id)? {
            if a.stripe_subscription.is_none()
                && a.plan != Plan::Free
                && a.plan != plan
                && a.paid_until > now
            {
                let bp = a.banked_plan.unwrap_or(a.plan);
                let banked = convert_secs(a.banked_secs, bp, a.plan) + (a.paid_until - now);
                self.db().execute(
                    "UPDATE accounts SET banked_secs = ?2, banked_plan = ?3, paid_until = ?4 WHERE id = ?1",
                    params![account_id, banked, a.plan.id(), now],
                )?;
            }
        }
        let db = self.db();
        // A customer/subscription id can belong to one account only.
        if let Some(s) = subscription {
            db.execute(
                "UPDATE accounts SET stripe_subscription = NULL WHERE stripe_subscription = ?1 AND id != ?2",
                params![s, account_id],
            )?;
        }
        if let Some(c) = customer {
            db.execute(
                "UPDATE accounts SET stripe_customer = NULL WHERE stripe_customer = ?1 AND id != ?2",
                params![c, account_id],
            )?;
        }
        db.execute(
            "UPDATE accounts SET plan = ?2, \
             stripe_customer = COALESCE(?3, stripe_customer), \
             stripe_subscription = COALESCE(?4, stripe_subscription) WHERE id = ?1",
            params![account_id, plan.id(), customer, subscription],
        )?;
        Ok(())
    }

    /// A subscription renewed or changed: paid_until never moves backwards
    /// here (already paid time is kept); `plan` changes when given.
    pub fn renew(&self, account_id: i64, plan: Option<Plan>, until: i64) -> Result<(), StoreError> {
        let db = self.db();
        if let Some(p) = plan {
            db.execute(
                "UPDATE accounts SET plan = ?2 WHERE id = ?1",
                params![account_id, p.id()],
            )?;
        }
        db.execute(
            "UPDATE accounts SET paid_until = MAX(paid_until, ?2) WHERE id = ?1",
            params![account_id, until],
        )?;
        Ok(())
    }

    /// The subscription ended at `ended_at`: paid time stops there (or
    /// earlier if it already ran out), cash/crypto time banked meanwhile is
    /// added on top, and the Stripe ids are forgotten.
    pub fn end_stripe(&self, account_id: i64, ended_at: i64) -> Result<(), StoreError> {
        let Some(a) = self.get(account_id)? else {
            return Ok(());
        };
        let mut until = a.paid_until.min(ended_at);
        let mut plan = a.plan;
        if a.banked_secs > 0 {
            let bp = a.banked_plan.unwrap_or(a.plan);
            until = until.max(ended_at) + a.banked_secs;
            plan = bp;
        }
        self.db().execute(
            "UPDATE accounts SET paid_until = ?2, plan = ?3, banked_secs = 0, banked_plan = NULL, \
             billing = 'ok', cancel_at = NULL, \
             stripe_subscription = NULL, stripe_customer = NULL WHERE id = ?1",
            params![account_id, until, plan.id()],
        )?;
        Ok(())
    }

    /// A bookkeeping line with no link to any account.
    pub fn ledger_add(
        &self,
        now: i64,
        method: &str,
        amount_cents: i64,
        currency: &str,
    ) -> Result<(), StoreError> {
        self.db().execute(
            "INSERT INTO ledger(day, method, amount_cents, currency) VALUES (?1, ?2, ?3, ?4)",
            params![day_of(now), method, amount_cents, currency],
        )?;
        Ok(())
    }

    pub fn ledger_count(&self) -> Result<i64, StoreError> {
        Ok(self
            .db()
            .query_row("SELECT COUNT(*) FROM ledger", [], |r| r.get(0))?)
    }

    // ---------------------------------------------------------- crypto invoices

    /// Remember which processor invoice belongs to a payment reference. The
    /// row holds no account id: the only link to an account is the
    /// `pay_refs` row, which is cut when the invoice is credited.
    pub fn put_crypto_invoice(
        &self,
        pay_ref: &str,
        backend: &str,
        invoice_id: &str,
        due_atomic: i64,
        expires_at: i64,
    ) -> Result<(), StoreError> {
        match self.db().execute(
            "INSERT INTO crypto_invoices(pay_ref, backend, invoice_id, due_atomic, expires_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![pay_ref, backend, invoice_id, due_atomic, expires_at],
        ) {
            Ok(_) => Ok(()),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(StoreError::Exists)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// The invoice with this processor id (BTCPay invoice id, or the Monero
    /// `sub:<account>/<index>` / `pid:<payment id>` key).
    pub fn crypto_invoice(&self, invoice_id: &str) -> Result<Option<CryptoInvoice>, StoreError> {
        Ok(self
            .db()
            .query_row(
                &format!("SELECT {CRYPTO_COLS} FROM crypto_invoices WHERE invoice_id = ?1"),
                params![invoice_id],
                row_crypto,
            )
            .optional()?)
    }

    /// Invoices of `backend` that are not settled yet.
    pub fn open_crypto_invoices(&self, backend: &str) -> Result<Vec<CryptoInvoice>, StoreError> {
        let db = self.db();
        let mut st = db.prepare(&format!(
            "SELECT {CRYPTO_COLS} FROM crypto_invoices \
             WHERE backend = ?1 AND settled_at IS NULL ORDER BY expires_at"
        ))?;
        let rows = st.query_map(params![backend], row_crypto)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Mark an invoice credited (kept only until the settlement window ends,
    /// to answer redeliveries, then swept).
    pub fn settle_crypto_invoice(&self, pay_ref: &str, now: i64) -> Result<(), StoreError> {
        self.db().execute(
            "UPDATE crypto_invoices SET settled_at = ?2 WHERE pay_ref = ?1 AND settled_at IS NULL",
            params![pay_ref, now],
        )?;
        Ok(())
    }

    /// Data-minimisation sweep: expired sessions, used or expired payment
    /// references, webhook ids older than [`EVENT_TTL`] and crypto invoices
    /// past [`CRYPTO_SETTLE_WINDOW`]. Returns rows removed.
    pub fn sweep(&self, now: i64) -> Result<usize, StoreError> {
        let db = self.db();
        let mut n = db.execute("DELETE FROM sessions WHERE expires_at <= ?1", params![now])?;
        n += db.execute(
            "DELETE FROM pay_refs WHERE used = 1 OR expires_at <= ?1",
            params![now],
        )?;
        n += db.execute(
            "DELETE FROM processed_events WHERE seen_at <= ?1",
            params![now - EVENT_TTL],
        )?;
        n += db.execute(
            "DELETE FROM crypto_invoices WHERE COALESCE(settled_at, expires_at) <= ?1",
            params![now - CRYPTO_SETTLE_WINDOW],
        )?;
        Ok(n)
    }

    /// Every stored value as text, table by table (tests check that no
    /// account number or personal data is ever written).
    #[doc(hidden)]
    pub fn debug_dump(&self) -> Vec<String> {
        use rusqlite::types::ValueRef;
        let db = self.db();
        let mut out = Vec::new();
        for t in [
            "accounts",
            "sessions",
            "pay_refs",
            "processed_events",
            "ledger",
            "crypto_invoices",
            "duplicate_subscriptions",
        ] {
            let Ok(mut st) = db.prepare(&format!("SELECT * FROM {t}")) else {
                continue;
            };
            let cols = st.column_count();
            let Ok(mut rows) = st.query([]) else { continue };
            while let Ok(Some(r)) = rows.next() {
                let mut line = format!("{t}:");
                for i in 0..cols {
                    let v = match r.get_ref(i) {
                        Ok(ValueRef::Null) => "NULL".to_string(),
                        Ok(ValueRef::Integer(n)) => n.to_string(),
                        Ok(ValueRef::Real(f)) => f.to_string(),
                        Ok(ValueRef::Text(s)) => String::from_utf8_lossy(s).into_owned(),
                        Ok(ValueRef::Blob(b)) => galion_common::hex::encode(b),
                        Err(_) => "?".into(),
                    };
                    line.push(' ');
                    line.push_str(&v);
                }
                out.push(line);
            }
        }
        out
    }

    /// Row counts for /internal/v1/health (no contents).
    pub fn counts(&self) -> Result<(i64, i64, i64), StoreError> {
        let db = self.db();
        let a = db.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?;
        let s = db.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
        let p = db.query_row("SELECT COUNT(*) FROM pay_refs", [], |r| r.get(0))?;
        Ok((a, s, p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The schema the live 95244f0d binary created (copied from that commit).
    const SCHEMA_95244F0D: &str = "
CREATE TABLE accounts(id INTEGER PRIMARY KEY, lookup BLOB NOT NULL UNIQUE,
  plan TEXT NOT NULL DEFAULT 'free', paid_until INTEGER NOT NULL DEFAULT 0,
  stripe_customer TEXT UNIQUE, stripe_subscription TEXT UNIQUE);
CREATE TABLE sessions(token_hash BLOB PRIMARY KEY, account_id INTEGER NOT NULL, expires_at INTEGER NOT NULL);
CREATE TABLE pay_refs(reference TEXT PRIMARY KEY, account_id INTEGER, method TEXT NOT NULL,
  plan TEXT NOT NULL, expires_at INTEGER NOT NULL, used INTEGER NOT NULL DEFAULT 0);
CREATE TABLE processed_events(id TEXT PRIMARY KEY, seen_at INTEGER NOT NULL);
CREATE TABLE ledger(id INTEGER PRIMARY KEY, day TEXT NOT NULL, method TEXT NOT NULL,
  amount_cents INTEGER NOT NULL, currency TEXT NOT NULL);
CREATE TABLE crypto_invoices(pay_ref TEXT PRIMARY KEY, backend TEXT NOT NULL,
  invoice_id TEXT NOT NULL UNIQUE, due_atomic INTEGER NOT NULL DEFAULT 0,
  expires_at INTEGER NOT NULL, settled_at INTEGER);
INSERT INTO accounts(id, lookup, plan, paid_until, stripe_customer, stripe_subscription)
  VALUES (1, x'0101010101010101010101010101010101010101010101010101010101010101', 'plus', 5000000, 'cus_old', 'sub_old');
INSERT INTO pay_refs VALUES ('GC-AAAA-BBBB-CCCC', 1, 'cash', 'plus', 9000000, 0);
";

    #[test]
    fn upgrades_a_95244f0d_database_in_place() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(SCHEMA_95244F0D).unwrap();
        let s = Store::from_connection(c).unwrap();
        let a = s.get(1).unwrap().unwrap();
        assert_eq!(a.plan, Plan::Plus);
        assert_eq!(a.paid_until, 5_000_000);
        assert_eq!(a.stripe_subscription.as_deref(), Some("sub_old"));
        assert_eq!(a.billing, Billing::Ok);
        assert_eq!(a.cancel_at, None);
        assert_eq!(a.banked_secs, 0);
        // New columns and tables work on the upgraded file.
        s.payment_failed(1, 6_000_000).unwrap();
        assert_eq!(s.get(1).unwrap().unwrap().billing, Billing::PastDue);
        assert_eq!(s.open_refs_of(1, "cash", 1000).unwrap().len(), 1);
        s.record_duplicate("sub_dup", 1000).unwrap();
        assert_eq!(s.admin_stats(1000).unwrap().duplicate_subscriptions, 1);
        // Opening again is a no-op.
        let c = s.db.into_inner().unwrap();
        let s = Store::from_connection(c).unwrap();
        assert_eq!(s.get(1).unwrap().unwrap().billing, Billing::PastDue);
    }

    #[test]
    fn account_lifecycle() {
        let s = Store::in_memory().unwrap();
        let id = s.create_account(&[7; 32]).unwrap();
        assert!(matches!(
            s.create_account(&[7; 32]),
            Err(StoreError::Exists)
        ));
        let a = s.find_by_lookup(&[7; 32]).unwrap().unwrap();
        assert_eq!(a.id, id);
        assert!(!a.entitlement(100).active);
        let until = s.add_days(id, Plan::Plus, 30, 1000).unwrap();
        assert_eq!(until, 1000 + 30 * DAY);
        let until = s.add_days(id, Plan::Plus, 30, 2000).unwrap();
        assert_eq!(until, 1000 + 60 * DAY);
        let e = s.get(id).unwrap().unwrap().entitlement(2000);
        assert!(e.active);
        assert_eq!(e.plan, Plan::Plus);
        assert_eq!(
            s.get(id).unwrap().unwrap().entitlement(until).plan,
            Plan::Free
        );
    }

    #[test]
    fn pay_ref_used_once_and_unlinked() {
        let s = Store::in_memory().unwrap();
        let id = s.create_account(&[1; 32]).unwrap();
        s.put_pay_ref("GC-1", id, "cash", Plan::Pro, 500).unwrap();
        assert!(s.take_pay_ref("GC-1", 600).unwrap().is_none(), "expired");
        s.put_pay_ref("GC-2", id, "cash", Plan::Pro, 5000).unwrap();
        let r = s.take_pay_ref("GC-2", 600).unwrap().unwrap();
        assert_eq!((r.account_id, r.plan), (id, Plan::Pro));
        assert!(s.take_pay_ref("GC-2", 600).unwrap().is_none(), "used");
        let linked: Option<i64> = s
            .db()
            .query_row(
                "SELECT account_id FROM pay_refs WHERE reference='GC-2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(linked, None);
        assert_eq!(s.sweep(600).unwrap(), 2);
    }

    #[test]
    fn events_dedupe_and_sweep() {
        let s = Store::in_memory().unwrap();
        assert!(s.mark_event("evt_1", 10).unwrap());
        assert!(!s.mark_event("evt_1", 11).unwrap());
        s.sweep(10 + EVENT_TTL).unwrap();
        assert!(s.mark_event("evt_1", 12).unwrap());
    }

    #[test]
    fn stripe_link_renew_end() {
        let s = Store::in_memory().unwrap();
        let id = s.create_account(&[2; 32]).unwrap();
        s.link_stripe(id, Some("cus_A"), Some("sub_A"), Plan::Plus, 0)
            .unwrap();
        s.renew(id, None, 5000).unwrap();
        s.renew(id, Some(Plan::Pro), 4000).unwrap();
        let a = s.find_by_subscription("sub_A").unwrap().unwrap();
        assert_eq!((a.plan, a.paid_until), (Plan::Pro, 5000));
        s.end_stripe(id, 3000).unwrap();
        let a = s.get(id).unwrap().unwrap();
        assert_eq!(a.paid_until, 3000);
        assert!(a.stripe_customer.is_none() && a.stripe_subscription.is_none());
    }

    #[test]
    fn ledger_has_no_account_column() {
        let s = Store::in_memory().unwrap();
        s.ledger_add(0, "cash", 2000, "EUR").unwrap();
        let cols: Vec<String> = s
            .db()
            .prepare("SELECT name FROM pragma_table_info('ledger')")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(!cols.iter().any(|c| c.contains("account")));
    }
}
