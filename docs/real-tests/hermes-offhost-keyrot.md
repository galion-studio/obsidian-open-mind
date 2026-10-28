<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real test: hermes-offhost-keyrot (wave-4 slot 340)

Branch `swarm/hermes-offhost-keyrot` off `swarm/hermes-offhost-fixes`
a885d473. 2026-10-28 CET (+01:00). Not deployed; PR #14 branch untouched.
Runbook: `docs/ops/hermes-offhost-keyrot.md`.

Test: `crates/hermes-gateway/tests/real_offhost_keyrot.rs`, gated by
`GALION_REAL=1`, ports 22100 (primary) and 22101 (standby), loopback only.
It starts the real release `hermes-gateway` (static x86_64 musl) with
`--mock`, `--check-config`, `--offhost-once` and `--offhost-standby`, and
reads the standby's real `GET /health`. Keys are throwaway random bytes made
at run time, mode 0400 (like a systemd credential), never printed. Key ids
shown below are public ids of those throwaway keys, already deleted.
`HERMES_GW_BASE_BIN` = a885d473 build (no window) for the before/after line.

Binaries (sha256): new `7a0982ea21c914e7...`, base a885d473
`fa2c118c70a5698f...`, test `d57fccea7a6ba546...`.

## Box run, 2026-10-28 02:35 CET (+01:00)

```
START 2026-10-28T02:35:29+01:00 exe=<box-target>/hermes-gateway
1 key A check-config: sealed=8cc053b1c3f57226 prev=no
1 export with A rc=0 store key_id=8cc053b1c3f57226
2 gate prev without until: exit 2 :: 2026-10-28T02:35:29+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_HMAC_PREV_UNTIL is required with HERMES_OFFHOST_HMAC_PREV_FILE (a rotation window must end)
2 gate until without prev: exit 2 :: 2026-10-28T02:35:29+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_HMAC_PREV_UNTIL is set but HERMES_OFFHOST_HMAC_PREV_FILE is not
2 gate until in UTC Z: exit 2 :: 2026-10-28T02:35:29+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_HMAC_PREV_UNTIL must be RFC 3339 in Europe/Zagreb time with +01:00 or +02:00 (no Z), for example 2026-11-04T12:00:00+01:00
2 gate until 31 days: exit 2 :: 2026-10-28T02:35:29+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_HMAC_PREV_UNTIL is more than 30 days ahead; keep rotation windows short
2 gate prev == current: exit 2 :: 2026-10-28T02:35:29+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_HMAC_PREV_FILE holds the same key as HERMES_OFFHOST_HMAC_FILE
2 gate prev without current: exit 2 :: 2026-10-28T02:35:29+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_HMAC_FILE is required when HERMES_OFFHOST_PATH or HERMES_OFFHOST_URI is set (unsigned off-host snapshots are refused)
2 gate window open: exit 0 :: sealed=f81fb01f87a052bc prev=8cc053b1c3f57226 prev_until=2026-10-28T03:35:29+01:00 prev_window=open
2 gate window closed: exit 0 :: sealed=f81fb01f87a052bc prev=8cc053b1c3f57226 prev_until=2026-10-28T02:34:29+01:00 prev_window=closed
2 gate prev mode 0640: exit 2 :: 2026-10-28T02:35:29+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_HMAC_PREV_FILE must be mode 0600 or stricter
BASE standby B (+ignored PREV vars) loaded=false auth="none" blockers=["snapshot was sealed with a different key; local state was not overwritten"]
3 standby B only: loaded=false auth="none" blockers=["snapshot was sealed with a different key; local state was not overwritten"]
4 standby B+prevA open: loaded=true auth="hmac-sha256+previous-key" restored=[] kept=["spend","cron_jobs","session_db"] state="standby"
5 primary export B (window open) rc=0 store key_id 8cc053b1c3f57226 -> f81fb01f87a052bc
6 standby B only after re-seal: loaded=true auth="hmac-sha256"
6 standby A only after re-seal: loaded=false blockers=["snapshot was sealed with a different key; local state was not overwritten"]
7 standby B+prevA closed: loaded=false blockers=["snapshot was sealed with the previous key 8cc053b1c3f57226 and its rotation window closed at 2026-10-28T02:34:29+01:00; local state was not overwritten"]
8 tampered A snapshot, window open: loaded=false blockers=["snapshot MAC mismatch (tampered or wrong key); local state was not overwritten"]
9 no key bytes (raw, hex, 16-byte prefix) in 144922 B of logs+store+restored files
END 2026-10-28T02:35:30+01:00 ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.28s
```

## VPS run, 2026-10-28 02:36 CET (+01:00)

Copied the three binaries to a throwaway `/var/tmp/keyrot-test-<ts>` (0700)
and ran the same test there with `TMPDIR` pointed at it (no sandbox code is
involved, so sandbox_vps_only does not apply; it was still run on the VPS per
no_skip_extensive_testing). Ports 22100-22109 were free before (0 listeners).

```
START 2026-10-28T02:36:24+01:00 exe=<vps-tmp>/gw-new
1 key A check-config: sealed=37f4ee9d46ba3a49 prev=no
1 export with A rc=0 store key_id=37f4ee9d46ba3a49
2 gate prev without until: exit 2 :: 2026-10-28T02:36:24+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_HMAC_PREV_UNTIL is required with HERMES_OFFHOST_HMAC_PREV_FILE (a rotation window must end)
2 gate until without prev: exit 2 :: 2026-10-28T02:36:24+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_HMAC_PREV_UNTIL is set but HERMES_OFFHOST_HMAC_PREV_FILE is not
2 gate until in UTC Z: exit 2 :: 2026-10-28T02:36:24+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_HMAC_PREV_UNTIL must be RFC 3339 in Europe/Zagreb time with +01:00 or +02:00 (no Z), for example 2026-11-04T12:00:00+01:00
2 gate until 31 days: exit 2 :: 2026-10-28T02:36:24+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_HMAC_PREV_UNTIL is more than 30 days ahead; keep rotation windows short
2 gate prev == current: exit 2 :: 2026-10-28T02:36:24+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_HMAC_PREV_FILE holds the same key as HERMES_OFFHOST_HMAC_FILE
2 gate prev without current: exit 2 :: 2026-10-28T02:36:24+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_HMAC_FILE is required when HERMES_OFFHOST_PATH or HERMES_OFFHOST_URI is set (unsigned off-host snapshots are refused)
2 gate window open: exit 0 :: sealed=501c382110c38c7e prev=37f4ee9d46ba3a49 prev_until=2026-10-28T03:36:24+01:00 prev_window=open
2 gate window closed: exit 0 :: sealed=501c382110c38c7e prev=37f4ee9d46ba3a49 prev_until=2026-10-28T02:35:24+01:00 prev_window=closed
2 gate prev mode 0640: exit 2 :: 2026-10-28T02:36:24+01:00 hermes-gateway offhost config error: HERMES_OFFHOST_HMAC_PREV_FILE must be mode 0600 or stricter
BASE standby B (+ignored PREV vars) loaded=false auth="none" blockers=["snapshot was sealed with a different key; local state was not overwritten"]
3 standby B only: loaded=false auth="none" blockers=["snapshot was sealed with a different key; local state was not overwritten"]
4 standby B+prevA open: loaded=true auth="hmac-sha256+previous-key" restored=[] kept=["spend","cron_jobs","session_db"] state="standby"
5 primary export B (window open) rc=0 store key_id 37f4ee9d46ba3a49 -> 501c382110c38c7e
6 standby B only after re-seal: loaded=true auth="hmac-sha256"
6 standby A only after re-seal: loaded=false blockers=["snapshot was sealed with a different key; local state was not overwritten"]
7 standby B+prevA closed: loaded=false blockers=["snapshot was sealed with the previous key 37f4ee9d46ba3a49 and its rotation window closed at 2026-10-28T02:35:24+01:00; local state was not overwritten"]
8 tampered A snapshot, window open: loaded=false blockers=["snapshot MAC mismatch (tampered or wrong key); local state was not overwritten"]
9 no key bytes (raw, hex, 16-byte prefix) in 145170 B of logs+store+restored files
END 2026-10-28T02:36:25+01:00 ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.23s
```

Staged drop-in check on the VPS (systemd 255): the staged `hermes-rs.service`
copy with `ExecStart=/bin/true` and the `80-offhost-hmac.conf` `[Service]`
lines merged in: `systemd-analyze verify` exit 0, no warnings (5 lines
mention `offhost-hmac`). Control: the same file with `LoadCredentail=` (typo)
gives `Unknown key name 'LoadCredentail' in section 'Service', ignoring.`,
so the parser really read the credential line. Not done: a real
`LoadCredential=` start, because that needs a (transient) unit on the
production VPS, which this slot may not create. The credential semantics the
gateway sees (regular file, mode 0400) are what the test uses.

After the run: folder removed, 0 leftover `t340-keyrot-*` dirs, 0 listeners
on 22100-22109, 0 `gw-new`/`gw-base` processes. Live `hermes-rs` unchanged:
MainPID 255539, NRestarts 0 before and after (active since 2026-10-27
19:46:18 CET). Nerve `127.0.0.1:8670/state` read before and after (cycle 42
both times). Cortex is not live, so the nerve is the observer.

## What the steps prove

| Step | Result |
|---|---|
| 1 | Primary seals with key A; store `key_id` = A. |
| 2 | 7 bad settings exit 2 with a named reason; open/closed windows print on `--check-config` with ids only. A 0640 previous key is refused, 0400 keys are accepted. |
| BASE | a885d473 ignores the new settings: a standby with key B cannot read A's store (the gap). |
| 3 | New binary, B only: same refusal, nothing written. |
| 4 | B + previous A, window open: standby restores A's snapshot (`auth=hmac-sha256+previous-key`, cron job present). |
| 5 | Primary with the window re-seals the store: `key_id` A -> B. |
| 6 | Window removed: B alone restores (`auth=hmac-sha256`); A alone is refused (old key retired). |
| 7 | Window closed: A-sealed store refused with the close time, nothing written. |
| 8 | Tampered A-sealed snapshot inside an open window: MAC mismatch, nothing written. |
| 9 | No key bytes (raw, hex, 16-byte prefix) in about 145 KB of logs, store and restored files. |

## Other checks (box)

- `cargo fmt --all --check` ok; `cargo clippy -p hermes-rs -p hermes-gateway
  --all-targets [--all-features] -D warnings` ok; `clippy -p hermes-bench -p
  hermes-tools --all-targets -D warnings` ok.
- `cargo test -p hermes-rs -p hermes-gateway --all-features`: hermes-rs lib
  63/0 (5 new: 3 guard, 2 offhost), golden 16/0, mcp 1/0, gateway real tests
  self-skip without `GALION_REAL`.
- aarch64-unknown-linux-musl release build of `hermes-gateway` (rust-lld) ok;
  armv7-unknown-linux-musleabihf check ok; armv7-linux-androideabi and
  aarch64-linux-android `check -p hermes-rs --no-default-features` ok. With
  default features the android check stops in `libsqlite3-sys` (no android C
  compiler on the box), same on base a885d473, not caused by this change.
- `scripts/no-python.sh` ok. No new dependencies.
