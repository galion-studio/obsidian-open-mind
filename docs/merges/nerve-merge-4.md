<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# nerve-merge-4 (wave-4 slot 292), 2026-10-27 CET (+01:00)

Fold of the pushed nerve tips onto `swarm/nerve-event-driven` bb287cc.
Box only. **Not deployed**, VPS read-only.

## Merges (all `--no-ff`, no textual conflicts)

| Order | Tip | Commits ahead of bb287cc | Notes |
|---|---|---|---|
| 1 | nerve-flood-cycle-gap 1d33c591 | 2 (incl. flood-baseline 5da6eed) | access.rs cycle-gap spread, zero-baseline skip |
| 2 | nerve-journal-reader-rs 9a76ff6f | 1 | `journal-native` feature (sdjournal, MIT), off by default |
| 3 | nerve-publish-recoveries ae3eee0 | 1 | evt/recover.rs unit fault incidents + recoveries |
| 4 | nerve-t24-wire-gaps-3 97fb7204 | 45 (merge base 8046c41) | brings wire-gaps 1/2/3 **and** the integration-5 lineage (candle-zip-fix vendored candle-core, voice-rust, deny-paste-replace, repo-check nerve gate): 202 files |

## Post-merge fixes (commit "workspace gates green after fold")

1. `evt/journal.rs` `Msg`: `#[allow(clippy::large_enum_variant)]`. Pre-existing:
   bb287cc itself fails `cargo clippy --workspace --all-targets -D warnings`
   with the current toolchain because workspace feature unification turns on
   `serde_json/preserve_order` (NerveEvent ~272 bytes). `-p openmind-nerve`
   alone was green, which is why earlier slots did not see it.
2. `tests/fixtures/cortex-replay.expected.json`: tuner-quick
   `baseline_buckets` 0 -> 168. Caused by the slot 278 cycle-gap spread
   (bisected: bb287cc passes, bb287cc+1d33c591 fails). The slot 278 tip only
   ran `--lib`, so `cortex_replay` was red on 1d33c591 itself. Every other
   value of the golden (score 21, grade F, markdown) is unchanged. The live
   nerve 9d1a4f3 also reports `baseline_buckets: 168` for its streams.
3. Real tests `real_flood_baseline`, `real_flood_cycle_gap`,
   `real_publish_recoveries`, `journal_native_rss`: compile-time
   `REAL_NERVE_PORT_BASE` port-block override (`option_env!`, default
   ports unchanged) so merged re-runs stay in the runner's slot range.
   `journal_native_rss` rewrote `docs/real-tests/nerve-journal-reader-rs.rss.txt`
   with fresh box numbers.

## Gates (box, 2026-10-27 19:15-19:40 CET +01:00)

- `cargo fmt --all --check`: ok
- `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings`: ok
- `cargo clippy -p openmind-nerve --all-targets --features journal-native -- -D warnings`: ok
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`: **281 suites, 1341 passed, 0 failed, 6 ignored**
- `cargo test -p openmind-nerve --features journal-native`: 18 suites, 167 passed, 0 failed
- `scripts/no-python.sh`: ok
- repo-check: 11 errors / 51 warnings = 10 vendored candle-core `portable`
  (known main-merge blocker 0 from integration-5) + guardian-tick nerve gap
  (D-T24-3, known). `--only nerve --nerve-gaps`: 37 bins, 23 covered,
  13 probe-only, 1 gap (guardian-tick).
- openmind-nerve release musl: x86_64 1,463,072 B (sha256 c807e5f3baecfc68...),
  aarch64 1,211,392 B (sha256 8ff378ef268f0c82...); aarch64 `--features journal-native` check ok.
- Sandbox crates not run (sandbox_vps_only).

## Real tests (GALION_REAL=1, release, box ports 21620-21629)

Env: `REAL_NERVE_PORT_BASE=21620 REAL_WIRE_GAPS_PORT_BASE=21620
REAL_WIRE_GAPS2_PORT_BASE=21620 REAL_WIRE_GAPS3_PORT_BASE=21620
REAL_WIRING_PORT_BASE=21620`, `--test-threads=1`. dep-info confirms
`env-dep:REAL_NERVE_PORT_BASE=21620` in the four rebuilt test binaries.

| Test | Result | Time |
|---|---|---|
| real_flood_baseline | 2/0 | 74 s |
| real_flood_cycle_gap | 2/0 | 74 s |
| real_publish_recoveries | 2/0 (box scenarios + VPS journal replay fixture, 82 lines) | 28 s |
| real_wire_gaps | 3/0 (live-first 6, gaps-2 6, gaps-3 5 services push started+heartbeat) | 144 s |
| real_wiring | 1/0 (hermes-rs/memory hung+down phases, client counters) | 115 s |
| journal_native_rss (`--features journal-native`) | 1/0: box journalctl -f 7424 KiB; nerve replay 4648 KiB; nerve native 4692 KiB, no journalctl child | 4 s |

Excerpts: publish-recoveries "units opened 4 resolved 4 repeat_faults 6
cancelled 3; escalations 3"; real_wiring "RSS kB: nerve 5120 hermes 4348
memory 4396". All started processes exited; no listeners left on 21620-21629.

## Live VPS (read-only, 2026-10-27 19:35 CET +01:00)

`galion-nerve.service` runs `/opt/galion/rust/bin/openmind-nerve-9d1a4f3`
on 127.0.0.1:8670: `/health` 200 (`status ok`, `system degraded`, score 39,
cycle 11), `/stats` 200 (`units.hold_s 30`, open [], opened_total 2,
recoveries_cancelled 3, repeat_faults 11), `/api/v1/ddos` 200 (not firing;
edge-cloudflared normal 0.23 req/s vs baseline 0.20, baseline_buckets 168),
`/incidents` 200. Cortex is live separately (not probed here).

Live 9d1a4f3 = bb287cc + publish-recoveries ae3eee0 + **nerve-own-services
9479050** + flood-cycle-gap 1d33c59. It does **not** contain
journal-reader-rs or wire-gaps-3. This branch does **not** contain
nerve-own-services 9479050 (not in the slot 292 row). A trial merge of
9479050 onto this tip is clean (5 files, +403/-1), not committed.

## Owner decisions

- **D-292-1** Add nerve-own-services 9479050 to this fold before any deploy?
  Without it, deploying nerve-merge-4 would drop own-services behaviour that
  is live now. Recommendation: yes, as merge 5 (clean).
- **D-292-2** wire-gaps-3 drags the whole integration-5 lineage (vendored
  candle-core, voice crates, 100+ files) into the nerve line. Accept the full
  fold (done here), or rebuild the line by cherry-picking only the 17 wiring
  commits? Recommendation: accept; main-staging already carries it.
- **D-292-3** The `large_enum_variant` allow vs boxing `NerveEvent` (one
  allocation per journal event). Recommendation: keep the allow.
