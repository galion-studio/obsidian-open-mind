<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: ci-artefact-rehearsal (slot 228, Integration S5-T3)

All times Europe/Zagreb (CEST +02:00), 2026-10-21/22. Box only plus
read-only VPS probes. Ports 20980-20989 (used: 20980-20981 memory, 20982-20985
real_release tuner/nerve). No tag created or pushed, nothing published.
Base `swarm/ci-release-artefacts` 04ce874 (slot 216).

## What was rehearsed

The packaged release exactly as release.yml builds it:
`scripts/ucpu-gate.sh build static size` (zig 0.15.2 cc, rust-lld, `--locked`)
with `SOURCE_DATE_EPOCH` / `GALION_BUILD_COMMIT` / `GALION_BUILD_TIME` from the
HEAD commit, then `galion-release package` x2 targets, `sums`, `verify`,
`sha256sum -c`, `rehearse` (x86_64 native, aarch64 under
`qemu-aarch64-static`), plus the new S5-T3 checks
(`crates/galion-release/src/rehearsal.rs`): provenance stamps vs the build,
cross-target parity, a `--help` run of every binary, size headroom, and
openmind-memory (rusqlite bundled = zig-cc C code) started from both packages.

| run | build | build window | real tests |
|---|---|---|---|
| 1 | 04ce874 (base), stamp `04ce8743ede1` / `2026-10-21T21:35:38+02:00` | 23:47:40-23:54:07, `RESULT ok` (static + size) | 23:54:17-23:56:21, all green |
| 2 | 593d56a (with the --help fixes), stamp `593d56aea502` / `2026-10-21T23:57:33+02:00` | 23:57:33-23:59:22, `RESULT ok` | 23:59:39-00:00:14, all green |

Command (run 2):

```
GALION_REAL=1 GALION_RELEASE_TARGET_DIR=$T GALION_BUILD_COMMIT=593d56aea502 \
GALION_BUILD_TIME=2026-10-21T23:57:33+02:00 GALION_REHEARSAL_EVIDENCE=$EV \
GALION_RELEASE_EVIDENCE=$EV/real_release.tsv GALION_RELEASE_PORT_BASE=20982 \
  cargo test -j 2 --release --locked -p galion-release -- --nocapture
unit 22/22, real_rehearsal 1/1 (20.46 s), real_release 2/2 (14.92 s), workflow 4/4
```

## Run 2 summary (real_s5t3_rehearsal, verbatim)

```
OK obsidian-open-mind-v0.1.0-rehearsal-aarch64-unknown-linux-musl.tar.gz  target aarch64-unknown-linux-musl  version 0.1.0  36 binaries
OK obsidian-open-mind-v0.1.0-rehearsal-x86_64-unknown-linux-musl.tar.gz  target x86_64-unknown-linux-musl  version 0.1.0  36 binaries
verify: 2 files in SHA256SUMS OK
obsidian-open-mind-v0.1.0-rehearsal-aarch64-unknown-linux-musl.tar.gz: OK
obsidian-open-mind-v0.1.0-rehearsal-x86_64-unknown-linux-musl.tar.gz: OK
stamps: 20 stamped (593d56aea502), 52 without provenance (non-binding)
parity: ok (same binaries and --version lines on every target)
--help: 28 nonzero, 44 ok (non-binding)
headroom: hermes-gateway on x86_64-unknown-linux-musl at 99.3% of the budget (> 90%, non-binding)
rehearse: 72 runs, 0 failed, 0 parity problems
negative wrong commit: exit 1, 20 FAIL rows = 20 stamped binaries
negative wrong build time: exit 1
negative arch skew (aarch64 without tuner): exit 1, parity FAIL tuner
x86_64-unknown-linux-musl openmind-memory :20980 /health 200 201 ms db=vectors.db {"status":"ok","version":"0.1.0","uptime":0,"stores":{"vectorStore":true,"embeddingService":false},"services":{"timerScanner":null,"pipelineWorker":null,"stateB
aarch64-unknown-linux-musl openmind-memory :20981 /health 200 210 ms db=vectors.db {"status":"ok","version":"0.1.0","uptime":0,"stores":{"vectorStore":true,"embeddingService":false},"services":{"timerScanner":null,"pipelineWorker":null,"stateB
```

Stamped binaries (10 per target, from galion-common build_info): desk-hermes
galion-boot galion-desk galion-one galion-origin link-watch one-talk
openmind-memory room tuner. The other 26 per target print `<bin> 0.1.0` only.

## Gap found and fixed: `--help` started servers (run 1)

Run 1 (base 04ce874), `--help`: 36 ok, 32 nonzero, **4 TIMEOUT**:

```
aarch64-unknown-linux-musl	galion-health	TIMEOUT	5010	ts=2026-10-21T23:55:16+02:00 level=info svc=galion-health msg="listening on 127.0.0.1:8660 checks=12...
aarch64-unknown-linux-musl	galion-origin	TIMEOUT	5001	galion-origin 0.1.0 on 127.0.0.1:8655
x86_64-unknown-linux-musl	galion-health	TIMEOUT	5004	ts=2026-10-21T23:55:27+02:00 level=info svc=galion-health msg="listening on 127.0.0.1:8660 checks=12...
x86_64-unknown-linux-musl	galion-origin	TIMEOUT	5005	galion-origin 0.1.0 on 127.0.0.1:8655
```

galion-health and galion-origin ignored `--help` and bound their default
ports (8660 / 8655) until the 5 s deadline killed them (box only; ports
checked free afterwards). On the VPS the same call would collide with a live
service port. hermes-gateway treated `--help` as a start and only stopped
because `API_SERVER_KEY` was absent (with the unit's env it would try to
serve); openmind-mcp-stack read `--help` as a crate name. Fix (593d56a): a
first-in-main `--help`/`-h` handler (usage on stdout, exit 0, no config,
bind or I/O) in those 4 binaries, same pattern as slot 216's `--version`.
Run 2: **0 TIMEOUT**, 44 ok, 28 nonzero.

Still non-zero on `--help` (usage printed, but exit != 0; non-binding), same
14 on both targets: bench-banking, bench-scoreboard, galion-backup,
galion-boot (`unknown command --help`), galion-one, galion-vending (`unknown
command '--help'`), galion-wallstreet, guardian-tick (`unknown argument`),
hermes-bench (`unknown command`), one-set-model, one-talk, openmind-memory,
persona-plex-relay, repo-check.

## Run 2 full table (rehearse.tsv)

```
result	target	binary	bytes	budget%	runner	ms	--version	stamp	--help
PASS	aarch64-unknown-linux-musl	bench-banking	632096	30.1	qemu-aarch64-static	20	bench-banking 0.1.0	none	nonzero
PASS	aarch64-unknown-linux-musl	bench-scoreboard	430992	20.6	qemu-aarch64-static	10	bench-scoreboard 0.1.0	none	nonzero
PASS	aarch64-unknown-linux-musl	desk-hermes	1349952	64.4	qemu-aarch64-static	10	desk-hermes 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	ok
PASS	aarch64-unknown-linux-musl	gaia	846504	40.4	qemu-aarch64-static	10	gaia 0.1.0	none	ok
PASS	aarch64-unknown-linux-musl	galion-backup	1419808	67.7	qemu-aarch64-static	10	galion-backup 0.1.0	none	nonzero
PASS	aarch64-unknown-linux-musl	galion-boot	743336	35.4	qemu-aarch64-static	10	galion-boot 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	nonzero
PASS	aarch64-unknown-linux-musl	galion-deploy	565056	26.9	qemu-aarch64-static	10	galion-deploy 0.1.0	none	ok
PASS	aarch64-unknown-linux-musl	galion-desk	665368	31.7	qemu-aarch64-static	10	galion-desk 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	ok
PASS	aarch64-unknown-linux-musl	galion-guard	1415912	67.5	qemu-aarch64-static	10	galion-guard 0.1.0	none	ok
PASS	aarch64-unknown-linux-musl	galion-health	447280	21.3	qemu-aarch64-static	10	galion-health 0.1.0	none	ok
PASS	aarch64-unknown-linux-musl	galion-one	738440	35.2	qemu-aarch64-static	20	galion-one 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	nonzero
PASS	aarch64-unknown-linux-musl	galion-origin	444560	21.2	qemu-aarch64-static	20	galion-origin 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	ok
PASS	aarch64-unknown-linux-musl	galion-realtest	660056	31.5	qemu-aarch64-static	10	galion-realtest 0.1.0	none	ok
PASS	aarch64-unknown-linux-musl	galion-release	600280	28.6	qemu-aarch64-static	10	galion-release 0.1.0	none	ok
PASS	aarch64-unknown-linux-musl	galion-vending	593144	28.3	qemu-aarch64-static	10	galion-vending 0.1.0	none	nonzero
PASS	aarch64-unknown-linux-musl	galion-voice-serve	517544	24.7	qemu-aarch64-static	10	galion-voice-serve 0.1.0	none	ok
PASS	aarch64-unknown-linux-musl	galion-wallstreet	696688	33.2	qemu-aarch64-static	10	galion-wallstreet 0.1.0	none	nonzero
PASS	aarch64-unknown-linux-musl	galleon-arena	402128	19.2	qemu-aarch64-static	10	galleon-arena 0.1.0	none	ok
PASS	aarch64-unknown-linux-musl	guardian-tick	761840	36.3	qemu-aarch64-static	10	guardian-tick 0.1.0	none	nonzero
PASS	aarch64-unknown-linux-musl	hades-gate	534648	25.5	qemu-aarch64-static	10	hades-gate 0.1.0	none	ok
PASS	aarch64-unknown-linux-musl	hermes-bench	1302648	62.1	qemu-aarch64-static	10	hermes-bench 0.1.0	none	nonzero
PASS	aarch64-unknown-linux-musl	hermes-gateway	1877224	89.5	qemu-aarch64-static	10	hermes-gateway 0.1.0	none	ok
PASS	aarch64-unknown-linux-musl	link-watch	678664	32.4	qemu-aarch64-static	10	link-watch 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	ok
PASS	aarch64-unknown-linux-musl	omnode	1293584	61.7	qemu-aarch64-static	10	omnode 0.1.0	none	ok
PASS	aarch64-unknown-linux-musl	one-defence	1285264	61.3	qemu-aarch64-static	10	one-defence 0.1.0	none	ok
PASS	aarch64-unknown-linux-musl	one-set-model	375832	17.9	qemu-aarch64-static	10	one-set-model 0.1.0	none	nonzero
PASS	aarch64-unknown-linux-musl	one-talk	378760	18.1	qemu-aarch64-static	10	one-talk 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	nonzero
PASS	aarch64-unknown-linux-musl	openmind-discovery	666304	31.8	qemu-aarch64-static	10	openmind-discovery 0.1.0	none	ok
PASS	aarch64-unknown-linux-musl	openmind-mcp-stack	404920	19.3	qemu-aarch64-static	10	openmind-mcp-stack 0.1.0	none	ok
PASS	aarch64-unknown-linux-musl	openmind-memory	1670944	79.7	qemu-aarch64-static	20	openmind-memory 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	nonzero
PASS	aarch64-unknown-linux-musl	openmind-nerve	1001488	47.8	qemu-aarch64-static	10	openmind-nerve 0.1.0	none	ok
PASS	aarch64-unknown-linux-musl	openmind-queue	744056	35.5	qemu-aarch64-static	10	openmind-queue 0.1.0	none	ok
PASS	aarch64-unknown-linux-musl	persona-plex-relay	426528	20.3	qemu-aarch64-static	10	persona-plex-relay 0.1.0	none	nonzero
PASS	aarch64-unknown-linux-musl	repo-check	408104	19.5	qemu-aarch64-static	10	repo-check 0.1.0	none	nonzero
PASS	aarch64-unknown-linux-musl	room	445464	21.2	qemu-aarch64-static	10	room 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	ok
PASS	aarch64-unknown-linux-musl	tuner	429984	20.5	qemu-aarch64-static	20	tuner 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	ok
PASS	x86_64-unknown-linux-musl	bench-banking	741176	35.3	native	0	bench-banking 0.1.0	none	nonzero
PASS	x86_64-unknown-linux-musl	bench-scoreboard	488472	23.3	native	0	bench-scoreboard 0.1.0	none	nonzero
PASS	x86_64-unknown-linux-musl	desk-hermes	1447344	69.0	native	10	desk-hermes 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	ok
PASS	x86_64-unknown-linux-musl	gaia	996360	47.5	native	10	gaia 0.1.0	none	ok
PASS	x86_64-unknown-linux-musl	galion-backup	1525040	72.7	native	0	galion-backup 0.1.0	none	nonzero
PASS	x86_64-unknown-linux-musl	galion-boot	859032	41.0	native	10	galion-boot 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	nonzero
PASS	x86_64-unknown-linux-musl	galion-deploy	635224	30.3	native	0	galion-deploy 0.1.0	none	ok
PASS	x86_64-unknown-linux-musl	galion-desk	757736	36.1	native	0	galion-desk 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	ok
PASS	x86_64-unknown-linux-musl	galion-guard	1765960	84.2	native	10	galion-guard 0.1.0	none	ok
PASS	x86_64-unknown-linux-musl	galion-health	504440	24.1	native	0	galion-health 0.1.0	none	ok
PASS	x86_64-unknown-linux-musl	galion-one	829992	39.6	native	0	galion-one 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	nonzero
PASS	x86_64-unknown-linux-musl	galion-origin	492104	23.5	native	10	galion-origin 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	ok
PASS	x86_64-unknown-linux-musl	galion-realtest	754312	36.0	native	0	galion-realtest 0.1.0	none	ok
PASS	x86_64-unknown-linux-musl	galion-release	686312	32.7	native	0	galion-release 0.1.0	none	ok
PASS	x86_64-unknown-linux-musl	galion-vending	684152	32.6	native	10	galion-vending 0.1.0	none	nonzero
PASS	x86_64-unknown-linux-musl	galion-voice-serve	574824	27.4	native	10	galion-voice-serve 0.1.0	none	ok
PASS	x86_64-unknown-linux-musl	galion-wallstreet	801480	38.2	native	10	galion-wallstreet 0.1.0	none	nonzero
PASS	x86_64-unknown-linux-musl	galleon-arena	452536	21.6	native	10	galleon-arena 0.1.0	none	ok
PASS	x86_64-unknown-linux-musl	guardian-tick	886808	42.3	native	10	guardian-tick 0.1.0	none	nonzero
PASS	x86_64-unknown-linux-musl	hades-gate	618776	29.5	native	10	hades-gate 0.1.0	none	ok
PASS	x86_64-unknown-linux-musl	hermes-bench	1586600	75.7	native	10	hermes-bench 0.1.0	none	nonzero
PASS	x86_64-unknown-linux-musl	hermes-gateway	2082400	99.3	native	10	hermes-gateway 0.1.0	none	ok
PASS	x86_64-unknown-linux-musl	link-watch	784952	37.4	native	0	link-watch 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	ok
PASS	x86_64-unknown-linux-musl	omnode	1515528	72.3	native	0	omnode 0.1.0	none	ok
PASS	x86_64-unknown-linux-musl	one-defence	1369360	65.3	native	10	one-defence 0.1.0	none	ok
PASS	x86_64-unknown-linux-musl	one-set-model	423960	20.2	native	10	one-set-model 0.1.0	none	nonzero
PASS	x86_64-unknown-linux-musl	one-talk	427224	20.4	native	10	one-talk 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	nonzero
PASS	x86_64-unknown-linux-musl	openmind-discovery	765480	36.5	native	11	openmind-discovery 0.1.0	none	ok
PASS	x86_64-unknown-linux-musl	openmind-mcp-stack	461016	22.0	native	10	openmind-mcp-stack 0.1.0	none	ok
PASS	x86_64-unknown-linux-musl	openmind-memory	1814368	86.5	native	10	openmind-memory 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	nonzero
PASS	x86_64-unknown-linux-musl	openmind-nerve	1201832	57.3	native	10	openmind-nerve 0.1.0	none	ok
PASS	x86_64-unknown-linux-musl	openmind-queue	871304	41.5	native	10	openmind-queue 0.1.0	none	ok
PASS	x86_64-unknown-linux-musl	persona-plex-relay	481592	23.0	native	10	persona-plex-relay 0.1.0	none	nonzero
PASS	x86_64-unknown-linux-musl	repo-check	463160	22.1	native	10	repo-check 0.1.0	none	nonzero
PASS	x86_64-unknown-linux-musl	room	495880	23.6	native	10	room 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	ok
PASS	x86_64-unknown-linux-musl	tuner	477608	22.8	native	10	tuner 0.1.0 (commit 593d56aea502, built 2026-10-21T23:57:33+02:00)	stamped	ok
```

## VPS (read-only, 2026-10-21 23:51:46 CEST)

* nerve `GET 127.0.0.1:8670/health`: `{"appended":30,"cycle":9,"score":66,"status":"ok","system":"degraded","version":"0.1.0"}`; `/state` 200, 36,702 B.
* cortex `GET 127.0.0.1:8674/health`: `{"busy":false,"escalations_open":0,"events_in":2,"handoffs":0,"hermes_handoff":true,"incidents_open":0,"llm_enabled":true,"mode":"live","nerve":true,"ok":true,...}` (cortex live; observed only).
* live binaries (`systemctl show ExecStart`, `stat`, `--version`, `timeout 3 <bin> --help`):

| unit | binary | bytes | --version | --help rc |
|---|---|---|---|---|
| gaia-cortex | gaia-cortex-bd6fbe6 | 1,041,248 | `gaia-cortex 0.1.0` | 2 |
| hermes-rs | hermes-gateway-aa77864 | 1,841,904 | **no version: tries to start**, `refusing to start: API_SERVER_KEY is missing` | 2 (same refusal) |
| openmind-memory | openmind-memory-aa77864 | 1,702,640 | `openmind-memory 0.1.0 (commit aa77864, built 2026-10-21T09:39:35+02:00)` | 64 |
| galion-one | galion-one | 963,904 | `galion-one 0.1.0` | 2 |
| tuner | tuner | 480,032 | `tuner 0.1.0 (commit unknown, built unknown)` | 0 |

  The live hermes-gateway predates slot 216's `--version` fix: `--version`
  and `--help` both enter the start path. It refused because the probe shell
  has no `API_SERVER_KEY`; nothing was bound (the live port stays with the
  unit). Note for the owner: running the live build's `--version` with the
  unit's environment would be unsafe until it is redeployed from a release
  that includes 216 + 228. No unit was touched; nothing written or installed.

## Gates (box)

| gate | result |
|---|---|
| `cargo fmt --all --check` | ok (00:00:22) |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | ok (00:00:23-00:00:51) |
| `cargo test --workspace --no-fail-fast --locked --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools` | 269 suites, 1270 passed, 0 failed, 6 ignored (00:00:51-00:03:05) |
| `scripts/ucpu-gate.sh build static size` (x86_64 + aarch64 musl, workspace + omnode) | RESULT ok (both runs) |
| `scripts/ucpu-gate.sh check` (armv7 musleabihf + aarch64-android, workspace + omnode) | RESULT ok (00:03:24-00:04:36) |
| `scripts/no-python.sh` / `scripts/spdx-scan.sh` | ok / missing 0 |
| new deps | none (Cargo.lock unchanged) |
