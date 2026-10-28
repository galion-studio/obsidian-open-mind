<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code and dependencies: swarm/deploy-sim (slot 33)

- Written: 2026-10-20 (Europe/Zagreb, CEST +02:00).

## New Rust dependencies

**None.** `sim/deploy-sim` (crate `simsup`) uses only `std`. `Cargo.lock` lists
`simsup` alone. The workspace `Cargo.lock` is unchanged.

| crate | version | licence | why no dependency |
|---|---|---|---|
| (none) | | | Process supervision uses `std::process` (`process_group(0)`, `try_wait`). Signals go through one `extern "C" fn kill` from the libc that std already links. The control socket is `std::os::unix::net`. Time is `SystemTime`, with a 20-line EU summer-time rule (see `src/zagreb.rs`). Pulling in `nix`/`libc`/`chrono` for this much was not worth it. |

## Copied code

**None.** Two things are written from public descriptions, not copied:

- `src/zagreb.rs` `civil()` / `days_from_civil()` use the well-known days-to-civil
  arithmetic (the era/day-of-era method that Howard Hinnant describes in
  "chrono-Compatible Low-Level Date Algorithms"). It is written from that description;
  no code was copied.
- `src/sup.rs` `start_allowed()` copies the *behaviour* of systemd's
  `ratelimit_below()` (fixed window, reset once the interval has passed). No systemd
  source (LGPL-2.1) was copied or translated line by line. Only the documented
  semantics are modelled ("units which are started more than burst times within an
  interval time span are not permitted to start any more", `systemd.unit(5)`).

## Tools used at run time (not linked, not shipped)

`bash`, `curl`, `ss`, `pgrep`, `timeout`, `sha256sum`, `jq`, `unshare` (util-linux)
and `git`. These are box tools that the rehearsal scripts call. The galion-deploy
script already depended on `bash`, `curl`, `sha256sum`, `od` and `install`. The
optional `GALION_DEPLOY_VERSION_TIMEOUT` uses coreutils `timeout` only when it is set.
With nothing set, the script needs exactly what it needed before.
