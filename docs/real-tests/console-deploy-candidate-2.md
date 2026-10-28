<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# console-deploy-candidate-2 (wave 4 slot 413) — next console candidate after 5aa31927

Branch `swarm/console-deploy-candidate-2`. **NOT the live cutover.** Nothing deployed, no VPS
change; the VPS was only read (systemctl show, GET through `ssh -N` forwards, `cat board.jsonl`).
Live at the time of testing (2026-10-28 ~18:42 CET +01:00, `systemctl show`): console.service
(:8080) and console-rust-primary (:18080) both `galion-console-serve-5aa31927`, active,
NRestarts 0, argv identical to `deploy/console-rust-serve/candidate/`.

## What is in it
- `0024d746` merge of console-rust-routes-a **820dfaea** (slot 395: `--owner-routes`,
  `--hermes`, 12 retired rows -> 410) onto console-deploy-candidate **4f763ade** (live 5aa31927
  + tests/drop-ins). Clean merge.
- **Code SHA `4e9e9e50`** — fixes:
  - **F-395-2** `--owner-routes` without an arena reader now **refuses at startup (exit 2)**:
    `--owner-routes needs --arena-root DIR (or --data-upstream with --data-path /api/arena):
    /api/arena would 404`. Chosen over a warning because `--owner-routes` is new (not live) and
    fail-closed is safer. `--arena-root` that is not a directory warns (not fatal). New startup
    line `galion-console-static: api owner_routes .. pulse .. hermes .. arena_root ..
    data_upstream .. data_paths ..` names every upstream.
  - **F-395-3** with `--owner-routes`, the **default** `--data-upstream` path list drops retired
    rows, so `/api/v1/bank` answers **410**, not the proxy. An explicit `--data-path
    /api/v1/bank` still forwards (that is the owner's opt-in; **D-384-2 stays the owner's**).
    Without `--owner-routes` behaviour is unchanged (bank still proxied by default).
  - **F-395-4** real-test forward log lines name the real upstream:
    `127.0.0.1:22831=local galion-console-static (CONSOLE_REAL_API=local)` instead of `->8080`
    (`galion_realtest::console_routes::api_forward_label`; VPS target keeps `->8080`).
    `local_api_args` now passes `--arena-root` (the harness's local API needs it after F-395-2).
- `84d89543`+ staged drop-ins + test + docs (no `crates/*/src` change), so the binary is named
  after **4e9e9e50**.

## Staged (NOT installed) drop-ins — owner decision D-413-1
`deploy/console-rust-serve/candidate-2/console.service.d/60-admin-store.conf` (:8080) and
`deploy/console-rust-serve/candidate-2/console-rust-primary.service.d/60-admin-store.conf`
(:18080): `/opt/galion/rust/bin/galion-console-serve-4e9e9e50`, **exactly the live argv +
`--owner-routes --hermes 127.0.0.1:8642`**, `--arena-root /workspace/arena` kept,
`ReadWritePaths` unchanged. No `--data-upstream` (bank stays 410, D-384-2). Rollback = put back
`candidate/` drop-ins (5aa31927), `daemon-reload`, restart. Pure test
`candidate2_dropins_are_live_plus_owner_routes` pins this (argv diff = 3 words, both units
agree, config parses, removing `--arena-root` errors).

## Tests (box, Zagreb CET +01:00)
- Debug `cargo test -p console-static -p console-ui -p galion-realtest` (before the new test
  file): 52 suites, 319 passed, 0 failed, 8 ignored. New unit/integration tests:
  `owner_routes_refuse_without_arena_reader`, `retired_bank_410_wins_over_default_data_proxy`,
  `forward_label_names_real_upstream`, `candidate2_dropins_are_live_plus_owner_routes`.
- **`real_deploy_candidate_2`** (GALION_REAL=1, release, ports 22830-22839):
  - 18:43:42-18:44:29 fake owners: A 9 board routes 200; owner rows pulse x3 + hermes x1
    forwarded with no Cookie/Authorization; POST 405; models/history 404; gate before owner;
    12 retired rows 410 incl. `/api/v1/bank`; `/api/arena` 300 B rows OK; lockout 5x401 then
    429. B: without `--arena-root` the binary exits 2 with the F-395-2 message. **2/2 pass.**
  - 18:44:35-18:48:18 (test 12.5 s) **live read-only leg** `CONSOLE_CANDIDATE2_LIVE=1`:
    `ssh -N` 22838->8091 (pulse), 22837->8642 (hermes), 22839->8646 (desk), 22835->8670 (nerve);
    live `board.jsonl` (12 lines) as arena root. `/api/v1/awake` 200 260 B == live owner;
    `/api/v1/analytics` 401 16 B == live owner (unpaired); `/api/hermes/health` 200 122 B ==
    hermes `/health`; `/api/arena` 200 4039 B ids 12/12; 12 retired 410; pages /console
    /services /hermes /arena /status 200 no JS; nerve `/state` 200 33694 B (cycle 5, console
    listed) before and after. Forwards closed, ports free. **pass.**
- **Mode A** (`CONSOLE_LIVE_ROUTES=rust CONSOLE_REAL_API=local`, ports 2283x) **7/7 pass**:
  real_port2 18:48:55-18:49:08, real_port3 -18:49:17, real_port4 -18:49:29, real_port5
  -18:49:43, real_route31 -18:49:56, real_port1 -18:50:15, real_console_ui -18:50:26. Local API
  started with `--owner-routes --arena-root <tmp>`; `/api/arena` Compare, `/api/v1/bank` live
  410 -> Gone.
- The older `real_deploy_candidate` GALION_REAL leg was not re-run (fixed ports 2213x belong to
  slot 343); its pure drop-in test runs in the workspace suite.

## Relation to main (GitHub compare, read-only)
main **7ba85b60** contains live **5aa31927** (behind 584). It does **not** contain 4f763ade
(1 ahead) nor 820dfaea (19 ahead / 100 behind, merge base 3f015986). main / main-staging are
not on the box, so no local merge-tree vs main was run; a PR of this branch into main needs a
merge from main first (both sides touch console-static).

## Gates and artifacts (box, 2026-10-28 ~18:50-18:55 CET +01:00)
- `cargo fmt --check` ok; `cargo clippy --workspace --all-targets --exclude openmind-sandbox
  --exclude cgroup-detect -- -D warnings` ok; `cargo test --workspace --exclude
  openmind-sandbox --exclude cgroup-detect --exclude hermes-tools` (GALION_REAL unset):
  394 suites, 2110 passed, 0 failed, 18 ignored; `scripts/no-python.sh` ok; SPDX on new files ok.
- Release from the 4e9e9e50 source tree (no `crates/*/src` change after it):

| target | binary | bytes | sha256 (first 16) |
|---|---|---|---|
| x86_64-unknown-linux-musl | galion-console-serve | 1041280 | acd7a5a7db5740b0 |
| x86_64-unknown-linux-musl | galion-console-static | 1041280 | 8b24db5e85fb28fc |
| aarch64-unknown-linux-musl (rust-lld) | galion-console-serve | 879088 | af7f64d6c34d22fa |

`cargo check` aarch64-linux-android, armv7-linux-androideabi, armv7-unknown-linux-musleabihf: ok.
Install (owner, D-413-1): build x86_64 musl from 4e9e9e50, copy to
`/opt/galion/rust/bin/galion-console-serve-4e9e9e50`, then :18080 first, then :8080.
