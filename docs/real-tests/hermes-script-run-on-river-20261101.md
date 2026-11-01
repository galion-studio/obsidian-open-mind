<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Hermes terminal script mode on River (live, 2026-11-01)

Branch swarm/hermes-script-run-on-river-20261101 = bbf6d1be (live River pricing) + b29271d5
(test-only files-tests fix) + 4de4a982 (opt-in script mode). Approved by Mate, card t544s2665,
13:46 CET.

Box (CI=1, sandbox tests skipped on the box): hermes-rs + hermes-tools 221 passed, 0 failed,
1 ignored; clippy -D warnings (hermes-rs, hermes-tools, hermes-gateway, all targets) and fmt clean.
musl release (zig cc + rust-lld): hermes-gateway 3,401,536 B, sha256 3de1ecf8e7af...0664aa.

VPS deploy: /opt/galion/rust/bin/hermes-gateway-4de4a982, drop-in 99m-script-run-4de4a982.conf
(= 99l + new binary + HERMES_RS_GUARD_SCRIPT_MODE=1), 99l renamed .off. One restart 13:51:34 CET,
health 200, openmind 19 tools.

Live test through :8642, default model (River, DeepSeek V4.1 Flash), times CET:
- 13:52 a/b: write_file refused by the workspace quota (4288 of 4096 MiB used by Vis Reels
  output in /srv/galion-files); nothing ran.
- 13:52 c: files_write made the scripts, but as root 0700/0600, so the sandboxed terminal
  (uid 65534) got "Permission denied" on hello.sh. fetch.sh (curl) refused S7-network.
- 13:53 d: printf with \n escapes refused S3-escape (deny list, unchanged behaviour).
- 13:53 e: terminal-only: mkdir, echo > hello.sh, `sh script-test-20261101/hello.sh` ->
  "hello from script mode / sum=7", exit 0. Script mode works live.
- 13:54 services_status through Hermes: state ok, 9 units, 0 failed, 1 journal error (ssh).
River spend for all six calls: $0.0060 (ledger day_usd 21.223458 -> 21.229459).

Findings: (1) the 4096 MB tools workspace is over quota, so write_file is unusable until the
Reels output moves; (2) files_write creates root-only files the terminal sandbox cannot read.
