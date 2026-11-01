<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
> **DRAFT, not legal advice.** Written 2026-11-01 (Europe/Zagreb, CET) for Slavik. It must be
> reviewed by a Croatian lawyer and accountant before it is published or relied on. Every
> `[PLACEHOLDER]` must be filled in by Slavik; nothing in brackets is a real company detail.

# Privacy Policy

Last updated: [DATE OF PUBLICATION]

## The short version

- Your Galion account is a random 16-digit number. We do not ask for your name, email or phone.
- We do not store your IP address. We keep no activity logs about what you do with your account.
- We keep only what we need to know whether your account has paid time, and the totals our
  accountant is required by law to keep.
- If you pay by card, Stripe handles the payment. Stripe may ask for your email and country and
  keeps its own records. If you want no link at all between you and your account, pay by cash
  (or by crypto, when it is switched on).
- If you run Galion yourself (the Free, open source version), it runs on your computer and
  sends nothing to us.

## Who is responsible

The data controller is:

[COMPANY NAME]
[LEGAL FORM, e.g. obrt / j.d.o.o. / d.o.o.]
[ADDRESS], Croatia
OIB: [OIB]
Contact: [CONTACT EMAIL OR OTHER CONTACT CHANNEL]

## Your numbered account

When you press "Generate account number", our server creates a random 16-digit number and shows
it to you once. That number is your user name and your password.

- We store only a **scrambled form** of the number (a one-way hash made with a secret key). It
  cannot be turned back into the number. This is why **we cannot show you your number again,
  and we cannot recover a lost number.**
- With the account we store: the plan (Free, Plus or Pro) and the date your paid time ends.
- Anyone can create as many accounts as they like. An account number does not tell us who you are.

## What we keep, and for how long

| What | Why | How long |
|---|---|---|
| Scrambled account number, plan, paid-until date | To know whether your account has paid time | While the account exists [Slavik: decide whether unpaid Free accounts are deleted after N months] |
| Sign-in session (cookie `galion_acct`; we store only a scrambled copy of it) | To keep you signed in | Up to 30 days, or until you sign out |
| Stripe customer id and subscription id | So monthly card renewals reach your account | Only while your card subscription is active. Deleted when it ends |
| Payment reference (a random code such as `gp_…` or `GC-…`) | To match a payment to your account | Card and online: 20 days. Cash: 120 days (post can be slow). Unlinked from your account once used |
| Stripe event ids | To avoid counting one payment twice | 30 days |
| Money ledger: day, payment method, amount, currency | Required by accounting and tax law | The legal retention period [lawyer/accountant: confirm, Croatian law may require up to 11 years for some records]. It has **no** link to any account |
| Rate-limit counters (a scrambled form of your IP address, with a key replaced every 24 hours) | To stop password-guessing and abuse | In memory only, never written to disk, gone within 24 hours |

**What we do not keep:** IP addresses, email addresses, names, phone numbers, browsing or
activity logs, or analytics. We use no tracking cookies and no third-party analytics.

## Payments

### Card (Stripe)

Card payments run on Stripe's own checkout page. We never see your card number.

- We send Stripe a random payment reference. We never send it your account number or an email.
- Stripe may ask you for an email address and your billing country (for receipts, fraud checks
  and VAT) and keeps its own records as a payment company. For that processing Stripe is
  generally its own controller; see [Stripe's privacy policy](https://stripe.com/privacy).
- **Be aware:** while a monthly card subscription is active, we keep Stripe's customer and
  subscription ids next to your account. Someone holding both our records and Stripe's could
  link that account to the card holder. When the subscription ends, we delete those ids.

### Cash by post

You get a reference such as `GC-PA20-FS10-VCT8`. Put only that reference on a slip of paper,
with euro notes, in an envelope, and post it to [CASH POSTAL ADDRESS]. Please do not write your
name, your address or your account number on the envelope or the slip.

We open the envelope, add the time to the account, and shred the envelope and slip. We keep the
day, the amount and the currency in the money ledger, with no link to any account.

### Crypto (not yet available)

When it is switched on, we plan to use our own self-hosted wallet ([BTCPay Server for Bitcoin
and/or Monero], no third-party custodian). Note that Bitcoin payments are public on the
blockchain. This section will be updated before crypto payments open.

## Using the hosted service (Plus and Pro)

[SLAVIK: describe exactly what the hosted Galion One, Hermes and the Pro cloud computer store,
for example conversations, memory, files on the Pro SSD; where they are stored; whether any
request is sent to a third-party AI model provider and which one; and when this data is deleted
after a subscription ends. This section is required and must match how the service really works.]

Your own content belongs to you. We do not sell it, and we do not use it to train AI models
[SLAVIK: confirm].

## Running Galion yourself (Free)

The Free version is open source under the MIT License and runs on your own computer. It does not
send us your data. If you connect it to an online AI model, that model's provider receives what
you send it, under that provider's terms.

## Who else processes data

We only share data with suppliers who help us run the service:

| Supplier | What they do | What they can see |
|---|---|---|
| Stripe ([STRIPE ENTITY, e.g. Stripe Payments Europe, Ltd., Ireland]) | Card payments | What you enter on the Stripe checkout page |
| Cloudflare ([CLOUDFLARE ENTITY]) | Network in front of galion.app (delivery and protection from attacks) | Your IP address and the requests you make, as part of delivering them [lawyer: confirm Cloudflare's role and logging] |
| [HOSTING PROVIDER] | The servers Galion runs on | Data stored on those servers |
| [ACCOUNTANT] | Bookkeeping | The money ledger only (no account link) |

### Transfers outside the EU/EEA

[SLAVIK/lawyer: confirm. Stripe and Cloudflare are part of groups based in the United States and
may transfer data there, under the EU-US Data Privacy Framework or Standard Contractual
Clauses. Say where Galion's own servers are located.]

## Why we are allowed to process this (legal bases)

- **To provide the service you asked for** (GDPR Art. 6(1)(b)): account, sessions, plan and paid
  time, payment references, Stripe ids.
- **Legal obligation** (Art. 6(1)(c)): the money ledger for accounting and tax.
- **Legitimate interest** (Art. 6(1)(f)): rate limits and security, to protect accounts from
  guessing and the service from abuse.

We make no automated decisions about you, and we do no profiling.

## Your rights

Under the GDPR you can ask to see, correct or delete personal data about you, to restrict or
object to its processing, and to receive it in a portable form.

Because we do not know who owns an account, in most cases we hold nothing that identifies you
(GDPR Art. 11). If you can show us your account number, we can tell you what is stored for that
account, and delete the account. Deleting an account cannot be undone and ends any paid time.

You can complain to the Croatian data protection authority: Agencija za zaštitu osobnih podataka
(AZOP), [AZOP ADDRESS], [azop.hr](https://azop.hr), or to the authority where you live.

## If authorities ask

We follow the law. But we can only hand over what we have, and that is described above: for most
accounts, "some account has paid time until a certain date", with no way to say who owns it.

## Changes

If we change this policy, we will publish the new version here with a new date, and announce
material changes on galion.app at least [30] days before they apply.

## Contact

[CONTACT EMAIL OR OTHER CONTACT CHANNEL]. Please do not send your account number by email unless
we ask for it, and never send it to anyone claiming to be us.

---

*Modelled on the structure and plain language of Mullvad VPN's public pages, read on
2026-11-01: [Privacy policy](https://mullvad.net/en/help/privacy-policy),
[No-logging of user activity policy](https://mullvad.net/en/help/no-logging-data-policy).
No text was copied; Galion's facts come from the galion-accounts design
(`/workspace/swarm/tmp/accounts/DESIGN.md`, sections 2.4, 2.6-2.8).*
