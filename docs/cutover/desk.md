<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# galion-desk: live Oct 10 binary vs box build, and a cutover checklist

- Branch: `swarm/desk-linkwatch` (base 13a296c), swarm slot 18.
- Written: 2026-10-20 19:15 +02:00 (Europe/Zagreb).
- **No cutover was done.** Nothing on the VPS was stopped, restarted, copied,
  built or written. VPS access was read-only: `systemctl status/show`,
  `ss -ltnp`, `ls`, `sha256sum`, `strings`/`nm` on the live binary, `cat` of
  the live binary's source file, and HTTP GETs to `127.0.0.1:8646` over ssh.

## 1. What runs live today

| | live (VPS) |
|---|---|
| unit | `galion-desk.service`, active since 2026-10-12 07:01:36 +02:00, pid 231, runs as root, no `Environment=` |
| binary | `/opt/galion/bin/galion-desk`, 4,735,088 bytes, mtime 2026-10-10 11:02 +02:00, sha256 `555cc443…11e6` |
| kind | x86_64 glibc dynamic PIE, not stripped, with debug info |
| source | `/workspace/galion-light/desk-rs/main.rs` on the VPS (17,732 bytes, mtime 2026-10-10 11:02 +02:00, sha256 `2e44456b…ab76`). The binary's debug info names this file, and its function symbols (`main::snapshot`, `main::handle`, `main::write_note` …) match it. It is **not** in any git repo. |
| listener | `127.0.0.1:8646` |
| mindpack | runs `python3 /opt/galion/bin/desk-hermes.py` on **every** `GET /desk` and `POST /desk` |

The workspace `crates/galion-desk` at GitHub main (13a296c) already differed
from that source (configurable paths, Rust desk-hermes). This branch adds the
changes listed in section 4.

| | box build (this branch) |
|---|---|
| x86_64 musl release | 758,560 bytes, static, sha256 `b35e2e7b398f9c99…` |
| aarch64 musl release | 663,416 bytes, static, runs under qemu-aarch64 |
| x86_64 glibc release | 632,944 bytes |
| desk-hermes x86_64 musl | 1,515,296 bytes (bundled SQLite, opens state.db read-only) |

## 2. Route diff (no change)

The same GET matrix against both (live: over ssh to `127.0.0.1:8646` on
2026-10-20 18:53:50 +02:00; box: release build on `127.0.0.1:19180`,
2026-10-20 19:07:37 +02:00):

| request | live status | box status | body |
|---|---|---|---|
| `GET /` | 404 | 404 | `{"error":"not found"}` (21 B) on both |
| `GET /health` | 200 | 200 | `{"ok":true}` (11 B) on both |
| `GET /healthz` | 200 | 200 | health (prefix match) |
| `GET /health/x` | 200 | 200 | health (prefix match) |
| `GET /desk` | 200 | 200 | snapshot |
| `GET /desk/` | 200 | 200 | snapshot |
| `GET /deskx` | 200 | 200 | snapshot (prefix match) |
| `GET /desk?x=1` | 200 | 200 | snapshot |
| `GET /nope` | 404 | 404 | not found |
| `GET /DESK` | 404 | 404 | not found (case-sensitive) |

From the source (both sides, and pinned by `routing_matches_the_live_desk_probe_matrix`):
`POST /desk*` applies an act then returns the snapshot; `/health*` answers
any method; `HEAD`/`PUT /desk` are 404. Response headers are identical:
`content-type: application/json`, `content-length`, `connection: close`.

The prefix quirks (`/deskx`, `/healthz`) are kept on purpose so the console
and any scripts keep working. Tightening them is a separate decision.

## 3. Response shape diff (no change)

`GET /desk` from the live desk was reduced to a type shape (no values kept;
`crates/galion-desk/tests/data/live-desk-shape-20261020.json`). The gated
real test `real_desk_matches_the_live_shape` compares the box desk with it
in both directions and passed: the same 22 top-level keys in the same order
(`foundation, at, line, model, vram, ctx, hermes, one, tunnel, voice, heard,
said, turns, waking, resting, hold, services, board, events, mindpack, focus,
notes`), the same 13 `board` keys, the same 8 services in the same order
(`Console, Origin, Hermes, Laptop, One, Room, Tulu, Tunnel`), and the same 13
`mindpack` keys with the same nested keys. The live mindpack came from
Python desk-hermes.py on live state; the box one came from Rust desk-hermes on
a fixture home. That the shapes match is evidence the port is faithful.
Value-level parity on the same state can only be checked at cutover time
(checklist step 4).

Live service rows on 2026-10-20 18:53 +02:00: Console up, Origin down,
Hermes up, Laptop down, One up, Room down, Tulu down, Tunnel up.

## 4. What changed since the Oct 10 build

Defaults keep live behaviour: with no environment and no
`/etc/openmind/openmind.toml` (absent on the VPS), the new binary uses the
same port, the same state paths and the same probe ports.

| # | change | live Oct 10 | box build | effect at cutover |
|---|---|---|---|---|
| 1 | mindpack program | `python3 /opt/galion/bin/desk-hermes.py` | Rust `desk-hermes` at `DESK_HERMES_BIN` (default `/opt/galion/openmind/dist/bin/desk-hermes`), stdin closed, stderr dropped | no Python fork per GET. The dist file on the VPS has mtime 2026-10-18 19:43 +02:00 and predates this branch; deploy the new desk-hermes too (same output shape) |
| 2 | configuration | everything hard-coded | `DESK_BIND`, `DESK_OLLAMA`, `DESK_STATE_DIR`, `DESK_ONE_DIR`, `DESK_HERMES_DIR`, `DESK_HERMES_BIN`, `OPENMIND_CONFIG` | none with defaults |
| 3 | probe targets | hard-coded 8642/8002/20241/8080/80/4020/8093 | same defaults, `DESK_PROBE_{HERMES,ONE,TUNNEL,CONSOLE,ORIGIN,ROOM,TULU}` override | none with defaults; lets box/test instances be wired |
| 4 | `Laptop` row / `line` / `stamp` | TCP `127.0.0.1:11434` | TCP to the Ollama address (default the same) | none with defaults |
| 5 | Ollama address | IP literal only; a hostname silently became `127.0.0.1:1` | hostnames resolve | none with defaults |
| 6 | request reading | one `read()` of up to 8 KiB; a body split across packets was lost | reads the head, then `content-length` body bytes | POSTs from slow clients now always apply |
| 7 | oversize | silently truncated | head > 16 KiB or body > 64 KiB: `413 Payload Too Large` | no real client sends that much |
| 8 | JSON string reading | `\n`, `\t`, `\u00e9` came out as `n`, `t`, `u00e9` | decoded (incl. surrogate pairs) | correct non-ASCII in notes, heard/said, laptop name |
| 9 | `events` | cut at the first `]`, even inside a string | string-aware | events containing `]` no longer truncate |
| 10 | notes | raw lines echoed; one torn line made `/desk` invalid JSON | lines that are not `{…}` are skipped | `/desk` stays valid JSON |
| 11 | mindpack check | output starting with `{` accepted | must start with `{` and end with `}` | torn output becomes `{}` instead of invalid JSON |
| 12 | bind failure | `expect("bind")` panic | message and exit 1 | cleaner journal |
| 13 | `--help` | ignored (would bind 8646) | prints usage, exits 0 | safe to run by hand |
| 14 | `voice` / `hold` | wrote into existing dirs | create the dir if missing | none on the VPS (dirs exist) |
| 15 | code | 1 file, globals, `unwrap` on parse | library (`config`, `net`, `scan`, `view`) + 25-line `main`; no globals; 0 `unwrap`/`expect` outside tests; `#![forbid(unsafe_code)]` | testable in-process |

Unchanged: `foundation: "rust"`, key order, `desk-notes.jsonl` line format
(`{"at":<unix>,"text":"…"}`), 160-char note cut, 80-char focus id filter,
`samantha`/`jarvis` only, wake/rest Ollama bodies, timeouts (200 ms probes,
400/700 ms `/api/ps`, 90 s generate), 404 body.

Measured: live `GET /desk` took 0.15–0.17 s per request (the Python fork).
The box desk took 2–3 ms. That is not a like-for-like comparison: the box
used a small fixture Hermes home and fewer live ports.

link-watch interplay: the live `link-watch-rs` writes `france` as a bool (its
France URL is set), and the desk reads it with the same first-match scanner.
The new optional `urls` key (only when `LINK_WATCH_URLS` is set) is appended
after `events` and contains none of the keys the desk scans (`name`, `model`,
`events`, `france`, `*_ms`, `gpu`, `cpu`); there is a test for that.

## 5. Cutover checklist (for the owner; nothing here has been done)

Pre-flight (no live impact)
1. [ ] Owner approves this cutover. This document is not approval.
2. [ ] Merge `swarm/desk-linkwatch`. On the **box** (never on the VPS), build:
       `cargo build --release --target x86_64-unknown-linux-musl -p galion-desk -p desk-hermes`;
       record both sha256 values.
3. [ ] Copy both to new paths on the VPS without touching the live ones, e.g.
       `/opt/galion/rust/bin/galion-desk.new` and `/opt/galion/rust/bin/desk-hermes.new`.
4. [ ] Shadow run on a spare port with a **copy** of the state, because POSTs write:
       `rsync -a /opt/galion/galion-light/state/ /tmp/desk-shadow/state/`, then
       `DESK_BIND=127.0.0.1:18646 DESK_STATE_DIR=/tmp/desk-shadow/state
        DESK_HERMES_BIN=/opt/galion/rust/bin/desk-hermes.new /opt/galion/rust/bin/galion-desk.new`
       (Hermes home stays `/opt/galion/hermes`; desk-hermes opens `state.db` read-only).
5. [ ] Compare `GET /desk` on 8646 (live, Python mindpack) with 18646 (shadow, Rust
       mindpack): same key shape (`jq` paths diff is empty), and the same `mindpack`
       numbers (`mind.sessions`, `mind.messages`, `cache.*`, `roles`, `road[].id`).
       Differences other than `at`, `waking`/`resting` timing and `history` stamps
       are blockers.
6. [ ] Open the console (console.galion.app → node on 8080) and check it renders
       from the shadow `/desk` (point a dev console at 18646, or compare the JSON it
       consumes).
7. [ ] Stop the shadow process; remove `/tmp/desk-shadow`.

Cutover (owner runs; short desk outage)
8. [ ] Keep the old binary: `cp -p /opt/galion/bin/galion-desk /opt/galion/bin/galion-desk.oct10`.
9. [ ] Install: `install -m 0755 /opt/galion/rust/bin/desk-hermes.new /opt/galion/openmind/dist/bin/desk-hermes`
       (or set `Environment=DESK_HERMES_BIN=/opt/galion/rust/bin/desk-hermes` in a
       drop-in so the read-only openmind tree is not written), and
       `install -m 0755 /opt/galion/rust/bin/galion-desk.new /opt/galion/bin/galion-desk`.
       Note: `/opt/galion/openmind` must stay at 2f5a730, so the drop-in route is preferred.
10. [ ] `systemctl restart galion-desk`.
11. [ ] Verify: `ss -ltnp | grep 8646` shows the new pid; `curl -s 127.0.0.1:8646/health`
        is `{"ok":true}`; `curl -s 127.0.0.1:8646/desk | jq '.mindpack|keys|length'` is
        `13`; `journalctl -u galion-desk -n 20` shows `galion-desk 127.0.0.1:8646`
        and no errors; no `python3` child appears during `GET /desk`
        (`ps --ppid <pid>`).
12. [ ] Watch for 24 h: RSS (box run: about 2.7 MB) and CPU (expected to drop: no
        Python fork per poll).

Rollback
13. [ ] `install -m 0755 /opt/galion/bin/galion-desk.oct10 /opt/galion/bin/galion-desk && systemctl restart galion-desk`
        (remove the drop-in if one was added). State files are compatible both ways.

Afterwards (separate decisions)
14. [ ] Check whether anything else still runs `/opt/galion/bin/desk-hermes.py`
        (crontabs, units, the console); if not, archive it.
15. [ ] Put the Oct 10 source (`/workspace/galion-light/desk-rs/main.rs`) under version
        control or archive it next to the `.oct10` binary.
16. [ ] Update LIVE.md (binary table) and `docs/STATUS.md`.
