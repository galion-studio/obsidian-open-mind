// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `galion-accounts`: the anonymous account + payment service.
//!
//! ```text
//! galion-accounts serve        [--listen 127.0.0.1:19970] [--internal 127.0.0.1:19971]
//!                              [--db accounts.sqlite] [--pepper-file pepper] [--insecure-cookies]
//! galion-accounts stripe-setup [--apply] [--webhook-url https://galion.app/webhooks/stripe]
//! galion-accounts cash-credit  --ref GC-XXXX-XXXX-XXXX --amount-cents 2000 [--currency EUR] [--db ..]
//! galion-accounts crypto-poll  [--db ..]
//! galion-accounts sweep        [--db ..]
//! galion-accounts new-pepper   --out pepper
//! ```
//!
//! Payment config comes only from the environment (see README.md); with
//! nothing set every paid method reports "not connected".

use std::path::PathBuf;
use std::sync::Arc;

use galion_accounts::hash::{Cost, Hasher};
use galion_accounts::pay::cash::{Cash, CashConfig};
use galion_accounts::pay::crypto::{self, Backend, Crypto, CryptoConfig};
use galion_accounts::pay::stripe::{Stripe, StripeConfig};
use galion_accounts::pay::transport::CurlTransport;
use galion_accounts::pay::PaymentAdapter;
use galion_accounts::ratelimit::Limiter;
use galion_accounts::server::{self, App, Settings};
use galion_accounts::store::Store;

const USAGE: &str = "usage: galion-accounts <serve|stripe-setup|cash-credit|crypto-poll|sweep|new-pepper> [options]\n\
  serve        [--listen ADDR] [--internal ADDR] [--db PATH] [--pepper-file PATH] [--insecure-cookies]\n\
  stripe-setup [--apply] [--webhook-url URL]   (dry run unless --apply; uses STRIPE_SECRET_KEY)\n\
  cash-credit  --ref REF --amount-cents N [--currency EUR] [--db PATH]\n\
  crypto-poll  [--db PATH]   (one monero-wallet-rpc poll; needs the MONERO_* config)\n\
  sweep        [--db PATH]\n\
  new-pepper   --out PATH";

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

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
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

fn stripe() -> Stripe {
    Stripe::new(StripeConfig::from_env(), Arc::new(CurlTransport::default()))
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
    if app.crypto.cfg.ready().contains(&Backend::Monero) {
        let poller = app.clone();
        let every = poller.crypto.cfg.monero.poll_seconds;
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(every));
            match crypto::poll_monero(&poller.crypto, &poller.store, galion_accounts::now_unix()) {
                Ok(r) if r.credited > 0 => {
                    eprintln!("monero poll: credited {} invoice(s)", r.credited)
                }
                Ok(_) => {}
                Err(e) => eprintln!("monero poll: {e:?}"),
            }
        });
    }
    eprintln!("galion-accounts listening on {listen} (public) and {internal} (internal)");
    if let Err(e) = server::serve(app, public, inner) {
        die(&format!("serve: {e}"));
    }
}

fn cmd_stripe_setup(a: &Args) {
    let s = stripe();
    let apply = a.flag("--apply");
    match s.setup(apply, a.opt("--webhook-url").as_deref()) {
        Ok(lines) => {
            for l in lines {
                println!("{l}");
            }
            if !apply {
                println!("(dry run: nothing was created; run again with --apply)");
            }
        }
        Err(e) => die(&format!("stripe-setup: {e:?}")),
    }
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
    match cmd.as_str() {
        "serve" => cmd_serve(&a),
        "stripe-setup" => cmd_stripe_setup(&a),
        "cash-credit" => cmd_cash_credit(&a),
        "sweep" => {
            let n = open_store(&a)
                .sweep(galion_accounts::now_unix())
                .unwrap_or_else(|e| die(&format!("database: {e}")));
            println!("swept {n} expired rows");
        }
        "crypto-poll" => {
            let c = crypto();
            match crypto::poll_monero(&c, &open_store(&a), galion_accounts::now_unix()) {
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
