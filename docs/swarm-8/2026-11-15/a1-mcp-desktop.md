<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Maciej Grajczyk -->

# Swarm 8, group A, agent A1

Written 2026-11-15T15:00:21+01:00 (Europe/Zagreb).

Draft pull requests, still drafts:

- https://github.com/galion-studio/obsidian-open-mind/pull/51 (`cursor/hermes-mcp-client-3083`)
- https://github.com/galion-studio/obsidian-open-mind/pull/52 (`cursor/desktop-ci-artifacts-c14b`)

Both branches were 11 commits behind `main` (`de7480e7`) and had already been pushed. A rebase would have rewritten those commits and needed a force-push, so `main` was merged in. Nothing was force-pushed, merged into `main`, or marked ready for review.

## Pull request 51

`origin/main` merged cleanly. The follow-up commit tightens the client:

- A `url` that contains a token-shaped string, or a query name ending in `key`, `token`, `secret`, or `password` with a value of 8 characters or more, is dropped and not stored (`url_embeds_secret` in `crates/hermes-rs/src/config.rs`).
- Tests cover an exact GitHub host (including a different server name and a mixed-case host), a lookalike host that is not clamped, an empty allowlist that does not spawn, a write-only GitHub allowlist that does not connect, session ids with spaces or control characters, and curl arguments `--max-redirs 0` plus `--proto`.
- `crates/hermes-gateway/tests/mcp_http_off.rs` checks that the default gateway build reports a `url` server and does not dial. `mcp-http` stays out of `default`.
- `docs/hermes-mcp.md` has a "How to use it" section for stdio and for `HERMES_RS_MCP_GITHUB=1`. The x86_64 musl size-budget log for `05adf328` (2026-11-15T15:05:37+01:00, job 132836192211) measured `hermes-gateway` at 2,092,640 bytes, stripped, status ok, 4,512 bytes under the 2,097,152 cap. The earlier log for `08b8318e` measured 2,091,904 bytes. Both leave `mcp-http` off.

## Pull request 52

`main` did not merge cleanly. Conflicts were in `.github/workflows/openmind-desktop.yml` and `docs/desktop/README.md`. The resolution keeps main's Windows and macOS clippy and tests, then packages and uploads. Windows and macOS timeouts are 40 minutes. The Linux job also checks that a changed README makes `check-package.sh` exit 1, and that a LICENSE missing the last brand line makes it exit 1 after the checksums are rewritten.

`docs/desktop/README.md` names the four artifact directories and how to run the unpacked binary or install the `.deb`. `crates/openmind-desktop/src/page.rs` checks that `scripts/desktop/brand-lines.txt` matches `BRAND_LINES` and that `scripts/desktop/artifact-readme.md` says how to run the binary.

## Commands and results

Rust on the default toolchain here is 1.83.0. `galion-common` on current `main` does not compile with it (`E0658` on `is_multiple_of` and `as_chunks`). The commands below used `rustc 1.100.0` (`RUSTUP_TOOLCHAIN=stable`), except where noted.

On the MCP branch, after the code change:

```
cargo +stable clippy -j 2 -p hermes-rs --all-targets --locked -- -D warnings
cargo +stable test -j 2 -p hermes-rs --locked --lib --test mcp
cargo +stable test -j 2 -p hermes-gateway --locked --test mcp_http_off
cargo +stable clippy -j 2 -p hermes-gateway --all-targets --locked -- -D warnings
cargo +stable fmt --all -- --check
```

Results: hermes-rs clippy exit 0. hermes-rs library tests 61 passed. `tests/mcp.rs` 2 passed. `mcp_http_off` 2 passed. hermes-gateway clippy exit 0, with one pre-existing manifest warning that `galion-feature-pin` is an unused dependency (`crates/hermes-gateway/Cargo.toml`). The same warning is in the `08b8318e` size-budget log. `fmt --all -- --check` exit 0.

`cargo test -j 2 --workspace --locked` on that branch ran for about 204 seconds and failed only `galion-feature-pin` `release_feature_unify` (2 tests). `cargo tree --offline` could not download `curve25519-dalek-derive` v0.1.1 for the musl targets. After `cargo tree -p hermes-gateway --locked --target x86_64-unknown-linux-musl` (and the aarch64 target) downloaded that crate, the same test file passed: 5 passed, 0 failed.

On the desktop branch:

```
cargo +stable test -j 2 -p openmind-desktop --locked --lib
cargo +stable clippy -j 2 -p openmind-desktop --all-targets --locked -- -D warnings
cargo +stable fmt --all -- --check
```

Library tests 45 passed, including `brand_lines_file_matches_the_page` and `artifact_readme_explains_how_to_run`. Clippy exit 0. `fmt --all -- --check` exit 0. After dropping two assertions, `cargo +stable test -j 2 -p openmind-desktop --locked --lib page::tests::` passed 7 tests.

`scripts/desktop/package-artifact.sh --out /tmp/openmind-desktop-ci` with rustc 1.83.0 failed while compiling `galion-common` (`E0658`). The same command with rustc 1.100.0 wrote the tarball and the amd64 `.deb`. `scripts/desktop/check-package.sh /tmp/openmind-desktop-ci` exited 0. A README with one extra newline failed `sha256sum -c` and `check-package.sh` exited 1. A LICENSE missing the last brand line, after `SHA256SUMS` was rewritten, failed the byte compare and exited 1. `--publish`, `--sign`, and `OPENMIND_DESKTOP_PUBLISH=1` each exited 2 and did not create `/tmp/openmind-desktop-no-artifact`.

This machine did not build the Windows, macOS, or aarch64 packages. Those jobs stay on CI.

## CI before this update

Pull request 51 head `08b8318e`: 26 check runs, 25 success, 1 skipped (`miri full`). I read the tail of job 132641771940. The x86_64 musl size-budget log at 2026-11-15T01:31:46+01:00 lists `hermes-gateway` at 2,091,904 bytes, stripped, status ok, cap 2,097,152.

Pull request 52 head `0cde1c54`: 26 check runs, 25 success, 1 skipped. GitHub reported `mergeable_state` dirty before the merge.

CI on `05adf328` (pull request 51) completed with 26 checks: 25 success, 1 skipped (`miri full`). Pull request 52's `self-eval` workflow was still running when this note was updated. The other four workflows on `406dece3` (`openmind-desktop`, `realtest`, `launch-gates`, `miri`) had already succeeded.

## Secrets and addresses

I searched the pull-request diffs for token prefixes, private-key blocks, and dotted IPv4 addresses. Pull request 51's new commit adds no token and no dotted address. The branch still has loopback literals in the MCP docs and tests, the same kind the desktop docs already use. No other IPv4 address showed up. Pull request 52's resolution adds no dotted address and no token. The packaged LICENSE and README are checked for URLs and IPv4 addresses by `check-package.sh`. No GitHub token was set and `api.githubcopilot.com` was not called.

## Decisions for Maciej

1. Leave `mcp-http` out of the default `hermes-gateway` build. On `05adf328` the x86_64 musl binary is 2,092,640 bytes against a 2,097,152 cap (4,512 bytes of room). Turning the feature on in the default build still needs a size-cap decision.
2. Keep read-only GitHub opt-in (`HERMES_RS_MCP_GITHUB=1`), with `GITHUB_TOKEN` read at request time, host `api.githubcopilot.com`, and header `X-MCP-Readonly: true`.
3. Drop a `url` that already contains a token, or a query name ending in key, token, secret, or password with a value of 8 or more characters, instead of storing it. The entry disappears from config with no separate error line.
4. Cleartext HTTP still allows `localhost` and `::1` as well as the IPv4 loopback address. `localhost` is a name, not a parsed loopback address.
5. Copyright lines were not edited. The repository MIT text says `Copyright (c) 3000 Galion / galion-studio`. The fourth brand line says `Copyright © 2026 Galion Labs. All rights reserved.` This note names Maciej Grajczyk, as the swarm brief does. Say if the packaged LICENSE or the brand line should name you.
6. Desktop output stays a workflow artifact: 14-day retention, `actions: write`, no GitHub Release, no signature. Windows and macOS run clippy and tests, then package. The job timeout is 40 minutes.
7. Both updates are merges of `main`, not rebases.

## Brand lines

These four lines are `BRAND_LINES` in `crates/openmind-desktop/src/page.rs` and `scripts/desktop/brand-lines.txt`. They were not changed.

G.A.L.I.O.N. — General Autonomous Local Intelligence Open Network
Developed and distributed by Galion Labs
Perpetual Open MIT License
Copyright © 2026 Galion Labs. All rights reserved.

MIT License

Copyright (c) 2026 Maciej Grajczyk
