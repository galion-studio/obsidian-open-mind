<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Legacy tooling archive + removal plan (slot 284, 2026-10-27 17:55 +01:00)

Read-only VPS inventory (du/ls/systemctl/grep/ps), probed 2026-10-27 17:54 +01:00.
**Nothing was deleted, moved, or changed in this slot.** Every delete below needs explicit owner approval.
`galion-light/secrets` was excluded from `du` and never read.

## 1. Size + live-reference table

| Path | Size | Live references | Verdict |
|---|---|---|---|
| /opt/galion/tooling/hermes-agent | 258 MB (venv 101 MB) | none in systemd units, cron, profile/PATH; no running process | **Safe archive candidate** |
| /opt/galion/hermes | 417 MB (archive 267, sessions 81, state.db 20, uploads 18, hook_outputs 15) | **LIVE**: `hermes-rs.service` (active) uses `HERMES_HOME`, `.env`, `state.db`, config/SOUL/channels/cron as ReadOnlyPaths; `gaia-hermes-handoff.path` (active) → `.service` uses `.env` + skills/ | **Do NOT remove**; only prune sub-dirs (archive/) after approval |
| /opt/galion/galion-light | 4.9 GB excl. secrets (tulu 4.6 GB, hermes-home 150 MB, shadow 114 MB) | **LIVE**: `galion-one.service` (active, WorkingDirectory); `link-watch-rs.service` (active) writes `state/`; `link-watch.service` (python, inactive/disabled) runs `scripts/link-watch.py` | **Do NOT remove**; candidate sub-dirs only (tulu, hermes-home) |
| /usr/local/bin/hermes | 193 B bash wrapper | execs `/opt/galion/venv/bin/hermes` (Python V1) with HERMES_HOME=/opt/galion/hermes; no unit/cron/profile reference found; `hermes.service` is **masked** | **Safe to remove** with the V1 venv on Oct 28 |

Other: `hermes.service` masked/dead (V1 replaced by hermes-rs). Disk: / 30 % used (71 G free) — no space pressure, so removal is hygiene, not urgency.

## 2. Phased plan (owner approves each delete)

| Phase | Step | Risk |
|---|---|---|
| P0 (done here) | Read-only inventory | none |
| P1 archive (Oct 28, after hermes-rs green ≥24 h) | `tar --zstd` of tooling/hermes-agent, /usr/local/bin/hermes, /opt/galion/venv (if V1-only), unit file `link-watch.service`, to an off-VPS / backup location; record sha256 | low (read-only copy; mind disk if archived locally) |
| P2 remove V1 Hermes | delete /usr/local/bin/hermes, tooling/hermes-agent, V1 venv; keep `hermes.service` masked | low; rollback = untar. Check no operator muscle-memory use of `hermes` CLI |
| P3 remove dead python unit | `link-watch.service` + `scripts/link-watch.py` (already superseded by link-watch-rs) | low |
| P4 prune /opt/galion/hermes/archive (267 MB) | archive then delete | medium: confirm hermes-rs never reads archive/ |
| P5 galion-light/tulu (4.6 GB) | owner decision: model weights still wanted? archive then delete | medium/unknown purpose |
| P6 relocate HERMES_HOME + galion-light state off legacy paths | requires unit changes on hermes-rs, gaia-hermes-handoff, galion-one, link-watch-rs | **high** (live units) — separate slot, owner + maintenance window |
| never | galion-light/secrets, /root/galion-secrets, /opt/galion/openmind | — |

## 3. Owner items
- D-284-1: Approve P1 archive + P2 delete of V1 Hermes (hermes-agent, /usr/local/bin/hermes, V1 venv) on Oct 28.
- D-284-2: Approve P3 removal of disabled python `link-watch.service`.
- D-284-3: Decide on galion-light/tulu (4.6 GB): keep, archive, or delete.
- D-284-4: Decide on /opt/galion/hermes/archive (267 MB) pruning.
- D-284-5: Whether to schedule P6 (move live state off legacy paths) — needs unit changes, not in this wave.
- D-284-6: Note `galion-one.service` sets GALION_HOME=/workspace/galion-light while WorkingDirectory is /opt/galion/galion-light — mismatch to verify (no change made).
