<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Cross C compiler for aarch64 / armv7 musl (slot 338, 2026-10-28 CET +01:00)

Closes D-318-3 / D-325-3: the box had no aarch64/armv7 C cross compiler on
PATH, so every crate with bundled SQLite (`libsqlite3-sys` 0.30.1: hermes-gateway,
hermes-rs, openmind-memory, desk-hermes, one-defence) failed for the arm musl
targets with `failed to find tool "aarch64-linux-musl-gcc"`, and the aarch64
solo-vs-full byte proof was skipped.

## What is on the box now (all under /workspace/swarm, nothing system-wide)

| path | what |
|---|---|
| `tools/zig-0.15.2/` | zig 0.15.2 (MIT), `zig-x86_64-linux-0.15.2.tar.xz` |
| `tools/zig-dl/` | the tarball + `.minisig` kept for re-verification |
| `bin/zig` | pinned zig, cache in `cache/zig` (`ZIG_GLOBAL_CACHE_DIR`) |
| `bin/aarch64-linux-musl-gcc`, `-g++`, `-ar` | `zig cc/c++ -target aarch64-linux-musl`, `zig ar` |
| `bin/arm-linux-musleabihf-gcc`, `-g++`, `-ar` | `zig cc/c++ -target arm-linux-musleabihf -mcpu=generic+v7a+vfp3d16` (= Rust armv7 musleabihf baseline: ARMv7-A, VFPv3-D16, hard float) |
| `tools/qemu/qemu-aarch64`, `qemu-arm` + `bin/qemu-aarch64-static`, `bin/qemu-arm-static` | static qemu-user 10.0.13 (Debian trixie `qemu-user`), test runner only, never shipped |

The wrappers carry the names cc-rs looks for by default, so with the swarm rule
`export PATH=/workspace/swarm/bin:$PATH` **no `CC_*` / `AR_*` env is needed**:

```sh
export PATH=/workspace/swarm/bin:$PATH
cargo build --release --locked --target aarch64-unknown-linux-musl -p hermes-gateway
cargo build --release --locked --target armv7-unknown-linux-musleabihf -p hermes-gateway
```

The linker stays rust-lld from `.cargo/config.toml`; zig only compiles C.
Each wrapper drops the rust-style `--target=<triple>` that cc-rs adds for a
clang-family compiler (zig gets its own `-target`). Wrapper body:

```sh
#!/bin/sh
ZIG=/workspace/swarm/tools/zig-0.15.2/zig
: "${ZIG_GLOBAL_CACHE_DIR:=/workspace/swarm/cache/zig}"; export ZIG_GLOBAL_CACHE_DIR
for a do
  shift
  case "$a" in --target=*|-target) continue ;; esac
  set -- "$@" "$a"
done
exec "$ZIG" cc -target aarch64-linux-musl "$@"
```

Elsewhere (CI, another machine) `scripts/install-zig.sh DIR` installs the same
pinned zig and `docs/toolchain-aarch64.md` shows the `CC_<triple>` form.

## Verification of the downloads

- zig: sha256 `02aa270f183da276e5b5920b1dac44a63f1a49e55050ebde3aecc9eb82f93239`
  = ziglang.org `download/index.json` and = `ZIG_SHA256` in `scripts/install-zig.sh`;
  minisign signature verified against the ziglang.org key
  `RWSGOq2NVecA2UPNdBUZykf1CCb147pkmdtYxgb3Ti+JO/wCYvhbAb/U` with a 15-line Rust
  checker on `minisign-verify` 0.2 (MIT) built in a scratch dir (not in this repo);
  a one-byte-tampered copy fails the check.
- qemu-user: `qemu-user_10.0.13+ds-0+deb13u1_amd64.deb` sha256
  `ca6ede739327a20ae5a3498230c8a3a9a072c586e131bc6bcc1201eb1725e336` = trixie
  `main/binary-amd64/Packages.xz`, whose sha256 `7778d3e3…2498` = the trixie
  `InRelease` entry (all over HTTPS from deb.debian.org). The InRelease OpenPGP
  signature was NOT checked (no gpgv on the box). Only the two qemu binaries were
  extracted with `dpkg-deb -x`; nothing installed.

## Regression test

`crates/galion-realtest/tests/real_cross_cc.rs` (GALION_REAL=1): builds
hermes-gateway (sqlite + coding) for both arm musl targets, checks static ELF +
SQLite linked, then runs each under qemu with `--mock` on 127.0.0.1:22080/22081,
writes a chat session, restarts on the same state.db and reads it back. Results:
`docs/real-tests/cross-cc-restore.md`.

## Re-install (if the box is rebuilt)

```sh
mkdir -p /workspace/swarm/tools && scripts/install-zig.sh /workspace/swarm/tools/zig-0.15.2
# then recreate the bin/ wrappers above (same body, per target)
```
