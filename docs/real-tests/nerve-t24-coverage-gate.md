<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Real-life tests: nerve-t24-coverage-gate (wave-4 slot 226)

Branch `swarm/nerve-t24-coverage-gate`, base integration-5 `0d68f46`.
Rule: repo-check `nerve` (error). Every workspace bin reaches `nerve-client`
(normal, non-optional dependency, direct or through a workspace crate), or it is
listed probe-only with a reason in `crates/repo-check/nerve-probe-only.txt`.
No services were wired. This change only lists the gaps.
New dependencies: none. The rule is std only, like the rest of repo-check.

## Box: real binary on the real tree (2026-10-21 23:51 to 23:54 CEST +02:00)

`repo-check --only nerve --nerve-gaps` (release build) exits 1 with
`nerve coverage: 37 bins, 4 covered, 14 probe-only, 19 gap(s)` and
`1523 files, 19 error(s), 0 warning(s) -> FAIL`.

Cross-check against Cargo: `cargo metadata --no-deps` lists 37 bin targets.
That set is identical to repo-check's 37 (`diff` is empty). The resolver shows
exactly two crates with a direct `nerve_client` dependency: hermes-rs (lib) and
openmind-memory.

| status | bins |
|---|---|
| covered, direct | openmind-memory |
| covered via hermes-rs | hermes-gateway (live hermes-rs.service), hermes-bench, hermes-tools:hermes-mock-model |
| probe-only (14) | openmind-nerve (is the nerve), repo-check, bench-banking, bench-scoreboard, galion-bench-mock, galion-vending, galion-wallstreet, galleon-arena, galion-realtest, gaia-dummy, openmind-mcp-stack, one-set-model, one-talk, one-defence |
| **GAP (19)** | desk-hermes, gaia, galion-backup, galion-boot, galion-deploy, galion-desk, galion-guard, galion-health, galion-one, galion-origin, galion-voice-cpu:galion-voice-serve, guardian-tick, hades-gate, link-watch, openmind-discovery, openmind-queue, persona-plex-relay, room, tuner |

Full repo-check on the tree: 29 errors. That is 19 `nerve` plus the 10 `portable`
errors in vendored candle-core that were already there (main-merge blocker 0,
slot 177). There are also 48 utc warnings, which are unchanged.

## VPS: read-only, compared with the live nerve (2026-10-21 23:49 to 23:54 CEST +02:00)

`systemctl list-units` (read-only) shows these services running from workspace
bins: galion-desk, galion-health, galion-nerve (openmind-nerve-bb287cc),
galion-one, hermes-rs (hermes-gateway-aa77864), link-watch-rs, openmind-memory
(-aa77864), openmind-queue and tuner. galion-backup.timer is also active. The nerve
reports cycle score 66/D.

`GET 127.0.0.1:8670/state` returns `push_age_s` for each component. It agrees
with the static verdict in 7 of 7 cases:

```
openmind-memory  push_age_s=30    openmind-memory:openmind-memory    covered
hermes-rs        push_age_s=14    hermes-gateway:hermes-gateway      covered
link-watch-rs    push_age_s=null  link-watch:link-watch              gap
galion-desk      push_age_s=null  galion-desk:galion-desk            gap
tuner            push_age_s=null  tuner:tuner                        gap
galion-one       push_age_s=null  galion-one:galion-one              gap
galion-health    push_age_s=null  galion-health:galion-health        gap
```

The nerve also pushes data to gaia-cortex, but gaia-cortex is not in this tree.
openmind-queue runs live but is not a nerve component at all. It has no push
and no pull probe, so it is the blindest gap.

## Tests

* `cargo test -p repo-check`: 13/13 existing checks plus 8/8 new `tests/nerve.rs`
  (parser, Cargo bin rules, direct/transitive/optional/dev dependencies, the
  allowlist's stale/redundant/malformed/duplicate entries, a missing allowlist,
  and the format of the checked-in allowlist).
* `GALION_REAL=1 GALION_REAL_VPS=1 cargo test --release -p repo-check --test real_nerve_gate`:
  3/3 (the real tree compared with cargo metadata, exit codes on a fixture
  tree, and the live VPS nerve cross-check).
* gaia-minerva (uses repo_check::scan_text): clippy clean, 5/5 tests.
* fmt is clean and clippy `-D warnings` is clean. `cargo check` passes for
  x86_64-musl, aarch64-musl, aarch64-android and armv7-musleabihf. The
  x86_64-musl release build is 508,592 bytes. no-python is ok.

## Owner decisions

* D-T24-1: The CI self-eval job and `scripts/real/docs-sync.sh` run full
  repo-check, and they now fail on 19 more errors. Choose one: keep `nerve` at
  error (the CI job is already red from the candle portable errors), or run it
  as `--warn nerve` until the gaps are wired.
* D-T24-2: Classify the 19 gaps. Wire first the live services the nerve cannot
  see: openmind-queue, galion-one, galion-health, galion-desk, link-watch and
  tuner, then galion-backup, galion-deploy, galion-guard and guardian-tick.
  The other option is to accept some as probe-only. gaia, hades-gate,
  galion-origin, room, desk-hermes, openmind-discovery, persona-plex-relay and
  galion-voice-serve also need a decision.
* D-T24-3: guardian-tick writes nerve events to a file through the
  `openmind-nerve` lib and does not use nerve-client. Should that count as
  coverage?
* D-T24-4: Transitive coverage counts. hermes-bench and hermes-mock-model are
  covered because they depend on hermes-rs. Should benches be required to opt
  out explicitly?
