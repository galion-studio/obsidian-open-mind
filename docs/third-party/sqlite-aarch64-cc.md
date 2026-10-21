<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: sqlite-aarch64-cc (slot 156)

No new crates. `xtask` has an empty `[dependencies]` (std only, guarded by
`cargo_alias_and_member` in `crates/galion-realtest/tests/real_xtask_cross.rs`);
Cargo.lock only gains the `xtask` package itself.

| item | version | licence | source | use | why |
|---|---|---|---|---|---|
| galion-ccwrap logic (argument rewrite, triple table) | swarm/realscript-port-3 03967fc | MIT (this project) | `crates/galion-realtest/src/bin/galion-ccwrap.rs` on that branch | cc/ar wrapper mode of xtask | same project's own code; reused, not re-invented. Extended: `$TARGET` wins over cc-rs's clang `--target=`, clang triple spellings |
| zig (tool, not linked) | 0.15.2 (pinned in scripts/install-zig.sh) | MIT | https://ziglang.org | C cross compiler for bundled SQLite | already the project's choice (slot 145, ucpu-gate.sh, CI); nothing new |

Licence check: no new transitive crates (`cargo metadata` / Cargo.lock diff
adds only `xtask`).
