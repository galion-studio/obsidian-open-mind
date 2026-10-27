<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# PR #14 review: hermes-offhost (wave-4 slot 306)

Reviewed 2026-10-27 21:01–21:10 CET (+01:00, Europe/Zagreb) by Grok Bot.
PR: https://github.com/galion-studio/obsidian-open-mind/pull/14,
branch `cursor/hermes-offhost-continuity-de9e`, tip
`1e0f427be5e142eebdb64b96efbbfa4dd076e9e1` (GitHub-verified signature),
parent `aa77864`, PR base `swarm/main-staging` `98466ab`. 1 commit,
8 files, +2450 / -14. The cloud agent owns that branch: nothing was pushed
there and no review was posted on GitHub. This doc is the review.

## Verdict

Safe to merge **as an off-by-default feature** (nothing changes unless
`HERMES_OFFHOST=1` or a `--offhost-*` flag is set; `/health` is byte-identical
with the feature off). **Not ready to enable in production** until F1–F3 are
fixed. Recommendation: merge after F4 (cheap), track F1–F3 as blockers in
SURVIVAL.md "Production blockers".

## How it was reproduced on the box

PR #14 is not in `/workspace/swarm/base.git` and the box cannot read GitHub
directly, so the commit was rebuilt from the GitHub commit API patches on top
of `aa77864`: `git write-tree` = `dfe92e61119ac69513fe588a25115a3b20b3c285`,
identical to the PR commit's tree. Local branch `pr14-repro` (not pushed).

## Gates (box, x86_64)

| gate | result |
| --- | --- |
| `cargo fmt --all --check` | ok |
| `cargo clippy -p hermes-rs -p hermes-gateway --all-targets -D warnings` | 0 warnings |
| `cargo test -p hermes-rs -p hermes-gateway` | lib 48/48 (incl. 17 offhost), golden 16/16, mcp 1/1 |
| release `x86_64-unknown-linux-musl` hermes-gateway | ok, 2,038,512 B, sha256 `803fc0e9dcc9702eb608c64d8e2da79afe66424273b43b2fd2a0f349dce24064` |
| `cargo check --target aarch64-unknown-linux-musl -p hermes-rs --no-default-features` | ok |
| `cargo check --target armv7-linux-androideabi -p hermes-rs --no-default-features` | ok |
| real test `GALION_REAL=1` (below) | 1/1 pass, 7 FINDING lines |

The PR's own claim (48 lib tests, 17 offhost) is confirmed.

## Real-binary test (box, ports 21760–21764, 2026-10-27 21:07 CET)

Source: `docs/reviews/hermes-offhost-pr14/real_offhost_pr14.rs` (drop into
`crates/hermes-gateway/tests/` on the PR tree; gated by `GALION_REAL=1`;
std + serde_json only; tiny in-test HTTP store on 21762; fake secrets are
built at run time). It starts the real release gateway (`--mock`), seeds a
real SQLite session through `/v1/chat/completions`, exports, wipes, resumes,
runs the standby, and serves a tampered snapshot. Output (verbatim):

```
S1 chat pr14-a (fake github_pat_ in text) -> 200
S1 /api/sessions/pr14-a/messages -> 200 bytes=269
S1 /health (offhost off) -> {"status":"ok","platform":"hermes-agent","version":"0.21.4","implementation":"hermes-rs","implementation_version":"0.1.0"}
S1 files: rs-state.db=40960B mode=644 spend=true gateway_state=false
S2 --offhost-once rc=0 in 10 ms
  stdout: offhost beat role=primary local_ok=true path_ok=Some(true) uri_ok=Some(true) survival_ok=Some(true) sessions=0 bytes=56993 blockers=
S2 external continuity.json 56993 B mode=600
  file role="gateway_state" included=false bytes=0 reason=null
  file role="cron_state" included=false bytes=0 reason=null
  file role="spend" included=true bytes=96 reason=null
  file role="cron_jobs" included=true bytes=31 reason=null
  file role="config" included=false bytes=0 reason=null
  file role="session_db" included=true bytes=40960 reason=null
S2 store puts=1 auth_schemes=["Bearer"] body_same_as_path=true
S2 fake github_pat_ present in snapshot body: true
S2 survival.json keys=Some(["offhost_enabled", "last_heartbeat", "external_state_uri", "resume_policy", "offhost_role", "continuity_schema", "last_export_ok"])
S3 chat pr14-b (fake sk-) -> 200; once rc=0; session_db included=false reason="secret_like_bytes"
  stdout: offhost beat role=primary local_ok=true path_ok=Some(true) uri_ok=Some(true) survival_ok=Some(true) sessions=0 bytes=2462 blockers=session_db omitted: secret_like_bytes
S4 /health -> {"status":"ok","platform":"hermes-agent","version":"0.21.4","implementation":"hermes-rs","implementation_version":"0.1.0","offhost_enabled":true}
S4 /health/detailed auth=200 noauth=401; session pr14-a messages -> 200
  log: 2026-10-27T21:07:48+01:00 hermes-gateway offhost resume role=primary loaded=true source=external_path restored=spend,cron_jobs,session_db kept= blockers=
S4 after resume: cron/jobs.json exists=true (cron.rs says hermes-rs never writes jobs.json)
S5 /health -> {"status":"ok","platform":"hermes-agent","version":"0.21.4","implementation":"hermes-rs","implementation_version":"0.1.0","offhost_enabled":true}
S5 /health/detailed auth=200 noauth=401; session pr14-a messages -> 200
  log: 2026-10-27T21:07:48+01:00 hermes-gateway offhost resume role=primary loaded=true source=external_path restored=session_db kept=spend:cap,cron_jobs blockers=
  rs-spend.json: content_changed=false mode_now=644
  rs-state.db: content_changed=false mode_now=600
S4b /health -> {"status":"ok","platform":"hermes-agent","version":"0.21.4","implementation":"hermes-rs","implementation_version":"0.1.0","offhost_enabled":true}
S4b /health/detailed auth=200 noauth=401; session pr14-a messages -> 404
  log: 2026-10-27T21:07:48+01:00 hermes-gateway offhost resume role=primary loaded=true source=external_path restored=spend,cron_jobs kept=session_db:secret_like_bytes blockers=
S4b resume from a snapshot with session_db omitted -> session pr14-a messages 404
S6 tampered store snapshot -> /api/jobs lists injected job: true
S7 standby /health=200 gateway_state="standby" loaded=true source="external_path"; chat=404 models=404
S7 standby on 0.0.0.0 -> exit Some(2)
S7 standby + --offhost-once -> exit Some(2)
S8 HERMES_OFFHOST_URI bad -> exit Some(2)
S8 HERMES_OFFHOST_URI bad -> exit Some(2)
S8 HERMES_OFFHOST_INTERVAL_S bad -> exit Some(2)
S9 db=1347584 cap=None rc=0 57 ms store_body=1798585
  stdout: offhost beat role=primary local_ok=true path_ok=None uri_ok=Some(true) survival_ok=None sessions=0 bytes=1798585 blockers=
S9 db=6000000 cap=Some("16777216") rc=0 230 ms store_body=8001805
  stdout: offhost beat role=primary local_ok=true path_ok=None uri_ok=Some(true) survival_ok=None sessions=0 bytes=8001805 blockers=
```

Extra measurement (same binary, random bytes as `rs-state.db`, path store):
1,347,584 B DB (the live size) → 1,798,864 B snapshot in 48 ms; 2,200,000 B
DB → `session_db omitted: over_cap`, exit 0, 2,135 B snapshot. Through the
http store: 1.35 MB DB → 1,798,585 B PUT in 57 ms; 6 MB DB with
`HERMES_OFFHOST_MAX_BYTES=16777216` → 8,001,805 B PUT in 237 ms (curl `-K -`
handles an 8 MB inline body).

## Live VPS, read-only (2026-10-27 21:03–21:07 CET)

- `hermes-rs.service` active since 19:46:18 CET, `ExecStart
  /opt/galion/rust/bin/hermes-gateway-b7fbbc78`, NRestarts=0. `GET :8642/health`
  has no `offhost_enabled` → PR #14 is not deployed (expected).
- Live session store: `/var/lib/hermes-rs/rs-state.db` **1,347,584 B**
  (SQLite, journal_mode=DELETE so no WAL to miss), `rs-spend.json` 2,136 B,
  no `rs-cron-state.json`, all 0600 root. Counts only (no values read out):
  0 matches for `sk-`/`ghp_`/`xoxb-`/`github_pat_`/`rv_`/`AKIA` shapes, so
  today the live DB would be included, about 1.8 MB per snapshot.
- Headroom: the DB falls out of the snapshot at about 2.0 MiB raw
  (`max_bytes - 4096`), i.e. ~0.7 MB more history.
- `/opt/galion/galion-light/state/survival.json` 0644 root, 1,385 B;
  galion-desk and hermes-rs both run as root, so the 0600 rewrite (F7) does
  not break the desk today.
- Nerve `:8670/state` (cortex view) at 21:00 CET reported
  `hermes-rs: failing-soon: 11 restarts in the last hour` while systemd says
  NRestarts=0 and one start at 19:46 CET. Not caused by PR #14; worth a look
  by whoever owns that incident rule (probably counts the 19:4x cutover).

## Findings

**F1 (high) — no authenticity on resume.** The `sha256` per file sits in the
same document, so it only detects corruption. Whoever can write the store
(path or URI) can make a booting primary restore `cron/jobs.json` (when
missing), `rs-state.db` (when missing or stale), and the spend ledger. Proven
in S6: a tampered snapshot put an agent cron job `pr14evil` into
`/api/jobs`. Scripts stay confined to `HERMES_HOME/scripts`, so it is not
direct RCE, but it is scheduled model prompts (spend, prompt injection via
history) and a crafted SQLite file parsed by rusqlite. Fix: HMAC-SHA256 over
the canonical snapshot bytes with a key kept on the host and outside the
store (`HERMES_OFFHOST_HMAC_FILE`), refuse resume on mismatch; the crate
already has sha256 in `mcp.rs`, HMAC is ~20 lines std-only.

**F2 (high) — session data leaves the host in clear, and the DB filter is
weaker than the text filter.** The SQLite file is shipped raw (base64);
`secret_like()` knows only `sk-`, `ghp_`, `xoxb-` (≥ N chars) while
`crate::redact` also knows `or-v1-`, `gho_`, `xoxp-`, `AKIA`, Bearer and
KEY=… pairs. S2: a fake `github_pat_` in a chat went off-host inside the DB.
There is no encryption at rest in the store. Fix: encrypt the bundle
(e.g. age/ChaCha20-Poly1305 with an MIT/Apache crate, document in
third-party) or at least reuse one shared pattern list for both paths and add
`github_pat_`, `rv_`, `AKIA`, `gho_`, `xoxp-`.

**F3 (medium, data loss) — all-or-nothing DB.** One secret-shaped string in
one session removes the whole DB from every later snapshot (S3), with exit 0
and only a blocker line. After a disk loss the resume then brings back no
sessions at all (S4b: `pr14-a` → 404). Same for over-cap (>~2 MiB raw). The
next heartbeat also overwrites the last good snapshot with the DB-less one.
Fix: keep the previous good `session_db` entry when the new one is omitted
(or write `continuity.prev.json`), and make `--offhost-once` exit non-zero
(or the nerve alert) when the DB is omitted. Better long-term: export sessions
via SQL with per-row redaction instead of copying the file.

**F4 (medium) — plain `http://` with a Bearer token.** `check_uri` allows
`http://`; S2 shows the Authorization header and the session payload sent in
clear. Fix: require `https://` except for loopback hosts.

**F5 (low) — resume result is hidden.** `spawn_periodic` beats immediately,
so `/health/detailed` `offhost` shows the first beat and never
`restored`/`kept` (S4, S5); the resume result exists only in the journal.
Fix: keep `resume` as a sub-object in the status view.

**F6 (low) — staleness by mtime rewrites intact files.** An idle but intact
DB older than 6 h is rewritten from the snapshot on every restart (S5:
`restored=session_db`, content unchanged, mode 0644 → 0600). Harmless today,
but it is churn and a race if two processes touch the file. Fix: skip when
local sha256 == snapshot sha256.

**F7 (low) — modes and invariants.** `write_private` sets 0600 on restored
files and on `survival.json` (fine while everything runs as root). Restore
writes `cron/jobs.json`, contradicting the cron.rs invariant "hermes-rs
never writes jobs.json" (S4); document it or restore jobs.json to a side
file. `sessions=` in the beat summary counts only `.jsonl` files, so it is 0
with the default SQLite store even when the DB is included (misleading).
`/health` (unauthenticated) now discloses `offhost_enabled` when on.
`curl_exchange` reads the whole response before the 16 MiB check (a hostile
store can make it buffer more).

**F8 (docs) — SPDX.** `deploy/survival/skills/galion/survival/SKILL.md` has
no SPDX header (front matter first; an HTML comment after it works). The JSON
files cannot carry one; cover them in REUSE metadata if the repo uses it.

What is good: default off and `/health` unchanged when off; standby refuses
non-loopback (exit 2), refuses `--offhost-once` (exit 2), serves health only
(chat/models → 404); OpenRouter host and userinfo URIs refused at config;
token never printed (checked stdout/stderr/survival.json); survival.json keeps
cash/lane/when/volume fields untouched; primary refuses standby snapshots;
spend ledger cannot be lowered by a stale snapshot (`spend:cap`); atomic
tmp+fsync+rename writes; std-only, no new dependencies.

## Owner decisions

- D1: where the secondary store lives (path mount vs https URI) and who holds
  its token — PR blocker 1/2, unchanged.
- D2: whether session history may leave the host at all before F2 (encryption)
  is done. Recommendation: not before F1+F2.

No deploy, no VPS writes, no GitHub review posted, PR branch untouched.
Third-party: none added.
