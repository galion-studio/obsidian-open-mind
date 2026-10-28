<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real test: console-studio-names-retire (slot 433, 2026-10-28 23:12 CET +01:00)

Branch `swarm/console-studio-names-retire` off console-deploy-candidate-2
c2522a99 (+ slot 414 docs). Ports 23030-23039. No deploy, no live change, no
DNS / tunnel change. VPS read-only.

## Command

```
GALION_REAL=1 CONSOLE_433_BASE_BIN=<c2522a99 release galion-console-static> \
  cargo test --release -p console-static --test real_studio_names_retire -- --nocapture
```

Starts the real release binary (new) and the c2522a99 binary (base), each in
config A (`--ui rust --upstream <closed 23039>`, new :23030 / base :23032) and
config B (`--dist <tmp> --ui rust --admin-store <tmp>`, live console-dist
layout, new :23031 / base :23033). 5 paths x 14 Host headers per config.
Result 23:12:21 +01:00: **1 passed** (also passed at 23:11). `live-host rows
identical to base: 70; retired rows changed: 24`.

## Key rows (new | base)

| cfg | Host | / | /console | /agents | /api/v1/health |
|---|---|---|---|---|---|
| A | 127.0.0.1 | 302 \| 302 | 200 \| 200 | 200 \| 200 | 502 \| 502 |
| A | console.galion.app | 302 \| 302 | 503 \| 503 | 503 \| 503 | 502 \| 502 |
| A | console.galion.studio | **200** \| 302 | **200** \| 503 | **200** \| 503 | 502 \| 502 |
| A | dashboard.galion.studio | **200** \| 302 | **200** \| 503 | **200** \| 503 | 502 \| 502 |
| A | galion.app (unknown) | 200 \| 200 | 200 \| 200 | 200 \| 200 | 502 \| 502 |
| B | 127.0.0.1 | 302 \| 302 | 200 \| 200 | 200 \| 200 | 200 \| 200 |
| B | console.galion.app | 302 \| 302 | 200 \| 200 | 200 \| 200 | 401 \| 401 |
| B | console.galion.studio | **200** \| 302 | 200 \| 200 | 200 \| 200 | 401 \| 401 |
| B | dashboard.galion.studio | **200** \| 302 | 200 \| 200 | 200 \| 200 | 401 \| 401 |
| B | galion.app (unknown) | 200 \| 200 | 200 \| 200 | 200 \| 200 | 401 \| 401 |

Asserted: every retired form (plain, `www.`, `:443`, trailing dot, mixed
case) returns the same status AND body as the unknown-host control
(galion.app == evil.example) on all paths in both configs; every live name
(console.galion.app incl. `www.`/`:443`/trailing dot, dashboard.galion.app,
console.example.org, 127.0.0.1) differs from the unknown host on at least one
path and has the same status as base on every row; on base the retired names
answered byte-identically to console.galion.app (negative control). Loopback
`/api/v1/domains` lists console.galion.app and neither retired name; services
no longer show dashboard.galion.studio. Status 200 on B `/console` is shared by all hosts; the retired rows are
still asserted byte-identical to the unknown-host body.

## Live VPS (read-only GET via ssh, 23:12:28 +01:00)

Live :8080 (console 5aa31927) today, Host header only:

| Host | / | /console | /api/v1/health |
|---|---|---|---|
| console.galion.app | 302 | 200 | 401 |
| console.galion.studio | 302 | 200 | 401 |
| dashboard.galion.studio | 302 | 200 | 401 |
| galion.app | 200 | 200 | 401 |
| 127.0.0.1 | 302 | 200 | 200 |

= base config B exactly; after this change ships the two .studio rows become
the galion.app row. Harmless today (no DNS record). console.service and
console-rust-primary.service active. Nerve /state 200: cycle 24, score 68,
grade D (cortex is live; this slot read only the nerve).

## Other tests (box, 2026-10-28 23:13-23:25 +01:00)

* `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets
  --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` ok.
* `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect
  --exclude hermes-tools`: 395 suites, 2111 passed, 0 failed, 18 ignored
  (incl. gate unit test + board 13 domains + ui.rs Up tab).
* GALION_REAL on ports 23030+: real_landing 1/1, real_merge 1/1, real_port2-5
  4/4, real_route31 1/1, console-ui real_port1 1/1 (read-only ssh forward to
  live 8080/8646/8670).
* x86_64 musl release 1,041,280 B, aarch64 musl release 878,992 B;
  aarch64-linux-android + armv7-unknown-linux-musleabihf `cargo check` ok
  (armv7 gnueabihf std not installed on the box).
* no-python ok, spdx-scan 0 missing; repo-check 2 errors = pre-existing on
  base (hard-coded IPv4 in real_admin_flags_live.rs, fixed separately by
  slot 425 bf5ffc7d), 86 warnings.
