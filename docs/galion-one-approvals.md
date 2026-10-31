<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Galion One: Approvals screen and admin key (approval-system C)

Written 2026-10-31 09:17 CET (+01:00) Europe/Zagreb. Formats: approval-system `A-core/FORMATS.md` v1.

## What it is
`/approvals` on one.galion.app (built into the galion-one binary; same sky, glass and type as the
V2 room). A link "Approvals" sits under Devices in the room and shows "N waiting for you".

Each Hermes self-change request is one card: the change in plain words, the diff, the signed
sandbox test (stages), the signed dry run (checks, 0 real model calls), the build fingerprint
(sha256, grouped in 4s), the base commit, and a 30-minute one-time code with a live countdown.
**Approve** signs on Slavik's device (passkey preferred; Ed25519 key file as fallback, a
non-extractable WebCrypto key in IndexedDB). **Reject** records a note and withdraws any earlier
approval of that request.

The console checks everything before it offers a code: stack signature bound to proposal + patch,
test record and dry-run record signatures (sandbox-v2 key) and bindings, every signed log
re-hashed, `test/artifact` re-hashed. It then writes `<entry>/owner/approval.json` in FORMATS
section 4 field order. It never deploys: `galion-sm-deploy` (root) does that.

## Admin key
1. "Create a passkey on this device" (or "Create a key file instead" / "Use a key file I have").
   Only the public half goes to the server (passkey: SPKI + credential id + clientData of the
   create call; key file: the public key + a signature over the setup code).
2. The screen shows the device's fingerprint next to the one the server received. Slavik ticks
   "I compared them and they are the same" and taps Activate: the NEW key signs
   `galion-one/owner-key-activate/v1` (with `confirmed_match: true`). Replacing a key also needs
   the CURRENT key's signature over `galion-one/owner-key-change/v1`, and the paired cookie.
3. The console stages `install-request.json` + `owner.pub.staged` under its state dir. It never
   writes `/etc/hermes-self-modify/owner.pub`. Root runs:
   `galion-owner-key install --request /var/lib/galion-one/approval/owner-key/install-request.json --dry-run`
   then the same without `--dry-run`. The helper re-checks the signatures (activation within 24 h,
   current-key signature when replacing), refuses `/var/lib/galion-gate/*` and symlinks, writes
   root:root 0444 atomically, keeps the old key as `owner.pub.off-<unix>`.

## Endpoints (all paired-only; every POST needs Origin in ONE_WEBAUTHN_ORIGINS)
| method | path | does |
|---|---|---|
| GET | /approvals | the page (no data in it) |
| GET | /api/approvals | cards + admin key state |
| GET | /api/approvals/count | `{"ready":N}` |
| POST | /api/approvals/challenge `{id}` | one-time code `{nonce, body, webauthn_challenge, expires}` (30 min; same code reused while > 60 s left) |
| POST | /api/approvals/approve `{nonce, assertion}` | verifies, consumes the code, writes owner/approval.json |
| POST | /api/approvals/reject `{id, note}` | writes owner/rejected.json, withdraws approval.json |
| GET | /api/owner-key | none / pending / staged / active, fingerprints, install command |
| POST | /api/owner-key/begin `{scheme}` | setup code |
| POST | /api/owner-key/register | public half -> pending |
| POST | /api/owner-key/activate-challenge | bodies to sign (new key, and current key if replacing) |
| POST | /api/owner-key/activate `{nonce, confirmed, activate, change}` | stages the install request |

Codes are consumed by rename (`challenges-used/`, `owner-key/used/`), never deleted.

## Settings (env, defaults are the server paths)
`ONE_APPROVAL_QUEUE` /var/lib/openmind-hermes/queue · `ONE_APPROVAL_STATE` /var/lib/galion-one/approval ·
`ONE_APPROVAL_OUT` (optional, else `<entry>/owner/`) · `ONE_OWNER_PUB` /etc/hermes-self-modify/owner.pub ·
`ONE_SANDBOX_PUB` /etc/hermes-self-modify/sandbox-v2.pub · `ONE_STACK_PUB` /etc/hermes-self-modify/stack.pub ·
`ONE_WEBAUTHN_RP_ID` galion.app · `ONE_WEBAUTHN_ORIGINS` https://galion.app,https://one.galion.app.
