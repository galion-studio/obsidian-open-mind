<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: Hermes readiness re-run (2026-10-30, owner t445u)

Branch `swarm/hermes-readiness-core-20261030`, on `swarm/hermes-readiness-recheck` 7c7a613.
Times are Europe/Zagreb (CET, +01:00). VPS access was read-only (`ss -ltn`, `systemctl show`, GET /health, GET nerve /state).

## Result

`hermes readiness --repo <clone with main = 7ba85b60>`: **12 of 15 pass, not ready** (11:41 CET with the old config, 11:44 CET with this branch).
The console card still said 11/15: it is a constant written on 2026-10-20, not a live probe.

| Gate | Now | Why |
|---|---|---|
| G1-G8, G10, G13, G15 | pass | unchanged since 2026-10-21 |
| G12 | **pass (was fail)** | 8fd26f6, ccaab42, 9d3ec18 are all ancestors of main 7ba85b60 (PR #12, 2026-10-28) |
| G9 | fail | `[hades] config` is empty; HADES admin key custody is an owner decision |
| G11 | fail | provider = mock; no spend approved for the coordinator's own model calls |
| G14 | fail | 5 owner decisions open (was 7) |

## What changed on this branch

- `OwnerItem.closed` (optional): evidence that events settled an item. A closed item is no longer open but is never counted as decided; G14 lists it separately as "closed by events, not by Slavik".
- `sectors.toml`: closed "wave-1 merge order" (all three required merges are on main) and "stray processes" (nothing listens on 8652, 8653 or 8431-8433 at 11:41 CET). Reworded the "OpenRouter credit (402)" item to a spend approval: no 402 seen since 2026-10-18 and live hermes-rs answers on x-ai/grok-4.7 within its own caps.
- G11's name no longer hard-codes Opus/Sonnet (the planned models stay in the evidence from config).

## Tests (box)

- `cargo test -p hermes-core`: 31 + 6 + 5 + 1 passed, 0 failed (new test `owner_item_closed_by_events_is_not_open_and_not_decided`).
- `cargo clippy -p hermes-core --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- `GALION_REAL=1 HERMES_REAL_PORT=19930 HERMES_REAL_PORT_REFUSED=19931 cargo test --release -p hermes-core --test real_hermes`: 1 passed (R1-R8), ports free afterwards.

No new dependencies. Not deployed: hermes-core has no VPS unit.
