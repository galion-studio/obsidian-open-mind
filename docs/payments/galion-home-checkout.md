<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# galion.app checkout: design (Mate t548u/t549u/t550u, 2026-11-01 CET, UTC+01:00)

Status: **phase 1 built (buttons show "Coming soon" until galion-accounts answers), phase 2 sketched only.** No Stripe account,
product, price, key or webhook was created. Nothing on the page takes a payment today.

## Accounts (t549u)
Accounts are number-only, in the Mullvad style: a generated account number, no email, no
personal details. The number is the account. Payment methods: card (Stripe, main), crypto, cash
by post. Coins and the mailing address are to be decided. The account system itself is built by
another worker; galion.app only links to `/account`, which is a static placeholder for now.

## Phase 1: buttons wired to galion-accounts (built)
One universal subscription for all of galion.app (t550u): `plus` = EUR 20 / month, `pro` = EUR 220 / month.
The page shows "Coming soon" by default. With `ONE_CHECKOUT=probe`, `home.js` sends `HEAD /api/checkout?plan=plus|pro` (no
session is created by a HEAD); only a 2xx or 3xx answer turns the button into a `Subscribe` link to
`/api/checkout?plan=..`. A "not connected" answer (any 4xx/5xx), a 404 or no network keeps "Coming soon".
`/api/checkout` itself is served by galion-accounts (swarm/galion-accounts-20261101, another worker),
which owns the provider choice (`provider = stripe` default, pluggable) and can start with Stripe
Payment Links (a 303 to `https://buy.stripe.com/...`, zero JS on our side).

| galion-one env | Meaning |
|---|---|
| `ONE_CHECKOUT=probe` | page asks `HEAD /api/checkout?plan=..` and shows Subscribe only on 2xx/3xx (off by default: today that path is a 404 and browsers would log it) |
| `ONE_CHECKOUT=on` | render the Subscribe links server-side too (visitors without JS), once accounts is live |
| `ONE_WAITLIST_URL` | optional `https:` or `mailto:` link; waiting buttons read "Coming soon: join the waitlist" |

Contract proposed to galion-accounts: `HEAD /api/checkout?plan=P` -> 204 when checkout is connected,
503 with body `not connected` otherwise, never creating a session; `GET` -> 303 to the provider page.
Routing: cloudflared (or galion-one) must send `galion.app/api/checkout` to galion-accounts; today
galion-one answers that path with 404 (checked locally and on live galion.app, 2026-11-01), so the buttons stay "Coming soon".

## Phase 2: `/api/checkout` (sketch, not built)
```text
POST /api/checkout {plan: "plus"|"pro", account: "<number>"}
  -> provider = config.provider (default "stripe")
  -> stripe: POST https://api.stripe.com/v1/checkout/sessions
       mode=subscription, line_items[0][price]=<PRICE_ID>, line_items[0][quantity]=1,
       client_reference_id=<account number>, success_url, cancel_url
       Authorization: Bearer <restricted key from an env file, 0600, never in the page>
  <- 303 Location: session.url
POST /api/stripe/webhook  (verify Stripe-Signature with the webhook secret)
  checkout.session.completed / customer.subscription.updated|deleted -> account plan state
```
- HTTP client: `reqwest` with `rustls-tls` and default features off (RULES: no openssl). It
  would be a new dependency for galion-one, which is std-only today, so it may fit better in a
  small separate `galion-pay` service; owner decision.
- Trait sketch: `trait Checkout { fn create(&self, plan: Plan, account: &str) -> Result<Url>; fn verify_webhook(&self, body: &[u8], sig: &str) -> Result<Event>; }`, with `Stripe` first; crypto and cash are separate flows (manual cash credit, crypto processor TBD).

## Needed from Slavik's Stripe account (nothing created yet)
1. One product (the universal subscription) with two **recurring monthly Prices** in EUR: EUR 20.00 (`plus`) and EUR 220.00 (`pro`) (price IDs).
2. Phase 1: a **Payment Link** for each Price (two `https://buy.stripe.com/...` URLs).
   Phase 2 instead: a **restricted API key** with write access to Checkout Sessions only (plus
   read on Prices), stored in an env file on the server, never in git or the page.
3. Later: a **webhook endpoint** (for example `https://galion.app/api/stripe/webhook`) for
   `checkout.session.completed`, `customer.subscription.updated` and
   `customer.subscription.deleted`, and its signing secret.
4. Success and cancel URLs (suggested: `https://galion.app/account?paid=1` and `https://galion.app/#pricing`).
