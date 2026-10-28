<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: console-rust-routes-a (slot 395, D-384-1 option A prep)

Console V2 prep, **not** the live cutover. Nothing deployed; live :8080
untouched. All times CET (+01:00, Europe/Zagreb), 2026-10-28.

## What changed

* Branch `swarm/console-rust-routes-a` off `console-realtests-rust-routes`
  ab0b7d04, merged with `swarm/console-deploy-candidate` 4f763ade (commit
  892b7f86) to get the `--local-api` Rust console that live runs.
* New opt-in flag `galion-console-static --owner-routes` (requires
  `--local-api`), plus `--hermes ADDR` (loopback). Without the flag the
  binary behaves exactly as today.
  * Owner rows of the slot-384 `console_routes` MAP are forwarded,
    exact path, GET/HEAD only (other methods 405), cookie and
    authorization dropped, query kept, after the API gate:
    `/api/v1/analytics`, `/api/v1/awake` -> galion-one (`--pulse`),
    `/api/hermes/health` -> hermes-rs `/health` (`--hermes`).
  * The 12 Retired rows answer explicit `410 {"error":"retired",...}`
    (D-384-2): bank, instances, laptop, keys, security, v3 guardian
    status/incidents, mesh, cloudflare, pack, hermes guard, hermes modes.
  * `/api/hermes/models` and `/api/hermes/history` stay 404 on purpose
    (D-384-3: they need the bearer key; not proxied).
* Real-test harness: `CONSOLE_REAL_API=local` (+ `CONSOLE_REAL_API_BIN`)
  starts a local console build instead of forwarding VPS :8080. galion-one,
  hermes-rs, desk and nerve stay the live VPS services via read-only
  `ssh -N -L` forwards (GET only). `decide_in` now treats a 410 on a
  non-retired row as an error.
* New `tests/owner_routes.rs` (6 tests, fake owners): forwarding, no
  credential leak, 410s, gate, behaviour without the flag, flag
  validation, list/MAP drift.

## Evidence (local release build b13164b3, 934,496 B, ports 22650-22659)

Mode A (`CONSOLE_LIVE_ROUTES=rust`, `CONSOLE_REAL_API=local`): **7/7 pass**

| test | time | result |
|---|---|---|
| real_port2 | 16:26:06-16:26:12 | ok |
| real_port3 | 16:26:24-16:26:28 | ok |
| real_port4 | 16:26:28-16:26:35 | ok |
| real_port5 | 16:26:35-16:26:43 | ok |
| real_route31 | 16:26:43-16:26:50 | ok |
| real_port1 (console-ui) | 16:26:50-16:27:03 | ok |
| real_console_ui (console-ui) | 16:27:03-16:27:09 | ok |

Live owner answers through the local build: `/api/v1/awake` 200,
`/api/v1/analytics` 401 (gate, unpaired host), `/api/hermes/health` 200,
all 12 retired rows 410, board routes (health, domains, services, repos,
markers, ledger, catalog, admin) 200. Nerve before/after: cycle 100, live,
14 components, hermes-rs/desk/galion-one/console healthy.

Controls:
* Mode B (`retire`) against the same local build, 16:27:30-16:28:18:
  3/7 pass (port2, port5, route31), 4 fail (port3 `/markers: no honest
  miss`, port4 `/memory: fail-closed gate 200`, port1 `/analytics`
  pairing, console_ui `/status` board). Expected: this build implements
  the routes, so retire-mode expectations do not match. Mode switch
  discriminates.
* Mode A against VPS :8080 (`CONSOLE_REAL_API=vps`): real_port2 and
  real_route31 pass (16:28:18-16:29:01); live unchanged.

Gates (16:29-16:33): `cargo fmt --check` ok; clippy workspace
(excl. openmind-sandbox, cgroup-detect) `-D warnings` ok;
`env -u GALION_REAL cargo test --workspace` (same exclusions) 2127 passed,
0 failed, 18 ignored; `scripts/no-python.sh` ok.

## Findings

* F-395-1: after the candidate merge `SPA_FIRST` is `[]` (slot 299 made
  `/arena` and `/bank` Rust-first); console-ui `fallbacks_on_this_base`
  updated to assert empty.
* F-395-2: `/api/arena` returns 404 locally because no `--arena` root is
  given (route is Gone, test passes). A cutover needs `--arena` set.
* F-395-3: `DATA_DEFAULT_PATHS` still lists `/api/v1/bank`; with
  `--data-upstream` set the data proxy answers before the 410 block.
  Decide before cutover whether bank stays retired (D-384-2) or proxied.
* F-395-4: the forward log line still says `->8080` for the slot that the
  local build replaces (cosmetic).
* Cutover (option A) still needs: deploy of this build with
  `--owner-routes --pulse 127.0.0.1:8091 --hermes 127.0.0.1:8642`,
  Slavik/Mate approval. Not done here.
