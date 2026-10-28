<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# console-dist-arena-bank (wave 4 slot 336) — D-321-1

Branch `swarm/console-dist-arena-bank` off `swarm/console-v2-merge-2` cd1204a0.
Console V2 prep only: NOT the live cutover, NO deploy, no VPS writes. Box ports 22060-22069.

## Problem (D-321-1)
`galion-console-static --dist DIR --ui rust` (the live :8080 shape) loops its UI API to
itself (self-upstream) and clears every proxy prefix, so `/api/arena` and `/api/v1/bank`
had no source: the V2 `/arena` and `/bank` pages always showed the empty state.
The slot 321 suggestion (`--upstream 127.0.0.1:18080 --proxy-prefix /api/arena ...`) does
not work as written: `--proxy-prefix` must end in `/`, and `--upstream` would also move
every other UI API read (health, board) off the in-process local API. And the live primary
:18080 itself answers 404 for both routes (see below), so a plain proxy would show nothing.

## Change
- `--arena-root DIR` (absolute, needs `--local-api`/`--dist`): in-process, read-only
  `GET/HEAD /api/arena` = Rust port of `arena.py status` (`crates/console-static/src/arena.rs`):
  last 16 lines of `board.jsonl` (blank/bad lines skipped), `champions/current.json`
  (null if missing/bad), `spend.json` (script default if missing/bad), `STOP` present.
  Same keys and order: `ok, stopped, spend, champion, rows, hermes, note`.
  Non-GET -> 405 `{"ok":false,"error":"read-only: arena actions ... held ..."}` (Breed is a paid
  run; Promote/Stop not ported). Unreadable file -> 500 `{"ok":false,"error":"arena read failed"}`
  (no path / OS text). Board > 1 MiB: only its tail is read. Runs nothing (the Node route ran
  `python3 arena.py`).
- `--data-upstream ADDR` (loopback, needs `--local-api`, must not be `--bind`) +
  `--data-path P` (exact path under `/api/`, repeatable; default `/api/arena`, `/api/v1/bank`):
  data routes the local API did not answer are proxied GET/HEAD-only to a reader, AFTER the
  API gate (foreign host 401, loopback Host from remote peer 401, remote console host needs
  admin cookie). Non-GET -> 405 without opening the upstream. Exact paths only (`/api/v1/bankx`,
  `/api/arena/x`, `..` tricks are 404 and never proxied). Works with `--dist` self-upstream.
- Precedence: local arena reader > data upstream > 404. `/api/v1/bank` is never answered
  locally: the console reads no key file and calls no provider (bank.ts needed provider keys +
  OpenRouter/River calls + root-only `hermes/state.db`).
- `--help` text documents the three flags. No new dependencies.

## VPS read-only baseline (2026-10-28 01:14 CET +01:00)
```
:18080 /api/arena    404 "Not Found"      :8080 /api/arena    404
:18080 /api/v1/bank  404 "Not Found"      :8080 /api/v1/bank  404
:18080 /api/v1/health 200 galion-console-static local-api (same on :8080)
console, console-rust-primary: active (both galion-console-serve-2d846fb8, User= root)
/workspace/arena: board.jsonl 12 lines (4152 B, no key/token strings), no champions/,
no spend.json, no STOP.  /opt/galion/hermes/state.db root 0600, last write 2026-10-18.
```
Nerve 01:32:35 CET (+01:00): `:8670/state` 200 36515 B, console (:8080, console.service) `healthy`,
restarts_1h 0, unit active. Temp files /tmp/.s336* on the VPS created and removed. Nothing changed.
(So the slot 321 "live upstream 200" at 23:05 is not what the live primary answers now.)

## Box real tests (release), 2026-10-28 ~01:35 CET +01:00
`GALION_REAL=1 CONSOLE_ARENA_LIVE=1 cargo test --release -p console-static --test real_dist_arena_bank -- --nocapture --test-threads=1`
```
real_dist_arena_live_readonly:
  live board.jsonl: 12 lines, 12 ids in tail
  live :18080 /api/arena -> 404 "Not Found"
  live :18080 /api/v1/bank -> 404 "Not Found"
  box /api/arena from live board -> 200 4039 bytes 0 ms, ids 12/12
  box /arena from live board -> 200 17198 bytes 1 ms
  box /api/v1/bank via live :18080 -> 404; /bank 14949 bytes 618 ms
real_dist_data_upstream_proxy:
  proxy GET /api/v1/bank -> 200 325 bytes 0 ms   (fake BankSnap reader on :22062)
  /bank renders OpenRouter $9.50 (not "route yet"); /arena renders reader board
  9 non-proxy cases (POST/DELETE, evil host, unpaired console.galion.app, /bankx, ..,
  /api/v1/keys, /api/arena/x) -> 401/404/405, reader saw 0 of them
  reader saw 4 requests, all GET (query passed through)
real_dist_local_arena_reader:
  local GET /api/arena -> 200 1873 bytes 0 ms, 14 rows (22 lines -> last 16 -> 14 JSON)
  local GET /arena -> 200 16024 bytes (champion + rows, no <script>/<form>)
  POST/DELETE/PUT /api/arena -> 405, board file unchanged; POST /arena -> 405
  evil.example / unpaired console.galion.app -> 401, no board leak on remote page
  STOP + broken spend -> stopped:true + default spend; non-UTF-8 spend -> 500 no path,
  page says "HTTP 500"; /api/v1/bank without reader -> 404, page "route yet"
  200 requests, worst 1 ms
test result: ok. 3 passed; 0 failed
```
Live leg = `ssh cat /workspace/arena/board.jsonl` (read-only) into a temp root + `ssh -L 22069 ->
VPS 127.0.0.1:18080` as `--data-upstream`; GETs only. Bank passthrough body/status equal live.

Negative control: with the GET/HEAD-only check disabled in server.rs,
`real_dist_data_upstream_proxy` FAILS (`POST /api/v1/bank` reached the proxy path -> 413).

Regression (GALION_REAL release): real_arena_bank 3/3, real_v2_merge 1/1, real_landing 1/1,
real_board_api_parity 1/1, real_console_rust_serve 1/1, real_pairing_e2e 1/1,
real_sprite_health 1/1. real_console_static FAILS before and after this change for an
environment reason only: `/workspace/swarm/artifacts/console-static/out` (prebuilt Node console)
is not on the restored box.

## Owner decisions
- **D-336-1** Cutover flags: add `--arena-root /workspace/arena` to the `--dist` unit (live
  board then shows 12 rows instead of "no route"). Read-only; root already owns the files.
- **D-336-2** `/api/v1/bank` source: who reads provider balances (needs OpenRouter/River keys
  and the stale root-only `hermes/state.db`). Proposal: a Rust bank reader inside hermes-rs
  (it already holds the keys) on loopback, plugged in with `--data-upstream <hermes-rs>
  --data-path /api/v1/bank`; the console stays key-free. Until then `/bank` = honest empty state.
- **D-336-3** Arena Breed/Promote/Stop stay held (405) until Slavik approves the paid run.

## Gates (box, 2026-10-28 ~01:30 CET +01:00)
- `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets --exclude openmind-sandbox
  --exclude cgroup-detect -- -D warnings` ok.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`:
  142 suites, 697 passed, 0 failed, 7 ignored. console-static lib 57/0 (arena 5 new, api 1 new,
  config 1 new).
- x86_64 musl release `galion-console-static` 1033088 B (sha256 75898d396bfd2112...),
  aarch64 musl (rust-lld) 874344 B; `cargo check` aarch64-linux-android, armv7-linux-androideabi,
  armv7-unknown-linux-musleabihf ok. `scripts/no-python.sh` ok. No new deps.
- Not in this tip (carried from base cd1204a0): the F-313-1 admin lockout fix lives on
  console-admin-lockout-fix dfa120c6 / console-v2-admin-fold c7cebb28; fold this branch there
  before any deploy.
