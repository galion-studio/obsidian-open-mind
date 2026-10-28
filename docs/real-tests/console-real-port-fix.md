<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# console-real-port-fix (wave 4 slot 364, F-349-1)

Branch `swarm/console-real-port-fix` off `swarm/console-dist-arena-bank` 7888d8fb.
Box only, VPS read-only (ssh `-N` forwards, GET only, plus the existing
real_port4 read-only `find/stat/date/head` probe). No live console change,
no deploy, no VPS write. Ports 22340-22344 (slot 364 range 22340-22349).

## Problem (F-349-1)

`real_port2/3/4` (`#[ignore]`, `GALION_REAL=1`) compared the Rust rooms with the
**Node** console API on VPS :8080. Since the cutover, :8080 is
`galion-console-serve-2d846fb8 --dist /opt/galion/console-dist --ui rust --local-api
--pulse 127.0.0.1:8091 --desk 127.0.0.1:8646` (systemd `console.service`, same
binary on :18080 `console-rust-primary.service`). Baseline on 7888d8fb,
2026-10-28 05:49 CET (+01:00), all three FAILED:

| test | failed at | live answer |
|---|---|---|
| real_port2 | `/api/hermes/history` must be 200 | 404 |
| real_port3 | `/api/v1/markers` must be 200 | 404 |
| real_port4 | live Node `/md/user` must be 200 | 404 (Rust 404 page, 886 B) |

## Path matrix: what the rooms read vs what serves it

Recorded by `tests/real_port_shape.rs` at 05:57:56 CET (+01:00). "candidate" =
this branch's release binary started with the exact live flags on the box
(throwaway `--dist` dir), "live" = VPS :8080 through the forward.

| path | live :8080 | candidate | rooms | source |
|---|---|---|---|---|
| /api/v1/health | 200 | 200 | /gateway /node header-dot | local-api |
| /api/v1/ping | 200 | 200 | /gateway | local-api |
| /api/v1/domains | 404 | 200 | /gateway /domains | board (9c0f9d1) |
| /api/v1/services | 404 | 200 | /gateway /domains /services | board |
| /api/v1/repos | 404 | 200 | /gateway | board |
| /api/v1/markers | 404 | 200 | /gateway /markers | board |
| /api/v1/ledger | 404 | 200 | /gateway | board |
| /api/v1/catalog | 404 | 200 | /gateway /docs | board |
| /api/v1/docs | 404 | 200 | (board only) | board |
| /api/v1/protocol | 404 | 200 | (board only) | board |
| /api/v1/protocol/snapshot | 404 | 200 | /gateway | board |
| /api/v3/guardian/status | 404 | 404 | /guardian | Node only, retired |
| /api/hermes/{history,health,models,modes,guard} | 404 | 404 | /hermes | Node only, retired |
| /api/v1/instances, /api/v1/laptop | 404 | 404 | /instances | Node only, retired |
| /api/v1/keys | 404 | 404 | /keys | Node only, retired |
| /api/mesh | 404 | 404 | /mesh | Node only, retired |

Direct probe of the neighbours (05:47 CET): hermes-rs :8642 answers 401 (API key)
for every `/api/*` path and galion-desk :8646 404s them, so there is no keyless
loopback source to route the retired paths to. Routing them would need the
Hermes API key in the console process: **not done** (secrets rule).

So: two different causes.
1. **Deploy delta** (9 board paths): live binary 2d846fb8 predates the board-API
   parity commit 9c0f9d1 (slot 303). This branch's binary already serves them;
   the live rooms /gateway /markers /domains /services /docs will show data once
   a candidate containing 9c0f9d1 is deployed (owner decision; not done here).
2. **Retired Node routes** (hermes, guardian, instances, laptop, keys, mesh,
   md stub): gone everywhere. Rooms show an honest "did not load: HTTP 404".

## Fix (tests only, no routing change)

- `tests/common/mod.rs`: `CONSOLE_REAL_PORT_BASE` (validated 1024..=65525, invalid
  value panics; default = each test's old base 20060/20070/20080) and
  `Shape::{Data, Retired, Other}` from the live status.
- real_port2/3/4: each Node-API comparison now branches on the live status. 200 =
  the original data comparison (kept verbatim in `hermes_with_data` /
  `instances_with_data`), 404 = assert the room's exact failure text
  (`p3::miss(..)`, "The thread list (/api/hermes/history) did not load.",
  "Mesh status did not load: HTTP 404.", ...), no data cards, no script.
  real_port4 gate: live `/api/v1/admin` 200 -> pairing page, otherwise the
  binary must fail closed with 503 "Console API is not answering" and no data.
- New `tests/real_port_shape.rs`: the matrix above, candidate in live shape;
  pins candidate status per path, records live status and the deploy delta,
  fails on any unexpected live status ("matrix out of date"); candidate rooms:
  /markers = its own ring, /gateway every GET 200, /context = live desk,
  retired rooms say 404. `matrix_is_consistent` (no network) checks every
  /gateway GET endpoint is in the matrix.

## Real runs (box, release, `GALION_REAL=1 CONSOLE_REAL_PORT_BASE=22340`)

Final run 2026-10-28 05:55:26 - 05:57:07 CET (+01:00), shape re-run 05:57:56:

| test | result |
|---|---|
| real_port2 | ok (1 real + 1 unit) 24.2 s |
| real_port3 | ok (1 real + 1 unit) 19.8 s |
| real_port4 | ok (1 real + 1 unit) 12.4 s |
| real_port_shape | ok (1 real + 2 unit) 23.4 s |

Negative checks: `CONSOLE_REAL_PORT_BASE=80` -> panics "not a port base
1024..=65525"; without `GALION_REAL` the ignored tests skip (ok).
Nerve `/state` before/after every run: hermes-rs:8642, galion-desk:8646,
galion-one:8091, console:8080 all healthy (cycle 55, mode live: event-mode
cadence, N-332-1). Cortex not consulted (read-only nerve suffices here).
Ports 22340-22344 free after the runs; temp mirror/dist dirs removed.

Gates: `cargo fmt --check -p console-static` ok; `cargo clippy -p console-static
-p console-ui --all-targets -- -D warnings` ok; `cargo test -p console-static -p
console-ui` 239 passed / 0 failed / 8 ignored; `scripts/no-python.sh` ok.
No new dependencies.

### Final-run output (`real:` lines)

```text
ok
real: forward 127.0.0.1:22341->8080 127.0.0.1:22342->8646 127.0.0.1:22343->8670 (ssh -N, GET only)
real: nerve before: cycle 55 mode live components 14 -> ["hermes-rs:8642 healthy", "galion-desk:8646 healthy", "galion-one:8091 healthy", "console:8080 healthy"]
real: /context 200 20725 B in 795 ms
real: /context ok: ctx 65536 window 262144 road 12 tools 12
real: /gateway ep=0 GET /api/v1/health -> live 200, page 15801 B in 609 ms
real: /gateway ep=1 GET /api/v1/domains -> live 404, page 15476 B in 605 ms
real: /gateway ep=2 GET /api/v1/services -> live 404, page 15478 B in 608 ms
real: /gateway ep=3 GET /api/v1/repos -> live 404, page 15479 B in 796 ms
real: /gateway ep=4 GET /api/v1/markers -> live 404, page 15476 B in 608 ms
real: /gateway ep=5 GET /api/v1/ping -> live 200, page 15646 B in 605 ms
real: /gateway ep=6 GET /api/v1/ledger -> live 404, page 15482 B in 608 ms
real: /gateway ep=8 GET /api/v1/catalog -> live 404, page 15489 B in 378 ms
real: /gateway ep=9 GET /api/v1/protocol/snapshot -> live 404, page 15491 B in 605 ms
real: /guardian live status 404, page says so
real: /hermes 200 14677 B in 755 ms (live /api/hermes/history 404: Retired)
real: /hermes live 404, page says the thread list did not load
real: nerve after: cycle 55 mode live components 14 -> ["hermes-rs:8642 healthy", "galion-desk:8646 healthy", "galion-one:8091 healthy", "console:8080 healthy"]
real: stopped galion-console-static
real: stopped ssh forward
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 24.16s
ok
real: forward 127.0.0.1:22341->8080 127.0.0.1:22343->8670 (ssh -N, GET only)
real: nerve before: cycle 55 mode live components 14 -> ["hermes-rs:8642 healthy", "galion-desk:8646 healthy", "galion-one:8091 healthy", "console:8080 healthy"]
real: live /api/v1/markers 404 (Retired)
real: /instances 200 14818 B in 568 ms
real: /instances live The machine list 404 (Retired)
real: /instances live The laptop status 404 (Retired)
real: /keys 200 14823 B in 377 ms
real: /keys live 404 (), page says: The key list did not load: HTTP 404.
real: /markers 200 14432 B in 564 ms
real: /markers live 404 (Retired), page names it
real: /instances x5 ms [789, 793, 604, 748, 793]
real: /keys x5 ms [602, 563, 792, 603, 566]
real: /markers x5 ms [561, 561, 561, 561, 567]
real: nerve after: cycle 55 mode live components 14 -> ["hermes-rs:8642 healthy", "galion-desk:8646 healthy", "galion-one:8091 healthy", "console:8080 healthy"]
real: stopped galion-console-static
real: stopped ssh forward
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 19.84s
ok
real: VPS probe 3011 ms: 1799 tree entries; find counts l0 249 l1 215 l2 249 l3 34907 B scenes 66 refs 940 canvases 17
real: mirror built: 42 dirs, 1757 files (names anonymised), persona 34907 B head 1600 B, USER.md placeholder 1280 B
real: forward 127.0.0.1:22341->8080 127.0.0.1:22342->8670 (ssh -N, GET only)
real: nerve before: cycle 55 mode live components 14 -> ["hermes-rs:8642 healthy", "galion-desk:8646 healthy", "galion-one:8091 healthy", "console:8080 healthy"]
real: /memory 200 15466 B in 377 ms
real: /memory = VPS find on all 7 rows; model "galion"; excerpt 400 UTF-16 units, fnv 59fc4d94b4cc5d82 (text not printed)
real: /md/persona shows "persona.md · 34907 bytes · 2026-10-18 21:40 CEST (+02:00)" (= VPS stat + TZ=Europe/Zagreb date)
real: /md/user shows "USER.md · 1280 bytes · 2026-10-18 23:41 CEST (+02:00)" (= VPS stat + TZ=Europe/Zagreb date)
real: live /md/user on a loopback Host: 404 886 B (Retired), no file page (the box binary above serves it from --user-md)
real: /mesh 200 14033 B in 374 ms; live /api/mesh 404 (Retired)
real: /mesh page says "Mesh status did not load: HTTP 404." and Cold.
real: public host unpaired, live /api/v1/admin 404 (Retired) -> 503 fail-closed gate for all 3 rooms, no data
real: nerve after: cycle 55 mode live components 14 -> ["hermes-rs:8642 healthy", "galion-desk:8646 healthy", "galion-one:8091 healthy", "console:8080 healthy"]
real: stopped galion-console-static
real: stopped ssh forward
real: removed the mirror (true)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.41s
ok
ok
real: forward 127.0.0.1:22341->8080 127.0.0.1:22342->8646 127.0.0.1:22343->8670 127.0.0.1:22344->8091 (ssh -N, GET only); live console galion-console-static 0.1.0
real: nerve before: cycle 55 mode live -> ["hermes-rs:8642 healthy", "galion-desk:8646 healthy", "galion-one:8091 healthy", "console:8080 healthy"]
real: path                         live candidate  rooms
real: /api/v1/health                200       200  /gateway /node /status (Board)
real: /api/v1/ping                  200       200  /gateway (Board)
real: /api/v1/domains               404       200  /gateway /status (Board)
real: /api/v1/services              404       200  /gateway /services (Board)
real: /api/v1/repos                 404       200  /gateway (Board)
real: /api/v1/markers               404       200  /gateway /markers (Board)
real: /api/v1/ledger                404       200  /gateway (Board)
real: /api/v1/catalog               404       200  /gateway /catalog (Board)
real: /api/v1/docs                  404       200  /catalog (Board)
real: /api/v1/protocol              404       200  /catalog (Board)
real: /api/v1/protocol/snapshot     404       200  /gateway (Board)
real: /api/v3/guardian/status       404       404  /guardian (Retired)
real: /api/hermes/history           404       404  /hermes (Retired)
real: /api/hermes/health            404       404  /security (Retired)
real: /api/hermes/models            404       404  /agents (Retired)
real: /api/hermes/modes             404       404  /agents (Retired)
real: /api/hermes/guard             404       404  /security (Retired)
real: /api/v1/instances             404       404  /instances (Retired)
real: /api/v1/laptop                404       404  /instances (Retired)
real: /api/v1/keys                  404       404  /keys (Retired)
real: /api/mesh                     404       404  /mesh (Retired)
real: deploy delta (candidate 200, live 404): 9 path(s) ["/api/v1/domains", "/api/v1/services", "/api/v1/repos", "/api/v1/markers", "/api/v1/ledger", "/api/v1/catalog", "/api/v1/docs", "/api/v1/protocol", "/api/v1/protocol/snapshot"]
real: candidate /markers: Ring of 42 · 0 stored (= its /api/v1/markers)
real: candidate /gateway: every GET endpoint shows 200 from its own local API
real: candidate /context: 65536 live · 262144 window (= live desk)
real: candidate /hermes -> The thread list (/api/hermes/history) did not load.
real: candidate /instances -> The machine list did not load: HTTP 404.
real: candidate /instances -> The laptop status did not load: HTTP 404.
real: candidate /keys -> The key list did not load: HTTP 404.
real: candidate /mesh -> Mesh status did not load: HTTP 404
real: candidate /guardian -> Guardian status did not load: HTTP 404
real: nerve after: cycle 55 mode live -> ["hermes-rs:8642 healthy", "galion-desk:8646 healthy", "galion-one:8091 healthy", "console:8080 healthy"]
real: stopped galion-console-static (candidate)
real: stopped ssh forward
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 23.89s
```

## Owner decisions

- **D-364-1** Deploy a console candidate containing 9c0f9d1 (board parity) to
  :18080 then :8080 (cf. D-343-1, console-deploy-candidate 4f763ade) so the 9
  board paths stop 404ing live. Not done here (no deploy).
- **D-364-2** Retired Node routes (hermes x5, guardian, instances, laptop, keys,
  mesh): port them to Rust, or drop the rooms from the V2 nav. Routing /api/hermes/*
  to hermes-rs would need the gateway API key in the console process.
- real_port5 still needs its ssh-target env (not in scope of this row).
