<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: galion-accounts

All times are Europe/Zagreb, CET (+01:00). Branch `swarm/galion-accounts-20261101`. Box
only. Nothing ran on or wrote to the VPS. No Stripe network calls were made.

## What runs

`tests/real_binary.rs`, gated by `GALION_REAL=1` and `--ignored`, runs the built binary
with the production argon2 cost and a real SQLite file under
`/workspace/swarm/tmp/accounts/real-<pid>/`, on 127.0.0.1:19970 (public) and :19971
(internal). It checks that:

- `new-pepper` writes the file with mode 0600 and refuses to overwrite it;
- the `stripe-setup` dry run needs no key and sends nothing;
- `serve` with no Stripe variables reports "not connected";
- the page loads and the account create, login and me calls work, with card checkout answering 503;
- a cash reference can be credited through the `cash-credit` CLI;
- `client::check_session` against the internal port returns the right plan and fails
  closed when the token is wrong;
- the database file holds no account number.

## Results

| When | Build | create | login | db + wal |
|---|---|---|---|---|
| 2026-11-01 (earlier run) | debug | ok | ok | n/a |
| 2026-11-01 (earlier run) | release | 39 ms | 38 ms | 123608 bytes |
| 2026-11-01 14:52 +01:00 | release | 32 ms | 32 ms | 123608 bytes |

The argon2id settings are 19 MiB, t=2, p=1.

## Screenshots

On 2026-11-01 around 14:45 +01:00, `serve` ran on 19970 with only cash connected, and
headless Chrome 156 was driven over CDP from port 19975. The screenshots are in
`/workspace/swarm/tmp/accounts/shots/`: logged out, new number, logged in, and cash
reference, each at 390x844 (DPR 2) and 1440x900. No view had horizontal overflow. The Jost
font is not served by this crate, so system fonts appear in the shots.
