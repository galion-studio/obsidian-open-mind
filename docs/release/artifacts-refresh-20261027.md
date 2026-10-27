<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Release artifacts refresh — 2026-10-27 22:00 (Europe/Zagreb CET +01:00)

- Wave-4 slot **312** `release-artifacts-refresh`, follow-up to slot **275** (`docs/release/artifacts-20261027.md` on `swarm/release-artifacts-matrix` 319685c8) and slot **294** (`swarm/release-size-trim` b2d15657, D-294-3 "rebuild with default cc-rs flags").
- Source: `swarm/main-staging` **38a07700** (PR #13 merge, 21 checks green). base.git only had 98466ab9, so the first commit on this branch is a **tree snapshot** of 38a07700 on top of 98466ab9: the 19 changed files were taken from the GitHub compare API, every blob SHA was checked, and the resulting tree **4864589f** is byte-identical to the tree of GitHub 38a07700. The history differs from GitHub (one snapshot commit instead of the 7 PR #13 commits); the content does not.
- Box artifacts: `/workspace/swarm/artifacts/release-20261027-r312/` (`<triple>/<bin>-<tag>`, `SHA256SUMS`, `logs/`). **Not deployed.** No unit, DNS, HADES or secrets changes.
- Real-test evidence: `docs/real-tests/release-artifacts-refresh.md` (21 checks on the x86_64 release bytes, reproducibility, VPS read-only). Ports used: 21820-21821 (21829 as a dead upstream), all free afterwards.

## 1. Build recipe (D-294-3 applied)

| Item | slot 275 | **this refresh** |
|---|---|---|
| C compiler | zig cc, `CFLAGS="-O2 -fPIE"`, `CRATE_CC_NO_DEFAULTS=1` | zig 0.15.2 cc wrappers exactly as `scripts/ucpu-gate.sh`; **no CFLAGS, no CRATE_CC_NO_DEFAULTS** (cc-rs keeps `-Oz -ffunction-sections -fdata-sections`) |
| Linker | rust-lld | rust-lld |
| Cargo | `--release` | `cargo build --release --locked -j 4` (cargo-slot, nice 10), workspace profile unchanged (opt z, fat LTO, cgu 1, panic abort, strip) |
| Provenance | `GALION_BUILD_COMMIT=7c95348b…` | `GALION_BUILD_COMMIT=38a07700…`, `GALION_BUILD_TIME=2026-10-27T22:00:00+01:00`, `SOURCE_DATE_EPOCH=1793134800` |

Exact command (the package set matters, D-312-4), once per target:

```
cargo build --release --locked -j 4 --target <x86_64|aarch64>-unknown-linux-musl \
  -p openmind-nerve -p hermes-gateway -p openmind-memory -p openmind-queue -p galion-one \
  -p galion-health -p galion-desk -p link-watch -p tuner -p console-static -p galion-backup -p desk-hermes
# cortex-staging-fold 77df80f9:     -p gaia-cortex --features llm-advisor
# console-rust-serve-impl 2d846fb8: -p console-static --bin galion-console-serve
```

The slot 294 `.cargo/config.toml` trim (unwind tables off for C, five SQLite omits) is **not** on 38a07700 (D-294-2 still open), so it is not in these artifacts.

## 2. Live Rust units covered (VPS read-only 2026-10-27 22:00 CET)

| Live unit | Live ExecStart binary | Built here | Source |
|---|---|---|---|
| galion-nerve | openmind-nerve-9d1a4f3 | openmind-nerve-38a0770 | main-staging 38a07700 |
| hermes-rs | hermes-gateway-b7fbbc78 | hermes-gateway-38a0770 | main-staging 38a07700 |
| openmind-memory | openmind-memory-aa77864 | openmind-memory-38a0770 | main-staging 38a07700 |
| openmind-queue | /usr/local/bin/openmind-queue | openmind-queue-38a0770 | main-staging 38a07700 |
| galion-one | galion-one | galion-one-38a0770 | main-staging 38a07700 |
| galion-health | galion-health | galion-health-38a0770 | main-staging 38a07700 |
| galion-desk (+ DESK_HERMES_BIN) | galion-desk, desk-hermes | galion-desk-38a0770, desk-hermes-38a0770 | main-staging 38a07700 |
| link-watch-rs | link-watch | link-watch-38a0770 | main-staging 38a07700 |
| tuner | /opt/galion/tuner/bin/tuner | tuner-38a0770 | main-staging 38a07700 |
| galion-backup (timer) | galion-backup-98466ab | galion-backup-38a0770 | main-staging 38a07700 |
| console / console-rust-primary | galion-console-serve-2d846fb8 | galion-console-serve-2d846fb (+ galion-console-static-38a0770) | **not on main-staging**: `swarm/console-rust-serve-impl` 2d846fb8 |
| gaia-cortex | gaia-cortex-73583ab-llm | gaia-cortex-77df80f-llm (`--features llm-advisor`) | **not on main-staging**: `swarm/cortex-staging-fold` 77df80f9 (= 98466ab9 + cortex-live-merge-3 73583abf) |

New vs slot 275: galion-backup, desk-hermes and the console (both Rust and live since 275) are now in the matrix.

## 3. Size table (bytes, stripped, static-pie)

Budget: `scripts/size-budget.sh` on 38a07700 = 2 MiB (2,097,152) per binary, **hermes-gateway 2.5 MiB (2,621,440)** (raised by PR #13).

| Binary | 275 x86_64 | **x86_64** | Δ vs 275 | live x86_64 | 275 aarch64 | **aarch64** | Budget |
|---|---:|---:|---:|---:|---:|---:|---|
| openmind-nerve | 1,918,568 | **1,923,432** | +4,864 | 1,458,760 (9d1a4f3) | 1,587,984 | **1,591,824** | ok |
| hermes-gateway | 2,740,032 | **2,313,344** | **−426,688** | 2,732,248 (b7fbbc78) | 2,449,944 | **2,045,952** | ok (2.5 MiB); x86_64 still > 2 MiB |
| openmind-memory | 2,241,376 | **1,843,616** | **−397,760** | 1,702,640 (aa77864) | 2,074,624 | **1,692,200** | ok |
| openmind-queue | 894,472 | **894,472** | 0 (sha identical) | 889,632 | 763,096 | **763,096** | ok |
| galion-one | 961,256 | **961,256** | 0 | 963,904 | 845,592 | **845,592** | ok |
| galion-health | 557,256 | **557,256** | 0 (sha identical) | 503,512 | 496,888 | **496,888** | ok |
| galion-desk | 757,736 | **757,736** | 0 | 403,056 | 665,400 | **665,400** | ok |
| link-watch | 784,952 | **784,952** | 0 | 508,248 | 678,664 | **678,664** | ok |
| tuner | 477,672 | **477,672** | 0 | 480,032 | 430,016 | **430,016** | ok |
| galion-console-static | — | **936,520** | new | — | — | **799,128** | ok |
| galion-backup | — | **1,533,488** | new | 1,959,376 (98466ab) | — | **1,427,984** | ok (−425,888 vs live) |
| desk-hermes | — | **1,447,376** | new | 1,427,792 | — | **1,349,952** | ok |
| gaia-cortex (llm) | 1,082,088 (73583ab, no llm) | **1,152,840** (77df80f-llm) | +70,752 (llm feature) | 1,136,200 (73583ab-llm) | 923,920 | **980,032** | ok |
| galion-console-serve | — | **949,064** (2d846fb) | new | 949,568 (2d846fb8) | — | **809,064** | ok |

`size-budget.sh --check-dir`: aarch64 **ok (14 binaries)**; x86_64 **ok (14 binaries)** after the fix in section 6 (before it: FAIL on `hermes-gateway-38a0770`, a naming bug, see D-312-2).

Reading the table:
- The two slot-275 overshoots were the build recipe (D-294-3), not code: with default cc-rs flags hermes-gateway drops 426,688 B and openmind-memory 397,760 B; galion-backup (also SQLite) is 425,888 B smaller than the live 98466ab build, which was evidently built the slot-275 way too.
- openmind-queue and galion-health are **bit-for-bit identical** to slot 275 on x86_64 (queue also on aarch64): reproducible builds across two runs and two staging tips. galion-one, galion-desk, link-watch and tuner differ from slot 275 in exactly 38 bytes each (`cmp -l`): the embedded build commit; code is unchanged.
- hermes-gateway x86_64 is 216,192 B over 2 MiB but inside its own 2.5 MiB budget. Applying slot 294 (D-294-1/2) would bring it to 2,068,288 B.
- The live nerve (1,458,760 B) is smaller than the staging nerve (1,923,432 B): 9d1a4f3 is an older, narrower build; not investigated here.

## 4. SHA256 manifest

Also on disk: `/workspace/swarm/artifacts/release-20261027-r312/SHA256SUMS` (+ per-triple `SHA256SUMS`).

```
25d2181595aa5d0937deb6d1d12f046a90a78d10d4ef606e4925063e850be1c4  x86_64-unknown-linux-musl/desk-hermes-38a0770
603daa34c7875234d99df1e78fc0be324e60c2fa0b60a4b2449bacf3254d3b69  x86_64-unknown-linux-musl/gaia-cortex-77df80f-llm
668b0a0646e80223537513017a80895c2c4ed39bceb949bdae94a4e0f1c32bd8  x86_64-unknown-linux-musl/galion-backup-38a0770
3674348e23596f768fa947868b6995b24e05bd1b622c2adc919a22858ecf0778  x86_64-unknown-linux-musl/galion-console-serve-2d846fb
b1e0cc08b40e19014c6136f749591c0dab3abb1f95fa2d350d05711d4068a632  x86_64-unknown-linux-musl/galion-console-static-38a0770
11cc148b22f33cda6f088997f8fa1df1746997a7a4453d2c12730b217463ed89  x86_64-unknown-linux-musl/galion-desk-38a0770
3fdc6f2176ba292a6dde2eb5c253d9733481bb04b7003f84fbbc0bf583369225  x86_64-unknown-linux-musl/galion-health-38a0770
49d937089285f6288c80f79c5dc993a6066ee3e9543fd15a713d39b9b7f484cd  x86_64-unknown-linux-musl/galion-one-38a0770
344f804efd2e9f3d7eb0109f69b352420bc89942ebfba7a78c50c1e3c49083ec  x86_64-unknown-linux-musl/hermes-gateway-38a0770
27ed88b01266cf8e12323256191b5ba0df71269b66e9a41711aa3bf233e2cd04  x86_64-unknown-linux-musl/link-watch-38a0770
f8b3339c14326e5f9fd428ccfddbbe58826e3a87bbdf1b34ac739e6587f344e5  x86_64-unknown-linux-musl/openmind-memory-38a0770
689dc81fa212772432dfa4a8c9a4a81b662dd306fb88026efd37d51a67825f67  x86_64-unknown-linux-musl/openmind-nerve-38a0770
940a4d1c002ec80e4a50585028fada62ef656637ade041ff20a62befc80723c1  x86_64-unknown-linux-musl/openmind-queue-38a0770
8d5b9eafbcfac9bd6d1b2c7cf17f93ec352984cbbb8037de782071c8eac247bf  x86_64-unknown-linux-musl/tuner-38a0770
a9cea82d90316e205b6e144335dd618417209a889518a5de86cb9758cf451ef2  aarch64-unknown-linux-musl/desk-hermes-38a0770
ed731d5d03b0246b123d14a6c71101326d4fe7d44723b6d636a3a7da0b209480  aarch64-unknown-linux-musl/gaia-cortex-77df80f-llm
276c6767b813a79d06c0c202ae01fdba416cef850443cd6a44cce660a9059a66  aarch64-unknown-linux-musl/galion-backup-38a0770
dc394d219624fdd43d5e26d69ebcf99cdd3aefac188f975f670189d59cc7a612  aarch64-unknown-linux-musl/galion-console-serve-2d846fb
4a16fb781985717074796761659d94f3dcb71a5df488cf3f5a3635ef3e6c67ea  aarch64-unknown-linux-musl/galion-console-static-38a0770
0594ee2930125c1f0bf38a4815c8c65c4ab7ac5ecccd2243b2454adc5ee98bc0  aarch64-unknown-linux-musl/galion-desk-38a0770
047114d5876d15ae5d2c4abc704ca0e5045636069377cdff8029bae0b4abbe11  aarch64-unknown-linux-musl/galion-health-38a0770
6b443021fd2f5d2f008eb1dcd81cfb929d2a5358c352899d631103f9db6e0a4a  aarch64-unknown-linux-musl/galion-one-38a0770
760b22b0a72757f5aa52b465173433201848fe8fadbf1c831deba049af503b37  aarch64-unknown-linux-musl/hermes-gateway-38a0770
021ce987db4aa6589043e452edfa1266c97b2d439f1574521499740a3e3e848e  aarch64-unknown-linux-musl/link-watch-38a0770
809ea7c7294593789d8ccfafa42f2a2083ae66bbc46fdc7035a468c2f7a6483b  aarch64-unknown-linux-musl/openmind-memory-38a0770
c9ae0778b733596a699d2eb22fa06662d455e90d180f3d51b0e485b90c91269c  aarch64-unknown-linux-musl/openmind-nerve-38a0770
eefeecb692ac596ca2683140dd4e92c57b9ef8fc45f0982d0cb2bbaa8496bd08  aarch64-unknown-linux-musl/openmind-queue-38a0770
2e62e0e65570c066462f228c0a783900f2e49f5c5a3d3cfd02d11358763d58e4  aarch64-unknown-linux-musl/tuner-38a0770
```

## 5. Static / machine checks

All 28 ELFs: no PT_INTERP, no DT_NEEDED, no `.symtab` (stripped), e_machine x86-64 / AArch64 as expected (`logs/verify-static.log`). The real test re-checks PT_INTERP, `.symtab` and e_machine from the bytes for the host triple.

## 6. Change on this branch: `scripts/size-budget.sh`

The hermes-gateway 2.5 MiB cap matched only the exact file name `hermes-gateway`. Tagged release artifacts (`hermes-gateway-38a0770`, live `hermes-gateway-b7fbbc78`) fell back to 2 MiB and failed. The cap now also applies to `hermes-gateway-<7+ hex>[...]`. Checked: `--check-dir` on the x86_64 artifact dir FAIL → ok; a copy named `hermes-gateway-tool` still gets 2 MiB and FAILs (no wildcard widening). `bash -n` ok.

## 7. Gaps

| Item | Status |
|---|---|
| aarch64 artifacts executed | **NO**: no qemu-user on the box (apt mirror SSL errors, package lists unavailable) and the VPS is x86_64. aarch64 is covered by build + ELF checks only. CI (`real_toolchain_aarch64`, PR #13) runs aarch64 under qemu. |
| gaia-cortex / galion-console-serve on main-staging | **ABSENT**: built from their branches (section 2). One staging SHA cannot yet build every live unit (D-275-2 still open). |
| galion-console-static / galion-backup `--version` | no version flag (console-static exits 2, backup prints usage, exit 1). Exercised for real instead (HTTP / snapshot+verify). D-312-3. |
| hermes-gateway `--version` | still enters the start path (D-228-4). |

## 8. Owner decisions

- **D-312-1** Use this matrix (default cc-rs flags) as the publish recipe; the live hermes-gateway (2,732,248 B) and galion-backup (1,959,376 B) were built with the old flags. Deploy is out of scope here and needs its own approval.
- **D-312-2** Merge the `size-budget.sh` tagged-name fix (section 6) into main-staging.
- **D-312-3** Add `--version` to galion-console-static/serve and galion-backup (one line each via `galion_common::build_info`), so every release binary self-reports its commit.
- **D-312-4** Pin the release package set: artifact bytes depend on which `-p` packages are built together (feature unification). This matrix is reproducible 12/12 with the exact set in `docs/real-tests/release-artifacts-refresh.md` section 2; solo builds differ.
- Still open from earlier slots: **D-294-1/2** (size trim + coding opt-in, would put hermes-gateway x86_64 under 2 MiB), **D-275-2** / **D-293-4** (land gaia-cortex in main-staging), console-rust-serve-impl landing.
