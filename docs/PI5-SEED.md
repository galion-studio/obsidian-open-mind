<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Pi 5 seed

Static musl binaries of the release set in `config/release/size-budget.tsv` (every named row except `default`, including `openmind-desktop`) for `aarch64-unknown-linux-musl` and `x86_64-unknown-linux-musl`. The aarch64 set is what you copy onto a Raspberry Pi 5 next to Galion OS. This repository does not build the Galion OS image.

The aarch64 binaries are executed only under qemu-user (`--help` and/or `--version`, whichever that binary already exits 0 on without opening a socket). They have not been run on Raspberry Pi 5 hardware. Nothing here was booted as Galion OS.

## Profile

`[profile.pi-seed]` in the root `Cargo.toml`: `opt-level = 3`, `lto = true`, `codegen-units = 1`, `panic = "abort"`, `strip = true`. It inherits the release profile and overrides the size setting.

`[profile.release]` stays `opt-level = "z"`. That is the 2 MiB budget (`docs/low-end-profile.md`, `scripts/size-budget.sh`). The seed does not call `size-budget.sh`. opt-level 3 binaries can be larger than 2 MiB.

The musl linker is `rust-lld` (`.cargo/config.toml`). The build also exports `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld` and the x86_64 twin. Bundled SQLite is compiled with the zig cc wrappers from `scripts/linux/build-musl.sh`. For this profile only, the script drops the `-fno-inline` size flag on those C objects and keeps `-fno-unwind-tables`. SQL results are unchanged.

## Build

```sh
scripts/linux/pi-seed.sh build aarch64-unknown-linux-musl
scripts/linux/pi-seed.sh verify aarch64-unknown-linux-musl
scripts/linux/pi-seed.sh build x86_64-unknown-linux-musl
scripts/linux/pi-seed.sh verify x86_64-unknown-linux-musl
```

`verify` on aarch64 needs `qemu-aarch64-static` or `qemu-aarch64`. x86_64 probes run on the host. Binaries land in `target/pi-seed-out/<target>/`.

`.github/workflows/pi-seed.yml` builds both targets, runs that smoke, and uploads the workflow artifact `obsidian-pi5-seed-<commit>.tar.gz`. Permissions are `contents: read`. It does not create a GitHub Release.

## Tarball

```
obsidian-pi5-seed-<sha>/
  bin/aarch64-unknown-linux-musl/
  bin/x86_64-unknown-linux-musl/
  LICENSE
  SHA256SUMS
  systemd/user/...    # only when the tree has user units
```

`LICENSE` is the repository MIT file, copied unchanged (`Copyright (c) 3000 Galion / galion-studio`). `SHA256SUMS` covers the other files in the archive. There are no systemd user units in the tree today (no `systemd/user` path and no `WantedBy=default.target`), so the archive has no `systemd/` directory. System units under `deploy/` are not included.

Probes that exit 0 without binding, from the `# probe:` lines in `scripts/linux/pi-seed.sh`:

| binary | flags |
| --- | --- |
| desk-hermes, galion-backup, galion-desk, hermes-gateway, link-watch, openmind-desktop, openmind-nerve, openmind-queue, tuner | `--help` and `--version` |
| gaia-cortex, galion-health, galion-one, openmind-memory | `--version` only (`--help` is not a usage exit; galion-health and gaia-cortex would otherwise start) |
| galion-console-static, galion-console-serve | `--help` only (no `--version`) |

`hermes-gateway` and `galion-backup` grew a single-argument `--help` / `--version` that returns before config, sqlite, or a bind, so the smoke can run them.

## Install

On Raspberry Pi OS Bookworm 64-bit, one command installs the desktop and the services its probes ask. See `docs/PI-BOOKWORM.md`.

```sh
sudo scripts/linux/pi-bookworm-install.sh --seed target/pi-seed-out/aarch64-unknown-linux-musl
```

The `bookworm arm64 emulator` job in `.github/workflows/pi-seed.yml` runs that install twice under qemu-user in a Debian Bookworm arm64 root. That job is not a Raspberry Pi 5 boot and not Raspberry Pi OS.
