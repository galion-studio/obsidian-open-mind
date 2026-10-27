<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Mouth honest-health path (branch work only) — slot 259

Prepared **2026-10-27 17:37 CET (+01:00)** on `swarm/mouth-honest-health`, base
`swarm/integration-5` `0d68f469` (already carries `swarm/voice-mouth` `5d776c1`
honest `:8002 /health`). Builds on slot 181 `docs/ops/mouth-loading.md`.

**No live unit was changed. No VPS write. No deploy.** This branch is the
deploy-ready source for option 1 from mouth-loading.md; Slavik still has to OK
the binary swap.

## 1. What honest-health does

`crates/galion-one/src/main.rs` → `mouth_health()` (wired from `:8002` GET
`/health` inside `tts_http`):

| Condition | HTTP | Body gist |
|---|---|---|
| No `GALION_MOUTH`, local stub not ready (today's VPS) | **503** | `"status":"unavailable"`, reason `set GALION_MOUTH to a galion-voice-serve address` |
| `GALION_MOUTH` set, remote `/health` answers | passthrough | remote status/code/body |
| `GALION_MOUTH` set, remote down / empty | **503** | `"status":"unreachable"`, includes the mouth addr |
| Local `speech::local().ready()` true (future in-process) | **200** | `"status":"ok"`, provider/model from the Mouth |

It never answers `"status":"loading"` / `"model":"chatterbox"`. That constant was
the lie that kept nerve `galion-one-mouth` on "stuck loading" for days.

`speech.rs` documents the same contract: `local()` is the `Unavailable` stub;
CPU speech stays out-of-process in `galion-voice-serve` (MemoryMax=128M on
galion-one cannot hold the model).

## 2. What is still 503 on live (read-only, 2026-10-27 17:36 CET +01:00)

| Probe | Result |
|---|---|
| `GET 127.0.0.1:8002/health` | **503** `{"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}` (0.3 ms) |
| `GET 127.0.0.1:8091/health` | 200 `ok` |
| Listeners | `galion-one` pid 304 on `:8091` and `:8002` only. **Nothing on `:8003`** (planned voice-serve) or `:8643` (ear) |
| Env **names** (values not read) | `ONE_MOUTH_BIND`, `ONE_NO_WARM` present. **No `GALION_MOUTH`, no `GALION_EAR`.** |
| Unit | `galion-one.service` active, MainPID 304, NRestarts 0, started 2026-10-22 23:46:47 CET (+01:00) (UTC stamp 21:46:47) |
| Nerve `:8670/state` | `galion-one-mouth` degrading: health probe HTTP 503 — stuck loading for **4.8 d** since 2026-10-22T23:46:47+02:00; incident `stuck-loading-galion-one-mouth`. `voice-worker` **absent**. |
| Live binary line | still the one-no-shellout `40454f9` family (hard-coded loading body); this branch is **not** deployed |

So live still lies. Deploying this branch (or main-staging which already merged
the same honest path with one-no-shellout) is what flips `:8002` to
`unavailable` without needing a speech backend.

## 3. Tunnel-closed ports (mouth / voice related)

Named-tunnel ingress was read read-only from
`/opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml`
(2026-10-27 17:36 CET +01:00). Credentials were not opened. Full dead-port
audit remains in `docs/ops/tunnel-audit.md` (slot 35); this table is the
mouth/voice slice plus the closed origins nerve was still flagging.

### 3a. Mouth / ear ports — intentionally NOT in the tunnel

| Port | Role | Listens on VPS? | In cloudflared ingress? | Note |
|---|---|---|---|---|
| **8002** | galion-one mouth (`ONE_MOUTH_BIND`) | yes (127.0.0.1, galion-one) | **no** | Loopback only. Must stay off the public tunnel. |
| **8003** | planned `galion-voice-serve` | **no** | **no** | Not deployed. Leave closed / unrouted until option 2. |
| **8643** | ear / whisper (legacy) | **no** | **no** | Absent; ear would be `GALION_EAR` → voice-serve. |

### 3b. Tunnel hostnames that point at closed origins (voice-adjacent + current nerve set)

| Hostname | Origin port | Listens? | Status |
|---|---|---|---|
| `voice.galion.app` | **4003** | no | **CLOSED** (old `galion-voice-test` / voice-backend tree; not galion-one `:8002`) |
| `api.galion.app` | **8000** | no | **CLOSED** (nerve edge alert set) |
| `studio.galion.app` / `galion.studio` / `www.galion.studio` | **4001** | no | **CLOSED** |
| `workspace3d.app` (+ www / ws3d) | **4020** | no | **CLOSED** |
| `merlinelement.com` (+ www) | **3011** | no | **CLOSED** |
| `maivirtue.com` (+ www) | **3020** | no | **CLOSED** |
| `forge.galion.studio` | **18800** | no | **CLOSED** (present in live config; not in the 2026-10-20 slot-35 snapshot) |

Nerve alert at probe time: *tunnel routes 18 public hostname(s) to 6 closed
origin port(s) :3011 :3020 :4001 :4020 :8000 :18800*. Staging those rules with
`# tunnel-lint: closed port N` (see tunnel-audit.md) is a separate owner action;
this branch does not touch cloudflared.

**Do not** add `:8002` or `:8003` to the tunnel as part of honest-health. The
honest `/health` fix is loopback-only; public TTS would be a new decision.

## 4. Deploy path (owner OK required — not done here)

1. Build this branch (or `swarm/main-staging` tip, which already merged
   voice-mouth + one-no-shellout) static musl:
   `cargo build --locked --release -p galion-one --target x86_64-unknown-linux-musl`
2. Box + VPS sandbox real tests (same pattern as `docs/ops/galion-one-deploy.md`).
3. Swap `/opt/galion/rust/bin/galion-one`, restart `galion-one` once.
4. Expect `:8002/health` → 503 `"unavailable"` + reason (nerve may still say
   degrading until option 1b treats unavailable as absent).
5. Rollback = previous binary under `/var/backups/galion-one-*`.

Optional follow-ons (still owner-gated): nerve 1b; real CPU voice option 2
(`galion-voice.service` on 127.0.0.1:8003 + `GALION_MOUTH`); do **not** expose
those ports on the tunnel without a separate OK.

## 5. Branch contents

- Honest `mouth_health()` already on integration-5 (from voice-mouth).
- `crates/galion-one/tests/real_mouth_honest_health.rs` — ports **21280–21289**,
  gated by `GALION_REAL=1`.
- Slot 181 diag docs kept: `docs/ops/mouth-loading.md`,
  `docs/real-tests/mouth-loading-diag.md`.
- This file + `docs/real-tests/mouth-honest-health.md`.
