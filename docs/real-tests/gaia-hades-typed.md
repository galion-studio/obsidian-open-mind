<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: gaia-hades-typed (wave 4 slot 152)

All times Europe/Zagreb CEST +02:00, 2026-10-21. Box ports 20220-20223 (slot 152 range). Throwaway keys in
temp dirs; no token or key is printed. VPS: read-only (`systemctl`, loopback GETs). Cortex is not deployed on
the VPS; the nerve (127.0.0.1:8670 `/state`) was used to observe it.

## 1. gaia typed-plan -> hades-gate (release binaries, CLI + HTTP), 07:06:24-07:07:30

```
GALION_REAL=1 HADES_GATE_BIN=$T/release/hades-gate GAIA_TYPED_PORT=20220 \
  cargo-slot test --release -p gaia --test real_typed_plan -- --nocapture
```
```
REAL hades-gate serve pid=4049592 on 127.0.0.1:20220
REAL gaia typed-plan rollback --intensity 100: sha256=94f69bbbc9a0a300b23448d14d5c6f1ae16b5b29a8fcc50c80c76c074b18332d steps=6
REAL gaia typed-plan rollback --intensity 0:   sha256=94f69bbbc9a0a300b23448d14d5c6f1ae16b5b29a8fcc50c80c76c074b18332d steps=6
REAL gaia typed-plan heal-apply --intensity 0:   sha256=d624865979c6b8d9b812b003d9f54918087043e919d339bc9f915ed93c1d0764 steps=4
REAL gaia typed-plan heal-apply --intensity 50:  sha256=caefd182816bdf8e45923821475a8fce07c4f04357426364d0d250c8c23c009a steps=4
REAL gaia typed-plan heal-apply --intensity 100: sha256=d3077c7fc60acae8adebd2c55673ca3f0d48e762862a16762c7383efa7594f9b steps=4
REAL gaia typed-plan heal-apply --intensity 255: sha256=d3077c7fc60acae8adebd2c55673ca3f0d48e762862a16762c7383efa7594f9b steps=4
REAL prose rollback plan on typed path: exit=2 {"code":"typed-plan","decision":"Refused",...,"reason":"typed plan must start with the \"hades-plan/v1\" line"}
REAL typed: cases=6 hash_eq=6 cli_WouldApply=6 http_WouldApply=6 replays_refused=12 cross_refused=2 prose_refused=1 not_expressible=3 not_hades=1 ledger_lines=33
test result: ok. 1 passed; 0 failed
```
`hash_eq`: gaia's `plan_sha256` equals `hades-gate plan-hash` of the `--out` file. Cross: a heal-apply token
for intensity 50 is refused for the intensity-100 plan and under action `rollback`. `--out` onto an existing
file exits 2 (never overwrites). teardown / kill-switch / secret-rotation-plan exit 4 `not-expressible` and
write no file; `repo-check` exits 4 `not-hades`. Server killed, port free after the test (asserted).

## 2. Red team stays 0 (release, 07:07:37-07:08:26)

```
GALION_REAL=1 HADES_TYPED_PORT=20221 HADES_REDTEAM_PORT=20222 HADES_CLI_PORT=20223 \
  cargo-slot test --release -p hades-gate --test real_typed --test real_redteam --test real_cli -- --nocapture --test-threads 1
```
```
REAL shell: 0 WouldApply / 25   file: 0 / 19   access: 0 / 14   allowverb: 0 / 10   unicode: 0 / 6   traversal: 0 / 8
REAL TOTAL WouldApply: 0 / 82          (prose path, POST /v1/approve)
REAL legit not approved: 0 / 9
REAL typed TOTAL WouldApply: 0 / 164   (typed path, raw + hades-plan/v1-wrapped, valid tokens)
REAL typed legit not approved: 0 / 9   (now with target/service `tuner`)
REAL check-typed token exit=0 {"approved":{"action":"heal-apply",...,"plan_sha256":"10cce9ac...","target":"tuner"},"decision":"WouldApply","executed":false}
REAL check-typed replay exit=2 {"code":"replayed","decision":"Refused",...}
REAL check-typed prose exit=2 {"code":"typed-plan","decision":"Refused",...}
real_cli 1/1, real_redteam 1/1, real_typed 1/1
```
In-process (gaia-hades tests/typed_gate.rs): prose 0/82 and typed 0/164 again, 20/20 typed plans approved once
(rollback + heal-apply x 5 intensities x 2 targets), every replay refused, prose bytes refused on the typed path.

## 3. heal-tick real test (release, 07:08:45-07:08:56)

```
GALION_REAL=1 GAIA_NERVE_BIN=$T/release/openmind-nerve \
  cargo-slot test --release -p gaia --test real_gaia_live --test heal_tick -- --nocapture --test-threads 1
```
`heal_tick` 10/10, `real_gaia_live` 1/1 (real openmind-nerve + gaia-dummy: kill, no-token / forged / replay
refused, 3 approved restarts, 4th capped, nerve read-back). This test's ports are fixed at 19233/19234
(slot 23, inside the heal-tick 19000-19999 guard, which the slot 152 range 20220-20229 is outside); both
were free before and after. heal-tick itself is unchanged (prose heal plan v2).

## 4. Real VPS data (read-only), 07:09:10-07:09:50

```
GALION_REAL=1 HADES_VPS_SSH=slavik-vps cargo-slot test --release -p hades-gate --test real_vps_inventory -- --nocapture
```
```
REAL vps nerve ts="2026-10-21T07:09:05+02:00" mode="live" cycle=6789
REAL vps units=152 deny-list accepts=142 refuses=10
REAL vps security units present=[firewalld, fail2ban, auditd, apparmor, nftables, iptables, ufw, ssh, cron] deny-list lets through=[firewalld, fail2ban, auditd, apparmor, cron]
REAL nerve units not installed on the VPS: ["galion-voice"]
REAL inventory (10): cloudflared console galion-desk galion-health galion-one hermes-rs link-watch-rs openmind-memory tuner tuner-quick
REAL typed restart plans for real units: 10/10
REAL inventory refuses deny-list leaks: 5/5
REAL corpus legit plans naming a unit not on the VPS: 0/9      (slot 124: 5/9, all galion-tuner)
```
Extra probes 07:09:41: `tuner.service` active, `galion-tuner*` 0 units / 0 unit files, `*hades*` 0 units
(HADES not deployed, mode untouched), tuner 127.0.0.1:8650/health 200, 127.0.0.1:8420/health 200.
Nerve `/state` 07:09:35 cycle 6790, mode live: 11 healthy (openmind-memory, hermes-rs, link-watch-rs,
galion-desk, tuner, galion-one, console, galion-health, tuner-quick, lxc-host, galion-nerve), 2 degrading
(galion-one-mouth, cloudflared; pre-existing), 2 absent (galion-voice, gaia).

Slip: one probe piped the nerve `/state` body into a temp file `/dev/shm/.x152` on the VPS to count its bytes
and removed it in the same command (public health JSON, no secrets). No other VPS write.

## 5. Gates

* `cargo fmt --check` ok; `cargo clippy --workspace --all-targets -- -D warnings` 0 warnings.
* `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`: 272 suites, 1254 passed,
  0 failed, 6 ignored.
* gaia-hades 12 unit + 6 integration; hades-gate typed 6 + typed_redteam 2 + unit 23.
* Release musl static: x86_64 and aarch64 (rust-lld) for gaia, gaia-hades, hades-gate; `cargo check`
  aarch64-linux-android and armv7-unknown-linux-musleabihf ok.

## 6. Re-run after restart (2026-10-21 07:55-08:02 +02:00)

The first slot 152 agent was interrupted at 07:29 before committing. A new executor re-ran every gate on the
same tree (no code change) before committing:

* fmt ok; clippy `--workspace --all-targets -D warnings` 0 warnings.
* `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`
  (hermes-tools added to the local exclude list at 07:54, it runs the sandbox): 266 suites, 1242 passed,
  0 failed, 6 ignored. One earlier run hit `room::tests::default_and_fixture_do_not_open_a_socket` (it counts
  process-wide socket fds, so a parallel test's socket races it; crate untouched here): 30/30 with
  `--test-threads 1`, and the full `--no-fail-fast` re-run above was green.
* 07:56:56 real_typed_plan 1/1: same 6 hashes as section 1, hash_eq=6, cli/http WouldApply 6/6,
  replays_refused=12, cross_refused=2, prose_refused=1, not_expressible=3, not_hades=1, ledger_lines=33.
* 07:57:13 real_redteam 0/82 (legit 0/9 not approved), real_typed 0/164 (legit 0/9 not approved, target
  `tuner`), real_cli 1/1; check-typed dry-run 0 / token 0 / replay 2 / prose 2.
* 07:57:25 heal_tick 10/10, real_gaia_live 1/1. Ports 20220-20223 and 19233/19234 free afterwards.
* 07:57:31-08:00:53 real_vps_inventory 1/1 (nerve cycle 6892, mode live): units 152, deny-list lets through
  firewalld/fail2ban/auditd/apparmor/cron, inventory refuses 5/5, real restart plans 10/10, corpus plans
  naming a missing unit 0/9.
* 08:00:53 VPS read-only: `tuner` active, `galion-tuner*` 0 units, `*hades*` 0 units (HADES not deployed,
  mode untouched), 127.0.0.1:8650/health 200, 127.0.0.1:8420/health 200. 08:01:00 nerve `/state` cycle 6892
  (body read over ssh to the box, nothing written on the VPS): status fields 17 healthy, 4 degrading,
  3 absent (all nested status fields counted). Cortex still not deployed.
* musl release x86_64 + aarch64 (rust-lld) for gaia, gaia-hades, hades-gate: static ELF; `cargo check`
  aarch64-linux-android + armv7-unknown-linux-musleabihf ok; no-python ok; repo-check 1397 files, 0 errors,
  49 warnings, PASS.
