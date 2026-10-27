<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# t315u — overnight remaining Python/Node gaps (Rust stack)

Written **2026-10-27 20:03 CET** (Europe/Zagreb). REQUEUE after request-limit kill.
Overnight goal: Rust stack by ~08:00 CET 2026-10-28. desk-hermes cutover already DONE
(`LIVE_PY=NO`, see `t314u-desk-hermes-cutover.md` / `t315u-desk-hermes-verify.md`).
Titanaxe panel screenshot (t317u ~19:56 CET): VPS Max 3XL Running, CPU 0%, RAM 119 MB / 16 GB,
SSD ~32 GB / 100 GB, Ubuntu 24.04, IP 51.38.137.193, expires 2026-10-29 23:46 CET; root password
hidden in UI (not copied). Guest loadavg remains host-noise (~25–28); live Galion services idle.

Constraints: no force-push; no secrets; push via VPS; no Titanaxe panel actions.

## Inventory — remaining live Python/Node on VPS

### Live Galion process tree (2026-10-27 20:03 CET)

**No Galion Python or Node process is running.** `ps` matches for `python3 /opt`, node under
`/opt/galion`, and tsx: empty.

| Service | Port / role | Binary | Lang |
|---|---|---|---|
| console | :8080 | `galion-console-serve-2d846fb8` | Rust |
| console-rust-primary | :18080 | same | Rust |
| galion-desk | :8646 | `galion-desk` + child `desk-hermes` | Rust |
| hermes-rs | :8642 | `hermes-gateway-b7fbbc78` | Rust |
| galion-nerve | :8670 | `openmind-nerve-9d1a4f3` | Rust |
| gaia-cortex | :8674 | `gaia-cortex-73583ab-llm` | Rust |
| galion-one | :8091 / :8002 | `galion-one` | Rust |
| galion-health | :8660 | `galion-health` | Rust |
| openmind-memory | :8420 | `openmind-memory` | Rust |
| openmind-queue | :19120 | `openmind-queue` | Rust |
| link-watch-rs | state files | `link-watch` | Rust |
| tuner / tanga-proto | :8650 / :8671 | static Rust servers | Rust |
| galion-backup.timer | oneshot | `galion-backup-98466ab` | Rust |
| guardian-tick.timer | oneshot | `guardian-tick` | Rust |

System Python only (OS, not Galion app): `fail2ban-server`, `networkd-dispatcher`.

### On-disk leftovers (not live)

| Path | Status |
|---|---|
| `/opt/galion/bin/desk-hermes.py` | rollback keep; not exec'd (`LIVE_PY=NO`) |
| `/opt/galion/bin/pw.py` | manual helper; Rust `pw-check` now installed beside it |
| `/opt/galion/bin/replicate-picture.py` | manual helper; Rust binary now installed beside it |
| `link-watch.service` (was Python) | **masked** (unit archived) |
| `memory-gateway.service` (was Node/tsx) | **masked** (unit archived); replaced by `openmind-memory` |
| `hermes.service` | already masked; `hermes-rs` live |
| `/opt/galion/tooling/node*` | present on disk; no live consumer |

### Known non-Python gaps (not closed this turn)

| Gap | Why left |
|---|---|
| Console `--local-api` stub vs full former Node srvx board API | Already on Rust :8080/:18080; full TanStack board parity still open (needs `--upstream` / control board work) |
| `galion-backup` intermittent `size_kb=1` log metric | Archive content OK (~18 MB .age); metric uses `st_blocks` — code fix deferred |
| Hermes coding runtime OFF | No Landlock on VPS; needs owner allow + worktree |
| Archive `.py` after 7 days | Needs owner approval (B64 archive policy) |

## Gaps closed this turn (top 2)

### 1) Build/deploy — last `/opt/galion/bin` Python helpers → Rust

From branch `swarm/opt-bin-py-port` @ `60b692b` (plan steps 1–4; no paid Replicate call):

- Built on box: `cargo-slot build --release --target x86_64-unknown-linux-musl -p pw-check -p replicate-picture`
- Installed:
  - `/opt/galion/rust/bin/pw-check` sha256 `3ff1a2dea218c433eff85e6fd1c675c12c611ee9256fbe988adb4381eda58b66`
  - `/opt/galion/rust/bin/replicate-picture` sha256 `38238887e06bf0325a7025c8131d3a2e5a9ed900f5a15062b9942c3a7fb2b8ed`
- Symlinks: `/opt/galion/bin/pw` → rust pw-check; `/opt/galion/bin/replicate-picture` → rust binary
- `.py` files **kept** for rollback
- Smoke: both `--help` OK

Also hardened leftovers so they cannot accidentally start:

- Archived unit files to `/opt/galion/archive/py-units-20261027/`
- `systemctl mask link-watch.service memory-gateway.service`
- Replacements still **active**: `link-watch-rs`, `openmind-memory`

### 2) Merge green — PR #13

- PR: https://github.com/galion-studio/obsidian-open-mind/pull/13
- Head `48744d5002d513a88ac8cfb8f58a76e4e15aaf4a` → base `swarm/main-staging`
- Checks: **21** completed — **20 success**, **1 skipped** (miri full); `mergeable_state=clean`
- Merged (merge commit, no force-push): `38a07700018906fc21de7ebce1500bdcc4762671`
- Unblocks PR #12 publish gate CI fixes into `swarm/main-staging`

## Smoke after changes

| Probe | Result |
|---|---|
| `GET :8646/desk` | 200 |
| `GET :8642/health` | 200 (`implementation=hermes-rs`) |
| Galion py/node processes | none |
| Masked units | link-watch, memory-gateway, hermes |

## Rust-stack overnight status

**Live Galion stack is Rust.** Remaining `.py` on disk are rollback/manual only; dead Python/Node
units masked. Console board API parity and backup `size_kb` metric remain follow-ups, not live
language cutovers.
