<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Nerve T4.1(a): state-dir compatibility rehearsal (slot 223)

Base `swarm/nerve-event-driven` bb287cc, branch `swarm/nerve-t41-statedir`.
Box only; ports 20930-20939. Times Europe/Zagreb (+02:00).

## What the nerve keeps in `/var/lib/galion-nerve`

| file | read at start | written | risk found on bb287cc |
|---|---|---|---|
| `reflex.json` (live restart timestamps, last rollbacks) | yes | after every reflex action | only integer timestamps read: float / string / RFC 3339 entries silently dropped (**budget reset**); unknown top-level keys dropped on next save; a damaged file = empty budget (**reset**) |
| `mode` (`live` / `dry-run`) | every decision | by operators | none (trimmed; anything else falls back to `NERVE_DRY_RUN`) |
| `alerts.json` (last 200) | yes | on every alert | a non-list file = empty history, overwritten by the next alert (**lost history**); 30-min dedupe not restored, so a restart re-raises (and pages) recent alerts |
| `evt-llm-spend.json` (event mode paid cap) | yes | on every escalation, also on refusals | strict serde: an older record without `refused` (or any missing field) failed to parse → spend 0 → **cap reset** and a paid call past the cap; a damaged file was overwritten with zero by the next refusal |
| `self-eval/` (latest + 96 timestamped) | no (pruned by instant, legacy names by mtime) | every report | none: history kept |
| `state.json`, `audit.jsonl` | no | every cycle / action | none (derived / append-only) |

## Changes (52d56cf)

- `reflex.json`: timestamps accepted as int, float, numeric string or RFC 3339; a bare value is one restart; unknown top-level keys kept and written back. Damaged file (unreadable, bad JSON, not an object): copied to `reflex.json.corrupt-<unix>`, `/state.reflex.load_note` set, **live** restarts and rollbacks held for one hour (fail closed); dry-run unaffected.
- `alerts.json`: list or `{recent|alerts: [...]}`; non-object entries dropped; newest 200 kept; dedupe window restored from history (future stamps clamped to now); damaged file copied aside, `/state.alerts.load_note` set.
- `evt-llm-spend.json`: `#[serde(default)]` + unknown fields kept; damaged record (bad JSON, not an object, negative or non-numeric total) copied aside, every paid escalation refused with the reason, and the file is never overwritten.
- `load_note` keys appear only after a load problem, so existing goldens and consumers see the same `/state`.

## Fixtures (read-only GETs of the live nerve, no file copies, no secrets)

2026-10-21 22:55:37-22:55:57 +02:00, `ssh slavik-vps curl -s http://127.0.0.1:8670/<path>` for `/state` (37,427 B), `/alerts` (13,206 B, 50 entries), `/self-eval` (18,835 B), `/stats`, `/health` → `crates/openmind-nerve/tests/fixtures/statedir-live/api-*.json`. Grep for token/secret/password/key: none (only the unit name `galion-desk-password-console` appears in `/inventory`, which was not committed). Live then: event mode, cycle 17, score 66 (degraded), mode `dry-run`, all live budgets 0.

Fresh capture for the gated run: 23:03:17 +02:00 (`/state`, `/alerts`). The live nerve had restarted at 22:56:55 +02:00 (cycle 2, mode now `live`) and still served its 50 alerts: the live alert history survived a real restart.

## Tests

`tests/statedir_compat.rs` (real binary, both modes):

1. `old_layout_loads_in_polling_then_event_mode`: old layout built from the fixtures (restarts in every timestamp form, rollbacks int + RFC 3339, unknown keys, alert with unknown field, 5 self-eval history files in legacy and offset names, `notes.txt`, spend 1.25 USD without `refused`). Polling boot then event boot on the same dir: live_1h hermes-rs 2 / galion-one 2 / cloudflared 1, last_rollback exact, mode honoured, all 50 alerts on disk and served, every history file kept + new report, reflex.json unknown keys intact, spend file untouched by polling mode; event mode with a 1.251 USD cap refuses (`spent 1.2500`), the loopback stub receives **0** requests, spend file keeps total/calls/unknown field.
2. `damaged_layout_fails_closed_and_keeps_copies`: broken reflex.json, alerts object, negative spend: both load notes in `/state`, escalation refused "not a usable spend record", 0 stub hits, spend bytes unchanged, `.corrupt-*` copies of all three.
3. `real_live_capture_round_trips_through_new_binary` (`GALION_REAL=1`, release): the fresh live capture rebuilt into a state dir; both modes report all 10 live budgets equal (live_1h, last_rollback), all 23 live `/state` keys present, 50 alerts served.

Negative control: the same test file on bb287cc fails: `galion-one.service live_1h 0 != 2` (legacy forms dropped); with integer-only timestamps the reflex part passes but the spend part fails — the old binary **called the stub** (`curl exit 56` → `failed`, not `refused`) because the older spend record reset to 0.

Unit tests: `reflex::old_reflex_json_forms_and_unknown_keys_survive`, `reflex::corrupt_reflex_json_holds_live_actions_for_an_hour`, `alerts::old_alert_history_restores_dedupe_and_tolerates_layouts`, `evt::escalate::old_spend_file_keeps_its_total_and_damaged_one_fails_closed`.

Results (23:04-23:07 +02:00): `cargo test -p openmind-nerve` lib 102/0; `statedir_compat` 3/3 (release, GALION_REAL=1, fresh capture and fixtures); `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect` 1298 passed / 0 failed; `cargo fmt --all --check` clean; `cargo clippy -p openmind-nerve --all-targets -D warnings` clean; `scripts/no-python.sh` ok.

Known, not from this change: `cargo clippy --workspace --all-targets -D warnings` fails on `evt/journal.rs:165` `large_enum_variant` (`Msg::Event(NerveEvent)` 272 B), a file this branch does not touch; it shows only with workspace feature unification.

## Decisions for Slavik

- D-T41-1: fail closed on a damaged `reflex.json` (live restarts + rollbacks held 1 h) vs fail open (old behaviour: empty budget).
- D-T41-2: restore the alert dedupe window from `alerts.json` on start (stops re-paging after a restart).
- D-T41-3: damaged spend record blocks paid escalation until an operator fixes or removes it.
- D-T41-4: `dry_1h` budgets stay in memory only (reset on restart), as before.
- D-T41-5: `/stats.llm_usd` is per process (since start), not the persisted total; the cap itself uses the persisted total.
