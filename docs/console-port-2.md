<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# console-port-2: `/context`, `/gateway`, `/guardian`, `/hermes` in Rust

Wave 4 slot 136, branch `swarm/console-port-2`, base `landing-overview` 669dc18. Written 2026-10-21 05:25 CEST (+02:00).

## Which routes
The 17 console routes still in TypeScript at 669dc18, alphabetical, split 4/4/3/3/3:
analytics, arena, bank, cache | **context, gateway, guardian, hermes** | instances, keys, markers | md.$name, memory, mesh |
node, security, session. This branch ports the second fifth. `PORTED` goes from 13 to 17 (plus `/` = 18 of 31).

Source: the live console-ide tree read 2026-10-20 21:25 CEST
(`/workspace/swarm/artifacts/console-ui/live-src-20261020-2125`): `routes/_console/{context,gateway,guardian,hermes}.tsx`,
`lib/desk-foundation.ts`, `lib/fleet.ts` (`API_ENDPOINTS`), `components/hermes-panel.tsx`.

## What each room does now (no JavaScript, CSP unchanged)

| room | data (all through the existing loopback client) | writes upstream | differences from the TSX |
|---|---|---|---|
| `/context` | galion-desk `GET /desk` (new flag `--desk 127.0.0.1:8646`, loopback only) + `/api/v1/health` for the dot | none | no 4 s poll (Refresh link; decision 1); stamps on the Zagreb clock with the zone named; says when the desk is down instead of showing blanks; tool bars are `<progress>` (no inline style) |
| `/gateway` | the 11 `API_ENDPOINTS`, fixed in the binary | only the 2 POST endpoints of that list (`/api/v1/command {"cmd":"ping"}`, `/api/v1/boot {"mode":"light"}`), from a same-origin-checked form that carries only the endpoint number | pick = link, GET send = GET form (`?ep=N&send=1`), answer pretty-printed like `JSON.stringify(j,null,2)`, cap 200 000 chars |
| `/guardian` | `GET /api/v3/guardian/status` (browser's admin cookie forwarded) | `POST /api/v3/guardian/scan {}`; `POST /api/v3/guardian/heal {}` with `X-Galion-Pin` from a password field (1-64 visible ASCII, never rendered/logged/kept; empty PIN sends nothing) | `window.prompt` replaced by a password field; a failed status says why (live: `HTTP 401 (phone required)` unpaired) instead of "… · score — · incidents 0" |
| `/hermes` | `GET /api/hermes/{health,guard,modes,models,history}` and `history?id=` (id must be `[A-Za-z0-9._:-]{1,128}`, else 400 and no upstream call) | **none** | read-only: gateway line (bind address not shown, only "loopback"), skills count, guard / auto-accept state, plan/action models, saved modes, models, 60 newest threads, one thread (user/assistant text capped 6 000 chars each; tool rows show name, status, first command line, never tool output) |

`/hermes` was in `port2::SPA_FIRST` here (the SPA kept it under `--root`). Since console-route-31 (slot 170, see
`docs/console-route-31.md`) `SPA_FIRST` is empty and the read-only Rust room answers with or without a fallback.

Code: `crates/console-ui/src/port2.rs` (handlers), `pages/{context,gateway,guardian,hermes,p2}.rs`, a `PORT2` block in
`css.rs`, `client::call_with` (extra request headers), `UiConfig.desk` / `--desk`. Kept in separate files so the five
console-port branches touch as few shared lines as possible; the shared hunks are `lib.rs` (PORTED, `mod port2`, route arm,
`post_ok`, `owns`), `pages/mod.rs`, `css.rs` (sheet/defined_classes chain), `config.rs`/`server.rs` (`--desk`), `tests/ui.rs`
(fake routes, not-ported count 16 -> 12). Slot 135 (console-port-1) also adds a `desk` field and `--desk`; on merge keep one.

## Tests
- Unit (console-ui): p2 (JS number formats, Zagreb DST edges 2026-03-29 / 2026-10-25, `JSON.stringify` printer), context (live
  shapes, desk down), gateway (list = fleet.ts, render), guardian (render, refusal, PIN rules incl. CR/LF), hermes (ids, render,
  bind hidden, tool output hidden, no form).
- End-to-end, fake upstream (`crates/console-static/tests/ui_port2.rs`, 8 tests): GET-only for plain views; gateway CSRF
  (no proof / cross-site / foreign Origin -> 403, nothing upstream), GET endpoint POSTed -> 400, extra form fields ignored, body
  from the binary; guardian CSRF, empty/injected PIN -> 400 nothing sent, PIN only in `X-Galion-Pin`, never in the page; hermes
  never POSTs, bad id never reaches upstream; gate (unpaired public host -> pairing, other hosts -> Soon, unpaired POST does
  nothing); SPA fallback serves the Rust `/hermes` (was: keeps it, until slot 170); every emitted class is in the sheet.
- Real-life against the live VPS, read-only (`tests/real_port2.rs`, `GALION_REAL=1`, ports 20060-20063): see
  `docs/real-tests/console-port-2.md`.

## Owner decisions (Slavik)
1. **Hermes asking** (the JS ruling, decision 1 of `docs/console-ui-decision.md`, plus H1): (i) no-JS ask form, the server
   waits for the whole answer, no mic, no streaming; (ii) one hand-written script island for mic + streamed tokens; (iii) keep
   `/hermes` on the old SPA until after Oct 31. Until then the Rust `/hermes` is read-only. Any ask may reach a paid model
   (modes list OpenRouter/River models): H1 stays Slavik's.
2. **Guard / auto-accept / loops / settings switches on `/hermes`**: port them as CSRF-checked forms (they change Hermes's
   safety switches; live today: guard on, auto-accept on) — yes / no / only guard.
3. **`/gateway` Boot light stack** (`POST /api/v1/boot {"mode":"light"}`): kept for parity (the Agents room already sends
   `boot`), never sent in any test. Keep, or drop it from the Rust list.
4. **Refresh for `/context`** (TSX polled every 4 s): Refresh link now; `<meta http-equiv=refresh>` or a script island per
   decision 1.
5. **Interim**: ~~with an SPA fallback configured, `/hermes` stays SPA~~ — slot 170 (console-route-31) made the Rust room
   Rust-first; reverting is one line (`port2::SPA_FIRST = &["/hermes"]`). See `docs/console-route-31.md` decision R1.
