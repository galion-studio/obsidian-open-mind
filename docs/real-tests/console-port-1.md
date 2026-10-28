<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: console-port-1 (`/analytics`, `/arena`, `/bank`, `/cache`)

Box, Europe/Zagreb (CEST +02:00). Release binary `galion-console-static --ui rust --desk`, slot-135 ports
20050-20059. VPS reached through one `ssh -N` forward (8080 console API, 8646 galion-desk, 8670 nerve). GET only:
no POST was sent anywhere (the front's access log is checked), no `/api/v1/markers/stream`, no `/api/v1/ping`.
The only process started on the VPS side is the live console's own `arena.py status` behind `GET /api/arena`
(read-only: it reads the board, spend and champion files; checked in the source, read-only, before running).

```text
GALION_REAL=1 PORT1_VPS=slavik-vps cargo test -p console-ui --release --test real_port1 -- --ignored --nocapture
```

## Restart runs (slot 135 resumed), 2026-10-21 05:41-05:44 +02:00 (all pass)

Run on the resumed branch (same code plus section 4e: methods and gate against the live-backed front).
`PORT1_VPS=slavik-vps` (ssh alias; the address is not in the code or this doc).

- Run A 05:41:32 → 05:41:56: pass (same numbers as below without the 4e line).
- Run B 05:42:54 → 05:43:19, with 4e:

```text
real: start 2026-10-21T05:42:54+02:00
real: binary 685896 bytes
real: forward 127.0.0.1:20059->8080, 127.0.0.1:20058->8646, 127.0.0.1:20057->8670 (ssh -N, read-only)
real: live GET /api/v1/analytics -> 401 0.738 s
real: GET /analytics -> 200 14896 B 0.841 s, <script>: 0, style=: 0
real: GET /analytics?board=overview -> 200 14896 B 0.839 s, <script>: 0, style=: 0
real: GET /analytics?board=systems -> 200 14896 B 0.845 s, <script>: 0, style=: 0
real: GET /analytics?board=infrastructure -> 200 14896 B 0.840 s, <script>: 0, style=: 0
real: GET /analytics?board=services -> 200 14896 B 0.840 s, <script>: 0, style=: 0
real: GET /analytics?board=agents -> 200 14896 B 0.840 s, <script>: 0, style=: 0
real: GET /analytics?board=users -> 200 14896 B 0.843 s, <script>: 0, style=: 0
real: GET /analytics?board=workforce -> 200 14896 B 0.844 s, <script>: 0, style=: 0
real: live GET /api/arena -> 200 4039 B 0.854 s
real: GET /arena -> 200 17520 B 0.843 s, <script>: 0, style=: 0
real: /arena spend "2/3 generations · $0.00 / $2", rows 12 (live) = 12 (page), champion none
real: live GET /api/v1/bank (Accept: application/json) -> 500 "{\"error\":\"Only HTML requests are supported here\"}"
real: GET /bank -> 200 15278 B 0.844 s, <script>: 0, style=: 0
real: live GET :8646/desk -> 200 10105 B 0.786 s
real: GET /cache -> 200 27593 B 0.804 s, <script>: 0, style=: 0
real: /cache headline "109.1M read · 1.2M write · 1142 files · 16.7 MB"
real: /cache cached 35, stores 10, models 6, recent 8, meters 10
real: POST/PUT/DELETE x4 pages -> 405 (Allow: GET, HEAD); HEAD x4 -> 200 empty; unpaired console host -> pairing page, no live data; public host -> Soon online.
real: nerve GET :8670/state -> 200 13090 B 0.797 s, 15 top-level keys
real: live TS console GET /analytics -> Some((500, 49))
real: live TS console GET /arena -> Some((500, 49))
real: live TS console GET /bank -> Some((500, 49))
real: live TS console GET /cache -> Some((500, 49))
real: front RSS 2876 kB
real: done 2026-10-21T05:43:19+02:00
real: stopped front
real: stopped ssh forward
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 25.25s
```

- Regression `real_console_ui` 05:43:40 (ports 20051-20053): pass, `/analytics` 200 14896 B, `/arena` 200 17520 B,
  `/bank` 200 15278 B, `/cache` 200 15167 B (no `--desk` in that test: empty state), 0 `<script>`, "17 Rust pages,
  12 rooms answer Not in Rust yet".
- Live data unchanged between 05:07 and 05:43: arena 2/3 generations, $0.00 of $2, 12 rows, no champion; desk
  109.1M read / 1.2M write / 1142 files / 16.7 MB; nerve /state 200 with 15 top-level keys (cortex not deployed,
  so the nerve was the observation point). Front RSS 2.8-2.9 MB.
- Box checks at 05:40-05:44 +02:00: `cargo fmt --check` ok; `clippy --workspace --all-targets -D warnings`
  (excl. openmind-sandbox, cgroup-detect) ok; `cargo test --workspace` (same exclusions) 556 passed / 0 failed;
  `scripts/no-python.sh` ok; repo-check 783 files 0 errors 43 warnings (pre-existing, none in console-*);
  x86_64 + aarch64 musl release build ok; armv7 musleabihf + aarch64-linux-android `cargo check` ok.
- Trial merge with slot 136 `swarm/console-port-2` 0916684 (scratch clone, not pushed): 5 files conflict exactly as
  listed in docs/console-port-1.md "Merge notes"; resolved that way, clippy -D warnings ok and console-ui +
  console-static tests all pass (console-ui 59 unit, ui 11, ui_port1 6, ui_port2 8).

## First run (earlier agent), 2026-10-21 05:07:48 → 05:08:13 +02:00 (pass)

```text
real: start 2026-10-21T05:07:48+02:00
real: binary 685880 bytes
real: forward 127.0.0.1:20059->8080, 127.0.0.1:20058->8646, 127.0.0.1:20057->8670 (ssh -N, read-only)
real: live GET /api/v1/analytics -> 401 0.752 s
real: GET /analytics -> 200 14896 B 0.856 s, <script>: 0, style=: 0
real: GET /analytics?board=overview -> 200 14896 B 0.856 s, <script>: 0, style=: 0
real: GET /analytics?board=systems -> 200 14896 B 0.856 s, <script>: 0, style=: 0
real: GET /analytics?board=infrastructure -> 200 14896 B 0.852 s, <script>: 0, style=: 0
real: GET /analytics?board=services -> 200 14896 B 0.856 s, <script>: 0, style=: 0
real: GET /analytics?board=agents -> 200 14896 B 0.852 s, <script>: 0, style=: 0
real: GET /analytics?board=users -> 200 14896 B 0.852 s, <script>: 0, style=: 0
real: GET /analytics?board=workforce -> 200 14896 B 0.851 s, <script>: 0, style=: 0
real: live GET /api/arena -> 200 4039 B 0.873 s
real: GET /arena -> 200 17520 B 0.867 s, <script>: 0, style=: 0
real: /arena spend "2/3 generations · $0.00 / $2", rows 12 (live) = 12 (page), champion none
real: live GET /api/v1/bank (Accept: application/json) -> 500 "{\"error\":\"Only HTML requests are supported here\"}"
real: GET /bank -> 200 15278 B 0.860 s, <script>: 0, style=: 0
real: live GET :8646/desk -> 200 10105 B 0.565 s
real: GET /cache -> 200 27593 B 0.816 s, <script>: 0, style=: 0
real: /cache headline "109.1M read · 1.2M write · 1142 files · 16.7 MB"
real: /cache cached 35, stores 10, models 6, recent 8, meters 10
real: nerve GET :8670/state -> 200 13138 B 0.806 s, 15 top-level keys
real: live TS console GET /analytics -> Some((500, 49))
real: live TS console GET /arena -> Some((500, 49))
real: live TS console GET /bank -> Some((500, 49))
real: live TS console GET /cache -> Some((500, 49))
real: front RSS 2932 kB
real: done 2026-10-21T05:08:13+02:00
real: stopped front
real: stopped ssh forward
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 25.30s
```

What it proves against the live data:
- `/analytics`: the live API refuses without the paired-phone cookie (401, as the TS route `requireAdmin` does);
  the Rust page shows the error plus why, all 7 board links render. With a cookie the boards render (e2e test
  with a fake API; no phone cookie was used or copied for the live run).
- `/arena`: spend line identical to the live `/api/arena` payload, 12 of 12 live rows on the page, newest first,
  no action forms.
- `/bank`: the live console has no `/api/v1/bank` (TanStack answers a JSON GET of an unknown path with
  `500 {"error":"Only HTML requests are supported here"}`); the page shows the TSX empty state and says why.
- `/cache`: headline numbers computed from the live `/desk` payload match the page (109.1M read, 1.2M write,
  1142 files, 16.7 MB); 10 stores = 10 bars, all 6 model names present.
- Nerve observed during the run: `GET :8670/state` 200, ~13 KB, 15 top-level keys (cortex not confirmed live,
  so the nerve was used as the standing rule says).
- Live TS console, same paths, with the Rust client's `Accept: application/json`: 500 (49 B, the TanStack JSON
  refusal); with `curl` (HTML) from the VPS: `/bank` 200 3433 B, `/arena` 200 3437 B, `/analytics` 200 3453 B,
  SPA shells that load their data in the browser, so there is no server-side HTML to diff against; the comparison
  is payload-to-page above.
- Front RSS ~2.9 MB; ~0.85 s per page = mostly the box→VPS round trip of the forward (each page does 2 upstream
  GETs in parallel).

## Regression: the earlier real test with the 4 new pages included (pass)

`real_console_ui` (c242fed's test), run 2026-10-21 05:04-05:05 +02:00 on ports 20051-20053
(`CONSOLE_UI_PORT/UPSTREAM/PULSE` overrides):

```text
real: GET /console -> 200 31664 B 0.843 s, <script> tags: 0
real: GET /analytics -> 200 14896 B 0.842 s, <script> tags: 0
real: GET /arena -> 200 17520 B 0.840 s, <script> tags: 0
real: GET /bank -> 200 15283 B 0.854 s, <script> tags: 0
real: GET /cache -> 200 15167 B 0.961 s, <script> tags: 0
real: 17 Rust pages, 12 rooms answer "Not in Rust yet": /instances /hermes /context /memory /mesh /gateway /security /guardian /keys /node /markers /session
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 36.41s
```

## Screenshots

Headless Chrome, 412x915 and 1280x900, front on 20050 against the live forward, 2026-10-21 05:05 +02:00:
`/workspace/swarm/artifacts/console-port-1/{analytics,arena,bank,cache,cache-wide}.png` (box only, not committed).
