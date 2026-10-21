<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real checks: sec-tooling-licence-memo (slot 234)

Docs-only slot. Nothing was built, started or changed; ports 21040-21049 unused.
All times Europe/Zagreb, CEST (+02:00). Tree: integration-5 `0d68f46`.

## 1. Inventory references (box, 2026-10-22 00:15)

Every `file:line` cited in `docs/security/tooling-licence-memo.md` was re-read and
checked to contain the tool name (or the matching constant): **86/86 ok, 0 missing**.
Searches: `rg` over the tree excluding `vendor/`, `Cargo.lock` and `*.md` for strace,
unshare/nsenter/prlimit, qemu-*, `Command::new("git")`, readelf, nft, journalctl,
systemctl, libfuzzer/cargo-fuzz/fuzz_target!, codeql, UAPI header names.
history-purge (`swarm/sec-runbooks`, not in base.git) read from GitHub: `src/repo.rs:19`
`Command::new("git")`, `:213` fast-export, `:229` fast-import.

## 2. Installed tools and licences on the real server (VPS, read-only, 2026-10-22 00:10:46)

`command -v`, `dpkg -S`, `dpkg-query -W`, `License:` lines of `/usr/share/doc/<pkg>/copyright`:

| tool | package | version | licence lines |
|---|---|---|---|
| strace | strace | 6.8-0ubuntu2 | (non-DEP5 file) "GNU Lesser General Public License" |
| unshare, nsenter, prlimit | util-linux | 2.39.3-9ubuntu6 | GPL-2, GPL-2+ |
| qemu-aarch64-static | - | missing | - |
| nft | nftables | 1.0.9-1build1 | GPL-2, GPL-2+ |
| journalctl, systemctl | systemd | 255.4-1ubuntu8 | CC0-1.0, GPL-2 with Linux-syscall-note, LGPL-2.1+ |
| git | git | 1:2.43.0-1ubuntu7.3 | BSD-3-clause, GPL-2, Zlib |
| readelf | binutils-x86-64-linux-gnu | 2.42-4ubuntu2.10 | (non-DEP5 file) "GNU General Public License ... version 3" |
| curl | curl | 8.5.0-2ubuntu10.15 | OLDAP-2.8, curl |
| bash | bash | 5.2.21-2ubuntu4 | GPL-3+ |

Box (Debian 13): strace 6.13+ds-1, util-linux 2.41.5-0+deb13u1, qemu-user 1:10.0.13+ds-0+deb13u1,
systemd 257.13-1~deb13u1, git 1:2.47.3-0+deb13u1, binutils 2.44-3, nft not installed.

Live nerve at the same time (GET 127.0.0.1:8670/state): mode live, cycle 10, score 61, grade D.
Cortex: GET 127.0.0.1:8674/health 200, 127.0.0.1:8672/healthz 200 (galion-nerve and
gaia-cortex units `active`). The nerve is itself an L2 case: it runs `journalctl` live.

## 3. What we ship contains no third-party tool (box, 00:12)

- `release/omnode-0.1.0/omnode-0.1.0-linux-{x86_64,aarch64}.tar.gz`: `tar tzf` lists only `omnode`.
- omnode x86_64: 0 `NEEDED`, no program interpreter (static musl); `strings` finds 70
  `libunwind` strings (e.g. `libunwind: _Unwind_GetDataRelBase`), i.e. LLVM libunwind from
  Rust's self-contained musl (`libc.a`, `libunwind.a`, crt objects in the sysroot) -> L7.
- `dist/bin/{desk-hermes,galion-desk,galion-one,link-watch,room}` (tracked since e27ced9,
  2026-10-20 19:49:57 +02:00): dynamic glibc, NEEDED `libgcc_s.so.1 libc.so.6` (+ `libm.so.6`,
  `libsqlite3.so.0` for galion-one, `libm.so.6` for desk-hermes) -> L8.

## 4. cargo-deny with the proposed L7 entry (box, 00:13)

cargo-deny 0.20.2, `--offline --all-features --config <file> check licenses`:

| config | result | exit |
|---|---|---|
| deny.toml as committed | licenses ok | 0 |
| + `"Apache-2.0 WITH LLVM-exception"` in `allow` (temp copy in /tmp) | licenses ok, no warning | 0 |
| negative control: `"MIT"` removed (temp copy) | 187 `rejected` lines | 4 |

`cargo metadata --offline`: 3 crates declare `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`
(wasi 0.11.1, wasip2 1.0.4, wit-bindgen 0.57.1); they pass today via MIT. deny.toml is unchanged.

## 5. Primary sources read (2026-10-22 00:09)

- CodeQL terms: https://raw.githubusercontent.com/github/codeql-cli-binaries/main/LICENSE.md
  (CI/automated analysis only for an Open Source Codebase hosted on GitHub.com, or with paid GHAS;
  no use with a non-open-source codebase "e.g. code in a private repo").
- Repo visibility: anonymous `GET https://api.github.com/repos/galion-studio/obsidian-open-mind` -> 404 (private).
- Linux-syscall-note: https://raw.githubusercontent.com/torvalds/linux/master/LICENSES/exceptions/Linux-syscall-note
- compiler-rt (libFuzzer) licence: https://raw.githubusercontent.com/llvm/llvm-project/main/compiler-rt/LICENSE.TXT
  (Apache-2.0 WITH LLVM-exception; legacy NCSA OR MIT).
- GPL FAQ #MereAggregation: https://www.gnu.org/licenses/gpl-faq.html#MereAggregation
  ("pipes, sockets and command-line arguments are communication mechanisms normally used
  between two separate programs").

repo-check (own debug build, 00:15): 1522 files, 10 errors / 48 warnings, identical to base 0d68f46
(1519 files, 10/48; the 10 errors are the known vendored candle-core portable findings);
no finding in the 3 new files. `scripts/no-python.sh`: ok.

Hermes research helper: not used (primary sources were enough; $0 spent).
