<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# galion-admin — Galion OS admin CLI skeleton (plan L1)

Written 2026-10-21 08:35 CEST (+02:00), wave-4 slot 165. Skeleton only: nothing was installed,
deployed or flashed anywhere, and no phone was touched.

## What it is

`crates/galion-admin`: one Rust binary (one dependency, `libc`) that is the plan's L1 "Rust
`galion-admin`" for the sandbox, tunnel and audit parts. It is **device-independent**: the same
code builds for x86_64/aarch64 Linux (static musl), `aarch64-linux-android` and
`armv7-unknown-linux-musleabihf`. It has no Android-specific paths; the state directory and the
launcher path are inputs.

| Command | What it does | Plan goal |
|---|---|---|
| `sandbox start NAME --rootfs DIR [--net none\|lo] [--ro-root] [--bind-ro S:D] [--timeout S] [--nproc N] [--as-mb N] [--nice N] -- CMD...` | Starts a sandbox in the background through `galion-box run` (slot 144 launcher). Watches 500 ms for an early failure and reports it with the launcher's exit code and log tail. | Coexist, Secure |
| `sandbox stop NAME [--grace S]` | SIGTERM to the launcher (its pid 1 then kills the whole namespace), SIGKILL after the grace period. Re-checks the pid's kernel start time first, so a recycled pid is never signalled. | Secure |
| `sandbox status [NAME]` | `running` / `exited` (exit code or signal, Zagreb times) / `lost` (supervisor died without a record). | — |
| `sandbox probe` | Runs `galion-box probe` (read-only capability report). | — |
| `tunnel status [--config F]` | TCP-connects each local tunnel end listed in `tunnel.conf` (`name = 127.0.0.1:PORT`) and closes it; sends no bytes. Non-loopback IPs and host names are refused at parse time (no DNS, nothing leaves the device). | Anonymous |
| `audit tail [-n N] [--json] [--follow]`, `audit verify` | Hash-chained local audit log; `verify` names the first edited/removed/reordered line and prints the head hash. | Secure, Private |

### How a sandbox runs

```
galion-admin sandbox start NAME ...            (returns after the settle window)
  └─ galion-admin __supervise NAME             own process group; holds NAME.lock for its life;
       │                                       writes NAME.state; audits start / exit
       └─ galion-box run ... -- CMD            PR_SET_PDEATHSIG=SIGTERM: dies with the supervisor
            └─ pid 1 (init) ─ payload          galion-box: caller death kills the pid namespace
```

* **Liveness = the lock.** A sandbox is running exactly while its supervisor holds the
  `flock` on `sandboxes/NAME.lock`. The kernel drops it when the supervisor dies for any reason,
  so a stale pid file can never claim a sandbox is up. Two `start`s of one name cannot both run.
* **Lost supervisors are recorded.** If the supervisor is killed (`kill -9`, OOM, reboot), the
  next `status`/`start` records `sandbox.lost` once (under the lock) with `box_alive=` evidence.
  Proven on the VPS: supervisor SIGKILL → all 3 sandbox processes gone in 3-57 ms.
* **Fail closed.** No `start` if the newest audit record is damaged (the start could not be
  audited). `stop` still works then: stopping is the safe direction.
* **No host env.** Children get only `PATH` and `TZ`; galion-box itself never passes host env
  to the payload.

### Files (state dir: `--state`, else `$GALION_ADMIN_HOME`, else `$HOME/.galion-admin`)

All directories are created 0700 and a group/world-writable state dir is refused. Files are
0600: `audit.jsonl`, `sandboxes/NAME.{state,lock,log}`, optional `tunnel.conf`.

### Audit record

```
{"v":1,"seq":2,"ts":"2026-10-21T08:32:38+02:00","uid":995,"action":"sandbox.start","target":"v02",
 "result":"launched","detail":"cmd=galion-box argc=3 box_pid=22992","prev":"<64 hex>","hash":"<64 hex>"}
```

`hash` = SHA-256 of the line up to `,"hash":`; `prev` = previous line's hash (genesis = 64 zeros).
Actions: `sandbox.start` (launched / failed), `sandbox.exit` (exit:N / signal:N),
`sandbox.stop`, `sandbox.lost`, `sandbox.probe`, `tunnel.status`.
**Payload arguments are never logged**, only the program's base name and the argument count
(`cmd=curl argc=3`), so a token on a command line cannot reach the log (tested). Times are local
with an explicit offset (never `Z`); the DST switch on 2026-10-25 is covered by a unit test
(`02:59:59+02:00` → `02:00:00+01:00`). Appends take an exclusive `flock` and `fsync`; 4 writers ×
25 records keep a valid chain (unit test), 4 parallel sandboxes give 12 chained records (real test).

Limits of the chain: it is tamper-*evident*, not tamper-proof. Someone with the same uid can
rewrite the whole file consistently, and dropping the newest lines is only visible against a head
hash remembered elsewhere (D-A2).

### Gold-image verification (slot 204)

`galion-admin image verify DIR --key PUBFILE [--json]` checks a signed gold image (format:
[docs/galion-os/gold-image-format.md](../galion-os/gold-image-format.md)): Ed25519 signature in
the hades-gate key format first, then size, mode and SHA-256 of every listed file; unlisted
files, symlinks and special files fail. Read-only; exit 0 ok, 4 failed/rejected, 1 key or I/O
error. Each run adds an `image.verify` audit record. The builder side (keygen, manifest, sign)
is `examples/gold_image.rs` and is not shipped on the device. Evidence:
[docs/real-tests/galion-os-image-format.md](../real-tests/galion-os-image-format.md).

### Not in this skeleton

* **No `su`/root path at all**: the binary exits 3 if run as uid 0 (D-A1).
* **HADES public enroll** (also in plan L1): not in this row; needs the hermes-core G9 and key
  location decisions.
* No seccomp/Landlock/cgroup layer (that is galion-box's D-S1/D-S3).

## Evidence (details in docs/real-tests/galion-os-admin-cli.md)

* Box: unit 25/25 on x86_64, 25/25 aarch64 under `qemu-aarch64`, 25/25 armv7 under `qemu-arm`;
  clippy `-D warnings` clean on 5 targets; real CLI test 9/9 with a fake launcher (no sandbox
  on the box).
* VPS, as throwaway user `sbx165` in `/var/tmp/sandbox-test-20261021-083025`, real galion-box:
  4 runs × 7/7 (probe, start/status/stop, timeout → exit:124, isolation report via the admin log,
  fail-closed missing rootfs → exit:125, supervisor kill → 0 leftovers, tunnel status), 0 leftover
  processes after each run, cleaned up.

## Findings worth acting on

1. **galion-box defaults are permissive inside the box** (from `inside-check` through the admin,
   without `--ro-root`): `write_root=ALLOWED`, `kill_init=ALLOWED`, `nested_userns=ALLOWED`. None of
   these reach the host, but nested user namespaces are the classic kernel-bug entry point. The
   admin passes policy through unchanged today (D-A3).
2. **armv7 `time_t`**: the libc crate still declares a 32-bit `time_t` on armv7 musl (deprecated
   alias, served by musl's time32 compat symbols). Only the zone offset uses it, so the effect is
   a wrong offset after 2038; dates are formatted from the 64-bit epoch. Revisit when libc moves.
3. **Exit codes are only known while the supervisor lives.** That is why the supervisor exists;
   if it is killed the record says `lost` (no exit code), which is honest but not complete.

## Owner decisions (Slavik) — not decided here

| # | Decision | Options | Recommendation / evidence |
|---|---|---|---|
| D-A1 | Root (`su`) path for L1 "su only for Galion paths" | (a) keep refusing root; add later a separate tiny `galion-admin-root` helper with a fixed allowlist of Galion paths, invoked through `su`, audited by the unprivileged CLI; (b) let galion-admin itself call `su` per command; (c) never on the daily phone | (a): keeps the big binary unprivileged; matches galion-box's D-S1 root mode. Needs the Phase 0/1 handset gate first. |
| D-A2 | Where the audit head hash is anchored | (a) device only (today); (b) send the head hash over the admin tunnel to the VPS/Hermes every N records; (c) sign each head with the HADES key once enrolled | (b) then (c): (b) detects truncation and wipe; (c) proves the device wrote it. Both need the tunnel / HADES decisions. |
| D-A3 | Default sandbox policy in the admin | (a) pass-through (today); (b) admin defaults `--ro-root --nproc 64 --as-mb 512 --timeout 3600` unless overridden; (c) presets like openmind-sandbox (`tool`, `build`) | (b) now, (c) once galion-box gains seccomp (D-S3); finding 1 above. |
| D-A4 | Repo layout | merge swarm/galion-os-sandbox-proto (galion-box) and this branch into one workspace; galion-admin keeps calling galion-box as a separate binary (process isolation) rather than linking it | Only conflict expected: root `Cargo.toml` members, `Cargo.lock`, `.gitignore`. |
| D-A5 | Plan §11 gate (same as D-S4) | the plan says no build before §11 answers | Confirm this skeleton may stay as the L1 starting point. |
| D-A6 | Licence: GPL test-only tools | `qemu-aarch64`/`qemu-arm` (GPL-2.0) were used on the dev box only to run tests; nothing linked or committed; not copied to the VPS this time | Same open question as slot 144. |
