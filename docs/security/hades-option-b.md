<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# HADES Option B prototype: typed, signed plan steps

2026-10-21 04:55 CEST +02:00 · branch `swarm/hades-option-b` on `swarm/hades-redteam` e6ceca7 · wave 4 slot 124

**This document does not decide anything.** It gives Slavik the evidence to choose A, B or C from
`docs/security/hades-redteam.md` ("Owner decision"). The live HADES security mode is not changed; HADES is
not deployed on the VPS (no listener, no unit, no binary, checked 04:46 CEST).

## What the prototype is

A second, parallel approval path. The prose path (`Approver::approve`, `/v1/approve`, `check`) is untouched.

| Piece | Where |
|---|---|
| `typed::Plan`, closed `enum Step` (11 steps), validated `Service`, `Release`, `Probe`, closed `Runbook`/`Source` | `crates/hades-gate/src/typed.rs` |
| Canonical encoding `hades-plan/v1\n<step>\n<step>` (no trailing newline). **These bytes are what the token hashes and signs**; token format, ledger and replay rules are unchanged | `Plan::canonical`, `Plan::parse` |
| Strict parser: printable ASCII + `\n` only, header required, ≤ 8 KiB, ≤ 64 steps, and the parse must re-encode byte-for-byte, else refused | `Plan::parse` |
| `Approver::approve_typed`: same disable / action allow-list / target / token / single-use order as `approve`; the prose `safety::review` of the plan is replaced by "must parse as a typed plan" | `src/approval.rs` |
| `POST /v1/approve-typed`, CLI `hades-gate check-typed` | `src/main.rs` |
| `typed::Inventory` (new this session): an exact allow-list of service names; `check(&Plan)` refuses any service not listed. Not wired into `approve_typed` (owner decision B2 below) | `src/typed.rs` |

Steps: `read-releases`, `restart <svc>`, `switch <svc> release=<id>`, `rollback <svc> release=<id>`,
`verify [host:port] /path`, `operator deploy-rollback|deploy-rollback-console`, `take-findings demeter`,
`apply-fixes max=1..100`, `append-actions`, `keep-failed`, `record-lineage`.
There is no step that deletes, writes a file, runs text, or names a path outside `verify`'s URL path (no `..`).

## Evidence (all numbers from real runs, see `docs/real-tests/hades-option-b.md`)

| Check | Result |
|---|---|
| Prose path, 82 red-team attacks (unchanged) | **0 / 82** WouldApply (unit test + real HTTP server) ; 9/9 legit approved |
| Typed path, same 82 attacks, raw **and** wrapped in the `hades-plan/v1` header, each with a **valid** signed token | **0 / 164** WouldApply (unit test + real HTTP server :19940) ; refusal codes typed-plan 150, malformed-target 14 |
| Typed-only attacks (protected services, `;`, `$()`, `..`, CR, tab, NUL, non-ASCII, case, extra args, unknown steps, 65 steps) | 30 + 1 refused |
| 9 legit corpus plans re-written as typed plans | 9/9 approved once, replay refused (HTTP 403 and CLI exit 2) |
| Token for prose cannot approve typed bytes and vice versa | refused both ways |
| Real VPS units (152, `systemctl list-units`, read-only) | deny-list alone accepts 142, refuses 10 (ssh, nftables, iptables, ufw, systemd-logind, user@0, …) |
| Security units the deny-list lets through on the real VPS | `firewalld fail2ban auditd apparmor cron` (5) -> refused by an `Inventory` of the 10 nerve-managed units (5/5) |
| Typed `restart <unit>` + `verify 127.0.0.1:<port> /health` for every real nerve-managed unit | 10/10 parse, round-trip and pass the inventory |
| Corpus legit plans vs the real VPS | 5/9 name `galion-tuner`, which **does not exist** on the VPS (the unit is `tuner`, port 8650; 8420 is openmind-memory) |
| Tests | hades-gate 61 passed / 0 failed (14 suites); workspace 512 / 0 (122 suites, sandbox crates excluded per sandbox_vps_only); clippy -D warnings 0; fmt clean; x86_64 + aarch64 musl release build; aarch64-android + armv7 check |

## What B buys, honestly

* The residual risk in the red-team doc ("prose after an allowed verb is unchecked") is gone on the typed
  path: an executor can `match` on `Step` and call fixed Rust code per variant. There is no text to feed to a shell.
* New capabilities become a code review: a new `Step` variant or `Runbook` name, with a test.
* The signature still covers the exact bytes, so the console shows `Plan::render_prose()` while the admin
  signs `Plan::canonical()`; the two cannot be swapped (tested).

## What B does not buy (yet)

* The deny-list is still the only service filter unless an inventory is loaded: on the real VPS it lets
  `firewalld`, `fail2ban`, `auditd`, `apparmor`, `cron` through. `Inventory` fixes this but is not wired in.
* Nothing executes. `WouldApply` is still only a verdict; an executor is separate work.
* gaia-hades `plan_for` (on integration branches, not on this base) still emits prose; it needs a typed
  `plan_for` before B can be the only path. Estimated ~0.5 day on integration-3 plus the console signing view.
* The corpus legit plans use `galion-tuner`; real names should come from the inventory.

## Decision items for Slavik (options, not decided)

**D-B1. Which HADES plan model?**
* A: prose + allow-list; executor runs only fixed actions per (action, target), plan is a record.
* B: typed plans (this prototype) as the only approvable form; prose path kept read-only for old ledgers, then removed.
* B': both paths side by side (as this branch) for a transition period.
* C: as is (safe only while nothing executes).

**D-B2. Service filter on the typed path.**
* (a) deny-list only (today);
* (b) exact inventory, from the nerve-managed units (today: cloudflared console galion-desk galion-health
  galion-one hermes-rs link-watch-rs openmind-memory tuner tuner-quick);
* (c) a hand-written inventory in the HADES config, reviewed per change.
  Note: (b) would make `cloudflared` restartable through HADES; (c) lets that be excluded.

**D-B3. Step set.** Keep the 11 steps, or drop `operator <runbook>` (a runbook is still a script run by a human).

None of these is decided here. No live mode was changed, no HADES was deployed.
