<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Swarm tooling (box-side helpers), recorded 2026-10-20 23:31 CEST (+02:00)

These small helpers live on the swarm box under `/workspace/swarm`. They are recorded here as text so they survive a box restart. They are kept as documentation, not as `.sh` files in the repo, because the project is Rust-only and the branch should add no new scripts. `<vps>` stands for the VPS address.

## Why they exist

- **push-branch.sh / remote-push.sh**: the GitHub token never leaves the VPS. The box makes a git bundle of exactly one `swarm/*` branch, copies it to the VPS over ssh, and the VPS pushes it with the token read there. Pushes to `main` or any branch outside `swarm/*` are refused on both sides. Nothing is force-pushed.
- **bin/python, python3, pip, pip3, python3.13**: each one exits with an error. Every agent puts `/workspace/swarm/bin` first on `PATH`, so an accidental Python call fails loudly instead of running (standing Rust-only rule, Slavik, 2026-10-20 20:41 CEST).
- **Read-only GitHub check (swarm-record, 23:27 CEST)**: the same askpass pattern as `remote-push.sh`, but running `git ls-remote` only. It was used to find which local branch heads were missing from GitHub. The token value is never printed. Output is filtered for token patterns anyway.

## push-branch.sh

```bash
#!/bin/bash
# usage: /workspace/swarm/push-branch.sh <local-repo-dir> <swarm/branch-name>
# Pushes ONE swarm/* branch to GitHub through the VPS; the token never leaves the VPS.
set -euo pipefail
repo="$1"; br="$2"
case "$br" in swarm/*) ;; *) echo "only swarm/* branches"; exit 2;; esac
SSHO=(-i "$HOME/.ssh/id_ed25519_vps" -o IdentitiesOnly=yes)
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
git -C "$repo" bundle create -q "$tmp/b.bundle" "refs/heads/$br"
rb="/tmp/swarm-$(echo "$br" | tr '/' '-')-$$.bundle"
scp -q "${SSHO[@]}" "$tmp/b.bundle" "root@<vps>:$rb"
ssh "${SSHO[@]}" root@<vps> bash -s -- "$rb" "$br" < /workspace/swarm/remote-push.sh
```

## remote-push.sh

```bash
#!/bin/bash
# runs on the VPS via: ssh ... bash -s -- <bundle-path> <branch>
set -euo pipefail
b="$1"; br="$2"
case "$br" in swarm/*) ;; *) echo "refused: only swarm/* branches"; rm -f "$b"; exit 2;; esac
d=$(mktemp -d /tmp/swarm-push.XXXXXX)
trap 'rm -rf "$d" "$b"' EXIT
git init -q --bare "$d/r.git"
git -C "$d/r.git" fetch -q "$b" "refs/heads/$br:refs/heads/$br"
cat > "$d/ap" <<'AP'
#!/bin/sh
case "$1" in Username*) echo x-access-token;; *) cat /opt/galion/galion-light/secrets/persist/github.token;; esac
AP
chmod 700 "$d/ap"
GIT_ASKPASS="$d/ap" GIT_TERMINAL_PROMPT=0 git -C "$d/r.git" push -q https://github.com/galion-studio/obsidian-open-mind.git "refs/heads/$br:refs/heads/$br" 2>&1 | sed -E 's/(gh[pousr]_|github_pat_)[A-Za-z0-9_]+/***/g'
echo "pushed $br $(git -C "$d/r.git" rev-parse "refs/heads/$br")"
```

## bin/pip

```bash
#!/bin/sh
echo "BLOCKED: Python is forbidden in this project (RULES.md hard rule 1). Do not run it, not even --version." >&2
exit 127
```

## bin/pip3

```bash
#!/bin/sh
echo "BLOCKED: Python is forbidden in this project (RULES.md hard rule 1). Do not run it, not even --version." >&2
exit 127
```

## bin/python

```bash
#!/bin/sh
echo "BLOCKED: Python is forbidden in this project (RULES.md hard rule 1). Do not run it, not even --version." >&2
exit 127
```

## bin/python3

```bash
#!/bin/sh
echo "BLOCKED: Python is forbidden in this project (RULES.md hard rule 1). Do not run it, not even --version." >&2
exit 127
```

## bin/python3.13

```bash
#!/bin/sh
echo "BLOCKED: Python is forbidden in this project (RULES.md hard rule 1). Do not run it, not even --version." >&2
exit 127
```

## Read-only GitHub check (scratch/swarm-record/remote-lsremote.sh)

```bash
#!/bin/bash
# runs on the VPS via ssh bash -s; read-only ls-remote of swarm/* heads, same askpass pattern as remote-push.sh
set -euo pipefail
d=$(mktemp -d /tmp/swarm-ls.XXXXXX); trap 'rm -rf "$d"' EXIT
cat > "$d/ap" <<'AP'
#!/bin/sh
case "$1" in Username*) echo x-access-token;; *) cat /opt/galion/galion-light/secrets/persist/github.token;; esac
AP
chmod 700 "$d/ap"
GIT_ASKPASS="$d/ap" GIT_TERMINAL_PROMPT=0 git ls-remote https://github.com/galion-studio/obsidian-open-mind.git 'refs/heads/*' 2>&1 | sed -E 's/(gh[pousr]_|github_pat_)[A-Za-z0-9_]+/***/g'
```
