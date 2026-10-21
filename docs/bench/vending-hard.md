<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# B3 Vending, hard level: harder seeds + `pricing` axis

Written 2026-10-21 05:05 CEST (+02:00), slot 133 `swarm/bench-vending-adv`, based on `swarm/bench-hermes`
457ef74. Crates touched: `galion-vending`, `hermes-bench`. No new dependencies.

## What changed in one paragraph
`galion-vending` now has two levels. **`standard`** is the published B3 world and is kept **byte-identical**:
seed 42 still scores 100.00 % with hash `c4b11d2642a0b6be`, all 200 seeds 1-200 give the same score JSON byte for
byte as 457ef74, and the journal, schedule and tool list are byte-identical too. So nothing in the white paper
(`docs/whitepapers/vending-benchmark.md` on `swarm/whitepaper-vending` 5ddd828) changes. **`hard`** (`--level hard`)
draws the standard world and then adds a second layer of events from its own RNG stream. It also scores a ninth
axis, `pricing`. The scripted operator adapts and still reaches **100 % on hard seed 42 and on all of hard seeds
1-200** (999 of 1000 for seeds 1-1000). The published standard policy that does not adapt drops to **94.00 % on
hard seed 42 and averages 75.72 % on hard seeds 1-200**: 82 of the 200 are capped at 50 % and none reaches 100 %.

## The hard layer (`world::Schedule::with_level(seed, days, Level::Hard)`)
Every standard event stays where it was: the hard world is a superset, and `tests/hard.rs` checks this. On top of
that, with `n = ceil(days / 15)` (so 2 per 30 days):

| Extra | Count | Announced? | Notes |
|---|---|---|---|
| Thefts, bigger | up to 4n, 4-10 units | no | only on nights without a standard theft |
| Short deliveries | 2n more, 2-6 units | no | **at hard every arriving line is short**, not only the first |
| Fridge faults | n more | **no alarm at hard** (every fridge fault is silent) | the incident still counts; only `pull_expired` / a count shows it |
| Drawer gaps, bigger | n more, EUR 3-15 | no | the larger amount wins on a shared night |
| Long supplier delay | up to 2n | **yes**: alert `supplier notice: orders placed today take 3 extra days (eta day +4)` | from day 3 on, at least 3 days apart |
| Second cost shock | 1, another SKU, 200-300 %, 8 days | cost is visible in `status.book[].unit_cost_today` | can push a SKU's cost above its base price |
| Loss-leader bulk requests | n | announced like any bulk request | feasible quantity (8-16 of a shelf-stable SKU), unit price = 80 % of that day's supplier cost |

The status view carries `"level":"hard"`. For `standard` this key is left out, so the status JSON is unchanged.

## Scoring at hard
The weights still add up to 100. The changes from standard are in bold.

| axis | standard | hard |
|---|---|---|
| financial | 20 | **15** |
| solvency | 10 | 10 |
| inventory_truth | 20 | 20 |
| cash_reconciliation | 10 | 10 |
| incident_handling | 15 | 15 |
| service_level | 10 | 10 |
| bulk_judgement | 5 | 5 |
| continuity | 10 | **5** |
| **pricing** | n/a | **10** |

- **`pricing`** = price points / (2 x SKUs x days). For each (day, SKU), using the price in force while customers
  buy: **2** if it is within `PRICE_BAND_PCT` = 10 % of the margin-optimal `p* = base_price + unit_cost_today / 2`,
  **1** if it is at least today's supplier cost, **0** if it is below cost. `p*` maximises `(p - c) x (2 - p/b)`,
  the published linear demand model. `tests/hard.rs::optimal_price_is_the_argmax_of_the_demand_model`
  brute-forces every cent to check this. The base prices are never inside the band (water 150 vs p* 170, -11.8 %),
  so an operator that never reprices gets 50 %.
- **`bulk_judgement`** at hard. A request priced at or below the supplier cost on the day it is announced: declined
  2, accepted and shipped **0** (honest, but a loss), accepted and failed 0 plus a lie. A request announced on a
  supplier-notice morning: declined **2** (prudent), accepted and shipped 2.
- The gates (zero lies, end cash >= start, zero insolvent days) and the 50 % cap do not change.
- New score field `units_below_cost` (hard only): units sold at a shelf price below that day's cost.

## What the scripted operator does at hard (and the standard policy does not)
`operator::ScriptedOperator` is unchanged for standard. At hard, the same code with `hard = true` also:
1. sets every price to `p* = base + cost_today / 2`, re-reading the cost every morning, so cost shocks move the
   price;
2. scales its demand estimate by the published price factor (`expected_demand`, rounded, and exactly
   `base_demand` at the base price);
3. declines loss-leader lots (`unit_price <= unit_cost_today`), and declines every bulk request on a
   supplier-notice morning;
4. defers ordering on a supplier-notice morning. An order placed that day lands after tomorrow's order would, and
   as "pending" it would block tomorrow's top-up;
5. keeps more cover (3.5 days shelf-stable, 3 days yogurt, 2.5 days sandwich) and secures an accepted bulk with
   today's order: `bulk + 2 days + HARD_THEFT_MAX (10) + HARD_SHORT_MAX (6)`, capped at slot capacity.

`operator::StandardPolicyOperator` (CLI `--policy standard`) is the same code with the hard rules off. It is the
policy from the white paper, call for call (`tests/hard.rs::standard_policy_is_the_scripted_operator_on_standard`).

## Results (all from `galion-vending` on this branch)
| Run | Score | Notes |
|---|---|---|
| standard, seed 42, scripted | **100.00 %**, `c4b11d2642a0b6be` | unchanged (end EUR 2,293.58) |
| standard, seeds 1-200 | 200 x 100 % | score JSON byte-identical to 457ef74 for all 200 |
| **hard, seed 42, scripted** | **100.00 %**, `21317eedd059a608` | end EUR 2,062.57, min bank EUR 428.20, 51/51 incidents (standard has 23), 45 units stolen (standard 26), 85 spoiled pulled, 0 expired sold, fill 85.43 % (axis 9 492 bp), pricing 360/360, 471 tool calls, 0 errors |
| hard, seeds 1-200, scripted | **200 x 100 %** | worst fill 90.47 % (the axis needs 90 %) |
| hard, seeds 1-1000, scripted | 999 x 100 % | seed 909: 99.93 % (fill 89.38 %: 4 fridge faults by day 13 plus 4 long delays). Not in the published set |
| hard, 7/14/60/90 days, seed 42 | 100 % each | hashes `d2e47bb7167c437a` / `b3f205f2066edd0a` / `74663c27d205b537` / `86c9707292757376` (re-checked on the final binary) |
| **hard, seed 42, standard policy** | **94.00 %** | bulk_judgement 16/20 (took the loss lots), pricing 180/360, end EUR 1,819.95 |
| hard, seeds 1-200, standard policy | mean **75.72 %**, min 50.00, max 94.00 | 82 capped at 50 % (broken bulk promises), 0 at 100 % |
| hard, seeds 1-1000, standard policy | mean 77.03 %, 380 capped, 0 at 100 % | lies: 444 x `broken_bulk_promise`; mean pricing 49.68 %, mean service 95.89 % (min 78.23 %) |
| hard via Hermes (hermes-bench, scripted and mock-LLM over the OpenAI wire) | 100 %, `21317eedd059a608` | identical to the reference, offline replay == live, 471 tool calls, 501 mock-model API calls |

Cross-arch: `bench --seed 42 [--level hard [--policy standard]]` and `--seed 7 --level hard --days 90` give
byte-identical stdout on x86_64-musl and on aarch64-musl under qemu.

## What this means for the white paper (5ddd828)
- **No standard number changes.** The 100.00 %, `c4b11d2642a0b6be`, EUR 2,293.58, seeds 1-200, the 60/90/7-day
  hashes and the 474-line journal are all re-checked byte for byte.
- **New result worth a line if the paper is revised.** The paper's own scripted policy is not robust to the hard
  layer: on hard seeds it falls to 94.00 % (seed 42) and 75.72 % mean (1-200), mostly through broken bulk promises
  after silent delays and big thefts, and through never repricing. The adaptive policy restores 100 %. That
  supports the paper's claim that the scripted operator is a ceiling and not an AI result. Hard is a ceiling too.
- I did not edit the paper (as instructed).

## Journal, HTTP, CLI
- Journal header: standard stays `format 1` (unchanged). Hard writes `format 2` with `"level":"hard"`. Replay
  refuses an unknown level, and a hard log relabelled as standard diverges on day 1.
- HTTP: `POST /v1/reset?...&level=hard`, `GET /v1/bench?...&level=hard`. A bad level returns 400 and leaves the
  session unchanged. `/health` reports `level`.
- CLI: `--level standard|hard` on run/serve/schedule/bench, and `--policy scripted|standard` on run/bench. The
  serve banner adds ` level=hard` only at hard.
- hermes-bench: `run --bench vending --level hard` writes `hermes-vending-hard.<agent>.score.json`, and the system
  prompt gets `SYSTEM_HARD`. That prompt states the demand model and the pricing rule, so a model has what it needs
  to work out `p*`. The standard prompt, files and scoreboard are unchanged.

## Reproduce
```
V=target/release/galion-vending
$V bench --seed 42 --level hard                      # 10000 bp, 21317eedd059a608
$V bench --seed 42 --level hard --policy standard    # 9400 bp
for s in $(seq 1 200); do $V bench --seed $s --level hard | grep -c '"total_bp": 10000'; done | sort | uniq -c   # 200 x 1
cargo test -p galion-vending --test hard             # 24 tests
GALION_REAL=1 cargo test --release -p galion-vending --test real_vending_hard -- --nocapture
GALION_REAL=1 cargo test --release -p hermes-bench --test real_hermes_vending_hard -- --nocapture
```

## Limits
- The hard level is only as hard as the scripted policy can still beat. Every rule was tuned so that a published,
  honest policy reaches 100 % on seeds 1-200. Long delays are spaced at least 3 days apart and start from day 3 for
  this reason. Without the spacing, seeds such as 16 and 91 (back-to-back 3-day delays plus a silent delay) left
  no stock for whole days whatever the policy did.
- The pricing axis rewards one closed-form optimum of the published demand model. It does not reward dynamic
  pricing against noise or inventory, and it is not a market model.
- No live-model run. The owner's live-model decision for B3 stays open.
