<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# real-test-port-env (wave-4 slot 363)

Follow-up to F-354-2: real tests that hard-coded ports outside a later slot's
range now read a port base from env through `galion_realtest::ports`
(`base_from_env`: unset = old default, bad value = panic naming the var,
never a silent fall-back). Defaults are unchanged, so earlier runs and docs
stay valid. Branch `swarm/real-test-port-env` off
`swarm/real-toolchain-aarch64-ports` 8555a8f7. Box only; VPS read-only.
All times Europe/Zagreb (CET +01:00).

## Env knobs

| Test | Env var | Default | Span | Layout |
|---|---|---|---|---|
| `openmind-nerve/tests/real_cross_target.rs` | `NERVE_XT_PORT_BASE` | 20870 | 5 | nerves +0..+2, live svc +3, dead +4; `NERVE_LISTEN_SCOPE` = block |
| same | `NERVE_XT_LIVE_PIN` (new) | 1 | - | `0` = report the aa77864 binary pin instead of asserting it (see F-363-2) |
| `gaia/tests/real_gaia_loop.rs` | `GAIA_LOOP_NERVE_PORT` | 20300 | 1 | nerve |
| same | `GAIA_LOOP_DUMMY_PORT_BASE` | 19600 | 10 | `--allow-ports` = block, dummies +1..+3; must stay inside `gaia_live::heal::TEST_PORTS` 19000-19999 (guard not widened) |
| `galion-health/tests/real_fixes.rs` | `GALION_HEALTH_REAL_PORT_BASE` | 20190 | 5 | health +0, upstream +1, black hole +2, raw +3, closed +4 |
| `hermes-bench/tests/real_hermes_bench.rs` | `HERMES_BENCH_REAL_PORT_BASE` | 19600 | 10 | bank +0, wallstreet +1, vending +2, mock LLM +5 |
| `hermes-bench/tests/real_banking_adv.rs` | `BANKING_ADV_REAL_PORT_BASE` | 20010 | 10 | bank +0, mock LLM +5 |
| `hermes-bench/tests/real_hermes_vending_hard.rs` | `VENDING_HARD_REAL_PORT_BASE` | 20035 | 5 | vending +0, mock LLM +1, unused bank URL +2 |
| `galion-wallstreet/tests/real_wallstreet.rs` | `WALLSTREET_REAL_PORT` | 19521 | 1 | broker |
| `galion-wallstreet/tests/real_adversarial.rs` | `WALLSTREET_ADV_REAL_PORT` | 20021 | 1 | broker |

`gaia/tests/heal_loop.rs` keeps its literal `127.0.0.1:20300`: it is a CLI
parse unit test and never binds.

## Real runs (GALION_REAL=1, slot ports 22330-22339, 2026-10-28)

| Time (+01:00) | Test | Base | Result |
|---|---|---|---|
| 05:51:07 | galion-health real_fixes (`--ignored`) | 22330 | 23 pass / 0 fail, threads 5, RSS 2176 kB |
| 05:51:37 | wallstreet real_adversarial | 22331 | broker on 127.0.0.1:22331, screen 212/212, order 212/212, `/adversarial` 100 % |
| 05:51:37 | wallstreet real_wallstreet | 22330 | broker on 127.0.0.1:22330, REAL-1 pnl 102305 fingerprint fnv1a64:a7856967de663bd5 twice, score 100 |
| 05:51:55 | hermes-bench real_hermes_bench (release) | 22330 | bank :22330, wallstreet :22331, vending :22332, mock LLM :22335; banking/wallstreet/vending 100 % scripted + mock-llm |
| ~05:52 | hermes-bench real_banking_adv (release) | 22330 | bank :22330, LLM :22335; gate refuse 38/38 allow 16/16, forbidden 0 (scripted + mock-llm); "ports 22330/22335 free again" |
| ~05:52 | hermes-bench real_hermes_vending_hard (release) | 22335 | vending :22335, LLM :22336, unused bank :22337; hard hash 21317eedd059a608, standard c4b11d2642a0b6be, 100 %; ports free again |
| 05:55:43 | nerve real_cross_target (`NERVE_XT_LIVE_PIN=0`) | 22330 | x86_64 :22330, aarch64 (qemu) :22331, live recipe :22332, first cycle 0.10/0.21/0.10 s; /health wire order equal on all 3; android + armv7 check ok; 2/2 pass |

Negatives (fail as intended):

- `GALION_HEALTH_REAL_PORT_BASE=70000` -> panic `port base "70000" is not a number 1024-65535`.
- `GAIA_LOOP_DUMMY_PORT_BASE=22330` -> panic `22330-22339 must lie inside the heal test range 19000-19999`.
- `GAIA_LOOP_NERVE_PORT=19605` with dummy base 19600 -> panic `overlaps the dummy block 19600-19609`.
- foreign listener (galion-wallstreet) on 127.0.0.1:22337, `VENDING_HARD_REAL_PORT_BASE=22335` -> `port 22337 busy before start`.
- nerve real_cross_target with the default pin (05:53:27) -> `live recipe no longer reproduces the VPS binary` (ccc551fb... 1930048 B vs pinned f0994b6d... 1213216 B), i.e. the pin is stale, not the port change (F-363-2).

## Not run: gaia real_gaia_loop

The supervised-loop test cannot run inside this slot's range: its dummy
services must sit in 19000-19999 (`gaia_live::heal::TEST_PORTS`, a safety
guard of the heal apply path) and slot 363 owns 22330-22339 only. The nerve
port is now movable and the dummy block is movable inside 19000-19999; the
guard was deliberately not widened (D-363-1). Only the negative checks above
ran for it.

## Findings

- **F-363-1** about 60 other `tests/real_*.rs` files still carry literal
  ports (heuristic scan; e.g. tunnel-lint real_tunnel_lint, nerve
  real_cortex_*, galion-one real_mouth_loading / real_one_*, bench-banking
  real_banking, console-static real_port*, console-ui real_port1). Same
  `ports::base_from_env` pattern applies; not in this row.
- **F-363-2** `real_cross_target` pins the aa77864-era build (recipe ==
  live SHA, solo != live, solo /health keys sorted). On 8555a8f7 serde_json
  preserve_order is unified: solo == recipe (ccc551fb..., 1930048 B) and all
  three emit insertion order. The pin can only pass on the aa77864 tree.
  `NERVE_XT_LIVE_PIN=0` reports instead and asserts the stronger "all 3 wire
  orders equal". Default still enforces the pin.
- **F-363-3** (pre-existing, not from this change) `gaia/tests/real_cli.rs`
  `real_gaia_commands` under GALION_REAL=1 expects `crates_total 63`, tree has
  68 (no crate added here).

## Owner decisions

- **D-363-1** gaia heal tests vs slot ranges: keep TEST_PORTS 19000-19999 and
  reserve a 19xxx block for heal real tests (recommended), or widen the guard
  (security change, not done).
- **D-363-2** re-pin `real_cross_target` to the live nerve build (current
  VPS sha) or retire the pin and keep `NERVE_XT_LIVE_PIN=0` semantics as
  default.

## VPS (read-only)

2026-10-28 05:53:34 +01:00: nerve `GET 127.0.0.1:8670/health` 200, cycle 55,
score 64, system degraded; 0 VPS listeners on any of the ports touched here.
Nothing changed on the VPS; nothing deployed.
