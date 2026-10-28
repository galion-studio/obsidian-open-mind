<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# hermes-gateway deploy candidate (D-373-3 prep), 2026-10-28

Wave 4 slot 416, branch `swarm/hermes-gateway-candidate`, base `swarm/land-order-gateway-size` 83e573e2
(the slot 373 land tree). All times are Europe/Zagreb, CET (+01:00). **Nothing was deployed.** The gateway
deploy is its own window (D-373-3); this page and the staged drop-in are the inputs for that window.

## What the candidate is

| | value |
|---|---|
| name on the VPS | `/opt/galion/rust/bin/hermes-gateway-989958be` |
| source | 83e573e2 + cherry-pick of `swarm/crashloop-r3-rebase` 657a9988 (b06dca88 = 5ca48cc4 `--check-config` crashloop R3 fix, 989958be = its docs) |
| commit the bytes are from | 989958bed31f18f92dafc99059cd0eb384898c5e (later commits on this branch add only tests, a staged drop-in and docs) |
| target | x86_64-unknown-linux-musl, static-pie, RELR (`-zpack-relative-relocs`, via build.rs with rust-lld) |
| size | 1,952,592 B (budget 2,097,152 B in `config/release/size-budget.tsv`, headroom 144,560 B) |
| sha256 | `0f7bb47123628ff6b76231844991cd3bc577d5cd4eb541a0b8e29df158e38ac8` |
| replaces | live `hermes-gateway-b7fbbc78`, 2,732,248 B, sha256 `c5432a88ba1747d8fc1ff843546fe33cc6718e75abbccfa9ae585690c0dcc71f` (over budget, D-353-2) |

Why the fold: 657a9988 was not in the 373 land tree. Without it, `--check-config` (ExecStartPre in the
staged D-355-1 drop-in) exits 0 on a coding config whose worktree sits under `/var/lib`, then the service
crashloops at start. With it the candidate exits 2 (proved in the real test, P2). A plain merge of 657a9988
conflicts because that branch carries the b7fbbc78 survival commits; cherry-picking the two own commits is clean.

## Reproducible build recipe

Box toolchain: rustc 1.99.1 (58cf8f9c3 2026-10-13), rust-lld, C compiler = Debian `x86_64-linux-musl-gcc`
(gcc 14.2.0-19, musl-tools 1.2.5-3.1~deb13u1), the cc-rs default for the musl target. No CFLAGS, no
`CRATE_CC_NO_DEFAULTS`, no RUSTFLAGS (the repo `.cargo/config.toml` sets the linker flags).

```sh
export PATH=/workspace/swarm/bin:$PATH
git -C <repo> archive 989958be | tar -x -C src      # or any checkout of this branch
cd src
unset CFLAGS CRATE_CC_NO_DEFAULTS RUSTFLAGS CC AR
GALION_BUILD_COMMIT=989958bed31f18f92dafc99059cd0eb384898c5e SOURCE_DATE_EPOCH=1793206972 \
CARGO_TARGET_DIR=<fresh dir> cargo-slot build -j 2 --release --locked \
  -p hermes-gateway --target x86_64-unknown-linux-musl
sha256sum <fresh dir>/x86_64-unknown-linux-musl/release/hermes-gateway   # 0f7bb471...8ac8
```

`SOURCE_DATE_EPOCH` 1793206972 is the 989958be commit time (2026-10-28 18:02:52 CET). hermes-gateway carries
no build stamp (no commit in the binary or in `/health`), so neither variable changes the bytes: a second build
from the branch checkout (a different path) with `GALION_BUILD_COMMIT=0000…` and `SOURCE_DATE_EPOCH=1` gave
the same sha256 (see the real-test record). The path does not leak into the binary.

The C compiler does change the bytes. The same tree with zig 0.15.2 `cc` (zig clang 20.1.2) gave 2,088,128 B,
sha256 `47d3897a…`, only 9,024 B under budget; the plain 83e573e2 tree with zig gave 2,083,776 B. Box gcc is the
recipe slot 373 used for its land gates (1,948,240 B for 83e573e2; the fold adds 4,352 B). Owner decision D-416-1.

## Finding F-416-1: the live b7fbbc78 binary is not reproducible from its recorded recipe

Five rebuilds of b7fbbc78 with the recorded t291u recipe (zig cc, `CFLAGS="-O2 -fPIE"`, `CRATE_CC_NO_DEFAULTS=1`,
rust-lld) gave 2,739,872 B (solo, two different source paths, same sha afcf09e6…), 2,733,312 B
(`--workspace --bins` and `--all-targets`, sha c157a197…) and 2,681,728 B (without `CRATE_CC_NO_DEFAULTS`,
sha 4b504e32…). None is the live 2,732,248 B c5432a88…. So the parity test uses the exact live artifact,
`/workspace/swarm/artifacts/t291u/hermes-gateway-b7fbbc78`, whose sha256 equals the VPS file and its `.sha256`
— that is the byte-identical live build. The rollback target is the binary already on the VPS, so rollback
does not depend on rebuilding it. D-416-3 asks to record the exact command line for every future deploy.

## Staged drop-in (not installed)

`deploy/staged/hermes-rs.service.d/95-gateway-candidate.conf` only clears and resets `ExecStart` to the
candidate (and keeps `SyslogIdentifier=hermes-gateway`). It sets no environment and does not turn coding on,
so the coding-off live shape stays. `95-` sorts after the live `90-deploy-t291u.conf`, so it wins while present.
`crates/hermes-gateway/tests/staged_candidate.rs` pins all of that.

## Deploy-window runbook (for the owner-approved D-373-3 window only)

1. Copy the candidate to the VPS: `install -m 0755 hermes-gateway /opt/galion/rust/bin/hermes-gateway-989958be`
   and write `/opt/galion/rust/bin/hermes-gateway-989958be.sha256`; `sha256sum -c` must say OK.
2. Dry check: `/opt/galion/rust/bin/hermes-gateway-989958be --check-config` with the service environment (exit 0).
3. `install -m 0644 95-gateway-candidate.conf /etc/systemd/system/hermes-rs.service.d/`
4. `systemctl daemon-reload && systemctl restart hermes-rs`
5. Check: `systemctl show hermes-rs -p ActiveState -p NRestarts -p ExecStart` (active, 0, the candidate);
   `curl -s http://127.0.0.1:8642/health` 200 with `"implementation":"hermes-rs"`; nerve `:8670/state` 200.
6. Watch `journalctl -u hermes-rs` for 10 minutes; any restart = roll back.

Rollback (one step, back to live b7fbbc78 via `90-deploy-t291u.conf`):

```sh
rm /etc/systemd/system/hermes-rs.service.d/95-gateway-candidate.conf
systemctl daemon-reload && systemctl restart hermes-rs
```

Not folded (separate decisions): slot 397 tmpfiles for `/srv/hermes-coding`, D-355-1 `40-config-no-loop.conf`
(ExecStartPre `--check-config`). They can go in the same window but are not needed for the binary swap.
