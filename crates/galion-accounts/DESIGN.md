<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Galion accounts: a number, not a person

Written 2026-11-01 (Europe/Zagreb, CET +01:00). Branch `swarm/galion-accounts-20261101`,
crate `crates/galion-accounts`. Prototype on the box only: nothing is deployed, there is no
real Stripe account, and nothing has been pushed.

## Part 1: in plain words

**What you get.** You press "Generate account number" and get 16 digits, for example
`4364 5676 0536 3521`. That number is your user name and your password. We don't ask for an
email, a name or a phone number. Write the number down. We can't show it to you again, and
if you lose it we can't recover it, because we never stored it.

**One subscription for all of Galion.** The same number works on galion.app, in the console,
in Galion One and across the OpenMind stack:

| Plan | Price | What it is |
|---|---|---|
| Free | €0 | Galion is open source (MIT). Run it on your own computer, no account needed. |
| Plus | €20 / month | One person. The whole platform, hosted for you, Galion One included. |
| Pro | €220 / month | Everything in Plus, and your Hermes agent gets its own cloud computer and SSD. |

**Three ways to pay:**

1. **Card (Stripe), monthly subscription.** Stripe's own checkout page asks for your card
   details. We never see them. We send Stripe a random payment reference, never your account
   number and never an email. Stripe itself can still ask for an email or a country (for
   receipts, fraud checks and VAT), and as a payment company it keeps its own records. While
   you are subscribed, we keep Stripe's customer and subscription ids next to your account so
   renewals land on it. When you cancel, we delete those ids.
2. **Cash in an envelope.** You get a reference such as `GC-PA20-FS10-VCT8`. Write only that
   on a slip of paper, put it in an envelope with euro notes and post it. We open the
   envelope, add the time and shred the envelope. €20 gives 30 days of Plus.
3. **Crypto.** Designed, not switched on yet. The choice is between BTCPay Server (Bitcoin)
   and monero-wallet-rpc (Monero).

**What we keep:** a scrambled form of your number (a one-way hash that can't be turned back
into the number), your plan, the date your paid time ends, sessions that last up to 30 days,
and payment references that expire on their own. Separately, we keep a money ledger with the
day, method, amount and currency and **no** link to any account, because the accountant needs
it. We keep no IP addresses, no emails and no names.

**If the police come:** the database would show that "some account has Plus until 3 December".
It can't show who owns that account, what its number is, or which payment paid for which
account, except for card subscriptions that are active right now (see 2.7).

## Part 2: technical design

### 2.1 What Mullvad does (public sources)

| Mullvad fact | Source |
|---|---|
| Accounts are random numbers. No user name, password or email. The account has only two properties: its number and the time remaining. | [No-logging policy](https://mullvad.net/en/help/no-logging-data-policy) |
| Cash goes in an envelope with a payment token. They add time, then shred the envelope and everything except the money. GDPR doesn't apply to an envelope that is never filed. | same, and [Clarifying our no-logging policy](https://mullvad.net/en/blog/clarifying-our-no-logging-policy) |
| Payment tokens: an unused token is kept 120 days, a used one 20 days. After that the account can't be recovered from the payment. | [Your privacy is your privacy, updated policy](https://mullvad.net/en/blog/your-privacy-your-privacy-updated-policy), [Recover lost account: cash](https://mullvad.net/en/account/recover/cash) |
| Stripe: they keep the charge id plus the expiry date, the last 4 card digits, the card type and the country. The transaction id is deleted after 20 days. Accounting data is kept for the statutory period (Sweden: 7 years). | [Privacy policy](https://mullvad.net/en/help/privacy-policy) |
| Crypto runs on their own full nodes and self-hosted wallets. The BTC receive address is dropped after 20 days. The Monero tx hash is kept so nothing is credited twice. | [No-logging policy](https://mullvad.net/en/help/no-logging-data-policy) |
| Recurring subscriptions were removed on 2022-06-20, because a subscription forces them to keep the link between payment and account for as long as it runs. | [Removing new subscriptions](https://mullvad.net/en/blog/were-removing-the-option-to-create-new-subscriptions) |
| On 2023-04-18 Swedish police arrived with a search warrant and left with nothing, because no customer data existed. | [Search warrant post](https://mullvad.net/en/blog/2023/4/20/mullvad-vpn-was-subject-to-a-search-warrant-customer-data-not-compromised), [follow-up](https://mullvad.net/en/blog/2023/5/2/update-the-swedish-authorities-answered-our-protocol-request) |
| Postal address for cash: Mullvad VPN AB, Box 53049, 400 14 Gothenburg. Accepted currencies: EUR, USD, GBP and others. | [Pricing](https://mullvad.net/en/pricing), [New address](https://mullvad.net/en/blog/sending-cash-use-our-new-address) |

**Where Galion differs, on purpose:** Slavik asked for a monthly subscription. That is the
exact tradeoff Mullvad chose to drop. We keep it, contain it (2.7), and still offer cash
(and, later, crypto) for people who want no link at all.

### 2.2 Account numbers

- 16 decimal digits, drawn uniformly from [10^15, 10^16), so 9×10^15 possibilities, using
  `getrandom` with rejection sampling (`number.rs`, `rng.rs`). Shown in groups of 4. Input
  accepts spaces and dashes.
- Brute force is not practical. Even with 10^6 live accounts, a random guess hits one with a
  chance of about 1 in 9×10^9. On top of that there is rate limiting (2.4).

### 2.3 Storing the number (`hash.rs`)

- `lookup = argon2id(number, salt = const "galion-accounts/lookup/v1", secret = pepper)`.
  Settings: m = 19 MiB, t = 2, p = 1 (the OWASP minimum), 32-byte output.
- The hash has to be **deterministic** because there is no user name to find a per-user
  salt with. The number itself is the lookup key, so the hash sits under a UNIQUE index.
  The per-user salt is replaced by a **pepper**, a 256-bit secret in a file outside the
  database (`new-pepper` writes it with mode 0600). A stolen database without the pepper
  can't be brute-forced at all. With the pepper, each guess still costs one argon2id run.
- Tradeoff: if the pepper is lost, nobody can log in again. Back it up offline.
- Measured on the box (release build, real binary): create 32 ms, login 32 ms.
- Flag: through `cpufeatures`, argon2 0.5 picks an AVX2 code path at runtime on x86. That
  is Rust intrinsics, not assembly, but it is a SIMD path, and the swarm rules say "no SIMD
  deps". Slavik should decide whether to accept it. The alternative would be a portable
  fork or a different KDF.

### 2.4 Sessions and abuse limits

- Logging in gives a 256-bit random token in cookie `galion_acct`, set with `HttpOnly;
  Secure; SameSite=Lax; Path=/` and a 30-day Max-Age. Only sha256(token) is stored.
  Logout deletes the row. After 30 days you log in again with your number.
- `ACCOUNTS_COOKIE_DOMAIN=.galion.app` shares the session across galion.app subdomains.
- CSRF: every POST must carry `Content-Type: application/json` (otherwise 415), and when the
  browser sends `Origin` it must be our own.
- Rate limits are kept in memory, never on disk:
  - 10 failed logins per 15 minutes from one client lead to a 15-minute lockout;
  - 600 failures per minute in total trips a global limit;
  - 20 new accounts per hour per client.
  - "Client" is sha256(salt || IP) cut to 16 bytes. The salt is random per process and
    replaced every 24 hours. The IP is never stored. `ACCOUNTS_TRUST_PROXY=1` reads `X-Forwarded-For` from the local proxy.

### 2.5 Paid time

- Each account has `plan` (free/plus/pro) and `paid_until` (unix seconds). Entitlement means
  `now < paid_until`, otherwise Free. For card subscriptions there is a 1-day grace period
  (`RENEW_GRACE`) so a late renewal webhook doesn't cut anyone off.
- Paying for a different plan sets that plan. Extra time is added on top of any unused
  time. Cash and crypto are one-off top-ups, priced pro rata at 30 days per monthly price
  (€40 buys 60 days of Plus).

### 2.6 Payment adapters (`pay/`)

They share one trait, `PaymentAdapter { status, start, webhook }`. Each method reports
`Connected` or `NotConnected(reason)`. The detailed reason (which variable is missing) goes
only to the server log at start-up. Visitors see "Card payment is not connected yet." and
the matching buttons are disabled.

**Stripe (`pay/stripe.rs`)**, using the plain REST API ([API reference](https://docs.stripe.com/api)):

- **Transport.**
  - Default: `curl` as a subprocess. The key is passed on stdin with `-K -`, so it never
    appears in `ps`.
  - Optional cargo feature `reqwest`: blocking client with rustls and native roots (2.10).
  - `Stripe-Version: 2025-03-31.basil` is pinned. Older invoice shapes are parsed too.
- **Checkout.** `POST /v1/checkout/sessions` with:
  - `mode=subscription`, `line_items[0][price]`, `client_reference_id=<gp_ref>` and
    `subscription_data[metadata][galion_ref]`;
  - success and cancel URLs `/account?checkout=done|cancelled`, optional
    `automatic_tax[enabled]`, and `customer` when the account already has one;
  - **no** `customer_email`.
  ([Checkout Sessions](https://docs.stripe.com/api/checkout/sessions/create))
- **Prices.**
  - `STRIPE_PRICE_PLUS` / `STRIPE_PRICE_PRO` take a price id, or fall back to a lookup by
    `lookup_keys[]=galion_plus_monthly|galion_pro_monthly`, cached in memory.
- **Customer Portal.** `POST /v1/billing_portal/sessions` for "Manage" (change plan, card,
  cancel). ([Portal](https://docs.stripe.com/customer-management/integrate-customer-portal))
- **Idempotency.** Checkout sends `Idempotency-Key: galion-checkout-<ref>` (one per
  reference, so a double click cannot create two sessions). Every `stripe-setup` create sends
  a fixed key. Portal sessions are harmless to repeat and send none. ([Idempotent requests](https://docs.stripe.com/api/idempotent_requests))
- **Webhook `/webhooks/stripe`.**
  - The `Stripe-Signature` header is parsed into `t` and `v1`.
  - We compute `HMAC-SHA256(whsec, t + "." + raw_body)` (HMAC in `hmac.rs` over the in-repo
    pure-Rust SHA-256 of `galion-common`, constant-time compare) and accept any matching `v1`.
  - `|now - t| ≤ 300 s` is required. ([Verify manually](https://docs.stripe.com/webhooks#verify-manually))
- **Replay protection and ordering.**
  - Event ids are stored in `processed_events` for 30 days. A duplicate gets 200 with no
    effect.
  - An event we can't match yet gets **409** and its id is released again, so Stripe's
    automatic retry is applied later. This covers `invoice.paid` arriving before
    `checkout.session.completed`.
- **Events handled:**
  - `checkout.session.completed` links customer and subscription to the account via the ref
    and marks the ref used;
  - `invoice.paid` adds time up to `period_end` (+ grace) for the plan on the invoice's
    price;
  - `customer.subscription.updated` handles a plan change and a cancel-at-period-end;
  - `customer.subscription.deleted` cuts time to `ended_at`, **removes the Stripe ids**
    from the account, and leaves the plan reading Free once the time has run out.
  - Only `invoice.paid` ever adds time.
- **Setup command (Slavik runs it, not the agents).**
  - `galion-accounts stripe-setup` is a dry run by default and prints what it would create.
  - `--apply` creates:
    - a product for Plus and one for Pro (skipped if a price with that lookup key already exists);
    - prices of €20.00 and €220.00 a month (`tax_behavior=inclusive`), with lookup keys
      `galion_plus_monthly` and `galion_pro_monthly`;
    - with `--webhook-url`, a webhook endpoint for the 4 events. It prints the signing
      secret once.
  - Every create sends a fixed idempotency key, and existing lookup keys are skipped, so
    running it twice is safe.

**Cash (`pay/cash.rs`):**

- `POST /api/pay/cash` returns a reference `GC-XXXX-XXXX-XXXX` (60 bits, Crockford base32)
  that is valid for 120 days, plus the postal address from `ACCOUNTS_CASH_ADDRESS` (lines
  separated by `|`).
- The operator opens the envelope and runs
  `galion-accounts cash-credit --ref GC-… --amount-cents 4000`, or calls
  `POST /internal/v1/cash/credit` with the admin token.
  - Only EUR is accepted. Other currencies are refused, and the operator converts and
    credits by hand.
  - A reference can be used once.
  - After crediting, the reference row is **unlinked** from the account.
  - The ledger line has no account.
- Shred the envelope.

**Crypto (`pay/crypto.rs`):** a stub that reports "not connected". The two options:

| | BTCPay Server | monero-wallet-rpc |
|---|---|---|
| Coins | BTC (+ Lightning), others via plugins | XMR only |
| Privacy | On-chain BTC is public and traceable. A new address per invoice helps, but the payment is still visible. | Private by default. Our view key sees only our own incoming payments. |
| Ops | Docker stack with bitcoind/NBXplorer/Postgres, around 600 GB for a full node (or pruned). Webhooks are HMAC-signed (`BTCPay-Sig`). | monerod (around 100 GB pruned) plus wallet-rpc. We poll `get_transfers` and match by subaddress. No inbound webhook. |
| Licence | MIT | BSD-3 (Monero) |
| Prices | Fiat-denominated invoices, rate from BTCPay | We would need our own EUR/XMR rate source. |

Recommendation: start with Monero only if privacy is the goal. Add BTCPay if demand for
Bitcoin shows up. Either way, a self-hosted wallet on its own VPS, with no custodian.

### 2.7 The subscription privacy tradeoff (be honest about it)

While a card subscription is active, we store `stripe_customer` and `stripe_subscription`
on the account row. Anyone holding both our database and Stripe's records can link that
account to a card holder. This is exactly why Mullvad dropped subscriptions (2.1). Here is
how we contain it:

- The ids are deleted the moment `customer.subscription.deleted` arrives. The test
  `stripe_flow::full_lifecycle` dumps every table afterwards and finds no `cus_` or `sub_`.
- No email or account number is ever sent to Stripe, only `gp_<random>` references, which
  are unlinked after use.
- Users who want no link can pay by cash or crypto. Those are top-ups and never
  subscriptions.
- Option for later: one-off Stripe payments (`mode=payment`). They give the same
  convenience with no lasting link, at the cost of no auto-renewal.

### 2.8 Retention

| Data | Kept | Where |
|---|---|---|
| lookup hash, plan, paid_until | while the account exists. A sweep could delete accounts that have stayed free for N months (not built, Slavik to decide) | accounts |
| Stripe customer/subscription ids | only while subscribed | accounts |
| sessions | 30 days at most | sessions |
| payment refs | online 20 days, cash 120 days, unlinked once used | pay_refs |
| webhook event ids | 30 days | processed_events |
| money ledger (day, method, amount, currency) | statutory accounting period, no account link | ledger |
| IPs | never stored. Rate-limit keys stay in memory with a daily salt | n/a |

`galion-accounts sweep` (and an hourly thread in `serve`) deletes expired rows.

### 2.9 How other Galion services ask "what plan is this?"

- **Internal listener** (default 127.0.0.1:19971, never exposed publicly):
  `POST /internal/v1/entitlement` with `Authorization: Bearer $ACCOUNTS_INTERNAL_TOKEN` and
  `{"session": "<cookie value>"}` returns `{"plan","active","paid_until"}`.
- `galion_accounts::client::check_session(addr, token, session)` wraps that call for Rust
  consumers (console, Galion One, Hermes) and **fails closed to Free** on any error.
  `Entitled::at_least(Plan::Plus)` gates features.
- Health check: `GET /internal/v1/health`.

### 2.10 Transport note: reqwest

The brief said MIT or Apache dependencies, Rust first. Notes on how that was applied:

- reqwest's `rustls-tls` pulls in `webpki-roots`, which is CDLA-Permissive-2.0 and not on
  the allow-list. The optional feature therefore uses `rustls-tls-native-roots`.
- That feature still pulls in `ring`, which is Apache-2.0 AND ISC and contains C and
  assembly. The swarm rules say no asm, so the feature is **off by default**.
- The default build talks to Stripe through the system `curl` binary, so no TLS code is
  compiled into the crate.

### 2.11 How galion.app plugs in (the `/api/checkout` contract)

This matches the proposal in the galion-home worktree (`docs/payments/galion-home-checkout.md`).

- `HEAD /api/checkout?plan=plus|pro` answers **204** when card checkout is connected,
  **503** "not connected" otherwise, and 400 for an unknown plan. home.js uses it to switch
  its price buttons on or off.
- `GET /api/checkout?plan=…` (the price button links here):
  - logged in and connected: **303** to Stripe Checkout;
  - otherwise: 303 to `/account?plan=…`, and the page tells the visitor to create a number
    or log in first.
- Deploy-time routing on galion.app (not done, for Slavik): send `/account`,
  `/account.css`, `/account.js`, `/api/account/*`, `/api/pay/*`, `/api/checkout` and
  `/webhooks/*` to `127.0.0.1:19970`. Keep `19971` internal.
- The galion-home branch still has its own placeholder `home/account.html`, which must be
  removed or routed over at merge time.

### 2.12 Legal and tax flags (not legal advice, for a lawyer or accountant)

- **EU VAT (OSS):**
  - Digital services to EU consumers are taxed at the customer's country rate. That
    needs two non-conflicting pieces of location evidence, or a simplified rule under
    €100k a year.
  - An anonymous number provides none. Stripe Tax and automatic_tax collects the billing
    country at Checkout, which adds data on Stripe's side.
  - Cash and crypto have **no** location evidence. Decide: charge Croatian VAT (25 %) on
    them, or ask for a country with no other details.
  - Prices are set tax-inclusive. Confirm Croatian VAT registration and OSS.
- **Cash and AML:** check the Croatian AML law (ZSPNFT) cash limits and reporting duties,
  and the rules on accepting cash by post. Shredding the envelopes needs a written
  procedure (two people, a log of totals only).
- **Crypto:**
  - Accepting crypto as payment for our own service is generally not a CASP activity
    under MiCA, but check it.
  - The EU AMLR (from 2027) bans anonymous crypto accounts at CASPs, not at merchants.
    Check the Croatian tax treatment (HANFA and Porezna uprava).
- **KYC thresholds:** none apply to a merchant selling subscriptions, as far as we know.
  Stripe does its own KYC on Slavik's business.
- **Accounting retention:** Croatian law requires bookkeeping records (11 years for some
  documents). The ledger keeps totals with no account link. Check that this counts as
  enough evidence for cash income, and whether cash receipts must be fiscalised (Croatian
  fiscalisation applies to **cash** sales: this is a real risk to check).
- **Right of withdrawal:** EU consumers have 14 days for digital services unless they
  expressly agree to immediate start and acknowledge losing the right. Checkout needs that
  consent text (Stripe `consent_collection` or custom text).
- **PSD2/SCA:** handled by Stripe Checkout (3-D Secure). Off-session renewals use the
  mandate Stripe stores.
- **GDPR:** the numbered account is plausibly not personal data on its own. Stripe data is.
  Publish a privacy policy listing exactly the table in 2.8.

### 2.13 What Slavik has to provide

1. **Stripe:**
   - an account for the Galion business (and its KYC);
   - run `galion-accounts stripe-setup` (dry run), then `--apply --webhook-url
     https://galion.app/webhooks/stripe` with `STRIPE_SECRET_KEY` (a restricted key with
     write access to Checkout Sessions, Billing Portal, Products, Prices and Webhook
     Endpoints is enough);
   - put the printed `whsec_…` in `STRIPE_WEBHOOK_SECRET`;
   - in the Dashboard, configure the Customer Portal (allow cancel, plan switch
     Plus↔Pro, card update);
   - decide on Stripe Tax (`STRIPE_AUTOMATIC_TAX=1`).
2. **Crypto:** choose BTCPay Server, monero-wallet-rpc, or neither for now.
3. **Cash:** a mailing address (a PO box is best) for `ACCOUNTS_CASH_ADDRESS`, and who
   opens the envelopes.
4. **Secrets on the server:**
   - a pepper file (`galion-accounts new-pepper --out …`), backed up offline;
   - `ACCOUNTS_INTERNAL_TOKEN` and `ACCOUNTS_ADMIN_TOKEN`.
5. **Legal:** a lawyer or accountant to check 2.12, especially VAT location evidence and
   fiscalisation of cash, plus the withdrawal consent text and the privacy policy.
6. **Routing and deploy:** the rules in 2.11, a systemd unit and a backup of the SQLite
   file. Not done: nothing was deployed.
