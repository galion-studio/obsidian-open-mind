<!-- SPDX-License-Identifier: MIT -->
# Hermes device enrollment (design, v0, NOT implemented on Hermes, no listener)

| Field | Value |
|---|---|
| Written | 2026-10-18 21:05 CEST (+02:00), Europe/Zagreb |
| Status | **Design only.** No endpoint exists, no listener was started, no Hermes code or unit was changed. The device-side check (`pair::verify`, `omnode pair verify`) is implemented and tested in `edge/openmind-node`. |
| Related | `docs/phone-instance.md` §6, `edge/openmind-node/src/pair.rs` |

## 1. Goal

A phone or home device running `omnode` asks to join the stack. Hermes receives the
signed pairing payload, checks it, shows it to Slavik, and records the device only after
he approves. The VPS never reaches into the LAN; the device always dials out.

## 2. Transport (proposed)

- `POST /openmind/enroll` with `Content-Type: application/json`, body = the JSON that
  `omnode pair` prints: `{"payload": {...}, "signature": "<hex>", "signed_string": "..."}`.
  `signed_string` is informational; Hermes recomputes it and never trusts the sent copy.
- Alternative without any network path: Slavik scans the QR (short form
  `omp1|id|pk_b64url|net|caps|nonce|expires_unix|sig_b64url`) in the console, or pastes the
  JSON. Same checks.
- Body limit 16 KiB; rate limit 5 requests/min per source IP; HTTPS only, behind the
  existing Cloudflare tunnel hostname that Slavik approves.

## 3. Disabled by default

```toml
# hermes config (proposed key names)
[openmind.enroll]
enabled = false          # default; route is not mounted at all when false
path = "/openmind/enroll"
max_pending = 8          # pending (unapproved) requests kept
nonce_ttl_secs = 900     # nonces remembered for replay protection
```

When `enabled = false` the route does not exist (404 from the router, no handler
compiled into the request path). Turning it on needs Slavik's approval in STAGED.md.

## 4. Checks (all must pass; order matters only for cheap-first)

Implemented as the pure function `pair::verify(payload, signature_hex, expected_url, now)`
in omnode; Hermes should reuse it (copy, or depend on the crate once it is published):

1. `v == 1`, `kind == "openmind-pair"`, unknown JSON fields rejected.
2. No `|` in signed fields, no `|`/`,` in capabilities (canonical-string injection).
3. `public_key` is 32 bytes hex and a valid ed25519 point.
4. `device_id == "omn-" + sha256(pk)[..12 hex]`, `fingerprint` recomputed from pk.
5. ed25519 `verify_strict` over the canonical string
   `omnode-pair-v1|device_id|public_key|net|caps|arch|cores|ram_mb|enroll_url|nonce|created|expires`.
6. `expires_unix` (unsigned) must match the signed `expires` and `created = expires - 600 s`.
7. Not expired (`now <= expires_unix + 120 s` skew), not from the future.
8. `enroll_url` equals the URL Hermes is actually served at (prevents relaying a payload
   meant for another server).
9. `nonce` is 96-bit hex. **Hermes-side only:** nonce not seen in the last
   `nonce_ttl_secs` (store it before answering); same `device_id` with a different key is
   refused and flagged.

Unsigned fields (`network_trusted`, `platform`) are shown to Slavik as device claims,
never used for decisions.

## 5. Approval and storage

- Passing requests go to a pending list (max `max_pending`), shown in the console with
  the fingerprint `SHA256:xxxx-...` that Slavik compares against `omnode identity show`
  on the device. Nothing is stored as trusted until he clicks approve.
- On approve: a Provider row `{device_id, public_key, fingerprint, capabilities, arch,
  cores, ram_mb, network_hash, approved_at (Zagreb RFC 3339), approved_by}`. Optional
  reply: the stack-wide network salt (`[network] salt_hex`) so devices on the same home
  network produce the same hash.
- Reject or expiry: dropped, nonce kept until its TTL.
- Money/Revolut capabilities are never granted by enrollment (always denied).

## 6. Responses

| Case | HTTP | Body |
|---|---|---|
| route disabled | 404 | none |
| check failed | 400 | `{"ok":false,"error":"<reason from verify>"}` |
| replayed nonce | 409 | `{"ok":false,"error":"nonce already used"}` |
| accepted, waiting for owner | 202 | `{"ok":true,"status":"pending","fingerprint":"SHA256:..."}` |
| pending list full | 429 | `{"ok":false,"error":"too many pending"}` |

## 7. Test without any listener

```
omnode pair https://<hermes-host>/openmind/enroll --no-qr > pair.json
omnode pair verify pair.json --url https://<hermes-host>/openmind/enroll
```

`pair verify` runs every check from §4 except the nonce store, offline. Unit tests cover
acceptance, tampered capabilities, edited expiry, wrong fingerprint, expired payload,
wrong URL, a foreign public key and a malformed signature.
