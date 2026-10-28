<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Cortex parser fuzzing (plan T5.2)

Slot 185 `cortex-t52-fuzz`, base `cortex-merge-all` 6bcb882 plus
`swarm/tunnel-audit-2` 988e47f (which brings `crates/tunnel-lint`, the
ingress YAML parser). Written 2026-10-21 +02:00.

## Why not cargo-fuzz / proptest

The task says "no new asm deps". `cargo-fuzz` needs nightly and
`libfuzzer-sys` (C++ built by `cc`); `proptest` / `quickcheck` /
`arbitrary` pull in a handful of crates. The harness here is ~400 lines of
std-only Rust in `crates/openmind-nerve/tests/fuzz/mod.rs`:

- SplitMix64 PRNG (fixed default seed so `cargo test` is reproducible).
- Mutators: bit flip, random byte, insert, delete range, duplicate range
  (up to x200, for long lines), dictionary tokens (common + per target),
  replace run with token, splice with another corpus entry, truncate, hex
  digit swap. Byte-level edits are repaired with `from_utf8_lossy`, which
  also injects U+FFFD (multi-byte) into inputs.
- Corpus keeps an input when its **output signature** is new (a cheap
  coverage proxy; each target defines a "shape" signature, e.g. counts of
  parsed rows, Some/None per field), capped at 512.
- `catch_unwind` around every input; panics and property violations are
  counted and the input saved under `CORTEX_FUZZ_CRASH_DIR`
  (default `$TMPDIR/cortex-fuzz-crashes`).

The only new dependency is a **dev-dependency on the workspace path crate
`tunnel-lint`** (no external crates) so one harness can drive the ingress
parser too.

## Targets

| Target | Parser(s) | Properties beyond "no panic" |
|---|---|---|
| `ss-ltn` | `probe::parse_ss`, `tunnel_lint::SsSnapshot::parse` | sorted+deduped; loopback flag matches addr; no `%`/`[` left; **differential**: both parsers give the same (addr, port) set on lines starting with the token `LISTEN` |
| `proc-net-tcp` | `probe::parse_proc_net_tcp` (v4+v6), `proc_owner::parse_proc_net_tcp`, `connrate::parse_tables` | every proc_owner row is also a probe row; proc_owner addr is an IP and its loopback flag is right; conn-rate keys == watch list, ESTAB+TIME-WAIT <= identities <= lines |
| `journald-json` | `probe::parse_journal_lines` (filtered + unfiltered), `journal_counts`, `logscan::insights` | records <= lines; MESSAGE <= 2000 chars; filtered unit in filter; counts add up; errors+warnings <= lines |
| `cloudflared-msg` | `edgerate::parse`, `edgerate::host_of`, `logscan::dest_host`, `refused_port`, `scan_paths`, `probe::classify`, `insights` | hosts are lower-case DNS names <= 253; `dest_host` <= 100 chars, no `/`/`?`; refused port only on "connection refused"; scan paths sorted, start with `/`, <= 120 chars |
| `event-jsonl` | `EventFeed::ingest_line` + one `sense` cycle, `server::event_from` | `stats.lines` == non-empty lines; bad+skipped+old <= lines; findings <= `MAX_KEYS`; ids `[A-Za-z0-9._:-]`; no control chars in title/component/evidence; cortex JSON valid; everything expires after the window |
| `rfc3339` | `event_feed::parse_rfc3339` vs `util::parse_rfc3339` | **differential**: whatever the strict event-feed parser accepts, the shared parser accepts with the same value |
| `http-event-req` | `server::read_req` (the `POST /event` / `/signal` reader) | error codes only 400/413/431; path keeps no query |
| `ingress-yaml` | `tunnel_lint::parse`, `origin`, `lint`, `stage` | rule indices 1..n; line ranges inside the file, ordered, not overlapping; service non-empty; **the staged config re-parses with exactly n - (closed, non-catch-all, local) rules** |

Generative round-trip properties (render known values the way the kernel /
journald / cloudflared write them, parse, compare): `gen-proc-net`,
`gen-ss`, `gen-journal`, `gen-cloudflared`, `gen-event`, `gen-rfc3339`
(random instants through `zagreb_time::from_unix`, both parsers),
`gen-ingress` (random configs incl. quoting, nested `originRequest` lists,
comments, trailing top-level keys).

## Running

```
# short reproducible budget (part of cargo test, ~3 s debug)
cargo test -p openmind-nerve --test fuzz_parsers
# box run: N seconds per target, one count line each
CORTEX_FUZZ_SECS=25 cargo test -p openmind-nerve --test fuzz_parsers -- --ignored fuzz_all_summary --nocapture
# seeds from real captures (ss-ltn.txt, proc-net-tcp.txt, proc-net-tcp6.txt,
# journal.jsonl, ingress.yml, events.jsonl)
CORTEX_FUZZ_SEEDS=/tmp/captures ...
# real-life test (release; VPS part is read-only ssh)
GALION_REAL=1 CORTEX_FUZZ_VPS=1 cargo test --release -p openmind-nerve --test real_cortex_fuzz -- --nocapture --test-threads 1
```

Run the debug build for long runs too: it keeps integer overflow checks on;
the release profile turns them off (its `panic = "abort"` does not apply to
test binaries, so `catch_unwind` works in both).

## Finds and fixes (all four have a regression test in `fuzz_parsers.rs`)

1. **`probe::parse_ss` zone order** (`ss-ltn` differential + `gen-ss`): `ss`
   prints scoped addresses as `[::1]%lo:8670`; the old code trimmed
   brackets before cutting the `%zone`, leaving `::1]`, which was then
   flagged **not loopback**. Only `replay` (captured `ss.txt`) uses
   `parse_ss`; the live nerve reads `/proc`. Fix: cut the zone first.
2. **`probe::hex_ipv6` char-boundary panic** (`proc-net-tcp`, found after a
   multi-byte seed was added): a 32-byte address field with multi-byte
   UTF-8 sliced `&h[i*8..]` mid-character and panicked. Not reachable from
   the kernel's ASCII `/proc`, but reachable from replay files. Fix: `get`.
3. **`event_feed::parse_rfc3339` signed fields** (`rfc3339` differential):
   `str::parse::<i64>` accepts a sign, so `2026-10-+5T...` or
   `...T01:+0:00` and offsets like `++2:00` were accepted (the shared
   `util` parser refused them). Reachable through any JSONL file in
   `NERVE_EVENT_FEED_FILE` (the `ts` of `POST /event` records is written by
   the nerve itself). Fix: digits only.
4. **`tunnel_lint::parse` silent stop** (`ingress-yaml` staged-config
   property): a list item **less indented** than the first ingress item
   ended the list silently, so every later rule was not linted (one
   mutant hid 36 of 37 rules). YAML has no such construct and cloudflared
   refuses the file; now it is a parse error naming the line.
