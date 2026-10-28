<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# omnode pairing + Hermes enrollment (swarm/omnode-enroll)

- Branch `swarm/omnode-enroll`, based on **integration-5 0d68f46** (holds hermes-gateway, the
  hermes routes and nerve; its hermes code is the code of the live aarch64 build `aa77864`),
  with `swarm/hermes-enroll-2` c28374e merged in (b2ebc89) and built on top.
- Written 2026-10-21 17:30 +02:00. Local only. **Nothing deployed; the VPS was not changed.**
- Design: `docs/phone-instance.md` §6 (pairing), `docs/hermes-enrollment.md` (design),
  `docs/hermes-enroll.md` (first server cut). Real test: `docs/real-tests/omnode-enroll.md`.
  Cutover (not approved): `docs/cutover/omnode-enroll.md`.

## 1. What it does, end to end

```
phone (Termux)                           Hermes (hermes-gateway, hermes-rs)            owner (Hermes host)
omnode pair https://H/openmind/enroll
  key: load ~/.config/openmind/node.key
       (create it if missing, mode 600)
  sign payload v1 (600 s, 96-bit nonce)
  POST /openmind/enroll  ───────────────▶ verify sig, expiry, skew, nonce
                                          store nonce, then pending + CODE  ── log line "pending ... code XXXX-XXXX"
  print QR + CODE + fingerprint  ◀── 202 {code, pending_expires, poll_secs}
  every 3 s: signed POST /openmind/enroll/status                              hermes-gateway enroll pending
                                          pending | approved | rejected(403)   hermes-gateway enroll approve CODE
                                          | expired(410)                       (or: reject CODE)
  POST /openmind/enroll/claim (signed) ─▶ one-time scoped credential
  store hermes-credential.json (600)
  GET /openmind/node/tunnel (Bearer) ───▶ SSE stream held open (phone dials OUT)
     hello{challenge} ◀──                                                     hermes-gateway enroll ping DEVICE
  POST /openmind/node/tunnel/answer ───▶ verify device-key signature → "verified"
     ping{challenge} ◀── every 15 s; answer signed; Hermes records rtt
```

The phone never listens on any port (tested: zero listening sockets under qemu).

## 2. Server (`crates/hermes-rs/src/enroll/`)

Off unless `HERMES_RS_ENROLL=1` (unchanged from hermes-enroll-2; routes return 404 when off).

| Route | Auth | What |
|---|---|---|
| `POST /openmind/enroll` | device signature | check signature (Ed25519 over the canonical string, recomputed by Hermes), expiry (600 s life, 120 s skew), nonce reuse (409; nonce persisted *before* the answer), URL binding; then `202` pending with a short CODE |
| `POST /openmind/enroll/status` | device signature over `omnode-status-v1\|dev\|ts\|url` | `pending` / `approved` (200) / `rejected` (403) / `expired` (410). Own rate bucket (240/min). Never returns a token |
| `POST /openmind/enroll/claim` | device signature | after approval: one-time scoped credential (30 days) |
| `GET /openmind/node/tunnel` | Bearer credential | SSE stream: `hello`, `ready`, `challenge`, `ping`, `bye` |
| `POST /openmind/node/tunnel/answer` | Bearer + device signature over `omnode-tunnel-v1\|dev\|session\|id\|challenge\|url` | proves the key; a bad hello answer closes the tunnel |
| owner `GET /openmind/enroll/pending`, `GET /openmind/enroll/devices` | `API_SERVER_KEY` | pending list with code, waiting time, timeout, recent refusals/timeouts; devices with tunnel state |
| owner `POST /openmind/enroll/approve`, `/reject` | `API_SERVER_KEY` | `{"code":"XXXX-XXXX"}` or `{"device_id","fingerprint"}`; code is case-insensitive, O→0, I/L→1 |
| owner `POST /openmind/enroll/ping`, `/revoke` | `API_SERVER_KEY` | challenge a live tunnel (rtt); revoke closes it with `bye` |

* **Approval:** a request stays *pending* until the owner approves. Refusal is recorded as a
  `rejected` outcome and told to the device (exit 4). A request nobody answers times out after
  `HERMES_RS_ENROLL_PENDING_TTL` (now **900 s**, was 3600) and becomes `expired` (exit 5).
  Outcomes are kept 24 h (max 200), so the device learns what happened.
* **Short code:** Crockford base32 of the first 40 bits of
  `SHA-256("omnode-code-v1|<pk hex>|<nonce hex>")`, shown `XXXX-XXXX`. Computed the same way
  on both sides (pinned vector: pk `11`×32, nonce `22`×12 → `J820-7XVQ`). The owner should also
  compare the fingerprint; the code is a convenience, not a secret.
* **Persistence:** the existing enroll store (`$HERMES_HOME/rs-enroll.json`, mode 600, atomic
  rename, hand-written codec). New optional fields `outcomes` and pending `code`; stores
  written by hermes-enroll-2 still open (tested).
* **Tunnel limits:** `HERMES_RS_ENROLL_TUNNELS` (default 8, max 32; the gateway has a
  64-connection cap), `HERMES_RS_ENROLL_TUNNEL_PING` (15 s), `HERMES_RS_ENROLL_TUNNEL_LIFE`
  (3600 s, then `bye` and the device reconnects). A device that does not answer a challenge
  within 30 s is dropped. A second tunnel for the same device replaces the first.
* **Owner CLI** (`crates/hermes-gateway/src/owner.rs`): on the Hermes host,
  `hermes-gateway enroll pending | devices | approve CODE [--scopes a,b] | reject CODE | ping DEVICE | revoke DEVICE`.
  It calls the running gateway on loopback with `$API_SERVER_KEY` from the environment (never
  printed). Exit 0 ok, 1 refused, 2 usage / no key.

## 3. Client (`edge/openmind-node`, binary `omnode`)

`omnode pair <url>` now sends and waits (old print-only behaviour: `--offline`).

| Flag | Meaning |
|---|---|
| `--wait SECS` | stop waiting earlier than the server timeout |
| `--poll-ms N` | status poll interval (default 3000) |
| `--no-tunnel` | stop after the credential is stored |
| `--tunnel-secs N` | run the tunnel N seconds then exit (tests) |
| `--insecure-http` | allow plain http to a non-loopback host (own LAN only) |
| `--via-curl` | use system curl even for http (tests the phone code path) |
| `--dry-run`, `--offline`, `--verbose` | print the plan / print only / log tunnel frames |

`omnode tunnel` re-opens the tunnel later from the stored credential (reconnect backoff
1→60 s, 75 s idle limit). Exit codes: 0 ok, 2 no/invalid credential, 4 refused or revoked,
5 timed out, 6 tunnel refused.

**TLS:** `https://` goes through the system `curl` (there is no pure-Rust TLS stack that
builds without asm). Bearer, headers and body go to curl via `-K -` on stdin, never argv.
Plain http is only for loopback or `--insecure-http`.

## 4. On the phone (Redmi Note 9 Pro "joyeuse", SD720G, aarch64, Termux)

Binary: static `aarch64-unknown-linux-musl` (1,342,808 bytes in the e2e build; runs on
Android because it needs no libc). `aarch64-linux-android` passes `cargo check`; there is no
NDK on the build box, so no bionic binary was linked.

Install Termux **from F-Droid** (the Play Store build is outdated), then in Termux:

```sh
pkg update && pkg upgrade -y
pkg install -y curl            # https transport; required
pkg install -y termux-api      # optional: Wi-Fi identity (+ Termux:API app, Location permission)
termux-setup-storage           # once, to reach ~/storage/downloads
```

Get the binary (one of):

```sh
# a) copied from a computer: adb push omnode-aarch64 /sdcard/Download/omnode
cp ~/storage/downloads/omnode ~/omnode
# b) build on the phone (slow, ~1 GB space): 
pkg install -y rust git && git clone <repo> && cd obsidian-open-mind/edge/openmind-node \
  && cargo build --release -j 2 && cp target/release/omnode ~/omnode
```

Check and run:

```sh
chmod 700 ~/omnode
sha256sum ~/omnode             # compare with the sum published next to the binary
~/omnode identity show || ~/omnode identity init
termux-wake-lock               # keep the tunnel alive with the screen off (optional)
~/omnode pair https://<hermes-host>/openmind/enroll
```

What the phone shows: the device key line, a terminal QR (signed short form, ~250 chars),
then

```
  CODE         H1WN-6XNQ
  device       omn-949aa3458bb3
  fingerprint  SHA256:949a-a345-8bb3-a99c-17a5-e464-0d9b-5da5
  approve by   2026-10-21T17:35:01+02:00

The owner approves on the Hermes host with:  hermes-gateway enroll approve H1WN-6XNQ
(or refuses with: hermes-gateway enroll reject H1WN-6XNQ). Waiting for the owner...
  2026-10-21T17:20:02+02:00 status: approved (HTTP 200)
approved: scopes ["cpu","sandbox","voice-client"] until 2026-11-20T16:20:02+01:00; credential stored in ~/.config/openmind/hermes-credential.json (mode 600, never printed)
2026-10-21T17:20:02+02:00 tunnel: UP: Hermes verified the device key (session 676ee56e6bd3fcd4); outbound only, nothing listens on this device
```

What the owner does (Hermes host, `API_SERVER_KEY` in the environment):

```sh
hermes-gateway enroll pending
#   CODE H1WN-6XNQ  omn-949aa3458bb3  SHA256:949a-...  android-termux 8 cores ... waiting 12 s (times out ...)
# compare CODE and fingerprint with the phone screen, then
hermes-gateway enroll approve H1WN-6XNQ      # or: reject H1WN-6XNQ
hermes-gateway enroll devices                # tunnel connected/verified, last ping
hermes-gateway enroll ping omn-949aa3458bb3  # rtt through the tunnel
```

Later runs (after reboot): `~/omnode tunnel`. Termux:Boot could start it; not set up here.

## 5. Deviations from the design

* Tunnel is **SSE over HTTPS** (long GET + signed POST answers), not an SSH reverse tunnel
  or WSS: it reuses the gateway's HTTP server and the Cloudflare hostname, needs no new
  dependency, and the phone needs only curl. It carries control frames (challenge/ping)
  today, not arbitrary traffic. Owner decision item D2.
* Pending timeout lowered from 3600 s to 900 s (the user is standing at the phone). D1.
* A refused device may send a new request (new nonce); a revoked device may not. D5.
* No Provider row / network salt on approval yet (design §5, optional).

## 6. Decision items for Slavik (not decided here)

| # | Question | Default in code |
|---|---|---|
| D1 | Pending timeout | 900 s |
| D2 | SSE-over-curl tunnel vs WSS / SSH reverse tunnel | SSE |
| D3 | Which public hostname reaches Hermes `/openmind/*` (today no ingress points to :8642) | none |
| D4 | hermes-gateway size budget (see real-test record) | 2 MiB |
| D5 | May a refused device ask again? | yes |
| D6 | How `API_SERVER_KEY` reaches the owner's shell for the CLI | env var only |
| D7 | How the phone gets the binary (no release host approved) | adb / Downloads |
| D8 | Run the tunnel permanently (termux-wake-lock, Termux:Boot) | manual |
