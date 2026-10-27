<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Android admin console: plan (overnight goal step 2)

Written 2026-10-27 21:15 CET (+01:00), wave-4 slot 307, branch `swarm/android-admin-plan`
(base `swarm/main-staging` 98466ab9). Box work plus read-only VPS probes. **No device was
touched, flashed or connected (§11 handset gate is still closed).**

**In one line:** the phone becomes the admin's remote by being a **browser client** of the
Rust console. Pairing, the device token and a phone-sized status page live in one small Rust
crate, `galion-admin-console`, that the live console can sit in front of. Galion OS (the
on-device sandbox, gold image, own root path) stays parked behind §11.

---

## 1. What the history says (sources read)

| When (Zagreb) | Source | What it decided or built | What we keep |
|---|---|---|---|
| 2026-10-20 23:05 CEST | Galion OS master plan (`/workspace/swarm/plans/galion-os.md`, staged repo `galion-os-repo` 2b55908) | Admin interface = coexisting ARM sandbox beside stock Android; L0 root, L1 `galion-admin` CLI, L2 gold image, L3 proot/chroot sandbox, L4 admin bridge (tunnel + adb). Secure / private / anonymous / coexist are first-class. §4 handset gate and §11 acceptance block every build on a real phone. | The four goals, node ids instead of device ids, hash-chained audit, "phone never flashed without Slavik". |
| 2026-10-21 06:38 CEST | slot 143 `swarm/galion-os-threat-model` f76eb96 | Threat model F1-F11, controls C-01..C-10, owner decisions D-OS1..D-OS7 (D-OS2: raw adb only owner-present; D-OS4: audit retention 90 d phone / 1 y server). | Pairing must be short-lived and mutual; nothing unattended over adb. |
| 2026-10-21 06:52 CEST | slot 144 `swarm/galion-os-sandbox-proto` 4374605 | `galion-box`: rootless userns sandbox launcher (default-deny net, caps 0, rlimits); proven on the VPS as a temp user. qemu-user cannot create namespaces. | Parked with Galion OS (on-device sandbox needs §11). |
| 2026-10-21 08:29 CEST | slot 164 `swarm/galion-os-box-escape` 7c7644e | Escape families all contained; open D-S3 seccomp, D-E1 AF_VSOCK, D-E2 nested userns. | Parked. |
| 2026-10-21 08:35 CEST | slot 165 `swarm/galion-os-admin-cli` bf33cba | `galion-admin` CLI: sandbox start/stop/status, loopback tunnel check, **hash-chained 0600 audit log** with Zagreb stamps, refuses root. | **Audit chain design reused** in `galion-admin-console::audit` (same scheme, simplified). |
| 2026-10-21 18:27 CEST | slot 204 `swarm/galion-os-image-format` f35f735 | Signed gold-image manifest (Ed25519, hades-gate key format). | Parked (device image). |
| 2026-10-27 17:29 CET | parent `slavik-hermes-android-admin` | Phone → galion.app → Hermes worked on the **Node srvx** console: `/api/v1/admin` 200 `paired:true`, `/api/v1/session-pack` 401 without a device. | The working contract to restore. |
| 2026-10-27 17:39 CET | slot 265 `swarm/console-session-pack` 21fd3ba | Rust console-ui: `galion_device` cookie, silent device resume, session-pack gate. | Cookie hygiene (HttpOnly, Secure, SameSite). |
| 2026-10-27 18:11 CET | Mate t272u inventory | **Phone = client only, OS-agnostic**; drop the Android-host / Galion-OS-flash path for the admin link; 1:1 phone ↔ instance over HTTPS console :8080 with pairing cookies. | The architecture of this plan. |
| 2026-10-27 ~19:4x CET | overnight goal (t313u/t314u) | After the Rust stack: "begin Android admin console for the device admin runs, from Galion OS history (galion-box, galion-admin, handset gate, §11 no flash)". | This document + skeleton. |

All five `swarm/galion-os-*` heads are on GitHub (galion-studio/obsidian-open-mind) since
2026-10-27 17:44 CET (push-agent); the separate `galion-studio/galion-os` repo still does not exist.

## 2. Live finding (read-only probe, 2026-10-27 21:02 CET)

The Rust console cutover (`galion-console-serve-2d846fb8 --ui rust --local-api`, live on
127.0.0.1:8080) **dropped the admin pairing API** the phone relied on:

| GET on 127.0.0.1:8080, Host `console.galion.app` | Node srvx 17:29 CET | Rust now 21:02 CET |
|---|---|---|
| `/console` | 200 (paired) | **503** "Console API is not answering — the pairing check runs on the console API" |
| `/api/v1/admin` | 200 `paired:true` | **404** |
| `/admin` | 200 | **404** (`/pair` also 404; not probed on srvx) |
| `/session` | 200 | **503** |
| `/api/v1/session-pack` | 401 phone required | 401 `{"error":"phone required"}` (same) |
| `/api/v1/health` | 401 phone required | 200 `galion-console-static`, `implementation: rust`, `mode: local-api` |

Cause: console-ui's gate asks its upstream `GET /api/v1/admin` for `{"you":true}`, but the
`--local-api` upstream in console-static only answers `health`, `session`, `session-pack`,
`ping` (`crates/console-static/src/api.rs` on `swarm/console-rust-serve-impl`). So every
console page on a public host fails closed with 503, and **no phone can pair right now**.
Fail-closed is the safe direction (nothing leaks), but the Android admin path is down.
`galion-admin-console` implements exactly the missing contract (section 4), so it is the fix
path; wiring it into the live console is a deploy step for a later slot (section 6, P1).

The nerve (127.0.0.1:8670, cycle 19, 21:00:01 CET) still lists `console` as healthy because it
only checks the port; it does not see the pairing break. Proposal N-307-1: a nerve probe of
`GET /console` on a console host that treats 503 as degrading.

## 3. Architecture

```
 phone browser (any OS; no app, no root, no adb)
     │ https://console.galion.app  (Cloudflare)
     ▼
 galion-console-serve :8080 (Rust, live)
     │  console-ui gate: GET /api/v1/admin  -> {"you":bool}
     │  /_ui/pair form   : POST /api/v1/admin {"code":..} -> Set-Cookie galion_admin
     │  /admin, /api/v1/admin/*  (phone page + status)            ◄── P1 wiring
     ▼
 galion-admin-console 127.0.0.1:<port>   (new crate; or linked into console-static, D-307-1)
     │  state dir 0700: codes (hash) · devices (token hash, node id) · fails · audit.jsonl (chain)
     ├─ GET 127.0.0.1:8670/state       (nerve, read-only, summarised: names, counts, ids, grade)
     └─ GET 127.0.0.1:8080/api/v1/health (console, read-only)
 operator on the VPS terminal:  galion-admin-console issue-code | devices | revoke NODE | audit-verify
```

Goals from the Galion OS plan, applied to the client-only design:

| Goal | How the skeleton meets it |
|---|---|
| Secure | One-time 8-char codes (40 bits, Crockford base32), 5 min TTL, single use, stored as SHA-256 only; 5 failures in 15 min lock pairing (even a right code) → 429; device token 256-bit, stored as SHA-256 only, `HttpOnly; Secure; SameSite=Strict`; POST needs a matching `Origin` (or same-origin fetch metadata); loopback-only bind and upstreams (refused otherwise, exit 2); 4 KiB body cap; CSP `default-src 'none'`, no JavaScript. |
| Private | Codes and tokens never on disk in clear and never in the audit log (tested); status page carries only component names, counts, incident ids/causes and the grade (no titles, hosts, ports, log text; tested on live data). |
| Anonymous | The device is `node-<8 hex>` derived from its token; no IMEI, serial, phone number, user agent or IP is stored. |
| Coexist | Nothing runs on the phone. Daily Android stays untouched; no app install, no root, no wakelock. |
| Audit | Hash-chained JSONL (galion-admin design), Zagreb `+01:00/+02:00` stamps, `audit-verify` exit 4 on tamper. |

## 4. The skeleton crate (`crates/galion-admin-console`)

| Route | Contract |
|---|---|
| `GET /api/v1/admin` | `{"you":bool,"paired":bool[,"node","label"]}` — console-ui gate contract |
| `POST /api/v1/admin` | JSON `{"code":".."[,"label"]}` → 200 + `Set-Cookie: galion_admin=…`; 401 refused; 429 locked (`retry-after: 900`) — console-ui `/_ui/pair` relay contract |
| `POST /api/v1/admin/pair` | same, as a plain HTML form (no JS) → 303 `/admin` or `/admin?e=bad|locked` |
| `POST /api/v1/admin/unpair` | paired only; revokes this device, clears the cookie |
| `GET /api/v1/admin/status` | paired only (else 401 `{"error":"phone required"}`): nerve + console summary |
| `GET /admin` | phone page: pair form, or grade/score, component counts, attention list, open incidents, console health, unpair button |
| `GET /healthz` | liveness |

CLI: `serve`, `issue-code`, `devices`, `revoke NODE`, `audit-verify`. Deps: `galion-common`
(in-repo std-only HTTP/1.1 server+client, SHA-256, base64, Zagreb time) and `serde_json`
(already in the workspace). No new third-party crate. Static musl sizes: x86_64 561 KiB,
aarch64 493 KiB.

## 5. Reuse research (MIT-compatible only)

Checked on crates.io 2026-10-27 ~21:05 CET (licence from the crate's published metadata).

| Need | Candidate | Licence | Verdict |
|---|---|---|---|
| HTTP server/client, SHA-256, base64, time | in-repo `galion-common` | MIT | **Used** (zero new deps). |
| Audit chain | galion-os `galion-admin` audit (slot 165) | MIT (ours) | **Design reused.** |
| QR code for pairing (show code as QR on a second screen) | `qrcode` 0.14.1 (kennytm/qrcode-rust) | MIT OR Apache-2.0 | **Next (P2)**, with `default-features = false` (no `image`), SVG output. |
| | `fast_qr` 0.14.0 | MIT | Alternative; has SIMD/wasm paths, so `qrcode` preferred (no SIMD rule). |
| TOTP second factor (D-307-4) | `totp-rs` 6.0.0 | MIT | Candidate if Slavik wants TOTP on top of the cookie; pulls hmac/sha1/sha2 (RustCrypto, MIT/Apache). |
| adb wire client (wireless debugging) | `adb_client` 3.2.3 (cocool97) | MIT | **Parked**: adb steering is the §11 / D-OS2 path (owner-present only). |
| Web Push alerts to the phone | `web-push` 0.11.0 | Apache-2.0 | Later; needs VAPID keys = a new secret (owner decision), and its crypto backend must be checked against the no-asm rule → prefer polling the status page or a hand-written SSE stream. |
| SSE framing | `sse-codec` 0.3.3 | **MPL-2.0** | **Not allowed** — write the ~30 lines of SSE ourselves if needed. |
| Screen mirroring | scrcpy | Apache-2.0 (app); its server ships into the phone over adb | Parked with §11 (device access). |
| Terminal on the phone | Termux | **GPL-3.0** | Not reused; not needed (client-only). |

## 6. Phases

| Phase | What | Gate |
|---|---|---|
| **P0** (this slot) | Plan, skeleton crate, box real test, live read-only nerve/console run | done |
| **P1** | Restore phone pairing on the live console: either (a) run `galion-admin-console` as a loopback unit and route `/api/v1/admin*` + `/admin` to it from console-serve, or (b) link the crate into console-static `--local-api`. Migrate the existing Node-era device store only if Slavik wants old phones to stay paired (otherwise re-pair with `issue-code`). | live deploy = auto-accept scope of the overnight loop, but its own slot with rollback |
| **P2** | QR pairing (`qrcode`), session list + revoke on the phone page, nerve probe N-307-1 | — |
| **P3** | Hermes on the phone: read-only Hermes status card, then owner-confirmed actions through HADES approval (never money moves) | HADES admin key (hermes-core G9) |
| **P4** | Galion OS on-device (galion-box sandbox, gold image, adb bridge) | **§11 answers 1 and 7 + handset gate** |

### P1 client (slot 319, done on swarm/android-admin-pairing)

`galion-admin-console client-status|client-pair|client-resume|client-get|client-forget`: Rust pairing
client for the `/api/v1/admin` contract of console-static `--admin-store` (303) and of `serve` (307).
Loopback only, code from stdin/file, creds 0600, node id output only. Real tests: box,
VPS throwaway as nobody, live read-only (still 404). See docs/real-tests/android-admin-pairing.md.
It is the tool to verify P1 the moment D-303-1/D-313-1 lands (`client-status` 404 -> `paired=..`).

## 7. Owner decisions

* **D-307-1** P1 shape: separate loopback unit (a) or linked into console-static (b)? Rec: (b) for one binary, (a) first if we want it live tonight with an easy rollback.
* **D-307-2** Keep or drop the Node-era paired devices (re-pair is one `issue-code`)? Rec: re-pair.
* **D-307-3** Ownership row: Console / HEPHAESTUS (pending Slavik, set) vs guardian-watchdog / MINERVA.
* **D-307-4** Second factor on top of the cookie (TOTP via `totp-rs`) for admin actions in P3? Rec: yes before any write action.
* **D-307-5** Pairing lock window 15 min / 5 failures (matches console-ui's "Wait 15 minutes" text). OK?

## 8. Not done / limits

* Not deployed; the live 503/404 pairing gap stays until P1.
* The audit chain is tamper-evident, not tamper-proof (same limit as galion-admin, D-A2).
* aarch64 unit tests did not run (no qemu on the box this time); aarch64 musl builds, android/armv7 clippy clean.
* `issue-code` prints the code to the operator's terminal once; that is the only clear copy.
