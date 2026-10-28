<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# console-survival-card (wave 4 slot 408) — Console V2 prep, NOT the live cutover

Branch `swarm/console-survival-card` off `swarm/survival-refresh-writer` d25f8479. Times
Europe/Zagreb CET (+01:00). NO deploy, nothing installed, no live service touched.

## What it adds

- `crates/console-ui/src/pages/survival.rs` — a "Survival snapshot" card on the Rust `/bank`
  page, fed by the staged `GET /api/v1/survival` (slot 389, `--survival-file`). The UI fetches it
  in parallel with `/api/v1/bank` (same upstream, same forwarded Host/admin cookie, so the same
  API gate as every other room). Rows: `survival.json` as of (Zagreb clock, CET/CEST with offset),
  age, stale-after (3 × refresh), writer (status only / refreshing now / last run + result),
  refresh tally (runs, failures, mode).
- States (`data-state` on the `<section>`): `fresh`, `stale` (age > `staleAfterS`, or the route
  says stale), `missing`, `skew` (mtime ahead of the console clock), `unknown` (no readable time),
  `off` (404: console without `--survival-file`), `gated` (401/403), `error` (other HTTP),
  `down` (no answer / not a JSON object). Failed writer runs get a red note with the back-off.
- Never file content: the route already reports only the mtime; the card additionally reads an
  allowlist of 13 keys (`KEYS`, unit-tested against the code), never echoes a string from the
  reply (`asOf` parsed and re-rendered, `mode` matched to two names, writer path not shown),
  ignores absurd numbers (> 1e9).
- `pages::bank::body_with` / `insert_card` — card goes after Cash inside the page.
- Also fixed: repo-check `secrets` error on slot 389's decoy env value in
  `real_survival_refresh.rs` (value renamed to `decoy-389-not-a-key`); repo-check now PASS (0 errors).

## Tests

- Unit: `cargo test -p console-ui --lib` 124/0 (+10: 9 survival card + 1 bank insert; the
  every-emitted-class-is-styled test covers all card states). `-p console-ui -p console-static`
  263 passed / 0 failed / 7 ignored (GALION_REAL-gated). Workspace (`--exclude openmind-sandbox
  --exclude cgroup-detect`): 146 suites, 729 passed / 0 failed / 7 ignored.
- Real (box): `GALION_REAL=1 CONSOLE_SURVIVAL_LIVE=1 cargo test --release -p console-static --test
  real_survival_card -- --nocapture --test-threads=1` — 1/1 pass, 2026-10-28 17:24 CET, real release
  `galion-console-static` on 22780-22789, real release `galion-survival` writer (empty key file,
  no network, nothing spent). Card text as rendered:

```
1) stale  [stale]   2026-10-27 18:19 CET (+01:00) · 23.1 h · status only (no writer) · "Stale: survival.json is 23.1 h old, more than 3 refreshes of 15 min."
2) fresh  [fresh]   2026-10-28 17:21 CET (+01:00) · 2 min ; CEST instant -> 2026-10-18 21:35 CEST (+02:00) [stale] ; +2 h mtime -> [skew] "2.0 h ahead (clock skew)"
3) missing[missing] "survival.json is missing: the writer has not written it yet."
4) /bin/false writer -> "last run 2026-10-28 17:23 CET (+01:00): exit 1", "1 runs, 1 failed · on-request",
   "The last writer run failed (exit 1); the console waits 15 min"; 20 more /bank loads -> still 1 run;
   missing writer binary -> "could not start"
5) real galion-survival: first load [stale] "refreshing now" -> after run [fresh] "0 s", "last run 17:23 CET: ok",
   "1 runs, 0 failed"; real file has cpu/disk/lane content -> none of it on the page
6) no --survival-file: GET /api/v1/survival 404 -> card [off]
7) fake upstream: leaky 200 (asOf "<script>", writer "<img>", whole survival.json + decoy key inside) -> [fresh] "60 s",
   none of <script/<b>/<img//opt/galion/cash/key on page; 502 -> [error]; 401 -> [gated]; "not json" -> [down];
   dead upstream -> [down]
8) live mtime (ssh stat, RO) 1793121590 = 2026-10-27 18:19:50 +01:00, 23.1 h -> [stale], same text as 1)
```
  Every /bank answered 200 in 0-2 ms; no content marker (cash, decoy key, lane, cpu) on any page.
- Existing real tests re-run on this branch: `real_survival_refresh` 1/1 (32.6 s),
  `real_bank_asof` 2/2, `real_console_static` 1/1.
- Gates: `cargo fmt --all --check` ok; `clippy -p console-ui -p console-static --all-targets -D
  warnings` ok; `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` missing 0; `repo-check` PASS
  0 errors; x86_64 musl release (1,102,720 B, sha256 bf3b92df…) + aarch64 musl release ok;
  aarch64-linux-android + armv7 musl `cargo check` ok. No new dependencies.

## Real server (VPS), 2026-10-28 17:24–17:25 CET — read-only on live files

Live state (GETs only): `:8080` and `:18080` `/api/v1/survival` → 404 (live console has no
`--survival-file`; D-389-1 not applied), `/bank` 200 on both. Live `survival.json` mtime still
2026-10-27 18:19:50 +01:00 (1385 B, root 0644). `console.service`, `console-rust-primary.service`,
`guardian-tick.timer` active. Nerve `/state` 200, score 64 / D, cycle 104, last ts 17:15:02 +01:00;
cortex `:8674/health` 200.

Throwaway: box-built x86_64 musl binary (sha256 bf3b92df…) in `/var/tmp/survival-card-test-20261028172444`,
run as `nobody` on 127.0.0.1:22780, `--ui rust --dist <tmp> --survival-file
/opt/galion/galion-light/state/survival.json`, status only (no writer, so it cannot write):

```
/api/v1/survival 200 0.4 ms  {"ok":false,...,"exists":true,"asOf":"2026-10-27T18:19:50+01:00","ageS":83114,"stale":true,
                              "staleAfterS":2700,"refreshS":900,"mode":"on-request","writer":null,"runs":0,...}
/bank 200 1.7 ms  card: 2026-10-27 18:19 CET (+01:00) · 23.1 h · 45 min (3 × 15 min) · status only (no writer) ·
                  "Stale: survival.json is 23.1 h old, more than 3 refreshes of 15 min."
content markers on page (4321|sk-or|"usd"|openrouter|revolut): 0
POST /api/v1/survival 405; Host: evil.example 401
Host: console.galion.app /bank 503 (page gate fail-closed, same as F-396-1: no admin store)
30 more /bank loads: RSS 2,844 kB, 1 thread after them
live survival.json mtime after: 1793121590 (unchanged)
```

Process killed, folder removed, ports 22780-22789 on the VPS: 0 listeners.

## Findings

- F-408-1: the card makes the live problem visible: `survival.json` is 23.1 h old at 17:25 CET
  and nothing refreshes it (D-389-1 still open). With the 389 drop-in + interval mode the card
  would show `fresh` and the writer tally.
- F-408-2: on the live public host the whole `/bank` page is behind the fail-closed page gate
  (503, F-396-1), so the card only shows once D-396-1 is resolved.

## Owner decisions

- D-408-1: ship the card with the Console V2 cutover (it is inert until the console runs with
  `--survival-file`: shows `off`). Recommended yes, together with D-389-1 (interval mode).
- D-408-2: show the card also on `/console` (desk) — not done here; recommended only after
  D-389-1, so the desk does not show a permanent red "stale" line.
