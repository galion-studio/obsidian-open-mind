<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Hermes off-host snapshot key: provisioning and rotation

Wave-4 slot 340, 2026-10-28 CET (+01:00). Follow-up to slot 314 owner
decision D3. Branch `swarm/hermes-offhost-keyrot` off
`swarm/hermes-offhost-fixes` a885d473. **Nothing here is deployed.** The PR #14
branch is untouched.

## What changed in the code

- `HERMES_OFFHOST_HMAC_FILE` (current key) is unchanged: it seals every new
  snapshot and verifies snapshots that carry its key id.
- New: `HERMES_OFFHOST_HMAC_PREV_FILE` + `HERMES_OFFHOST_HMAC_PREV_UNTIL`
  open a **dual-key verify window**. The previous key only verifies, it never
  seals. A snapshot sealed with it is accepted until the window end and
  refused after it. The first heartbeat inside the window re-seals the store
  with the current key, so the old key is out of use within one heartbeat
  (default 300 s).
- Both window settings come together; the window end is Europe/Zagreb RFC
  3339 with `+01:00`/`+02:00` (no `Z`), at most 30 days ahead; the previous
  key must differ from the current one and passes the same file checks
  (regular file, 0600 or stricter, 32 B..4 KiB, not in the store). Any
  mistake is a config error (exit 2), never a silent fallback.
- Resume reports `auth=hmac-sha256+previous-key` when the previous key was
  used, `auth=hmac-sha256` for the current key. `--check-config` prints
  `sealed=<id> prev=<id> prev_until=<time> prev_window=open|closed` (public
  key ids only, never key bytes).
- Key file errors now name the setting that failed (`..._PREV_FILE` vs
  `..._HMAC_FILE`).

## Provisioning (staged, needs approval)

Staged drop-in: `deploy/staged/hermes-rs.service.d/80-offhost-hmac.conf`.
It hands the key in as a systemd credential:

```
LoadCredential=offhost-hmac:/etc/galion/offhost/hmac.key
Environment=HERMES_OFFHOST_HMAC_FILE=%d/offhost-hmac
```

systemd copies the file to `$CREDENTIALS_DIRECTORY/offhost-hmac`, a private
read-only ramfs (mode 0400), so the gateway never reads `/etc` directly and the
key is not visible to other units. `%d` needs systemd >= 248; the VPS runs 255.
If the source file is missing the unit does not start: systemd fails the
credential load, and even without that the gateway exits 2 on a missing key
file (fail closed). The drop-in does not enable off-host export: the store
(D1) is still open.

P1. Make the key on the primary (never print it, never put it in git):

```
install -d -m 0700 -o root -g root /etc/galion/offhost
(umask 077; head -c 48 /dev/urandom > /etc/galion/offhost/hmac.key)
```

P2. Copy the same file to the standby host, same path and mode, over the
existing ssh trust (for example `scp -p`). Do not paste it anywhere.

P3. Check without showing the key: `hermes-gateway --check-config` on both
hosts must print the same `sealed=<id>`.

## Rotation (dual-key window)

Order matters: **standby first, primary second**, so the standby can read
both the old store and the first re-sealed one.

R1. On the primary, make the new key beside the old one:
`(umask 077; head -c 48 /dev/urandom > /etc/galion/offhost/hmac.key.new)`.
Copy it to the standby as `hmac.key.new`.

R2. On each host: `mv hmac.key hmac.prev.key && mv hmac.key.new hmac.key`
(both in `/etc/galion/offhost`, both stay 0600).

R3. On each host uncomment the window block in `80-offhost-hmac.conf` and set
`HERMES_OFFHOST_HMAC_PREV_UNTIL` to a short window, for example 24 h:
`2026-11-04T12:00:00+01:00`. `--check-config` must show the new `sealed=`
id, the old id as `prev=`, and `prev_window=open`.

R4. Standby: daemon-reload and restart at an approved time. Its next resume
reports `auth=hmac-sha256+previous-key` if the store still has the old seal.

R5. Primary: daemon-reload and restart at an approved time. Within one
heartbeat the store `key_id` field (public) changes to the new id. Check:
the store envelope's `key_id` equals the new `sealed=` id.

R6. After the store shows the new id (and before the window end), close
the window: comment the block again, daemon-reload, restart at an approved
time, then `shred -u /etc/galion/offhost/hmac.prev.key` on both hosts.
From now on the old key reads nothing (proven in the real test step 6).

If R5 does not happen before the window end, a standby that has not yet
restored refuses the old-key snapshot (`rotation window closed at ...`) and
leaves local state alone; extend the window (max 30 days) and redo R4/R5.

Emergency (key leaked): skip the window. Put the new key in place on both
hosts, no `PREV` settings, restart the primary first so it re-seals at once.
A standby restoring from an old-key snapshot in between is refused on
purpose; it waits for the next primary heartbeat.

## Owner decisions

- D3a (this slot): approve the staged drop-in and the key location
  `/etc/galion/offhost/` (outside the repo, outside the store, outside
  `/opt/galion`).
- D3b: default window length for routine rotation (suggest 24 h) and how
  often to rotate (suggest every 90 days and on any suspected leak).
- D1, D2, D4 from slot 314 stay open; this slot does not decide them.
