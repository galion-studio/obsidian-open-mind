<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# t314u — desk-hermes Python → Rust cutover

Written **2026-10-27 19:57 CET** (Europe/Zagreb). AUTO-ACCEPT under Mate t314u ~19:43 CET
(live-migrate remaining Python → Rust) + overnight loop; t315u/t317u confirmed Titanaxe idle
and mass-spawn authorized (not needed — existing Oct 18 binaries reused).

## Result: DONE

Live `GET/POST /desk` now forks **`/opt/galion/rust/bin/desk-hermes`** (Rust).  
`python3 /opt/galion/bin/desk-hermes.py` is **not** in the process tree.  
`desk-hermes.py` left on disk for rollback (not deleted).

## Before → after

| Item | Before | After |
|---|---|---|
| `galion-desk` ExecStart | `/opt/galion/bin/galion-desk` (Oct 10, hardcodes `python3 …/desk-hermes.py`) | `/opt/galion/rust/bin/galion-desk` (Oct 18 dist, respects `DESK_HERMES_BIN`) |
| MainPID | 283 | 264677 |
| Mindpack child | `python3 /opt/galion/bin/desk-hermes.py` | `/opt/galion/rust/bin/desk-hermes` |
| `/desk` HTTP | 200, foundation=rust, mindpack 13 keys, turns 78 | 200, same shape; mindpack keys == Python baseline |
| Unit drop-in | (none) | `/etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf` |

## Binary paths + SHAs (sha256)

| Path | SHA256 | Notes |
|---|---|---|
| `/opt/galion/rust/bin/desk-hermes` | `89bc45cd42db326c0ce1ff0cadd3be21649b641fa7ada232c9af3b5aa4889762` | 1427792 B; copy of openmind dist Oct 18 |
| `/opt/galion/rust/bin/galion-desk` | `889737d0337e7a3a087348566be29f0c2c57ee79db6eece61ea5463ff72adb5f` | 403056 B; DESK_HERMES_BIN-aware |
| `/opt/galion/bin/galion-desk` | `555cc44314aa3d6a2ffe2ca4273cee20c566fb1296ac43c0df7e1ca0a9fd11e6` | untouched Oct 10 live binary (rollback) |
| `/opt/galion/bin/desk-hermes.py` | `28a393f061319a56c34c51d4347009c0197489cb96b93d1b7230bfca62827e0e` | untouched (rollback) |

Source crate already existed (`crates/desk-hermes`); no new crate invented. VPS openmind tip at cutover: `202887d56ddbede97acfc47bebd262a6eedd2a39`. Slot 280 audit branch: `swarm/desk-py-callers-audit` @ `ec820e9cfa8f3905a86599bf6bf55b30b0d0e34f`.

## How wired

Drop-in (preferred over rewriting the main unit):

```
# /etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf
[Service]
ExecStart=
ExecStart=/opt/galion/rust/bin/galion-desk
Environment=DESK_BIND=127.0.0.1:8646
Environment=DESK_HERMES_BIN=/opt/galion/rust/bin/desk-hermes
```

Why ExecStart change was required: the Oct 10 `/opt/galion/bin/galion-desk` binary embeds
`python3` + `/opt/galion/bin/desk-hermes.py` and has **no** `DESK_HERMES_BIN` string. Env-only
override would have been ignored. The Oct 18 dist `galion-desk` forks only `DESK_HERMES_BIN`
(default `/opt/galion/openmind/dist/bin/desk-hermes`); we point it at `/opt/galion/rust/bin/desk-hermes`.

## Smoke evidence

1. **Direct binary parity:** Rust desk-hermes vs `desk-hermes.py` → both 13 top-level keys, key sets equal.
2. **Shadow** `DESK_BIND=127.0.0.1:18646` + `DESK_HERMES_BIN=/opt/galion/rust/bin/desk-hermes`:
   HTTP 200; child ` /opt/galion/rust/bin/desk-hermes`; `SHADOW_PY=NO`.
3. **Live after restart:** `systemctl is-active galion-desk` = active; listen `127.0.0.1:8646`;
   environ has `DESK_HERMES_BIN=/opt/galion/rust/bin/desk-hermes`; `/desk` HTTP 200;
   mindpack 13 keys equal to Python capture; child catch: `265032 /opt/galion/rust/bin/desk-hermes`;
   `LIVE_PY=NO`, `LIVE_RUST_DH=YES`.

## Rollback

```bash
rm -f /etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf
systemctl daemon-reload
systemctl restart galion-desk
# restores ExecStart=/opt/galion/bin/galion-desk → python3 desk-hermes.py
# .py and Oct 10 binary were never removed
```

Optional: keep `/opt/galion/rust/bin/{galion-desk,desk-hermes}` installed; they are unused once the drop-in is gone.

## Constraints honored

- No force-push; no secret/TELEGRAM invent; no Titanaxe; no §11 device flash; no Auto-review approval-retry.
- No delete of `desk-hermes.py`.
- Low cargo jobs unused (no rebuild; reused existing dist binaries under loadavg ~28).
- VPS IP not placed in commits.

## Follow-ups (not this ticket)

- After ~7 days with non-empty mindpack: archive (do not delete) `desk-hermes.py` per slot 280 D-280 advice.
- Optionally rebuild static musl copies into `rust/bin` from workspace tip (current copies are glibc Oct 18 dist).
