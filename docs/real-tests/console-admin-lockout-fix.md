<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# console-admin-lockout-fix (wave-4 slot 323) — F-313-1 fixed

Branch `swarm/console-admin-lockout-fix` off `swarm/console-admin-flags-stage` 49b31b3f.
Times Europe/Zagreb (CET +01:00). NO deploy, no live change.

## Finding (slot 313, F-313-1)
With a read-only admin store (drop-in without `ReadWritePaths`), `pair()`
wrote the fail counter with `let _ = self.write(..)`: the write failed
silently, so wrong codes never locked (6x 401, no 429), and the right code
reached `issue()` whose write failed -> 503. 401 vs 503 = code oracle with
unlimited guesses.

## Fix (`crates/console-static/src/admin.rs`)
- Lockout state also kept in process memory (`Guard {fails, locked_until}`
  inside the existing mutex). Effective state = max(store, memory), so a
  read-only store, a store reset to `fails:0`, a removed/expired
  `lockedUntil`, or a non-numeric/negative `fails` never unlocks a running
  process. Persisted lock still survives a restart when the store is intact.
- Charge-before-compare: every code attempt first records fails+1 (and the
  lock at 5) in memory, then writes it to the store. If the write fails the
  answer is a uniform `503 {"error":"store not writable"}` BEFORE the code is
  compared -> right and wrong codes are byte-identical. A right code refunds
  (fails 0, lock cleared, memory reset) only when the new token is stored.
- Unchanged: unreadable/corrupt store -> 503 and never overwritten; paired
  device secret resumes during a lock and clears it (TS behaviour, 256-bit
  secret); 5 fails -> 15 min; after expiry the next wrong code relocks.
- Residual (owner D-323-2): the memory lock is per process; a restart on a
  read-only store forgets it (attacker can't restart the unit though, and the
  RO case now answers 503 for every code, so no guess is ever confirmed).

## Tests (box, 2026-10-27 22:55-22:58 CET)
- Unit `cargo test -p console-static --lib admin`: 15/15 (5 new:
  `f313_1_read_only_store_is_uniform_and_still_locks`,
  `reset_store_does_not_unlock`, `charge_before_compare_then_refund_on_success`,
  `expired_lock_relocks_on_next_wrong_but_right_code_still_pairs`,
  `read_only_store_device_resume_and_existing_cookie`).
- `GALION_REAL=1 cargo test --release -p console-static --test real_admin_lockout -- --nocapture`
  (release `galion-console-serve` with the staged 60-admin-store.conf argv, ports 21930-21934):
  ```
  A: 5x401 -> 429 (wrong+right, same body), lockedUntil persisted, tampered-clean store still 429
  A: restart on locked store 429; restart on clean store 401 then right 200, fails 0
  B: RO -> right/wrong identical 503 x5, then 429 for both; device 503; store unchanged; RW again still 429
  C: 8x6 parallel wrong -> 401 x5, 429 x43; right code 429
  D: locked 429 -> device resume 200 (no secret echoed) -> right code 200
  E: corrupt store -> 503 unreadable for right+wrong, file untouched
  real_admin_lockout: A-E pass in 389 ms
  ```
- `real_admin_flags_stage` (slot 313) B now ASSERTS the fix:
  `B: F-313-1 fixed: RO store 6 wrong -> [503, 503, 503, 503, 429, 429], right -> 429`; A-D pass 223 ms.
- Negative control (old admin.rs from 49b31b3f, same tests):
  real_admin_lockout FAILS `A: tampered-clean store stays locked` (left 200, right 429);
  real_admin_flags_stage FAILS `F-313-1 fixed` (left [401 x6], right [503,503,503,503,429,429]).
- Gates: fmt ok; clippy --workspace --all-targets -D warnings (excl openmind-sandbox, cgroup-detect) ok;
  cargo test --workspace (excl openmind-sandbox, cgroup-detect, hermes-tools) 143 suites 700 pass 0 fail 7 ign;
  no-python ok; x86_64 musl release 1024896 B (sha256 5fa6004e6771f336..), aarch64 musl 866712 B;
  check aarch64-linux-android, armv7-linux-androideabi, armv7-unknown-linux-musleabihf ok.

## VPS read-only (2026-10-27 22:59 CET)
`:8080 /api/v1/admin` (Host console.galion.app) 404, `/console` 503 (admin gate not
deployed, D-303-1/D-313-1 pending); nerve `:8670/state` 200, console `healthy`
(blind spot F-313-2); console.service + console-rust-primary.service active, no
drop-ins. Nothing changed on the VPS.

## Owner decisions
- D-323-1: ship this fix in the binary that D-313-1 deploys (rebuild 60-admin-store.conf
  ExecStart to the new SHA instead of 9c0f9d1a) — do NOT deploy 9c0f9d1a with `--admin-store`.
- D-323-2: accept per-process memory lock (restart on RO store forgets it) or require a writable
  store at startup (refuse `--admin-store` when its dir isn't writable).
- D-323-3: keep TS behaviour that the device secret bypasses and clears the lock.

No new dependencies.
