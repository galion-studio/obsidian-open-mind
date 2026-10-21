<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# HADES admin key custody (Gaia S5.3)

Written 2026-10-21 21:05 CEST (+02:00) by wave-4 slot 215 on `swarm/hades-key-custody`, base
integration-5 `0d68f46`. **Docs only.** Nothing is installed, no key is made for real use, and no
owner decision is taken here. Every behaviour claim cites the code at `0d68f46` as `file:line`
(paths under `crates/hades-gate/src/` unless shown in full). Each claim marked **[K#]** was also
checked with the real release binary on the box at 21:03-21:04 CEST; see
`docs/real-tests/hades-key-custody.md`.

**Current state on the VPS (read-only, 2026-10-21 21:04:19 CEST):** HADES is **not installed**.
There is no `/etc/galion/hades`, no `/var/lib/galion/hades`, no `hades-gate` binary or unit, and
no process. So no HADES admin key exists on the VPS today. This document covers how a key should be
made, kept, rotated and revoked once an owner-approved install happens.

## 1. What the key is and what it can do

* One Ed25519 key pair. The **seed** (private half) signs approval tokens. The **public key**
  verifies them (`token.rs:14-17`, `approval.rs:145-148`).
* A token approves **one** action kind, **one** target and **one** exact plan (SHA-256), once, for
  at most `max_ttl_secs` (default 900 s, hard cap 3600 s) (`token.rs:8-12`, `token.rs:294-322`,
  `config.rs:17`, `config.rs:69-71`, `token.rs:36`). Single use is enforced by the fsynced ledger
  under `flock` (`ledger.rs:91-111`). **[K5, K6, K7 replay]**
* Even a valid token cannot get past: the disable file (`approval.rs:209-211`), the never-approvable
  actions `kill` and `rotate` (`lib.rs:58-61`, `lib.rs:112`, `approval.rs:212-215`), the admin-safety
  rule (`approval.rs:219-221`, `safety.rs`). Targets named like `hades`, `key`, `keys`, `token`,
  `secret(s)`, `ssh`, `root`, `admin` are never approvable (`safety.rs:268-293`), and a plan
  containing `keygen` is refused (`safety.rs:220`). **HADES can never approve its own key change.**
* An approval executes nothing: the gate is dry-run only (`lib.rs:41-47`, `approval.rs:150-154`,
  `main.rs:184-190` prints `"executed": false`). The holder of the seed can let a *caller*
  (for example `gaia heal-tick`) do one scoped, safety-checked restart. That is the blast radius.
* Signatures are domain-separated: `galion-hades-gate/approval/v1\n` (`token.rs:28`), checked with
  `verify_strict` (`token.rs:271-273`), weak public keys refused (`keys.rs:65-73`).

## 2. Key generation on the owner's device

```
hades-gate keygen --seed ~/hades/admin.seed --pub ~/hades/admin.pub     # owner device only
# -> {"admin_fingerprint":"<16 hex>","ok":true}
```

* 32 bytes from `/dev/urandom` (`keys.rs:84-94`); the seed buffer is zeroized (`keys.rs:123`,
  `keys.rs:126`).
* Seed file mode **0600**, public file **0644**, both `create_new`: keygen **never overwrites**
  (`keys.rs:96-115`, `keys.rs:117-134`). **[K1, K2]**
* Only the fingerprint is printed (first 16 hex of SHA-256 of the public key; `keys.rs:136-140`,
  `main.rs:89-96`). No key material is printed (`keys.rs:10`). **[K1]**
* The tool refuses to read or write key files under `/opt/galion/galion-light/secrets` and
  `/root/galion-secrets` (`keys.rs:26-41`, `keys.rs:44-46`, `keys.rs:97-99`). (Cited only; the box
  test did not touch those paths.)
* The binary builds for x86_64 and aarch64 musl and keygen works under qemu-aarch64 (slot 209,
  `swarm/hades-rustflags-guard` b187398), so an aarch64 Linux or Android owner device can run it.
  It needs only `/dev/urandom` and a writable directory.
* What the code does **not** do: no passphrase, no encryption at rest, no hardware-token support.
  The seed is a plain `ed25519-seed <64 hex>` line (`keys.rs:7-8`). Protection at rest is the
  device's job (disk encryption, screen lock). See D-KC-2.

## 3. Public key only on the host

* The gate config names `admin_pubkey`, `ledger` and `disable_file`. All three are required and
  unknown keys are errors (`config.rs:4-13`, `config.rs:38-73`). Documented example paths:
  `/etc/galion/hades/admin.pub`, `/var/lib/galion/hades/ledger.jsonl`, `/etc/galion/hades/DISABLED`
  (`config.rs:7-10`). None of these exist on the VPS today.
* The gate loads **only** the public key (`approval.rs:145-148`). A seed file given as
  `admin_pubkey` is refused with `not a ed25519-pub file`. A public file given to `mint` as the seed
  is refused with `not a ed25519-seed file` (`keys.rs:51-55`). **[K4]**
* The gate never needs the seed, and no gate-side command reads one: `check`, `serve`, `status`,
  `disable` and `enable` take `--config` only (`main.rs:6-11`, `main.rs:105-130`). Only `keygen`
  and `mint` touch a seed (`main.rs:13-16`, `main.rs:136`). **[K10: 0 seed files on the gate side]**
* **Fingerprint check after install:** compare `hades-gate status --config C`
  (`main.rs:106-111`, `main.rs:232-240`) with the fingerprint keygen printed on the owner device.
  They must match. **[K3: 29b4c69062704459 both sides]**
* **The public file is the trust anchor and nothing protects it in code.** `load_public` checks
  format and weak points, but not owner or mode (`keys.rs:43-59`, `keys.rs:65-73`). Anyone who can
  replace `admin.pub` can install their own key and approve their own plans (still limited by §1).
  The box test showed a swapped file is accepted silently by the next fresh process. **[K7]**
  The file and its directory must be root-owned and not writable by any service or agent user.
  See D-KC-4 and D-KC-8.

## 4. Minting (owner side)

* `hades-gate mint --seed S --action A --target T --plan-file F --out TOK [--ttl S]` (`main.rs:135-170`).
  Default TTL 300 s (`main.rs:138-141`). The token goes to a new file, mode 0600, `create_new`
  (`main.rs:157-167`). Stdout shows only the claims (action, target, plan SHA-256, nonce, Zagreb
  times), never the token (`main.rs:168`, `main.rs:172-182`). **[K5: token file 600]**
* A longer TTL than the gate allows is refused at check time with code `lifetime`
  (`token.rs:302-305`). **[K6: --ttl 7200 refused]**
* `mint` reads the seed without checking its mode (`keys.rs:79-82`). See D-KC-8.
* Not in base 0d68f46: `gaia tokens request/mint/status` (slot 175, `swarm/gaia-loop-decisions`
  b40208a, `crates/gaia/src/tokens.rs:16`, `docs/gaia-heal-loop.md` D-L3) mints batches of 1-3
  tokens on the admin side. Those pre-minted tokens then wait in a token directory on the gate
  host. They are bearer approvals until they expire (≤ 3600 s) or are consumed, so that directory
  is custody-relevant too (D-KC-10).

## 5. Rotation (planned)

There is **one** trusted key per gate (`config.rs:22-31`, `approval.rs:102-107`). There is no key
list and no overlap window, and `rotate` can never be approved through HADES (`lib.rs:60-61`).
Rotation is a manual admin act on two machines:

1. Owner device: `hades-gate keygen --seed admin-NEW.seed --pub admin-NEW.pub` (new paths; keygen
   never overwrites, `keys.rs:117-134`). Note the fingerprint.
2. Gate host (root): copy only `admin-NEW.pub` next to `admin.pub` and move it into place
   atomically (`mv`). The config path stays the same.
3. **Restart `hades-gate serve`.** `serve` loads the key once at start (`main.rs:243-244`) and
   keeps it for its lifetime. **[K7: after the swap the running server still approved an old-key
   token (200 WouldApply) and refused a new-key token as `forged`, and `/health` still showed the
   old fingerprint, until restart.]** CLI callers (`hades-gate check`, `gaia heal-tick` via
   `load_approver`, `crates/gaia/src/live_heal.rs:139-141`, `:381`) load the key per run, so they
   switch at once. **[K7: a fresh `check` approved the new-key token and refused the old-key one.]**
4. Verify: `hades-gate status` fingerprint = new fingerprint. Tokens minted with the old key are now
   refused as `forged` (`token.rs:301`, `approval.rs:230-233`). A token refused as forged is
   **not** consumed (`approval.rs:230-233` returns before `ledger.consume`). **[K7: b1 was
   refused under the old key and approved once the right key was loaded]**
5. Owner device: destroy the old seed (or archive it offline, D-KC-3).

The ledger keeps working across rotation (nonces are 128-bit random, `main.rs:150`). But consumed
lines record no key fingerprint (`ledger.rs:99-108`), so the ledger alone cannot show which key
approved what. **[K9: 0 fingerprint hits in the ledger]** Record the rotation time with the
fingerprints in CALL-RECORD (D-KC-7).

## 6. Revocation: the disable file

* `hades-gate disable --config C` writes the disable file. `enable` removes it. **Neither needs a
  key or token** (`main.rs:112-129`). `serve` also stops on `POST /v1/stop` without a token,
  loopback only (`main.rs:246`, `main.rs:291-293`). **[K7 /v1/stop, K8]**
* The disable file is checked **first** on every request (`approval.rs:209-211`) and again just
  before the nonce is consumed (`approval.rs:234-238`). It is read per request, so a running
  `serve` obeys it without a restart. **[K8: running serve → 403 `disabled`, fresh check → exit 2
  `disabled`, `/health` `"disabled":true`]**
* Fail closed: if the gate cannot tell whether the file exists, HADES counts as disabled
  (`approval.rs:168-173`). **[K8b: disable dir mode 000 → `disabled`]**
* **The disable file is global, not per key or per token.** A token that was valid before
  `disable` is **still valid after `enable`** if it has not expired. Refused requests never
  consume the nonce (`approval.rs:209-211` returns before `ledger.consume`). **[K8: token b4 refused
  while disabled, then approved after enable]** So after disabling for a compromise, either keep
  HADES disabled until every token the bad key could have minted has expired (at most
  `max_ttl_secs`, hard cap 3600 s, plus 60 s clock skew, `token.rs:34-36`), or rotate the key
  first (rotation kills all old-key tokens at once, §5).
* No command revokes a single token or nonce (`main.rs:33`, command list). See D-KC-10.
* The file is written with the process umask (`main.rs:115`, observed 0644 **[K8]**). Whoever can
  write its directory can delete it, which re-enables HADES. That directory must be root-only
  (D-KC-4).

## 7. Runbooks

### 7.1 Seed lost (no compromise suspected)
1. Nothing breaks on the host: the gate only verifies. No new approvals can be minted, so heal
   approvals stop. That fails safe.
2. Make a new pair on the owner device and rotate (§5 steps 1-4).
3. Any tokens already minted with the lost seed still work until they expire (≤ TTL). Since the
   seed is lost, not stolen, that is acceptable. Rotation also ends them at once.

### 7.2 Seed compromised (or the owner device is lost or stolen)
1. **Gate host, at once:** `hades-gate disable --config C` (no key needed, `main.rs:112-119`).
   This takes effect on the next request, including in a running `serve` (§6). **[K8]**
2. Owner (on a clean device): new key pair; rotate the public key on the host (§5 steps 2-4,
   including the `serve` restart).
3. Keep HADES disabled until rotation is verified (`status` shows the new fingerprint, and an
   old-key token gets `forged`). Only then `enable`. If rotation must wait, keep it disabled at
   least `max_ttl_secs` + 60 s after the last moment the attacker could have minted.
4. Review the ledger: every `consumed` line since the suspected time (action, target, plan
   SHA-256, nonce, Zagreb iat/exp; `ledger.rs:99-108`). The ledger never holds token strings
   (`ledger.rs:11-13`, refused tokens only as a 16-hex SHA-256 prefix, `ledger.rs:139-143`).
   **[K9: 0 of 11 token strings in the ledger]** Compare with the restarts the nerve recorded.
5. If gaia token batches (D-L3) exist on the host, delete the token directory contents. Rotation
   already made them useless.
6. If the same key also signs gold images (D-IF1 option a), §8 applies too, and that part is
   **not** fixed by the disable file.

### 7.3 Gate host compromised
* An attacker with root on the gate host does not need the seed. They can replace `admin.pub`
  (§3), delete the ledger (a new empty one is created on open, `ledger.rs:67-78`, which re-allows
  unexpired tokens), or remove the disable file. HADES protects nothing against a host root. The
  custody goal is the reverse direction: **a host compromise must not leak the seed**. It cannot,
  because the seed is never on the host (§3).
* Recovery: rebuild or clean the host, reinstall `admin.pub` from the owner device, and check the
  fingerprint (§3). Rotation is only needed if the seed was ever on that host.

## 8. What the gold-image key (D-IF1) means for HADES custody

Slot 204 (`swarm/galion-os-image-format` f35f735, local galion-os-repo) signs Galion OS gold images
with Ed25519 in the **same file format** (`ed25519-pub` / `ed25519-seed`, same fingerprint)
(`docs/galion-os/gold-image-format.md:84-109`; `crates/galion-admin/src/image.rs:11-12`, `:39-41`,
`:119-120`). Its signature domain is `galion-os/gold-image/v1\n` (`image.rs:37`) instead of
`galion-hades-gate/approval/v1\n` (`token.rs:28`). So a HADES approval never verifies as an image
signature and the reverse also holds (unit test `hades_approval_signature_never_verifies`,
`image.rs:1040`). Its owner decision D-IF1 is: (a) reuse the HADES admin key or (b) a separate
image-signing key (`gold-image-format.md:181-183`). Either works with no code change.

What each choice does to the custody in this document:

| Property | HADES token (this doc) | Gold-image signature (slot 204) |
|---|---|---|
| Lifetime of one signature | ≤ `max_ttl_secs`, hard 3600 s (`token.rs:36`) | **no expiry**: the manifest has no exp (`image.rs`: no expiry/serial field; anti-rollback is open item D-IF3) |
| Revocation | disable file on the gate host (§6) | **none** in v1. A signed image stays valid on every device that trusts the key, until that device gets a new public key |
| Single use | ledger nonce (`ledger.rs:91-111`) | no, any number of installs |
| Effect of rotation | instant at the host (one file + `serve` restart) | needs a new public key on **every device** and re-signing of every image still in use |
| Where the seed is used | owner device, `mint` | wherever images are signed: builder location is D-IF4 (dev-side example vs `galion-admin` subcommand) |

* **Option (a), shared key:** one seed to protect. But a seed compromise becomes **permanent**
  device-level trust: the attacker can sign gold images that never expire and that the HADES disable
  file cannot stop. Every HADES rotation also forces an image-trust rotation on all devices, so
  routine HADES rotation (D-KC-5) gets expensive. If image signing ever moves to CI (D-IF4), the
  HADES seed would leave the owner's device, which breaks §2.
* **Option (b), separate key:** two seeds to keep offline, but each blast radius stays separate.
  HADES keeps cheap, frequent rotation and the image key can be held in colder storage. Runbook
  7.2 then covers HADES only, and the image key gets its own loss/compromise runbook (not written
  here, since its revocation story is D-IF3).
* In both options, the domain separation above stops a signature *for one purpose* from being
  replayed for the other. What changes is who holds the key, not cross-protocol forgery.

## 9. Owner decisions (for Slavik, none taken here)

| ID | Question | Options and evidence |
|---|---|---|
| D-KC-1 | Which owner device holds the HADES seed? | admin phone (Galion OS, aarch64, keygen proven under qemu by slot 209) / a laptop / an offline machine. Must not be the VPS or any gate host (§3). |
| D-KC-2 | Seed protection at rest | device disk encryption only (today's code, plain hex file 0600, `keys.rs:7-8`) / passphrase-wrapped seed (code change) / hardware key (code change, new deps). |
| D-KC-3 | Seed backup | no backup (loss = rotate, runbook 7.1: safe, costs a host visit) / one offline paper or USB copy (a second thing to guard). |
| D-KC-4 | Host paths and ownership at install | e.g. `/etc/galion/hades/` root:root 0755, `admin.pub` 0644 root, disable-file directory writable by root only, ledger dir root 0700. Who may run `enable`? (`main.rs:120-129` needs no key, only write access.) Part of the install cutover, not done here. |
| D-KC-5 | Rotation cadence | on events only (loss, compromise, device change) / scheduled (e.g. every 90 days). Cheap under D-IF1 (b), expensive under (a) (§8). |
| D-KC-6 | `serve` key reload | keep "restart after rotation" (today, **[K7]**) / re-read `admin_pubkey` per request or on SIGHUP (code change; closes the window where a running server still trusts the old key). |
| D-KC-7 | Key fingerprint in the ledger | leave as is (`ledger.rs:99-108`, fingerprint not recorded, **[K9]**) and log rotations in CALL-RECORD / add `admin_fingerprint` to every consumed and refused line (code change). |
| D-KC-8 | File-mode checks in code | none (today: `keys.rs:43-59` reads any mode) / refuse a group- or world-writable `admin.pub` or directory, and a group- or world-readable seed (code change, needs tests). |
| D-KC-9 | = D-IF1: same key for gold images? | (a) shared / (b) separate. Custody effects in §8. |
| D-KC-10 | Revocation granularity | global disable only (today, tokens survive disable→enable, **[K8]**) / ops rule "stay disabled ≥ max_ttl + 60 s after a compromise, or rotate first" (no code) / `hades-gate revoke --nonce` writing a consumed line (code change). Also: where the D-L3 token directory lives and who can read it. |
| D-KC-11 | `max_ttl_secs` for production | 900 default (`config.rs:17`) / lower (shorter compromise window, more minting) / up to 3600 hard cap. |

## 10. Not covered

* Installing HADES on the VPS (a separate owner-approved cutover; nothing installed here).
* Any HADES mode change (there is only dry-run, `lib.rs:41-47`).
* The image key's own runbook (depends on D-IF1/D-IF3).
* SSH, galion-control or other secrets (never handled by HADES, `adapter.rs:20-33`,
  `safety.rs:268-293`).
