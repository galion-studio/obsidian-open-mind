<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# galion-one: galion.app account routes forwarded to galion-accounts

Run 2026-11-01 15:16 CET (+01:00) on the box. Nothing deployed, nothing pushed, the VPS not touched.

## What changed
- `crates/galion-one/src/accounts_proxy.rs` (new): on the home hosts only (`ONE_HOME_HOSTS`,
  default `galion.app,www.galion.app`), `/account`, `/account/*` (page, `account.css`,
  `account.js`), `/api/account/*`, `/api/pay/*`, `/api/checkout` and `/webhooks/*` are forwarded to
  galion-accounts at `ONE_ACCOUNTS_UPSTREAM` (default `127.0.0.1:19970`, loopback only,
  `off` or empty switches it off).
- Accounts off or down: `GET /account` shows the existing placeholder (`home/account.html`),
  the other paths answer 503 (`/api/checkout`: `not connected`, API: JSON `unavailable`,
  webhooks: 503 so Stripe retries).
- Forwarded headers: everything except hop-by-hop, `Authorization`, client-sent
  `X-Forwarded-*`/`X-Real-IP`/`Forwarded`, and cookies other than `galion_acct` (the Galion One
  pairing cookie never leaves galion-one). One `X-Forwarded-For` is added from Cloudflare's
  `CF-Connecting-IP` (when it is a valid IP) or the TCP peer. Chunked request bodies get 411.
- one.galion.app and loopback hosts are not affected.

## Tests
- Unit (`cargo test -p galion-one --bin galion-one accounts_proxy`): 8 passed (route table,
  loopback-only upstream, header rewrite, peer fallback, chunked refusal, fallbacks, byte-for-byte
  relay through a fake upstream, down upstream).
- Whole crate (`cargo test -p galion-one`): 122 + 6 passed, 1 ignored (pre-existing).
- `cargo fmt --all --check`, `cargo clippy -p galion-one --all-targets -- -D warnings`: clean.
  `scripts/no-python.sh`: ok. `repo-check`: no finding in the new files (2 pre-existing errors elsewhere, same as the base).
- Real (`GALION_REAL=1 GALION_ACCOUNTS_BIN=<galion-accounts f72188f8 release> cargo test -p galion-one --test real_one_accounts_proxy -- --nocapture`),
  real galion-one + real galion-accounts on 127.0.0.1:19975-19979, no Stripe keys:

```text
real ports: ONE_ACCOUNTS_REAL_PORT_BASE -> 127.0.0.1:19975-19979
accounts down: placeholder + 503s ok
accounts up: page, css, js, checkout 503/303, webhook, create + me ok
accounts stopped: placeholder + 503s ok
test real_accounts_proxy ... ok
```

  Checked on the way: `HEAD /api/checkout?plan=plus` 503 (card not connected), `GET ?plan=pro`
  303 to `/account?plan=pro`, `POST /webhooks/stripe` 503 `not_connected` from accounts itself,
  a foreign `Origin` refused by the accounts CSRF guard (real Host preserved), create a number,
  then `/api/account/me` with the session cookie gives `plan: free`; without it 401;
  one.galion.app `HEAD /api/checkout` is not proxied; chunked POST 411. No listener left behind.
