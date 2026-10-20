<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: swarm/one-hardening (galion-one)

All times Europe/Zagreb (CEST, +02:00). Box ports 19290-19299 only (slot 29).
Fixture data only (generated pages, a fixture device store whose code is
`FIXTURE42`); no live data or secrets were copied. The live comparison was
plain HTTPS GET to https://one.galion.app with no cookie, no pairing, no POST.
Nothing was deployed and the VPS was not touched. Every process started was
killed at the end (checked: no listener on 19290-19299, no leftover
galion-one or decoy process).

## 1. Builds

```
$ CARGO_TARGET_DIR=/workspace/swarm/target/one-hardening nice -n 10 cargo build -j 3 --release -p galion-one --target x86_64-unknown-linux-musl
$ CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld ... cargo build -j 3 --release -p galion-one --target aarch64-unknown-linux-musl   -> Machine: AArch64
$ cargo check -p galion-one --target aarch64-linux-android            -> ok
$ cargo check -p galion-one --target armv7-unknown-linux-musleabihf   -> ok
```

x86_64 musl: 0 `NEEDED` entries (static), 841024 bytes (b44cd4f: 832768).
The "before" binary was built from b44cd4f in a separate scratch worktree
with the same command.

## 2. `NO_BUILD=1 OLD_BIN=<b44cd4f build> scripts/real/one-hardening.sh`

Part 1 is `crates/galion-one/tests/real_one_hardening.rs` run against the
release binary: instance A on 19290 (TZ=Europe/Zagreb, fake `nvidia-smi`
script, console probe moved to a stand-in on 19292 with path `/health`, fake
ear on 19293, no mind on 19294) and instance B on 19295 (TZ=UTC,
`nvidia-smi` absent from PATH, `ONE_CONSOLE=off`, empty secrets dir, no
store). A renamed `sleep` shows up in /proc as `cloudflared tunnel run`.

```
== start 2026-10-20T20:47:57+02:00
== binary: 0 NEEDED, sha256 47118dccc19b6157, 841024 bytes
== Rust real test (release binary)
B: 3 failed /pair, secrets dir still empty
A: wrong code -> no, right code -> cookie, /me you:true
A /api/v1/awake: {"cpu":0,"memory":21,"disk":0,"cores":8,"gpu":1,"services":[{"id":"console","ok":true,"ms":0},{"id":"hermes","ok":false,"ms":0},{"id":"whisper","ok":true,"ms":0},{"id":"voice","ok":false,"ms":0},{"id":"vision","ok":false,"ms":0},{"id":"one","ok":true,"ms":0}]}
A analytics at: 2026-10-20T20:47:59+02:00
A infrastructure: {"cpu":0,"load":5.21,"cores":8,"memory":21,"memUsed":3593564160,"memTotal":16779640832,"disk":{"pct":0,"used":0,"total":107374182400},"gpu":{"name":"Fake GPU 19290","pct":6,"util":42,"usedMb":512,"totalMb":8192}}
A analytics/services: [{"id":"console","port":19292,"ok":true,"ms":0,"note":"up"},{"id":"hermes","port":19294,"ok":false,"ms":0,"note":"down"},{"id":"whisper","port":19293,"ok":true,"ms":0,"note":"up"},{"id":"voice","port":19291,"ok":false,"ms":0,"note":"down"},{"id":"vision","port":19294,"ok":false,"ms":0,"note":"down"},{"id":"one","port":19290,"ok":true,"ms":0,"note":"up"}]
A /api/v1/services: {"services":[{"id":"hermes-rs","port":19294,"ok":false,"ms":0},{"id":"openmind-memory","port":19294,"ok":false,"ms":0},{"id":"link-watch-rs","port":null,"ok":false,"ms":0}]}
A session.jsonl: {"id":"one-voice","unix":1792522079,"at":"2026-10-20T20:47:59+02:00","heard":"hello from the fake ear","said":"No mind on this machine."}
B /api/v1/awake: {"cpu":0,"memory":21,"disk":0,"cores":8,"gpu":0,"services":[{"id":"hermes","ok":false,"ms":0},{"id":"whisper","ok":true,"ms":0},{"id":"voice","ok":false,"ms":0},{"id":"vision","ok":false,"ms":0},{"id":"one","ok":true,"ms":0}]}
.
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.05s
== box 19297, unpaired
GET / -> 200 text/html; charset=utf-8 | sha=d03ee511c40c bytes=44
GET /index.html -> 200 text/html; charset=utf-8 | sha=d03ee511c40c bytes=44
GET /nerve -> 200 text/html; charset=utf-8 | sha=74894a4cac57 bytes=44
GET /admin -> 200 text/html; charset=utf-8 | sha=d5cd35f84101 bytes=44
GET /space -> 200 text/html; charset=utf-8 | sha=6be91359cfd7 bytes=44
GET /mark.svg -> 200 image/svg+xml | sha=3a18fec85360 bytes=60
GET /font/jost.woff2 -> 200 font/woff2 | sha=1c082eb90441 bytes=11
GET /health -> 200 text/plain | ok 
GET /link -> 200 application/json | {"ear":"127.0.0.1:19294","mouth":"127.0.0.1:19298","mind":"127.0.0.1:19294","eye":"127.0.0.1:19294"}
GET /me -> 200 application/json | {"you":false}
GET /api/v1/awake -> 200 application/json | {"cpu":43,"memory":21,"disk":0,"cores":8,"gpu":0,"services":[{"id":"console","ok":false,"ms":0},{"id":"hermes","ok":false,"ms":0},{"id":"whisper","ok":false,"ms":0},{"id":"voice","ok":false,"ms":0},{"id":"vision","ok":false,"ms":0},{"id":"one","ok":true,"ms":0}]}
GET /api/v1/catalog -> 200 application/json | [] 
GET /api/v1/services -> 200 application/json | {"services":[{"id":"hermes-rs","port":19294,"ok":false,"ms":0},{"id":"openmind-memory","port":19294,"ok":false,"ms":0},{"id":"link-watch-rs","port":null,"ok":false,"ms":0}]}
GET /api/v1/analytics -> 401 application/json | {"error":"pair"}
GET /api/v1/analytics/services -> 401 application/json | {"error":"pair"}
GET /api/v1/analytics/health -> 401 application/json | {"error":"pair"}
GET /session -> 401 application/json | {"error":"pair"}
GET /files -> 401 application/json | {"error":"pair"}
GET /files/abc -> 401 application/json | {"error":"pair"}
GET /model -> 401 application/json | {"error":"pair"}
GET /models -> 401 application/json | {"error":"pair"}
GET /pair -> 404 text/plain | not found 
GET /nope -> 404 text/plain | not found 
== failed pairing on 19297 (no store): secrets dir entries before/after
  before: 0
{"error":"no"}
  after: 0
== live https://one.galion.app, unpaired, GET only (2026-10-20T20:48:03+02:00)
GET / -> 200 text/html; charset=utf-8 | sha=6cae1b46d032 bytes=206340
GET /index.html -> 200 text/html; charset=utf-8 | sha=5b1cef8eeb93 bytes=206340
GET /nerve -> 200 text/html; charset=utf-8 | sha=8b18e61c163f bytes=3038
GET /admin -> 200 text/html; charset=utf-8 | sha=6b03a8a1033b bytes=10831
GET /space -> 200 text/html; charset=utf-8 | sha=ee20222d8a80 bytes=12562
GET /mark.svg -> 200 image/svg+xml | sha=7c80b9a82fa8 bytes=453
GET /font/jost.woff2 -> 200 font/woff2 | sha=f94cffda1651 bytes=9404
GET /health -> 200 text/plain | ok 
GET /link -> 200 application/json | {"ear":"127.0.0.1:8643","mouth":"127.0.0.1:8002","mind":"127.0.0.1:8642","eye":"127.0.0.1:8645"}
GET /me -> 200 application/json | {"you":false}
GET /api/v1/awake -> 200 application/json | {"cpu":7,"memory":10,"disk":0,"cores":8,"gpu":0,"services":[{"id":"console","ok":true,"ms":1},{"id":"hermes","ok":true,"ms":0},{"id":"whisper","ok":false,"ms":0},{"id":"voice","ok":false,"ms":0},{"id":"vision","ok":false,"ms":0},{"id":"one","ok":true,"ms":0}]}
GET /api/v1/catalog -> 200 application/json | [   {     "kind": "file",     "place": "box",     "name": "photo.jpg",     "note": "A picture on the pod.",     "read": "81 KB. Kept under share/image. It is not sent whole into the voice."   },   {     "kind": "file",     "place": "box",     "name": "photo.jpg",     "note": "The same picture, kept 
GET /api/v1/services -> 200 application/json | {"services":[{"id":"hermes-rs","port":8642,"ok":true,"ms":0},{"id":"openmind-memory","port":8420,"ok":true,"ms":0},{"id":"link-watch-rs","port":null,"ok":true,"ms":0}]}
GET /api/v1/analytics -> 401 application/json | {"error":"pair"}
GET /api/v1/analytics/services -> 401 application/json | {"error":"pair"}
GET /api/v1/analytics/health -> 401 application/json | {"error":"pair"}
GET /session -> 401 application/json | {"error":"pair"}
GET /files -> 401 application/json | {"error":"pair"}
GET /files/abc -> 401 application/json | {"error":"pair"}
GET /model -> 401 application/json | {"error":"pair"}
GET /models -> 401 application/json | {"error":"pair"}
GET /pair -> 404 text/plain | not found 
GET /nope -> 404 text/plain | not found 
== diff (path status content-type), box vs live
  no differences
== old binary (<b44cd4f release musl>) on 19299, same failed pairing, no store
{"error":"no"}
  old: secrets dir entries after: 1 admin-device.json
  old analytics at: "at":"18:48:21"  (box date: 2026-10-20T20:48:21+02:00)
== new binary, same: analytics at on 19297
  new analytics at: "at":"2026-10-20T20:48:21+02:00"  (box date: 2026-10-20T20:48:21+02:00)
== stderr 19297: one: listening voice: listening 
== done 2026-10-20T20:48:21+02:00
```

## 3. What it shows

| item | before (b44cd4f) | after (this branch) |
|---|---|---|
| analytics `at` with `TZ=Europe/Zagreb` at 20:48:21 CEST | `"18:48:21"` (UTC time of day, no offset) | `"2026-10-20T20:48:21+02:00"` |
| `one/session.jsonl` turn | `"at":"HH:MM:SS"` UTC (from the old `clock()` code; not re-run) | `"unix":1792522079,"at":"2026-10-20T20:47:59+02:00"` (unix kept) |
| failed `POST /pair` with no store | `admin-device.json` created in the secrets dir | `{"error":"no"}`, secrets dir stays empty (3 attempts on B, 1 on 19297) |
| GPU with nvidia-smi (fake, A) | - | `awake.gpu:1`, infrastructure `gpu:{"name":"Fake GPU 19290",...,"util":42}` |
| GPU without nvidia-smi (B) | - | `awake.gpu:0`, `/health` `ok`, no hang |
| console pulse | fixed `127.0.0.1:8080` `/` | A: `console` port 19292 ok:true via `/health` (stand-in hit); B (`off`): no console entry |
| unit scan | `ps -eo args` | `/proc/*/cmdline`: `{"id":"cloudflare","ok":true,"note":"tunnel"}` from the decoy |
| `/api/v1/services` | same shape | `{"services":[{"id":"hermes-rs","port":19294,...},{"id":"openmind-memory",...},{"id":"link-watch-rs","port":null,...}]}` |

**Box vs live one.galion.app (unpaired GETs, 23 routes):** status code and
content type identical for every route (`diff`: no differences). Bodies
differ only where they are fixture data (pages, catalog) or live values
(`/link` ports, `/api/v1/awake` numbers and live console/hermes up,
`/api/v1/services` ports). Paired routes were not compared with live (no
live cookie, on purpose).

Not covered live: the voice path past the ear (no real Hermes/mouth on the
box; the turn ends at "No mind on this machine."). Voice settings were not
changed.

## 4. Gates

```
cargo fmt --check                                   ok
cargo clippy --workspace --all-targets -D warnings  ok
cargo test --workspace                              525 passed, 0 failed (123 suites)
repo-check --root .                                 0 errors -> PASS
scripts/spdx-scan.sh                                missing 0
scripts/no-python.sh                                ok
```
