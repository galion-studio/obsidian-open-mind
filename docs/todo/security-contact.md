<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# TODO: the private security contact in SECURITY.md

Written 2026-10-29 (+01:00) Europe/Zagreb on `swarm/security-contact-todo-20261029`.

`SECURITY.md` still says `__CONTACT_EMAIL__` in the "Fallback" line. That placeholder is on
purpose (see `docs/PUBLISHING.md`) and must be replaced before the repository goes public.

## What was checked (2026-10-29, about 14:15 +01:00)

- Repository visibility: **private** (GitHub API). Not changed.
- GitHub code search across `galion-studio` for `security@` and `SECURITY.md` files.
- Addresses seen there are **not** confirmed for this project:
  - `security@galion.app`: only in old Project Nexus docs in `galion-studio/galion-platform`
    (`Documents/project-nexus/docs/security.md`). Nobody has confirmed that this mailbox
    exists or is read.
  - `security@nexuslang.dev`: the `nexuslang-v2` repo's `security.txt`. That is another
    product, so it does not apply here.

No official contact was found. Nothing is filled in, because a made-up address is worse than
a placeholder.

## What Mate has to decide

1. Give the official private address. It must be a team alias, not a personal mailbox.
2. Then replace `__CONTACT_EMAIL__` in `SECURITY.md` with it, in a separate PR.
3. When the repository goes public, turn on GitHub private vulnerability reporting. That is
   the preferred channel, and it needs no email address at all.
