<!--
SPDX-License-Identifier: MIT
SPDX-FileCopyrightText: 3000 Galion / galion-studio
Copyright (c) 3000 Galion / galion-studio
-->
# Real tests: android-admin-pairing (wave-4 slot 319)

Rust pairing client `galion-admin-console client-*` (crate `galion-admin-console`,
module `client.rs`) against the slot-303 Rust console admin store
(`galion-console-serve --dist .. --ui rust --admin-store ..`, swarm/console-board-api-parity
9c0f9d1a) and against this crate's own `serve` (slot 307). Box ports 21890-21899.
No live change. §11: nothing touches a device; the client is what a phone browser is.

## CLI

```
galion-admin-console client-status|client-pair|client-resume|client-get PATH|client-forget
    --client-state DIR --console 127.0.0.1:PORT [--console-host console.galion.app]
    [--origin URL] [client-pair: --code-stdin | --code-file FILE] [--label L]
```
Exit: 0 ok, 1 runtime (503 store / 404 no gate / transport), 2 usage (non-loopback,
code on argv, no device secret), 3 not paired / refused, 5 locked (429).
Credentials: `<DIR>/client.creds` (dir 0700, file 0600, atomic); a group/other
readable file is refused. Output shows only `node-<8 hex>` (one-way hash) and cookie flags.

## 1. Box real test (GALION_REAL=1) — 2026-10-27 22:29 CET (+01:00)

Build the slot-303 console once (not in this branch):
```
cd /workspace/swarm/wt/console-board-api-parity   # 9c0f9d1a
CARGO_TARGET_DIR=/workspace/swarm/target/android-admin-pairing-303 cargo-slot build -j 2 --release \
  --target x86_64-unknown-linux-musl -p console-static --bin galion-console-serve
GALION_REAL=1 CONSOLE_SERVE_BIN=/workspace/swarm/target/android-admin-pairing-303/x86_64-unknown-linux-musl/release/galion-console-serve \
  cargo-slot test -j 2 --release -p galion-admin-console --test real_pairing_client -- --nocapture
```
Output (pass, 177 ms):
```
A status: exit 3 paired=false you=false local=none
B wrong code: exit 3 galion-admin-console: pair: refused (wrong code or unknown device)
C pair: exit 0 paired node=node-87543c62 device=issued cookie=HttpOnly:true Secure:true SameSite:Lax
D status: exit 0 paired=true you=true local=node-87543c62
E resume: exit 0 paired node=node-194c2f57 device=kept cookie=HttpOnly:true Secure:true SameSite:Lax
E rotated-out old token: exit 3 paired=true you=false local=node-87543c62
F locked: exit 5 galion-admin-console: pair: locked (too many attempts)
G ro-store: wrong x6 exits [3, 3, 3, 3, 3, 3]; right code exit 1 galion-admin-console: pair: console admin store unavailable (503)
H corrupt: exit 1 galion-admin-console: status: console admin store unavailable (503)
J own pair: exit 0 paired node=node-262a95a9 device=none cookie=HttpOnly:true Secure:false SameSite:Strict
real_pairing_client: all steps ok in 177 ms (ro-store step ran)
```
Also asserted: code never accepted on argv (exit 2); non-loopback `--console` refused (exit 2);
creds 0600; store holds sha256 of token + device only (raw values absent); authenticated
`/api/v1/services` JSON and Rust `/services` page ("Edge gateway") with the cookie, 401 without;
old token valid after 1 resume and rotated out after 4 more (MAX_TOKENS 4); device resume
works under the code lock; corrupt store untouched; 0644 creds refused; forget; 307 compat:
same node id on client and `devices`, `/api/v1/admin/status` 200, single-use code refused on
reuse, resume refused (no device secret), audit chain ok and free of the token; no secret in any
CLI stdout/stderr.

**G = F-313-1 seen from the client:** with a read-only store dir, six wrong codes give 401 each
(never 429) and the right code gives 503 — the code oracle reported by slot 313 is real on
9c0f9d1a. The client reports it honestly (exit 1, "store unavailable"); the fix belongs in
console-static `admin.rs` (slot 313 proposal), not in the client.

## 2. VPS throwaway run (real server, non-root) — 2026-10-27 22:31:59 CET (+01:00)

Static x86_64 musl binaries copied to `/var/tmp/sandbox-test-pairclient-20261027223149`
(client sha256 prefix e901d42ffd9f00ac; console 1c8318746a5ddb41 = slot 303/313 VPS binary),
console on VPS 127.0.0.1:21892 and client both run as `nobody` (runuser), then killed and removed
(listeners left 0, dir gone). Nothing live touched.
```
A exit 3   paired=false you=false local=none
B exit 3   pair: refused (wrong code or unknown device)
C exit 0   paired node=node-4e1d6ef4 device=issued cookie=HttpOnly:true Secure:true SameSite:Lax
D exit 0   paired=true you=true local=node-4e1d6ef4
E exit 0   /api/v1/services status 200 {"count":16,"active":12,"chill":7,"items":[{"id":"console",...
F exit 0   paired node=node-462da1a5 device=kept (resume)
creds 600 nobody; store 600 nobody; store contains raw token: 0
G exit 0   forgot local credentials
```

## 3. Live VPS read-only (GET only) — 2026-10-27 22:30:50 CET (+01:00)

`ssh -L 21893:127.0.0.1:8080 -L 21894:127.0.0.1:8670` (box side), no POST to live:
```
client-status --console-host console.galion.app -> status: unexpected 404 (exit 1)
client-status --console-host 127.0.0.1          -> status: unexpected 404 (exit 1)
GET /api/v1/health (Host 127.0.0.1)  200
GET /console (Host console.galion.app) 503
nerve /state 200 37581 B: cycle 25, mode live, grade D score 64, console "healthy" :8080, 2 open incidents
```
The live Rust console still has no `/api/v1/admin` (D-303-1 / D-313-1 not deployed), so phone
pairing is still broken and fails closed; the nerve still calls the console healthy (blind spot
N-307-1 / F-313-2). After this run the client maps that 404 to a clear
"console does not serve /api/v1/admin (404): admin store not deployed" (unit `no_gate_is_404`).

## Gates (box)
- `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` ok.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`: 389 suites, 2049 passed, 0 failed, 18 ignored.
- Crate: 22 unit (7 new client) + 2 real tests.
- x86_64 + aarch64 musl release build ok (598 880 / 525 856 B); clippy -D warnings for aarch64-linux-android and armv7-unknown-linux-musleabihf ok. aarch64 tests not run (no qemu on the box).
- `scripts/no-python.sh` ok.
