<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Owner decisions digest: wave 4 (for Slavik)

Prepared 2026-10-27 21:05 CET (+01:00) by slot 301. Docs only. Live state below comes from read-only VPS probes at 21:02 CET (+01:00).
Each line gives the question, the evidence branch @ SHA, what is live now, and a recommendation. **You make the decisions. Nothing here has been applied.**

## Quick answer sheet (13 open picks, 3 settled by live deploys)

| ID | Question | Evidence (branch @ SHA) | Live now | Recommend |
|---|---|---|---|---|
| **D-245-1** | Cortex restart cap per unit/hour: 3 or 2? | `swarm/cortex-d245-pack` @ c3d13e07 (`docs/decisions/D-245.md`) | config `restarts_per_unit_hour: 3` on gaia-cortex-73583ab-llm, mode live | **2**, to match the nerve executor. It's a one-line config change in a later window. |
| D-245-2 | Keep R11 alerts on first-connect backfill? | same | **Already live** since the t273u deploy (backfill on). | Ratify **A: keep**. The replay showed 5 alerts, 1 handoff and 0 actions. |
| **D-275-1** | Ship binaries over the 2 MiB budget? | `swarm/release-size-trim` @ b2d15657 | hermes-gateway live is 2,732,248 B with coding compiled in | **Take the trim** (coding opt-in): x86_64 is 2,068,288 B, under budget. No exception needed. |
| **D-275-2** | Land gaia-cortex in main-staging so one SHA builds every unit? | `swarm/cortex-staging-fold` @ 77df80f9 (tests 2156/0) | Not in staging (base.git staging is still 98466ab9) | **Yes**, merge the fold. |
| D-275-3 | Cut over the musl artifacts live? | `swarm/release-artifacts-matrix` @ 319685c8 | **Largely done** under Mate t273u/t314u: nerve, cortex, backup, console, desk are Rust | Ratify. Remaining work is tracked by the overnight loop. |
| D-275-4 | console-ui: Node dist or Rust bin? | `swarm/console-v2-merge-1` @ 2087316a | **Rust live**: `galion-console-serve-2d846fb8 --ui rust` on :8080 | Ratify **Rust**. |
| **D-283-1** | fail2ban: A keep / B Rust guard / C staged shadow | `swarm/fail2ban-nft-memo` @ ca7dab2c | fail2ban active, galion-guard inactive | **C**: 7 days observe-only next to fail2ban, then switch. |
| **D-283-2** | Guard backend: netlink (nftnl/mnl) or `nft -j` JSON | same | none | **`nft -j`** (simpler; nft is already installed). Revisit netlink later. |
| **D-283-3** | Admin allowlist (IPs that are never banned) | same | none | **Only you can supply this.** Keep it outside the repo. This blocks B and C. |
| **D-283-4** | Ban policy | same | 3600 s / 600 s / 5 tries, IPv4 only | Keep the current values and **add IPv6** (cheap and safe). Hold escalating bans. |
| **D-283-5** | VPS unit change window for the guard | same | none | Approve a window once D-283-3 is in hand. |
| **D-T24-3** | guardian-tick to nerve: A dual sink / B file only / C nerve only | `swarm/guardian-tick-wire` @ 6e6cae43 (1271 tests, 0 fail; VPS throwaway PASS) | guardian-tick.timer active (report-only, from survival-s47), **no NERVE_URL drop-in** | **A: dual sink**. Then add `NERVE_URL=http://127.0.0.1:8670`. |
| **AP-1** | Apache-2.0-only safetensors: A allow as an exception / C replace with MIT | `swarm/safetensors-mit-alt` @ 97141c1c (TTS WAV byte-identical); gate `swarm/cargo-deny-gate` @ 400e93a5 | MIT-strict gate approved by Mate (t273u); AP-1 still pending | **C: MIT replacement.** This removes the only Apache-only crate. Also ST-2: accept stricter duplicate-key handling. |
| **AP-4** | Allow `Apache-2.0 WITH LLVM-exception`? | `swarm/apache-allowlist-memo` @ 56993b7a | listed as pending in the gate | **Yes.** It's the standard LLVM/Rust toolchain licence and no stricter than Apache-2.0. Mate still owes the exact package list. |
| **CF-403** | VPS's own HTTPS probes get a Cloudflare challenge | `swarm/cf-403-evidence` @ 138f8489 | still `403 cf-mitigated: challenge` from the VPS at 21:02 | **Option 1: do nothing.** Monitors use loopback; real users are not affected. If needed later, use option 2 (WAF skip for the VPS IP only). |
| **MOUTH** | Fix the `:8002` "loading/chatterbox" health claim | `swarm/mouth-honest-stub-rs` @ d552fc4b (real tests PASS) | :8002 still answers **503 `"status":"loading","model":"chatterbox"`** | **Option 1**: deploy honest galion-one so it reports "voice not deployed". Don't run the stub alone (it would collide with :8002). |

Bold IDs are still open. The others are effectively decided by live deploys and only need your OK.

## Context in one breath
- **Already ruled** (Mate t273u, 18:09–18:20 CET): hoff-23 approved; D-241 no-OpenRouter for nerve/cortex accepted; MIT-strict cargo-deny gate approved.
- **Still blocked on inputs only you hold**: HADES H1–C5 needs admin/custody keys. Hermes Telegram notify needs a bot token and chat id. D-283-3 needs the admin allowlist.
- **Side finding**: `api.galion.app` returned 502 from outside at 17:46 CET. That's an origin/route issue separate from CF-403 and has no owner item yet.
- **Not in the base staging**: none of the evidence branches above is an ancestor of `swarm/main-staging` 98466ab9 in the box mirror. They need the normal merge path.

## Fastest path (your 5 minutes)
1. Say "**D-245-1=2, D-275-1 trim, D-275-2 yes**". That's one config change plus two merges, and it closes the release-size and one-SHA blockers.
2. Say "**AP-1=C, AP-4 yes**". The licence gate then becomes fully MIT plus the LLVM exception.
3. Say "**D-T24-3=A, MOUTH option 1**". Two small live fixes, and the nerve then sees everything honestly.
4. Send the **admin allowlist** (D-283-3) privately, and the fail2ban migration (C) can start.
5. CF-403: nothing to do.
