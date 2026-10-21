<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# CI fuzzing (`.github/workflows/fuzz.yml`)

Security S1.3, slot 202, written 2026-10-21 (Europe/Zagreb, CEST +02:00).

## What runs

| Trigger | Budget per target | Corpus |
|---|---|---|
| `pull_request` | 60 s (smoke) | restored from cache, saved back (PR-scoped cache) |
| `schedule` weekly, Monday 05:17 CEST (+02:00) / 04:17 CET (+01:00) | 600 s | restored + saved (default-branch cache, which PRs can read) |
| `workflow_dispatch` | input `secs`, 1..1800 | same |

Targets (one matrix row each, jobs run in parallel, `fail-fast: false`):

| `target` | package | test binary | test fn | what it fuzzes |
|---|---|---|---|---|
| `http` | `galion-common` | `http_fuzz` | `fuzz_http_timed` | `http::parse_head` (two limit sets), `parse_response_head`, `dechunk`, `base64::decode` |
| `guard` | `galion-guard` | `fuzz_parser` | `fuzz_parser_timed` | sshd filter (22 fail2ban rules), session tracker, jail engine (dry-run nft), `split_stamp`, `parse_rfc3339`, `parse_journal_json` |

These are the only fuzz targets on integration-4 42de672. Both are std-only
seeded xorshift mutation fuzzers living as ordinary integration tests: stable
toolchain, no nightly, no `cargo-fuzz`, no libFuzzer, no new dependency.

Oracles (a violation panics, the job fails): HTTP: no panic, parsed head
length / header count within limits, no CR/LF/NUL in header values, no
`Transfer-Encoding`, prefix re-parse identical, errors map to status >= 400.
Guard: no panic, rule index in range, **every banned/failed address appears
literally in the line** (the parser never invents a victim), loopback is
never banned.

## Security posture of the workflow

* `permissions: contents: read` at the top, nothing wider anywhere; checkout
  with `persist-credentials: false`. No secrets are referenced.
* Every action pinned to a full commit SHA (tag in the comment):
  `actions/checkout@11d5960a…` v4.4.0, `dtolnay/rust-toolchain@b725eda5…`
  (stable head 2026-10-20), `actions/cache/restore` and `actions/cache/save
  @0057852b…` v4.3.0, `actions/upload-artifact@ea165f8d…` v4.6.2. The first,
  second and last are the pins already used by `realtest.yml`; the cache SHA
  was read with `git ls-remote https://github.com/actions/cache refs/tags/v4.3.0`
  (lightweight tag, no `^{}` peel) on 2026-10-21 17:2x +02:00.
* The dispatch input is validated (`^[0-9]+$`, 1..1800) before it reaches
  the environment; it is passed via `env:`, never spliced into the script.
* `cargo … --locked`, so a run never resolves new crate versions.
* `actionlint` 1.7.12 (MIT, release checksum verified) reports 0 findings
  (box only; the tool is not added to the repo).

## Corpus

`GALION_FUZZ_CORPUS=<dir>` (shared helper
`crates/galion-common/tests/fuzz_corpus/mod.rs`, included by path from the
guard test too):

1. every top-level file in `<dir>` is loaded (sorted, max 512 files, max
   8 KiB each) and **replayed verbatim first**, so old finds are re-checked
   and their outcome shapes count as seen;
2. during the run an input whose outcome signature is new is saved as
   `<dir>/<fnv1a64-hex>` (dedup by content hash, cap 512 files);
3. a panicking input is written to `<dir>/crashes/<target>-<hash>` before
   the panic continues; the `upload crash` step uploads it with the log.
   `crashes/` is not loaded as seeds (a fixed crash should become a unit
   test, as the guard find below did).

Cache key `fuzz-corpus-<target>-<run_id>` with restore prefix
`fuzz-corpus-<target>-`: each run restores the newest corpus and saves a new
entry (cache entries are immutable). Unset `GALION_FUZZ_CORPUS` = no file
I/O at all; unset `GALION_FUZZ_SECS` = the timed tests return at once, so a
plain `cargo test` is unchanged (the existing fixed-seed tests keep their
exact input sequence; `mutate` was only extracted into a function).

## Reproduce on a box (exactly what the job does)

```
export CARGO_TERM_COLOR=always TZ=Europe/Zagreb
export GALION_FUZZ_SECS=60            # 600 for the weekly budget
export GALION_FUZZ_CORPUS=$PWD/fuzz-corpus/guard
# optional: GALION_FUZZ_SEED=<seed printed by a failing run> to replay it
cargo test --release --locked -p galion-guard --test fuzz_parser --no-run
set -o pipefail
mkdir -p "$GALION_FUZZ_CORPUS"
cargo test --release --locked -p galion-guard --test fuzz_parser \
  -- --exact fuzz_parser_timed --nocapture 2>&1 | tee fuzz-guard.log
grep -E '^(fuzz seed|fuzz-summary|fuzz-crash)' fuzz-guard.log
```

(`http`: `-p galion-common --test http_fuzz -- --exact fuzz_http_timed`.)
Each run prints one line the job copies into the step summary:
`fuzz-summary target=… execs=… distinct=… corpus=… new=… crashes=0 secs=… rate=…/s seed=…`.
The guard fuzzer depends on the RNG *and* the engine/session state, so a
seed reproduces exactly only with the same corpus directory contents.

## How cortex-t52-fuzz c2f7cda joins once merged

`swarm/cortex-t52-fuzz` c2f7cda adds `crates/openmind-nerve/tests/fuzz_parsers.rs`
with `#[ignore] fn fuzz_all_summary()` (8 parser targets + 5 generators, one
table, `TOTAL execs …, failing N`) driven by its own env names:
`CORTEX_FUZZ_SECS` (time box **per target**), `CORTEX_FUZZ_SEED`,
`CORTEX_FUZZ_SEEDS=<dir>` (extra seed files, e.g. real VPS captures) and
`CORTEX_FUZZ_CRASH_DIR`. It is not on integration-4 42de672, so the matrix
does not list it yet. After it is merged, add one row and two env lines:

```yaml
          - target: cortex
            package: openmind-nerve
            test: fuzz_parsers
            filter: fuzz_all_summary
            ignored: "--ignored"   # the other rows set ignored: ""
```

and in the `fuzz` step use
`-- ${{ matrix.ignored }} --exact ${{ matrix.filter }} --nocapture` with, for
this row, `CORTEX_FUZZ_SECS` set from the budget **divided by 13** (it is
per target: 60 s × 13 would be 13 min on every PR), `CORTEX_FUZZ_SEEDS` and
`CORTEX_FUZZ_CRASH_DIR` pointing into `fuzz-corpus/cortex/`. Its corpus is
in-memory only (`MAX_CORPUS` 512), so the cached directory carries seeds and
crashes, not grown inputs; adopting `fuzz_corpus/mod.rs` there (or renaming
its env to `GALION_FUZZ_*`) is decision D-202-3. Its step-summary grep needs
`^(ss-ltn|proc-net-tcp|…|TOTAL)`; the simplest is to make it also print one
`fuzz-summary target=cortex …` line.

## Finds from the weekly-budget runs (fixed on this branch)

Three box runs at the 600 s budget hit the "invented address" oracle; the
fourth (fix in place) ran the full 600 s clean. Details and numbers:
`docs/real-tests/ci-fuzz-workflow.md`.

### Find 1 (parser, fixed in f042e5c)

Box run 2026-10-21 17:40:33-17:49:41 +02:00, 600 s budget, warm corpus (112
files), seed 1792597233379690538, failed after 548 s:
`invented address 0:1:2:3:4:5:6:7 from "… Failed publickey for invalid user … from ::1:2:3:4:5:6:7:81.00.0.1 port 4013"`.
The sshd filter's `<HOST>` took the valid prefix `::1:2:3:4:5:6:7` of one
longer invalid token because `:` counts as a token boundary (needed for
`1.2.3.4: 11:` tails). Fix (commit f042e5c, stricter only): an IPv6 match
followed by `:` and another hex digit, `:` or `.` is rejected.
Regression test `filter::tests::ipv6_run_with_colon_tail_is_not_truncated`
fails without the fix and passes with it. Impact is low: sshd itself writes
the address field (attacker-chosen usernames cannot place text there; the
existing `" from "` checks still stop user injection), so real logs never
contain such a token; fail2ban's stock regex has the same prefix behaviour.

### Find 2 (oracle gap, test-only fix)

Box run 2026-10-21 18:07:31 +02:00, seed 1792598853241971466, failed after
3.5 s: `… from ::1:2:3:4:5:6:7,:81.00.0.1 port 4013` gave 0:1:2:3:4:5:6:7.
The filter ends an address at `,` by design, and `::1:2:3:4:5:6:7` *is* in
the line; the oracle's token split did not know `,`. Fix (21b2496): the
oracle also splits on `,`. Parser unchanged.

### Find 3 (parser, fixed)

Box run 2026-10-21 18:51:10 +02:00, seed 1792601469964599518, failed after
0.1 s: `… from ::1:2:3:4:5:6:7:t1.rom …` gave 0:1:2:3:4:5:6:7. Find 1's fix
rejected `:` + hex/`:`/`.` only; a letter slipped through. Fix (37e9042,
stricter only): an IPv6 match followed by `:` must have whitespace or the
end of the line after that colon (`2001:db8::7: 11: Bye` still matches).
IPv4 is unchanged (an IPv4 prefix is always literally in the line, so it
never invents a victim). Tests `ipv6_run_with_colon_tail_is_not_truncated`
(unit) and `weekly_finds_replay` (fuzz_parser) fail on 21b2496's filter and
pass on 37e9042.

## Owner decisions (Slavik)

* D-202-1: keep 60 s per target on every PR (2 parallel jobs, ~2-3 min wall
  each incl. build) or run the smoke only on PRs touching `crates/galion-common`
  / `crates/galion-guard`.
* D-202-2: weekly finds open no issue automatically (the workflow has
  `contents: read` only); a failed scheduled run is the signal. Want an
  issue-opening step (needs `issues: write`)?
* D-202-3: cortex-t52-fuzz env names `CORTEX_FUZZ_*` vs `GALION_FUZZ_*` and
  its in-memory corpus (see above).
* D-202-4: IPv6 `:` tail strictness (find 3) is stricter than fail2ban's stock
  regex; accept it for galion-guard.
