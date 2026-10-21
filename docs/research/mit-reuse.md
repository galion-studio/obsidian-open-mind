<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# MIT reuse audit: in-house Rust code vs existing crates (slot 249, research-mit-reuse)

Written 2026-10-22 01:58 CEST (+02:00) on `swarm/research-mit-reuse`, base integration-5 `0d68f46`.
Doc only: no code changed, nothing built, no dependency added. This applies Mate's permanent rule t220
(2026-10-21 22:55 CEST): before writing Rust from scratch, look for good, lightweight, commercially
usable (MIT or MIT/Apache) code and reuse it.

Data sources (read 2026-10-22 01:55-01:57 CEST (+02:00)): crates.io API `/api/v1/crates/<name>` and
`/<name>/<ver>/dependencies` (latest version, licence, packed `.crate` size, last update, total
downloads, direct normal deps; `?` = optional), the GitHub API for last push and open issue counts, and the
in-repo source (line counts with `wc -l`). Swarm licence allowlist (RULES.md): MIT, Apache-2.0, MIT OR
Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0, 0BSD, CC0. Mate's rule prefers MIT or MIT/Apache.
No Hermes/OpenRouter call was needed: every figure below comes straight from the registry or the repo.

## Summary

| # | In-house code | Size | Best candidate | Verdict | Effort |
|---|---|---|---|---|---|
| 1 | HTTP/1.1 server + limits, `galion-common/src/http.rs` (used by 7 crates; 25 crates bind their own `TcpListener`) | 1,469 lines | `tiny_http` 0.12.0 (MIT OR Apache-2.0); `httparse` 1.10.1 (MIT OR Apache-2.0, no deps) for the parser only | **KEEP** the server. *Optional:* swap the head parser for `httparse` | parser swap ~1 day; full tiny_http move 3-5 days, not recommended |
| 2 | Loopback HTTP client (same file) + `curl` shell-out for HTTPS (hermes-rs, galion-one, galion-deploy, link-watch, openmind-nerve) | part of #1 | `ureq` 3.5.0 (MIT OR Apache-2.0), `minreq` 3.0.0 (ISC) | **KEEP** for loopback. **Revisit** curl only if a TLS crate gets approved | 2-3 days if TLS is ever wanted |
| 3 | Tar: `galion-backup/src/tarw.rs` (428) + `galion-deploy/src/tar.rs` (223); merged as `galion-tar` (1,246 lines) on `swarm/shared-tar-mergefix` 4eca4b7 | 651 -> 1,246 | `tar` 0.4.46 (MIT OR Apache-2.0) | **KEEP `galion-tar`** (byte-identical GNU 1.35 output is a hard requirement `tar` doesn't meet). Use `tar` only as a test oracle | n/a |
| 4 | SSE writer/parser (`hermes-rs/src/http.rs`, `provider.rs`) | ~60 lines | `eventsource-stream` 0.2.3 (async only), `sse-codec` 0.3.3 (**MPL-2.0, banned**) | **KEEP** (nothing sync, maintained and allowed) | n/a |
| 5 | JSONL append + size rotation (`openmind-nerve/src/writer.rs`; 18 crates open their own `.append(true)` logs) | 26 append sites | `serde-jsonlines` 0.7.0 (MIT), `file-rotate` 0.8.0 (MIT) | **KEEP** writer. *Optional cleanup:* one shared `JsonlAppender` in galion-common (in-house, not a crate) | ~1 day to consolidate |
| 6 | Cron: `hermes-rs/src/cron.rs` (488 lines; `interval` + `once` work, **cron expressions are stubbed, never fire**) | 488 | `croner` 4.0.2 (MIT), `cron` 0.17.0 (MIT OR Apache-2.0) | **ADOPT `croner`** for the cron-expression part only (fills a real gap) | ~1-2 days |
| 7 | CLI parsing: hand-rolled `std::env::args` loops in 36 crates | many small loops | `lexopt` 0.3.2 (MIT, no deps), `pico-args` 0.5.0 (MIT, no deps) | **ADOPT `lexopt`** for new binaries and when touching a CLI; no big-bang rewrite | ~1-2 h per binary |
| 8 | (bonus) SHA-256, base64, hex, Zagreb time in galion-common | 177 + 147 + 51 + 224 | `sha2` (already in Cargo.lock via hades-gate), `base64`, `hex`, `jiff`/`chrono-tz` | **KEEP** (std-only, tested vs chrono-tz; deps would add more than they remove) | n/a |

Net: two adoptions are worth doing (croner, lexopt). Everything else is either a hardening layer
the crates don't give us, or a hard compatibility requirement (GNU tar bytes, Zagreb time without a tz
database).

## 1. HTTP server and limits (galion-common `http.rs`)

**What we have.** 1,469 lines, std only. Thread-per-connection with a cap on live connections (503 on
overflow), total deadlines for head and body (stops slowloris), caps on head bytes/headers/body,
strict request-line and header parsing (no obs-fold, no control bytes, one numeric `Content-Length`),
`Transfer-Encoding` refused with 501 unless `accept_chunked` (then strictly decoded), one request per
connection. Fuzzed (`tests/http_fuzz.rs`, plus `swarm/ci-fuzz-workflow` 3a188b9: 134.6 M execs in
600 s, no crashes). Used by galion-boot, galion-origin, hermes-rs, one-talk, openmind-memory,
openmind-survival, tuner.

**Candidates.**

| Crate | Ver | Licence | .crate | Last release | Direct deps | Notes |
|---|---|---|---|---|---|---|
| [tiny_http](https://github.com/tiny-http/tiny-http) | 0.12.0 | MIT OR Apache-2.0 | 51 KB | 2022-10-06 (repo push 2024-08-14, 76 open issues) | ascii, chunked_transfer, httpdate, log (+ TLS optional) | Sync, thread pool, keep-alive. No per-connection total deadline or live-connection cap built in; would need a wrapper anyway. Release cadence is stale. |
| [httparse](https://github.com/seanmonstar/httparse) | 1.10.1 | MIT OR Apache-2.0 | 45 KB | 2025-03-03 | **none** | Zero-copy request/response head parser; the one under hyper, so it's very heavily used and fuzzed. Parser only: no I/O, no limits. |
| [rouille](https://github.com/tomaka/rouille) | 3.6.2 | MIT/Apache-2.0 | 87 KB | 2023-04-24 | 14 incl. chrono, rand, time, tiny_http, url | Too heavy. |
| [astra](https://github.com/ibraheemdev/astra) | 0.4.0 | MIT | 18 KB | 2024-11-07 | hyper, hyper-util, mio, bytes ... | Sync wrapper over hyper; pulls in the hyper tree. |
| [may_minihttp](https://github.com/Xudong-Huang/may_minihttp) | 0.1.11 | MIT/Apache-2.0 | 24 KB | 2024-09-22 | coroutine runtime `may` | Different runtime model; low adoption (33 k downloads). |

**Recommendation: KEEP.** None of the candidates gives the slowloris deadlines, connection cap and
strict smuggling rules out of the box. We would end up keeping most of `Limits` around them, and
tiny_http's last release is four years old. The one swap worth considering is **httparse for the
request-head parse step** (`read_request`): zero deps, MIT/Apache, battle-tested, about 150-250 of our
lines replaced. The caveats: httparse is more lenient in places than our parser (for example
`allow_obsolete_multiline_headers_in_responses` and spaces-before-colon configs), so our extra checks
(single CL, TE refusal, control bytes) must stay on top, and the existing fuzz corpus must pass
unchanged. Effort: about 1 day plus a fuzz run. Value: medium-low, since our parser is already fuzzed.
Owner choice **D-249-1**.

## 2. HTTP client (galion-common) and HTTPS via curl

**What we have.** A one-shot loopback client in `http.rs`. HTTPS (OpenRouter in hermes-rs, plus
galion-one, galion-deploy, link-watch, openmind-nerve) goes through the system `curl`, with the body and
key passed on stdin as curl config, never argv. That keeps TLS crates and C build deps out.

| Crate | Ver | Licence | .crate | Last release | Direct deps |
|---|---|---|---|---|---|
| [ureq](https://github.com/algesten/ureq) | 3.5.0 | MIT OR Apache-2.0 | 185 KB | 2026-10-21 | base64, log, percent-encoding, ureq-proto, utf8-zero (+ rustls etc. optional; default features include rustls) |
| [minreq](https://github.com/neonmoe/minreq) | 3.0.0 | ISC | 42 KB | 2026-06-15 | none by default; TLS optional (rustls or native-tls/openssl) |

**Recommendation: KEEP** both the loopback client and the curl path. A pure-Rust HTTPS client means
rustls, which brings `ring` or `aws-lc-rs` (asm/C, which conflicts with hard rule 1: no asm/SIMD). If Slavik ever
allows a TLS crate, `minreq` + rustls is the smallest swap for curl. Owner choice **D-249-2**, low
priority.

## 3. Tar (galion-backup, galion-deploy -> galion-tar)

**What we have.** On integration-5 there are two writers: `galion-backup/src/tarw.rs` (428 lines, GNU
format, byte-identical to GNU tar 1.35 `tar -c`: readdir order, hard-link detection, `././@LongLink`,
NSS uid/gid names, 10240-byte record padding) and `galion-deploy/src/tar.rs` (223 lines, POSIX ustar
writer + reader). Slot 242 merged them into the std-only `galion-tar` crate (1,246 lines, no deps, not
even libc) on `swarm/shared-tar-mergefix` 4eca4b7 (local; real_parity 26/26, gnu_oracle identical).

| Crate | Ver | Licence | .crate | Last release | Direct deps | Downloads |
|---|---|---|---|---|---|---|
| [tar](https://github.com/composefs/tar-rs) (was alexcrichton/tar-rs) | 0.4.46 | MIT OR Apache-2.0 | 71 KB | 2026-05-18 (push 2026-09-21; 160 open issues) | filetime, libc (+ xattr optional) | 262 M |

**Recommendation: KEEP `galion-tar`.** The `tar` crate is solid but doesn't promise byte-identical GNU
1.35 output (its `Builder` writes GNU or ustar headers its own way, with no readdir-order walk or LongLink
names matched to GNU), and the backup verifier depends on identical bytes. The deploy reader is small and
std-only. Two good uses of `tar` without shipping it: (a) as a **dev-dependency test oracle**
(read our archives back with `tar::Archive` in tests), next to the existing GNU tar oracle; (b) if a
future need goes beyond ustar/GNU (pax extended headers, sparse files), switch to `tar` then rather
than extending galion-tar. Owner choice **D-249-3** (add `tar` as a dev-dependency oracle: yes/no).

## 4. SSE / event stream

**What we have.** hermes-rs writes `text/event-stream` replies (`http.rs`: header block + `data:` frames,
30 s write timeout, `X-Accel-Buffering: no`) and parses upstream OpenAI-style SSE in `provider.rs`.
The nerve's event feed is `POST /event` + `GET /events?n=N` (JSONL-backed polling, not SSE).

| Crate | Ver | Licence | Last release | Notes |
|---|---|---|---|---|
| [eventsource-stream](https://github.com/jpopesculian/eventsource-stream) | 0.2.3 | MIT OR Apache-2.0 | 2022-02-17 | Parser over `futures::Stream`, so async only; stale |
| [sse-codec](https://github.com/goto-bus-stop/sse-codec) | 0.3.3 | **MPL-2.0** | 2026-02-12 | **Not allowed** (MPL-only) |

**Recommendation: KEEP.** SSE framing is a few dozen lines. The only maintained parsers are async or
MPL. If the nerve ever moves `/events` to push, reuse the hermes-rs writer (moved into galion-common)
rather than adding a crate.

## 5. JSONL logs

**What we have.** `openmind-nerve/src/writer.rs`: append-only JSONL, Zagreb `ts`, size-based rotation
that never rewrites bytes in place. Elsewhere, 26 `.append(true)` sites across 18 crates (bench-banking
audit, galion-vending journal, guardian-tick, hades-gate, hermes-rs sessions, openmind-queue ...)
each do their own open + `serde_json::to_writer` + `\n`.

| Crate | Ver | Licence | .crate | Last release | Direct deps |
|---|---|---|---|---|---|
| [serde-jsonlines](https://github.com/jwodder/serde-jsonlines) | 0.7.0 | MIT | 20 KB | 2025-01-14 | serde, serde_json (+ tokio/futures optional) |
| [file-rotate](https://github.com/kstrafe/file-rotate) | 0.8.0 | MIT | 22 KB | 2025-02-27 | chrono, **flate2** |
| [logroller](https://github.com/alasdairpan/logroller/) | 0.1.12 | MIT | 31 KB | 2026-07-01 | chrono, flate2, regex, thiserror (+ tracing-subscriber, xz2 optional) |

**Recommendation: KEEP, then consolidate in-house.** serde-jsonlines only saves the `to_writer + '\n'`
line (serde_json's `StreamDeserializer` already covers reading). The rotation crates pull in flate2/regex
and don't know about our Zagreb timestamps or the never-rewrite rule. Better: lift nerve's `Writer`
into `galion-common::jsonl` (behind the existing `serde-json` feature) and move the 26 ad-hoc sites over
gradually, which gives one place for fsync/rotation policy. About 1 day for the shared module, then
touch-as-you-go. Owner choice **D-249-4**.

## 6. Cron / timers (hermes-rs `cron.rs`)

**What we have.** 488 lines: reads upstream `cron/jobs.json` read-only; `interval` and `once`
schedules work; `no_agent` scripts confined, timed out, process-group killed, redacted.
**`cron` expressions are listed but never fired** (stub), which is a real functional gap compared with the
Python Hermes scheduler.

| Crate | Ver | Licence | .crate | Last release | Direct deps | Syntax |
|---|---|---|---|---|---|---|
| [croner](https://github.com/hexagon/croner-rust) | 4.0.2 | MIT | 83 KB | 2026-10-21 | derive_builder, strum (proc-macros), chrono? jiff? serde? | 5-field POSIX/Vixie (what jobs.json uses), optional seconds, `L`/`#`/`W`, DST-aware with chrono-tz |
| [cron](https://github.com/zslayton/cron) | 0.17.0 | MIT OR Apache-2.0 | 26 KB | 2026-06-18 | chrono, once_cell, phf, winnow | Quartz-style **6/7 fields with seconds first**; 5-field expressions need rewriting |
| [saffron](https://github.com/cloudflare/saffron) | 0.1.0 | non-standard | 2021 | - | **Not allowed** (non-SPDX licence), unmaintained |

**Recommendation: ADOPT `croner`** (MIT, actively maintained, released 2026-10-21) for the
expression-parsing and next-fire-time part only. Our scheduler loop, sandboxing and state stay in-house.
It reads standard 5-field expressions (the upstream Hermes format) as written, and with the `chrono` feature (chrono and chrono-tz are already
workspace deps) it computes next run times in Europe/Zagreb correctly across the Oct 25 03:00 switch.
Cost: derive_builder + strum add proc-macro build time, not runtime size. Check with
`cargo deny check licenses` (both MIT/Apache). The `cron` crate is the smaller fallback if proc-macros are
unwanted, but it needs a 5->6 field shim (prefix `0 `) and has different `dow` numbering
(Quartz-style 1-7 / SUN-SAT), which is a known source of off-by-one bugs. Effort: about 1-2 days including real tests
against a copy of jobs.json (read-only). Owner choice **D-249-5** (croner vs cron vs keep the stub).

## 7. CLI parsing

**What we have.** 36 crates walk `std::env::args` by hand (typical shape: `while let Some(a) =
args.next() { match a.as_str() { "--port" => ... } }`), with slightly different handling of `--x=y`,
`--`, missing values and non-UTF-8 paths.

| Crate | Ver | Licence | .crate | Last release | Direct deps | Downloads |
|---|---|---|---|---|---|---|
| [lexopt](https://github.com/blyxxyz/lexopt) | 0.3.2 | MIT | 27 KB | 2026-02-28 | **none** | 14 M |
| [pico-args](https://github.com/RazrFalcon/pico-args) | 0.5.0 | MIT | 12 KB | 2022-06-04 (push 2023-10) | **none** | 85 M |
| [argh](https://github.com/google/argh) | 0.1.19 | BSD-3-Clause | 32 KB | 2026-03-16 | argh_derive, argh_shared (proc-macro) | 17 M |
| [clap](https://github.com/clap-rs/clap) | 4.6.7 | MIT OR Apache-2.0 | 66 KB (+ clap_builder) | 2026-09-14 | clap_builder (+ derive) | 1.25 B |

**Recommendation: ADOPT `lexopt`** as the standard for new binaries and for any CLI we touch anyway.
It's MIT, has zero deps, is maintained (2026-02), handles `--x=y`, `-abc`, `--` and `OsString`
values correctly, and leaves help text to us, so it costs almost nothing in binary size (T3 target: <
2 MB per binary). pico-args is equally small but unreleased since 2022. clap is the right answer only
for a big user-facing CLI (galion-admin?), at noticeable size cost. No big-bang rewrite: about 1-2 h per
binary when it's touched. Owner choice **D-249-6**.

## 8. Bonus: hashes, encodings, time (galion-common)

| In-house | Lines | Crate alternative | Verdict |
|---|---|---|---|
| `sha256.rs` | 177 | [sha2](https://github.com/RustCrypto/hashes) 0.11.1 (MIT OR Apache-2.0; deps cfg-if, cpufeatures, digest). Already in Cargo.lock as 0.10.9 via hades-gate with `force-soft` | KEEP. In 0.11 `force-soft` became `--cfg sha2_backend="soft"` (RUSTFLAGS/.cargo config), and by default it runtime-selects SHA-NI/NEON, which conflicts with the no-SIMD rule. **Heads-up for hades-gate**: an upgrade from 0.10 to 0.11 would silently drop `force-soft`. |
| `base64.rs` | 147 | [base64](https://github.com/marshallpierce/rust-base64) 0.23.1 (MIT OR Apache-2.0, no deps) | KEEP (tiny, tested); acceptable swap if anyone wants it |
| `hex.rs` | 51 | [hex](https://github.com/KokaKiwi/rust-hex) 0.4.3 (MIT OR Apache-2.0, last release 2021) | KEEP |
| `time.rs` (Zagreb RFC 3339, no tz db) | 224 | [jiff](https://github.com/BurntSushi/jiff) 0.2.38 (Unlicense OR MIT; 678 KB crate; bundles tzdb optionally), chrono-tz (already used by `zagreb-time`) | KEEP. It's checked hour-by-hour against chrono-tz for 1996-2099 (`tests/time_vs_chrono.rs`), and jiff would be our heaviest dep for one fixed zone |

## Owner decisions (Slavik)

- **D-249-1** Swap galion-common's request-head parser for `httparse` (keep all limits/strict checks on top)? Recommended: optional, low priority.
- **D-249-2** Keep curl for HTTPS (recommended) vs approve a TLS crate (minreq + rustls; brings ring/aws-lc asm).
- **D-249-3** Add `tar` 0.4 as a dev-dependency test oracle for galion-tar? Recommended: yes (tests only, not shipped).
- **D-249-4** Consolidate the 26 JSONL append sites onto a shared `galion-common::jsonl` (in-house)? Recommended: yes, gradually.
- **D-249-5** Fill the hermes-rs cron-expression stub with `croner` 4 (MIT)? Recommended: yes.
- **D-249-6** Make `lexopt` 0.3 (MIT) the standard CLI parser for new and touched binaries? Recommended: yes.

Any adoption gets a `docs/third-party/<slug>.md` entry (name, version, SPDX, URL, purpose, why) and a
`cargo deny check licenses` run on its branch, per RULES.md.
