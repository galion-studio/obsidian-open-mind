<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# console-port-5: `/node`, `/security`, `/session` in Rust

Wave 4 slot 139, branch `swarm/console-port-5`, base `landing-overview` 669dc18. Written 2026-10-21 05:50 CEST (+02:00).

## Which routes
The 17 console routes still in TypeScript at 669dc18, alphabetical, split 4/4/3/3/3:
analytics, arena, bank, cache | context, gateway, guardian, hermes | instances, keys, markers | md.$name, memory, mesh |
**node, security, session**. This branch ports the last fifth. `PORTED` goes from 13 to 16 on this branch (+ `/` = 17 of 31
routes in Rust); all five console-port branches together cover the 17.

Source: the live console-ide tree read 2026-10-20 21:25 CEST (`/workspace/swarm/artifacts/console-ui/live-src-20261020-2125`):
`routes/_console/{node,security,session}.tsx`, `routes/api/v1/{security,session,session-pack}.ts`, `lib/session-docs.ts`,
`lib/runpod-start.ts`, `lib/machine.ts`, `lib/fleet.ts` (`sshCommand`).

## What each room does now (no JavaScript, CSP unchanged, no new CSS)

| room | data (existing loopback client) | writes upstream | differences from the TSX |
|---|---|---|---|
| `/node` (Pod) | `GET /api/v1/health` `.target` (public): `sshDirect`, `sshPort`, `publicIp` | none | SSH line and Last IP come from the live target instead of the bundle's `MACHINE` constant (same object); odd values (not user@host / IP / port 1-65535) are not shown; Copy buttons replaced by selectable `<pre>` |
| `/security` (Defence, "The fort") | `GET /api/v1/security`, `/api/v3/guardian/status`, `/api/v3/guardian/incidents` (admin cookie forwarded), in parallel | `POST /api/v3/guardian/scan {}`; `POST /api/v3/guardian/heal {}` with `X-Galion-Pin` (1-64 visible ASCII, from a password field, never rendered/logged); both only from a same-origin-checked form whose `act` picks one of the two fixed calls | **Denylist switch read-only** (decision 1); each source that does not load says why (live unpaired: `HTTP 401 (phone required)`) and its values show "—" instead of the TSX defaults (which read as "Denylist on, score 0, None open"); last scan on the Zagreb clock (`05:30:05 CEST`), TSX printed UTC; Refresh link instead of the 5 s poll |
| `/session` (Build session) | `GET /api/v1/session` (list), `GET /api/v1/session?name=` (markdown) | none | a file opens with a link `/session?name=…`; the name must match the server's own `safeName` rule (`^[A-Za-z0-9][A-Za-z0-9._-]*\.md$`) or the page answers 400 and nothing is asked upstream; a refused list says why instead of "0 files"; file text cut at 200 000 chars with a note; "Download zip" stays `/api/v1/session-pack` through the `/api/` proxy |

Code: `crates/console-ui/src/port5.rs` (handlers), `pages/{node,security,session}.rs`, `client::call_with` (identical
to slot 136 0916684, so the merge is clean). Shared hunks kept small: `lib.rs` (`PORTED`, `mod port5`, route arm,
`post_ok`, class test), `pages/mod.rs`, `tests/ui.rs` (fake routes, not-ported 16 -> 13).

## Tests
- Unit (console-ui, 35 total, 9 new): node (TSX ssh line, odd targets refused), session (`safeName` incl. `../`, `%2F`,
  NUL, `.MD`; render/escape; failures), security (Zagreb clock both DST edges 2026-03-29 / 2026-10-25 and New Year;
  full fort; unpaired says why; PIN rules incl. CR/LF), every emitted class styled.
- End-to-end, fake upstream (`crates/console-static/tests/ui_port5.rs`, 9 tests): GET-only plain views, HEAD, 405 on
  POST to `/node`/`/session`; Scan/Heal CSRF (no proof / cross-site / foreign Origin -> 403, nothing upstream), extra
  form fields ignored, unknown actions incl. a denylist try -> 400 nothing sent, never `/api/hermes/*`; PIN only in
  `X-Galion-Pin`, never in the page or body, injected/empty PIN -> 400 nothing sent; session list/doc/escape, 7 bad
  names never reach the API, zip through the proxy; refusals stated; only the admin cookie forwarded; gate (unpaired
  public host -> pairing, no reads, unpaired POST sends nothing; other host -> Soon; paired works); classes styled.
- Real-life against the live VPS, read-only (`tests/real_port5.rs`, `GALION_REAL=1`, ports 20090-20094): see
  `docs/real-tests/console-port-5.md`.

## Owner decisions (Slavik)
1. **Denylist switch on `/security`** (`POST /api/hermes/guard {"on":…}` turns Hermes's command denylist off/on;
   live state unknown from the box, unpaired): port it as a CSRF-checked form — yes / no / only "turn on". Same family
   as console-port-2 decision 2 (Hermes switches).
2. **`/node` content is RunPod-era**: it tells you to flash the pod with `curl … cpu-light/galion-light/flash.sh | bash`
   and shows `root@<VPS IP>` SSH. Ported 1:1 for parity; keep, rewrite for the VPS, or drop the room.
3. **Refresh** for `/security` (TSX polled every 5 s): Refresh link now; `<meta http-equiv=refresh>` or a script
   island per the JS ruling (decision 1 of `docs/console-ui-decision.md`).
4. **Merge note**: slots 135-139 all touch `PORTED`, the `handle` match, `post_ok` and `pages/mod.rs`; slot 136 adds
   `port2::POST_ROUTES`, this branch `port5::POST_ROUTES` — keep both in `post_ok`. `client::call_with` is byte-identical.
