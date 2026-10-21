<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# omnode edge boot path (swarm/edge-boot)

Written 2026-10-20T19:42:00+02:00 (Europe/Zagreb). Branch `swarm/edge-boot` off
main 13a296ce. Code: `edge/openmind-node/src/{boot,pair,seccomp,util,doctor,main}.rs`.
Nothing installed on any device, no listener left running, VPS not touched.

## Commands

```
omnode boot [--state DIR] [--listen IP:PORT] [--allow-lan] [--code-ttl S]
            [--code-file F] [--new-code] [--require-seccomp]
omnode pair verify FILE --url URL [--now UNIX]      # FILE = output of `omnode pair`, - = stdin
omnode sandbox selftest                             # exit 0 only if the deny-list is enforced
```

`boot` runs in the foreground, needs no init system (works directly, under
tini, under a restart loop, or as PID 1: SIGTERM/SIGINT exit 0), and keeps all
state in `--state` (default `~/.config/openmind`):

| File | Mode | Content |
|---|---|---|
| `node.key` | 600 | device seed (existing identity code, never printed) |
| `node-state.toml` | 644 | public key, device id, salts, trusted networks |
| `boot.json` | 600 | boot_count, first_boot, last_boot (Zagreb +02:00/+01:00) |
| `pair-store.json` | 600 | `codeHash`, `codeExpires`, `tokenHashes`, `fails`, `lockedUntil` (hashes only) |

Order on every boot: listen address check (loopback unless `--allow-lan`) ->
identity (create on first boot, else load; no network is bound on boot) ->
seccomp self-test (`--require-seccomp` stops here when not enforced) -> pair
store (corrupt or unreadable = refuse to boot, never reset) -> open a pairing
code if nobody is paired and no code is open, or with `--new-code` -> bump
`boot.json` -> bind -> one JSON boot event on stdout -> serve.

HTTP (one connection at a time, head <= 8 KiB, body <= 4 KiB, 3 s per request,
no chunked bodies): `GET /health`, `GET /v1/node`, `POST /v1/pair {"code":".."}`
-> `200 {"ok":true,"token":..}` / `403 no` / `403 expired` / `429 later`,
`GET /v1/whoami` with `Authorization: Bearer` -> 200 / 401.

## Pairing: fail-closed, matching galion-one (swarm/one-probe b44cd4f)

Same rules as galion-one's fixed `token_ok`/`pair`: a missing, unreadable or
empty store pairs nobody and authorises nobody; codes are trimmed and
upper-cased (here also `-`/spaces dropped); every refusal is counted and the 5th
locks pairing for 15 minutes; a success resets the counter and keeps earlier
tokens; only SHA-256 hashes are stored, file mode 600. Stricter on purpose:
codes expire (`--code-ttl`, default 600 s), are single use, and are only
revealed as "expired" when the code itself was right. A new code does not lift
a lock, so a crash-restart cannot reset it. Main's galion-one still fails open
(`castle()` false -> `token_ok` true); that fix lives on swarm/one-probe, not here.

## `pair::verify` (docs/hermes-enrollment.md §4)

Pure function `verify(payload, signature_hex, expected_url, now)`; every check
of §4 except the Hermes-side nonce store: version/kind and unknown fields
(`deny_unknown_fields`), `|`/`,` injection, 32-byte lower-hex key that is a valid,
non-weak ed25519 point, device id and fingerprint recomputed from the key,
`verify_strict` over the recomputed canonical string (the sent `signed_string` is
ignored), `expires_unix` and `created` tied to the signed `expires`, 600 s window
with 120 s skew both ways, exact `enroll_url`, 96-bit hex nonce. No "unknown"
outcome: anything not provably right is an error. `network_trusted` and
`platform` are returned as `claimed_*` (shown, never used for decisions).

## Seccomp deny-list fixes

* x86_64 listed `431` as "process_vm_readv-like". 431 is `fsconfig` on every arch
  we build (post-5.1 syscalls share one number). It now sits, correctly named,
  in `DENIED_UNIFIED` together with the rest of the new mount API (open_tree 428,
  move_mount 429, fsopen 430, fsmount 432, fspick 433, mount_setattr 442; `mount`
  itself was already denied), io_uring 425-427 (its requests bypass seccomp) and
  pidfd_getfd 438. This list applies to x86_64, aarch64 and 32-bit ARM.
* `kexec_file_load` added next to `kexec_load` (x86_64 320, aarch64 294).
* New unit test: every number in the active list must equal libc's `SYS_*`
  constant for the target arch (run on x86_64, aarch64 and armv7 under qemu).
* `available()` and `doctor`'s userns probe now use `util::fork_probe` (the
  previous agent's unfinished helper).
* armv7 support (`AUDIT_ARCH_ARM`, EABI table, fail-closed fallback for other
  arches) is **swarm/ucpu 3e70ffb**, not duplicated here. On this branch alone
  `cargo check --target armv7-unknown-linux-musleabihf` still fails exactly as on
  main (`AUDIT_ARCH` undefined). A scratch merge of this branch with 3e70ffb is
  conflict-free and armv7 check + tests pass; `DENIED_UNIFIED` then covers armv7
  without touching ucpu's table.

## Open issues

* Seccomp enforcement on real aarch64/armv7 hardware is not demonstrated: qemu-user
  cannot load guest filters (the node correctly fails closed there).
* The pairing listener is plain HTTP; with `--allow-lan` the token crosses the LAN
  in clear. Loopback default. TLS or a signed challenge would be the next step.
* Hermes has no enrollment endpoint (design only); nothing sends the payload.
* One request at a time: a slow client can delay others by up to 3 s.
* `sandbox::tests::memory_limit_is_set_and_enforced` fails under qemu-user on main
  too (RLIMIT_AS under qemu), unrelated to this branch.
