<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# HADES: a ledger that silently stops writing (finding 4 of cortex-s2-d)

- Written 2026-10-21 04:40 CEST (+02:00), slot 125, branch `swarm/hades-ledger-fail`, base `swarm/hades-redteam` e6ceca7.
- Code: `crates/hades-gate/src/{approval,ledger,adapter,main}.rs`. Tests: `tests/ledger_fail.rs` (5), `tests/real_ledger_fail.rs`
  (`GALION_REAL=1`). No new dependency. Nothing ran on the VPS.

## The finding

`Approver::approve` wrote refusals and dry-runs to the ledger with `let _ = self.ledger.note(..)`. When the ledger
stopped taking writes (full disk, read-only remount, lost permission, after the gate started), every refusal and
dry-run vanished: no ledger line, no stderr line, exit code and `/health` unchanged. Approvals already failed closed
(the consume write is mandatory), but the refusal saying so was dropped too. The cortex `hades` sensor reads that
ledger and the hades* journal for `ledger unavailable` / `ledger corrupt`; the gate printed neither, so the sensor saw
silence = healthy. The gate stayed safe (nothing extra approved) but blind: forged-token or admin-safety attacks during
the outage left no trace.

Measured on the old binary (e6ceca7, release), ledger symlinked to `/dev/full` (each write ENOSPC), 2026-10-21 04:36:47 +02:00:

```
check dry-run      exit 0  stderr empty  stdout {"decision":"DryRun",...}       # nothing written, nothing said
check forged token exit 2  stderr empty  code "malformed"                       # refusal not audited, silent
status             exit 0  stderr empty  {"service":"hades-gate",...}           # looks healthy
```

## The fix

1. `Approver::approve_audited` returns `Decided { verdict, audit: Audit::Written | Audit::Failed(LedgerError) }`.
   `approve` keeps its signature (returns the same verdicts) so callers on other branches don't break.
   A refusal caused by the ledger itself (`Corrupt`, `Io`) is always `Audit::Failed`, even if its own audit line landed.
2. `adapter::check_gaia_audited`: the same for GAIA; a never-approvable system (`kill-switch`, `teardown`, ...) is now
   also written to the ledger (it was refused without a trace before).
3. `Ledger::health` / `Approver::ledger_health`: open-for-append + `flock` + full parse, writes nothing. Catches lost
   permission, read-only fs, missing directory, corrupt file. A full disk is only seen by a real write, hence 4.
4. CLI and server (table in `docs/hades-gate.md`): stderr line per failed decision in the sensor's wording; JSON
   `audit` / `audit_error`; `check` exit 3 for an unaudited dry-run; `/v1/approve` 503 for it; `status` exit 3 and
   `GET /health` 503 while the probe fails, and 503 once after decisions failed to write since the previous
   `/health` (full disk: the probe passes but writes don't). `/health` logs only state changes, plus
   `ledger ok again (audit failures so far: N)`.
5. `scan` refuses a non-regular ledger file (`/dev/full`, `/dev/zero` or a FIFO made the read run forever).

Nothing from a request (target, plan, token, token hash) is printed in the new stderr lines; the real test asserts it.

## Not fixed here (open)

- **Deleted ledger = forgotten nonces.** Every open uses `O_CREAT`, so a deleted (or replaced-by-empty) ledger is
  recreated silently and a still-valid token (TTL <= 900 s, hard cap 3600 s) could be used a second time. Fix idea:
  a ledger-generation file or a first-line header checked against a config pin; needs a decision on where the pin
  lives (owner item D-L2).
- Rate of stderr lines: one per failed decision. A request flood during an outage floods the journal (journald rate
  limits it). Acceptable for a loopback-only gate; noted.
- The cortex sensor counts these journal lines only when `NERVE_HADES*` is configured (off by default,
  cortex-s2-d decision "enable in prod").

## Owner decisions (Slavik)

- **D-L1 exit / HTTP codes.** Chosen for the prototype: `check` exit 3 and HTTP 503 for a dry-run whose audit failed;
  refusals keep 2 / 403. Options: (a) keep as built; (b) keep 0 / 200 and rely on stderr + `/health` only (fully
  backward compatible for scripts that treat non-zero as error); (c) also refuse the dry-run (exit 2) when unaudited.
  Evidence: no caller in this workspace checks `hades-gate check` exit codes beyond 0 / non-zero; GAIA uses the library, not the CLI.
- **D-L2 deleted-ledger detection** (above): do it now, later, or accept.
- **D-L3 alerting.** Whether the live nerve's hades sensor (or a services probe of `/health`) should page on
  `hades-ledger-failing` once hades-gate is deployed; tied to cortex-s2-d's "enable in prod" decision.
