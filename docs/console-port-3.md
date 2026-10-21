<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# console-port-3: `/instances`, `/keys`, `/markers` in Rust

Wave 4 slot 137, branch `swarm/console-port-3`, base `landing-overview` 669dc18. Written 2026-10-21 05:52 CEST (+02:00).

## Which routes
The 17 console routes still in TypeScript at 669dc18, alphabetical, split 4/4/3/3/3:
analytics, arena, bank, cache | context, gateway, guardian, hermes | **instances, keys, markers** | md.$name, memory, mesh |
node, security, session. This branch ports the third fifth. `PORTED` goes from 13 to 16 (plus `/` = 17 of 31; with
console-port-2 merged, 21 of 31).

Source: the live console-ide tree read 2026-10-20 21:25 CEST
(`/workspace/swarm/artifacts/console-ui/live-src-20261020-2125`): `routes/_console/{instances,keys,markers}.tsx`,
`routes/api/v1/{instances,laptop,keys,markers}.ts`, `lib/{keys,markers,catalog}.ts`.

## What each room does now (no JavaScript, CSP unchanged)

| room | data (existing loopback client, admin cookie forwarded) | writes upstream | differences from the TSX |
|---|---|---|---|
| `/instances` | `GET /api/v1/instances`, `GET /api/v1/laptop`, `/api/v1/health` (dot), in parallel | **none** | no 8 s / 15 s poll (Refresh link); a failed call is named ("The laptop status did not load: …") instead of "Reading…" forever; the TSX kept the last `model-pull` job across polls, a server render shows the current jobs only; Copy buttons gone (laptop key and tunnel command in selectable `<pre>`); laptop terminal capped to 80 lines × 400 chars, control characters shown as spaces; job `pct` clamped 0-100 and drawn as `<progress>`; "N of M online" over the parts |
| `/keys` | `GET /api/v1/keys` | `POST /api/v1/keys` only from a same-origin-checked form: **mint** `{"name","scopes"}` (name trimmed, control characters dropped, max 64 chars; scopes only from chat/tools/files/admin, in that order) and **revoke** `{"action":"revoke","id"}` (id must be 1-32 hex digits, else 400 and nothing sent). `action:"check"` and any other `op` -> 400, nothing sent | scope toggles are checkboxes (`chat` ticked, as the TSX started); the fresh key is shown once in the answer to the mint post (`Cache-Control: no-store`), never logged, never in a URL; a mint answer without a sane `gal_sk_…` key is shown as a refusal; a failed list says why (live: `HTTP 401 (phone required)`) instead of "No keys."; created time shown on the Zagreb clock |
| `/markers` | `GET /api/v1/markers` | `POST /api/v1/markers` only from a same-origin-checked form with the TSX's fixed body `{"zone":"active","kind":"event","source":"desk","note":"manual ping"}` (form fields never reach the body) | no 4 s poll and no SSE heartbeat rail (`/api/v1/markers/stream` needs `EventSource`): Refresh link; times on the Zagreb clock with the zone named (TSX printed UTC `HH:MM:SS` without a zone); header shows "Ring of 42 · N stored"; unknown kinds shown as `event` |

All three rooms sit behind the existing page gate (loopback or a paired `galion_admin` cookie; unpaired public hosts get
the pairing page and their POSTs send nothing).

Code: `crates/console-ui/src/port3.rs` (handlers), `pages/{instances,keys,markers,p3}.rs`, a `PORT3` block in `css.rs`
(all classes `p3-*`). Kept apart so the five console-port branches touch as few shared lines as possible; the shared
hunks are `lib.rs` (PORTED, `mod port3`, route arm, `post_ok`), `pages/mod.rs`, `css.rs` (sheet/defined_classes chain),
`tests/ui.rs` (4 fake API routes, not-ported count 16 -> 13). No `UiConfig` change, no new flag.

Merge notes: console-port-2 (0916684) also edits `post_ok` (`port2::POST_ROUTES`), the PORTED tail, the css chain and the
not-ported count; on merge keep both (`port2::POST_ROUTES` **or** `port3::POST_ROUTES`; count 16 - 4 - 3 = 9 with both).
`pages/p3.rs` has its own Zagreb clock (std only); console-port-2 has one in `pages/p2.rs` — fold into one later. The
workspace crate `zagreb-time` was not used because console-ui is dependency-free (it pulls chrono-tz).

## Tests
- Unit (console-ui, 42 total): p3 (civil-date round trip, Zagreb DST edges 2026-03-29 / 2026-10-25 and 2027, HH:MM:SS and
  ISO-8601 -> Zagreb, caps, control characters), instances (live shape, jobs/lines/caps/clamp/escaping, failures named),
  keys (id/name/scope rules, list and forms, 401, fresh key once, refusals), markers (fixed ping body, empty live ring,
  newest-first Zagreb times, escaping), port3 (routes are rooms and PORTED; every emitted class is in the sheet).
- End-to-end, stateful fake upstream (`crates/console-static/tests/ui_port3.rs`, 7 tests): GET-only plain views and
  PUT/DELETE/PATCH -> 405; instances content; slow laptop (6 s > 4 s budget) still renders in < 6 s and names the call;
  dead upstream on all three rooms; mint -> exact upstream body, only the admin cookie forwarded, key once then gone on
  reload, revoke -> exact body, button gone, second revoke "Nothing revoked"; CSRF (no proof / cross-site / foreign Origin)
  -> 403 with nothing sent; bad ids, `op=check`, unknown op, empty body -> 400 with nothing sent; unpaired mint -> "Mint
  refused: HTTP 401"; Ping fixed body even with `note=evil&zone=chill&kind=dns`; gate (pairing page, Soon host, unpaired
  POSTs send nothing, paired public host works).
- Real-life against the live VPS, GET only (`tests/real_port3.rs`, `GALION_REAL=1`, ports 20070-20073): see
  `docs/real-tests/console-port-3.md`.

## Owner decisions (Slavik)
1. **Mint / Revoke as no-JS forms on `/keys`.** Ported as CSRF-checked forms (they change the live key store). Never sent
   live in any test. Keep, or make `/keys` read-only in Rust until the JS ruling (decision 1 of
   `docs/console-ui-decision.md`).
2. **Fresh key in the POST answer.** Reloading that answer makes the browser ask to resend the form, which would mint a
   second key. Options: (a) keep (the browser warns first); (b) one-time nonce field so a resend is refused; (c)
   post-redirect-get with the key held server-side for one read.
3. **Refresh for `/instances` and `/markers`** (TSX polled 8 s / 15 s / 4 s, plus the SSE heartbeat rail): Refresh link
   now; `<meta http-equiv=refresh>` or a script island per the JS ruling.
4. **Laptop terminal lines** (`/api/v1/laptop` `lines`): the TSX shows live Windows command lines of the laptop's
   processes; they could carry tokens if a command line ever does. Ported for parity (capped, escaped; live today: 0
   lines). Keep, hide, or show only the count.
