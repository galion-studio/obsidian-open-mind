<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: hermes-core readiness re-check (wave 4, slot 130)

Branch `swarm/hermes-readiness-recheck`, based on `swarm/hermes-core` 45b7980.
All times are Europe/Zagreb (CEST, +02:00). The VPS was only read: GETs, `systemctl status/show`, `ss`, `journalctl`, `stat`.
No secret was read or printed. The hermes-rs env file (`/opt/galion/hermes/.env`) was not opened, and unit environment values were redacted, apart from the non-secret caps and the bind address.

## Result

**Hermes is still not ready: 11 of 15 checks pass with the shipped config** (`hermes readiness --repo .`, 2026-10-21 05:13:35 and again at 05:16:16 after the G11 wording fix, exit 3).
The real test with a throwaway HADES key gives 12 of 15.

| Gate | Result | Evidence (05:13-05:16) | Who can fix it |
|---|---|---|---|
| G1 | pass | 7 sectors; 34 of 52 Gaia systems covered (18 not covered) | Hermes |
| G2 | pass | hermes-core -> boss-node / ARTEMIS | Hermes |
| G3 | pass | 7 plans, 200 tasks, 0 cycles | - |
| G4 | pass | 200 parsed = 200 reported | Hermes |
| G5 | pass | dry run: 9 dispatch, 11 wait-cap, 5 wait-deps, 0 held, 1 refused; 15/15 slots; no idle sector | - |
| G6 | pass | cap 16 (1 reserved), measured limit 18-20 | Hermes |
| G7 | pass | 26 in scope: 76 dispatched, 51 request-limit stops, 51 retries, 25 done, 0 failed; ledger chain sound | Hermes |
| G8 | pass | ledger self-test ok (no live ledger given) | Hermes |
| **G9** | **fail** | no HADES config (`hades.config` empty). HADES is also not on the VPS: no hades unit, no hades binary in /opt/galion/rust/bin | Slavik (admin key custody) |
| G10 | pass | merge and deploy without a token: refused | Hermes |
| **G11** | **fail** | provider = mock; no paid model call is approved. See the VPS probe below | Slavik (H1, spend) |
| **G12** | **fail** | 8fd26f6 (integration-1), ccaab42 (hades-approval) and 9d3ec18 (gaia-land) are NOT ancestors of main 13a296c | Slavik (merge) |
| G13 | pass | 0.0.0.0 refused; loopback GET 200 | Hermes |
| **G14** | **fail** | 7 open decisions in `config/hermes/sectors.toml` (see "Stale decision items") | Slavik |
| G15 | pass | "Hermes is not ready, eleven of fifteen checks pass, and what blocks it is the HADES key, the paid model approval, the unmerged branches and your open decisions." | Hermes |

Against `/workspace/swarm/base.git`, G12 reads UNKNOWN, because base.git does not hold those three commits. The worktree does hold them, and there they show NOT merged. Both repos have main at 13a296ce.

## G11: OpenRouter on the VPS (read-only, 2026-10-21 05:08-05:14)

- `hermes-rs.service` is active (running) since 2026-10-18 22:00:49 CEST, at 127.0.0.1:8642, and `/health` returns 200 `{"status":"ok","implementation":"hermes-rs","implementation_version":"0.1.0"}`. Other paths need API_SERVER_KEY (401). No authenticated call was made.
- Caps (non-secret): session USD 1.00, daily USD 2.00, daily calls 500, MCP 0.
- Spend ledger `/var/lib/hermes-rs/rs-spend.json` was last written 2026-10-19 14:57:55 CEST: day 2026-10-19, **0.57 USD over 380 calls**. Nothing has been spent since. This means OpenRouter served paid calls on 10-19, and no paid call has run since then.
- `journalctl -u hermes-rs` has **0** lines mentioning openrouter and **0** lines with 402 since 2026-10-20 00:00, and 0 lines in total over the last 24 h. Across all units since 2026-10-18, openrouter appears only in old `hermes.service` (3), `memory-gateway.service` (8) and `user@0` (1) lines. These are memory-recall text and a worker command line, not provider errors. **No 402 credit error is in the VPS journal since 10-18.**
- Reachability without a key: `GET https://openrouter.ai/api/v1/models` returns 200 (786 kB, 0.05 s). `GET /api/v1/key` with no auth returns 401, as expected.
- **The credit balance itself is unknown.** It can only be read with the key (the free `GET /api/v1/key`), and reading the key is outside this slot's permissions.
- Conclusion: **the old "OpenRouter answers 402" claim is not backed by current evidence.** G11 fails because the provider is set to mock and no paid model call is approved (H1), not because of a measured 402. The G11 evidence text, the spoken short name and the test doc comment now say this ("the paid model approval" instead of "the model credit").

## Live nerve view (no cortex)

The cortex is not deployed. The nerve is the live view (`GET 127.0.0.1:8670/state`, ts 2026-10-21T05:13:35+02:00, mode live, cycle 6558):
- hermes-rs healthy (`dry_1h` 0, `live_1h` 0, no rollback).
- Healthy: openmind-memory, link-watch-rs, galion-desk, galion-one, console, galion-health, cloudflared, tuner-quick, lxc-host, galion-nerve.
- Degrading: tuner and galion-one-mouth.
- Absent: voice-worker and gaia.

## Stale decision items (for Slavik; not changed here)

`sectors.toml` still lists "Stray processes on the VPS: 8652/8653 and openmind-memory 8431-8433" as open. At 05:13, none of 8431, 8432, 8433, 8652 or 8653 is listening, because they were stopped in the 04:35 deploy run. The item "OpenRouter credit (402) and spend approval" should be reworded to a spend approval (H1), since no 402 is seen. Both are owner decisions, so `decided` flags were left untouched.

## Box tests (2026-10-21 05:14-05:16)

- `GALION_REAL=1 HERMES_REAL_PORT=20000 HERMES_REAL_PORT_REFUSED=20001 cargo test -p hermes-core --release --test real_hermes -- --nocapture`: **1 passed, 0 failed, 1.99 s.** Results:
  - R1: 7/47/200/645/407, matching the planners.
  - R3: cap 16 against limit 18 gave 0 stops, 25 done.
  - R4: cap 24 gave 2 stops and 2 retries, 25 done; the ledger has 85 lines and its chain is sound.
  - R4b: the harsh limit gave 42 request-limit stops, 2 stalls and 44 retries, 25 done and 0 failed.
  - R5: the merge was refused even with a signed token.
  - R6: deploy was held without a token, refused for the wrong scope, approved with a token, and refused on replay.
  - R7: the 10 endpoints returned 200 JSON, /nope 404, POST 405, and the 0.0.0.0:20001 bind was refused.
  - R8: 12/15 with the test key.
  - The ports were free afterwards, and no hermes process was left.
- The test's ports can now be set with `HERMES_REAL_PORT` / `HERMES_REAL_PORT_REFUSED` (defaults 19380/19381), so swarm slots stay inside their own port range.
- `cargo fmt --check` 0; `cargo clippy -p hermes-core --all-targets -D warnings` 0; `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`: **626 passed, 0 failed**; `scripts/no-python.sh` ok. No sandbox test ran (sandbox_vps_only).

No new dependencies.
