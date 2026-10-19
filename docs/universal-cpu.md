<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Universal CPU

- DESK-ID: openmind.UCPU-1
- Written: 2026-10-19T17:41:39+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Script: scripts/target-check.sh

## Scope

This page records a name check only. The host `rustc` is asked whether it already knows four target names. A full cross build is not done and is not claimed.

No binary was produced for any target. No size or RSS was measured. `cross.sh` was not generalized. Nothing here is a merge gate pass.

## Targets

| target | later role (not done here) | this ticket |
| --- | --- | --- |
| x86_64-unknown-linux-musl | musl Linux x86_64 | name is KNOWN or MISSING |
| aarch64-unknown-linux-musl | musl Linux aarch64 | name is KNOWN or MISSING |
| aarch64-linux-android | Android aarch64 | name is KNOWN or MISSING |
| armv7-unknown-linux-gnueabihf | 32-bit ARM gnueabihf | name is KNOWN or MISSING |

KNOWN means `rustc --print cfg --target <name>` exits 0. That loads a built-in target specification. It does not mean the standard library for that target is installed, and it does not mean a crate was compiled.

MISSING means rustc does not know that name (or rustc itself is absent). The script still exits 0.

## Script

`scripts/target-check.sh`

- Puts `/root/.cargo/bin` on `PATH` so the existing host rustc proxy is visible. That does not install a toolchain.
- Runs `rustc -vV` and prints it.
- For each name above, runs `rustc --print cfg --target <name>` and prints `KNOWN` or `MISSING`. Cfg text is discarded.
- Exits 0 even if a target is MISSING, and even if `rustc` is missing.
- Does not run `cargo` or `cargo build`.
- Does not run `rustup target add`.
- Does not download.
- Does not write build artifacts.

## Not claimed

A full cross build is not done and is not claimed.

- No x86_64-unknown-linux-musl build.
- No aarch64-unknown-linux-musl build.
- No aarch64-linux-android build.
- No armv7-unknown-linux-gnueabihf build.
- No per-target size, RSS, or link result.
