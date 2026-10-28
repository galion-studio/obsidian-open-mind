<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Archive and GitHub safety record, 2026-10-20 (CEST +02:00)

Written 2026-10-20 23:34 CEST (+02:00) by swarm/swarm-record.

## 1. GitHub safety sweep

Method: for every worktree in `/workspace/swarm/wt/*`, record `git branch --show-current` and `HEAD`. Compare against a read-only `git ls-remote` of `galion-studio/obsidian-open-mind`, run on the VPS with the same askpass pattern as `remote-push.sh` (see TOOLING.md). The first sweep ran at 23:27. Branches missing from GitHub were pushed with `push-branch.sh` between 23:28 and 23:30 (fast-forward or new branch only, nothing forced). A second sweep at 23:32 confirmed every head.

Result: 68 worktrees, 68 branches. 66 heads are on GitHub under their own name. The other 2 (cutover-prep, hades-adversarial) point at a sha that is already on GitHub under another branch name, so nothing is at risk. No push was refused and nothing was non-fast-forward.

| Branch | Head sha | On GitHub |
|---|---|---|
| swarm/bench-banking | `adda6990e4a8` | yes |
| swarm/bench-harness | `ea8315c98a33` | yes |
| swarm/bench-hermes | `f7da2615ae1d` | pushed now (was missing) |
| swarm/bench-scoreboard | `f2ff16949eca` | yes |
| swarm/bench-vending | `a9daaf1e9fb0` | yes |
| swarm/bench-wallstreet | `4b1e1278ef32` | yes |
| swarm/build-info | `db9924dd5088` | yes |
| swarm/common-migrate-2 | `123eab282be9` | yes |
| swarm/common | `26d98b055bd9` | yes |
| swarm/console-static | `6bd3580baa79` | yes |
| swarm/console-ui-rust | `c242fed3e557` | yes |
| swarm/cortex-s2-a | `9557eb3dc08c` | pushed now (was missing) |
| swarm/cortex-s2-b | `51100e1dcd46` | pushed now (was missing) |
| swarm/cortex-s2-c | `dbec5c9bd65f` | yes (pushed by its own agent before 23:32) |
| swarm/cortex-s2-d | `f3468cea6bc5` | yes |
| swarm/cortex-sensor | `e53644648fc3` | yes |
| swarm/cortex-sources | `d692b9e53c3f` | yes |
| swarm/cutover-prep | `787a54d4dd37` | sha already on GitHub as swarm/deploy-rust (branch itself not pushed) |
| swarm/decision-memos | `1a0b851099f8` | yes |
| swarm/deploy-rust | `787a54d4dd37` | yes |
| swarm/deploy-sim | `c5ab8f96e788` | yes |
| swarm/desk-linkwatch | `a54c792acfc4` | yes |
| swarm/docs-sync | `80e28f377713` | yes |
| swarm/edge-boot | `753676e42b5b` | yes |
| swarm/flake-fix | `b2e69b9a41be` | yes |
| swarm/fuzz-sast | `e78b2e722980` | pushed now (was missing) |
| swarm/gaia-hades-tokens | `f3f41a2b0f80` | yes |
| swarm/gaia-land | `9d3ec18672c7` | yes |
| swarm/gaia-live | `4fbbb7f8e05f` | yes |
| swarm/gaia-poseidon | `498e3d298c3a` | yes |
| swarm/guard-rust | `d5f787471c03` | yes |
| swarm/hades-adversarial | `ccaab4203165` | sha already on GitHub as swarm/hades-approval (branch itself not pushed) |
| swarm/hades-approval | `ccaab4203165` | yes |
| swarm/hades-redteam | `e6ceca79b527` | yes |
| swarm/health-land | `f4a66ea1d573` | yes |
| swarm/hermes-core | `45b7980ef296` | yes |
| swarm/hermes-enroll | `f2121a8f79b7` | pushed now (was missing) |
| swarm/hermes-tools | `8ccdfcc4ba96` | yes |
| swarm/integration-1 | `8fd26f6ef568` | yes |
| swarm/integration-2 | `df42e0f01140` | yes |
| swarm/lan-discovery | `fe5f0bdc3deb` | yes |
| swarm/lan-routing | `e8169c99e08d` | yes |
| swarm/landing-overview | `669dc18f10af` | yes |
| swarm/launch-gates | `79ee76101261` | yes |
| swarm/memory-real | `723b46c855dc` | yes |
| swarm/memory-schema | `79526264f07e` | yes |
| swarm/nerve-wiring | `8d7a924047be` | pushed now (was missing) |
| swarm/nerve | `5802d0e1dd69` | yes |
| swarm/one-hardening | `fed3de436b47` | yes |
| swarm/one-probe | `b44cd4f00990` | yes |
| swarm/poseidon | `7d0c478375f9` | yes |
| swarm/real-harness | `5b3c662e6110` | yes |
| swarm/room | `5bb27e16e23b` | yes |
| swarm/rust-ports | `26a2a8e8971c` | yes |
| swarm/sandbox | `d194c19421b5` | yes |
| swarm/sec-runbooks | `c5fc269a1b60` | yes |
| swarm/settings-snapshot | `b7afd6d0d6ac` | yes |
| swarm/slavik-todo | `87fdd91bb999` | yes |
| swarm/sqlite-pure-spike | `98686ef129dd` | yes |
| swarm/survival-heal | `45a18aa92892` | yes |
| swarm/swarm-record | `32fadd521b6a` | pushed now (this record) |
| swarm/tuner-cutover | `5fd1611ac78c` | yes |
| swarm/tunnel-audit | `d35ef73c441f` | pushed now (was missing) |
| swarm/ucpu | `3e70ffb0811f` | yes |
| swarm/voice-mouth | `5d776c1caaf9` | yes |
| swarm/voice-rust | `03d21a9e68ce` | yes |
| swarm/vps-baseline | `f403eeb232f3` | yes |
| swarm/whitepaper-evidence | `8c0ac37df5f8` | yes |

## 2. VPS archive

- Path: `/opt/galion/archive/swarm-20261020-233213+0200/` on `<vps>`. This is a new directory, owned by root with mode 700. No service, unit or working copy was touched.
- Contents: `files/` (the non-secret copy of `/workspace/swarm`, 963 files), `repo-docs/swarm/` (this folder at commit 32fadd5), `swarm-branches-20261020-233213+0200.bundle` (git bundle with 95 refs: every base.git branch plus all 68 `swarm/*` worktree heads), `bundle-refs.txt`, `github-heads-ls-remote.txt`, `worktree-branch-status.txt`, `excluded-secret-like-paths.txt`, `README.md`, `SHA256SUMS`.
- Left out on purpose: `target/` and `wt/` build trees, `base.git` (covered by the bundle), `cache/` (RustSec advisory-db, downloadable again), `sim/*/src`, `sim/*/bin` and `sim/*/root-*` (rebuildable checkouts and binaries; the run logs are kept), `all.bundle` (18:44 bundle, replaced by the new one).
- Manifest: `SHA256SUMS` has 1000 entries. Its own sha256 is `f54f9282d0951dfdf63444acba8e832428365cdd745c20a841b0f2803969366a`, the same on the box and on the VPS.
- Verified on the VPS at 23:32 CEST: `sha256sum -c --quiet SHA256SUMS` gave MANIFEST-OK with 0 failures. `git bundle verify` in a scratch bare repo under /tmp, removed afterwards, gave BUNDLE-OK with 68 `swarm/*` heads.
- Restore: `git clone /opt/galion/archive/swarm-20261020-233213+0200/swarm-branches-20261020-233213+0200.bundle repo`.
- Note: the bundle carries the repository's full history, including any old blobs that SEC-1..5 plans to purge. It sits only on the VPS, in a root-only directory, next to the secrets that the VPS already holds.

## 3. Secret-like paths excluded (by path only; contents never printed)

A filename scan (key, token, secret, credential, .env, .pem and similar) and a content-pattern scan (GitHub, OpenAI-style and AWS key shapes, private-key blocks, JWTs, `secret=`/`token=`/`password=` with long values) matched these paths. They are not in the archive or on GitHub:

- `artifacts/console-static/out/client/assets/index-8epzlwHQ.js`
- `artifacts/console-static/src-2169d1c/src/lib/app-data/app-data.test.ts`
- `artifacts/console-static/src-2169d1c/src/lib/auth/server.ts`
- `artifacts/console-static/src-2169d1c/src/lib/auth/email-password.ts`
- `artifacts/console-ui/live-src-20261020-2105/src/lib/app-data/app-data.test.ts`
- `artifacts/console-ui/live-src-20261020-2105/src/lib/auth/server.ts`
- `artifacts/console-ui/live-src-20261020-2105/src/lib/auth/email-password.ts`
- `artifacts/console-ui/live-src-20261020-2125/src/lib/app-data/app-data.test.ts`
- `artifacts/console-ui/live-src-20261020-2125/src/lib/auth/server.ts`
- `artifacts/console-ui/live-src-20261020-2125/src/lib/auth/email-password.ts`
- `evidence/integration-1/realtest/stack/fixture/one-store/admin-device.json`
- `evidence/integration-1/realtest/stack/fixture/one-nostore/admin-device.json`
- `evidence/integration-1/realtest/stack/fixture/one-empty/admin-device.json`
- `evidence/integration-1/realtest/stack-nerve/fixture/one-store/admin-device.json`
- `evidence/integration-1/realtest/stack-nerve/fixture/one-nostore/admin-device.json`
- `evidence/integration-1/realtest/stack-nerve/fixture/one-empty/admin-device.json`
- `evidence/integration-1/real-integration-1/stack/fixture/one-store/admin-device.json`
- `evidence/integration-1/real-integration-1/stack/fixture/one-nostore/admin-device.json`
- `evidence/integration-1/real-integration-1/stack/fixture/one-empty/admin-device.json`
- `evidence/integration-1/real-integration-1/stack-nerve/fixture/one-store/admin-device.json`
- `evidence/integration-1/real-integration-1/stack-nerve/fixture/one-nostore/admin-device.json`
- `evidence/integration-1/real-integration-1/stack-nerve/fixture/one-empty/admin-device.json`

The live console source snapshots (`artifacts/console-ui/live-src-*`, `artifacts/console-static/src-*`) are only in the VPS archive, not in git, and only after these exclusions.

## 4. What stays box-only

Build output (`/workspace/swarm/target`), the worktrees themselves (their committed state is on GitHub and in the bundle), and uncommitted edits by agents still running at 23:32. Those agents push their own branches when they finish.
