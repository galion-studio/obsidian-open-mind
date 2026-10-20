<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: fuzz-sast (swarm slot 25, ports 19250-19259)

All times Europe/Zagreb (CEST, +02:00). Box: 8 vCPU shared by the swarm.
Nothing was run against or deployed to the VPS: replaying fuzz inputs at a
live service is not a read-only GET, so the live services were not probed.

## 1. Fuzzing on the box (cargo-fuzz 0.13.1, rustc 1.101.0-nightly 07e526db9 2026-10-19)

Each target ran 660 s (`-max_total_time=660 -timeout=10`), libFuzzer +
AddressSanitizer, `nice -n 15`, two targets at a time. Before these runs every
target had a 20 s shake-out run (that is where finding 1 came from; its
corpus was kept), so "start corpus" below is not just the seeds.

| Target | Window | Total execs | exec/s (avg) | Start corpus | End corpus (units / bytes) | Coverage (cov / ft) | Crashes / timeouts / OOM |
|---|---|---|---|---|---|---|---|
| http_parse_head | 20:41:53 -> 20:52:54 | 18 014 926 | 27 254 | 638 / 81 KB | 824 / 127 KB | 656 / 2 710 | 0 / 0 / 0 |
| mdns_parse | 20:41:53 -> 20:52:54 | 12 840 110 | 19 425 | 347 / 66 KB | 462 / 217 KB | 555 / 2 711 | 0 / 0 / 0 |
| base64_decode | 20:53:04 -> 21:04:05 | 7 962 268 | 12 045 | 83 / 11.7 KB | 87 / 18 KB | 177 / 679 | 0 / 0 / 0 |
| json_escape | 20:53:04 -> 21:04:05 | 4 007 759 | 6 063 | 766 / 22 KB | 2 090 / 497 KB | 2 286 / 10 960 | 0 / 0 / 0 |

Peak RSS 505-591 MB per target. The on-disk corpus directories hold every
unit libFuzzer ever wrote (not minimised): 5 450, 1 343, 120 and 4 062 files.
They stay on the box (git-ignored); only `fuzz/seeds/` is committed.

The 20 s shake-out found one failing assertion in `json_escape`
(docs/fuzzing.md, finding 1: harness compared floats after serde_json's
non-correctly-rounded parse); triaging it found finding 2 (jsonout float
layout was not CPython's), fixed in galion-common with regression tests. The
crash input is committed as `fuzz/seeds/json_escape/regress-float-1ulp` and
passes. The 660 s json_escape run above used the fixed harness and code.

`scripts/fuzz.sh 15` (the wrapper CI-style smoke, all four, 21:04 +02:00):
every target exit 0 (278 099 / 627 779 / 89 513 / 512 174 execs).

Box note: the box has gcc but no g++, and libfuzzer-sys compiles libFuzzer
(C++). `g++-14-x86-64-linux-gnu` and `libstdc++-14-dev` 14.2.0-19 were
fetched with `apt-get download` and unpacked with `dpkg -x` into
`/workspace/swarm/target/fuzz-sast/debs` (no root, no system change); `CXX`
points at it and `RUSTFLAGS=-L` at a dir with a `libstdc++.so` symlink to the
system `libstdc++.so.6`. CI runners need none of this.

## 2. Replaying the fuzz corpus at the real release binaries

`scripts/real/fuzz-sast.sh` (2026-10-20T20:56:01+02:00): release
`galion-origin` on 127.0.0.1:19250 (galion-common HTTP server) and release
`openmind-discovery announce` on mDNS test port 19251 (not 5353), then the
whole corpus from section 1 thrown at them over real sockets.

```
== 2026-10-20T20:56:14+02:00  start galion-origin on 127.0.0.1:19250 (empty ORIGIN_ROOT)
{"status":"healthy","service":"galion-origin","version":"0.1.0","uptime":0}

== 2026-10-20T20:56:14+02:00  replay http_parse_head corpus: .../fuzz/corpus/http_parse_head (5450 files)
{"files":5450,"refused":0,"status":{"200":100,"400":3329,"405":1983,"413":5,"501":10,"505":22,"no-reply":1}}
GET /health after replay -> 200

== 2026-10-20T20:56:14+02:00  start openmind-discovery announce on mDNS test port 19251 (up to 90 s)

== 2026-10-20T20:56:15+02:00  replay mdns_parse corpus: .../fuzz/corpus/mdns_parse (1343 files)
{"files":1343,"sent":1343,"replies_during_replay":0,"final_query_answered":true,"final_records":4}
announcer log: 1 queries answered, 0 rate-limited

== 2026-10-20T20:56:27+02:00  tear down
PASS
```

- HTTP: 5 450 connections, none refused, every one answered with a status
  except `no-reply: 1`, which is the 1-byte corpus file (byte 0 is the
  limits selector, so 0 bytes go on the wire; the server closes an empty
  connection without a reply, as it should). Statuses match the parser's
  error mapping (400 bad request line/header, 405 unknown method for the
  route, 413 body too large, 501 Transfer-Encoding, 505 bad version).
  `/health` still 200 afterwards.
- mDNS: 1 343 fuzz datagrams, no crash; none of them was a query for an
  announced name, so no replies; the following well-formed PTR query for
  `_galion._tcp.local.` got a legacy-unicast answer with 4 records (PTR +
  SRV + TXT + A).
- A first attempt (20:53 +02:00) failed the final mDNS check: the replay
  waited 20 ms per datagram and outlived the announcer's 25 s window. The
  replay now waits 2 ms and the announcer runs up to 90 s (killed at the
  end); not a product bug.

## 3. Build / lint / tests (2026-10-20, +02:00)

- `cargo fmt --check` (workspace and `fuzz/`): clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test --workspace`: 550 passed, 0 failed.
- `cargo check` for aarch64-linux-android and armv7-unknown-linux-musleabihf
  (galion-common all features, openmind-discovery with examples): ok.
  Release builds for x86_64-unknown-linux-musl (galion-origin 492 320 B,
  openmind-discovery 680 656 B) and aarch64-unknown-linux-musl
  (openmind-discovery 588 496 B, `rust-lld`): ok.
- `scripts/no-python.sh`: ok. `scripts/spdx-scan.sh`: 0 missing.
- Not run here: the GitHub workflows themselves (fuzz.yml, cflite-pr.yml,
  codeql.yml). The box has no Docker, so the ClusterFuzzLite image build was
  not tried locally.

## 4. Resume by slot 100 (swarm/fuzz-sast-2, ports 19700-19709), 2026-10-21 +02:00

The slot-25 agent stalled after committing e78b2e7 locally (never pushed).
Slot 100 resumed from that commit on `swarm/fuzz-sast-2`:

- **Rust-only port of section 2.** `scripts/real/fuzz-sast.sh` and the two
  replay examples (`galion-common/examples/replay_http.rs`,
  `openmind-discovery/examples/replay_mdns.rs`) are replaced by one Rust test,
  `crates/openmind-discovery/tests/real_fuzz_replay.rs`, gated by
  `GALION_REAL=1`. It builds release `galion-origin` + `openmind-discovery`,
  starts them on 127.0.0.1:19700 / mDNS test port 19701, replays the corpus,
  asserts (0 refused, no non-HTTP reply, `/health` 200 after, announcer alive,
  final PTR query answered) and kills both processes. Sections 1-2 above keep
  the slot-25 numbers and ports (19250/19251) as history.
- Run: `GALION_REAL=1 cargo test --release -p openmind-discovery --test real_fuzz_replay -- --nocapture`
  (`FUZZ_CORPUS_ROOT=` overrides the corpus dir; falls back to `fuzz/seeds/`).

Run 2026-10-21T00:57:32+02:00, slot-25 corpus copied from wt/fuzz-sast (git-ignored):

```
== 2026-10-21T00:57:35+02:00  start galion-origin on 127.0.0.1:19700 (empty ORIGIN_ROOT)
== 2026-10-21T00:57:35+02:00  replay http_parse_head corpus .../fuzz/corpus/http_parse_head
{"files":5666,"refused":0,"status":{"200": 104, "400": 3483, "405": 2041, "413": 5, "501": 10, "505": 22, "no-reply": 1}}
GET /health after replay -> Some(200)
== 2026-10-21T00:57:35+02:00  start openmind-discovery announce on mDNS test port 19701 (up to 90 s)
== 2026-10-21T00:57:36+02:00  replay mdns_parse corpus .../fuzz/corpus/mdns_parse
{"files":1429,"sent":1429,"replies_during_replay":0,"final_records":4}
== 2026-10-21T00:57:49+02:00  tear down
PASS
test result: ok. 1 passed; 0 failed; finished in 17.86s
```

(The corpus has 5 666 / 1 429 files now vs 5 450 / 1 343 in section 2: the
slot-25 runs kept writing units after that replay.) Without `GALION_REAL=1`
the test prints `skipped` and passes.

Not redone by slot 100: new libFuzzer campaigns. `cargo-fuzz` and the
unpacked g++ (section 1 box note) are no longer on the box (the slot-25 target
dir was deleted); section 1 stays the fuzzing evidence. `scripts/fuzz.sh` (dev
wrapper around `cargo +nightly fuzz run`, not used by CI) is still bash: open
item for the Rust-only rule.
