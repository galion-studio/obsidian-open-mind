<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# galion-common: third-party review (slot 16, branch `swarm/common`)

Reviewed 2026-10-20 19:45 +02:00 (Europe/Zagreb). Goal: minimal dependencies,
CPU-light, portable (x86_64/aarch64 musl, aarch64-android, armv7), and the
swarm rule "no asm, no SIMD, no target-cpu".

## Result: no new third-party dependency, no copied code

`galion-common` with default features depends on **nothing** (std only).
The optional `serde-json` feature (used only by desk-hermes and link-watch,
which already depended on them) uses the workspace `serde` / `serde_json`.
`Cargo.lock` gains no new external package. The dev-dependency `zagreb-time`
is our own crate (cross-check of the std-only time module).

Transitive licences with `--all-features` (all were already in the lock):
serde 1.0.229, serde_core 1.0.229, serde_derive 1.0.229, serde_json 1.0.154,
itoa 1.0.18, proc-macro2 1.0.107, quote 1.0.47, syn 3.0.9: `MIT OR Apache-2.0`;
memchr 2.8.3: `Unlicense OR MIT` (taken as MIT); unicode-ident 1.0.27:
`(MIT OR Apache-2.0) AND Unicode-3.0`; zmij 1.0.23: `MIT`. All allowed.

## Per-item decision

| Item | Candidate (licence, size) | Decision | Why |
|---|---|---|---|
| SHA-256 | `sha2` 0.11.1, MIT OR Apache-2.0, ~2.5k lines + `digest`, `block-buffer`, `crypto-common`, `hybrid-array`, `cpufeatures` (https://github.com/RustCrypto/hashes) | **Own** (`sha256.rs`, ~150 lines) | sha2 selects SHA-NI / ARMv8 SHA intrinsics at runtime through `cpufeatures`; forcing the portable backend needs the `force-soft` feature (0.10) or a `sha2_backend` cfg rustflag (0.11) in every build. That breaks the no-SIMD rule by default and adds 5 crates. We hash a few short tokens and names, never bulk data, so speed is irrelevant. The repo already had 3 verified copies; one streaming version replaces them, checked against the NIST FIPS 180-2 Appendix B vectors (incl. 1,000,000 x `a`) and 10 NIST additional-message vectors, every one also re-checked against coreutils `sha256sum` on the box. |
| base64 | `base64` 0.23.1, MIT OR Apache-2.0 (https://github.com/marshallpierce/rust-base64) | **Own** (`base64.rs`, ~90 lines) | We need encode (standard, padded) + encode (URL-safe, unpadded) + a strict decode. The crate's engine/config API is far larger than the need; no SIMD concern but no gain either. RFC 4648 section 10 vectors are in the tests. |
| HTTP parser | `httparse` 1.10.1, MIT OR Apache-2.0, zero deps, widely fuzzed (https://github.com/seanmonstar/httparse) | **Own** (`http.rs`) | httparse is the best candidate (tiny, no deps, `no_std`). But it uses SSE4.2/AVX2 on x86_64 and NEON on aarch64 with runtime detection; turning that off needs the `httparse_disable_simd` cfg rustflag in every build, which is easy to lose. It is also deliberately lenient where we want strict (it is the parser, not the policy: CL/TE conflicts, duplicate CL, limits and deadlines would still be ours). Our parser is ~250 lines, strict, and fuzz-tested (`tests/http_fuzz.rs`). Revisit if we ever serve the public internet directly. |
| HTTP server | `tiny_http` 0.12.0, MIT OR Apache-2.0 (https://github.com/tiny-http/tiny-http) | **Own** | tiny_http pulls `ascii`, `chunked_transfer`, `httpdate`, `log`, runs its own thread pool, and supports keep-alive/pipelining we explicitly do not want. Our needs (one request per connection, head deadline, size caps, bounded threads) are ~200 lines on std. |
| HTTP client | `ureq` / `minreq` (MIT OR Apache-2.0) | **Own** | Loopback-only plain HTTP; TLS goes through curl elsewhere. A one-shot client with deadline and size cap is ~150 lines. |
| JSON output | `serde_json` (already used) | **Reuse existing** | `jsonout` (CPython `json.dumps` formatter) moved into the crate behind feature `serde-json`; the std-only `json` module covers hand-built JSON. |
| Zagreb time | `chrono` + `chrono-tz` via `zagreb-time` (already used) | **Own std-only `time` module; zagreb-time kept** | `zagreb-time` stays for existing users. `galion_common::time` has the same API without chrono/chrono-tz (EU DST rule; identical output to chrono-tz for every switch 1996-2099 per `tests/time_vs_chrono.rs`). |

Nothing was copied from any of these projects.
