<!-- SPDX-License-Identifier: MIT -->
# openmind-node (`omnode`)

Phone/LAN-side node of **Obsidian Open Mind, made by Galion**. Rust only, MIT.
Runs on a device that is actually on the owner's Wi-Fi (Android phone via
Termux, Linux laptop, Raspberry Pi), never on the VPS.

```
omnode identity init [--no-bind] | show | bind [--force] | network
omnode discover [--seconds N] [--dry-run] [--mdns-query] [--no-msearch] [--probe] [--probe-subnet]
omnode announce [--seconds N] [--dry-run]
omnode pair [URL] [--no-qr] [--dry-run]     # prints payload + QR; --dry-run = HTTP plan only
omnode pair verify FILE --url URL [--now UNIX] # fail-closed payload check (what Hermes runs)
omnode boot [--state DIR] [--listen IP:PORT] [--code-ttl S] [--code-file F] [--require-seccomp]
                                            # edge boot: first boot, pairing code, loopback HTTP
omnode sandbox run [limits...] -- CMD...
omnode sandbox check
omnode sandbox selftest                     # seccomp deny-list really enforced?
omnode doctor                               # what this device can do
omnode version
```

Config: `~/.config/openmind/node.toml` (see `omnode.example.toml`).
Design: `docs/phone-instance.md`. Sandbox merge plan (do not merge):
`docs/sandbox-merge-proposal.md`.

## Install on a local device

```
# Linux laptop / Pi (from a checkout, or with OMNODE_RELEASE_URL=...):
sh install/linux.sh

# Android phone (Termux):
sh install/termux.sh
```

Release tarballs (static musl): `/opt/galion/openmind/release/omnode-0.1.0/`.

## Build

Standalone crate with its own empty `[workspace]` under `edge/` (not
`crates/`, so it does not join or edit the OpenMind workspace).

```
nice -n 19 cargo build --release -j 2
# Cross for phone/Pi:
rustup target add aarch64-unknown-linux-musl
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
  nice -n 19 cargo build --release -j 2 --target aarch64-unknown-linux-musl
```

Boot path, pairing and seccomp notes: `docs/edge-boot.md` (repo root).
