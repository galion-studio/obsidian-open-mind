<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: link-env-perm-stage (slot 356, 2026-10-28 CET)

Branch `swarm/link-env-perm-stage` off owner-decisions-digest-2 5ff76fad.
Ports: none used (CLI tool; range 22260-22269 reserved, unused).

## Box
* `cargo fmt --check -p repo-check` ok; `cargo clippy -p repo-check --all-targets -- -D warnings` 0 warnings.
* `cargo test -p repo-check`: checks 13, deps_pin 12, perms 5 (new), real_perms 2 (skip w/o GALION_REAL), real_serde_order 3: all pass.
* Release builds: x86_64 musl 484,080 B, aarch64 musl 425,600 B (rust-lld; run under qemu-aarch64: `--only perms` PASS); armv7-musleabihf and aarch64-android `cargo check` ok.
* Repo self-eval with the new binary: 2120 files, 0 errors, 83 warnings, PASS (base 5ff76fad: identical 0/83).
* `scripts/no-python.sh`: ok.

## GALION_REAL=1 (05:12:41 and 05:13:31 CET)
`GALION_REAL=1 GALION_REAL_VPS=slavik-vps cargo test -p repo-check --test real_perms -- --nocapture`

`real_box_tree_fix_and_rollback` (real binary, throwaway tree):
```
666 -> exit Some(1)
error perms    publish/obsidian/link.env: secret-like file mode 666 is world-writable; any local user can rewrite it (fix: chmod 600)
600 -> exit Some(0)   ... PASS
rollback chmod 666 -> exit 1 again
```
Contents never printed (asserted).

`real_vps_stat_list` (read-only `find -printf` over ssh, piped to `--stat-list -`):
```
VPS stat list: 104107 entries
exit Some(1), errors 22, warnings 3
  error /opt/galion/galion-light/publish/obsidian/link.env
  ... (full list in docs/ops/link-env-perm-stage.md)
D-318-2 still unapplied: /opt/galion/galion-light/publish/obsidian/link.env flagged
```
The first run (05:12:41) gave 22 errors / 22 warnings; 19 warnings were name
false positives (tokenizer, *_tokens.json, .sol/.sql). Fixed (token followed by
`i`/`s` ignored, more source suffixes) and re-run: 22 / 3.

## VPS read-only probes (05:09 to 05:15 CET)
`stat` of link.env and parents (666 root:root 107 B, inode 33909, unchanged since
2026-10-21), `lsattr`, `systemctl show galion-one` (EnvironmentFiles empty, NRestarts 0,
active), `/proc/*/fd` and cwd scan (0 hits), grep of units/cron with
`--exclude=link.env` (0 hits), `grep -n` of play.sh / install.sh for link.env,
umask, chmod (lines 6, 9, 80; no umask/chmod), `find -perm -0002` summaries.
Live: galion-one :8091 200, nerve :8670/state 200, console :8080 302;
galion-one, hermes-rs, galion-nerve, console-rust-primary active.
No VPS writes, no temp files, no chmod/chown, no service touched, openmind untouched.
