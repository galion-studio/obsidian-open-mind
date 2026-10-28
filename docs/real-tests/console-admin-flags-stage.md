<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Console admin-store flags: staged drop-ins (wave-4 slot 313, D-303-1 prep)

Branch `swarm/console-admin-flags-stage`, based on `swarm/console-board-api-parity`
`9c0f9d1a` (slot 303). Nothing here is installed on the VPS. Live units were
only read.

## The problem today (VPS, read-only, 2026-10-27 22:00 +01:00)

Two live Rust consoles run the same argv from `galion-console-serve-2d846fb8`.
That build has no board routes and no `--admin-store`:

| Unit | Bind | Drop-ins |
|---|---|---|
| `console.service` (FALLBACK, cloudflared `console.galion.app` target) | 127.0.0.1:8080 | none |
| `console-rust-primary.service` (PRIMARY) | 127.0.0.1:18080 | none |

| Probe | :8080 | :18080 |
|---|---|---|
| `/api/v1/admin` (loopback and remote Host) | 404 | 404 |
| `/api/v1/services`, `/api/v1/catalog` | 404 | 404 |
| `/api/v1/health` | 200 | 200 |
| `/console` with Host `console.galion.app` | **503** | **503** |

Nerve `/state` 200 (mode live) still shows `console` as **healthy**, because it
probes `/` (302) and never sees the remote 503. That is a nerve blind spot
(see slot 315 nerve-console-deep-health).

Store files are present: `/root/galion-secrets/admin-device.json` (root 0600,
616 B) and `/workspace/galion-light/secrets/persist/admin-device.json`
(root 0600, 533 B). Both directories are root 0700. Only `ls -l` metadata was
read, never the contents. The two files have different sizes, so the mirror
is already out of sync with the store (D-313-3).

## What is staged

- `deploy/console-rust-serve/live/{console,console-rust-primary}.service`:
  as-built mirrors. Everything below `# ---- live body ----` is byte-identical
  to `systemctl cat` (sha256 `cafa5246…` / `808e3f56…`).
- `deploy/console-rust-serve/console.service.d/60-admin-store.conf` and
  `…/console-rust-primary.service.d/60-admin-store.conf`. Each one:
  - resets `ExecStart=`, then sets the same live argv on
    `/opt/galion/rust/bin/galion-console-serve-9c0f9d1a`, with
    `--admin-store /root/galion-secrets/admin-device.json --admin-mirror
    /workspace/galion-light/secrets/persist/admin-device.json` placed before
    `--quiet`. The `2d846fb8` build would exit with `unknown argument
    "--admin-store"`, because its config.rs has no such flag (checked with
    `git show 2d846fb8:…/config.rs`, 0 hits).
  - sets `ReadWritePaths=/root/galion-secrets
    /workspace/galion-light/secrets/persist`. The live unit has
    `ProtectSystem=strict` and `ProtectHome=read-only`. The atomic write
    (temp file plus rename) needs the directory to be writable, not just the
    file.
  - changes nothing else. Hardening, MemoryMax, Nice and SyslogIdentifier are
    all inherited, and there is no User or DynamicUser (root is needed for the
    0700 root directories).

## Tests

| Test | When (+01:00) | Result |
|---|---|---|
| `cargo test -p console-static --test deploy_units` (offline, always on) | 2026-10-27 21:58 | **10/10 pass** |
| `GALION_REAL=1 cargo test --release -p console-static --test real_admin_flags_stage` (box, 127.0.0.1:21830-21833) | 2026-10-27 21:59 | **pass, A to D in 228 ms** |
| `GALION_REAL=1 CONSOLE_UNIT_LIVE=1 … --test real_admin_flags_live` (VPS read-only over ssh) | 2026-10-27 22:00 | **pass**, mirror == live for both units, no drop-ins, codes in the table above |
| Workspace: `cargo fmt --all --check`; `clippy --workspace --all-targets -D warnings` (excluding openmind-sandbox and cgroup-detect); `cargo test --workspace` (excluding openmind-sandbox, cgroup-detect and hermes-tools); `scripts/no-python.sh` | 2026-10-27 22:01 | pass. 142 suites: 694 passed, 0 failed, 7 ignored |
| x86_64 musl release `galion-console-serve` (`--locked`) | 2026-10-27 22:00 | 1,024,896 B, sha256 `1c8318746a5ddb41…`, **identical** to the slot-303 binary run on the VPS. The `-9c0f9d1a` artifact is reproducible. |
| aarch64 musl release (rust-lld) | 2026-10-27 22:00 | 866,568 B, sha256 `9837eb9f8e9993df…` |
| `cargo check` armv7-unknown-linux-musleabihf, aarch64-linux-android | 2026-10-27 22:00 | ok |

`deploy_units` checks:
- the live mirrors run 2d846fb8 with no store, and this build also parses that argv;
- the drop-in argv equals the live argv plus the admin pair, and it parses with
  `Config::from_args` (bind, pulse, desk, self-upstream, no proxy);
- RW covers both store directories and nothing more, while the binary and dist
  stay RO. A base-only negative check confirms the base unit has no RW;
- all 15 hardening keys are unchanged, `ProtectHome=read-only`, and there is
  no User or DynamicUser;
- the drop-in only touches ExecStart and ReadWritePaths;
- rollback means removing the file;
- a negative control without the `ExecStart=` reset gives 2 ExecStart lines
  (Type=simple then refuses to load);
- the two drop-ins differ only in bind and unit name;
- the binary refuses a mirror without a store, mirror == store, and a relative
  store path;
- SPDX headers are present.

`real_admin_flags_stage` runs the argv parsed from the staged file, with only
paths and ports remapped into a temp root shaped like the VPS
(`<dist>/static`, store directories 0700):

| Phase | Setup | Observed |
|---|---|---|
| A | drop-in as staged (dirs writable) | 10 board routes 200 on loopback. Remote `/api/v1/admin` 200 `{"ok":true,"paired":true,"you":false}`. Node-format store cookie (already-paired phone) `you:true`, `/services` 200. Remote `/console` 200 pair page (not 503). `/api/v1/services` 401 without cookie, 200 with it. JSON pair 200. Store == mirror, both 0600, hashes only, and the old phone still works. |
| B | same argv, dirs 0500 (= no ReadWritePaths) | board 200; paired cookie still `you:true`; new pair **503 `store not writable`**, no cookie, store bytes unchanged, no temp files left |
| C | live argv (no admin flags) on the new build | board 200; remote `/api/v1/admin` **503 `admin store not configured`**; remote `/console` 503. Swapping only the binary fixes the board but not phones. |
| D | only the mirror dir 0500 | pair 200 from the store; mirror stays stale (logged `admin mirror write failed: permission denied`) |

## Findings

- **F-313-1 (security, existing 9c0f9d1a code)**: when the store is not
  writable, `AdminStore::pair` ignores the failed write of the fail counter.
  Phase B measured 6 wrong codes → `[401 ×6]`, never 429, and the right code →
  **503**. With a read-only store there is no lockout, and 401 vs 503 tells an
  attacker when a code is right. The staged ReadWritePaths prevents this on
  the VPS, but it comes back if the store ever turns read-only (ZFS RO, a
  full disk, the drop-in missing). Proposed fix (not made here, it would
  change the 9c0f9d1a binary): if the fail-counter write fails, return 503
  `store not writable` for wrong codes too, so wrong and right look the same
  and the store fails closed.
- **F-313-2**: nerve marks `console` healthy while every remote page is 503.

## Owner decisions

- **D-303-1 / D-313-1 deploy (not done)**. Steps, once approved:
  1. copy the x86_64 musl build (sha256 `1c8318746a5ddb41…`) to
     `/opt/galion/rust/bin/galion-console-serve-9c0f9d1a` (+ `.sha256`);
  2. `install -D -m 0644` both `60-admin-store.conf` files into
     `/etc/systemd/system/<unit>.service.d/`;
  3. `systemd-analyze verify /etc/systemd/system/console.service` (this
     could not run on the box because it needs root for /run/systemd);
  4. `systemctl daemon-reload`, then restart `console-rust-primary` first and
     check, then `console`;
  5. check remote `/api/v1/admin` 200 and `/console` 200 (pair page).
  Rollback: remove the two drop-ins, daemon-reload, restart. The binary can
  stay.
- **D-313-2 RW scope**: `ReadWritePaths=/root/galion-secrets` makes the whole
  secrets directory writable for the console process. A narrower option is
  to move the store to a dedicated `/root/galion-secrets/console/` (or
  `/var/lib/galion-console/`), then point both flags and RW there. That
  requires a one-time store move by the owner.
- **D-313-3 mirror drift**: the live store and mirror are already different
  sizes (616 vs 533 B). The first pairing after deploy rewrites both. The
  owner should decide which copy is the source before deploy. If it is the
  store, nothing needs doing.
- **D-313-4**: two processes (8080 and 18080) share one store. Each request
  re-reads the file and writes are atomic, but the in-process lock does not
  span processes, so two pairings at the same instant are last-writer-wins
  (a lost token hash means one re-pair). Option: give admin flags only to
  the cloudflared target (:8080).
- **F-313-1 fix**: approve a follow-up slot that changes `admin.rs` and
  rebuilds before or with the deploy.

## Reproduce

```
export PATH=/workspace/swarm/bin:$PATH
cargo-slot test -j 2 -p console-static --test deploy_units
GALION_REAL=1 cargo-slot test -j 2 --release -p console-static \
  --test real_admin_flags_stage -- --nocapture
GALION_REAL=1 CONSOLE_UNIT_LIVE=1 cargo-slot test -j 2 -p console-static \
  --test real_admin_flags_live -- --nocapture   # VPS read-only
```
