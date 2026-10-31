<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Approval-system record formats (Agent A, v1 — 2026-10-31 CET +01:00)

Status: DRAFT-FROZEN for agents C (galion.app card) and D (dry-run harness).
Code: branch `swarm/approval-core-20261031`, crate `crates/galion-self-modify`
(already on the self-modify PROTECTED list, so Hermes can never edit it).
If you need a change, write it in `A-core/FORMATS-REQUESTS.md`; do not fork the format.

## 0. Common rules
- Hashes: SHA-256, lowercase hex (64 chars). "fingerprint" of a file = sha256 of its exact bytes.
- Ed25519 keys: seed file = 64 lowercase hex chars (32 bytes) + optional "\n", mode 0600.
  Public file = 64 hex chars (32 bytes). key_id = first 16 hex of the public key hex
  (same as hermes-rs `self_modify::key_id`).
- base64url = RFC 4648 §5, NO padding.
- Times: unix seconds (i64) in signed bodies. Human strings are Zagreb RFC 3339 (+01:00), informational only.
- Envelope (reused from hermes-rs self_modify `sign_env`/`verify_env`):
  `{"<field>": "<inner JSON as a STRING>", "sig": "<128 hex>", "key_id": "<16 hex>"}`
  Signed message bytes = `<domain>` + "\n" + `<inner JSON string, exact bytes>`.
  The inner JSON's `"version"` MUST equal `<domain>`. Verifiers never re-serialise: they verify the
  exact string, then parse it. The sha256 of a record = sha256 of the inner string (not the file).

## 1. Stack signature over the proposal (signed by openmind-hermes STACK key, origin proof only)
File: `<entry>/proposal.stack-sig.json`, field `"body_json"`, domain `galion-sm/proposal-sig/v1`.
```
{"version":"galion-sm/proposal-sig/v1","id":"<entry id>",
 "proposal_sha256":"<sha256 of proposal.json bytes>","patch_sha256":"<sha256 of change.patch>",
 "base_commit":"<40 hex>","signed_at":<unix>}
```
Stack key: seed `/var/lib/openmind-hermes/keys/stack.seed` (openmind-hermes 0600), pub
`/etc/hermes-self-modify/stack.pub` (root 0444). Never used for anything else.

## 2. Test record (signed by NEW sandbox key, held only by galion-sm-sign)
Input, written by the test runner (galion-sm-test, UNSIGNED, untrusted): `<entry>/test/result.json`
```
{"version":"galion-sm/test-result/v1","id":"..","base_commit":"..",
 "stages":[{"name":"build","exit":0,"log":"logs/build.log"}, ...],
 "artifact":"artifact"}            // paths relative to <entry>/test/, no "..", no "/" prefix
```
The signer re-hashes `test/artifact` and every log itself (regular files only, O_NOFOLLOW), checks
the stack signature, and writes `<entry>/test/record.json`, field `"record_json"`, domain
`galion-sm/test-record/v1`:
```
{"version":"galion-sm/test-record/v1","id":"..","proposal_sha256":"..","proposal_sig_sha256":"<sha256 of stack sig inner body_json>",
 "patch_sha256":"..","base_commit":"..",
 "artifact":{"name":"artifact","bytes":N,"sha256":".."},
 "stages":[{"name":"build","exit":0,"ok":true,"log_sha256":"..","log_bytes":N}],
 "verdict":"pass"|"fail","why":"..","signed_at":<unix>,"signer":"galion-sm-sign"}
```
verdict = pass only if there is >= 1 stage and every stage exit == 0.

## 3. Dry-run record (signed by the same NEW sandbox key)
Input from agent D's harness (UNSIGNED): `<entry>/dryrun/result.json`
```
{"version":"galion-sm/dryrun-result/v1","id":"..","artifact_sha256":"<sha256 of the binary it ran>",
 "port":19xxx,                       // must be inside GAIA_TEST_PORTS 19000-23999
 "model_calls_real":0,               // MUST be 0 (fake model service only; no OpenRouter)
 "checks":[{"name":"health","ok":true,"detail":"..","log":"logs/health.log"}]}   // log optional
```
The signer verifies the test record (its own key, verdict pass), re-hashes `test/artifact`, requires
`artifact_sha256` == tested artifact, re-hashes logs, writes `<entry>/dryrun/record.json`, field
`"record_json"`, domain `galion-sm/dryrun-record/v1`:
```
{"version":"galion-sm/dryrun-record/v1","id":"..","test_record_sha256":"..","artifact_sha256":"..",
 "base_commit":"..","port":19xxx,"model_calls_real":0,
 "checks":[{"name":"health","ok":true,"detail":"..","log_sha256":"..|null"}],
 "verdict":"pass"|"fail","why":"..","signed_at":<unix>,"signer":"galion-sm-sign"}
```

## 4. Owner approval (Slavik's device) — THE card signs this
Body (galion.app builds it as a STRING once, shows it, and keeps the exact bytes), domain
`galion-sm/owner-approval/v1`:
```
{"version":"galion-sm/owner-approval/v1","id":"..","artifact_sha256":"..","base_commit":"<40 hex>",
 "test_record_sha256":"..","dryrun_record_sha256":"..",
 "nonce":"<32 lowercase hex = 16 random bytes, fresh per card>",
 "issued_at":<unix>,"expires":<unix>}          // expires - issued_at <= 1800 (30 min), > 0
```
Message M = `galion-sm/owner-approval/v1` + "\n" + body string.

File `<entry>/owner/approval.json`, one of two schemes:

(a) plain Ed25519 key file:
```
{"scheme":"ed25519","approval_json":"<body string>","sig":"<128 hex Ed25519 over M>","key_id":".."}
```
(b) WebAuthn / passkey, ES256 (P-256):
- challenge given to `navigator.credentials.get` = the 32 raw bytes sha256(M)
  (so clientDataJSON.challenge = base64url(sha256(M))).
- `userVerification: "required"`, `rpId` = owner.pub `rp_id`.
```
{"scheme":"webauthn-es256","approval_json":"<body string>",
 "credential_id":"<b64url>","authenticator_data":"<b64url>","client_data_json":"<b64url>",
 "signature":"<b64url, DER ECDSA as returned by the authenticator>","key_id":".."}
```
Verifier checks: clientData.type == "webauthn.get"; clientData.challenge == base64url(sha256(M));
clientData.origin in owner.pub `origins`; clientData.crossOrigin != true;
authenticatorData[0..32] == sha256(rp_id); flags UP (0x01) AND UV (0x04) set;
credential_id == owner.pub credential_id (if owner.pub has one);
ECDSA P-256/SHA-256 verify over authenticatorData || sha256(clientDataJSON bytes);
high-S signatures are accepted (WebAuthn authenticators don't normalise), DER required.

## 5. Owner public key file `/etc/hermes-self-modify/owner.pub` (root 0444)
JSON, one of:
```
{"version":"galion-sm/owner-key/v1","scheme":"ed25519","public_key_hex":"<64 hex>"}
{"version":"galion-sm/owner-key/v1","scheme":"webauthn-es256","rp_id":"galion.app",
 "origins":["https://galion.app","https://one.galion.app"],
 "public_key_sec1_hex":"<130 hex, uncompressed 04||X||Y>","credential_id":"<b64url>"}
```
Legacy: a file that is just 64 hex chars = ed25519. Browsers' `getPublicKey()` returns SPKI DER;
for P-256 the SEC1 point is its LAST 65 bytes (fixed 26-byte prefix
`3059301306072a8648ce3d020106082a8648ce3d030107034200`); the verifier also accepts
`"public_key_spki_hex"` with exactly that prefix.
Fingerprint shown on the console for Slavik to compare with his device:
`sha256(raw public key bytes)` (32 bytes for ed25519, 65-byte SEC1 point for P-256), shown as
lowercase hex grouped in 4s, first 16 hex = short form. `galion-sm owner-fingerprint <file>` prints it.

## 6. Root deploy rules (galion-sm-deploy)
Refuses unless ALL hold: stack sig verifies with stack.pub over current proposal.json + change.patch;
test record verifies with sandbox pub, verdict pass, bound to the proposal/patch/stack sig;
dry-run record verifies with sandbox pub, verdict pass, model_calls_real==0, bound to test record;
owner approval verifies with owner.pub; its artifact_sha256 == test artifact == dry-run artifact ==
sha256(<entry>/test/artifact bytes now); its base_commit == proposal == test record; its
test/dryrun record hashes match; now <= expires; expires-issued_at <= 1800; issued_at <= now+120;
nonce is 32 hex and NOT in the ledger `/var/lib/galion-sm-deploy/used-nonces` (root 0600).
All four keys must be distinct. The nonce is written to the ledger BEFORE install (a failed deploy
burns the nonce: Slavik signs a fresh card).
Then: backup previous drop-in + note, install binary `/opt/galion/rust/bin/hermes-gateway-sm-<id>-<sha12>`
(create_new, 0755, re-hashed after write), write drop-in `99j-self-modify-approved.conf` atomically,
`systemctl daemon-reload`, ONE `systemctl restart hermes-rs`, poll `GET http://127.0.0.1:8642/health`;
on failure: restore previous drop-in (ours renamed `.rolled-back-<ts>`), daemon-reload, restart, re-check.
ntfy via the loopback relay `POST http://127.0.0.1:2586/` (Title/Priority/Tags headers) on success,
on refusal, and on rollback. Writes `<entry>/deploy.json`.

## 7. Entry layout and who writes what (summary)
```
<queue>/<id>/proposal.json, change.patch      hermes (existing hermes-self-modify/proposal/v1)
<queue>/<id>/proposal.stack-sig.json          hermes, `galion-sm stack-sign` (stack key)
<queue>/<id>/test/{result.json,artifact,logs/} galion-sm-test (no key)          [agent B runner]
<queue>/<id>/test/record.json                 galion-sm-sign `sign-test`  (sandbox-v2 key)
<queue>/<id>/dryrun/{result.json,logs/}       dry-run harness (no key)        [agent D]
<queue>/<id>/dryrun/record.json               galion-sm-sign `sign-dryrun` (sandbox-v2 key)
<queue>/<id>/owner/approval.json              galion.app card / `galion-sm owner-sign` [agent C]
<queue>/<id>/deploy.json                      root, galion-sm-deploy
```
Records are create-once: the signer refuses to overwrite an existing record (re-test = new entry id).
`key_id` in envelopes is informational only (verifiers use the configured public key); for
webauthn-es256 C may put the first 16 hex of the key fingerprint there.
Helper for C/D: `galion-sm owner-challenge --entry E --stack-pub P --sandbox-pub P [--ttl-s N]` prints
`{"approval_json": "<exact body string>", "webauthn_challenge_b64url": "<43 chars>"}` after verifying
both records pass; `galion-sm-deploy verify --entry E ...` is the read-only final check (burns nothing).
