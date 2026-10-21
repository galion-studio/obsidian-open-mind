<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: sec-deny-wildcards (slot 201, Security S2.5)

Base integration-4 42de672, branch swarm/sec-deny-wildcards. Box only; the
VPS was only read (one GET of the nerve). Times Europe/Zagreb (+02:00).

## What changed
- `publish = false` in `[package]` of all 63 `crates/*` members plus
  `edge/openmind-node` and `sim/deploy-sim` (65 manifests;
  `cargo metadata` reports `publish: []` for every package).
- `deny.toml`: `[bans] wildcards = "deny"` (was `"warn"`);
  `allow-wildcard-paths = true` kept (it was already in the base).
- repo-check: new `publish` check (error) — every `crates/*`, `edge/*`,
  `sim/*` Cargo.toml `[package]` must say `publish = false`, so a new crate
  cannot silently bring the 45 wildcard findings back. Unit tests in
  `crates/repo-check/tests/checks.rs`.
- Real test `crates/repo-check/tests/real_publish.rs` (`GALION_REAL=1`).
- Cargo.lock unchanged; no new dependencies.

## cargo-deny 0.20.2 (2026-10-21 18:48-18:50 +02:00)
| tree | policy | licenses | bans | sources |
|---|---|---|---|---|
| base 42de672 | old (`wildcards = "warn"`) | ok | ok, 45 `warning[wildcard]` | ok |
| base 42de672 | new deny.toml | ok | **FAILED, 45 `error[wildcard]`** | ok |
| this branch | new deny.toml (workspace) | ok | ok, 0 wildcard | ok |
| this branch | edge/openmind-node | ok | ok | ok |
| this branch | sim/deploy-sim | ok | ok | ok |

Remaining bans output is `warning[duplicate]` only (syn x2, hashbrown x3),
unchanged, `multiple-versions = "warn"`.

Advisories (RustSec DB fetched, 18:49 +02:00): `advisories FAILED` on both
the base and this branch with the same 4 known IDs, unchanged by this slot:
RUSTSEC-2024-0436 (paste unmaintained, via candle/gemm), RUSTSEC-2026-0367,
-0368, -0369 (zip extract: SUID/SGID bits, permissions through symlinks,
symlink targets outside the destination). These are the known zip/paste
items handled by candle-zip-fix / deny-paste-replace (integration-5), not
by integration-4.

## Real test (GALION_REAL=1, 18:50:34 +02:00)
`GALION_REAL=1 cargo-slot test -p repo-check -j 2 --test real_publish -- --nocapture --test-threads 1`
```
real_cargo_deny_wildcards:
  Cargo.toml: ok=true
  edge/openmind-node/Cargo.toml: ok=true
  sim/deploy-sim/Cargo.toml: ok=true bans ok
  mini public: ok=false     (2-crate path-dep workspace without publish=false -> error[wildcard])
  mini private: ok=true     (same with publish=false)
real_repo_check_publish:
  live tree: exit 0  repo-check 2026-10-21T18:50:36+02:00: 1384 files, 0 error(s), 0 warning(s) -> PASS
  copied 65 manifests
  stripped zagreb-time: exit 1
    error publish crates/zagreb-time/Cargo.toml: [package] lacks `publish = false` ...
  repo-check: 65 files, 1 error(s) -> FAIL
test result: ok. 2 passed; 0 failed
```
Without `GALION_REAL=1` both tests print `skip` and pass (2/2).
The aarch64 musl repo-check binary under qemu-aarch64-static on this tree
(18:55:30 +02:00): `1384 files, 0 error(s), 0 warning(s) -> PASS`, rc 0.

## Gates (2026-10-21 18:50-18:55 +02:00)
- `cargo fmt --check` ok; `cargo clippy --workspace --all-targets -- -D warnings` ok.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`:
  1229 passed, 0 failed, 6 ignored (267 suites). Sandbox crates not run
  locally (sandbox_vps_only); this slot changes only their manifests.
- `scripts/no-python.sh`: ok. repo-check on the tree: PASS, 0 errors.
- Release builds: x86_64-unknown-linux-musl workspace ok (2 m 05 s);
  aarch64-unknown-linux-musl workspace ok with
  `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld` and
  `CC_aarch64_unknown_linux_musl` = a temp zig-cc wrapper (libsqlite3-sys
  needs a C compiler; same known exception as slots 122/145; wrapper not
  committed). Without a C compiler it fails in libsqlite3-sys exactly as on
  the base.
- `cargo check --workspace`: aarch64-linux-android ok,
  armv7-unknown-linux-musleabihf ok (zig-cc for libsqlite3-sys again; the
  gnueabihf std is not installed on the box, so musleabihf was used).

## VPS (read-only, 18:55:28 +02:00)
GET 127.0.0.1:8670/state over ssh: nerve cycle 82, mode dry-run, system
degraded 66/D (11 healthy, 1 degrading, 1 failing-soon, 2 absent; edge
tunnel to closed origin ports). Cortex not live (no listener). Working copy
/opt/galion/openmind still 2f5a730. Nothing on the VPS depends on
`publish` or deny.toml, so there is no runtime behaviour to compare.

## Owner decisions
- D-201-1: keep `wildcards = "deny"` + repo-check `publish` as an error
  (recommended), or keep `deny` but make the repo-check check a warning.
- D-201-2: `publish = false` per crate (done) vs. one `[workspace.package]
  publish = false` with `publish.workspace = true` in each crate (repo-check
  accepts both forms; the root `[workspace.package]` has no `publish` key
  today, so `publish.workspace = true` would not parse until it is added).
- Advisories remain red on integration-4 until the zip/paste fixes from
  integration-5 land (no change here).
