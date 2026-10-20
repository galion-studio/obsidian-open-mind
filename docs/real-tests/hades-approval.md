<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: hades-approval (hades-gate admin approvals)

Run on the box 2026-10-20 between 19:03:25+02:00 and 19:03:29+02:00 (Europe/Zagreb), slot 02, port 19020.
Script: `scripts/real/hades-approval.sh` (release build, x86_64 musl, 615,120 bytes; aarch64 musl 529,040 bytes).
Rust version: `GALION_REAL=1 cargo test -p hades-gate --test real_cli` (port 19021): 1 passed.

Keys are throwaway Ed25519 keys made by `hades-gate keygen` inside a `mktemp -d` directory and deleted
at the end. Token strings and key material are never printed; only scope, nonce and times are.
No VPS probe: hades-gate is not installed or running on the VPS (see docs/hades-wire.md), so there is
no live service to compare against. Nothing on the VPS was touched.

## Output (verbatim)

```text

[2026-10-20T19:03:25+02:00] binary: /workspace/swarm/target/hades-approval/x86_64-unknown-linux-musl/release/hades-gate (615120 bytes)
{"admin_fingerprint":"a12ba5abaf981bdc","ok":true}

[2026-10-20T19:03:25+02:00] GET /health
{"admin_fingerprint":"a12ba5abaf981bdc","disabled":false,"mode":"dry-run","service":"hades-gate","ts":"2026-10-20T19:03:25+02:00"}
PASS health

[2026-10-20T19:03:25+02:00] 1. no token -> DryRun (default)
{"decision":"DryRun","dry_run":true,"executed":false,"reason":"no token: plan only","ts":"2026-10-20T19:03:25+02:00"} http=200
PASS dry-run

[2026-10-20T19:03:25+02:00] 2. old public token approved-test -> refused
{"code":"malformed","decision":"Refused","dry_run":true,"executed":false,"reason":"malformed token: prefix","ts":"2026-10-20T19:03:25+02:00"} http=403
PASS old-token

[2026-10-20T19:03:25+02:00] 3. mint a token (throwaway key) and approve the dry-run action once
{"action":"heal-apply","exp":"2026-10-20T19:05:25+02:00","iat":"2026-10-20T19:03:25+02:00","nonce":"3c5d7850c2ccd2f37dcdcc675bb441df","plan_sha256":"66615e425a965e3992379f331816b64041b57eb9ed7fa9836f00fb60ec2fc76a","target":"galion-tuner"}
{"approved":{"action":"heal-apply","exp":"2026-10-20T19:05:25+02:00","iat":"2026-10-20T19:03:25+02:00","nonce":"3c5d7850c2ccd2f37dcdcc675bb441df","plan_sha256":"66615e425a965e3992379f331816b64041b57eb9ed7fa9836f00fb60ec2fc76a","target":"galion-tuner"},"decision":"WouldApply","dry_run":true,"executed":false,"ts":"2026-10-20T19:03:25+02:00"} http=200
PASS approved
PASS not-executed

[2026-10-20T19:03:25+02:00] 4. replay the same token -> refused
{"code":"replayed","decision":"Refused","dry_run":true,"executed":false,"reason":"replayed token: nonce already consumed","ts":"2026-10-20T19:03:25+02:00"} http=403
PASS replay

[2026-10-20T19:03:25+02:00] 5. tamper: one payload hex digit changed (fresh token) -> refused
{"action":"heal-apply","exp":"2026-10-20T19:05:25+02:00","iat":"2026-10-20T19:03:25+02:00","nonce":"08714fa90d07714e8aa9bf3e36737d91","plan_sha256":"66615e425a965e3992379f331816b64041b57eb9ed7fa9836f00fb60ec2fc76a","target":"galion-tuner"}
{"code":"forged","decision":"Refused","dry_run":true,"executed":false,"reason":"forged or tampered token: signature does not verify","ts":"2026-10-20T19:03:25+02:00"} http=403
PASS tamper

[2026-10-20T19:03:25+02:00] 6. forged: token signed by a different throwaway key -> refused
{"code":"forged","decision":"Refused","dry_run":true,"executed":false,"reason":"forged or tampered token: signature does not verify","ts":"2026-10-20T19:03:25+02:00"} http=403
PASS forged

[2026-10-20T19:03:25+02:00] 7. wrong scope: valid token used for another target -> refused
{"code":"wrong-scope","decision":"Refused","dry_run":true,"executed":false,"reason":"token is scoped to a different target","ts":"2026-10-20T19:03:25+02:00"} http=403
PASS wrong-scope

[2026-10-20T19:03:25+02:00] 8. expiry: token with ttl 2 s, used after 3 s -> refused
{"action":"heal-apply","exp":"2026-10-20T19:03:27+02:00","iat":"2026-10-20T19:03:25+02:00","nonce":"489a69fab80e572c2c8360cfe9adf1b4","plan_sha256":"66615e425a965e3992379f331816b64041b57eb9ed7fa9836f00fb60ec2fc76a","target":"galion-tuner"}
{"code":"expired","decision":"Refused","dry_run":true,"executed":false,"reason":"expired token","ts":"2026-10-20T19:03:28+02:00"} http=403
PASS expired

[2026-10-20T19:03:28+02:00] 9. admin-safety: validly signed token for 'rm -rf' -> refused
{"code":"admin-safety","decision":"Refused","dry_run":true,"executed":false,"reason":"admin-safety: destructive command refused (rm -r)","ts":"2026-10-20T19:03:28+02:00"} http=403
PASS safety-destructive
{"code":"admin-safety","decision":"Refused","dry_run":true,"executed":false,"reason":"admin-safety: admin account/access change refused (usermod)","ts":"2026-10-20T19:03:28+02:00"} http=403
PASS safety-admin

[2026-10-20T19:03:28+02:00] 10. kill with a valid signature -> refused
{"code":"action","decision":"Refused","dry_run":true,"executed":false,"reason":"action \"kill\" can never be approved","ts":"2026-10-20T19:03:28+02:00"} http=403
PASS kill

[2026-10-20T19:03:28+02:00] 11. admin disables HADES (no token needed) -> everything refused
{"disabled":true,"ok":true}
{"code":"disabled","decision":"Refused","dry_run":true,"executed":false,"reason":"HADES is disabled by the admin (disable file present)","ts":"2026-10-20T19:03:28+02:00"} http=403
PASS disabled
{"disabled":false,"ok":true}
{"approved":{"action":"heal-apply","exp":"2026-10-20T19:05:28+02:00","iat":"2026-10-20T19:03:28+02:00","nonce":"d4b6a464bfc51e693ab319c3c9b5a6c0","plan_sha256":"66615e425a965e3992379f331816b64041b57eb9ed7fa9836f00fb60ec2fc76a","target":"galion-tuner"},"decision":"WouldApply","dry_run":true,"executed":false,"ts":"2026-10-20T19:03:28+02:00"} http=200
PASS re-enabled

[2026-10-20T19:03:28+02:00] 12. CLI check path: replay of t3 via the CLI -> exit 2
{"code":"replayed","decision":"Refused","dry_run":true,"executed":false,"reason":"replayed token: nonce already consumed","ts":"2026-10-20T19:03:28+02:00"} exit=2
PASS cli-replay
PASS cli-exit-2

[2026-10-20T19:03:28+02:00] 13. ledger (append-only, fsynced): event counts; no token strings stored
      2 consumed
      1 dry-run
     11 refused
PASS no-token-in-ledger
ledger mode 600
seed mode 600

[2026-10-20T19:03:28+02:00] 14. admin stop: POST /v1/stop
{"ok":true,"stopping":true}
PASS stopped
PASS port 19020 closed

[2026-10-20T19:03:29+02:00] result: ALL PASS
```

## fsync and lock evidence

`strace -f -e trace=openat,flock,write,fsync hades-gate check ...` on a first-use token (fd numbers
only, nothing else shown):

```text
fsync(5)            = 0   # new ledger file
fsync(6)            = 0   # its directory
flock(5, LOCK_EX)   = 0   # check-and-append under an exclusive lock
fsync(5)            = 0   # after appending the consumed nonce
```

## Unit and integration tests (hades-gate)

`cargo test -p hades-gate`: lib 17, approval 12, decide 5, journal 5, simd_free 1, real_cli 1 (skips without `GALION_REAL=1`).
Forged, tampered, malformed, expired, not-yet-valid, over-long TTL, wrong action/target/plan, replay (same
process, second process on the same ledger, 8 threads racing: exactly one wins), corrupt ledger fails
closed, kill/rotate/teardown refused with a valid signature, admin-safety refusals with a valid
signature, disable switch, GAIA adapter mapping.
