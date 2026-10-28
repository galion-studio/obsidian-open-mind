<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Real tests: publish-gates-main (slot 428, 2026-10-28 CET +01:00)

No ports were needed (slot range 22980-22989 left unused): the gates and the
`repo-check` CLI are offline file scanners. All runs on the box, `nice -n 10`.

## Published tree

- 21:40 CET GitHub API (read-only): `main` = `7ba85b60`, tree `a6f6c163`, repo private.
- `git rev-parse 2ce1a08f^{tree}` = `a6f6c163...` (slot 421 reconstruction) -> scanned.

## Launch gates (`scripts/public-export.sh --online`)

`GALION_TOOLS_BIN` = gitleaks 8.30.1, cargo-deny 0.20.2, comply, cargo-cyclonedx 0.5.9
(installed into this slot's own target dir, `cargo install --locked`).

| run | ref | result |
|---|---|---|
| 21:40:44-21:40:53 | 2ce1a08f (main tree), cyclonedx missing | gates 1-8 PASS, 9 SKIP, 10 WARN (71 IPv4, 1 quick tunnel) |
| 21:47 | 2ce1a08f | 9 PASS (74 files, 235 components), 10 WARN (71 / 1); 0 FAIL |
| final | branch head | see end of file |

gitleaks `git` mode (history, 637 commits): repo config 0 leaks; default rules
4 hits = the two allowlisted fixtures (file:line in the security doc).
`scripts/sbom.sh` 246 packages ok; `scripts/spdx-scan.sh` 0 missing of 815;
`scripts/no-python.sh` ok; `repo-check --copyright` 0 errors / 86 utc warnings.

## repo-check deny: real binary on real trees

Operator list = 4 entries (live VPS IPv4 from the box ssh config, box egress
IPv4, retired France IPv4, quick-tunnel host), file mode 600 under /tmp on the
box, never committed, values never printed.

| tree | result |
|---|---|
| main tree (2ce1a08f worktree) | 10 errors, FAIL: entry #1 x4, #2 x3, #3 x2, #4 x1 (F-428-1..4) |
| main export (`git archive 2ce1a08f`) | 10 errors, FAIL |
| branch worktree after 74125369 | 1 error: dist/bin/link-watch, entry #3, binary (F-428-5) |
| branch export (`git archive HEAD`) | 0 errors, PASS |
| list inside --root | exit 2 "inside --root" |
| aarch64 musl release binary under qemu-aarch64, branch export | 0 errors, PASS |

`GALION_REAL=1 GALION_DENY_FILE=<list> cargo test -p repo-check --test real_deny_cli`:
2/2 pass (21:45:52 CET): on-repo hit names file:line + entry #1, never the
value (text and JSON); inside-root refusal; operator list over `git archive HEAD`
-> PASS and no entry value in the output.

## Gates on the branch

- `cargo fmt --all --check` ok
- `cargo clippy --workspace --all-targets -- -D warnings` rc 0
- `env -u GALION_REAL cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`:
  425 suites, 2230 pass / 0 fail / 18 ignored, rc 0
- `cargo test -p repo-check` 39 / 0
- repo-check release x86_64 + aarch64 musl (rust-lld) built; armv7 musleabihf +
  aarch64-linux-android `cargo check` ok

## Live observation (read-only, 21:47 CET)

- nerve 127.0.0.1:8670 /state 200: mode live, cycle 18, self_eval 68 / D,
  3 incidents (galion-one mouth stuck loading, edge; unrelated to this slot).
- cortex 127.0.0.1:8674 /health 200: live, ok, incidents_open 2, escalations 0.
- Nothing written on the VPS.
