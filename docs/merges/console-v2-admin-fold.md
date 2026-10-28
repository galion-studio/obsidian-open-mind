<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# console-v2-admin-fold (wave-4 slot 331) — 2026-10-28 CET (+01:00)

Staging tip only. NOT the live cutover, NO deploy, NO live unit change, no VPS writes.
Follows the 23:37 +01:00 plan update (D-321-3 / D-323-1): base on console-v2-merge-2
and fold the lockout fix instead of 49b31b3f alone.

## What was folded

| step | commit | source |
|---|---|---|
| base | `cd1204a0` | swarm/console-v2-merge-2 (slot 321: merge-1 + board-api-parity 9c0f9d1a + arena-bank 57851e34) |
| merge (`--no-ff`) | `dfa120c6` | swarm/console-admin-lockout-fix (slot 323; carries console-admin-flags-stage 49b31b3f, slot 313) |

Merge result: 0 conflicts (code and docs). 9c0f9d1a was already in the base, so the merge only
adds 49b31b3f + dfa120c6: `admin.rs` fail-closed lockout (F-313-1 fixed), staged drop-ins
`deploy/console-rust-serve/{console,console-rust-primary}.service.d/60-admin-store.conf`,
as-built live unit mirrors under `deploy/console-rust-serve/live/`, tests `deploy_units`,
`real_admin_flags_stage`, `real_admin_flags_live`, `real_admin_lockout`. No new dependencies
(Cargo.toml / Cargo.lock unchanged vs cd1204a0); third-party doc
`docs/third-party/console-admin-flags-stage.md` (no deps) carried as is.

## Gates (box, 2026-10-28 00:13–00:15 +01:00)

- `cargo fmt --check` ok; `cargo clippy -p console-static -p console-ui --all-targets -- -D warnings` ok.
- `cargo test -p console-static -p console-ui`: 30 suites, 242 passed / 0 failed / 7 ignored
  (merge-2 was 224/0/7; +10 deploy_units, +8 admin unit + gated stubs).
- `scripts/no-python.sh` ok.
- release musl `galion-console-serve`: x86_64 1024896 B (sha256 d166f1ea2721604d…),
  aarch64 (rust-lld) 867096 B (110b96816e9aff6a…); `cargo check` armv7-unknown-linux-musleabihf
  and aarch64-linux-android ok.

Real-test evidence: `docs/real-tests/console-v2-admin-fold.md`.

## Owner notes

- This tip is the candidate for the D-303-1/D-313-1 admin deploy (supersedes 9c0f9d1a and
  49b31b3f for that purpose). Deploying it still needs the drop-ins + ReadWritePaths and a
  separate approved cutover; nothing was installed.
- D-321-1 (arena/bank source in `--dist`) still open; see real-test notes (live :18080
  `/api/arena` and `/api/v1/bank` now 404).
