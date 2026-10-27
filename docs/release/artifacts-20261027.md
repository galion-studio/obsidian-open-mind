<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Release artifacts matrix — 2026-10-27 (Europe/Zagreb CET +01:00)

- Wave-4 slot **275** `release-artifacts-matrix`.
- Branch `swarm/release-artifacts-matrix` off `swarm/main-staging` **7c95348b38f6**.
- Box artifacts: `/workspace/swarm/artifacts/release-20261027/` (not deployed).
- Targets: `x86_64-unknown-linux-musl` + `aarch64-unknown-linux-musl` (rust-lld, zig 0.15.2 cc, `CFLAGS=-O2 -fPIE`, `CRATE_CC_NO_DEFAULTS=1`).
- Provenance env: `GALION_BUILD_COMMIT=7c95348b38f6…`, `GALION_BUILD_TIME=2026-10-27T17:00:00+01:00`, `SOURCE_DATE_EPOCH=1793116800`.
- **No deploy.** No unit / DNS / secrets changes. Ports used for box probes: 21450–21454.

## 1. Live Rust services covered

| Live unit (VPS) | Binary built | Source tree | Notes |
|---|---|---|---|
| galion-nerve | openmind-nerve-7c95348 | main-staging 7c95348b | |
| gaia-cortex | gaia-cortex-73583ab | **not in main-staging**; built from `swarm/cortex-live-merge-3` **73583abf** | crate absent on 7c95348b |
| hermes-rs | hermes-gateway-7c95348 | main-staging 7c95348b | `hermes-rs` is a library; live unit runs hermes-gateway |
| openmind-memory | openmind-memory-7c95348 | main-staging 7c95348b | SQLite / zig cc |
| openmind-queue | openmind-queue-7c95348 | main-staging 7c95348b | |
| galion-one | galion-one-7c95348 | main-staging 7c95348b | |
| galion-health | galion-health-7c95348 | main-staging 7c95348b | |
| galion-desk | galion-desk-7c95348 | main-staging 7c95348b | |
| link-watch-rs | link-watch-7c95348 | main-staging 7c95348b | |
| tuner | tuner-7c95348 | main-staging 7c95348b | |
| console (Node live) | — | console-ui is **lib-only** on main-staging (no `main.rs` / `[[bin]]`) | Rust console V2 not cut over; live is Node `console-dist` |

## 2. Size table (bytes)

| Binary | x86_64-musl B | aarch64-musl B | ≤2 MiB? |
|---|---:|---:|---|
| openmind-nerve-7c95348 | 1918568 | 1587984 | yes |
| hermes-gateway-7c95348 | 2740032 | 2449944 | NO both |
| openmind-memory-7c95348 | 2241376 | 2074624 | NO x86_64 |
| openmind-queue-7c95348 | 894472 | 763096 | yes |
| galion-one-7c95348 | 961256 | 845592 | yes |
| galion-health-7c95348 | 557256 | 496888 | yes |
| galion-desk-7c95348 | 757736 | 665400 | yes |
| link-watch-7c95348 | 784952 | 678664 | yes |
| tuner-7c95348 | 477672 | 430016 | yes |
| gaia-cortex-73583ab | 1082088 | 923920 | yes |

Size-budget overshoots (ucpu 2 MiB = 2,097,152 B stripped): **hermes-gateway** x86_64 2,740,032 B (130.7%) and aarch64 2,449,944 B (116.8%); **openmind-memory** x86_64 2,241,376 B (106.9%) — aarch64 memory 2,074,624 B is under budget. Owner: D-275-1 keep shipping oversize vs cut features (see D-216-3).

## 3. SHA256 manifest

Also on disk: `/workspace/swarm/artifacts/release-20261027/SHA256SUMS` (and per-target `*/SHA256SUMS`).

```
dab210e16295252a63db70ca8d6e9a79e2c6043ef41d43bdc7eb382afbe2ca63  x86_64-unknown-linux-musl/gaia-cortex-73583ab
b950725d2a6afc3666ed96871d98819b4c3131b496cc50cbf46365f8f309cde6  x86_64-unknown-linux-musl/galion-desk-7c95348
3fdc6f2176ba292a6dde2eb5c253d9733481bb04b7003f84fbbc0bf583369225  x86_64-unknown-linux-musl/galion-health-7c95348
4fdef5400563b2c14893a73449a38a474a86b9781fc644f37305c2b52b9c8839  x86_64-unknown-linux-musl/galion-one-7c95348
1f0d472dba308f5ad4494576c60f188da94b867e2c9e96530493575d04207b65  x86_64-unknown-linux-musl/hermes-gateway-7c95348
c413fa6900c2b3ed49e3838bed743eccedad67f3598158d9d039556be8dae153  x86_64-unknown-linux-musl/link-watch-7c95348
1dcd1ff02ed601a754f79e276a8311e3ee305da5ebcc5d92b8a5ee9331b20da2  x86_64-unknown-linux-musl/openmind-memory-7c95348
ef1e45cdad121c064cad9e6836930844550f342221ea0d23d52a51fb8c1f3e41  x86_64-unknown-linux-musl/openmind-nerve-7c95348
940a4d1c002ec80e4a50585028fada62ef656637ade041ff20a62befc80723c1  x86_64-unknown-linux-musl/openmind-queue-7c95348
a2e87572a1f596b5d53714f502c5d955e36c1c4b26dbcca170e2d6c5788bcd1f  x86_64-unknown-linux-musl/tuner-7c95348
114c3a53cbccbc86bf2d194ad964a7752e00a8555ec349f3b6521603da84baa0  aarch64-unknown-linux-musl/gaia-cortex-73583ab
8569356d70fb27e9a854ca92c9c858a7ade1d2eb9856527311f1250d2d3298c7  aarch64-unknown-linux-musl/galion-desk-7c95348
047114d5876d15ae5d2c4abc704ca0e5045636069377cdff8029bae0b4abbe11  aarch64-unknown-linux-musl/galion-health-7c95348
8ffd27a95d78a5aaabaafd87d02226336149f6cb8dbe263d2c42a4ec32004342  aarch64-unknown-linux-musl/galion-one-7c95348
af691bbed5c14d8363669bed2bbfc7622a79c69aee6d881ed1098f28dd10e4aa  aarch64-unknown-linux-musl/hermes-gateway-7c95348
9d667ff2af5a167f995c1336aec480c66faf6d76759789ff829d059ca736b1d5  aarch64-unknown-linux-musl/link-watch-7c95348
5f67971ca237a7c763e74bef473c419a1274b709c032f1a6e0f787b0237e2e0a  aarch64-unknown-linux-musl/openmind-memory-7c95348
84ff0f5a3f8c0da7b56e0bf017053dd1377c9e0d0c50552b359e3845340beedf  aarch64-unknown-linux-musl/openmind-nerve-7c95348
eefeecb692ac596ca2683140dd4e92c57b9ef8fc45f0982d0cb2bbaa8496bd08  aarch64-unknown-linux-musl/openmind-queue-7c95348
d4946d4adae3ad85f80be6cf7c5c740edd237aea46543661ea6bb62350c24377  aarch64-unknown-linux-musl/tuner-7c95348
```

## 4. Static / machine checks

All 20 ELFs: no PT_INTERP, no DT_NEEDED, e_machine matches target. Log: `logs/verify-static.log` under the artifact dir.

## 5. Build failures / gaps

| Item | Result |
|---|---|
| 9 main-staging live bins × 2 musl | **OK** (exit 0) |
| gaia-cortex on main-staging | **ABSENT** — built from cortex-live-merge-3 73583ab instead |
| console-ui binary | **ABSENT** — lib crate only on this tip |
| hermes-gateway `--version` | still enters start path (refuses without API_SERVER_KEY); known D-228-4 |
| First x86_64 attempt with host `cc` linker | FAIL PIE/sqlite; fixed by rust-lld + `-fPIE` (ucpu-gate) |

## 6. Owner decisions

- **D-275-1** Size budget: allow hermes-gateway / openmind-memory over 2 MiB for this publish, or cut SQLite / features before tag.
- **D-275-2** Land `gaia-cortex` into main-staging before a main tag so one SHA builds every live Rust unit.
- **D-275-3** Deploy cutover of these musl artifacts (nerve/cortex/hermes/…) — **out of scope**; needs separate Mate/Slavik approve.
- **D-275-4** console-ui: ship Node dist only vs add a Rust `[[bin]]` for V2 on the next staging tip.
