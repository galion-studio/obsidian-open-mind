# Low-end release profile

- DESK-ID: openmind.B08
- Written: 2026-10-19T16:02:34+02:00 (Europe/Zagreb)
- Repo: /opt/galion/openmind
- Profile source: root `Cargo.toml` `[profile.release]` (unchanged by this ticket)

## Budget change: 2 MiB to 200 MB (2026-10-29 CET +01:00)

Owner decision t416u, 2026-10-29 23:37 CET (+01:00): the per-binary budget is now 200 MB,
`200 * 1024 * 1024 = 209715200` bytes, for every row of `config/release/size-budget.tsv`
(`default` included). Size is no longer a constraint when picking crates.

- Unchanged: the strip check, and "a row above the default needs an owner decision".
- `crates/galion-realtest/tests/size_budget_gate.rs` asserts the 200 MB default; its ELF
  fixtures are sparse files, so a 200 MB fixture costs no disk.
- The release profile below (`opt-level = "z"`, fat LTO, one codegen unit,
  `panic = "abort"`, strip) is unchanged. The 2 MiB text below is the history before this change.

## Chosen profile

The workspace release profile is the low-end size profile. It is already set in the root `Cargo.toml`. This page records the choice. Speed is not the goal: these crates are small I/O helpers and HTTP servers, and each workspace binary must stay at or under 2 MiB.

| key | value | why |
| --- | --- | --- |
| opt-level | `"z"` | Minimize size. `"z"` is chosen over `"3"` and over `"s"`. |
| lto | `true` | Fat LTO across crates. Smaller code, slower link. |
| codegen-units | `1` | One LLVM unit so the linker can shrink the binary. Compile is slower, so builds use `nice -n 19` and `-j 2`. |
| panic | `"abort"` | No unwind tables. A panic ends the process. Smaller binary. |
| strip | `true` | Symbols removed. `file(1)` must report `stripped`, not `not stripped`. |

Budget: 2 MiB means `2 * 1024 * 1024 = 2097152` bytes. `scripts/size-budget.sh` fails when any workspace ELF executable is strictly larger than that, or is not stripped. A binary of exactly 2097152 bytes passes the size check. Since slot 353 (2026-10-28 CET +01:00) the per-binary budgets live in one table, `config/release/size-budget.tsv` (`default` row plus one row per release binary, all 2097152 today); the script, ucpu-gate.sh, self-check.sh, CI and the Rust size tests all read it, and `crates/galion-realtest/tests/size_budget_gate.rs` enforces that no row is above 2 MiB.

## Size check

```sh
export PATH=/root/.cargo/bin:$PATH
# optional: export CARGO_TARGET_DIR=/some/other/dir
scripts/size-budget.sh
```

Default build line, and the only build the script starts:

```sh
nice -n 19 cargo build -j 2 --release --workspace
```

The script does not set or clear `CARGO_TARGET_DIR`. Cargo writes the release where that variable points. The check then reads ELF executables in `$CARGO_TARGET_DIR/release`, or `<repo>/target/release` when the variable is unset. A relative `CARGO_TARGET_DIR` is resolved from the repo root. Libraries (`.rlib`, `.rmeta`, `.a`, `.so`) and dep-info files are ignored.

`--check-dir DIR` and `--skip-build` do not run cargo. They exist so the budget can be re-checked, including against a fixture directory, without a workspace build.

A fresh `cargo build --release --workspace` was not run while this page was written. Other agents were editing crates, and a shared build would race them. The table below is a read-only snapshot of binaries already in `target/release`. It is not a build of today's sources. Bins added after that earlier build are absent. Run `scripts/size-budget.sh` when the tree is quiet for a real pass.

Measured read-only at 2026-10-19T16:02:34+02:00:

| binary | bytes | stripped | note |
| --- | ---: | --- | --- |
| desk-hermes | 1427792 | yes | already on disk, not rebuilt |
| galion-desk | 403056 | yes | already on disk, not rebuilt |
| galion-one | 642728 | yes | already on disk, not rebuilt |
| guardian-tick | 630896 | yes | already on disk, not rebuilt |
| link-watch | 394936 | yes | already on disk, not rebuilt |
| one-defence | n/a | n/a | not present in target/release |
| one-talk | n/a | n/a | not present in target/release |
| persona-plex-relay | n/a | n/a | not present in target/release |
| room | 344304 | yes | already on disk, not rebuilt |

## Musl static (option, not a requirement)

The required build is the host target, `x86_64-unknown-linux-gnu`, dynamically linked to glibc. A static musl build is optional. Use it only when a binary must be copied onto another CPU-only box that does not have a matching glibc. Do not fail a host release because musl was not built.

This host already has the rustup targets `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`. Adding them again is unnecessary. The musl targets link the C runtime statically by default. Do not pass `+crt-static` unless a later ticket asks for it.

```sh
export PATH=/root/.cargo/bin:$PATH
nice -n 19 cargo build -j 2 --release --workspace --target x86_64-unknown-linux-musl
nice -n 19 cargo build -j 2 --release --workspace --target aarch64-unknown-linux-musl
```

The same script can check that optional output. It still honours `CARGO_TARGET_DIR`:

```sh
scripts/size-budget.sh --target x86_64-unknown-linux-musl
```

Artifacts then live in `$CARGO_TARGET_DIR/<triple>/release` (or `target/<triple>/release`). That path is not the host gate.

`edge/openmind-node` is a separate package, not a member of this workspace. Its musl tarballs are out of scope here.
