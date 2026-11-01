// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `galion-accounts`: the anonymous account + payment service.
//!
//! ```text
//! galion-accounts serve        [--listen 127.0.0.1:19970] [--internal 127.0.0.1:19971]
//!                              [--db accounts.sqlite] [--pepper-file pepper] [--insecure-cookies]
//! galion-accounts init         --admin-email EMAIL [--access-db access.sqlite] [--db ..]
//! galion-accounts stripe-setup [--apply] [--webhook-url https://galion.app/webhooks/stripe]
//!                              [--secret-out whsec.txt] [--env-file accounts.env]
//! galion-accounts cash-credit  --ref GC-XXXX-XXXX-XXXX --amount-cents 2000 [--currency EUR] [--db ..]
//! galion-accounts crypto-poll  [--db ..]
//! galion-accounts readiness    [--env-file accounts.env] [--db ..] [--pepper-file ..] [--require stripe,cash,btcpay,monero]
//! galion-accounts sweep        [--db ..]
//! galion-accounts new-pepper   --out pepper
//! ```
//!
//! Payment config comes only from the environment (see README.md); with
//! nothing set every paid method reports "not connected".

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use galion_accounts::hash::{Cost, Hasher};
use galion_accounts::pay::cash::{Cash, CashConfig};
use galion_accounts::pay::crypto::{self, Backend, Crypto, CryptoConfig};
use galion_accounts::pay::stripe::{Stripe, StripeConfig};
use galion_accounts::pay::transport::CurlTransport;
use galion_accounts::pay::PaymentAdapter;
use galion_accounts::ratelimit::Limiter;
use galion_accounts::server::{self, App, Settings};
use galion_accounts::store::Store;

const USAGE: &str = "usage: galion-accounts <serve|init|stripe-setup|cash-credit|crypto-poll|readiness|sweep|new-pepper> [options]\n\
  serve        [--listen ADDR] [--internal ADDR] [--db PATH] [--pepper-file PATH] [--insecure-cookies]\n\
  init         --admin-email EMAIL [--access-db PATH] [--db PATH]\n\
               idempotent: roles, products (whitelist waves) and the admin record in access.sqlite; no secrets\n\
  stripe-setup [--apply] [--webhook-url URL] [--secret-out PATH]\n\
               dry run unless --apply (with a key the dry run only reads); uses STRIPE_SECRET_KEY.\n\
               A new webhook signing secret is written to --secret-out (0600, must not exist), never printed.\n\
  cash-credit  --ref REF --amount-cents N [--currency EUR] [--db PATH]\n\
  crypto-poll  [--db PATH]   (one Monero poll + BTCPay reconcile, for the backends that are set up)\n\
  readiness    [--db PATH] [--pepper-file PATH] [--require stripe,cash,btcpay,monero]\n\
               what is configured, as yes/no only; exit 1 if something required is missing\n\
  sweep        [--db PATH]\n\
  new-pepper   --out PATH\n\
  Every command accepts --env-file PATH (systemd EnvironmentFile syntax) instead of the process environment.";

struct Args(Vec<String>);

impl Args {
    fn flag(&self, name: &str) -> bool {
        self.0.iter().any(|a| a == name)
    }
    fn opt(&self, name: &str) -> Option<String> {
        let pre = format!("{name}=");
        let mut it = self.0.iter();
        while let Some(a) = it.next() {
            if a == name {
                return it.next().cloned();
            }
            if let Some(v) = a.strip_prefix(&pre) {
                return Some(v.to_string());
            }
        }
        None
    }
}

/// Values from `--env-file` (if given) override the process environment.
static ENV_FILE: OnceLock<HashMap<String, String>> = OnceLock::new();

fn env(name: &str) -> Option<String> {
    ENV_FILE
        .get()
        .and_then(|m| m.get(name).cloned())
        .or_else(|| std::env::var(name).ok())
        .filter(|v| !v.trim().is_empty())
}

/// Parse a systemd `EnvironmentFile`: `KEY=VALUE` lines, `#`/`;` comments,
/// optional matching single or double quotes. No shell expansion, so a
/// `|` in ACCOUNTS_CASH_ADDRESS is safe.
fn parse_env_file(text: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with(';') {
            continue;
        }
        let l = l.strip_prefix("export ").unwrap_or(l);
        let Some((k, v)) = l.split_once('=') else {
            continue;
        };
        let v = v.trim();
        let v = if v.len() >= 2
            && ((v.starts_with('"') && v.ends_with('"'))
                || (v.starts_with('\'') && v.ends_with('\'')))
        {
            &v[1..v.len() - 1]
        } else {
            v
        };
        m.insert(k.trim().to_string(), v.to_string());
    }
    m
}

fn die(msg: &str) -> ! {
    eprintln!("galion-accounts: {msg}");
    std::process::exit(2)
}

fn db_path(a: &Args) -> PathBuf {
    a.opt("--db")
        .or_else(|| env("ACCOUNTS_DB"))
        .unwrap_or_else(|| "accounts.sqlite".into())
        .into()
}

fn open_store(a: &Args) -> Store {
    let p = db_path(a);
    Store::open(&p).unwrap_or_else(|e| die(&format!("cannot open {}: {e}", p.display())))
}

fn read_pepper(a: &Args) -> Vec<u8> {
    let p = a
        .opt("--pepper-file")
        .or_else(|| env("ACCOUNTS_PEPPER_FILE"))
        .unwrap_or_else(|| {
            die("--pepper-file (or ACCOUNTS_PEPPER_FILE) is required; make one with `new-pepper`")
        });
    let raw = std::fs::read(&p).unwrap_or_else(|e| die(&format!("cannot read pepper file: {e}")));
    let s = String::from_utf8_lossy(&raw).trim().as_bytes().to_vec();
    if s.len() < 32 {
        die("pepper file must hold at least 32 bytes");
    }
    s
}

/// `--access-db`, else ACCOUNTS_ACCESS_DB, else `access.sqlite` next to the
/// accounts database (a separate file: the accounts DB stays email-free).
fn access_db_path(a: &Args) -> PathBuf {
    if let Some(p) = a.opt("--access-db").or_else(|| env("ACCOUNTS_ACCESS_DB")) {
        return p.into();
    }
    let db = db_path(a);
    match db.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join("access.sqlite"),
        _ => PathBuf::from("access.sqlite"),
    }
}

/// `init --admin-email EMAIL`: roles schema, products (whitelist waves) and
/// the admin record. Idempotent. Stores no secret.
fn cmd_init(a: &Args) {
    let email = a
        .opt("--admin-email")
        .unwrap_or_else(|| die("init needs --admin-email EMAIL"));
    let p = access_db_path(a);
    let mut access = galion_accounts::roles::Access::open(&p)
        .unwrap_or_else(|e| die(&format!("cannot open {}: {e}", p.display())));
    let r = access
        .init(&email, galion_accounts::now_unix())
        .unwrap_or_else(|e| die(&format!("init: {e}")));
    println!("access db: {}", p.display());
    println!(
        "admin {}: {}",
        r.admin_email,
        if r.admin_created {
            "created"
        } else if r.admin_promoted {
            "promoted to admin"
        } else {
            "already admin (no change)"
        }
    );
    println!(
        "roles {} | products {} | admin grants changed {} | other admins {}",
        r.roles, r.products, r.admin_grants_changed, r.other_admins
    );
    for prod in galion_accounts::roles::PRODUCTS {
        println!(
            "  product {:<16} wave {} {}",
            prod.id,
            prod.wave,
            if prod.available {
                "open (seed)"
            } else {
                "later (seed)"
            }
        );
    }
}

fn stripe() -> Stripe {
    Stripe::new(
        StripeConfig::from_vars(env),
        Arc::new(CurlTransport::default()),
    )
}

fn crypto() -> Crypto {
    Crypto::new(
        CryptoConfig::from_vars(env),
        Arc::new(CurlTransport::default()),
    )
}

fn cmd_serve(a: &Args) {
    let listen = a
        .opt("--listen")
        .unwrap_or_else(|| "127.0.0.1:19970".into());
    let internal = a
        .opt("--internal")
        .unwrap_or_else(|| "127.0.0.1:19971".into());
    let cost = if env("ACCOUNTS_TEST_COST").is_some() {
        Cost::TEST
    } else {
        Cost::PRODUCTION
    };
    let hasher = Hasher::new(read_pepper(a), cost).unwrap_or_else(|e| die(&e));
    let settings = Settings {
        cookie_domain: env("ACCOUNTS_COOKIE_DOMAIN"),
        secure_cookies: !a.flag("--insecure-cookies"),
        trust_proxy: env("ACCOUNTS_TRUST_PROXY").as_deref() == Some("1"),
        internal_token: env("ACCOUNTS_INTERNAL_TOKEN"),
        admin_token: env("ACCOUNTS_ADMIN_TOKEN"),
    };
    let app = Arc::new(App {
        store: open_store(a),
        hasher,
        limiter: Limiter::new(),
        stripe: stripe(),
        cash: Cash {
            cfg: CashConfig::from_vars(env),
        },
        crypto: crypto(),
        settings,
        clock: Box::new(galion_accounts::now_unix),
    });
    for (name, ad) in [
        ("stripe", &app.stripe as &dyn PaymentAdapter),
        ("crypto", &app.crypto),
        ("cash", &app.cash),
    ] {
        match ad.status() {
            galion_accounts::pay::Status::Connected => eprintln!("payment {name}: connected"),
            galion_accounts::pay::Status::NotConnected(r) => {
                eprintln!("payment {name}: not connected ({r})")
            }
        }
    }
    let public = std::net::TcpListener::bind(&listen)
        .unwrap_or_else(|e| die(&format!("bind {listen}: {e}")));
    let inner = std::net::TcpListener::bind(&internal)
        .unwrap_or_else(|e| die(&format!("bind {internal}: {e}")));
    let sweeper = app.clone();
    std::thread::spawn(move || loop {
        let _ = sweeper.store.sweep(galion_accounts::now_unix());
        std::thread::sleep(std::time::Duration::from_secs(3600));
    });
    let ready = app.crypto.cfg.ready();
    if !ready.is_empty() {
        // Monero has no webhooks; BTCPay is reconciled too in case a
        // webhook was missed or a partial payment confirmed late.
        let poller = app.clone();
        let every = if ready.contains(&Backend::Monero) {
            poller.crypto.cfg.monero.poll_seconds
        } else {
            300
        };
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(every.max(10)));
            match crypto::poll_all(&poller.crypto, &poller.store, galion_accounts::now_unix()) {
                Ok(r) if r.credited > 0 => {
                    eprintln!("crypto poll: credited {} invoice(s)", r.credited)
                }
                Ok(_) => {}
                Err(e) => eprintln!("crypto poll: {e:?}"),
            }
        });
    }
    eprintln!("galion-accounts listening on {listen} (public) and {internal} (internal)");
    if let Err(e) = server::serve(app, public, inner) {
        die(&format!("serve: {e}"));
    }
}

/// Create a 0600 file that must not exist yet.
fn create_private(path: &str) -> std::io::Result<std::fs::File> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    o.mode(0o600);
    o.open(path)
}

fn cmd_stripe_setup(a: &Args) {
    let s = stripe();
    let apply = a.flag("--apply");
    let webhook = a.opt("--webhook-url");
    let secret_out = a.opt("--secret-out");
    // Fail before any API call if a new secret would have nowhere to go.
    let secret_file = match (&secret_out, apply && webhook.is_some()) {
        (Some(p), true) => Some(
            create_private(p)
                .unwrap_or_else(|e| die(&format!("--secret-out {p}: {e} (it must not exist yet)"))),
        ),
        (None, true) => die("--apply with --webhook-url needs --secret-out PATH: the new signing secret is written there (0600), never printed"),
        _ => None,
    };
    match s.setup(apply, webhook.as_deref()) {
        Ok(rep) => {
            for l in &rep.lines {
                println!("{l}");
            }
            if let (Some(sec), Some(mut f), Some(p)) =
                (&rep.webhook_secret, secret_file, &secret_out)
            {
                use std::io::Write;
                f.write_all(format!("STRIPE_WEBHOOK_SECRET={sec}\n").as_bytes())
                    .unwrap_or_else(|e| die(&format!("write {p}: {e}")));
                println!("webhook signing secret written to {p} (0600). Copy the line into accounts.env, then remove the file.");
            } else if apply && webhook.is_some() {
                println!("no new webhook secret (endpoint existed): keep the STRIPE_WEBHOOK_SECRET you have; {} left empty", secret_out.as_deref().unwrap_or("-"));
            }
            if !apply {
                println!("(dry run: nothing was created or changed; run again with --apply)");
            }
        }
        Err(e) => die(&format!("stripe-setup: {e}")),
    }
}

/// `readiness`: yes/no per setting, never a value. Exit 1 when a base
/// requirement (pepper, database, curl, internal token) or a `--require`d
/// method is missing.
fn cmd_readiness(a: &Args) {
    let mut bad = 0;
    let line = |ok: bool, what: &str, note: &str| {
        println!(
            "{} {what}{}",
            if ok { "ok  " } else { "MISS" },
            if note.is_empty() {
                String::new()
            } else {
                format!(" ({note})")
            }
        );
        ok
    };
    // Pepper: readable and long enough. Never printed.
    let pepper = a
        .opt("--pepper-file")
        .or_else(|| env("ACCOUNTS_PEPPER_FILE"));
    let pep_ok = match &pepper {
        Some(p) => std::fs::read(p)
            .map(|b| String::from_utf8_lossy(&b).trim().len() >= 32)
            .unwrap_or(false),
        None => false,
    };
    if !line(
        pep_ok,
        "pepper file readable, >= 32 bytes",
        "contents not shown",
    ) {
        bad += 1;
    }
    let db = db_path(a);
    // Read-only and immutable: no migration, no -wal/-shm files created, so
    // running this as root next to the live service changes nothing on disk.
    let db_ok = db.exists() && db_readable(&db);
    if !line(db_ok, "database opens (read-only)", "") {
        bad += 1;
    }
    let curl_ok = std::process::Command::new("curl")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !line(curl_ok, "curl present (Stripe/BTCPay/Monero transport)", "") {
        bad += 1;
    }
    let tok = |k: &str| env(k).is_some_and(|t| t.len() >= 32);
    if !line(
        tok("ACCOUNTS_INTERNAL_TOKEN"),
        "ACCOUNTS_INTERNAL_TOKEN set, >= 32 chars",
        "",
    ) {
        bad += 1;
    }
    line(
        tok("ACCOUNTS_ADMIN_TOKEN"),
        "ACCOUNTS_ADMIN_TOKEN set, >= 32 chars",
        "needed for cash credit and admin status",
    );
    line(
        env("ACCOUNTS_PUBLIC_URL").is_some_and(|u| u.starts_with("https://")),
        "ACCOUNTS_PUBLIC_URL is https://",
        "",
    );
    let sc = StripeConfig::from_vars(env);
    let key_shape = sc.mode().is_some();
    let wh_shape = sc
        .webhook_secret
        .as_deref()
        .is_some_and(|w| w.starts_with("whsec_"));
    line(
        key_shape,
        "STRIPE_SECRET_KEY looks like sk_/rk_ test or live",
        sc.mode().unwrap_or("-"),
    );
    line(wh_shape, "STRIPE_WEBHOOK_SECRET looks like whsec_", "");
    line(
        sc.portal_config.is_some(),
        "STRIPE_PORTAL_CONFIG set",
        "optional: default portal config otherwise",
    );
    let stripe_ok = Stripe::new(sc, Arc::new(CurlTransport::default()))
        .status()
        .is_connected()
        && key_shape
        && wh_shape;
    let cc = CryptoConfig::from_vars(env);
    let btc_ok = cc.ready().contains(&Backend::BtcPay);
    let xmr_ok = cc.ready().contains(&Backend::Monero);
    let cash_ok = Cash {
        cfg: CashConfig::from_vars(env),
    }
    .status()
    .is_connected();
    let why = |b: Backend| cc.missing(b).unwrap_or("");
    line(stripe_ok, "method stripe", "");
    line(
        btc_ok,
        "method btcpay",
        if btc_ok { "" } else { why(Backend::BtcPay) },
    );
    line(
        xmr_ok,
        "method monero",
        if xmr_ok { "" } else { why(Backend::Monero) },
    );
    line(
        cash_ok,
        "method cash",
        if cash_ok {
            ""
        } else {
            "ACCOUNTS_CASH_ADDRESS empty"
        },
    );
    for r in a
        .opt("--require")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|r| !r.is_empty())
    {
        let ok = match r {
            "stripe" => stripe_ok,
            "btcpay" => btc_ok,
            "monero" => xmr_ok,
            "cash" => cash_ok,
            other => die(&format!("--require: unknown method {other}")),
        };
        if !ok {
            bad += 1;
        }
    }
    if bad > 0 {
        println!("NOT READY: {bad} required item(s) missing");
        std::process::exit(1);
    }
    println!("READY");
}

fn cmd_cash_credit(a: &Args) {
    let r = a.opt("--ref").unwrap_or_else(|| die("--ref is required"));
    let cents: i64 = a
        .opt("--amount-cents")
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| die("--amount-cents N is required"));
    let cur = a.opt("--currency").unwrap_or_else(|| "EUR".into());
    let store = open_store(a);
    match server::credit_cash(&store, &r, cents, &cur, galion_accounts::now_unix()) {
        Ok(Some(v)) => println!("{v}"),
        Ok(None) => die("unknown, used or expired reference"),
        Err(e) => die(&format!("database: {e}")),
    }
}

fn cmd_new_pepper(a: &Args) {
    let out = a
        .opt("--out")
        .unwrap_or_else(|| die("--out PATH is required"));
    let hex = galion_common::hex::encode(&galion_accounts::rng::bytes(32));
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    o.mode(0o600);
    let mut f = o
        .open(&out)
        .unwrap_or_else(|e| die(&format!("cannot create {out} (it must not exist yet): {e}")));
    f.write_all(hex.as_bytes())
        .and_then(|_| f.write_all(b"\n"))
        .unwrap_or_else(|e| die(&format!("write: {e}")));
    println!("wrote a new 256-bit pepper to {out} (mode 0600). Back it up: without it no account can log in.");
}

fn main() {
    galion_common::build_info::exit_if_version("galion-accounts", env!("CARGO_PKG_VERSION"));
    let mut argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.is_empty() {
        die(USAGE);
    }
    let cmd = argv.remove(0);
    let a = Args(argv);
    if let Some(p) = a.opt("--env-file") {
        let text = std::fs::read_to_string(&p)
            .unwrap_or_else(|e| die(&format!("cannot read --env-file: {e}")));
        let _ = ENV_FILE.set(parse_env_file(&text));
    }
    match cmd.as_str() {
        "serve" => cmd_serve(&a),
        "init" => cmd_init(&a),
        "stripe-setup" => cmd_stripe_setup(&a),
        "cash-credit" => cmd_cash_credit(&a),
        "readiness" => cmd_readiness(&a),
        "sweep" => {
            let n = open_store(&a)
                .sweep(galion_accounts::now_unix())
                .unwrap_or_else(|e| die(&format!("database: {e}")));
            println!("swept {n} expired rows");
        }
        "crypto-poll" => {
            let c = crypto();
            if c.cfg.ready().is_empty() {
                die("crypto-poll: no crypto backend is set up (ACCOUNTS_CRYPTO_BACKEND + BTCPAY_*/MONERO_*)");
            }
            match crypto::poll_all(&c, &open_store(&a), galion_accounts::now_unix()) {
                Ok(r) => println!(
                    "open {} credited {} ({} EUR cents)",
                    r.open, r.credited, r.credited_cents
                ),
                Err(e) => die(&format!("crypto-poll: {e:?}")),
            }
        }
        "new-pepper" => cmd_new_pepper(&a),
        "-h" | "--help" | "help" => println!("{USAGE}"),
        _ => die(USAGE),
    }
}

/// Open the database read-only (`immutable=1`: no locks, no side files) and
/// check the accounts table is there.
fn db_readable(p: &std::path::Path) -> bool {
    use rusqlite::OpenFlags;
    let Some(s) = p.to_str() else { return false };
    let uri = format!(
        "file:{}?mode=ro&immutable=1",
        s.replace('%', "%25")
            .replace('?', "%3f")
            .replace('#', "%23")
    );
    rusqlite::Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .and_then(|c| c.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get::<_, i64>(0)))
    .is_ok()
}
