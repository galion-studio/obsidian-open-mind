<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Fuzzing and SAST

Written 2026-10-20T21:00:00+02:00 (Europe/Zagreb), branch `swarm/fuzz-sast`.
Answers the OpenSSF Scorecard **Fuzzing** and **SAST** gaps that
swarm/launch-gates listed (docs/launch/SCORECARD-READINESS.md there).

## What is fuzzed

`fuzz/` is a cargo-fuzz package with its **own workspace** (excluded from the
main one in the root `Cargo.toml`). It is built with nightly and
`-Zsanitizer=address` and only ever for fuzzing: nothing in `fuzz/` is part
of a release build, and release builds stay on stable.

| Target | Code under test | Checks beyond "no panic / no ASan error" |
|---|---|---|
| `http_parse_head` | `galion_common::http::parse_head` (the request parser behind every `galion_common::http::serve` server), `parse_response_head`, `dechunk` | byte 0 picks the limits; an accepted head respects `max_head_bytes` / `max_headers`, has a token method, printable target, trimmed values, no `Transfer-Encoding`; and re-serialising it parses back to the same head |
| `base64_decode` | `galion_common::base64::{encode, encode_url, decode}` | `decode(encode(x)) == x` for both alphabets; whatever `decode` accepts is canonical (re-encodes to the same text up to alphabet and padding) |
| `json_escape` | `galion_common::json::{string, string_ascii, float_repr}`, `galion_common::jsonout::to_json_compat` | escaped strings are valid JSON that decodes back to the input; `ensure_ascii` output is pure ASCII; `to_json_compat` agrees with `string_ascii`; documents round-trip to the same `Value`; `float_repr` round-trips every finite f64 bit for bit |
| `mdns_parse` | `openmind_discovery::dns::parse_message` (reads untrusted LAN multicast) | names are safe ASCII with a trailing dot and bounded length; rdata is well formed; re-encoding with `build_response` parses back to the same questions and records |

**hades-gate token parsing: skipped.** On main (13a296c) `hades-gate` has no
token parser: `decide(action, token)` is a string equality against a fixed
test token. The Ed25519 single-use token parsing lives only on the unmerged
`swarm/hades-approval` branch, so it is not reachable from main. Add a target
for it after that branch merges.

## Running it

```
scripts/fuzz.sh 600                       # every target 10 min, two at a time, nice -n 15
scripts/fuzz.sh 60 mdns_parse             # one target (bash wrapper; Rust port pending)
cd fuzz && cargo +nightly fuzz run -O json_escape corpus/json_escape seeds/json_escape
```

- `fuzz/seeds/<target>/` (committed): hand-written seeds plus every
  regression input. `fuzz/corpus/<target>/` grows locally and is git-ignored.
- Crashes land in `fuzz/artifacts/<target>/`. Reproduce with
  `cargo +nightly fuzz run -O <target> fuzz/artifacts/<target>/crash-...`.
- Needs a C++ compiler (libfuzzer-sys builds libFuzzer). On the swarm box,
  which has none, `CXX` points at a g++ unpacked without root, see
  docs/real-tests/fuzz-sast.md.

## CI

- `.github/workflows/fuzz.yml`: on PRs that touch the fuzzed crates or
  `fuzz/`, on push to main and weekly: a 4-job matrix, 60 s of
  `cargo fuzz run` per target on a dated nightly; crash inputs are uploaded.
- `.github/workflows/cflite-pr.yml` + `.clusterfuzzlite/`: ClusterFuzzLite
  code-change fuzzing on PRs (240 s). Scorecard's Fuzzing check does not
  recognise plain cargo-fuzz for Rust; it does recognise ClusterFuzzLite
  (`.clusterfuzzlite/Dockerfile`) and OSS-Fuzz.
- `.github/workflows/codeql.yml`: CodeQL for `rust` and `actions`
  (`build-mode: none`, `security-extended`). Scorecard's SAST check looks for
  `github/codeql-action`. Runs only while the repository is public (the
  CodeQL CLI is free for public open-source repos; private repos need paid
  GitHub Advanced Security), same gate as `scorecard.yml`.
- Every action is pinned by commit SHA; the CFLite base image by digest.

## Findings

1. **json_escape, first 20 s, harness assertion, not a product crash.**
   Input `fuzz/seeds/json_escape/regress-float-1ulp` (the literal
   `88880000009377031677888000000937703167700000000`): parsing
   `to_json_compat`'s output back with serde_json's default (fast, not
   correctly rounded) float parser gave a value 1 ulp away
   (`8.888000000937702e46` vs `...703e46`). Our writer was right. Fix: the
   fuzz crate enables serde_json `float_roundtrip`; the input is kept as a
   regression seed. Note for consumers: without `float_roundtrip`,
   serde_json can read long float literals 1 ulp off.
2. **jsonout float layout was not CPython's (found while triaging 1).**
   `to_json_compat` claims byte-for-byte CPython `json.dumps`, but floats
   went through serde_json's writer (1.0.154): `1.5e-7` and `0.00001`, where
   CPython writes `1.5e-07` and `1e-05` (large exponents already matched:
   `1e+16`). Fixed with `galion_common::json::float_repr`
   (CPython `repr(float)` rules: positional for decimal exponent -4 < d <= 16,
   else `De[+-]XX`), used by `PyFormatter::write_f64`/`write_f32`.
   Regression tests: `json::tests::float_repr_matches_cpython` (20 values),
   `jsonout::tests::cpython_floats`; the fuzz target checks the bit-exact
   round trip of every finite f64. Affects desk-hermes and link-watch output
   only for some floats below 1e-4 in magnitude (e.g. `1e-05`, `1.5e-07`;
   `1e-10` and all larger floats were already equal).
3. No crash, ASan report, timeout or OOM in the 11-minute runs of all four
   targets (numbers in docs/real-tests/fuzz-sast.md).

## Real-life replay (Rust)

`GALION_REAL=1 cargo test --release -p openmind-discovery --test real_fuzz_replay -- --nocapture`
throws the whole `http_parse_head` corpus at a release `galion-origin` and
the `mdns_parse` corpus at a release `openmind-discovery announce`, over real
sockets on 127.0.0.1:19700/19701 (replaces the old `scripts/real/fuzz-sast.sh`).
Evidence: docs/real-tests/fuzz-sast.md section 4.
