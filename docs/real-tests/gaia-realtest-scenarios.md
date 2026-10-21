<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Gaia scenarios in galion-realtest (slot 230, Gaia S8.1)

Base: `ci-realtest-with-nerve` 7a95e06 -> branch `swarm/gaia-realtest-scenarios`.
Recorded 2026-10-21 23:52-23:59 (Europe/Zagreb, +02:00). Box only; the VPS was not touched.

## Change

- `galion-realtest stack --with-gaia` (new, `src/gaia.rs`): three suites that drive the real
  release `gaia`, `gaia-dummy`, `hades-gate` and `openmind-queue` against the stack's own nerve
  and a supervised openmind-queue.
  - `gaia-heal-tick` (11 checks, needs `--with-nerve`): heal-plan sha256 == `hades-gate plan-hash`,
    deterministic; healthy tuner left alone (pid unchanged, nerve `service.probe` up); no gate
    config fails closed; down target + no token = dry-run plan only (exit 4 `no-token`, steps
    scope>probe>nerve-in>demeter>hades-plan>approval, no apply step, nerve `service.probe` warn +
    `gaia.heal.refused` no-token with the same probe id, target port still closed); scope guard
    refuses live port 8420, a port outside the allow range, `/usr/bin/systemctl`, a relative
    program; `restarts.jsonl` never written.
  - `gaia-hades-refusals` (14 checks, needs `--with-nerve`): real `hades-gate keygen/mint/check/
    disable/enable`; forged (attacker key), wrong target, wrong plan, wrong action, expired
    (iat -600 s, ttl 60), malformed, empty token file, replayed (admin's `hades-gate check`
    consumed it first), disabled gate (and the token is NOT burned: after `enable` the admin's
    check still approves it), admin-safety (`x;id` argument) - each exit 4 with its code and
    exactly one `gaia.heal.refused` with that code on the nerve; 0 `gaia.heal.approved/applied/
    failed`; hades ledger: 9 refused (one per gated refusal), 2 consumed (both by the admin's
    check, none by heal-tick), no raw token text; no token text in any gaia output or nerve
    event; target never started. Seeds and token files are deleted after the suite.
  - `gaia-poseidon` (8 checks): queue healthz; operator setup via `openmind-queue post`; `gaia
    poseidon status` (terraforming); submit + `job` live; spend cap exit 4 / HTTP 402 after the
    operator worker spent 30 of 50, cost 10 still accepted, `caps` spent 30; quota exit 4 /
    HTTP 429; vram approved 24 GB + plan-vram 8; unreachable queue exit 3.
- Plan-only by construction: no valid, unconsumed token ever reaches `gaia heal-tick`, so the
  restart (apply) path cannot run in CI.
- Stack: nerve gets `NERVE_EVENT_LOG` (fixture path) so `POST /event` / `GET /events` work;
  openmind-queue supervised on BASE+53 (`/healthz`); BASE+52 = never-bound heal target; port
  layout BASE..BASE+53 (was +51). `--build` also builds the nerve / Gaia packages when asked.
- Without `--with-gaia` (or with a binary missing) the suites SKIP with the reason; heal suites
  also SKIP without `--with-nerve` or when BASE+52 is outside Gaia's test ports 19000-19999.
- `.github/workflows/realtest.yml`: builds `-p gaia -p hades-gate -p openmind-queue`, runs
  `stack --with-nerve --with-gaia`. `uses:` pins unchanged.
- `tests/real_stack.rs` (GALION_REAL=1): adds `--with-gaia` when the four binaries exist and
  asserts each Gaia suite ran in full (no skip, >= 8 checks).

## Box runs (release binaries, CARGO_TARGET_DIR=/workspace/swarm/target/gaia-realtest-scenarios)

Ports checked free before each run (`ss -ltnH`); after every run nothing listened on them.

| Run | Ports | Checks | Pass | FAIL | Skip | Exit |
|---|---|---|---|---|---|---|
| before: base 7a95e06 binary, `stack --with-nerve` (CI command at base) | 19500-19551 | 81 | 81 | 0 | 0 | 0 |
| after: `stack --with-nerve` (no `--with-gaia`) | 19500-19553 | 84 | 81 | 0 | 3 | 0 |
| after: `stack --with-gaia` (no nerve) | 19500-19553 | 88 | 85 | 0 | 3 | 0 |
| **after: `stack --with-nerve --with-gaia` (new CI command) x3** | 19500-19553 | 115 each | 115 | 0 | 0 | 0, ~10.8 s each |
| after: same, `--base-port 19000` | 19000-19053 | 115 | 115 | 0 | 0 | 0 |
| after: same, `--base-port 21000` (heal target 21052 outside test ports) | 21000-21053 | 92 | 90 | 0 | 2 | 0 |
| negative control: `gaia` replaced by a stub that prints `restarted` and exits 0 | 19500-19553 | 115 | 89 | 26 | 0 | 1 |
| `GALION_REAL=1 cargo test --release -p galion-realtest --test real_stack` x2 | 19500-19553 | 115 (3 gaia suites in junit) | 115 | 0 | 0 | 1/1 ok, 11.5 s each |

Count: 81 -> 115 = +34 (ready:openmind-queue 1 + gaia-heal-tick 11 + gaia-hades-refusals 14 +
gaia-poseidon 8). Suites 13 -> 16. openmind-queue healthy 151 ms after supervisor start.

## Gates

- `cargo fmt --all --check` ok; `cargo clippy -p galion-realtest --all-targets -- -D warnings` ok.
- `cargo test -p galion-realtest`: 15/0 unit (+3: report parsing, nerve event match, heal-tick
  argv/flag order) + 1 real_stack (skips without GALION_REAL).
- `cargo check -p galion-realtest` for x86_64 and aarch64 musl ok. `scripts/no-python.sh` ok.
- No new dependencies (std + serde_json + zagreb-time, as before). Cargo.lock unchanged.

## Owner decisions

- D-230-1: the stack needs BASE..BASE+53 (54 ports); slot range 21000-21009 cannot hold it.
  Runs used 19500 (CI default), 19000 and 21000 bases, each checked free first.
- D-230-2: the heal target must be in Gaia's TEST_PORTS 19000-19999 on this base (slot 199's
  19000-20999 widening is not on 7a95e06), so `--with-gaia` heal suites skip for BASE > 19946.
  Option: merge gaia-fault-followups first and widen.
- D-230-3: CI never runs the approved restart path (plan-only by design). The approved
  restart + cap path stays covered by `crates/gaia/tests/real_gaia_live.rs` (box, slot 23
  ports). Option: add an opt-in `--gaia-apply` that restarts gaia-dummy once.
- D-230-4: hades-gate `check` returns `"decision":"WouldApply"` (dry-run gate) while it does
  consume the nonce; the suite accepts that. Owner may want the name to say it consumed.
