# Publish blockers — 2026-10-27 (slot 260, docs only)

Sources: /workspace/swarm/CALL-RECORD.md (line refs), MERGE-20261021.md. No VPS access in this slot.

| # | Blocker | Evidence (CALL-RECORD) | Needs |
|---|---------|------------------------|-------|
| B1 | `main` still `13a296c`; nothing from waves 2-4 merged | L843 (01:27 Oct 22), backup swarm/main-backup-13a296c-20261021 | Slavik/Mate merge decision |
| B2 | PR #12 (swarm/main-staging `7c95348b`) CI failing: box stack 11/12, hermes-tools sandbox on runner, armv7 real_perf, ucpu grep, miri hades-gate, gitleaks 4 FP | L843 | CI fixes, then merge |
| B3 | Live cortex `gaia-cortex-bd6fbe6` vs `swarm/cortex-live-merge-2` `1305dec` (+ OOM rule `c68d8cb`); live shows 0 incidents while nerve holds open ones (237 gap) | L866, L882 | Decisions D-245-1 (caps 3 vs 2), D-245-2 (first-connect backfill burst ~9 R11 alerts) before deploy |
| B4 | Sandbox OOM rule R4b `c68d8cb` pushed, NOT deployed (does not cherry-pick onto bd6fbe6) | L882 | Ships only with B3 deploy |
| B5 | Hermes Telegram notify missing | wave4-tasks row 260 | Implementation + owner channel config |
| B6 | Nerve flood sensor false-critical (cycle-gap counter dumped into 10 s bucket → phantom ddos, score 61→41) | L821 (P-233-1) | Fix P-233-1 + deploy nerve |
| B7 | Mouth 503 (galion-one/mouth) left alone at console cutover | L884 | Investigate |
| B8 | External https → CF 403 challenge from VPS IP (origin OK) | L884 | CF rule decision (no CF changes by agents) |
| B9 | Push-pending LOCAL branches (Auto-review blocked): 233 416521d, 234 de85656, 235 a06bdbd, 236 5e0adba, 237 1754ff2, 238 9479050, 239 1919ca8, 240 4137301, 241 4b835a6, 242 4eca4b7, 243 2a01231, 244 7117d54, 246 ae3eee0 | L825, L870 | Parent/owner push |
| B10 | REUSE gate 7 fails on integration-5 (7 files) | L806 | Licence fix |

Done since: live console cutover to console-dist/srvx 17:29 (L884); /api/v1/admin paired:true, session-pack 401 needs device.
