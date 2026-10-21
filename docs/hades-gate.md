<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# hades-gate

Admin approval gate. First written 2026-10-19T17:12:17+02:00; real approvals added 2026-10-20T19:10:00+02:00 (Europe/Zagreb).

Dry-run by default and always: an approval is a decision (`WouldApply`), nothing is executed. `kill` and `rotate` are always `Refused`. The old public token `approved-test` approves nothing any more; `decide(action, token)` keeps its signature but, having no admin key, returns only `DryRun` or `Refused`.

## Approval tokens

`hades1.<payload-hex>.<ed25519-signature-hex>`, payload
`v=1;action=<kind>;target=<service>;plan=<sha256>;nonce=<128-bit>;iat=<unix>;exp=<unix>`.

* **Signed:** Ed25519 (`verify_strict`) with the admin public key from the config. The gate host holds only the public key; the seed stays with the admin.
* **Expiring:** `exp - iat` at most `max_ttl_secs` (default 900, hard cap 3600); `iat` at most 60 s in the future.
* **Scoped:** exact action kind, exact target service and SHA-256 of the exact plan bytes.
* **Single use:** the nonce is consumed in an append-only JSONL ledger, under `flock`, with `fsync` before the approval is returned. Any ledger error fails closed. The ledger stores no token string.

## Ledger failures are loud (2026-10-21, slot 125, `docs/security/hades-ledger-fail.md`)

Every decision reports whether its ledger line was written (`Approver::approve_audited` → `Decided { verdict, audit }`;
`adapter::check_gaia_audited`). A failed write never changes a verdict (a refusal stays a refusal, a dry-run grants
nothing, an approval whose consume fails is refused), but it is now visible:

| surface | healthy | ledger failing |
|---|---|---|
| `check` exit | 0 dry-run / approved, 2 refused | 3 dry-run not audited, 2 refused (as before) |
| JSON (`check`, `/v1/approve`) | `"audit":"written"` | `"audit":"failed"`, `"audit_error":"ledger unavailable (..): failing closed"` |
| `/v1/approve` HTTP | 200 / 403 | 503 dry-run not audited, 403 refused |
| stderr = journal | - | one line per decision: `hades-gate: ledger unavailable (<io kind>): failing closed; decision ..` |
| `status` / `GET /health` | exit 0 / 200, `ledger.ok:true` | exit 3 / 503, `ledger.error`; `/health` also 503 once after unwritten decisions (full disk) |

The stderr wording is what the cortex `hades` sensor (`hades-ledger-failing`, swarm/cortex-s2-d f3468ce) already matches
in hades* unit journals. The ledger scan also refuses a non-regular file (a ledger pointing at `/dev/full` or a FIFO
used to hang reading forever).

## Admin-safety rule (beats any token)

Checked before the token. Refuses destructive commands, secret exposure, admin account/login/access changes (passwd, usermod, sudoers, authorized_keys, sshd, firewall, chmod/chown, reboot), plans that cannot be read (non-ASCII, `$(...)`, backticks, `eval`, base64), and protected targets (hades itself, admin, root, ssh, secrets, galion-control, ...). Deliberately over-cautious. Since S3.1 (2026-10-20, `docs/security/hades-redteam.md`) the deny-list is backed by an allow-list: plans may only use letters, digits, whitespace and `/ . - _ : , ( ) ;`, every statement (line or `;` part) must start with a verb from `safety::PLAN_VERBS` (`apply append keep operator read record restart switch take verify`), and a target must be a plain service name (`A-Z a-z 0-9 . _ - @ :`, no `..`).

## The admin can always stop or disable HADES

* `hades-gate disable --config C` creates the disable file: every request is refused. No token or key needed. `enable` removes it.
* `hades-gate serve` binds 127.0.0.1 only; `POST /v1/stop` (no token) or a normal signal stops it. Stopping only removes the ability to approve.

## CLI

```
hades-gate keygen    --seed PATH --pub PATH                       # admin side
hades-gate mint      --seed PATH --action A --target T --plan-file F --out PATH [--ttl S]
hades-gate check     --config C --action A --target T --plan-file F [--token-file F]
hades-gate serve     --config C --port P                          # GET /health, POST /v1/approve, POST /v1/stop
hades-gate status|disable|enable --config C
hades-gate plan-hash --plan-file F
```

Config (flat TOML subset): `admin_pubkey`, `ledger`, `disable_file` (all required, quoted paths), `max_ttl_secs`.

## GAIA

`hades_gate::adapter::check_gaia(&approver, system, target, &plan_steps, token, now)` maps `heal-apply`, `rollback` and `deploy-switch` to gate actions; `teardown`, `kill-switch` and `secret-rotation-plan` are refused. The signed plan bytes are the steps joined with `\n`. No dependency on gaia-core.

`gaia heal-tick` (crate `gaia`, branch swarm/gaia-hades-tokens, 2026-10-20) is the first real caller. It replaced gaia-live's per-run random test token:

- `gaia heal-plan ... --out plan.txt -- START_CMD` writes the exact plan (header, service and loopback port, health path, JSON-quoted start argv, the 3-per-hour cap); the admin runs `hades-gate mint --action heal-apply --target SERVICE --plan-file plan.txt --out tok` on the admin side.
- `gaia heal-tick ... --hades-config hades.toml [--token-file tok] -- START_CMD` rebuilds the same plan and calls `check_gaia(approver, "heal-apply", service, plan, token, now)`. Disable file and admin-safety rule are checked first; then signature, lifetime, expiry and scope; then the nonce is consumed in the ledger. Only `Approved` lets GAIA core's `Execute` carry the `owner` Alpha and release the restart.
- The restart cap is checked before the token is consumed, so a capped tick does not burn an approval. Without a token the verdict is dry-run and nothing is restarted.
- `hades-gate disable --config C` still needs no token; heal-tick then refuses with code `disabled` and consumes nothing.
- Nerve events: `gaia.heal.refused` (with `code`: no-token, forged, malformed, wrong-scope, expired, lifetime, replayed, admin-safety, disabled, gate-unavailable, token-file), `gaia.heal.approved` (nonce, plan SHA-256, token expiry), `gaia.heal.applied` / `failed` / `capped`. The token string is never printed; only its SHA-256 prefix.

Real-life test: docs/real-tests/gaia-hades-tokens.md, scripts/real/gaia-hades-tokens.sh.

Real-life test: docs/real-tests/hades-approval.md. Dependencies: docs/third-party/hades-approval.md.

## Ledger under race and short writes (slot 219, 2026-10-21)

`tests/real_ledger_race.rs` (`GALION_REAL=1`, ports 20890-20891 or
`HADES_REAL_RACE_PORT`) races one token through 12 `check` processes, and
through 2 `serve` processes plus 6 `check` processes on one ledger: exactly one
approval per token. A write the kernel cuts short never approves and the gate
then fails closed (`ledger corrupt`) until the corrupt line is removed. See
`docs/security/sec-ledger-race.md` for findings and owner decisions D-219-1..4.
