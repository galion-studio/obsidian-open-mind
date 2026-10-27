<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# survival.json

Written 2026-10-27T18:30:00+01:00 (Europe/Zagreb). The live file is
`/opt/galion/galion-light/state/survival.json`. It is not in this checkout.
`galion-desk` reads `volume_used_pct` from it (`crates/galion-desk/src/lib.rs`).
Everything else in the file is opaque to the desk.

## Who writes what

| field | owner | off-host |
| --- | --- | --- |
| `cash_usd` | cash lane (`survival-cash`) | never written |
| `lane` | cash lane | never written |
| `when` | cash lane | never written |
| `volume_used_pct` | whoever records disk percent for the desk | never written |
| `offhost_enabled` | `hermes-rs` offhost heartbeat | written |
| `last_heartbeat` | offhost heartbeat | written |
| `external_state_uri` | offhost heartbeat | written |
| `resume_policy` | offhost heartbeat | written |
| `offhost_role` | offhost heartbeat | written |
| `continuity_schema` | offhost heartbeat | written (`1`) |
| `last_export_ok` | offhost heartbeat | written |

A heartbeat that cannot parse the file leaves the bytes untouched. It does
not invent a cash figure.

## Stale file from about 2026-10-18

A file last touched around that day may only have `cash_usd`, `lane`, `when`,
and `volume_used_pct`. `when` on some older writers used a `Z` suffix. Leave
that string as it is. Missing `offhost_enabled` means off-host continuity is
off, which is the same behavior as before this schema.

Do not install `survival.example.json` over the live file. The example uses
null for `cash_usd`, `lane`, and `volume_used_pct` because this repo has no
copy of the live numbers. The `when` value in the example is an illustration
for that calendar day, not a reading of the VPS file. A real heartbeat only
adds the off-host keys.

Before (shape only):

```json
{
  "cash_usd": null,
  "lane": null,
  "when": "2026-10-18T18:59:00+02:00",
  "volume_used_pct": null
}
```

After a heartbeat with `HERMES_OFFHOST=1` and no secondary store yet, the
same cash and disk fields are still there, plus `offhost_enabled`,
`last_heartbeat`, `external_state_uri` (null until a path or URI is set),
`resume_policy`, `offhost_role`, `continuity_schema`, and `last_export_ok`.

## Operator knobs

See `deploy/survival/skills/galion/survival/SKILL.md`. Short form:

- `HERMES_OFFHOST=1` turns the feature on. Default is off.
- `HERMES_OFFHOST_PATH` is a directory or a `.json` file on storage that
  survives this VPS. A path on the same disk does not.
- `HERMES_OFFHOST_URI` is `http` or `https`. No userinfo. Not a model host.
- `HERMES_OFFHOST_TOKEN` is read at call time and passed to curl on stdin.
- `HERMES_OFFHOST_SURVIVAL` is the survival.json path to patch. Unset means
  the cash file is not touched.
- `HERMES_OFFHOST_LOCAL` defaults to `offhost/continuity.json` beside the
  spend file. On the staged unit that directory should be under
  `/var/lib/hermes-rs`.
- Resume policy default `if_local_missing_or_stale`. Stale default 21600 s.
  Heartbeat default 300 s.

`hermes-gateway --offhost-once` exports once. `--offhost-standby` resumes and
serves loopback `/health` with `gateway_state` `standby`. Standby does not
run cron and does not call a model provider.

## Gap this is for

Caller note, 2026-10-27 CET: Galion servers may later run on new
government-linked infrastructure at Teglenice (Split-Dalmatia
waste-to-energy), about 1.5 years out. Off-VPS survival is for that gap.
The call brief path `/opt/galion/hermes/docs/CALL-CONTEXT-20261027.md` was
not in this checkout. This change does not deploy it.

Out of scope: the Galion One persona switch on the call, and any box snapshot
under `/opt/galion/backups/`. No unit is installed from this tree.

## Production blockers

1. There is no secondary URL or mount in the repo. Until one is set, the
   heartbeat only writes a local file and records the blocker.
2. A store token has to be provisioned outside git.
3. The staged hermes-rs unit is not installed and cannot write
   `/opt/galion/hermes` or the galion-light state dir. Enabling the feature
   on that unit needs a later, approved unit change.
4. A session db over 2 MiB, or one that contains token-like bytes, is stored
   as a hash only.
5. No live deploy is part of this change.
