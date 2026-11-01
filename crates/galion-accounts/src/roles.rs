// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Roles, the admin record and the per-product access whitelist
//! (hand-off 02-accounts-security, 2026-11-01 CET).
//!
//! This lives in its OWN SQLite file (`access.sqlite`, next to
//! `accounts.sqlite`), never in the numbered-accounts database: the account
//! database stays email-free (DESIGN.md "a number, not a person"), and only
//! people who ask to be whitelisted by email, plus the admin, appear here.
//!
//! | table            | columns                                                     |
//! |------------------|-------------------------------------------------------------|
//! | schema_meta      | key, value (`access_schema_version`)                        |
//! | roles            | name (admin / user / whitelist_pending), description         |
//! | products         | id, label, wave, available                                   |
//! | principals       | id, email (unique, lower-case), role, account_id?, created_at, updated_at |
//! | access_whitelist | email, product, status, added_by, added_at  (PK email+product) |
//! | access_audit     | id, at, actor, action, subject, detail                       |
//!
//! `access_whitelist` is the contract with part 03 (VPN whitelist): see
//! SCHEMA.md in the hand-off package. Statuses are `pending`, `approved` and
//! `revoked`; rows are never deleted, a revoke is an update.
//!
//! [`init`] is idempotent: run it any number of times with the same email and
//! the database ends in the same state (one admin principal, the three roles,
//! the four products, approved whitelist rows for the admin on every product).
//! It stores no secrets: no password, no token, no key.

use rusqlite::{params, Connection, OptionalExtension};

/// Bumped when the access schema changes shape.
pub const ACCESS_SCHEMA_VERSION: i64 = 1;

pub const ROLE_ADMIN: &str = "admin";
pub const ROLE_USER: &str = "user";
pub const ROLE_PENDING: &str = "whitelist_pending";

/// The roles, in order of power.
pub const ROLES: [(&str, &str); 3] = [
    (
        ROLE_ADMIN,
        "Owner/admin: every product, user and whitelist management, server control",
    ),
    (
        ROLE_USER,
        "Signed-in user: products where their whitelist row is approved",
    ),
    (
        ROLE_PENDING,
        "Signed in or asked by email, waiting for the admin to approve",
    ),
];

/// A product people are whitelisted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Product {
    pub id: &'static str,
    pub label: &'static str,
    /// Whitelist ordering: wave 1 opens first, higher waves later.
    pub wave: i64,
    /// `false`: the product is not open to non-admins yet (admin still has it).
    pub available: bool,
}

/// Hermes Research and Galion One open first; the Obsidian Open Mind stack
/// follows; Galion OS comes later (owner order, 2026-11-01).
pub const PRODUCTS: [Product; 4] = [
    Product {
        id: "hermes-research",
        label: "Hermes Research",
        wave: 1,
        available: true,
    },
    Product {
        id: "galion-one",
        label: "Galion One",
        wave: 1,
        available: true,
    },
    Product {
        id: "openmind-stack",
        label: "Obsidian Open Mind stack",
        wave: 2,
        available: false,
    },
    Product {
        id: "galion-os",
        label: "Galion OS",
        wave: 3,
        available: false,
    },
];

pub const STATUS_PENDING: &str = "pending";
pub const STATUS_APPROVED: &str = "approved";
pub const STATUS_REVOKED: &str = "revoked";

const SCHEMA: &str = "
PRAGMA foreign_keys=ON;
CREATE TABLE IF NOT EXISTS schema_meta(
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS roles(
  name TEXT PRIMARY KEY,
  description TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS products(
  id TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  wave INTEGER NOT NULL,
  available INTEGER NOT NULL DEFAULT 0 CHECK (available IN (0, 1))
);
CREATE TABLE IF NOT EXISTS principals(
  id INTEGER PRIMARY KEY,
  email TEXT NOT NULL UNIQUE,
  role TEXT NOT NULL REFERENCES roles(name),
  account_id INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS access_whitelist(
  email TEXT NOT NULL,
  product TEXT NOT NULL REFERENCES products(id),
  status TEXT NOT NULL CHECK (status IN ('pending', 'approved', 'revoked')),
  added_by TEXT NOT NULL,
  added_at INTEGER NOT NULL,
  PRIMARY KEY (email, product)
);
CREATE INDEX IF NOT EXISTS access_whitelist_product_status
  ON access_whitelist(product, status);
CREATE TABLE IF NOT EXISTS access_audit(
  id INTEGER PRIMARY KEY,
  at INTEGER NOT NULL,
  actor TEXT NOT NULL,
  action TEXT NOT NULL,
  subject TEXT NOT NULL,
  detail TEXT NOT NULL DEFAULT ''
);
";

/// Errors from the access store.
#[derive(Debug)]
pub enum AccessError {
    BadEmail(&'static str),
    Db(rusqlite::Error),
}

impl std::fmt::Display for AccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccessError::BadEmail(w) => write!(f, "bad email: {w}"),
            AccessError::Db(e) => write!(f, "database: {e}"),
        }
    }
}

impl From<rusqlite::Error> for AccessError {
    fn from(e: rusqlite::Error) -> Self {
        AccessError::Db(e)
    }
}

/// Trim and lower-case an email and check its shape (one `@`, a dot in the
/// domain, no spaces or control characters, at most 254 bytes). Not a full
/// RFC 5322 parser: delivery of a magic link is the real proof.
pub fn normalize_email(raw: &str) -> Result<String, AccessError> {
    let e = raw.trim().to_ascii_lowercase();
    if e.is_empty() {
        return Err(AccessError::BadEmail("empty"));
    }
    if e.len() > 254 {
        return Err(AccessError::BadEmail("longer than 254 bytes"));
    }
    if e.chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '<' || c == '>')
    {
        return Err(AccessError::BadEmail(
            "spaces, quotes or control characters",
        ));
    }
    let Some((local, domain)) = e.split_once('@') else {
        return Err(AccessError::BadEmail("no @"));
    };
    if local.is_empty() || local.len() > 64 || domain.contains('@') {
        return Err(AccessError::BadEmail(
            "one non-empty part before a single @",
        ));
    }
    if !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
        || domain.contains("..")
    {
        return Err(AccessError::BadEmail("domain needs a dot"));
    }
    Ok(e)
}

/// What [`init`] did, for the operator's summary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InitReport {
    pub admin_email: String,
    /// The admin principal was created on this run.
    pub admin_created: bool,
    /// An existing principal with this email was promoted to admin.
    pub admin_promoted: bool,
    /// Whitelist rows inserted or set back to approved for the admin.
    pub admin_grants_changed: usize,
    pub roles: usize,
    pub products: usize,
    /// Other principals that are also admin (shown, never changed).
    pub other_admins: usize,
}

/// The access database.
pub struct Access {
    db: Connection,
}

impl Access {
    pub fn open(path: &std::path::Path) -> Result<Access, AccessError> {
        Self::from_conn(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Access, AccessError> {
        Self::from_conn(Connection::open_in_memory()?)
    }

    fn from_conn(c: Connection) -> Result<Access, AccessError> {
        c.busy_timeout(std::time::Duration::from_secs(5))?;
        c.execute_batch(SCHEMA)?;
        Ok(Access { db: c })
    }

    pub fn conn(&self) -> &Connection {
        &self.db
    }

    fn audit(
        &self,
        now: i64,
        actor: &str,
        action: &str,
        subject: &str,
        detail: &str,
    ) -> Result<(), AccessError> {
        self.db.execute(
            "INSERT INTO access_audit(at, actor, action, subject, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![now, actor, action, subject, detail],
        )?;
        Ok(())
    }

    /// Set up the schema, the roles, the products and the admin record.
    /// Idempotent; one transaction; writes an audit row only when something
    /// changed.
    pub fn init(&mut self, admin_email: &str, now: i64) -> Result<InitReport, AccessError> {
        let email = normalize_email(admin_email)?;
        let tx = self.db.transaction()?;
        tx.execute(
            "INSERT INTO schema_meta(key, value) VALUES ('access_schema_version', ?1) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![ACCESS_SCHEMA_VERSION.to_string()],
        )?;
        for (name, desc) in ROLES {
            tx.execute(
                "INSERT INTO roles(name, description) VALUES (?1, ?2) \
                 ON CONFLICT(name) DO UPDATE SET description = excluded.description",
                params![name, desc],
            )?;
        }
        // Products: insert with the seeded wave/availability; on a re-run keep
        // whatever the admin has since changed (`available`, `wave`).
        for p in PRODUCTS {
            tx.execute(
                "INSERT INTO products(id, label, wave, available) VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(id) DO UPDATE SET label = excluded.label",
                params![p.id, p.label, p.wave, p.available as i64],
            )?;
        }
        let existing: Option<String> = tx
            .query_row(
                "SELECT role FROM principals WHERE email = ?1",
                params![email],
                |r| r.get(0),
            )
            .optional()?;
        let mut report = InitReport {
            admin_email: email.clone(),
            ..Default::default()
        };
        match existing.as_deref() {
            None => {
                tx.execute(
                    "INSERT INTO principals(email, role, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
                    params![email, ROLE_ADMIN, now],
                )?;
                report.admin_created = true;
            }
            Some(ROLE_ADMIN) => {}
            Some(_) => {
                tx.execute(
                    "UPDATE principals SET role = ?2, updated_at = ?3 WHERE email = ?1",
                    params![email, ROLE_ADMIN, now],
                )?;
                report.admin_promoted = true;
            }
        }
        for p in PRODUCTS {
            let status: Option<String> = tx
                .query_row(
                    "SELECT status FROM access_whitelist WHERE email = ?1 AND product = ?2",
                    params![email, p.id],
                    |r| r.get(0),
                )
                .optional()?;
            match status.as_deref() {
                Some(STATUS_APPROVED) => {}
                Some(_) => {
                    tx.execute(
                        "UPDATE access_whitelist SET status = ?3, added_by = 'init', added_at = ?4 \
                         WHERE email = ?1 AND product = ?2",
                        params![email, p.id, STATUS_APPROVED, now],
                    )?;
                    report.admin_grants_changed += 1;
                }
                None => {
                    tx.execute(
                        "INSERT INTO access_whitelist(email, product, status, added_by, added_at) \
                         VALUES (?1, ?2, ?3, 'init', ?4)",
                        params![email, p.id, STATUS_APPROVED, now],
                    )?;
                    report.admin_grants_changed += 1;
                }
            }
        }
        report.roles =
            tx.query_row("SELECT COUNT(*) FROM roles", [], |r| r.get::<_, i64>(0))? as usize;
        report.products =
            tx.query_row("SELECT COUNT(*) FROM products", [], |r| r.get::<_, i64>(0))? as usize;
        report.other_admins = tx.query_row(
            "SELECT COUNT(*) FROM principals WHERE role = ?1 AND email != ?2",
            params![ROLE_ADMIN, email],
            |r| r.get::<_, i64>(0),
        )? as usize;
        tx.commit()?;
        if report.admin_created || report.admin_promoted || report.admin_grants_changed > 0 {
            let detail = format!(
                "created={} promoted={} grants={}",
                report.admin_created, report.admin_promoted, report.admin_grants_changed
            );
            self.audit(now, "init", "admin.init", &email, &detail)?;
        }
        Ok(report)
    }

    /// Someone asks for access by email: a `pending` row (and a
    /// `whitelist_pending` principal if they are new). Never downgrades an
    /// approved or revoked row; the admin decides those.
    pub fn request(&self, email: &str, product: &str, now: i64) -> Result<String, AccessError> {
        let email = normalize_email(email)?;
        self.db.execute(
            "INSERT INTO principals(email, role, created_at, updated_at) VALUES (?1, ?2, ?3, ?3) \
             ON CONFLICT(email) DO NOTHING",
            params![email, ROLE_PENDING, now],
        )?;
        self.db.execute(
            "INSERT INTO access_whitelist(email, product, status, added_by, added_at) \
             VALUES (?1, ?2, ?3, 'self', ?4) ON CONFLICT(email, product) DO NOTHING",
            params![email, product, STATUS_PENDING, now],
        )?;
        self.status(&email, product)
            .map(|s| s.unwrap_or_else(|| STATUS_PENDING.into()))
    }

    /// The admin approves or revokes one email for one product.
    pub fn decide(
        &self,
        admin: &str,
        email: &str,
        product: &str,
        approve: bool,
        now: i64,
    ) -> Result<(), AccessError> {
        let email = normalize_email(email)?;
        let status = if approve {
            STATUS_APPROVED
        } else {
            STATUS_REVOKED
        };
        self.db.execute(
            "INSERT INTO principals(email, role, created_at, updated_at) VALUES (?1, ?2, ?3, ?3) \
             ON CONFLICT(email) DO NOTHING",
            params![email, ROLE_PENDING, now],
        )?;
        self.db.execute(
            "INSERT INTO access_whitelist(email, product, status, added_by, added_at) VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(email, product) DO UPDATE SET status = excluded.status, \
             added_by = excluded.added_by, added_at = excluded.added_at",
            params![email, product, status, admin, now],
        )?;
        if approve {
            self.db.execute(
                "UPDATE principals SET role = ?2, updated_at = ?3 WHERE email = ?1 AND role = ?4",
                params![email, ROLE_USER, now, ROLE_PENDING],
            )?;
        }
        self.audit(
            now,
            admin,
            if approve {
                "whitelist.approve"
            } else {
                "whitelist.revoke"
            },
            &email,
            product,
        )
    }

    pub fn status(&self, email: &str, product: &str) -> Result<Option<String>, AccessError> {
        let email = normalize_email(email)?;
        Ok(self
            .db
            .query_row(
                "SELECT status FROM access_whitelist WHERE email = ?1 AND product = ?2",
                params![email, product],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// The gate every product (and part 03's VPN) asks: admin always yes;
    /// otherwise the product must be available AND the row approved.
    pub fn allowed(&self, email: &str, product: &str) -> Result<bool, AccessError> {
        let email = normalize_email(email)?;
        let role: Option<String> = self
            .db
            .query_row(
                "SELECT role FROM principals WHERE email = ?1",
                params![email],
                |r| r.get(0),
            )
            .optional()?;
        if role.as_deref() == Some(ROLE_ADMIN) {
            return Ok(true);
        }
        let ok: Option<i64> = self
            .db
            .query_row(
                "SELECT 1 FROM access_whitelist w JOIN products p ON p.id = w.product \
                 WHERE w.email = ?1 AND w.product = ?2 AND w.status = 'approved' AND p.available = 1",
                params![email, product],
                |r| r.get(0),
            )
            .optional()?;
        Ok(ok.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump(a: &Access) -> Vec<String> {
        let mut out = Vec::new();
        for (t, q) in [
            ("roles", "SELECT name || '|' || description FROM roles ORDER BY name"),
            ("products", "SELECT id || '|' || label || '|' || wave || '|' || available FROM products ORDER BY id"),
            ("principals", "SELECT email || '|' || role FROM principals ORDER BY email"),
            ("access_whitelist", "SELECT email || '|' || product || '|' || status || '|' || added_by FROM access_whitelist ORDER BY email, product"),
        ] {
            let mut st = a.conn().prepare(q).unwrap();
            let rows: Vec<String> = st
                .query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .map(|r| format!("{t}:{}", r.unwrap()))
                .collect();
            out.extend(rows);
        }
        out
    }

    #[test]
    fn email_normalisation() {
        assert_eq!(
            normalize_email("  Owner@Example.COM ").unwrap(),
            "owner@example.com"
        );
        for bad in [
            "", "no-at", "a@b", "@x.io", "a@@x.io", "a b@x.io", "a@x..io", "a@.x.io", "a@x.io.",
        ] {
            assert!(normalize_email(bad).is_err(), "{bad} should be rejected");
        }
        let long = format!("{}@x.io", "a".repeat(65));
        assert!(normalize_email(&long).is_err());
    }

    #[test]
    fn init_is_idempotent() {
        let mut a = Access::in_memory().unwrap();
        let r1 = a.init("Admin@Galion.example", 100).unwrap();
        assert!(r1.admin_created);
        assert_eq!(r1.admin_grants_changed, PRODUCTS.len());
        assert_eq!((r1.roles, r1.products), (3, 4));
        let before = dump(&a);
        let r2 = a.init("admin@galion.example", 200).unwrap();
        assert!(!r2.admin_created && !r2.admin_promoted);
        assert_eq!(r2.admin_grants_changed, 0);
        assert_eq!(before, dump(&a));
        let audits: i64 = a
            .conn()
            .query_row("SELECT COUNT(*) FROM access_audit", [], |r| r.get(0))
            .unwrap();
        assert_eq!(audits, 1, "a no-op re-run writes no audit row");
    }

    #[test]
    fn init_on_file_survives_reopen() {
        let dir = std::env::temp_dir().join(format!("ga-roles-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("access.sqlite");
        let _ = std::fs::rename(&p, dir.join("access.sqlite.old"));
        Access::open(&p).unwrap().init("o@x.io", 1).unwrap();
        let mut again = Access::open(&p).unwrap();
        let r = again.init("o@x.io", 2).unwrap();
        assert!(!r.admin_created);
        assert!(again.allowed("o@x.io", "galion-os").unwrap());
    }

    #[test]
    fn product_waves_and_seed_order() {
        let mut a = Access::in_memory().unwrap();
        a.init("o@x.io", 1).unwrap();
        let wave = |id: &str| -> (i64, i64) {
            a.conn()
                .query_row(
                    "SELECT wave, available FROM products WHERE id = ?1",
                    params![id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap()
        };
        assert_eq!(wave("hermes-research"), (1, 1));
        assert_eq!(wave("galion-one"), (1, 1));
        assert_eq!(wave("galion-os"), (3, 0));
        assert!(wave("galion-os").0 > wave("galion-one").0);
    }

    #[test]
    fn init_keeps_admin_product_changes() {
        let mut a = Access::in_memory().unwrap();
        a.init("o@x.io", 1).unwrap();
        a.conn()
            .execute(
                "UPDATE products SET available = 1 WHERE id = 'galion-os'",
                [],
            )
            .unwrap();
        a.init("o@x.io", 2).unwrap();
        let av: i64 = a
            .conn()
            .query_row(
                "SELECT available FROM products WHERE id = 'galion-os'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            av, 1,
            "re-running init must not close a product the admin opened"
        );
    }

    #[test]
    fn promotes_and_regrants() {
        let mut a = Access::in_memory().unwrap();
        a.init("first@x.io", 0).unwrap();
        a.request("o@x.io", "galion-one", 1).unwrap();
        a.decide("someone", "o@x.io", "hermes-research", false, 2)
            .unwrap();
        let r = a.init("o@x.io", 3).unwrap();
        assert!(r.admin_promoted && !r.admin_created);
        assert_eq!(r.other_admins, 1);
        assert_eq!(r.admin_grants_changed, PRODUCTS.len());
        assert_eq!(
            a.status("o@x.io", "hermes-research").unwrap().as_deref(),
            Some(STATUS_APPROVED)
        );
    }

    #[test]
    fn whitelist_gate() {
        let mut a = Access::in_memory().unwrap();
        a.init("admin@x.io", 1).unwrap();
        // Unknown: no.
        assert!(!a.allowed("u@x.io", "galion-one").unwrap());
        // Pending: no.
        assert_eq!(
            a.request("U@x.io", "galion-one", 2).unwrap(),
            STATUS_PENDING
        );
        assert!(!a.allowed("u@x.io", "galion-one").unwrap());
        // Approved on a wave-1 product: yes.
        a.decide("admin@x.io", "u@x.io", "galion-one", true, 3)
            .unwrap();
        assert!(a.allowed("u@x.io", "galion-one").unwrap());
        // A self-request never undoes the admin's decision.
        assert_eq!(
            a.request("u@x.io", "galion-one", 4).unwrap(),
            STATUS_APPROVED
        );
        // Approved on Galion OS, which is not open yet: still no. Admin: yes.
        a.decide("admin@x.io", "u@x.io", "galion-os", true, 5)
            .unwrap();
        assert!(!a.allowed("u@x.io", "galion-os").unwrap());
        assert!(a.allowed("admin@x.io", "galion-os").unwrap());
        // Revoke.
        a.decide("admin@x.io", "u@x.io", "galion-one", false, 6)
            .unwrap();
        assert!(!a.allowed("u@x.io", "galion-one").unwrap());
        // Unknown product is refused by the foreign key.
        assert!(a.request("u@x.io", "no-such-product", 7).is_err());
    }

    #[test]
    fn no_secret_columns() {
        let a = Access::in_memory().unwrap();
        let mut cols = Vec::new();
        for t in [
            "schema_meta",
            "roles",
            "products",
            "principals",
            "access_whitelist",
            "access_audit",
        ] {
            let mut st = a
                .conn()
                .prepare("SELECT name FROM pragma_table_info(?1)")
                .unwrap();
            let names: Vec<String> = st
                .query_map(params![t], |r| r.get::<_, String>(0))
                .unwrap()
                .map(|r| r.unwrap().to_ascii_lowercase())
                .collect();
            assert!(!names.is_empty(), "table {t} exists");
            cols.extend(names);
        }
        for word in ["password", "token", "secret", "pepper", "hash", "otp"] {
            assert!(cols.iter().all(|c| !c.contains(word)), "column like {word}");
        }
    }
}
