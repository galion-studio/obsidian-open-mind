<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: ci-release-artefacts (slot 216)

All times Europe/Zagreb (CEST +02:00), 2026-10-21. Box only, plus read-only
VPS probes. Ports 20860-20863 (slot range 20860-20869). No tag, no publish.

## Build (as release.yml does it)

`SOURCE_DATE_EPOCH`, `GALION_BUILD_COMMIT=8720378e71f2`,
`GALION_BUILD_TIME=2026-10-21T21:14:30+02:00`; `cargo-slot build -j 2
--release --workspace --locked --target <t>` + omnode, rust-lld, zig 0.15.2 cc.
21:14:35-21:17:16. Then `scripts/ucpu-gate.sh static size` 21:19:50: rc 0,
38 workspace binaries (incl. galion-release and the 3 test mocks) + omnode per target, all static and within budget; the package ships 36 = 39 - 3 mocks.

Before the strip fix (base build 21:03-21:06): `size-budget.sh` FAIL
`galion-voice-serve 693080 stripped=no`; `cargo build -v` showed
`-C strip=debuginfo` for that bin; after the fix `-C strip=symbols`, 572,936 B.
The first package attempt (21:08) failed the same way:
`galion-voice-serve: not stripped (.symtab present)`.

## GALION_REAL=1 cargo test --release -p galion-release (21:20:32-21:20:52)

```
GALION_REAL=1 GALION_RELEASE_EVIDENCE=$T/rehearse.tsv \
  cargo-slot test -j 2 --release --locked -p galion-release -- --nocapture
unit 18/18, workflow 4/4, real_release 2/2 (real_release_rehearsal, real_release_negative_controls) in 14.02 s
```

real_release_rehearsal:
* package x86_64: 36 binaries, 15,964,799 B; aarch64: 36 binaries,
  15,294,802 B; excluded gaia-dummy galion-bench-mock hermes-mock-model.
* `galion-release verify`: `verify: 2 files in SHA256SUMS OK`, both archives
  `OK ... 36 binaries`.
* `sha256sum -c SHA256SUMS`: both `OK`. GNU `tar -tzvf` reads both archives
  (root/root, mode 755 bins / 644 docs); same 36-binary set for both arches.
* second packaging into another dir: every file byte-identical.
* rehearse: `rehearse: 72 runs, 0 failed` (table below).
* started from the extracted packages:
  `x86_64-unknown-linux-musl tuner :20860 /health 200 0 ms {"ok":true}`
  `x86_64-unknown-linux-musl nerve :20861 /health 200 0 ms {"status":"ok","service":"openmind-nerve","version":"0.1.0","appended":0,"ts":"2026-10-21T21:20:51+02:00","system":null,"score":null,"cycle":null}`
  `aarch64-unknown-linux-musl tuner :20862 /health 200 203 ms {"ok":true}`
  `aarch64-unknown-linux-musl nerve :20863 /health 200 4 ms {"status":"ok","service":"openmind-nerve","version":"0.1.0","appended":0,"ts":"2026-10-21T21:20:52+02:00","system":"degrading","score":95,"cycle":1}`

real_release_negative_controls (all must fail, all did):
* `rehearse --expect-version 9.9.9` -> exit 1 (72 FAIL rows).
* extra unlisted `stray.tar.gz` -> exit 1 `stray.tar.gz: not listed in SHA256SUMS`.
* one flipped byte in the x86_64 archive -> exit 1 `sha256 mismatch`; `sha256sum -c` also fails;
  after re-running `sums` the archive check still fails (gzip CRC / inflate).
* host (glibc, dynamic) galion-release as input -> exit 1 `PT_INTERP` / `DT_NEEDED`.
* `--budget 1000000` on aarch64 -> exit 1 `> budget 1000000`.
* aarch64 dir packaged as x86_64 -> exit 1 `machine AArch64 != x86_64-unknown-linux-musl`.

### rehearse table (21:20, bytes, % of 2 MiB, runner, ms, first line of --version)

```
result	target	binary	bytes	budget%	runner	ms	--version
PASS	aarch64-unknown-linux-musl	bench-banking	632096	30.1	qemu-aarch64-static	21	bench-banking 0.1.0
PASS	aarch64-unknown-linux-musl	bench-scoreboard	430992	20.6	qemu-aarch64-static	10	bench-scoreboard 0.1.0
PASS	aarch64-unknown-linux-musl	desk-hermes	1349952	64.4	qemu-aarch64-static	10	desk-hermes 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	aarch64-unknown-linux-musl	gaia	846352	40.4	qemu-aarch64-static	10	gaia 0.1.0
PASS	aarch64-unknown-linux-musl	galion-backup	1419808	67.7	qemu-aarch64-static	10	galion-backup 0.1.0
PASS	aarch64-unknown-linux-musl	galion-boot	743336	35.4	qemu-aarch64-static	10	galion-boot 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	aarch64-unknown-linux-musl	galion-deploy	565056	26.9	qemu-aarch64-static	10	galion-deploy 0.1.0
PASS	aarch64-unknown-linux-musl	galion-desk	665368	31.7	qemu-aarch64-static	10	galion-desk 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	aarch64-unknown-linux-musl	galion-guard	1415912	67.5	qemu-aarch64-static	10	galion-guard 0.1.0
PASS	aarch64-unknown-linux-musl	galion-health	446896	21.3	qemu-aarch64-static	10	galion-health 0.1.0
PASS	aarch64-unknown-linux-musl	galion-one	738440	35.2	qemu-aarch64-static	10	galion-one 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	aarch64-unknown-linux-musl	galion-origin	443616	21.2	qemu-aarch64-static	10	galion-origin 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	aarch64-unknown-linux-musl	galion-realtest	660056	31.5	qemu-aarch64-static	10	galion-realtest 0.1.0
PASS	aarch64-unknown-linux-musl	galion-release	564696	26.9	qemu-aarch64-static	10	galion-release 0.1.0
PASS	aarch64-unknown-linux-musl	galion-vending	593144	28.3	qemu-aarch64-static	10	galion-vending 0.1.0
PASS	aarch64-unknown-linux-musl	galion-voice-serve	517544	24.7	qemu-aarch64-static	10	galion-voice-serve 0.1.0
PASS	aarch64-unknown-linux-musl	galion-wallstreet	696688	33.2	qemu-aarch64-static	10	galion-wallstreet 0.1.0
PASS	aarch64-unknown-linux-musl	galleon-arena	402128	19.2	qemu-aarch64-static	10	galleon-arena 0.1.0
PASS	aarch64-unknown-linux-musl	guardian-tick	761840	36.3	qemu-aarch64-static	10	guardian-tick 0.1.0
PASS	aarch64-unknown-linux-musl	hades-gate	534648	25.5	qemu-aarch64-static	10	hades-gate 0.1.0
PASS	aarch64-unknown-linux-musl	hermes-bench	1302648	62.1	qemu-aarch64-static	10	hermes-bench 0.1.0
PASS	aarch64-unknown-linux-musl	hermes-gateway	1876904	89.5	qemu-aarch64-static	10	hermes-gateway 0.1.0
PASS	aarch64-unknown-linux-musl	link-watch	678664	32.4	qemu-aarch64-static	10	link-watch 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	aarch64-unknown-linux-musl	omnode	1294400	61.7	qemu-aarch64-static	10	omnode 0.1.0
PASS	aarch64-unknown-linux-musl	one-defence	1285264	61.3	qemu-aarch64-static	10	one-defence 0.1.0
PASS	aarch64-unknown-linux-musl	one-set-model	375832	17.9	qemu-aarch64-static	10	one-set-model 0.1.0
PASS	aarch64-unknown-linux-musl	one-talk	378760	18.1	qemu-aarch64-static	10	one-talk 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	aarch64-unknown-linux-musl	openmind-discovery	666304	31.8	qemu-aarch64-static	10	openmind-discovery 0.1.0
PASS	aarch64-unknown-linux-musl	openmind-mcp-stack	403816	19.3	qemu-aarch64-static	10	openmind-mcp-stack 0.1.0
PASS	aarch64-unknown-linux-musl	openmind-memory	1670944	79.7	qemu-aarch64-static	10	openmind-memory 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	aarch64-unknown-linux-musl	openmind-nerve	1001488	47.8	qemu-aarch64-static	10	openmind-nerve 0.1.0
PASS	aarch64-unknown-linux-musl	openmind-queue	744056	35.5	qemu-aarch64-static	10	openmind-queue 0.1.0
PASS	aarch64-unknown-linux-musl	persona-plex-relay	426528	20.3	qemu-aarch64-static	10	persona-plex-relay 0.1.0
PASS	aarch64-unknown-linux-musl	repo-check	408104	19.5	qemu-aarch64-static	10	repo-check 0.1.0
PASS	aarch64-unknown-linux-musl	room	445464	21.2	qemu-aarch64-static	10	room 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	aarch64-unknown-linux-musl	tuner	429984	20.5	qemu-aarch64-static	10	tuner 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	x86_64-unknown-linux-musl	bench-banking	741176	35.3	native	0	bench-banking 0.1.0
PASS	x86_64-unknown-linux-musl	bench-scoreboard	488472	23.3	native	0	bench-scoreboard 0.1.0
PASS	x86_64-unknown-linux-musl	desk-hermes	1447344	69.0	native	10	desk-hermes 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	x86_64-unknown-linux-musl	gaia	996136	47.5	native	0	gaia 0.1.0
PASS	x86_64-unknown-linux-musl	galion-backup	1525040	72.7	native	10	galion-backup 0.1.0
PASS	x86_64-unknown-linux-musl	galion-boot	859032	41.0	native	0	galion-boot 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	x86_64-unknown-linux-musl	galion-deploy	635224	30.3	native	0	galion-deploy 0.1.0
PASS	x86_64-unknown-linux-musl	galion-desk	757736	36.1	native	10	galion-desk 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	x86_64-unknown-linux-musl	galion-guard	1765960	84.2	native	0	galion-guard 0.1.0
PASS	x86_64-unknown-linux-musl	galion-health	504056	24.0	native	10	galion-health 0.1.0
PASS	x86_64-unknown-linux-musl	galion-one	829992	39.6	native	10	galion-one 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	x86_64-unknown-linux-musl	galion-origin	491272	23.4	native	10	galion-origin 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	x86_64-unknown-linux-musl	galion-realtest	754312	36.0	native	0	galion-realtest 0.1.0
PASS	x86_64-unknown-linux-musl	galion-release	640104	30.5	native	0	galion-release 0.1.0
PASS	x86_64-unknown-linux-musl	galion-vending	684152	32.6	native	0	galion-vending 0.1.0
PASS	x86_64-unknown-linux-musl	galion-voice-serve	574824	27.4	native	10	galion-voice-serve 0.1.0
PASS	x86_64-unknown-linux-musl	galion-wallstreet	801480	38.2	native	0	galion-wallstreet 0.1.0
PASS	x86_64-unknown-linux-musl	galleon-arena	452536	21.6	native	0	galleon-arena 0.1.0
PASS	x86_64-unknown-linux-musl	guardian-tick	886808	42.3	native	10	guardian-tick 0.1.0
PASS	x86_64-unknown-linux-musl	hades-gate	618776	29.5	native	0	hades-gate 0.1.0
PASS	x86_64-unknown-linux-musl	hermes-bench	1586600	75.7	native	10	hermes-bench 0.1.0
PASS	x86_64-unknown-linux-musl	hermes-gateway	2082048	99.3	native	10	hermes-gateway 0.1.0
PASS	x86_64-unknown-linux-musl	link-watch	784952	37.4	native	0	link-watch 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	x86_64-unknown-linux-musl	omnode	1515496	72.3	native	10	omnode 0.1.0
PASS	x86_64-unknown-linux-musl	one-defence	1369360	65.3	native	0	one-defence 0.1.0
PASS	x86_64-unknown-linux-musl	one-set-model	423960	20.2	native	0	one-set-model 0.1.0
PASS	x86_64-unknown-linux-musl	one-talk	427224	20.4	native	10	one-talk 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	x86_64-unknown-linux-musl	openmind-discovery	765480	36.5	native	0	openmind-discovery 0.1.0
PASS	x86_64-unknown-linux-musl	openmind-mcp-stack	459864	21.9	native	10	openmind-mcp-stack 0.1.0
PASS	x86_64-unknown-linux-musl	openmind-memory	1814368	86.5	native	10	openmind-memory 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	x86_64-unknown-linux-musl	openmind-nerve	1201832	57.3	native	10	openmind-nerve 0.1.0
PASS	x86_64-unknown-linux-musl	openmind-queue	871304	41.5	native	0	openmind-queue 0.1.0
PASS	x86_64-unknown-linux-musl	persona-plex-relay	481592	23.0	native	0	persona-plex-relay 0.1.0
PASS	x86_64-unknown-linux-musl	repo-check	463160	22.1	native	0	repo-check 0.1.0
PASS	x86_64-unknown-linux-musl	room	495880	23.6	native	10	room 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
PASS	x86_64-unknown-linux-musl	tuner	477608	22.8	native	0	tuner 0.1.0 (commit 8720378e71f2, built 2026-10-21T21:14:30+02:00)
rehearse: 72 runs, 0 failed
```

## VPS (read-only, 21:15 CEST)

* nerve `GET 127.0.0.1:8670/state` 200; `/health` `{"cycle":10,"score":61,"status":"ok","system":"degraded","version":"0.1.0"}`.
* cortex `GET 127.0.0.1:8674/health` `{"ok":true,"mode":"dry-run","llm_enabled":true,"incidents_open":0,"version":"0.1.0"}` (cortex live; observed, not changed).
* live Rust binaries (readelf, read-only): openmind-memory-aa77864 1,702,640 B, galion-one 963,904 B,
  openmind-nerve-bb287cc 1,415,176 B, tuner 480,032 B: all no PT_INTERP, 0 DT_NEEDED, no .symtab
  -> same static/stripped shape the release gate enforces. `--version` of the live binaries:
  `openmind-memory 0.1.0 (commit aa77864, built 2026-10-21T09:39:35+02:00)`, `galion-one 0.1.0`,
  `openmind-nerve 0.1.0`, `tuner 0.1.0 (commit unknown, built unknown)`: the live tuner was built
  without provenance; release.yml now always sets it.
* Nothing was started, stopped, written or installed on the VPS.
