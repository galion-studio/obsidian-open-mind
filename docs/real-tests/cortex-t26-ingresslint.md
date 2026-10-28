<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: cortex T2.6 `ingress-lint` sensor (slot 159)

- Branch `swarm/cortex-t26-ingresslint` = `swarm/cortex-t20-merge` ee75f75 + merge of `swarm/tunnel-audit-2` 988e47f
  (crates/tunnel-lint) + the sensor. Ports 20290-20299 (box, loopback only).
- Written 2026-10-21 08:20 CEST (+02:00). All VPS access read-only (ssh reads, `ss -ltn`, `journalctl`, HTTP GET).
  Cortex is not deployed on the VPS (`GET 127.0.0.1:8670/api/v1/cortex` -> 404 at 08:16:05 +02:00); live state was
  observed through the nerve `/state` instead.

## What the sensor does

`NERVE_INGRESS_LINT_CONFIG=<copy of config.yml>` (off when unset). Each live cycle it stats the copy, re-reads it only
when size/mtime/inode change (max 256 KiB), parses the `ingress:` list with `tunnel-lint`, and judges every local
origin against the nerve's own listener list (`/proc/net/tcp*`, no dials). One `ingress-closed-<port>` finding per
closed port (warn; error when the cloudflared journal window has refused dials to that port). Paths with a
`cloudflared` directory component (also through a symlink), `.json`/`.pem`/`.key`/`.crt` files and credentials-like
text are refused (`ingress-config-refused`) without showing content. Titles: ports and counts only; hostnames
stay in loopback evidence; B33 events carry neither.

## 1. Live VPS data, read 2026-10-21 08:11:55-08:13:03 +02:00

| read | result |
|---|---|
| `ps -C cloudflared` | 2 processes (root): pid 1198838 started 2026-10-18 19:19:30 +02:00, pid 1468472 started 2026-10-18 22:36:13 +02:00; only 1468472 has `--config /opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml` |
| `stat` config.yml | 3319 B, mode 644, owner root, mtime 2026-10-18 22:36:14 +02:00 |
| redacted copy | awk allow-list run on the VPS (only `ingress`/`hostname`/`path`/`service`/`originRequest` keys pass; `tunnel`, `credentials-file` -> `REDACTED`); credentials file never read. Rules identical to the 2026-10-20 tunnel-audit fixture |
| `ss -ltn` | 18 listeners (no process names) |
| cloudflared journal, last 60 min | refused dials to 12 ports, 3908 lines (1954 `Request failed`): 4001 1040, 4060 1034, 4000 572, 4042 548, 3011 488, 3005 78, 4003 56, 3004 40, 4444 30, 3010 10, 3020 6, 4020 6 |
| nerve `/state` 08:12:05 | cycle 6915; `cloudflared` component **degrading: tunnel origin errors 93/94 requests in 10 min**; NRestarts 0, MainPID 1468472 |

Fixtures (committed, secrets-free): `crates/openmind-nerve/tests/fixtures/ingress-lint/vps-config-20261021.yml`,
`vps-ss-20261021.txt`, `vps-refused-20261021.txt` (counts only, no hostnames or client addresses).

## 2. Replay on the redacted live copy (`tests/cortex_ingress_lint.rs`, 4 tests, deterministic)

```
replay: 39 rules, 33 closed routes on 21 ports, sense 494 us          (debug build)
journal: 3908 lines, error ports [3004, 3005, 3010, 3011, 3020, 4000, 4001, 4003, 4020, 4042, 4060, 4444],
         warn ports [3015, 3080, 3333, 4004, 4050, 5530, 7681, 8000, 8889]
```

- Closed ports from the sensor == `tunnel-lint` CLI path (`SsSnapshot`) on the same capture: 21 ports
  [3004, 3005, 3010, 3011, 3015, 3020, 3080, 3333, 4000, 4001, 4003, 4004, 4020, 4042, 4050, 4060, 4444, 5530, 7681,
  8000, 8889], 33 routes; open: 8671, 8091, 8080 (x2), 8650; catch-all `http_status:404`.
- 12 ports with failing requests in the journal -> error; 9 idle closed ports -> warn.
- One B33 event (`source ingress-lint`, `kind bleeding`, `severity high`, `ts 2026-10-21T08:12:04+02:00`), no
  hostname, no `127.0.0.1`, no `REDACTED`/`credentials` in events; the API JSON never carries `credentials-file`.
- A path-configured sensor in replay (`live = false`) reads nothing and reports nothing.

## 3. Public confirmation (HTTP GET from the box, 08:15:57-08:16:05 +02:00)

| hostname | port in config | sensor | GET / |
|---|---|---|---|
| one.galion.app | 8091 | open | 200 |
| tulu.galion.app | 5530 | warn (closed, idle) | 502 |
| slaviknomad.com | 4060 | error (1034 refused dials/h) | 502 |
| galion.studio | 4001 | error (1040 refused dials/h) | **502** |
| code.galion.app | 7681 | warn (closed, idle) | 502 |

These 4 GETs added 4 refused-dial pairs to the VPS cloudflared journal (expected; no other effect).

## 4. Real binary on the box (`tests/real_cortex_ingress_lint.rs`, `GALION_REAL=1`, `--release`)

```
GALION_REAL=1 cargo-slot test -j 2 --release -p openmind-nerve --test real_cortex_ingress_lint -- --nocapture
phase 1 (5.5 s, cycle 2): ["ingress-closed-20292", "ingress-closed-20293"]
phase 2 (4.6 s, cycle 3): ["ingress-closed-20294"] title "tunnel ingress: 2 route(s) to local port :20294 where nothing listens (cloudflared answers 502)"
nerve A: cycle_ms 23, rss 3652 KiB, ingress-lint 63 us, feed events from ingress-lint 1
nerve B (symlink -> cloudflared/): ["ingress-config-refused"], evidence ["path is inside a cloudflared directory (live config / credentials); point the sensor at a copy"]
test result: ok. 1 passed
```

4 runs 08:15:33, 08:19:28, 08:19:39, 08:19:50 +02:00: 4/4 pass; cycle_ms 44 / 23 / 23 / 23 (budget 50), RSS
5868 / 3652 / 3572 / 3632 KiB (budget 8 MiB), sensor 48-66 us. Phase 2 proves the sensor follows a new listener and
a rewritten copy without a restart. Nerve B proves a symlink into a `cloudflared` dir is refused and the
credentials-like marker text never appears in `/api/v1/state` or `/api/v1/cortex`. Ports 20290-20295 free afterwards.

## 5. Gates

- `cargo fmt --check` ok; `cargo clippy --workspace --all-targets -- -D warnings` (excl. openmind-sandbox,
  cgroup-detect, hermes-tools) ok.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`: 207 suites,
  1003 passed, 0 failed, 3 ignored (openmind-nerve lib 121 incl. 8 new sensor tests; replay 4/4).
- `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` 383 scanned, 0 missing; `repo-check`: 3 errors, 31 warnings, identical with and without this change (pre-existing `198.18.0.0` in sensors/surface.rs from cortex-s2-c).
- `openmind-nerve` + `tunnel-lint` release: x86_64 musl static-pie 1467168 B / 447152 B; aarch64 musl static
  1233528 B / 391656 B (rust-lld); aarch64-linux-android and armv7-unknown-linux-musleabihf `cargo check` ok.

## Deploying (owner decision, not done)

The live nerve does not run the cortex yet. When it does: copy the config as a redacted file the nerve user can read
(e.g. `/var/lib/galion-nerve/ingress-copy.yml`, regenerated by the same awk allow-list after each tunnel change) and set
`NERVE_INGRESS_LINT_CONFIG` on the unit. Pointing it at the live path is refused by design. Today it would report
21 closed ports, 12 of them error (galion.studio among them).
