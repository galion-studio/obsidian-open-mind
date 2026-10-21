<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Decision memo: licences of test tools, and the scope of "Rust only, no exceptions"

Written 2026-10-20 23:00 CEST (+02:00), slot 42 `decision-memos`. Read-only research; nothing decided here.
Sources: plans/security.md §2, §3 S6.1, §6.4; plans/non-rust-inventory.md; CALL-RECORD.md Q14 (20:41); RULES.md "Open-source borrowing"; code refs below at integration-1 8fd26f6.

## Part 1: licences (RULES.md allow-list today covers "dependencies or copied code"; it is silent on programs we only run)
| # | Item | Licence | How we use it | Where |
|---|---|---|---|---|
| a1 | strace | LGPL-2.1+ | test-only, run as a program (seccomp evidence) | edge-boot real-tests |
| a2 | util-linux unshare / nsenter | GPL-2.0+ | test-only, run as a program (sandbox/seccomp) | edge-boot, sandbox tests |
| a3 | qemu-user-static | GPL-2.0 | test-only, runs aarch64 binaries | edge-boot, ucpu gate |
| a4 | git | GPL-2.0 | driven by `galion-purge` (fast-export/fast-import) | sec-runbooks crates/history-purge |
| b | Kernel UAPI syscall numbers / ABI constants | GPL-2.0 WITH Linux-syscall-note | facts/interface; the note explicitly exempts userspace use | omnode seccomp, openmind-sandbox |
| c | libFuzzer (via libfuzzer-sys 0.4.13) | MIT OR Apache-2.0 AND NCSA | fuzz/ separate workspace only, never in a release | fuzz-sast fuzz/Cargo.toml |
| d | CodeQL CLI | GitHub CodeQL Terms (free for OSI-licensed public repos) | CI service only, nothing vendored | proposed S2 SAST |
| e | Rejected: BFG (GPL-3.0, JVM), git-filter-repo (Python), Semgrep CE (LGPL-2.1) | | replaced by Rust `galion-purge` + clippy/CodeQL | security.md S6.1 |
NCSA is permissive (BSD-like, OSI-approved) but not on our allow-list. deny.toml stays unchanged: it already guards linked crates; fuzz/ is excluded from the workspace.

Proposed RULES.md text (add under "Open-source borrowing"):
> The allow-list applies to code linked into or shipped with our binaries. External tools run only in tests or CI may be under any OSI licence, are recorded in docs/third-party, and must never be vendored, linked or shipped.

## Part 2: "Rust only, no exceptions" (Slavik 20:41 CEST): what does it cover?
Current state, one line each:
- OS packages (systemd, journald, sshd, fail2ban, networkd-dispatcher, cron, apt timers): running on VPS; fail2ban -> guard-rust d5f7874 in progress; rest have no sensible Rust replacement (non-rust-inventory §2-3).
- cloudflared x2 (Go): running (tunnel + tuner-quick); no mature Rust equivalent; SEC-5 recommends stopping tuner-quick.
- postfix (C): running local MTA; option is remove, or lettre in Rust.
- Browser JS: console UI TSX/TS (~4k lines) and apps/phone-voice/index.html `<script>`; console-static told no Node at build/run; Rust/WASM UI possible later.
- CI YAML: 6 workflows, 581 lines in .github/workflows (declarative, not code); deploy/ 30 systemd units likewise.
- CUDA / whisper.cpp C++ layers: GPU pod runtime and STT; whisper-rs binds whisper.cpp (C++); candle-whisper is pure Rust; CUDA has no replacement.
- curl / pdftotext as subprocesses: `Command::new("curl")` in openmind-nerve alerts.rs:161 and galion-one models_refresh.rs:77; `pdftotext` in galion-one main.rs:1558.
- Bundled SQLite C: desk-hermes uses rusqlite `bundled` (compiles SQLite C); sqlite-pure-spike 98686ef compared Turso vs redb+tantivy.
- nft / journalctl: guard-rust drives system `nft` (script on stdin) and `journalctl --follow --output=json`; gaia/nerve print `journalctl` inspect hints.

## Questions for Slavik (answer yes/no)
1. GPL/LGPL test-only tools run as programs (strace, unshare/nsenter, qemu-user-static) are allowed?
2. galion-purge may drive the system `git` (GPL-2.0) as a program?
3. Kernel UAPI syscall numbers/constants may be written into our MIT code?
4. libFuzzer (NCSA) allowed in the separate fuzz/ workspace, never released?
5. CodeQL CLI allowed in CI under GitHub's terms?
6. Confirm rejection of BFG, git-filter-repo and Semgrep CE?
7. Add the proposed rule text above to RULES.md?
8. "No exceptions" excludes base OS packages (systemd, sshd, journald, cron)?
9. cloudflared (Go) may stay until a Rust tunnel exists?
10. postfix: remove (rather than rewrite)?
11. Browser-side JS allowed for the console / phone UI?
12. CI YAML and systemd unit files count as config, not code (allowed)?
13. CUDA and whisper.cpp C++ allowed on the GPU pod (via Rust bindings)?
14. Calling curl / pdftotext as subprocesses allowed (else rewrite with Rust crates)?
15. Bundled SQLite C allowed in desk-hermes (else move to a pure-Rust store)?
16. Calling system nft / journalctl from Rust allowed?
