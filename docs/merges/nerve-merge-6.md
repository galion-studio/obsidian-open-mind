<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# nerve-merge-6 (wave-4 slot 333), 2026-10-28 CET (+01:00)

`swarm/nerve-merge-6` = `swarm/nerve-merge-5` b1a0d683
+ `--no-ff` merge `swarm/nerve-console-deep-health` 38750655 (slot 315, N-307-1 / F-313-2)
+ `--no-ff` merge `swarm/nerve-planned-stop-tag` 12956b45 (slot 310, F2 of slot 271)
+ staged nerve component `console-rust-primary` for :18080 (D-316-1 option A).
Box + VPS read-only. **Not deployed**; no live file, unit or service changed.

## Merges

| Commit | What | Conflicts |
|---|---|---|
| aef8846a | merge nerve-console-deep-health 38750655 (base 5dac9b4a = merge-4, already in merge-5) | 0 |
| c152d236 | merge nerve-planned-stop-tag 12956b45 (base 9d1a4f3 = live build; its own-services 9479050 is already in merge-5, so the merge adds only the planned-stop delta: engine, evt/journal, evt/mod, evt/runtime, state, contract doc, fixtures, real test) | 0 |
| ad803aa7 + this doc | staged component table, tests, REAL_NERVE_PORT_BASE for `real_console_deep` + `real_planned_stop` | n/a |

`state.rs` was touched by both branches (deep-route verdict in `assess`, planned
restarts in `push_planned`); git merged them textually and they are independent
(both unit tests `console_deep_routes_404_degrade_not_hard` and
`planned_restarts_do_not_degrade` pass in the merged tree).

## Staged component (D-316-1, option A)

Live fact found read-only (01:14 +01:00): the live nerve does **not** use the
built-in table. `galion-nerve.service` drop-ins set
`NERVE_COMPONENTS=/etc/galion-nerve/components-live.json` (2669 B, 13 entries,
every `restarts_per_hour` 0, no `deep`). Copied read-only as fixture
`crates/openmind-nerve/tests/fixtures/own-services/components-live-vps-20261028-0117.json`
(sha256 prefix dc3434937fbdbb99).

`deploy/staged/nerve/components-live.json` = that file + two changes only
(guarded by the hermetic test `tests/staged_components.rs`):

1. new `console-rust-primary`: unit `console-rust-primary.service`, port 18080,
   path `/`, HEPHAESTUS / system `console`, group `console` (one incident with the
   :8080 console), `critical: false`, `restarts_per_hour: 0` (alert-only),
   deep `["/api/v1/admin","/api/v1/services"]`.
2. `deep` added to `console` (:8080). Without it the deep probe of slot 315 stays
   **off** live after a nerve deploy, because the file replaces the built-in table
   where `CONSOLE_DEEP` lives (doc conflict below).

The live build 9d1a4f3 accepts the staged file as is (unknown key `deep` is ignored):
box replay with `openmind-nerve-9d1a4f3` (copied read-only from the VPS, sha256
prefix d0c70b879e5866bb) over the 01:16 capture: live table unmanaged `[18080]`
(14 components), staged table unmanaged `[]` (15 components).

Deploy (owner step, NOT done): back up `/etc/galion-nerve/components-live.json`,
install the staged file there, restart `galion-nerve` (or ship with the merge-6
nerve build). Rollback = restore the backup and restart.

## Tests (box unless noted)

| Gate | Time (+01:00) | Result |
|---|---|---|
| `cargo fmt --all --check` | 01:33 | ok |
| `clippy --workspace --all-targets -D warnings` (excl. openmind-sandbox, cgroup-detect) | 01:33-01:34 | ok |
| `clippy -p openmind-nerve --all-targets --features journal-native -D warnings` | 01:34 | ok |
| `cargo test --workspace` (excl. openmind-sandbox, cgroup-detect, hermes-tools) | 01:34-01:36 | 280 suites, 1345 passed, 0 failed, 6 ignored |
| `cargo test -p openmind-nerve --features journal-native` | 01:36 | 23 suites, 183 passed, 0 failed |
| `scripts/no-python.sh`, `scripts/spdx-scan.sh` | 01:36 | ok; 559 scanned, 0 missing |
| x86_64 musl release `openmind-nerve` | 01:32 | 1,479,456 B (sha256 prefix 8c9d7a5fa4f84d6d) |
| aarch64 musl release (rust-lld) | 01:37 | 1,221,440 B |
| `cargo check` aarch64-linux-android, armv7-unknown-linux-musleabihf | 01:37 | ok |

GALION_REAL=1, release, `REAL_NERVE_PORT_BASE=22030`, `--test-threads=1`, ports
22030-22039, final run 01:43:52-01:45:00 (also 01:22-01:25 before the event-mode
split), all pass, ports closed after:

| Test | Result |
|---|---|
| `real_own_services` (3) | replay 2026-10-22 capture unmanaged [18800] + override [8674,18800]; live-parity replay 22:26 capture unmanaged [18080], 17 listeners == live 9d1a4f3; serve legacy + event: stray :22031 reported |
| `real_console_deep` (1) | stub consoles (the full-console 9c0f9d1a binary is not on the box): blind degrading on 2 deep 404s, nodeep healthy, full healthy, killed -> failing |
| `real_planned_stop` (1) + negative control `GALION_NERVE_BASE_BIN` = live 9d1a4f3 | NEW: console failed/exit high 0, planned_stop 6, stopping 3, restarts_1h 0, planned 3, healthy; hermes crash control degrading. BASE 9d1a4f3: console failed high 3, restarts_1h 3, failing-soon (bug shown) |
| `real_staged_console` (2, new) | replay of both live captures (22:26 and 01:16): live table unmanaged [18080] -> staged [] and every other owner unchanged, 18080 owner `console-rust-primary`; serve legacy + `--mode event` with the staged console entries moved into the block against a stand-in answering like live 2d846fb8 (`/` 302, deep 404): primary degrading on both deep routes, consecutive_down 0, reflex 0, not unmanaged; live-table control: primary unmanaged, console healthy (the blind spot) |

VPS read-only throwaway (01:32:35-01:33:03 +01:00): musl nerve 8c9d7a5f as `nobody` in
`/var/tmp/nerve-333-20261028013224` on 127.0.0.1:22030 with the staged table,
`NERVE_SYSTEMD=0 NERVE_JOURNAL=0 NERVE_REFLEX=0 NERVE_ROLLBACK=0 NERVE_DRY_RUN=1
NERVE_EVT_ESCALATE=off`, legacy then `--mode event`, ~12 s each; killed, folder removed,
:22030 closed. Both modes, against the real live consoles:

```
console              degrading http 302 ["degrading: deep route /api/v1/admin HTTP 404","degrading: deep route /api/v1/services HTTP 404"] consecutive_down 0
console-rust-primary degrading http 302 (same two reasons)                                                            consecutive_down 0
exposure: unmanaged [] of 20 listeners judged; :18080 owner console-rust-primary HEPHAESTUS
```

Live nerve 9d1a4f3 at 01:33:11 +01:00 (`/state`, cycle 38): unmanaged [18080],
finding "1 loopback listener(s) no monitored unit owns: :18080", console healthy
http 302 (blind spot still live); `/health` 200, cortex :8674 `/health` 200.
Direct GETs 01:14: :8080 and :18080 both `/` 302, `/api/v1/admin` 404,
`/api/v1/services` 404, `/health` 404.

## Doc conflicts

- **C-333-1** slot 315 doc (D-315-2 "ship with the next nerve build") assumes the
  built-in `CONSOLE_DEEP`; the live nerve reads `components-live.json`, so a binary
  deploy alone keeps the deep probe off. Resolved here by `deep` in the staged file.
- **C-333-2** `docs/merges/nerve-merge-5.md` says console-rust-primary is "up since
  18:28 +01:00". The VPS clock is UTC: `ActiveEnterTimestamp=Tue 2026-10-27 18:28:33 UTC`
  = **19:28:33 +01:00**. Left as is in that doc; this one is the correction.
- **C-333-3** built-in `default_specs` (restarts_per_hour 3 for most services,
  `CONSOLE_DEEP` on console) differ from the live file (all 0, no deep). The
  built-in table is not what runs live; tests that assume built-in values do not
  describe production.
- `docs/nerve-event-contract.md`: both merges touched it; merged text is consistent
  (`unit.stopping` / `unit.planned_stop` rows added after `unit.finished`).

## Owner decisions

- **D-333-1** install the staged `components-live.json` (works with live 9d1a4f3 too:
  clears the :18080 stray finding at once; deep needs the merge-6 build).
- **D-333-2** accept that `console` and `console-rust-primary` show `degrading`
  right after a merge-6 deploy until the board API / admin store land on the live
  console (D-303-1 / D-313-1), or drop `deep` from the file to deploy quietly.
- **D-333-3** `console-rust-primary` as non-critical, alert-only, same group as
  `console`; say if it should become critical once it is the only console.
- merge-6 replaces merge-5 as the nerve deploy candidate (deploy is a separate step).
