<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# console.galion.app 503 diagnosis (wave 4 slot 396, F-384-1)

Branch `swarm/console-admin-503-diag` off `console-realtests-rust-routes` ab0b7d04.
Doc only. All times Europe/Zagreb (CET, +01:00), 2026-10-28. Box ports 22660-22669.
VPS read-only (systemctl show/is-active, ss, journalctl, loopback GETs); nothing
deployed, no DNS/tunnel/service change, no secrets read.

## Answer in one paragraph

Every room on console.galion.app (/console, /node, /hermes, /bank, /memory,
/md/user, /status ...) answers **503 "Console API is not answering"**, for
everyone, cookie or not. The 503 comes from the Rust page gate
(`crates/console-ui/src/lib.rs` `gate()`, `Decision::AskUpstream`): for any
non-loopback console host it asks its upstream `GET /api/v1/admin` and treats
any non-200 as "upstream down" -> 503 (fail-closed). The live binary
`galion-console-serve-2d846fb8` runs `--dist ... --local-api` with
self-upstream 127.0.0.1:8080; its in-process local API (`console-static/src/api.rs`
on swarm/console-rust-serve-impl) only answers /api/v1/health, /session,
/session-pack and /ping, so /api/v1/admin falls through to a static **404**.
The API is answering; it just has no admin route. This started at the Rust
cutover, 2026-10-27 19:28:35 CET (console.service ActiveEnter), ~21 h ago.
Expected owner of the fix: the staged console deploy candidate **5aa31927**
(swarm/console-deploy-candidate 4f763ade) with `--admin-store`/`--admin-mirror`,
which is waiting on owner decision **D-343-1**. Fail-closed means nothing is
exposed; the cost is that Slavik cannot use the public console at all.

## Who answers (chain)

| Hop | What | Evidence (read-only) |
|---|---|---|
| Cloudflare edge | `server: cloudflare`, cf-cache-status DYNAMIC (not cached, not a CF error page) | public curl 16:15 |
| cloudflared.service | active; `tunnel --config /opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml run`; ingress `console.galion.app -> http://127.0.0.1:8080`; catch-all `http_status:404` | systemctl show, grep hostname/service lines only, 16:16 |
| console.service (Rust FALLBACK) | active, PID 240630, `/opt/galion/rust/bin/galion-console-serve-2d846fb8 --bind 127.0.0.1:8080 --dist /opt/galion/console-dist --ui rust --local-api --pulse 127.0.0.1:8091 --desk 127.0.0.1:8646 --quiet`, no drop-ins | systemctl show 16:16 |
| startup log | `listening 127.0.0.1:8080 ui rust dist true local_api true ... pages 31 upstream 127.0.0.1:808[0]` (self-upstream) | journalctl -u console.service |
| console-rust-primary.service | active, PID 240579, same argv on :18080 (not tunnelled), same behaviour | systemctl show, loopback GETs |
| binary | VPS sha256 516556f9...0795041 == box artifacts/t291u copy (byte-identical) | sha256 files |
| `galion-console-serve-5aa31927` | absent on the VPS (candidate not installed) | ls /opt/galion/rust/bin |

## Public probes (16:15:29-16:15:41 CET)

| Path | Status | Body |
|---|---|---|
| / | 302 -> /console | gate RedirectConsole |
| /api/v1/health | 200 192 B | `galion-console-static` 0.1.0, rust, local-api |
| /api/v1/session | 401 | `{"error":"phone required"}` |
| /api/v1/admin | 404 10 B | `Not Found` |
| /pair, /console/, /desk | 404 | (not a Rust page / not proxied) |
| /console /node /hermes /bank /memory /md/user /status | **503 1792 B** | "Admin - Console API is not answering" (854 B page + Cloudflare challenge script) |
| console.galion.studio, dashboard.galion.studio | curl TLS error (exit 35) | not routed through this tunnel; out of scope |

## VPS loopback probes (16:16 CET)

| Port | Host header | /api/v1/admin | /api/v1/health | /console |
|---|---|---|---|---|
| 8080 | console.galion.app | 404 | 200 | 503 |
| 18080 | console.galion.app | 404 | 200 | 503 |
| 8080 | 127.0.0.1 (loopback peer) | - | - | **200** |

So the pages themselves render; only the admin gate blocks them.

## Box reproduction with the byte-identical live binary (16:17:44 CET)

`artifacts/t291u/galion-console-serve-2d846fb8 --bind 127.0.0.1:22660 --dist <copy of console-dist static> --ui rust --local-api --pulse 127.0.0.1:22668 --desk 127.0.0.1:22669`
(startup line: `upstream 127.0.0.1:22660 prefixes []`, i.e. self-upstream, no proxy prefixes).

| Host | / | /api/v1/health | /api/v1/admin | /console | /node | /hermes | /status | /api/v1/session |
|---|---|---|---|---|---|---|---|---|
| console.galion.app | 302 | 200 | 404 | 503 | 503 | 503 | 503 | 401 |
| same + `Cookie: galion_admin=fake` | | | | 503 | | | | |
| 127.0.0.1:22660 (loopback peer) | 302 | 200 | 404 | 200 31675 B | 200 | 200 | 200 | 401 |

Same answers as live. A cookie does not help: the gate never gets a 200 to
check `{"you":true}`. Process killed, port range free, temp dir removed.

## Why the message is misleading

`gate()` maps every non-200 (`_ =>`) to `pages::gate::unavailable()`. Here the
upstream answered quickly with 404, which means "this API has no admin
route", not "not answering". Fail-closed is right; the wording sends people
looking for a dead process that is healthy.

## Monitoring gap

Nerve `:8670/state` (16:16 CET) reports console (:8080) **healthy** since
2026-10-27 20:30:01 CET, rss 744 kB, restarts_1h 0, while 100 % of public
rooms are 503. The deep check that would flip it to degrading (admin + board
404 = blind console) is on swarm/nerve-console-blindspot-test 607e44fe
(slot 332, F-313-2) and is not deployed. Cortex not consulted (nerve used).

## Expected owner and fix path

* Fix owner: console deploy candidate 5aa31927 (swarm/console-deploy-candidate
  4f763ade = console-v2-admin-fold c7cebb28 + console-dist-arena-bank 7888d8fb;
  carries --admin-store from slot 313 49b31b3f and the F-313-1 lockout fix
  dfa120c6). Staged drop-ins
  `deploy/console-rust-serve/candidate/{console,console-rust-primary}.service.d/60-admin-store.conf`
  add `--admin-store /root/galion-secrets/admin-device.json`,
  `--admin-mirror .../persist/admin-device.json`, `--arena-root /workspace/arena`
  and `ReadWritePaths` for those two store dirs.
* Slot 343 evidence: with the store the gate answers 200 and shows the pairing
  page or the room (real_admin_flags_stage, real_admin_lockout,
  real_pairing_e2e all 1/1 on the box).
* Not checked here (secrets dir, read ban): whether
  /root/galion-secrets/admin-device.json already holds a paired device. If it
  does not, the first visit after the deploy shows the pairing page (fine).

## Owner decisions (proposal; nothing decided here)

* **D-396-1 (the fix): approve D-343-1** - install `galion-console-serve-5aa31927`
  + the two candidate drop-ins, primary :18080 first, check
  `GET /api/v1/admin` 200 and `/console` pairing page with Host
  console.galion.app on :18080, then fallback :8080 after a soak. Rollback:
  remove drop-in, daemon-reload, restart (back to 2d846fb8 argv).
  Options: **A (recommended)** deploy candidate as above; **B** keep 503
  (fail-closed) until the next planned console deploy, and use the console only
  over `ssh -L 8080:127.0.0.1:8080` (loopback renders, verified 200); **C**
  add a minimal `/api/v1/admin` to `--local-api` - rejected: duplicates the
  candidate's admin store code and still needs a live unit change.
* **D-396-2: deploy the nerve console blindspot check** (607e44fe, slot 332)
  so a blind console shows as degrading, not healthy. Recommend yes, with the
  next nerve deploy (same window as D-393-1 stamp).
* **D-396-3: gate wording** - split the `_ =>` arm: 404/405 from upstream ->
  "Admin sign-in is not set up on this console" (still 503, fail-closed);
  connect error/5xx -> keep "Console API is not answering". Small console-ui
  change for a later slot on top of 5aa31927. Recommend yes.

## Findings

* **F-396-1:** public console is fully unusable (503 on all rooms) since
  2026-10-27 19:28:35 CET; root cause self-upstream local-api lacks
  /api/v1/admin; reproduced byte-identically on the box.
* **F-396-2:** nerve reports the console healthy throughout (blindspot not live).
* **F-396-3:** the 503 text blames a dead API; the API answered 404.
* **F-396-4:** console.galion.studio / dashboard.galion.studio fail TLS at the
  edge (curl exit 35); not in this tunnel's ingress as seen; separate item if
  they are meant to be live.
