<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# console-route-31: `/hermes` Rust-first (wave 4 slot 170)

Branch `swarm/console-route-31`, base `console-port-2` 0916684. No new dependencies (std only, as before).

## What changed
- `crates/console-ui/src/port2.rs`: `SPA_FIRST` is now empty, so `/hermes` is served by the Rust room even when a
  pre-built SPA fallback (`--root`) is configured. The list stays, so handing a room back to the SPA is one line.
- The Rust `/hermes` stays **read-only**: the same five GETs (`/api/hermes/{health,guard,modes,models,history}`) plus
  `history?id=`; no `<script>`, no `<form>`, no inline style; the CSP header is byte-for-byte unchanged
  (`default-src 'none'; style-src 'self'; img-src 'self' data:; form-action 'self'; frame-ancestors 'none'; base-uri 'none'`).
- Writes held: POST / PUT / DELETE / PATCH `/hermes` -> `405 Allow: GET, HEAD` before the gate and before any upstream call
  (`/hermes` is not in `POST_ROUTES`). Asking, runs, steer/stop, approvals, attach, mic, speak and the guard / auto-accept /
  loops / settings switches remain held for Slavik (decisions 1 and 2 of `docs/console-port-2.md`, H1).
- The `/api/hermes/*` proxy that the old SPA uses is untouched (proxy prefixes come before the UI in `server.rs`).
- New `port2::LIVE_SRC_ROUTES` (the 31 page routes of the live source tree) and `Ui::spa_fallbacks(spa)` (live routes that
  are not Rust-first), so the coverage list is computed, not written by hand.
- The page note now says the room is served from Rust even when the old console is installed.

## The 31 live-source routes vs Rust on this base (0916684)
Source: `/workspace/swarm/artifacts/console-ui/live-src-20261020-2125/src/routes` (29 `_console/*.tsx` incl. `index` and
`md.$name`, plus `status.tsx`, `soon.tsx`).

| | count | routes |
|---|---|---|
| Rust-first (with or without `--root`) | 18 | `/` (302 -> `/console`), `/agents`, `/chill`, `/cloudflare`, `/console`, `/context`, `/data`, `/docs`, `/domains`, `/gateway`, `/guardian`, **`/hermes`** (new), `/jupyter`, `/pack`, `/repos`, `/services`, `/soon`, `/status` |
| still falling back to the SPA here | 13 | `/analytics`, `/arena`, `/bank`, `/cache`, `/instances`, `/keys`, `/markers`, `/md/$name`, `/memory`, `/mesh`, `/node`, `/security`, `/session` |

The 13 are not ported **on this base**; they are ported on console-port-1/-3/-4/-5 and merged in
`swarm/console-port-merge` 508401d (PORTED = 30, "31 of 31"). In 508401d two of them are still SPA-first on purpose:
`lib::SPA_FIRST = ["/arena", "/bank"]` (and `port2::SPA_FIRST = ["/hermes"]`). So after merging this branch into
console-port-merge, the routes still falling back under `--root` would be **`/arena` and `/bank`** only.

Merge notes for console-port-merge: take this branch's `port2.rs` (empty `SPA_FIRST`, `LIVE_SRC_ROUTES`, `spa_fallbacks`);
update the unit test `port2::tests::fallbacks_on_this_base` to `["/arena", "/bank"]`; in `tests/ui_route31.rs` /
`tests/real_route31.rs` the expected fallback list must also add `lib::SPA_FIRST`.

## Tests
- Unit (console-ui, 43 total, 3 new in `port2::tests`): the list is 31 unique routes and holds every PORTED path;
  `/hermes` is owned with and without a fallback and `SPA_FIRST` is empty; exact fallback list on this base (13).
- End-to-end, fake API (`crates/console-static/tests/ui_route31.rs`, 4 new): real server in-process **with** an SPA root,
  the 31 page routes and the `/api/` proxy: `/hermes` and `/hermes?id=` are the Rust page (CSP, X-Frame-Options, no-store,
  no script/form/inline style, bind hidden), HEAD has no body; 6 write variants (same-origin/Origin/cross-site POST, PUT,
  DELETE, PATCH) -> 405, zero upstream requests; every live route goes where `spa_fallbacks(true)` says; the old SPA's
  `POST /api/hermes/chat` is still proxied with its body, SPA assets still served.
- `tests/ui_port2.rs`: `spa_fallback_keeps_hermes` -> `spa_fallback_serves_rust_hermes`.
- Real-life against the live VPS, GET only (`tests/real_route31.rs`, `GALION_REAL=1`, ports 20400-20403): see
  `docs/real-tests/console-route-31.md`. `real_port2` re-run green too.

## Owner decision (Slavik)
- **R1** `/hermes` Rust-first under `--root`. The plan row (slot 170) asked for it, which answers decision 5 of
  `docs/console-port-2.md` ("confirm, or always serve the Rust room") as "always Rust". Effect once deployed with a
  fallback: the console has **no ask / runs / mic UI for Hermes** until decision 1 (JS ruling) and H1 (paid model) are
  made; the Hermes gateway and `/api/hermes/*` are unchanged, so other clients keep working. Options: (a) keep (this
  branch); (b) revert to SPA-first, one line `SPA_FIRST = &["/hermes"]`; (c) keep Rust-first and add a plain link to the
  old SPA room under another path (needs a route; not done).
- Nothing is deployed: the live console on the VPS still serves the old SPA; this branch changes no VPS file or service.
