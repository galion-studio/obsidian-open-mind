# overnight-test-hermes (t311u first pass)

Checked: 2026-10-27 19:47:04 CET (Europe/Zagreb). Service `hermes-rs` / gateway `:8642`.

## Reachable
| check | result |
|---|---|
| GET /health | **200** `{"status": "ok", "platform": "hermes-agent", "version": "0.21.4", "implementation": "hermes-rs", "implementation_version": "0.1.0"}` |
| GET /v1/models (+ API_SERVER_KEY) | **200** models: `hermes-agent`, `galion` (root anthropic/claude-haiku-5.5) |
| POST /v1/chat/completions model=hermes-agent | **502** `agent_failed` / `output truncated` (3/3 suite + retries with galion & anthropic/claude-haiku-5.5) |
| Coding runtime endpoints (/v1/runtime,/coding,…) | none responding (empty) |
| Spend caps on disk | HERMES_RS_SESSION_USD_CAP=5.00 DAILY=7.00 CALLS=2000 (70-research-budget.conf) — **unchanged** |
| day_usd after tests | `0.436656` (was ~0.399 before suite) |

## Chat rows (suite)
| prompt | HTTP | latency_ms | approx_cost_usd | correctness | excerpt |
|---|---:|---:|---:|---|---|
| short | 502 | 3452 | 0.0 | FAIL http=502 | `{"error": {"message": "output truncated", "type": "agent_error", "param": null, "code": "a` |
| json | 502 | 4075 | 0.0 | FAIL http=502 | `{"error": {"message": "output truncated", "type": "agent_error", "param": null, "code": "a` |
| refusal_edge | 502 | 4970 | 0.0 | FAIL http=502 | `{"error": {"message": "output truncated", "type": "agent_error", "param": null, "code": "a` |

## Coding runtime / Landlock AUTO-ACCEPT attempt
Documented path (90-deploy-t291u + t291u D-SB*): `HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1` + dedicated worktree + branch.

| step | result |
|---|---|
| AUTO-ACCEPT enable with WT `/var/lib/hermes-rs/coding-worktree` | **FAIL** — gateway: worktree overlaps denied path `/var/lib`; crash loop status=2 |
| Immediate rollback (rm drop-in) | **OK** — :8642 healthy again |
| Retry WT `/srv/hermes-coding-worktree` + state `/srv/hermes-coding-state` | **FAIL** — `mkdir .../stage: Read-only file system` (ProtectSystem/strict) |
| Final state | Coding **OFF**; drop-in **removed**; hermes-rs **active**; chat still **502 output truncated** |

Conclusion: Landlock missing; ALLOW_NO_LANDLOCK path attempted under owner AUTO-ACCEPT; no writable non-denied worktree+state location found under current unit hardening without further ReadWritePaths change (not done in first pass). Chat 502 appears independent of coding tools.

## Secrets
API_SERVER_KEY / OPENROUTER / RIVER used on VPS only; not written to /workspace.
