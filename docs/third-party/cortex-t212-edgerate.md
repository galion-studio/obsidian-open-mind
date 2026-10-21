<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: cortex-t212-edgerate

- Written: 2026-10-21 01:25 CEST (+02:00).
- New dependencies: **none** (std + the crate's existing serde_json). `Cargo.lock` unchanged.
- Copied code: none. The cloudflared log line shapes were read from cloudflared's source
  (`proxy/logger.go`, Apache-2.0, https://github.com/cloudflare/cloudflared) and from the live VPS journal; only the
  format is matched, no code taken.
