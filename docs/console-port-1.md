<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# console-port-1: `/analytics`, `/arena`, `/bank`, `/cache` in Rust

Branch `swarm/console-port-1`, based on `swarm/landing-overview` 669dc18. Wave 4 slot 135.
Written 2026-10-21 05:08 CEST (+02:00). **Nothing deployed.**

## Which routes (the split for slots 135-139)

At 669dc18, 14 of the 31 console routes render in Rust. The 17 still in TypeScript, alphabetical, split
4/4/3/3/3 (17 = 5 x 3 + 2, the first two slots take one more):

| slot | slug | routes |
|---|---|---|
| **135** | **console-port-1** | **/analytics, /arena, /bank, /cache** (this branch) |
| 136 | console-port-2 | /context, /gateway, /guardian, /hermes |
| 137 | console-port-3 | /instances, /keys, /markers |
| 138 | console-port-4 | /md/$name, /memory, /mesh |
| 139 | console-port-5 | /node, /security, /session |

The 17 are the 16 rooms that still answered "Not in Rust yet" plus `/md/$name` (`md.$name.tsx`, a page route
that is not a room). `/console` was in the c242fed list of 17 but landing-overview ported it, so it is not counted.

## Source of truth

The TSX was read from the live tree `/opt/galion/console-ide` on the VPS (read-only `sha256sum` and `cat`
2026-10-21 05:06 CEST; nothing written). `bank.tsx`, `cache.tsx`, `lib/analytics.ts`, `lib/bank.ts` and
`lib/desk-foundation.ts` are byte-identical to the box copy in `/workspace/oom-test`; `analytics.tsx` (Memory row
shows "—" unless both `memUsed` and `memTotal` exist) and `arena.tsx` (a `mode` type field) differ and the live
versions were followed.

## What each page does

| route | data (GET only) | Rust page | differs from the TSX |
|---|---|---|---|
| `/analytics` | console API `/api/v1/analytics` (phone cookie forwarded) | 7 boards, same rows and words, `analyticsHealth()` ported | boards are `?board=` links; no 5 s poll; a 401 says "paired phones only" |
| `/arena` | console API `/api/arena` (`arena.py status`, read-only) | spend line, champion, board newest first, answers | **Breed / Stop / Promote not ported** (decision A) |
| `/bank` | console API `/api/v1/bank` (**does not exist yet**) | purses, cash rows, River estimate, note; `money/count/days/clean` ported (the secret-scrubbing regexes as hand-written matchers) | without the API: the TSX empty state plus why (decision B) |
| `/cache` | galion-desk `127.0.0.1:8646/desk` (new `--desk` flag) | header, road, read by model, on disk, recent files | `<meter>` bars (CSP forbids inline style); times in Europe/Zagreb; no 4 s poll |

Shared: `pages/jsfmt.rs` prints numbers exactly like JS (`String(n)`, `toFixed` with JS tie rounding, `Math.round`,
the "drop the word undefined" helpers). All four pages: no `<script>`, no inline style, GET/HEAD only (POST = 405
before any upstream call), behind the same gate as every room (loopback or paired phone; other hosts get Soon).

`console_ui::SPA_FIRST = ["/arena", "/bank"]`: when the front has an SPA fallback (`--root`), those two stay on the
TypeScript build, because the Rust pages are not yet full replacements. Without a fallback, Rust answers them.

TanStack quirk found on the live server: a JSON GET (`Accept: application/json`) of a path the console has no route
for gets `500 {"error":"Only HTML requests are supported here"}`, not 404. `get_json_why` counts that as 404.

## Owner decisions (Slavik)

**A. Arena actions.** Breed runs `arena.py generation` (paid model calls against the arena's $2 cap; live board now
2/3 generations, $0.00 spent). Options: (1) keep actions on the TS console (current); (2) port them as no-JS forms
with the same same-origin check as `/agents`, Breed included; (3) port Stop and Promote only (no spend), Breed stays
TS. Recommendation: (3), Stop is the safety button and Promote spends nothing.

**B. Where the bank reading lives.** `readBank()` reads provider keys from the secrets env files and calls OpenRouter
`/key` and River `/models` with them, and reads `hermes/state.db`. Options: (1) add `GET /api/v1/bank` to the Node
console returning `readBank()` as is (a 10-line TS route; the Rust page already renders it); (2) serve it from a Rust
service that is allowed to read those keys (hermes-core / Hermes Rust V2, where Revolut is being wired); (3) leave
`/bank` on the SPA. This is decision 3 of docs/console-ui-decision.md for this room. Recommendation: (2) for the
V2 redesign, (1) as the bridge.

**C. Refresh.** The TSX polled every 4-5 s. The Rust pages are one fresh sample per load. A `<meta http-equiv="refresh"
content="5">` per page is one line if wanted (decision 1 of console-ui-decision.md covers JS; meta refresh is not JS).

## Tests

- `cargo test -p console-ui -p console-static`: console-ui 45 unit (19 new), console-static e2e `tests/ui_port1.rs` 6
  new (boards, cookie forwarding, 401, arena never acts, bank with/without API and the TanStack 500, desk, gate,
  SPA_FIRST), `ui.rs` count made invariant (28 rooms = ported + not ported) so slots 136-139 do not collide on it.
- Real life: `crates/console-ui/tests/real_port1.rs` (GALION_REAL=1, ports 20050-20059; pages vs live payloads, 405 for POST/PUT/DELETE, HEAD, gate), results in
  docs/real-tests/console-port-1.md.

## Merge notes (with slot 136 `swarm/console-port-2` 0916684)

Both branches add `--desk ADDR` / `Config.desk` / `UiConfig.desk: Option<SocketAddr>` with the same semantics
(loopback galion-desk, `GET /desk`). On this branch the field order, the parser arm, the server wiring and the
config unit test are byte-identical to 0916684, so those hunks merge cleanly. Verified by a trial merge in a scratch clone (2026-10-21 05:45 +02:00; clippy and tests green after). Textual conflicts, all
mechanical (keep both sides):

- `config.rs` help line: use `loopback galion-desk for /cache, /context (GET /desk),`.
- `console-ui/src/lib.rs`: `PORTED` (append both blocks), the `UiConfig.desk` doc comment (one field, mention
  `/cache` and `/context`), the match arm list (both `port1_room` and `port2_room` arms), and `owns()`: use
  `PORTED.contains(&path) && !(spa_fallback && (SPA_FIRST.contains(&path) || port2::SPA_FIRST.contains(&path)))`.
- `pages/mod.rs`: keep all module lines, alphabetical.
- `css.rs`: keep both `PORT1` and `PORT2` blocks and chain both. Both define `.road` / `.road li` (timeline);
  the values are the same apart from font-size, so either order renders the same timeline.
- `console-static/tests/ui.rs`: this branch counts `not_ported + ported == 28` (invariant), slot 136 asserts
  `not_ported == 12`; after merging both the invariant form is the one that stays true (8 not ported).
