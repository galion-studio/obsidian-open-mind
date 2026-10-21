<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real-life tests: galion-desk + desk-hermes + link-watch (slot 18)

- Branch `swarm/desk-linkwatch`, box only, ports 19180-19189.
- Command: `REAL_LONG=1 scripts/real/desk-linkwatch.sh` (builds the release
  binaries, starts the stack, probes it, runs the gated Rust real tests, and
  stops everything). Without `REAL_LONG=1` it skips the 60 s rate-limit refresh
  wait.
- Run: 2026-10-20 19:07:36 to 19:08:41 +02:00 (Europe/Zagreb). Result: **PASS**.
- Public GETs made by link-watch across all runs today: https://example.com/
  and https://one.galion.app/, one GET each per 60 s at most, 4 to 5 GETs per
  site in total. GET only, no redirects followed, body discarded.

## Wiring

| port | process | stands in for |
|---|---|---|
| 19180 | `galion-desk` (release) | live :8646 |
| 19181 | `galion-one` (release, `ONE_NO_WARM=1`) | live :8091 |
| 19182 | `hermes-gateway --mock` (release; mock provider, no paid calls; throwaway inbound key, never printed) | live :8642 |
| 19183 | `galion-one` mouth | live :8002 |
| 19189 | nothing (closed) | Ollama/laptop, tunnel, console, origin, room, tulu |

- `link-watch` (release, loop mode) probes 19189 as Ollama and 19182 as
  Hermes, GETs the public URLs (`LINK_WATCH_URLS`, `LINK_WATCH_URLS_EVERY=60`),
  and writes `link.json` into the desk's state dir.
- `galion-desk` reads that `link.json` and runs the release `desk-hermes` on a
  fixture Hermes home (`cargo run --example fixture_home`; 3 sessions, 5
  messages, known aggregates).
- Every child runs under `env -i` with a minimal PATH, so no provider keys can
  reach it.

## Output (verbatim from the run log)

```
[2026-10-20T19:07:37+02:00] listeners:
  LISTEN 0      128        127.0.0.1:19182      0.0.0.0:*    users:(("hermes-gateway",pid=814100,fd=6))
  LISTEN 0      128        127.0.0.1:19183      0.0.0.0:*    users:(("galion-one",pid=814102,fd=7))    
  LISTEN 0      128        127.0.0.1:19180      0.0.0.0:*    users:(("galion-desk",pid=814106,fd=5))   
  LISTEN 0      128        127.0.0.1:19181      0.0.0.0:*    users:(("galion-one",pid=814102,fd=5))    
[2026-10-20T19:07:37+02:00] desk route matrix (GET):
  /          404 application/json 21B 0.000333s
  /health    200 application/json 11B 0.000300s
  /healthz   200 application/json 11B 0.000221s
  /health/x  200 application/json 11B 0.000213s
  /desk      200 application/json 2571B 0.003056s
  /desk/     200 application/json 2571B 0.002130s
  /deskx     200 application/json 2571B 0.002041s
  /desk?x=1  200 application/json 2571B 0.001922s
  /nope      404 application/json 21B 0.000597s
  /DESK      404 application/json 21B 0.000364s
[2026-10-20T19:07:37+02:00] one/hermes health:
  19181 200
  19182 200
  19183 503
[2026-10-20T19:07:37+02:00] link.json from link-watch:
  {"laptop_port":false,"model":"","hermes":true,"hermes_ms":0,"france":"unconfigured","laptop":"Box test laptop","events":0,"urls":[{"url":"https://example.com/","ok":true,"status":200,"ms":95,"checked_at_local":"2026-10-20T19:07:36+02:00","error":""},{"url":"https://one.galion.app/","ok":true,"status":200,"ms":360,"checked_at_local":"2026-10-20T19:07:36+02:00","error":""}]}
[2026-10-20T19:07:37+02:00] desk snapshot (selected fields):
  {"foundation":"rust","line":false,"hermes":true,"one":true,"tunnel":false,"voice":"jarvis","services":["Console=false","Origin=false","Hermes=true","Laptop=false","One=true","Room=false","Tulu=false","Tunnel=false"],"board":{"studio":"Box test laptop","gpu":"RTX 4060, 8188 MiB","cpu":"i7-13620H","laptop_ram":"20 / 32 GB","hermes_ms":0,"agents":2,"version":"box-real","france":false},"mindpack":{"sessions":3,"messages":5,"line":"Mate builds Galion. Second line","road":2,"tasks":["one-galion","d2","d1"]}}
[2026-10-20T19:07:37+02:00] desk acts (POST):
  note   200 application/json 2625B 0.002139s
    notes: [{"at":1792516057,"text":"real-life note from slot 18"}]
  stamp  200 application/json 2681B 0.002181s
    last note: "Stamp. Line down. Hermes up."
  focus  200 application/json 2695B 0.002382s
    focus: "desk-linkwatch"
  hold   200 application/json 2694B 0.002121s
    hold: true
  hold   200 application/json 2695B 0.002067s
    hold: false
  drop   200 application/json 2639B 0.002141s
    notes left: 1
  voice  200 application/json 2641B 0.002661s
    voice: "samantha"
[2026-10-20T19:07:37+02:00] gated Rust real tests against the stack:
  test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.21s
  test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
[2026-10-20T19:07:37+02:00] link-watch rate limit (LINK_WATCH_URLS_EVERY=60):
  checked_unix at start: [1792516056,1792516056]; after 9 s (3 more loop passes, last at 2026-10-20T19:07:46+02:00): [1792516056,1792516056]
  after 64 s: [1792516117,1792516117]
  [{"url":"https://example.com/","ok":true,"status":200,"ms":50,"checked_at_local":"2026-10-20T19:08:37+02:00"},{"url":"https://one.galion.app/","ok":true,"status":200,"ms":439,"checked_at_local":"2026-10-20T19:08:37+02:00"}]
[2026-10-20T19:08:41+02:00] process RSS:
  hermes-gateway 4060 kB
  galion-one 3392 kB
  galion-desk 2756 kB
  link-watch 2556 kB
[2026-10-20T19:08:41+02:00] done: PASS
[2026-10-20T19:08:41+02:00] stopped: 814100 814102 814106 814145
```

Notes on the output:
- `19183 503`: the galion-one mouth's `/health` reports the speech backend as
  unavailable (it is a stub). The live :8002 mouth also returns 503 (LIVE.md).
  The desk's `One` row is a TCP probe, so it is `true` on both.
- `hermes_ms: 0`: link-watch rounds loopback latency down to whole
  milliseconds.
- The rate limit held: the check times did not change across 3 more 3 s loop
  passes, then refreshed once after the 60 s interval.

## Gated Rust real tests

`crates/galion-desk/tests/real_wired.rs` (3 tests, `#[ignore]`, run with
`GALION_REAL=1 GALION_REAL_DESK=127.0.0.1:19180 GALION_REAL_STATE=<run>/state`):
- `real_desk_matches_the_live_shape`: the box `/desk` type shape equals the
  live VPS desk's shape (recorded read-only, see `docs/cutover/desk.md`) in both
  directions.
- `real_desk_is_wired_to_link_watch_hermes_one_and_desk_hermes`: Hermes and
  One are up through the box instances; the mindpack has the fixture
  aggregates; the board comes from link-watch's `link.json`; every public URL
  answered over HTTPS with 2xx/3xx.
- `real_desk_note_round_trip_and_routes`: a note is written to
  `desk-notes.jsonl` and dropped again; same route matrix as live.

`crates/galion-desk/tests/live_ports.rs` with `GALION_DESK_LIVE_DESK_PORT=19180
GALION_DESK_LIVE_ONE_PORT=19181 GALION_DESK_LIVE_REQUIRED=1`: both box
listeners answer `/health`; an in-process desk serving `/health` dials
nothing, runs no mindpack, and leaves both listeners alone. Without the
variables it checks 8646/8091 (on the VPS, the live ones) and skips absent
listeners. With `GALION_DESK_LIVE_REQUIRED=1` and nothing listening, it fails
(checked).

## Live VPS probes (read-only)

2026-10-20 18:52:40 to 18:53:50 +02:00, over `ssh` to the VPS:
- `systemctl status galion-desk`: active since 2026-10-12 07:01:36 +02:00,
  pid 231, `/opt/galion/bin/galion-desk`.
- `ss -ltnp`: 8646 galion-desk, 8091/8002 galion-one, 8642 hermes-gateway,
  8080 node, 20241 cloudflared.
- The `GET` matrix on `127.0.0.1:8646` (10 paths; table in
  `docs/cutover/desk.md`), plus one more `GET /desk` whose body was reduced to a
  type shape on the box and then deleted. No values (notes, heard/said, IPs)
  were kept. Each live `GET /desk` makes the live desk fork its own
  `python3 desk-hermes.py`; that is the service's behaviour, not a command
  run by this agent.
- `link-watch-rs` is active and has a France URL configured (host redacted
  here).

## Unit and integration tests

| crate | before (CODE.md) | now |
|---|---|---|
| galion-desk | 4 (GH) / 6+1 env-fail (VPS), source-grep health test | 38 pass + 3 gated real; source-grep tests replaced by behaviour tests |
| desk-hermes | 0 (GH) / 1 source-grep (VPS) | 12 pass (fixture DB golden aggregates, read-only proof, damaged inputs, binary) |
| link-watch | 2 (GH) / 10 (VPS) | 25 pass |

Gates on the branch: `cargo fmt --check`, `cargo clippy --workspace
--all-targets -D warnings`, `cargo test --workspace` (all pass),
`scripts/no-python.sh` ok, `scripts/spdx-scan.sh` missing 0. Release builds for
x86_64-unknown-linux-musl and aarch64-unknown-linux-musl; `cargo check` for
aarch64-linux-android and armv7-unknown-linux-musleabihf (C for the bundled
SQLite compiled with the ucpu agent's zig cc wrappers); aarch64 binaries run
`--help` under qemu-aarch64. The armv7 gnueabihf target is not installed on the
box, so the musleabihf variant was checked instead.

## Ported to Rust (2026-10-21, +02:00)
`scripts/real/desk-linkwatch.sh` was replaced on swarm/realscript-port-1 by the gated Rust test `crates/galion-realtest/tests/real_sh_desk_linkwatch.rs` (same checks; run with `GALION_REAL=1`). The evidence above is from the shell version; the Rust run is in `docs/real-tests/realscript-port-1.md`.
