<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Third-party: opt-bin-py-port (pw-check, replicate-picture)

No new registry crates: Cargo.lock only gains the two local packages.

| Crate | Version (lock) | Licence | Source | Used for | Why not write it |
|---|---|---|---|---|---|
| libc | 0.2.191 | MIT OR Apache-2.0 | https://github.com/rust-lang/libc | pw-check: pipe2, fcntl(F_DUPFD_CLOEXEC), dup2 to hand fds 3/4 to Chromium | already in the lock (galion-backup, galion-guard); std has no dup2 |
| serde (+derive) | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde | pw-check report rows | workspace dep |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | https://github.com/serde-rs/json | CDP messages, Replicate JSON | workspace dep |

External programs (run, not linked, nothing copied):

- `curl` (curl licence, MIT-style): HTTPS for replicate-picture, as galion-deploy/galion-one already do. Token and
  body go through `-K -` on stdin.
- Chromium / chrome-headless-shell (BSD-3-Clause): Playwright already installed it on the VPS
  (`/root/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-linux64/chrome-headless-shell`).

Considered and not used: `chromiumoxide`, `headless_chrome` (MIT/Apache, but pull tokio/websocket/TLS stacks,
dozens of crates for what is ~250 lines of pipe CDP); `ureq`+`rustls` (ring has asm; repo rule: no asm/openssl).
No code copied. The CDP method names come from the public protocol docs
https://chromedevtools.github.io/devtools-protocol/.
