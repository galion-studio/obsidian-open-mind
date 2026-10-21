<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Hermes device enrollment: implementation (swarm/hermes-enroll)

Written 2026-10-20T20:55:00+02:00 (Europe/Zagreb). Branch `swarm/hermes-enroll`
(main 13a296ce + swarm/ucpu 3e70ffb + swarm/edge-boot 753676e, normal merges).
Implements the design in [docs/hermes-enrollment.md](hermes-enrollment.md),
including the nonce store that `pair::verify` does not have. Nothing was deployed;
the VPS was only probed read-only (GET).

Code: `crates/hermes-rs/src/enroll/{mod,verify,codec,tests}.rs`, wiring in
`crates/hermes-rs/src/api.rs` and `crates/hermes-gateway/src/main.rs`; device side
in `edge/openmind-node/src/enroll.rs` (+ `pair --now/--state`, `enroll` CLI).

## Flow

```
omnode (device, dials out)                 hermes-gateway                       owner
--------------------------                 --------------                       -----
enroll request URL  -- POST /openmind/enroll (signed pair doc) -->
                    <-- 202 {"status":"pending","fingerprint"} --  nonce stored first
                                                                   GET  /openmind/enroll/pending   (API key)
                                                                   POST /openmind/enroll/approve
                                                                        {device_id, fingerprint, scopes?}
enroll claim        -- POST /openmind/enroll/claim {device_id,nonce,ts,sig} -->
                    <-- 200 {"token":"omn1_...","scopes",...} --   only sha256(token) stored
enroll status       -- GET /openmind/node/whoami   (Bearer omn1_...) --> 200 / 401
                    -- GET /openmind/node/allowed?scope=S         --> 200 / 403 / 401
                                                                   POST /openmind/enroll/revoke {device_id}
```

* The claim signature is Ed25519 (strict) over
  `omnode-claim-v1|device_id|nonce|ts|enroll_url` with the key that was approved;
  `ts` must be within 120 s; `nonce` is 96-bit hex and goes into the same nonce
  store as pairing nonces (a pairing nonce can not be reused as a claim nonce).
* A claim by a device that is still pending gets `202 pending` (no token); unknown
  device `403 not enrolled`; bad signature `401`; revoked `403 device revoked`.
* Every successful claim issues a new credential and replaces the old one
  (rotation and recovery: a node that lost its credential claims again).
* Credentials expire (`HERMES_RS_ENROLL_CRED_TTL`, default 30 days).
* Node tokens work only on `/openmind/node/*`. They are not the gateway key: every
  other route still needs `API_SERVER_KEY`, and node routes never accept that key.

## Responses (design §6, plus the new routes)

| Case | HTTP | Body |
|---|---|---|
| enrollment off (default) | 404 | any `/openmind/*` path, with or without a key |
| check failed (verify §4 1-9) | 400 | `{"ok":false,"error":"<reason>"}` |
| replayed nonce (pairing or claim) | 409 | `{"ok":false,"error":"nonce already used"}` |
| accepted, waiting for owner | 202 | `{"ok":true,"status":"pending","device_id","fingerprint"}` |
| pending list full | 429 | `{"ok":false,"error":"too many pending"}` |
| rate limit | 429 | `{"ok":false,"error":"rate limited, try again in a minute"}` |
| device already enrolled / id bound to another key (flagged) | 409 | |
| device revoked (enroll or claim) | 403 | `{"ok":false,"error":"device revoked"}` |
| body > 16 KiB / not JSON content-type | 413 / 415 | |
| store write failed | 503 | nothing changed in memory or on disk |
| owner route without the gateway key | 401 | |
| approve: fingerprint differs from the pending request | 409 | |

## Configuration (env; off unless enabled)

| Variable | Default | Notes |
|---|---|---|
| `HERMES_RS_ENROLL` | off | `1` enables; anything other than 1/0/true/false/yes/no/on/off refuses to start |
| `HERMES_RS_ENROLL_URL` | (required) | the exact URL devices put in `enroll_url`; path must be `/openmind/enroll`; `http://` only for 127.0.0.1/localhost/::1 |
| `HERMES_RS_ENROLL_ALLOW_PUBLIC` | off | needed when `HERMES_RS_HOST` is not loopback |
| `HERMES_RS_ENROLL_STORE` | `$HERMES_HOME/rs-enroll.json` | mode 600, atomic writes |
| `HERMES_RS_ENROLL_MAX_PENDING` | 8 | design §3 `max_pending` |
| `HERMES_RS_ENROLL_NONCE_TTL` | 900 | design §3; must be >= 840 (600 s payload life + 2 x 120 s skew) |
| `HERMES_RS_ENROLL_PENDING_TTL` | 900 (was 3600; swarm/omnode-enroll) | how long an unapproved request waits for the owner; then it is recorded as expired |
| `HERMES_RS_ENROLL_CRED_TTL` | 2592000 | credential lifetime (30 days) |
| `HERMES_RS_ENROLL_RATE_PER_MIN` | 20 | public POSTs (enroll + claim) per minute, all sources together |
| `HERMES_RS_ENROLL_TUNNELS` | 8 (max 32) | open device tunnels (swarm/omnode-enroll, docs/omnode-enroll.md) |
| `HERMES_RS_ENROLL_TUNNEL_PING` | 15 (max 300) | seconds between signed ping challenges on a tunnel |
| `HERMES_RS_ENROLL_TUNNEL_LIFE` | 3600 (min 10) | tunnel session length; the device reconnects |

## Fail closed

* Off by default: the router answers 404 for every `/openmind/*` path.
* Enabled with no URL, a bad URL/path, plain http to a non-loopback host, a
  non-loopback bind without `ALLOW_PUBLIC`, a too-short nonce TTL, or a store that
  is unreadable, corrupt, has unknown fields, is not mode 600, or can not be
  written: hermes-gateway logs the reason and exits 2 before binding. A damaged
  store is never reset or rewritten.
* Every mutation is computed on a copy, written (temp file, fsync, rename) and only
  then made current. A failed write answers 503 and the token, if any, never leaves.
* Nonces are stored before the answer, also for refused-but-valid requests
  (already enrolled, revoked, pending full), and kept for the TTL.
* Owner routes need the gateway key **and** `key_ok()` (16+ chars); without a key
  configured they are closed, not open.
* Scopes: the subset of the device's signed capabilities that the owner grants
  (default: all of them), `[a-z0-9._-]{1,32}`, max 16. Anything containing
  money/revolut/pay/bank/wallet/card/spend is never granted and is reported in
  `denied`.
* Credentials and token hashes are never logged, listed or printed; the owner's
  device list shows status and dates only. omnode stores the credential in
  `hermes-credential.json` (mode 600) and never prints it.

## Deviations from the design (and why)

* Rate limit is global (all sources together), not per source IP: the request
  struct has no peer address, and behind the Cloudflare tunnel every request comes
  from 127.0.0.1 anyway (a per-IP limit would need a trusted `CF-Connecting-IP`).
* Config is env, not a `[openmind.enroll]` TOML block: Hermes reads YAML + env;
  the key names follow the design's.
* The check in `enroll/verify.rs` is a copy of omnode's `pair::verify` (same repo,
  MIT) without serde derive, so the two must be kept in step; the real-life test
  signs with the real omnode binary, so a drift shows up there.
* Same-`device_id`-different-key (design §4.9) is implemented and flagged, but it
  can only happen with a 48-bit hash collision, so no test covers it (it would
  need a constructed collision); the code path was reviewed only.
* Approval also needs the fingerprint in the request body: the owner confirms the
  value they compared with `omnode identity show`, so a mix-up of two pending
  devices can not approve the wrong one.

## Device CLI (omnode)

```
omnode enroll request URL [--state DIR] [--insecure-http]   # exit 3 = pending
omnode enroll claim [--state DIR] [--insecure-http]         # exit 0 = credential stored
omnode enroll status [--state DIR] [--scope S]               # exit 0 accepted, 2 refused
omnode pair URL --dry-run [--state DIR] [--now UNIX]          # raw document (tests)
```

Plain http only to a loopback Hermes (else `--insecure-http`); https goes through
the system `curl` with the bearer header on stdin (never argv).

## Size

`hermes-gateway` x86_64 musl release: 2,088,384 bytes (budget 2,097,152; was
1,921,312 at swarm/ucpu). aarch64 musl: 1,849,872. Ed25519 (`ed25519-dalek`,
`curve25519-dalek` incl. its runtime-detected AVX2 backend, `sha2`) and the enroll
code cost about 167 KB. The store codec is hand-written instead of serde derive to
stay under budget (derive cost ~28 KB). **Only 8,768 bytes of headroom are left**;
see open issues.

## Open issues

* **Size headroom** on x86_64 hermes-gateway is 8.8 KB. Merging other wave-2
  branches that grow hermes-gateway (for example hermes-tools) will probably push
  it over 2 MiB. Options for the owner: drop the curve25519 AVX2 backend with
  `--cfg curve25519_dalek_backend="serial"` (a rustflag, which UNIVERSAL-CPU.md
  currently forbids), move enrollment behind a cargo feature, or raise the budget.
* No owner console UI yet: approval is `curl` against the owner routes (or a
  future console page). The console must show the fingerprint for comparison.
* Two hermes-gateway processes on the same store would race (last writer wins);
  run one instance per store.
* No network-salt reply on approval (design §5, optional).
* Revoked devices stay revoked (re-enrolling the same key is refused). Forgetting
  a device needs a manual store edit today; a `forget` route was not added.
* Enabling it on the VPS needs Slavik's approval in STAGED.md and the Cloudflare
  hostname (design §2/§3). Nothing was enabled or deployed.
* With enrollment off, `/openmind/*` now answers 404 without a key; the live
  hermes-rs shadow (main) answers 401 (auth before routing). Design §6 asks for 404.
