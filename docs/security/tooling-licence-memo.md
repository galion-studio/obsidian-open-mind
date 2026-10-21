<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Decision memo: GPL/LGPL tools we run, kernel constants, libFuzzer, CodeQL

For Slavik. Security plan S6.1 + Galion OS D-A6 (slot 234), 2026-10-22 00:20 CEST (+02:00).
Tree measured: integration-5 `0d68f46`. **No rule was changed.** Inventory with file:line is in
the appendix; evidence in `docs/real-tests/sec-tooling-licence-memo.md`.

## The question in one line

RULES.md bans GPL/LGPL/AGPL "dependencies or copied code". Agents keep asking whether that
also bans *running* GPL programs (strace, unshare, qemu, git, nft, journalctl) or using
kernel syscall numbers. Today six places say "pending owner ruling" (`third-party/room.md:27`,
`ucpu.md:13`, `guard-rust.md:31`, register O2/D15, admin-cli D-A6, slot 144).

## Why it is safe to say yes (short)

- Running a program as a separate process (exec, pipe, argv) is "two separate programs" in
  the FSF's own GPL FAQ (#MereAggregation): the GPL does not reach our code. We never link,
  vendor or ship these tools: the two shipped packages contain only `omnode`
  (`release/omnode-0.1.0/*.tar.gz`, checked), CI release artefacts contain only our binaries.
- Kernel UAPI is `GPL-2.0 WITH Linux-syscall-note`; the note says programs that use kernel
  services by normal system calls are *not* derived works. We copy numbers, not header code.

## Decisions (yes / no per line)

| # | Item | Today | Recommendation |
|---|---|---|---|
| L1 | **Test/CI-only tools run as programs**: strace (LGPL-2.1+), util-linux `unshare`/`nsenter`/`prlimit` (GPL-2.0+), qemu-user-static (GPL-2.0), binutils `readelf` (GPL-3.0+), bash/coreutils (GPL-3.0+) | used in 6 Rust tests + 7 scripts + 1 CI job | **YES**, with the rule text below |
| L2 | **OS programs our shipped binaries exec at runtime** (new: not in the plan): `journalctl`/`systemctl` (systemd, LGPL-2.1+) by openmind-nerve, galion-guard, galion-health, gaia-live, galion-deploy; `nft` (GPL-2.0) by galion-guard; `git` (GPL-2.0) by history-purge; `curl` (curl licence) by hermes-rs/link-watch | **live today**: nerve on the VPS calls journalctl | **YES**, same reasoning; rule: never bundle the tool, look it up on the host, document it |
| L3 | **Kernel UAPI facts**: syscall numbers, `AUDIT_ARCH_*`, seccomp/landlock constants and `#[repr(C)]` layouts typed in by hand | edge seccomp tables, openmind-sandbox landlock | **YES** as "interface facts"; keep the source comment; no header text copied |
| L4 | **libFuzzer** | **not used anywhere**: all fuzzing (slots 212, 221, ci-fuzz-workflow) is std-only seeded Rust, no cargo-fuzz | **YES if ever needed**, fuzz builds only. Correction to the plan: libFuzzer is now `Apache-2.0 WITH LLVM-exception` (legacy NCSA/MIT dual), not plain NCSA |
| L5 | **CodeQL CLI** | **not used**: only `codeql-action/upload-sarif` (MIT action) uploads the Scorecard SARIF (`scorecard.yml:44`); dependabot branch bumps it to v4 | **NO while the repo is private.** CodeQL terms allow CI analysis only of an OSI-licensed codebase hosted on GitHub.com, or with paid GHAS. The repo answers 404 to anonymous API calls now (private). **YES after the public MIT launch (Oct 31)**, CI only, never self-hosted for others |
| L6 | Rejected tools (confirm) | none used | **Confirm NO**: BFG (GPL-3.0, JVM), git-filter-repo (Python, breaks rule 1; already refused by hades-gate `safety.rs:161`), Semgrep CE (LGPL-2.1) |
| L7 | **Add `Apache-2.0 WITH LLVM-exception` to the allow-list** (new) | already in every shipped musl binary (Rust's self-contained `libunwind.a`) and in rust-lld | **YES**: it is Apache-2.0 plus extra permissions; cargo-deny would otherwise flag it if a crate ever declares it |
| L8 | Static vs dynamic system libraries (new finding) | `dist/bin/*` (5 tracked binaries, e27ced9) are glibc-dynamic: `libc.so.6` (LGPL-2.1+), `libgcc_s.so.1` (GPL-3.0 WITH GCC-exception-3.1), `libsqlite3.so.0`; launch-gates already drops `dist/bin/` from the public tree | Keep them out of releases (ship musl static only, MIT musl). No new rule needed if L7 text says "OS-provided shared libraries are not shipped code" |

## Proposed RULES.md text (not applied)

Replace the first line of "Open-source borrowing" licences with:

> **Allow-list scope.** The licence allow-list (MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception,
> MIT OR Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0, 0BSD, CC0) applies to everything **linked
> into, vendored in, copied into, or shipped with** our binaries and packages. **External
> programs** that tests, CI or our binaries *run as a separate process* (for example strace,
> util-linux, qemu-user, git, nft, journalctl, readelf) may be under any OSI licence, provided
> that they are (1) installed from the OS or a checksum-pinned upstream release, never vendored,
> committed or bundled into a release; (2) called by exec/pipe/argv only, never via FFI or
> linking; (3) recorded in `docs/third-party/<slug>.md` with name, version, licence and the
> file that runs them. Kernel UAPI numbers and struct layouts (`GPL-2.0 WITH
> Linux-syscall-note`) are interface facts and may be typed in with a source comment.
> Tools with non-OSI terms (for example the CodeQL CLI) need the owner's approval per tool.

`deny.toml` stays unchanged except adding `"Apache-2.0 WITH LLVM-exception"` if L7 is YES
(checked: cargo-deny 0.20.2 accepts the entry, `licenses ok` with and without it; 3 crates already
declare it as an OR choice: wasi 0.11.1, wasip2 1.0.4, wit-bindgen 0.57.1, passing via MIT today).

## What changes for agents if all are YES

Six "pending owner ruling" notes can be closed; nothing to remove from the tree; one CI
step (CodeQL analysis) becomes possible after launch. Copying qemu-aarch64-static to the
VPS temporarily for a test (slot 144) is internal use, not distribution, and is covered by L1.

## Appendix: inventory on integration-5 0d68f46 (+ history-purge on swarm/sec-runbooks)

Licences confirmed from `/usr/share/doc/<pkg>/copyright` on the VPS (Ubuntu) and box (Debian 13).

**strace** (LGPL-2.1+; box 6.13+ds-1, VPS 6.8-0ubuntu2): `crates/room/tests/no_bind.rs:66,95`;
`scripts/real/room.sh:85,87`; `scripts/real/edge-boot.sh:125,126`.

**util-linux** (GPL-2.0+; box 2.41.5, VPS 2.39.3): `unshare` in `crates/room/tests/real_room.rs:69`,
`crates/room/tests/no_bind.rs:53,54`, `crates/galion-backup/tests/real_parity.rs:113`,
`crates/galion-guard/tests/real_netns.rs:88,100`, `scripts/real/room.sh:31,32,72,85,87`,
`scripts/real/edge-boot.sh:127,254,258`; `nsenter` in `crates/room/tests/real_room.rs:97`,
`scripts/real/room.sh:78,80`. (`crates/openmind-sandbox/tests/preset.rs:11` is a banned-word
string, not a use.)

**qemu-user-static** (GPL-2.0; box 1:10.0.13, not on VPS): `crates/tuner/tests/real_ucpu.rs:80`;
`scripts/real/ucpu.sh:40,60`; `scripts/real/edge-boot.sh:275,276`;
`.github/workflows/self-eval.yml:151` (apt install in CI). Also slot 228 `galion-release`
rehearsal (branch ci-artefact-rehearsal, not on integration-5).

**git** (GPL-2.0; box 2.47.3, VPS 2.43.0): tests `crates/galion-deploy/tests/real_rehearsal.rs:461,756`,
`real_build.rs:133,140`, `real_backup.rs:55`, `crates/galion-common/tests/realsupport/mod.rs:112,118,132`,
`crates/galion-backup/tests/real_parity.rs:48`, `crates/bench-banking/tests/real_banking.rs:152`,
`crates/hermes-tools/tests/sandboxed.rs:33`, `real_ticket.rs:38`, `crates/hermes-bench/tests/real_hermes_bench.rs:153`;
scripts `scripts/ucpu-gate.sh:80,89`, `scripts/real/launch-gates.sh:54`. **Runtime:** history-purge
`crates/history-purge/src/repo.rs:19,213,229` (git, fast-export, fast-import; swarm/sec-runbooks).

**binutils readelf** (GPL-3.0+): `scripts/ucpu-gate.sh:199-203`, `scripts/size-budget.sh:125,140,151`,
`scripts/real/ucpu.sh:114,115`, `scripts/real/{tuner-cutover,room,one-probe,edge-boot}.sh`.

**Runtime OS programs (L2)**: `journalctl` `crates/openmind-nerve/src/probe.rs:419`,
`crates/galion-guard/src/source.rs:278`, `crates/galion-deploy/src/backend.rs:103`;
`systemctl` `crates/galion-health/src/main.rs:110`, `crates/gaia-live/src/lib.rs:350`,
`crates/galion-deploy/src/backend.rs:74-114`; `nft` `crates/galion-guard/src/nft.rs:162`
(bin from `deploy/staged/galion-guard.toml:15`, staged in dry-run).

**Kernel UAPI facts (L3)**: `edge/openmind-node/src/seccomp.rs:42-54` (seccomp_data offsets,
`AUDIT_ARCH_*`), `:69,100,131` (per-arch deny tables), `:171` (unified 425+ numbers);
`crates/openmind-sandbox/src/lib.rs:249-252,722` (`SYS_*` 436-446), `:256` (repr(C) landlock
attr), `:385-395` (landlock rights). Sources already noted in `docs/third-party/ucpu.md:19-20`
and `edge-boot.md:33`.

**libFuzzer / cargo-fuzz**: 0 uses (`rg` over the tree; docs/openssf.md:83 says "no cargo-fuzz target").
**CodeQL**: 0 CLI uses; `.github/workflows/scorecard.yml:44` upload-sarif only.
