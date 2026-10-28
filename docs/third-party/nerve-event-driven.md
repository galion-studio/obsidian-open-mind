<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: nerve-event-driven

No new dependencies and no copied code. `Cargo.lock` is unchanged.

| need | how | why not a crate |
|---|---|---|
| inotify, pidfd_open, poll, read | `extern "C"` declarations of the C library symbols std already links (glibc / musl / bionic), in `crates/openmind-nerve/src/evt/sys.rs` (the only `unsafe`, documented per block; fds owned by `OwnedFd`) | `libc` / `inotify` / `nix` would add 1-3 crates for four calls; the swarm rules ask for minimal dependencies |
| systemd events | `journalctl -f -o json` child with server-side matches | `zbus` pulls ~40 crates (async runtime); the journal carries the same unit transitions |
| X.509 notAfter | ~60-line DER walker + base64 in `evt/threshold.rs` | `x509-parser` / `openssl` (openssl banned) for one date |
| LLM call (disabled slot) | `curl` subprocess, key in a 0600 header file removed right after, `--max-redirs 0` | no TLS stack in the nerve (std + serde only) |

Used system tools (already on the VPS, read-only): `journalctl`, `systemctl list-units`, `ss -tlnpH`, `ps`, `df -P`, `docker ps` (only if present; absent on the VPS), `curl` (only in the disabled `openai` slot), `systemctl restart --no-block` (only `POST /actions` with all three live switches on; off by default).

`#![forbid(unsafe_code)]` became `#![deny(unsafe_code)]` in `lib.rs` so `evt::sys` can opt in with an inner `allow` (precedent: `galion-voice-cpu`). `main.rs` keeps `forbid`.
