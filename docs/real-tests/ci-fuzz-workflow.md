<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-test record: ci-fuzz-workflow (slot 202, Security S1.3)

Box: shared 8 vCPU / 15 GB swarm box, other agents' builds running (load
4-18), so exec rates vary up to 10x between runs. Every run is the exact
`fuzz` step of `.github/workflows/fuzz.yml` (release build, `--locked`,
`-- --exact <filter> --nocapture`, `GALION_FUZZ_SECS`, `GALION_FUZZ_CORPUS`,
`TZ=Europe/Zagreb`), one process per matrix row, rows in parallel like the
jobs. The corpus directory was kept between runs like the actions cache.
All times Europe/Zagreb, CEST +02:00.

## PR budget (60 s per target)

| start | target | corpus before | execs | rate/s | distinct | corpus after | result |
|---|---|---|---|---|---|---|---|
| 17:29 | http | 0 | 17,682,688 | 294,711 | 125,663 | 512 | ok (pre-signature-fix run, cap hit) |
| 17:29 | guard | 0 | 1,301,401 | 21,675 | 93 | 92 | ok |
| 17:32 | http (cold) | 0 | 1,228,800 | 20,478 | 156 | 156 | ok |
| 17:32 | guard (cold) | 0 | 264,122 | 4,391 | 89 | 89 | ok |
| 17:33 | http (warm) | 156 | 1,300,992 | 21,681 | 160 | 316 | ok |
| 17:33 | guard (warm) | 89 | 220,705 | 3,667 | 92 | 180 | ok |
| 17:38 | http (cold 2) | 0 | 5,372,416 | 89,539 | 163 | 163 | ok |
| 17:38 | guard (cold 2) | 0 | 589,166 | 9,801 | 88 | 88 | ok |
| 17:39 | http (warm 2) | 163 | 4,402,432 | 73,370 | 163 | 163 | ok |
| 17:39 | guard (warm 2) | 88 | 513,324 | 8,517 | 85 | 112 | ok |
| 19:04:30 | http (warm 3, final code) | 163 | 13,398,528 | 223,308 | 163 | 163 | ok |
| 19:04:30 | guard (warm 3, final code) | 200 | 1,217,285 | 20,274 | 92 | 224 | ok |

## Weekly budget (600 s per target)

| start | target | corpus before | seed | result |
|---|---|---|---|---|
| 17:40:33 | http | 163 | 1792597233423677715 | ok, 28,333,312 execs, 47,220/s, 600.0 s |
| 17:40:33 | guard | 112 | 1792597233379690538 | **FAIL at 548 s** find 1 (parser) -> fixed f042e5c |
| 18:07:31 | guard | 144 | 1792598853241971466 | **FAIL at 3.5 s** find 2 (oracle gap) -> fixed 21b2496 |
| 18:51:09 | http | 163 | 1792601469959840709 | ok, 134,606,336 execs, 224,344/s, 600.0 s |
| 18:51:10 | guard | 166 | 1792601469964599518 | **FAIL at 0.1 s** find 3 (parser) -> fixed 37e9042 |
| 18:54:04 | guard (final code) | 173 | 1792601644469030334 | **ok**, 7,983,733 execs, 13,305/s, distinct 96, corpus 200 (+27), 600.0 s |

Exact summary lines (final code):

```
fuzz-summary target=http execs=134606336 distinct=163 corpus=163 new=0 crashes=0 secs=600.0 rate=224344/s seed=1792601469959840709
fuzz-summary target=guard execs=7983733 distinct=96 corpus=200 new=27 crashes=0 secs=600.0 rate=13305/s seed=1792601644469030334
fuzz-summary target=http execs=13398528 distinct=163 corpus=163 new=0 crashes=0 secs=60.0 rate=223308/s seed=1792602270579093261
fuzz-summary target=guard execs=1217285 distinct=92 corpus=224 new=24 crashes=0 secs=60.0 rate=20274/s seed=1792602270587750139
```

The http target saturates at 163 outcome shapes: the cached corpus stops
growing after the first PR run, which is what the cache is for (old finds
re-checked first, no unbounded growth; cap 512 files x 8 KiB).

## Crash path (the `upload crash` step's input)

* 17:44, forced http crash with a planted panic in a scratch corpus dir
  (not committed): the input was written to `<corpus>/crashes/http-38e05d43ea8012ea`
  and the line `fuzz-crash target=http seed=… input=…` printed before the
  panic continued; exit 101 (the job fails, the artifact step uploads it).
* All three real guard finds produced `crashes/guard-<hash>`; 19:05, those 3
  files used as the whole corpus (5 s run): 3 loaded, replayed, `crashes=0`,
  exit 0 on 37e9042.
* Without the fix (filter.rs of 21b2496, tests of 37e9042):
  `ipv6_run_with_colon_tail_is_not_truncated` FAILED, `weekly_finds_replay`
  FAILED; with it both pass.

## Real server data (VPS, read-only)

* 17:45 +02:00: 400 sshd journal lines (plain + `journalctl -o json`) read
  from the VPS with `journalctl` (read-only), split into seed files under
  scratch on the box (addresses not committed). 18:08 and 19:04:30 runs with
  that set as the corpus (512 loaded, 60 s): 106,596 and 1,300,570 execs,
  `crashes=0`, every real line parses without an invented address or a
  loopback ban.
* 18:51:30 +02:00 nerve `GET 127.0.0.1:8670/state` over ssh: HTTP 200,
  galion-nerve `active`, cycle 74, mode dry-run, score 66/D (2 error
  incidents, edge closed-origin ports; unrelated to this slot). The cortex
  is not deployed as a separate service yet, so the nerve was the observer.
  No galion-guard unit runs on the VPS (`systemctl list-units galion-guard*`
  empty), so there is no live guard to compare against; this branch changes
  nothing live.

## Gates (head 37e9042 + docs)

* `cargo fmt --check`: ok.
* `cargo clippy --locked --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings`: ok (19:00).
* `cargo test --release -p galion-guard`: 51 + 3 + 1 + 9 tests, 0 failed.
* Workspace test: see the slot 202 DONE line in CALL-RECORD (numbers).
* `actionlint` 1.7.12 on `.github/workflows/fuzz.yml`: 0 findings.
* `scripts/no-python.sh`: ok. `scripts/spdx-scan.sh`: 506 scanned, 0 missing.
