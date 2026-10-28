<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: hermes-gateway under 2 MiB (swarm/hermes-gateway-2mib, wave 4 slot 329)

| Field | Value |
|---|---|
| Written | 2026-10-28 00:50 +01:00 (Europe/Zagreb) |
| Base | main-staging 38a07700 (worktree commit 3f015986, tree 4864589f byte-identical) |
| Goal | release hermes-gateway x86_64 musl was 2,313,344 B (slot 312), over the 2 MiB budget; bring it to 2,097,152 B or less without losing tests or features |
| Result | **x86_64 musl 2,080,128 B** (-233,216 B, 17,024 B headroom); aarch64 musl 1,883,744 B |
| Deploy | none (no live service, permission or unit change) |

## 1. What changed

| Change | Where | x86_64 effect (solo build) |
|---|---|---|
| Baseline | | 2,313,184 |
| C code (SQLite) built without unwind tables: `-fno-asynchronous-unwind-tables -fno-unwind-tables` (`panic = "abort"`, C never unwinds) | `.cargo/config.toml` `CFLAGS_<musl triple>` | 2,222,656 |
| SQLite trims that change no used behaviour: OMIT DEPRECATED, SHARED_CACHE, PROGRESS_CALLBACK, DECLTYPE, TRACE, AUTHORIZATION, DESERIALIZE, UTF16, COMPLETE, GET_TABLE, TCL_VARIABLE, COMPILEOPTION_DIAGS, UNTESTABLE, DEFAULT_MEMSTATUS=0, -U ENABLE_MEMORY_MANAGEMENT | `.cargo/config.toml` `LIBSQLITE3_FLAGS` | 2,191,296 |
| `SQLITE_OMIT_JSON` (no SQL JSON functions in any crate or in any live schema, see section 3) | same | 2,151,584 |
| RELR relative relocations (`-zpack-relative-relocs`, rust-lld; musl >= 1.2.4 supports DT_RELR) .rela.dyn ~50 KB -> 720 B | `crates/hermes-gateway/build.rs` (x86_64 musl + lld only) | 2,096,352 |
| sha2 `force-soft-compact` (no SHA-NI/SIMD dispatch; hades-gate already uses force-soft) | `crates/hermes-rs/Cargo.toml` | **2,080,000** (solo) / **2,080,128** (workspace build) |

The 2.5 MiB hermes-gateway exception in `scripts/size-budget.sh` is removed: every binary is 2 MiB again.

Kept on purpose: FTS5 (openmind-memory l0_fts/l1_fts; legacy Python state.db), INCRBLOB (FTS5 needs it), date/time
functions (live vectors.db schema_migrations uses strftime() as DEFAULT), the `coding` feature (staged
hermes-rs.service.d/coding-tools.conf relies on it).

Tried and rejected: `--icf=all` (-18 KB, unsafe: SQLite compares function pointers; `--icf=safe` gave 0 B); a linker
script dropping .eh_frame (binary grew to 2.88 MB); dropping `coding` (2,005,824 B, feature loss); OMIT_DATETIME_FUNCS
(live schema); MAX_EXPR_DEPTH=0, LIKE_DOESNT_MATCH_BLOBS, -U API_ARMOR (behaviour changes). The std backtrace symbolizer
(~210 KB, gimli/addr2line) cannot be removed on stable Rust.

## 2. Gates (box, 2026-10-28 00:50 +01:00)

| Gate | Result |
|---|---|
| `scripts/size-budget.sh --check-dir` x86_64 musl release (workspace build, new 2 MiB-for-all script) | ok, 40 binaries, hermes-gateway 2,080,128 |
| same, aarch64 musl release | ok, 40 binaries, hermes-gateway 1,883,744 |
| Static check x86_64 hermes-gateway | no PT_INTERP, no DT_NEEDED, stripped, DT_RELR present (RELRSZ 720) |
| `GALION_REAL=1 HG2_BIN=<musl release> cargo test -p hermes-gateway --test real_size_2mib` | PASS (2026-10-28 00:36:42 +01:00) |
| same test against the host debug binary | PASS (00:36:49 +01:00) |
| `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect` | 385 suites, 2027 passed, 0 failed, 18 ignored |
| `GALION_REAL=1` openmind-memory / desk-hermes / galion-backup / one-defence (all link the SQLite flags) | 48/0, 13/0, 17/0, 13/0 |
| `GALION_REAL=1` hermes-gateway | 1 fail in real_enroll ("rejected B: claim refused" error text), **same failure on base 3f015986** (00:47:29 +01:00) |
| `GALION_REAL=1` hermes-rs | 66/1: heap_growth debug (123.5 B/chat vs limit 32), **same on base** (121.8 B/chat) |
| `GALION_REAL=1` hades-gate | 48/1: real_vps_inventory needs HADES_VPS_SSH (env not set; unrelated) |
| fmt --check, clippy --workspace --all-targets -D warnings, no-python.sh | ok |
| cargo check armv7-unknown-linux-musleabihf, aarch64-linux-android (zig cc) | ok |

## 3. Real HTTP test (`crates/hermes-gateway/tests/real_size_2mib.rs`)

Ports 21990 (production CurlProvider -> in-test SSE stub on 21992), 21991 (`--mock`). Loopback only, no paid model.
Builds a legacy Python-style state.db with an FTS5 table and trigger (MATCH returns 2 rows with the trimmed SQLite).
Output against the x86_64 musl release binary:

```
SIZE 2080128 B (budget 2097152 B, headroom 17024 B)
ELF stripped yes, .relr.dyn yes
gateway :21990 /health 200 after 50 ms
GET /v1/models -> 200
POST /v1/chat/completions -> 200, body has 'stub answer ok': true (X-Hermes-Session-Id set)
turn 2 same session -> 200; own session messages roles 4
GET /api/sessions/s-py/messages (legacy FTS5 db, read-only) -> 200 roles 2
GET /api/sessions -> own true, legacy true
restart: own session still 4 roles
GET /v1/models without key -> 401
--mock :21991 chat -> 200, mock-fail -> 502
stub upstream hits 2; legacy db size unchanged
PASS real_size_2mib
```

## 4. Live VPS read-only probes (no change made)

| Time (+01:00) | Probe | Result |
|---|---|---|
| 2026-10-27 evening | hermes-rs.service binary | /opt/galion/rust/bin/hermes-gateway-b7fbbc78 (2,732,248 B), :8642/health 200, no HERMES_RS_CODING_WORKTREE |
| 2026-10-27 evening | JSON in schemas of 902 SQLite files (read-only grep) | 0 hits -> OMIT_JSON safe |
| 2026-10-28 00:48:58 | text encoding (header offset 56) of /opt/galion/hermes/state.db, /var/lib/hermes-rs/rs-state.db | 1 (UTF-8) both -> OMIT_UTF16 safe |
| 2026-10-28 00:49 | encoding of 35 *.db/*.sqlite files under /var/lib and /opt/galion | 31 UTF-8; 4 are empty 1-page archived fixtures (encoding 0 = not yet set, defaults to UTF-8) |
| 2026-10-28 00:48:58 | nerve 127.0.0.1:8670/state, hermes :8642/health, hermes-rs NRestarts | 200, 200, 0 active |

## 5. Owner decisions (D-329)

- D-329-1: SQLite flags in `.cargo/config.toml` apply to every crate that links SQLite (openmind-memory, desk-hermes,
  galion-backup, one-defence, hermes-rs): no JSON1 and no UTF-16 APIs workspace-wide. All their tests pass.
- D-329-2: RELR only on x86_64 musl with an lld linker (build.rs); `.cargo/config.toml` now sets rust-lld for that target
  (CI and ucpu-gate already export it).
- D-329-3: size budget exception removed; future hermes-gateway growth must fit in 17 KB headroom or trim more
  (next candidate: dropping `coding`, -74 KB, owner call).
- Not deployed. A deploy would be a normal hermes-rs binary swap, separate decision.
