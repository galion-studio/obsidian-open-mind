<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: sec-unsafe-safety (slot 229)

All times are Europe/Zagreb (CEST, +02:00). Box only; ports 20990-20999 were
not needed, since nothing listens.

## Real binary: repo-check `safety`

`crates/repo-check/tests/real_safety.rs` (GALION_REAL=1) builds and runs the
real `repo-check` binary:

```
$ GALION_REAL=1 cargo test --release -p repo-check --test real_safety -- --nocapture
2026-10-21 23:56:48 +0200
warn  safety   edge/x/src/lib.rs:3: unsafe block without a `// SAFETY:` comment (or `# Safety` doc); say why the call is sound
repo-check 2026-10-21T23:56:57+02:00: 3 files, 0 error(s), 1 warning(s) -> PASS
test real_planted_block_warns_then_errors_then_clears ... ok
repo-check 2026-10-21T23:56:57+02:00: 1521 files, 0 error(s), 0 warning(s) -> PASS
test real_tree_has_no_undocumented_unsafe ... ok
test result: ok. 2 passed; 0 failed
```

- On the real tree, `--only safety` gives 0 findings across 1521 files.
- In a planted tree (one bare block in edge/, the same file in vendor/):
  the default run warns and exits 0 (warn first). `--error safety` exits 1.
  `--json` names `edge/x/src/lib.rs` line 3 with a +02:00 timestamp. vendor/
  is ignored. Adding the comment makes it PASS.
- Before (base 0d68f46 plus the new check only): 38 safety warnings (edge
  31, openmind-sandbox 1, galion-backup 3, galion-guard 2, galion-voice-cpu 1).
  Full repo-check: base 10 errors / 48 warnings; branch 10 errors / 48
  warnings. The 10 errors are the known vendored candle `portable` blocker
  (slot 177), unchanged.

## Clippy cross-check

```
cargo clippy --all-targets -- -D warnings -W clippy::undocumented_unsafe_blocks -W clippy::missing_safety_doc
```

- Base: edge 32 undocumented blocks, openmind-sandbox 1.
- Branch: 0 on edge, and 0 on openmind-sandbox, galion-backup, galion-guard,
  galion-voice-cpu and repo-check (exit 0).

## Tests

- Workspace (`--exclude openmind-sandbox --exclude cgroup-detect`):
  269 suites, 1265 passed, 0 failed, 6 ignored.
- repo-check: lib 11/11 (new `safety` unit tests), checks 14/14 (new
  `safety_scope_level_and_message`), real_safety 2/2.
- edge openmind-node on the box: 51 passed with `--skip sandbox:: --skip
  seccomp:: --skip doctor::`, plus the pure seccomp tests
  `program_ends_in_allow_and_denies_ptrace`,
  `denied_numbers_match_libc_for_this_arch` and the new
  `program_length_fits_sock_fprog` (3/3, nothing installed).
  Deviation: the 51 included `boot::tests::require_seccomp_matches_self_test`,
  which runs `seccomp::self_test()`: throwaway forked children install the
  deny-list filter and exit. No command was sandboxed and nothing persists,
  but strictly that belongs on the VPS.
- **Not run: the sandbox/seccomp tests on the VPS.** Static x86_64 musl test
  binaries were built (omnode unit tests, openmind_sandbox lib tests,
  openmind-sandbox tests/sandbox.rs). The upload to
  `/var/tmp/sandbox-test-<ts>` was blocked by Auto-review at 23:55. Per the
  slot instructions, no approval was requested and nothing was retried, so
  the VPS sandbox run of the edited tests (F2 fix) is still open. Nothing was
  created on the VPS.

## Builds

- x86_64 musl release: repo-check, omnode (static).
- aarch64 musl release (rust-lld): repo-check, omnode (static).
- `cargo check` aarch64-linux-android and armv7-musleabihf: repo-check and
  omnode pass. galion-backup fails only in libsqlite3-sys's C build script
  (no cross C compiler; pre-existing, the same zig-cc gap other slots
  recorded). Its change here is comments only.
- fmt, workspace clippy `-D warnings` (excl. sandbox crates), clippy on
  openmind-sandbox, and no-python all pass.

## Live VPS (read-only)

2026-10-21 23:56:25 +0200, `GET 127.0.0.1:8670/state`: nerve mode live,
cycle 9, score 66 grade D (17 healthy, 2 failing-soon, 1 degrading
(galion-one mouth 503 loading), 1 degraded, 1 absent). `systemctl
is-active galion-nerve gaia-cortex`: active, active (the cortex is live).
No /var/tmp/sandbox-test-* folders. This slot changes no live service. The
edge node and openmind-sandbox aren't deployed as units, so there's nothing
live to compare against.
