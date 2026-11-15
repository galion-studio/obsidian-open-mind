<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Swarm 9 core / linux

- Lane: linux
- Swarm: core
- Written: 2026-11-15T15:34:08+01:00 (Europe/Zagreb)
- Base: `main` `de7480e71ce98191d2ee11f018fa15be6c5a1707`
- Toolchain for the builds: rustc 1.100.0 (0ef1bc386 2026-11-09), zig 0.15.2 (checksum from `scripts/install-zig.sh` matched)

## Plan PR

Searched open pull requests for "Swarm 9" in `galion-studio` at 2026-11-15T15:22+01:00 and again before the branch was cut. No draft titled "Swarm 9 core plan" was present. This lane followed the assignment in the launch message.

## What was checked

`cargo` 1.83.0 cannot fetch the lockfile. `cargo fetch --locked` stopped on `zip` 9.0.5: `feature edition2024 is required`. The builds below used stable 1.100.0.

After `cargo fetch --locked --target x86_64-unknown-linux-musl` (serial backend from `.cargo/config.toml`), `cargo tree --offline` failed for `hermes-gateway` and `hades-gate`:

```
error: failed to download `curve25519-dalek-derive v0.1.1`
Caused by:
  attempting to make an HTTP request, but --offline was specified
```

`openmind-desktop` does not depend on that crate; its offline tree exited 0. `RUSTFLAGS= cargo fetch --locked --target x86_64-unknown-linux-musl` downloaded `curve25519-dalek-derive v0.1.1`. The empty `RUSTFLAGS` was not exported. After that, `scripts/linux/fetch-offline-tree.sh` exited 0 at 2026-11-15T15:29:02+01:00 for desktop, gateway, and hades-gate on both musl targets.

`scripts/linux/build-musl.sh` then built release binaries. `scripts/size-budget.sh --check-dir` at 2026-11-15T15:33:48+01:00, after `openmind-desktop` was added to the budget table:

| Binary | Target | Bytes | Cap | Margin | Stripped | Machine | PT_INTERP |
| --- | --- | ---: | ---: | ---: | --- | --- | --- |
| openmind-desktop | x86_64-unknown-linux-musl | 622824 | 2097152 | 1474328 | yes | Advanced Micro Devices X86-64 | no |
| hermes-gateway | x86_64-unknown-linux-musl | 2087296 | 2097152 | 9856 | yes | Advanced Micro Devices X86-64 | no |
| openmind-desktop | aarch64-unknown-linux-musl | 559120 | 2097152 | 1538032 | yes | AArch64 | no |
| hermes-gateway | aarch64-unknown-linux-musl | 1889528 | 2097152 | 207624 | yes | AArch64 | no |

Both size checks printed `size-budget: ok (2 binaries)`. sha256:

- `e9fee53855fb70c0b4c4be322e9e70b52383a89d537f43b27af0465577a3095b` openmind-desktop x86_64
- `e1792584c1b253030b83ccab9e977eece96c78922dd0d78e3f49a2db7837947e` hermes-gateway x86_64
- `522c6e89680db4ed8bb4f6577634d85eed98a2ea16f04760d617432dd25afa4b` openmind-desktop aarch64
- `943833ee8fcab5ac0e8f8eeeb5c69f436939c1025879f5e692b94cbb0b161c73` hermes-gateway aarch64

`cargo test -p galion-feature-pin --test release_feature_unify --locked` at 2026-11-15T15:33:48+01:00: 5 passed, 0 failed. The log line was `release_feature_unify: 658 solo units checked, 0 mismatches`. `cargo fmt --all -- --check` exited 0. Workspace `cargo test` and workspace clippy were not run locally.

`scripts/linux/verify-units.sh` with systemd 255 (255.4-1ubuntu8.18) exited 0. It scanned 46 deploy unit and drop-in files. No non-comment line binds port 18081 or a non-loopback address. `deploy/console-rust-serve/live/console-rust-primary.service` and `deploy/console-rust-serve/console-rust-primary.service.d/60-admin-store.conf` both contain `--bind 127.0.0.1:18080`. There is no `openmind-desktop` unit. `DEFAULT_BIND` in `crates/openmind-desktop/src/lib.rs` is `127.0.0.1:18181`, and `Service::Console` in `crates/openmind-desktop/src/probe.rs` is 18080. `systemd-analyze verify` on each `.service` reported missing executables (31 lines; `/opt/galion` is not installed here) and four `StartLimitIntervalSec` warnings under `[Service]` in `deploy/boot/rendered/{galion-desk,galion-one,link-watch,tuner}.service`. Those four warnings were already documented in `deploy/proposals/unit-hygiene/README.md`. The script records them and does not fail on them.

## What changed

- `.github/workflows/linux-musl.yml` builds `openmind-desktop` and `hermes-gateway` in release mode for `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`, prints the byte counts, and runs the unit check. No artifact upload.
- `scripts/linux/fetch-offline-tree.sh` is the fetch used by that workflow and by the self-eval test job. `.github/workflows/openmind-desktop.yml` was not edited.
- `config/release/size-budget.tsv` names `openmind-desktop` at the same 2097152 cap as `default`.

## Overlap with #52

Draft [#52](https://github.com/galion-studio/obsidian-open-mind/pull/52) (`cursor/desktop-ci-artifacts-c14b`, head `406dece3067a6fbc1be5e9df46900e090d8b3aba` when read) edits `.github/workflows/openmind-desktop.yml` and packages desktop artifacts. Its Linux gnu job is `x86_64-unknown-linux-gnu`. Its aarch64 job is `aarch64-unknown-linux-musl` for `openmind-desktop` only. This branch does not change that workflow file. Both will build the aarch64 musl desktop if both land. This branch also builds the x86_64 musl desktop and both musl `hermes-gateway` binaries, and it checks the size cap.

## Open questions for Maciej

1. `hermes-gateway` on x86_64 musl is 2087296 bytes, 9856 under the 2 MiB cap. The cap was left at 2097152. A small growth fails the new CI job.
2. There is still no systemd unit for `openmind-desktop`. The listener default stays the binary constant `127.0.0.1:18181`.
3. The four rendered units still put `StartLimitIntervalSec` in `[Service]`, where systemd ignores it. They were not rewritten. `docs/staged-not-live.md` records their 2026-10-19 byte sizes.

## Not run / not done

- Workspace `cargo test` and workspace clippy: not run.
- The binaries were not executed and no port was bound.
- No unit was installed. Nothing was merged or deployed.
