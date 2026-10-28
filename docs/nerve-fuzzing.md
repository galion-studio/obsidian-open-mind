<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Nerve parser fuzzing (T3.1, wave-4 slot 212)

Written 2026-10-21 (Europe/Zagreb, CEST +02:00). Branch `swarm/nerve-parser-fuzz`,
base aa77864 (the live nerve build lineage, nerve-dst-clock).

## Layout

The harness is the cortex T5.2 one (`swarm/cortex-t52-fuzz` c2f7cda,
`crates/openmind-nerve/tests/fuzz/mod.rs`), copied unchanged except
`pub mod targets;` -> `pub mod nerve;`, because aa77864 does not contain
the cortex lineage. std only: SplitMix64, structure-unaware mutators,
signature corpus, `catch_unwind` per input, no new dependency, stable
toolchain.

| file | what |
|---|---|
| `crates/openmind-nerve/tests/fuzz/mod.rs` | harness (from t52) |
| `crates/openmind-nerve/tests/fuzz/nerve.rs` | the nerve targets below |
| `crates/openmind-nerve/tests/nerve_fuzz.rs` | short fixed-seed runs, 60 s summary (`--ignored`), regressions, stack probe |
| `crates/openmind-nerve/tests/real_parser_fuzz.rs` | `GALION_REAL=1` real binary + VPS read-only captures |

Merge note: when the cortex lineage (with t52) and this branch meet,
`tests/fuzz/mod.rs` is an add/add conflict; resolve by taking t52's file
and adding `pub mod nerve;`. No target overlaps by file.

## Targets

Skipped because they already exist (t52, cortex lineage): ss / proc-net-tcp,
`/event` JSONL feed (event_feed sensor), ingress YAML, edgerate `dest=`.
New here, on the live-build parsers:

| target | parser(s) | oracle (violation = fail) |
|---|---|---|
| `signal-body` | `hub::parse_signal` | accepted only <= MAX_BODY; source `[A-Za-z0-9-_.]{1,64}`; kind in the 4 kinds; name <= 64 / msg <= 300 chars, no control chars; value finite; `to_json` is JSON; a fresh hub ingests it |
| `event-body` | `server::event_from` | source/kind/severity non-empty <= 128 B; `data` carried (numbers within 4 ULP, D-212-3) |
| `http-read-req` | `server::read_req` | same outcome for one read, 2048 B, 13 B and 1 B segments; parsed path has no `?`; head incl. blank line <= 8192 B; errors only 400/413/431 |
| `journald-json` | `probe::parse_journal_lines`, `journal_counts`, `logscan::insights` | records <= lines; msg <= 2000 chars; unit filter respected; counts add up |
| `cf-metrics` | `probe::parse_cloudflared_metrics` | differential against the spec: last line wins, only finite non-negative values that fit u64 |
| `cf-log-lines` | `classify`, `zerolog_level`, `logscan::refused_port`, `dest_host`, `scan_paths`, `insights` | every extracted port/host/path is in the line; classify in 0..=2, prio <= 3 is error; dead-origin counts sane |
| `access-line` | `access::parse_line` | ip <= 64, path <= 200 chars; a non-zero status appears in the line |
| `signal-roundtrip` | gen | valid signal in, same fields out (cleaned/capped as documented) |
| `metrics-roundtrip` | gen | random metrics text (comments, labels, exponents) parses to the generated values |
| `http-roundtrip` | gen | random valid request reads back unchanged at 4 segmentations |
| `stamp-roundtrip` | gen | random instants, offsets -14..+14 h (and :30) via RFC 3339 and CLF |
| `deep-json-stack` | gen | 1..160 nested arrays/objects through /signal and /event on a `server::CONN_STACK` thread |

Env: `CORTEX_FUZZ_SECS`, `CORTEX_FUZZ_ITERS`, `CORTEX_FUZZ_SEED`,
`CORTEX_FUZZ_SEEDS=<dir>` (`journal.jsonl`, `cloudflared-metrics.txt`,
`cloudflared-log.txt`, `access.log`, `signals.jsonl`, `events.jsonl`),
`CORTEX_FUZZ_CRASH_DIR`.

```
CORTEX_FUZZ_SECS=60 cargo test --release -p openmind-nerve --test nerve_fuzz -- --ignored --nocapture --test-threads 1 nerve_fuzz_all_summary
```

## Finds and fixes (each with a regression test in `tests/nerve_fuzz.rs`)

1. **read_req head limit depended on TCP segmentation** (`regress_read_req_head_limit_is_segmentation_independent`).
   The 8 KiB check ran only when the terminator was not yet found, after a
   2 KiB read: heads of 8193..10240 B were accepted in large segments and
   refused (431) in small ones. Now `MAX_HEAD = 8192` covers the head incl.
   `\r\n\r\n` whatever the segmentation; the terminator search also only
   rescans new bytes (was O(n^2) per request with small segments).
2. **cloudflared metrics: `inf`/`1e20` became u64::MAX, `-1`/`NaN` became 0**
   (`regress_cloudflared_metrics_reject_non_finite_and_negative`). Such
   values are now `None` (unknown). Defence in depth: the tunnel error-rate
   check in `state.rs` uses `de.saturating_mul(4)` (u64::MAX errors would
   have overflowed: panic in debug, wrap in release).
3. **access JSON status wrapped** (`"status":70000` -> 4464 via `as u16`)
   (`regress_access_status_does_not_wrap`). Out-of-range status is 0 now.
4. **access JSON client address uncapped** (CLF branch caps at 64 chars; it
   is the per-IP map key) (`regress_access_json_ip_capped`). Capped at 64.
5. **stack overflow on deep JSON** (`regress_deep_json_fits_server_stack`):
   found by the workspace `cargo test` run, not by a target alone: with the
   workspace feature set (serde_json `preserve_order`) a debug build needs
   256-384 KiB to parse 128 nested levels; the connection thread had
   256 KiB, so the process aborted (`has overflowed its stack`). Release
   needs 64 KiB (default features) / 96 KiB (preserve_order), so release
   builds were not affected at the 256 KiB size. `server::CONN_STACK` is
   now 512 KiB (pages are touched only when used). Measured with
   `probe_deep_json_stack_kb` (ignored test, one process per size):

   | build | 32 | 48 | 64 | 96 | 128 | 192 | 256 | 384 KiB |
   |---|---|---|---|---|---|---|---|---|
   | debug, default | ovf | ovf | ovf | ovf | ovf | ovf | ok | ok |
   | debug, preserve_order | ovf | ovf | ovf | ovf | ovf | ovf | **ovf** | ok |
   | release, default | ovf | ovf | ok | ok | ok | ok | ok | ok |
   | release, preserve_order | ovf | ovf | ovf | ok | ok | ok | ok | ok |

Not bugs (oracle adjusted, documented):
- `/signal` value and `/event` data: serde_json without `float_roundtrip`
  can move a big float by 1 ULP (`/event` prints and re-parses `data`).
  `regress_event_big_number_within_ulps`. Owner decision D-212-3.
- journald `__REALTIME_TIMESTAMP` far in the future: the field is set by
  journald (trusted `__` field), not by services, so no clamp.

## Owner decisions

- **D-212-1** Keep the stricter head limit (8192 B incl. the blank line;
  before, 8193..10240 B could pass depending on segmentation)? Recommended
  yes (only oversize heads are affected; nerve-client heads are < 300 B).
- **D-212-2** Connection stack 512 KiB vs keep 256 KiB and make debug /
  workspace builds out of scope. Recommended 512 KiB.
- **D-212-3** Enable serde_json `float_roundtrip` workspace-wide (exact
  floats in /signal values and /event data) or accept <= 1 ULP drift.
  Recommended accept (feature change touches every crate).
- **D-212-4** `read_req` leniencies left unchanged (behaviour of the live
  build): a body shorter than Content-Length at EOF is accepted truncated;
  `Content-Length: +5` parses; duplicate differing Content-Length takes the
  first; a client may drip a byte every < 5 s and hold one of 16 slots.
  Options: 400 on short body / non-digit / conflicting length, an overall
  per-request deadline. Loopback-only listener, so low risk.
- **D-212-5** Merge layout: keep `tests/fuzz/mod.rs` shared with t52 (one
  harness) as described above.
