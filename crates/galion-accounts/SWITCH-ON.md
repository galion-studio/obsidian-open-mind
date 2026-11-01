<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Switching payments on for galion.app

This is for Slavik. It assumes galion-accounts is already running on the server as
`galion-accounts-95244f0d` and galion-one already forwards `/account`, `/api/account/*`,
`/api/pay/*`, `/api/checkout` and `/webhooks/*` (drop-in `92-accounts-…`).

Everything below runs **as root on the server**. Nothing has been run there yet.

Some rules for every step:

- **Never delete anything.** Rollback means renaming to `.off`, then `daemon-reload` and a
  restart.
- **Keep secrets off the screen.**
  - Edit secret files with `sudoedit` or `nano`. Never `cat` them, never `echo` a key,
    never paste one into chat or a ticket.
  - `readiness` prints only yes/no and `ok`/`MISS`. It never prints a value.

`<SHA>` is the commit the new binary was built from. It is listed in `bin/SHA256SUMS`
next to `galion-accounts-<SHA>`.

## What you get

Plans (all EUR, VAT included in the price):

| Plan | Price | Lookup key |
|---|---|---|
| Free | €0 (open source, MIT, runs locally) | none |
| Plus | €20 per month | `galion_plus_monthly` |
| Pro | €220 per month (Hermes gets its own cloud computer and SSD) | `galion_pro_monthly` |

Payment methods. Each one stays **off** until its settings are filled in:

| Method | How it works | Turned on by |
|---|---|---|
| **Card (Stripe, main)** | Monthly subscription through Stripe Checkout. The customer can cancel or resume on `/account`, or use the Stripe portal to change card or switch Plus↔Pro. If a renewal fails, the customer keeps 7 days of access while Stripe retries. Refunds are booked automatically; a full refund ends the paid time. | `STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET`, `STRIPE_PORTAL_CONFIG` |
| **BTC (BTCPay)** | EUR invoice; credited once BTCPay confirms it. Partly paid, late or overpaid invoices are credited pro rata from confirmed payments only. A webhook triggers the credit, and a poll every 5 minutes catches any missed webhook. | `ACCOUNTS_CRYPTO_BACKENDS=btcpay` + `BTCPAY_*` |
| **XMR (monero-wallet-rpc)** | One subaddress per payment; credited after 10 confirmations, pro rata. | `ACCOUNTS_CRYPTO_BACKENDS=monero` + `MONERO_*` |
| **Cash in an envelope** | The customer gets a `GC-XXXX-XXXX-XXXX` reference and a printable slip. The slip has no account number on it. The operator then marks the cash received. | `ACCOUNTS_CASH_ADDRESS` |

Cash or crypto time bought while a card subscription is running is **saved for later**
("banked"). It starts when the card subscription ends, so the customer never pays twice
for the same days.

---

## 0. Look first (read-only)

```sh
systemctl cat galion-accounts | grep -E '^# /|ExecStart|EnvironmentFile'
ls /etc/systemd/system/galion-accounts.service.d/ 2>/dev/null   # expected: missing
ls /etc/systemd/system/galion-one.service.d/                    # expected: highest is 92-accounts-…
ls -l /etc/galion/accounts.env /etc/galion/accounts.pepper      # root, 0600 / 0400
command -v curl                                                 # Stripe, BTCPay and Monero calls go through curl
```

- The unit must have `EnvironmentFile=/etc/galion/accounts.env`. If it doesn't, add that
  line to the drop-in in step 6.
- If `galion-accounts.service.d/` already has drop-ins, give ours a number **higher than
  the highest one**.

## 1. Back up the pepper offline (once, before anything else)

`/etc/galion/accounts.pepper` turns account numbers into database keys. **If it is lost,
nobody can ever log in again.** Database backups do not contain it.

```sh
install -d -m 0700 /root/offline
cp -p /etc/galion/accounts.pepper /root/offline/accounts.pepper.$(date +%Y%m%d)
sha256sum /etc/galion/accounts.pepper /root/offline/accounts.pepper.*   # the two hashes must match; this is not the secret
```

Then copy `/root/offline/accounts.pepper.*` to your offline storage (USB stick, password
manager file attachment) with `scp` from your own machine. Do not open it.

Also take a database snapshot before the new binary migrates the schema (it only adds
columns):

```sh
systemctl start galion-accounts-backup.service 2>/dev/null || echo "no backup unit: copy accounts.sqlite with the service stopped"
```

## 2. Install the new binary (it is not running yet)

```sh
cd /opt/galion/rust/bin
# copy bin/galion-accounts-<SHA> and bin/SHA256SUMS here the usual way, then:
sha256sum -c SHA256SUMS --ignore-missing
chown root:root galion-accounts-<SHA> && chmod 0755 galion-accounts-<SHA>
./galion-accounts-<SHA> --help | head -1
```

## 3. Generate the internal and admin tokens on the server

The values go straight into the file and are never shown on screen:

```sh
cp -p /etc/galion/accounts.env /etc/galion/accounts.env.bak.$(date +%Y%m%d%H%M)
gen() { head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n'; }
sed -i "s/^ACCOUNTS_INTERNAL_TOKEN=\$/ACCOUNTS_INTERNAL_TOKEN=$(gen)/" /etc/galion/accounts.env
sed -i "s/^ACCOUNTS_ADMIN_TOKEN=\$/ACCOUNTS_ADMIN_TOKEN=$(gen)/"       /etc/galion/accounts.env
grep -c '^ACCOUNTS_\(INTERNAL\|ADMIN\)_TOKEN=.\{64\}$' /etc/galion/accounts.env   # 2
```

If the file has no `ACCOUNTS_ADMIN_TOKEN=` line yet, add the missing lines from
`accounts.env.template` with `sudoedit` first. Copying the template over the file would
also work, but it would drop anything already set.

Use them like this. Read the token from the file and send it as a header, so it never
shows in `ps` or the shell history:

```sh
admin() { awk -F= '/^ACCOUNTS_ADMIN_TOKEN=/{sub(/^[^=]*=/,""); print "Authorization: Bearer " $0}' /etc/galion/accounts.env \
          | curl -sS -H @- "$@"; }
admin http://127.0.0.1:19971/internal/v1/admin/status      # accounts, active plans, ledger per month, duplicates
```

## 4. Stripe (Slavik types the key himself)

1. In the Stripe Dashboard (start in **test mode**; repeat everything with live keys
   when the test works):
   - Business details and payouts are done, and the account currency is EUR.
   - Create a **restricted key** (`rk_…`) with **write** access to: Products, Prices,
     Checkout Sessions, Customer portal (configurations and sessions), Subscriptions,
     Webhook Endpoints. A normal `sk_…` key also works.
2. Put the key in the file yourself: `sudoedit /etc/galion/accounts.env`, line
   `STRIPE_SECRET_KEY=rk_…`. Do not type it on the command line.
3. **Dry run.** It only reads from Stripe and shows what it would create:
   ```sh
   /opt/galion/rust/bin/galion-accounts-<SHA> stripe-setup --env-file /etc/galion/accounts.env \
       --webhook-url https://galion.app/webhooks/stripe
   ```
   Expect `would create plus … EUR 20.00/month`, `would create pro … EUR 220.00/month`, a
   billing portal configuration, and a webhook endpoint with 6 events. If it says a price
   exists with a **wrong amount**, stop. Fix it in the Dashboard by archiving the old
   price and removing its lookup key.
4. **Apply:**
   ```sh
   /opt/galion/rust/bin/galion-accounts-<SHA> stripe-setup --env-file /etc/galion/accounts.env \
       --apply --webhook-url https://galion.app/webhooks/stripe --secret-out /root/whsec.env
   ```
   - It creates the Plus and Pro products, their EUR monthly prices (tax inclusive), the
     customer portal configuration, and the webhook endpoint with these events:
     - `checkout.session.completed`
     - `invoice.paid`
     - `invoice.payment_failed`
     - `customer.subscription.updated`
     - `customer.subscription.deleted`
     - `charge.refunded`
   - Every create has an idempotency key, so running it again is safe. A re-run creates
     nothing, and adds any events missing from an existing endpoint.
   - The new signing secret is written **only** to `/root/whsec.env`, with mode 0600.
     The command refuses to run if that file already exists.
   - It prints `set STRIPE_PORTAL_CONFIG=bpc_…`. That id is not a secret.
5. Move the secret and the portal id into the env file without showing them:
   ```sh
   # only when the file is non-empty AND the line in accounts.env is still empty:
   [ -s /root/whsec.env ] && grep -q '^STRIPE_WEBHOOK_SECRET=$' /etc/galion/accounts.env && \
     sed -i '/^STRIPE_WEBHOOK_SECRET=$/{r /root/whsec.env
   d}' /etc/galion/accounts.env
   grep -c '^STRIPE_WEBHOOK_SECRET=whsec_' /etc/galion/accounts.env   # 1
   mv /root/whsec.env /root/whsec.env.used        # keep it until the switch works, then shred it yourself
   sudoedit /etc/galion/accounts.env              # STRIPE_PORTAL_CONFIG=bpc_… (from the output)
   ```
   - If the endpoint **already existed**, Stripe does not hand out its secret again. No
     file is written. Copy the `whsec_…` from Dashboard → Developers → Webhooks →
     endpoint → "Signing secret" into the file yourself. In that case, `/root/whsec.env`
     is left empty, and the command says so.
6. Optional: `STRIPE_AUTOMATIC_TAX=1`, but only after Stripe Tax is set up (registrations,
   EU OSS).
7. Cloudflare: make sure no WAF rule, Bot Fight Mode or challenge blocks `POST
   /webhooks/stripe` or `/webhooks/crypto`. Stripe and BTCPay cannot solve challenges.

## 5. BTC through BTCPay (optional)

1. Use a self-hosted BTCPay Server, with its own node, on a separate machine. Create a
   store with default currency **EUR**.
2. Create a Greenfield API key (Account → API keys) for that store with these
   permissions:
   - `btcpay.store.cancreateinvoice`
   - `btcpay.store.canviewinvoices`
   - `btcpay.store.canviewstoresettings` (only needed if XMR uses the BTCPay rate)
3. Create a store webhook (Store → Webhooks):
   - URL: `https://galion.app/webhooks/crypto`
   - Events: **InvoiceSettled** and **InvoiceExpired** (or "all events")
   - Automatic redelivery: on
   - Copy its secret.
4. `sudoedit /etc/galion/accounts.env`:
   ```
   ACCOUNTS_CRYPTO_BACKENDS=btcpay
   BTCPAY_URL=https://pay.example        (no trailing slash)
   BTCPAY_STORE_ID=…
   BTCPAY_API_KEY=…
   BTCPAY_WEBHOOK_SECRET=…
   ```

What the customer gets:

| Payment | Days credited |
|---|---|
| Paid in full | 30 days of the plan |
| Underpaid, then the invoice expired | Pro rata, from confirmed payments only |
| Overpaid | Pro rata, more than 30 days, capped at 12 months |
| Still confirming | Nothing yet; credited when confirmed. A webhook triggers it, and the 5-minute poll catches any missed webhook. |

## 6. XMR through monero-wallet-rpc (optional)

1. Run `monerod` and `monero-wallet-rpc` on our own machine. Use a **view-only** wallet
   (the spend key stays offline), and start it with `--rpc-login user:pass`, bound to
   `127.0.0.1` or a private network.
2. `sudoedit /etc/galion/accounts.env`:
   ```
   ACCOUNTS_CRYPTO_BACKENDS=btcpay,monero          (or just monero)
   MONERO_WALLET_RPC_URL=http://127.0.0.1:18083/json_rpc
   MONERO_WALLET_RPC_LOGIN=user:pass
   MONERO_EUR_RATE=150.25        (EUR per XMR, you keep it current)
   MONERO_RATE_SOURCE=btcpay     (or this instead: the XMR_EUR rate from the BTCPay store)
   ```
3. Defaults: 10 confirmations, one subaddress per payment, 60-minute invoices, wallet
   polled every 60 s. Partial payments are credited pro rata.

## 7. Cash address (optional)

`sudoedit /etc/galion/accounts.env` and set:

```
ACCOUNTS_CASH_ADDRESS=Galion|PO Box 123|10000 Zagreb|Croatia
```

The `|` separates lines. It is safe in this file because systemd reads it with no shell.

When an envelope arrives:

1. Look up the reference. The result shows the plan and price, never the account:
   ```sh
   admin 'http://127.0.0.1:19971/internal/v1/cash/lookup?ref=GC-XXXX-XXXX-XXXX'
   ```
2. Mark the cash received. `amount_cents` is what was in the envelope, and days are
   credited pro rata:
   ```sh
   admin -X POST -H 'Content-Type: application/json' \
     -d '{"reference":"GC-XXXX-XXXX-XXXX","amount_cents":2000}' \
     http://127.0.0.1:19971/internal/v1/cash/credit
   ```
3. Shred the envelope.

References stay valid for 120 days. The customer can reprint the slip at
`/account/cash-slip?ref=…` while logged in. The slip shows the reference, plan, amount
and address, but no account number.

## 8. Check before switching (prints yes/no only)

```sh
/opt/galion/rust/bin/galion-accounts-<SHA> readiness --env-file /etc/galion/accounts.env \
   --pepper-file /etc/galion/accounts.pepper --db /var/lib/galion-accounts/accounts.sqlite \
   --require stripe          # add ,cash ,btcpay ,monero for what you turned on
```

- This opens the database **read-only** (immutable), so it is safe next to the running
  service.
- Expected result: `READY`, exit code 0. Any `MISS` line names the setting that is
  missing.

## 9. Deploy: a new drop-in, nothing edited

`/etc/systemd/system/galion-accounts.service.d/50-payments-<SHA>.conf` keeps the same
arguments as today and only changes the binary:

```ini
# galion-accounts payments complete (<SHA>). Rollback: rename to .off, daemon-reload, restart.
[Service]
ExecStart=
ExecStart=/opt/galion/rust/bin/galion-accounts-<SHA> serve --listen 127.0.0.1:19970 --internal 127.0.0.1:19971 --db /var/lib/galion-accounts/accounts.sqlite --pepper-file /run/galion-accounts/pepper
```

(Add `EnvironmentFile=/etc/galion/accounts.env` here only if step 0 showed the unit lacks
it.)

```sh
install -d -m 0755 /etc/systemd/system/galion-accounts.service.d
install -m 0644 50-payments-<SHA>.conf /etc/systemd/system/galion-accounts.service.d/
systemctl daemon-reload && systemctl restart galion-accounts
systemctl show galion-accounts -p ExecStart | grep -o 'galion-accounts-<SHA>'
journalctl -u galion-accounts -n 15 --no-pager      # "payment stripe: connected", cash/crypto as configured
curl -s http://127.0.0.1:19971/internal/v1/ready    # booleans only, no token needed (loopback)
curl -sI 'http://127.0.0.1:19970/api/checkout?plan=plus' | head -1   # HTTP/1.1 204 = card is live
```

Turn the "Subscribe" buttons on. This is
`/etc/systemd/system/galion-one.service.d/93-checkout-probe.conf`:

```ini
# Home page price buttons ask HEAD /api/checkout; they show "Subscribe" only on 2xx.
# Rollback: rename to .off, daemon-reload, restart galion-one.
[Service]
Environment=ONE_CHECKOUT=probe
```

```sh
install -m 0644 93-checkout-probe.conf /etc/systemd/system/galion-one.service.d/
systemctl daemon-reload && systemctl restart galion-one     # about 1 s blip for galion.app
curl -sI -H 'Host: galion.app' 'http://127.0.0.1:8091/api/checkout?plan=plus' | head -1   # 204
```

**End-to-end test in test mode.** Pay on https://galion.app/account with card
`4242 4242 4242 4242`, then check:

- `/account` shows Plus and "renews".
- Dashboard → Webhooks shows 200s.
- Run `admin …/admin/status`: `active_plus` is 1 and the ledger shows +2000.
- Then try **Cancel renewal**: the page shows "ends on …". Then **Resume**.

Then repeat steps 4–9 with the **live** key: a new `rk_live_…`, then `stripe-setup
--apply` again. Live mode has its own products, webhook and secret.

## 10. Rollback (no deletes)

| What | How |
|---|---|
| Buttons back to "Coming soon" | `mv /etc/systemd/system/galion-one.service.d/93-checkout-probe.conf{,.off}`, then `systemctl daemon-reload && systemctl restart galion-one` |
| Card off, keep the new binary | `sudoedit /etc/galion/accounts.env`, prefix `STRIPE_SECRET_KEY=` with `#`, then `systemctl restart galion-accounts`. Card shows "not connected"; checkout answers 503. |
| Back to the old binary 95244f0d | `mv /etc/systemd/system/galion-accounts.service.d/50-payments-<SHA>.conf{,.off}`, then `systemctl daemon-reload && systemctl restart galion-accounts` |
| Undo the env edits | `cp -p /etc/galion/accounts.env.bak.<time> /etc/galion/accounts.env`, after first keeping the current one as `.bak2` |

The new binary only **adds** database columns and tables. The old binary names its
columns explicitly, so it keeps working on the migrated database.

## 11. Running it

**Refund.** Refund in the Stripe Dashboard. The `charge.refunded` webhook books it:

- **Partial refund** (goodwill): the paid time is kept.
- **Full refund:** the paid time ends now.

A full refund does **not** cancel the subscription. Also cancel it in the Dashboard, or
the next month will be charged again.

**Duplicate subscription.** This happens when someone finishes two checkouts. `admin
…/admin/status` lists `duplicate_subscription_ids`. Those subscriptions gave no time.
Cancel each one in the Dashboard (Subscriptions → search the id) and refund its invoice.

**Failed card.** Stripe retries (Dashboard → Billing → Revenue recovery: "Smart Retries",
then "cancel subscription" when retries run out). The customer keeps access for 7 days
past the end of the period and sees "your last card payment failed". If a retry succeeds,
everything goes back to normal. If it doesn't, the subscription is deleted and the paid
time ends.

**Crypto stuck.** Run `admin -X POST http://127.0.0.1:19971/internal/v1/crypto/poll` for
a manual reconcile.

## What only Slavik can supply

1. An offline copy of `/etc/galion/accounts.pepper` (step 1).
2. Stripe: the business account and KYC, the restricted key typed into `accounts.env`,
   and the `whsec_…` file from `stripe-setup --apply` (or from the Dashboard). Test mode
   first, then live. Also the Stripe Tax decision.
3. Cloudflare: allow `POST /webhooks/*` through with no challenge.
4. BTCPay (if wanted): server URL, store id, Greenfield API key, webhook secret.
5. Monero (if wanted): the monero-wallet-rpc URL and `--rpc-login`, a view-only wallet,
   and the rate (a fixed EUR/XMR or the BTCPay store rate).
6. Cash (if wanted): the postal address (a PO box) and who opens the envelopes.
7. The internal and admin tokens, generated on the server (step 3).
8. The legal and tax points in DESIGN.md 2.12: VAT location evidence, fiscalisation of
   cash, withdrawal consent, privacy policy.
9. **The yes** to copy the binary and install the two drop-ins.
