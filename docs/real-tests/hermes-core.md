<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: hermes-core

- Test: `crates/hermes-core/tests/real_hermes.rs`. It is gated: it runs only with `GALION_REAL=1` and otherwise returns at once.
- Command: `GALION_REAL=1 CARGO_TARGET_DIR=/workspace/swarm/target/hermes-core cargo test -j 3 -p hermes-core --release --test real_hermes -- --nocapture`
- Run: 2026-10-20T21:28:22+02:00 (Europe/Zagreb, CEST), on the box. **Result: 1 passed, 0 failed, 2.09 s.**
- Ports: 19380 (status API) and 19381 (bind refused on 0.0.0.0, nothing listened). The server was killed at the end of the test (Drop), and a GET afterwards failed as expected.
- Nothing under test was mocked:
  - the real `config/hermes/sectors.toml`;
  - the real plans in `/workspace/swarm/plans`;
  - the real `hermes` release binary;
  - the real hades-gate verifier and nonce ledger, with a throwaway admin key generated in the temp dir (not a custody key);
  - the real JSONL ledger, governor and loopback HTTP server.
- The only stand-in is the model provider, the offline mock, because OpenRouter answers 402 and no credit may be spent.
- No VPS access and no network beyond loopback.

## Results

**R1: the real plans load into the registry.**
- 7 sectors, 47 sessions, 200 tasks, 645 target agents and 407 useful. These equal every planner's reported numbers, per sector.
- 28 tasks are gated (17 deploy, 11 destructive).
- 34 of the 52 Gaia systems are covered.

**R2: dry-run dispatch of the first session of each sector** (cap 16, 1 reserved, 15 worker slots).
- 15 of 15 slots were used: 9 dispatch, 11 wait-cap, 5 wait-deps, 0 held, 1 refused (docs-whitepaper T1.3 "stepped ceiling test", classed destructive, with no HADES token).
- Every sector got work (round-robin): gaia S1.1, nerve T1.1, cortex T1.1 (4 slots), vps-ops S1.1, integration-ci S1.T1 (3 slots), security S0.1, docs-whitepaper T1.1, gaia S1.2 and nerve T1.2 (2 slots).
- The dry run wrote nothing to the ledger.

**R3: default cap 16 against a platform limit of 18.**
- 26 in scope, 25 dispatched, 25 done.
- **0 request-limit stops**, 0 failed, peak 15 slots, 1 waiting for Slavik (refused).
- Spoken: "The run finished with 25 tasks done, zero stops retried and zero failed, and one wait for Slavik."

**R4: cap 24 against a limit of 18.**
- 27 dispatched, 2 request-limit stops, 2 retries, 25 done, 0 failed, peak 18.
- Retry ledger: docs-whitepaper T1.2 and T1.5 each took 2 attempts (1 request-limit stop each) and ended done.
- `ledger --verify`: 85 lines, chain sound, Zagreb stamps, no torn tail.

**R4b: harsh limit of 8 with a stall on every 7th call.**
- 58 dispatched, 30 request-limit stops, 3 stalls, 33 retries, **25 done, 0 failed**.
- Spoken: "The run finished with 25 tasks done, 33 stops retried and zero failed, and one wait for Slavik."

**R5: HADES refuses an unapproved merge.**
- Operator intake "Merge swarm/integration-1 (8fd26f6) into main" with `--class code` was stored as `integration-ci:H1`, class **merge**, because the classifier wins.
- With no token, `gate` exits 3: `refused, code action, "action \"merge\" can never be approved"`.
- With a correctly scoped merge token **signed by the admin key**, `gate` still exits 3 with the same refusal. HADES has no merge kind.

**R6: the HADES deploy path** (vps-ops S4.1, the tuner 8650 binary swap).
- With no token: **held** (action switch, target `hermes.vps-ops.S4.1`, plan hash 7412cf6e…).
- With a token for S4.2: refused, `wrong-scope`.
- With a signed switch token for S4.1: **approved**.
- Replaying the same token: refused, `replayed`.

**R7: the status API on 127.0.0.1:19380.**
- `serve --host 0.0.0.0` exits 1 (loopback only).
- All of these returned 200 with valid JSON: `/health`, `/v1/sectors`, `/v1/agents`, `/v1/tasks`, `/v1/dispatch`, `/v1/ledger?tail=3`, `/v1/retries`, `/v1/governor`, `/v1/readiness` and `/v1/report`.
- `/nope` returned 404 and `POST /v1/dispatch` returned 405.
- `/v1/report`: "Hermes holds 200 tasks across seven sectors, zero running, 25 done and one waiting for you."

**R8: readiness report.**
- 12 of 15 pass with the test HADES key, so not ready (exit 3). G11 (provider), G12 (branches not merged into main) and G14 (7 open decisions) fail.
- With the shipped config (no HADES key), the result is 11 of 15 and G9 fails too. The run was `hermes readiness --repo <worktree>`, 2026-10-20 21:24 CEST.
- Spoken: "Hermes is not ready, twelve of fifteen checks pass, and what blocks it is the model credit, the unmerged branches and your open decisions."

The readiness self-test G7, on the real first sessions with the governor at 12 against a mock limit of 6, gave 81 dispatched, 56 request-limit stops, 56 retries, 25 completed and 0 failed.

## Processes

The test started one `hermes serve` (port 19380) and killed it, then checked that the port no longer answers. The other `hermes` invocations are one-shot commands. No process was left behind.
