<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Console `/bank` honours `asOf` + survival age (wave 4 slot 371)

Branch `swarm/console-bank-asof` off `swarm/console-dist-arena-bank` 7888d8fb. Fixes
F-351-5 (page ignored `asOf`/`stale`) and the console half of F-351-3 (one-shot
`survival.json` shown as if current). NO deploy, no VPS change. Times Europe/Zagreb CET +01:00.

## What changed (console-ui only; console-static untouched)

- `pages::bank::body_at(snap, now)` reads the optional reader keys `asOf` (ISO with any
  offset, shown as `2026-10-28 04:00 CET (+01:00)`), `ageS`, `stale`, `refreshS`.
  - Line under Cash: `Snapshot as of <Zagreb time> · <age> ago. The reader refreshes every
    <n>; this page reloads every <n>.`
  - Stale (`stale:true`, or age > 3 x `refreshS`, default 300 s; D-351-3 "warn, never hide"):
    `<p class="note t-bad" role="status">Stale: the last snapshot is 20 min old, more than 3
    refreshes of 5 min. ...` - figures stay visible.
  - `asOf` more than 60 s in the future: "clock skew, age unknown" (never marked fresh/stale
    from a bad clock). Unparsable `asOf`: "Snapshot time could not be read." (raw text never
    echoed). No time keys at all (old reader / Node shape): says the freshness is unknown.
- Per purse: optional `asOf`/`ageS`/`stale`/`refreshS`; appended to the note as
  `stale · as of <Zagreb time>, <age> old`. Purses with `"source":"survival"` are judged on the
  15-min interval of the retired `survival-cash` cron (900 s -> stale after 45 min), so the
  one-shot `survival.json` (F-351-3) shows as stale in an otherwise fresh snapshot.
- Interval refresh without JavaScript: `Frame.refresh_s` -> `<meta http-equiv="refresh"
  content="N">`, only on `/bank`; N = reader `refreshS` clamped 60..=3600, default 300 (also
  on 404/502/503 so the page picks up a reader once one answers). Every load is one reader GET
  (no console-side cache).

## Tests

- Unit (console-ui): 114 passed / 0 failed (+6 new: clock constant, asOf on Zagreb clock incl.
  a UTC-stamped input, stale edge 900/901 s + flag + ageS-only, missing/bad/future/hostile asOf,
  survival purse ageing, refresh clamp + span format). CSS class coverage test includes the stale
  render.
- console-ui + console-static all tests: 242 passed / 0 failed (7 ignored real tests skipped).
- `GALION_REAL=1 real_bank_asof` (ports 22410-22411, release `galion-console-static --dist
  --ui rust --data-upstream`), 2026-10-28 06:24 CET:

```
1) reader warming (503)            /bank 15006 B  "HTTP 503" + "reloads every 5 min", meta 300
2) asOf now-120 s, refreshS 120    meta 120; "Snapshot as of 2026-10-28 06:23 CET (+01:00) · 2 min ago.
                                   The reader refreshes every 2 min; this page reloads every 2 min."
3) asOf now-1200 s, refreshS 300   "Stale: the last snapshot is 20 min old, more than 3 refreshes of 5 min." $9.50 still shown
4) ageS 30 + stale:true            "Snapshot 30 s old." + "the reader marks this snapshot as stale"
5) no time keys                    "sent no snapshot time (asOf)", meta 300
6) survival purse 10 h old         Runpod: "from survival probe · stale · as of 2026-10-27 20:25 CET (+01:00), 10.0 h old"
                                   Spare (10 min): "as of 2026-10-28 06:15 CET (+01:00), 10 min old" (not stale)
7) 5 loads -> 5 reader GETs; /arena has no meta refresh
```

- Live leg READ-ONLY (`CONSOLE_BANK_LIVE=1`, ports 22416-22419), 2026-10-28 06:25 CET:

```
ssh stat -c %Y /workspace/galion-light/state/survival.json  -> 2026-10-27T18:19:50+01:00 (12 h ago; content not read)
fake reader with that survival asOf -> Runpod: "from survival probe · stale · as of 2026-10-27 18:19 CET (+01:00), 12.1 h old"
ssh -L 22419 -> VPS 127.0.0.1:18080: GET /api/v1/bank -> 404 "Not Found"
box console --data-upstream tunnel: /bank 200 15021 B 553 ms, "no /api/v1/bank route yet", meta refresh 300
```

- Regression: `GALION_REAL=1 real_dist_arena_bank` 3/3, `ui_port1` 6/6.
- VPS read-only 06:26 CET: nerve `/state` 200 cycle 58 mode live; cortex `:8674/health` 200;
  `console.service` + `console-rust-primary.service` active; `:8080/api/v1/bank` 404. Nothing
  changed on the VPS.
- Gates: `cargo fmt --check` ok; clippy `-D warnings` console-ui + console-static all targets ok;
  no-python ok; spdx missing 0; repo-check 0 errors (52 warnings, +1 = the page's
  `SystemTime::now()` that only feeds the Zagreb clock). x86_64 musl `galion-console-static`
  1037184 B (unchanged vs 5aa31927 line), aarch64 musl 877816 B; aarch64-android + armv7 check ok.

## Findings / owner

- **F-371-1:** the survival writer itself is gone: `survival.json` was rewritten every 15 min
  by the Python Hermes `survival-cash` cron (stack/hermes/cron/jobs.json) which retired with
  `hermes.service`; last write 2026-10-27 18:19 CET. The page now flags it; a real refresh needs
  the hermes-rs bank reader (slot 351 design, `HERMES_RS_BANK_REFRESH_S`) or a Rust survival
  timer. Owner choice, not done here.
- **D-371-1:** the bank reader should send `asOf` (Zagreb ISO), `refreshS`, and per-purse
  `asOf` + `"source":"survival"` for survival-derived figures; this branch is the reader contract.
- **D-371-2:** ship with the next console deploy candidate (7888d8fb / 5aa31927 line); NO deploy here.
