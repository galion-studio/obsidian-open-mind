<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# galion-accounts

Galion accounts work the way Mullvad's do: an account is a 16-digit number, with no email
and no name. One subscription covers all of galion.app: Free, Plus at €20/month and Pro at
€220/month. You can pay by card through Stripe (a monthly subscription), by cash in an
envelope, or by crypto through our own BTCPay Server or Monero wallet (off unless configured). See [DESIGN.md](DESIGN.md) for why it
works this way and what it stores.

## Run it locally

```sh
cargo build -p galion-accounts --release
galion-accounts new-pepper --out /path/accounts.pepper   # once; back it up offline
galion-accounts serve --db /path/accounts.sqlite --pepper-file /path/accounts.pepper \
  --listen 127.0.0.1:19970 --internal 127.0.0.1:19971
# open http://127.0.0.1:19970/account (add --insecure-cookies for plain-http testing)
```

At start-up the server prints each payment method as `connected` or `not connected`, with
the reason. Any method that isn't connected stays switched off on the page.

## Configuration (environment)

| Variable | Meaning |
|---|---|
| `ACCOUNTS_DB`, `ACCOUNTS_PEPPER_FILE` | Same as `--db` and `--pepper-file`. |
| `ACCOUNTS_INTERNAL_TOKEN` | Bearer token for `/internal/v1/entitlement` on the internal port. |
| `ACCOUNTS_ADMIN_TOKEN` | Bearer token for `/internal/v1/cash/credit`. |
| `ACCOUNTS_COOKIE_DOMAIN` | For example `.galion.app`, to share the login across subdomains. |
| `ACCOUNTS_TRUST_PROXY=1` | Read the client IP from `X-Forwarded-For`. The IP is only used for in-memory rate limits. |
| `ACCOUNTS_PUBLIC_URL` | Base URL for the Stripe return URLs. Default `https://galion.app`. |
| `STRIPE_SECRET_KEY` | `sk_…` or a restricted `rk_…` key. If unset, card payment shows as not connected. |
| `STRIPE_WEBHOOK_SECRET` | `whsec_…` from the webhook endpoint. |
| `STRIPE_PRICE_PLUS`, `STRIPE_PRICE_PRO` | Optional price ids. Otherwise the server looks up `galion_plus_monthly` / `galion_pro_monthly`. |
| `STRIPE_AUTOMATIC_TAX=1` | Turns on Stripe Tax in Checkout (Stripe then collects the billing country). |
| `STRIPE_WEBHOOK_TOLERANCE_SECS` | Default 300. |
| `ACCOUNTS_CASH_ADDRESS` | Postal address, with lines separated by `\|`. If unset, cash shows as not connected. |
| `ACCOUNTS_CRYPTO_BACKENDS` | Comma list: `btcpay`, `monero`. Unset means crypto is off and the button stays hidden. |
| `BTCPAY_URL`, `BTCPAY_STORE_ID`, `BTCPAY_API_KEY`, `BTCPAY_WEBHOOK_SECRET` | BTCPay Greenfield settings (all four are needed). `BTCPAY_INVOICE_MINUTES` defaults to 60. |
| `MONERO_WALLET_RPC_URL`, `MONERO_WALLET_RPC_LOGIN` | monero-wallet-rpc `/json_rpc` and its `--rpc-login`. |
| `MONERO_EUR_RATE` or `MONERO_RATE_SOURCE=btcpay` | EUR per XMR, or the `XMR_EUR` rate of our BTCPay store. One of them is required. |
| `MONERO_CONFIRMATIONS`, `MONERO_ADDRESS_MODE`, `MONERO_ACCOUNT_INDEX`, `MONERO_INVOICE_MINUTES`, `MONERO_POLL_SECONDS` | Defaults: 10, `subaddress`, 0, 60, 60. |
| `ACCOUNTS_PUBLIC_URL` | Base URL for the BTCPay redirect (default `https://galion.app`). |

## Stripe setup (the owner runs this, not an agent)

```sh
export STRIPE_SECRET_KEY=sk_live_...           # or a restricted key
galion-accounts stripe-setup                   # dry run: prints the plan, sends nothing
galion-accounts stripe-setup --apply --webhook-url https://galion.app/webhooks/stripe
# put the printed STRIPE_WEBHOOK_SECRET=whsec_... into the service environment
```

This creates the Plus and Pro products and their EUR monthly prices (tax-inclusive, with
lookup keys `galion_plus_monthly` and `galion_pro_monthly`). It also creates a webhook
endpoint for `checkout.session.completed`, `invoice.paid`, `customer.subscription.updated`
and `customer.subscription.deleted`. Running it twice is safe.

In the Stripe Dashboard, turn on the **Customer Portal** and allow cancelling, switching
between Plus and Pro, and updating the payment method. The "Manage" button opens it.

## Cash

1. The visitor asks for a reference and posts euro notes with a slip that shows only
   `GC-XXXX-XXXX-XXXX`.
2. The operator credits it:

   ```sh
   galion-accounts cash-credit --ref GC-XXXX-XXXX-XXXX --amount-cents 4000 --db /path/accounts.sqlite
   ```

3. Shred the envelope.

## Crypto

Both backends are self-hosted and stay off unless they are configured (DESIGN 2.6).

- **BTCPay:** we create a EUR invoice tagged only with a `gx_…` reference. BTCPay calls
  `/webhooks/crypto`. We check `BTCPay-Sig`, read the invoice back and credit the plan pro
  rata.
- **Monero:** each payment gets a new subaddress (or integrated address), priced in XMR
  from the configured rate. `serve` polls `get_transfers`. You can also run one poll by
  hand:

  ```sh
  galion-accounts crypto-poll --db /path/accounts.sqlite
  ```

## Routes

- Public:
  - `/account` and its css/js;
  - `/api/account/{create,login,logout,me,plans}`;
  - `/api/pay/stripe/{checkout,portal}`, `/api/pay/cash` and `/api/pay/crypto`;
  - `HEAD|GET /api/checkout?plan=` (the galion-home contract);
  - `/webhooks/{stripe,crypto}`.
- Internal (keep off the internet): `/internal/v1/{health,entitlement,cash/credit,crypto/poll}`.

Rust consumers can call `galion_accounts::client::check_session`. It fails closed to Free.

## Tests

```sh
cargo test -p galion-accounts                    # no network; Stripe, BTCPay and wallet-rpc are local mocks
GALION_REAL=1 cargo test -p galion-accounts --release --test real_binary -- --ignored --nocapture
```

The default build sends Stripe calls through the system `curl` binary, with the key passed
on stdin. The optional `--features reqwest` build uses rustls with native roots instead
(it pulls in `ring`, see DESIGN 2.10).
