<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: hermes-offhost-fixes (wave-4 slot 314, ports 21840-21849)

Fixes for the slot 306 review of PR #14 (hermes-offhost continuity), on
`swarm/hermes-offhost-fixes` off the PR tip `1e0f427`. The PR branch itself is
untouched. Nothing is deployed.

| Finding (306) | Fix here |
|---|---|
| F1 no authenticity: whoever controls the store can plant cron jobs, sessions, spend on resume | Snapshots are sealed with HMAC-SHA256 (`offhost_guard::SealKey`, RFC 4231 vectors). `HERMES_OFFHOST_HMAC_FILE` is required once a path or URI is set (exit 2 otherwise; 0600, 32 B to 4 KiB, not in the store). Resume refuses tampered, unsigned and foreign-key snapshots before writing anything. |
| F2 raw SQLite shipped; filter knew only `sk-`/`ghp_`/`xoxb-` | Strict span scanner (github_pat_, gh*_ , sk_live/rk_live, or-v1-, glpat-, xox*-, xapp-, hf_, AIza, npm_, JWT, AKIA/ASIA, Bearer, Telegram bot token, PEM private keys), no word-boundary escape. Session text, spend and survival scrub use it. The DB leaves the host only as a sanitised copy: VACUUM INTO a private temp file, drop virtual tables, redact every TEXT/BLOB value with secure_delete on, VACUUM, final byte scan; any failure omits the DB (`unsanitizable`). |
| F3 one secret drops every session | Partly fixed: the DB is now redacted and kept, so all sessions survive disk loss. Over-cap behaviour is unchanged (owner decision D4). |
| F4 Bearer token over plain http | `http://` only for 127/8, ::1, localhost; anything else exits 2. |

## Command (2026-10-27 22:40 CET, +01:00)

```
export CARGO_TARGET_DIR=/workspace/swarm/target/hermes-offhost-fixes
cargo-slot build --release --target x86_64-unknown-linux-musl -p hermes-gateway -j 2
GALION_REAL=1 HERMES_GW_BASE_BIN=$CARGO_TARGET_DIR/base/x86_64-unknown-linux-musl/release/hermes-gateway \
  cargo-slot test -j 2 --release --target x86_64-unknown-linux-musl -p hermes-gateway \
  --test real_offhost_f1f2 -- --nocapture
```

Binaries (static x86_64 musl, release):

- fixed `hermes-gateway`: 2,079,472 B, sha256 `fa2c118c70a5698f475fbc0ea93342aa486e61788a5aae59f8656d622da166db`
- base (PR #14 tip 1e0f427, built from a detached worktree): 2,038,512 B, sha256 `803fc0e9dcc9702eb608c64d8e2da79afe66424273b43b2fd2a0f349dce24064`

The fake secrets (github_pat_, sk-, AKIA) are built at run time and never
printed. The store is an in-test HTTP server on 127.0.0.1:21842; the gateway
runs `--mock` on 21840, the standby on 21841.

## Output

`BASE` lines run the same leak and tamper steps against the PR tip; they must
reproduce the bugs (the test asserts that). Everything after runs the fixed binary.

    running 1 test
    BASE export rc=0 unsigned (no key needed) leaks=["github_pat_", "AKIA"]
      stdout: offhost beat role=primary local_ok=true path_ok=Some(true) uri_ok=None survival_ok=None sessions=0 bytes=57005 blockers=
    BASE http://192.0.2.10 (non-loopback) --check-config -> exit Some(0)
    BASE tampered store -> /api/jobs lists planted job: true
    C1 path, no key -> exit Some(2) 2026-10-27T22:40:19+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_HMAC_FILE is required when HERMES_OFFHOST_PATH or HERMES_OFFHOST_URI is set (unsig
    C1 uri, no key -> exit Some(2) 2026-10-27T22:40:19+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_HMAC_FILE is required when HERMES_OFFHOST_PATH or HERMES_OFFHOST_URI is set (unsig
    C1 key mode 0644 -> exit Some(2) 2026-10-27T22:40:19+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_HMAC_FILE must be mode 0600 or stricter
    C1 key inside store -> exit Some(2) 2026-10-27T22:40:19+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_HMAC_FILE must not live in the secondary store
    C1 http non-loopback -> exit Some(2) 2026-10-27T22:40:19+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_URI must be https (plain http only for loopback)
    C1 http loopback + key -> exit Some(0) 
    E1 --offhost-once rc=0 in 18 ms
      stdout: offhost beat role=primary local_ok=true path_ok=Some(true) uri_ok=Some(true) survival_ok=None sessions=0 bytes=56998 sealed=true blockers=
    E1 envelope kind="hermes-offhost-sealed" alg="hmac-sha256" key_id_len=16 bytes=56998 store_put_same=true
    E1 session_db included=true redacted_values=6 reason=null
    E1 leaks path=[] store_put=[]
    R1 log: 2026-10-27T22:40:19+01:00 hermes-gateway offhost resume role=primary loaded=true source=external_path auth=hmac-sha256 restored=spend,cron_jobs,session_db kept= blockers=
    R1 session t314-a -> 200 redacted=true has_fake_secret=false
    R1 session t314-b -> 200 redacted=true has_fake_secret=false
    R1 session t314-c -> 200 redacted=false has_fake_secret=false
    T1 payload edited, mac kept -> planted=false jobs_json_written=false session_a=404 log: 2026-10-27T22:40:19+01:00 hermes-gateway offhost resume role=primary loaded=false source=uri auth=none restored= kept= blockers=snapshot MAC mismatch (tampered or wrong key); local state was not overwritten
    T1 unsigned plain doc -> planted=false jobs_json_written=false session_a=404 log: 2026-10-27T22:40:20+01:00 hermes-gateway offhost resume role=primary loaded=false source=uri auth=none restored= kept= blockers=snapshot is not sealed; an HMAC key is set, so unsigned snapshots are refused; local state was not overwritten
    T1 resealed with attacker key -> planted=false jobs_json_written=false session_a=404 log: 2026-10-27T22:40:20+01:00 hermes-gateway offhost resume role=primary loaded=false source=uri auth=none restored= kept= blockers=snapshot was sealed with a different key; local state was not overwritten
    S1 standby genuine -> /health=200 loaded=true auth="hmac-sha256" jobs_json=true
    S1 standby tampered -> /health=200 loaded=false auth="none" jobs_json=false
    P1 db=1527808 B rc=0 375 ms store_body=2040264 leaks=[]
      stdout: offhost beat role=primary local_ok=true path_ok=None uri_ok=Some(true) survival_ok=None sessions=0 bytes=2040264 sealed=true blockers=
    test real_offhost_f1f2_fixed ... ok
    test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.23s

Reading it:

- BASE: an unsigned export carried `github_pat_` and `AKIA` off-host; plain
  http to 192.0.2.10 passed `--check-config`; a store-side edit of
  `cron_jobs` with a recomputed per-file sha256 planted job `t314evil`.
- C1: all five bad configs exit 2 with a clear reason; loopback http + key is 0.
- E1: sealed envelope (`key_id` 16 hex), identical bytes on path and store,
  `session_db` included with 6 values redacted, zero fake secrets in the raw
  envelope or in any decoded file body.
- R1: after deleting all local state the gateway resumes from the sealed
  snapshot (`auth=hmac-sha256`); all three sessions return 200, the secrets
  are `[REDACTED]`, the plain session is intact.
- T1: edited payload, unsigned document, and an attacker re-seal are all
  refused with `local state was not overwritten`; no job planted and no
  `cron/jobs.json` written.
- S1: standby loads the genuine snapshot (`auth=hmac-sha256`) and refuses the
  tampered one (`loaded=false`, nothing written).
- P1: a 1.5 MB real gateway DB with a secret: sealed export in 384 ms,
  2,040,264 B, no leak.

## Live VPS, read-only (2026-10-27 22:40 CET)

Over ssh, GET only:

- `GET 127.0.0.1:8642/health` -> `{"status":"ok","platform":"hermes-agent","version":"0.21.4","implementation":"hermes-rs","implementation_version":"0.1.0"}`: no offhost fields, so PR #14 / these fixes are not deployed. `/health/detailed` without auth -> 401.
- `systemctl is-active hermes-rs` -> active. Load average 15.6 / 17.8 / 20.0.
- Nerve `GET 127.0.0.1:8670/state`: up; recent alerts are inc-865945dd (galion-one-mouth stuck loading 4.2 h) and inc-06ffb32f (edge tunnel to closed ports). Nothing from hermes-rs. The cortex is not consulted here.

## Unit and gate numbers (code as committed)

- `cargo-slot test -j 2 -p hermes-rs -p hermes-gateway`: hermes-rs lib 58/58, golden 16/16, mcp 1/1, gateway real test 1/1 (skips without GALION_REAL). The 27 offhost + offhost_guard tests include RFC 4231, seal/open/tamper, key hygiene, strict patterns, SQLite freed-page and overflow-page redaction, and the F1/F2/F4 config and resume cases.
- `cargo-slot test -p hermes-rs --no-default-features --lib`: 54/54.
- `cargo fmt --all --check`: clean. `clippy -p hermes-rs -p hermes-gateway --all-targets -D warnings`: clean with default features and with `--no-default-features`.
- aarch64-unknown-linux-musl release build `--no-default-features` (rust-lld): ok. With default features it stops in `libsqlite3-sys` because this box has no aarch64 C cross compiler (same for the base tree; not caused by this change).
- armv7-linux-androideabi `check -p hermes-rs --no-default-features`: ok.
- `scripts/no-python.sh`: ok. `scripts/crate-python-scan.sh`: exits 1 with the same file list as the base tree (pre-existing).
- `repo-check --copyright "3000 Galion / galion-studio"`: PASS, 0 errors, 49 warnings (base 48); the one new review-list `utc` warning is the temp-file name in offhost_guard, which uses the wall clock only for uniqueness.

## Residual risks

- Rollback: the MAC does not stop someone replaying an older genuine sealed
  snapshot after disk loss. A monotonic counter kept off the store would.
- Not encrypted: sealed snapshots are readable by whoever holds the store.
  Secrets are redacted, but chat text still leaves the host (owner D2).
- The final raw byte scan may false-positive across adjacent SQLite values;
  the outcome is an omit (fail closed), not a leak.
- Triggers in the DB copy are kept; a WITHOUT ROWID table makes the DB
  `unsanitizable` (omitted).
- The beat summary prints `sessions=0` even with sessions (PR #14 behaviour,
  not changed here). F5-F8 from 306 are not addressed.
- Base64 of a 1.35 MB live DB is about 1.8 MB, close to the documented
  example `HERMES_OFFHOST_MAX_BYTES=2097152`; above it the DB is omitted.

## Owner decisions

- D1 where the store lives and who holds its token (306 D1, still open).
- D2 encryption at rest, or whether redacted session text may leave the host at all.
- D3 key provisioning and rotation: one key file on primary and standby, a
  place outside both the store and the repo, and a rotation step (today a
  key change makes old snapshots unreadable on purpose).
- D4 F3 remainder: raise the cap, or ship sessions-only when over cap.
