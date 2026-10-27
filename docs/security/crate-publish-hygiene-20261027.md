<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Crate publish hygiene (slot 296, 2026-10-27 CET +01:00)

Follow-up to slot 288 `swarm/cargo-deny-gate` 400e93a5 (owner item CD-1).
Branch `swarm/crate-publish-hygiene`, base 400e93a5.

## Change

| What | Before | After |
|---|---|---|
| `publish = false` in workspace crates | 2/68 (galion-tar, galion-deploy) | **68/68** (+ edge/openmind-node, already set) |
| path deps without a version req | 103 lines in 46 crates + 1 in edge/openmind-node | **0** |
| `deny.toml [bans] wildcards` | `"warn"` (49 warnings) | **`"deny"`** |
| `deny.toml [bans] allow-wildcard-paths` | `true` | **`false`** (path deps get no free pass) |

- `publish = false` is inserted right after the `version` line of `[package]`.
- Each `{ path = "../x" }` became `{ path = "../x", version = "<x's version>" }`:
  `0.1.0` (workspace version) for all targets; galion-health is `0.2.0` but no
  crate depends on it. The "46 wildcard deps" from slot 288 are the 46 crates
  with unversioned path deps (103 dep lines in total).
- No registry dependency used `"*"`; no dependency was added or removed;
  `Cargo.lock` content is unchanged (cargo only re-sorted the existing galion-tar entry into alphabetical order).
- New test `crates/repo-check/tests/publish_hygiene.rs` (std only):
  static checks always run (every crate has `publish = false`; every path dep
  has a version equal to its target crate's version; parser self-test).
  `GALION_REAL=1` also runs real `cargo-deny --offline check bans` on the repo
  and on a throwaway 2-crate workspace using this `deny.toml` as a negative
  control (unversioned path dep must FAIL, versioned must pass).

Caveat for future work: bumping a crate's version past a caret boundary
(e.g. 0.1.x to 0.2.0) needs the dependents' `version = "..."` bumped too —
cargo fails resolution loudly, and the hygiene test names the stale line.

## Evidence (box, Europe/Zagreb CET +01:00)

Manual negative controls (2026-10-27 ~19:16):

```
wildcards=deny, allow-wildcard-paths=false, all versioned      -> bans ok
gaia-live: gaia-core version removed (strict)                  -> error[wildcard] ... 'gaia-live'; bans FAILED
same, allow-wildcard-paths=true (publish=false still set)      -> bans ok
same, allow-wildcard-paths=true and publish=false removed      -> error[wildcard] ... does not apply to public crates; bans FAILED
```

Gate results: see GATES section below.

## GATES (box, 2026-10-27 19:16-19:27 CET +01:00)

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | ok |
| `cargo-deny --offline check licenses bans sources` (workspace, wildcards=deny) | licenses ok, bans ok, sources ok (0 wildcard warnings, was 49) |
| `cargo-deny check advisories` | ok |
| same deny.toml on `edge/openmind-node` | licenses ok, bans ok, sources ok |
| `cargo clippy --workspace --all-targets -- -D warnings` (excl. openmind-sandbox, cgroup-detect: sandbox_vps_only) | ok |
| `cargo test --workspace` (same exclusions) | 385 suites, 2030 passed, 0 failed, 18 ignored |
| `GALION_REAL=1 cargo test -p repo-check --test publish_hygiene` | 5/5 (incl. real cargo-deny on repo + negative control) |
| `scripts/no-python.sh` | ok |
| `repo-check --root .` | 2121 files, 0 errors, 83 warnings, PASS |

Cross builds not re-run: manifest-only change (publish flag + version reqs on
existing path deps), no code or dependency graph change.

VPS read-only (2026-10-27 19:16:47 CET +01:00): nerve 127.0.0.1:8670/state
200 (35796 B), /healthz `ok`; cortex :8674 200. Nothing deployed. Ports
21660-21669 unused (no server in this task).

## Owner decisions

- **CD-3**: keep `allow-wildcard-paths = false` (strict: every path dep needs a
  version req) or relax to `true` (path deps OK as long as the crate has
  `publish = false`). Strict is what this branch ships; branches that add new
  crates/path deps must follow it or the gate fails.
- CD-1 from slot 288 is resolved by this branch once merged into main-staging.
