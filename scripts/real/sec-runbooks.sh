#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio
#
# Real-life test for swarm/sec-runbooks (slot 26). Runs ONLY on the box, on
# scratch copies. Never pushes, never touches GitHub or the VPS.
#   1. release-build galion-purge
#   2. git clone --mirror of the box mirror (base.git) into a scratch dir
#   3. scan, gitleaks (before), rewrite with the purge list, verify, plain-git
#      cross-checks, gitleaks (after), original mirror unchanged
#   4. clone the rewritten mirror, check out main, cargo check --workspace --locked
#   5. SEC-3 perms script on a fixture tree: dry-run changes nothing, --apply
#      fixes modes, --rollback restores them
# Usage: GALION_REAL=1 scripts/real/sec-runbooks.sh [SOURCE_REPO]
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH
[ "${GALION_REAL:-}" = 1 ] || { echo "set GALION_REAL=1"; exit 0; }
here=$(cd "$(dirname "$0")/../.." && pwd)
src=${1:-/workspace/swarm/base.git}
tgt=${CARGO_TARGET_DIR:-/workspace/swarm/target/sec-runbooks}
work=$tgt/real-scratch
gitleaks=${GITLEAKS:-$(command -v gitleaks || echo /workspace/swarm/target/launch-gates/tools/bin/gitleaks)}
list=$here/docs/launch/purge-paths.obsidian-open-mind.txt
ts() { TZ=Europe/Zagreb date +%Y-%m-%dT%H:%M:%S%:z; }
step() { echo; echo "== $(ts) $*"; }
fail() { echo "FAIL: $*"; exit 1; }
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0

step "build galion-purge (release)"
(cd "$here" && CARGO_TARGET_DIR=$tgt nice -n 10 cargo build -q --release -p history-purge -j 3)
bin=$tgt/release/galion-purge
ls -l "$bin" | awk '{print $5, $9}'

rm -rf "$work"; mkdir -p "$work"
trap 'rm -rf "$work"' EXIT
src_main_before=$(git -C "$src" rev-parse refs/heads/main)
src_refs_before=$(git -C "$src" for-each-ref --format='%(objectname) %(refname)' | sha256sum | cut -c1-16)

step "scratch mirror clone of $src (main $src_main_before)"
git clone -q --mirror "$src" "$work/scratch.git"
echo "refs: $(git -C "$work/scratch.git" for-each-ref | wc -l), commits: $(git -C "$work/scratch.git" rev-list --all --count), remotes: $(git -C "$work/scratch.git" remote | tr '\n' ' ')"

step "refusals"
if "$bin" rewrite --repo "$work/scratch.git" --paths "$list" >/dev/null 2>&1; then fail "ran without --confirm-scratch-copy"; fi
out=$("$bin" rewrite --repo "$work/scratch.git" --paths "$list" --confirm-scratch-copy 2>&1 || true)
echo "$out" | grep -q -- "--strip-remotes" || fail "remote not refused: $out"
echo "without --confirm-scratch-copy: refused; with a remote and no --strip-remotes: refused"

step "scan before (paths only)"
"$bin" scan --repo "$work/scratch.git" --paths "$list" --quiet

step "plant a canary secret in listed paths on a scratch-only branch (generated now, never committed to the repo)"
git clone -q "$work/scratch.git" "$work/canary-wt"
key="AKIA$(head -c 64 /dev/urandom | base32 | head -n1 | cut -c1-16)"
mkdir -p "$work/canary-wt/contrib" "$work/canary-wt/stack/kms"
printf "aws_access_key_id = %s\n" "$key" > "$work/canary-wt/contrib/canary.env"
printf "AWS_ACCESS_KEY_ID=%s\n" "$key" > "$work/canary-wt/stack/kms/canary.env.redacted"
unset key
git -C "$work/canary-wt" checkout -q -b canary
git -C "$work/canary-wt" add -A
git -C "$work/canary-wt" -c user.name=canary -c user.email=canary@example.invalid commit -q -m "canary: fake key in listed paths"
git -C "$work/scratch.git" fetch -q "$work/canary-wt" canary:refs/heads/canary
rm -rf "$work/canary-wt"
echo "canary branch added to the scratch mirror only"

step "gitleaks before (all history, redacted)"
if "$gitleaks" git "$work/scratch.git" --redact --no-banner --log-level warn --report-path "$work/gl-before.json"; then fail "gitleaks missed the canary"; fi
nb=$(grep -c '"RuleID"' "$work/gl-before.json" || true)
echo "findings before: $nb (files: $(grep -o '"File": "[^"]*"' "$work/gl-before.json" | sort -u | tr '\n' ' '))"
[ "$nb" -ge 1 ] || fail "canary not found before"

step "rewrite"
"$bin" rewrite --repo "$work/scratch.git" --paths "$list" --confirm-scratch-copy --strip-remotes --quiet

map=$work/scratch.git/galion-purge-commit-map.tsv
new_main=$(awk -v o="$src_main_before" '$1==o{print $2}' "$map")
echo "commit map: $(grep -vc '^#' "$map") lines; main ${src_main_before:0:7} -> ${new_main:0:7}"
[ "$new_main" = "$(git -C "$work/scratch.git" rev-parse refs/heads/main)" ] || fail "commit map does not match new main"

step "verify (galion-purge)"
"$bin" verify --repo "$work/scratch.git" --paths "$list" --quiet

step "cross-check with plain git (every commit diff vs every parent, every reachable object path)"
re='^(contrib/|stack/kms/|stack/INDEX\.md$|dist/bin/|edge/dist/)|\.redacted"?$'
n1=$(git -C "$work/scratch.git" log --all -m --no-renames --format= --name-only | grep -cE "$re" || true)
n2=$(git -C "$work/scratch.git" rev-list --all --objects | cut -d' ' -f2- | grep -cE "$re" || true)
echo "log -m --name-only hits: $n1; rev-list --objects path hits: $n2"
[ "$n1" = 0 ] && [ "$n2" = 0 ] || fail "plain git still sees listed paths"
[ -z "$(git -C "$work/scratch.git" remote)" ] || fail "remote still configured"
git -C "$work/scratch.git" fsck --full --no-dangling && echo "fsck: clean"
echo "refs after: $(git -C "$work/scratch.git" for-each-ref | wc -l), commits after: $(git -C "$work/scratch.git" rev-list --all --count)"

step "gitleaks after"
"$gitleaks" git "$work/scratch.git" --redact --no-banner --log-level warn --report-path "$work/gl-after.json" || fail "gitleaks found something after the purge"
echo "findings after: $(grep -c '"RuleID"' "$work/gl-after.json" || true)"

step "source mirror unchanged"
[ "$(git -C "$src" rev-parse refs/heads/main)" = "$src_main_before" ] || fail "source main moved"
[ "$(git -C "$src" for-each-ref --format='%(objectname) %(refname)' | sha256sum | cut -c1-16)" = "$src_refs_before" ] || fail "source refs changed"
echo "source refs digest $src_refs_before unchanged"

step "rewritten tree still builds (main, cargo check --workspace --locked)"
git clone -q "$work/scratch.git" "$work/tree"
git -C "$work/tree" log --oneline -1
for p in contrib stack/kms stack/INDEX.md dist/bin edge/dist; do [ ! -e "$work/tree/$p" ] || fail "$p present in checkout"; done
(cd "$work/tree" && CARGO_TARGET_DIR=$tgt/real-tree nice -n 10 cargo check -q --workspace --locked -j 3) && echo "cargo check: ok"
rm -rf "$tgt/real-tree"

step "SEC-3 script on a fixture HERMES_HOME"
fx=$work/hermes-fixture; mkdir -p "$fx/vault" "$fx/sessions"
: > "$fx/config.yaml"; : > "$fx/SOUL.md"; : > "$fx/vault/.vault.lock"; : > "$fx/USER.md"; : > "$fx/notes.md"
chmod 700 "$fx"; chmod 666 "$fx/config.yaml" "$fx/SOUL.md" "$fx/vault/.vault.lock" "$fx/notes.md"; chmod 777 "$fx/vault"; chmod 664 "$fx/USER.md"
modes() { (cd "$fx" && stat -c '%a %n' config.yaml SOUL.md vault vault/.vault.lock USER.md notes.md | tr '\n' ' '); }
echo "before:   $(modes)"
s3="$here/deploy/staged/sec3-hermes-perms.sh"
HERMES_HOME=$fx SEC3_STATE_DIR=$work/sec3-state SEC3_NO_CHOWN=1 bash "$s3" >/dev/null
echo "dry-run:  $(modes)"
[ "$(modes)" = "666 config.yaml 666 SOUL.md 777 vault 666 vault/.vault.lock 664 USER.md 666 notes.md " ] || fail "dry-run changed modes"
HERMES_HOME=$fx SEC3_STATE_DIR=$work/sec3-state SEC3_NO_CHOWN=1 bash "$s3" --apply | tail -3
echo "applied:  $(modes)"
[ "$(modes)" = "600 config.yaml 644 SOUL.md 700 vault 600 vault/.vault.lock 600 USER.md 666 notes.md " ] || fail "apply result"
HERMES_HOME=$fx SEC3_STATE_DIR=$work/sec3-state SEC3_NO_CHOWN=1 bash "$s3" --apply --fix-world-writable | tail -1
echo "o-w:      $(modes)"
first=$(ls "$work/sec3-state"/sec3-before-*.tsv | head -1)
HERMES_HOME=$fx SEC3_STATE_DIR=$work/sec3-state SEC3_NO_CHOWN=1 bash "$s3" --rollback "$first" | tail -1
echo "rollback: $(modes)"
[ "$(modes)" = "666 config.yaml 666 SOUL.md 777 vault 666 vault/.vault.lock 664 USER.md 666 notes.md " ] || fail "rollback result"

step "PASS"
