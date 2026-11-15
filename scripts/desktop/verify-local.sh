#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Check a local openmind-desktop package directory.
# Confirms SHA256SUMS, one tarball, the ELF binary, and the local-artifact notice.
# Does not publish or upload.
set -euo pipefail

usage() {
  cat << 'EOF'
usage: scripts/desktop/verify-local.sh DIR

DIR is the directory written by scripts/desktop/package-local.sh.
It must contain SHA256SUMS and one openmind-desktop-*.tar.gz.

Checks the checksum, then the archive: an ELF openmind-desktop binary,
console.html for the control console, LOCAL-ARTIFACT.txt stating this
is not a GitHub Release and was not published, and LICENSE byte for
byte with the repository file. Line 4 of LICENSE is the last brand line.

This script never publishes. --publish, --upload, --gh-release and
--cargo-publish are refused.

exit 0  the local artifact matches
exit 2  refused publish flag, or bad arguments
exit 1  checksum or contents failed
EOF
}

refuse() {
  printf 'desktop-verify: refusing to publish or upload (%s)\n' "$1" >&2
  exit 2
}

die() {
  printf 'desktop-verify: %s\n' "$1" >&2
  exit 1
}

for arg in "$@"; do
  case "$arg" in
    --publish|--upload|--gh-release|--cargo-publish|publish|upload)
      refuse "$arg"
      ;;
  esac
done

if [ "${OPENMIND_DESKTOP_PUBLISH:-}" = "1" ]; then
  refuse "OPENMIND_DESKTOP_PUBLISH"
fi

if [ $# -eq 1 ] && { [ "$1" = "--help" ] || [ "$1" = "-h" ]; }; then
  usage
  exit 0
fi

if [ $# -ne 1 ]; then
  usage >&2
  exit 2
fi

dir=$1
if [ ! -d "$dir" ]; then
  die "not a directory: $dir"
fi
dir=$(CDPATH= cd -- "$dir" && pwd)

sums="$dir/SHA256SUMS"
if [ ! -f "$sums" ]; then
  die "missing SHA256SUMS"
fi

lines=0
listed=""
while IFS= read -r line || [ -n "$line" ]; do
  if [ -z "$line" ]; then
    continue
  fi
  lines=$((lines + 1))
  listed=$line
done < "$sums"
if [ "$lines" -ne 1 ]; then
  die "SHA256SUMS must list exactly one file"
fi

name=${listed##* }
case "$name" in
  openmind-desktop-*.tar.gz) ;;
  *) die "SHA256SUMS does not name an openmind-desktop tarball" ;;
esac
case "$name" in
  *://*|/*|../*|*/../*) die "checksum path is not a local file name" ;;
esac

if [ ! -f "$dir/$name" ]; then
  die "missing $name"
fi

count=0
for f in "$dir"/openmind-desktop-*.tar.gz; do
  if [ ! -f "$f" ]; then
    die "no tarball"
  fi
  count=$((count + 1))
done
if [ "$count" -ne 1 ]; then
  die "expected one tarball, found $count"
fi

(
  cd "$dir"
  sha256sum -c SHA256SUMS
)

unpack=$(mktemp -d)
trap 'rm -rf "$unpack"' EXIT
tar -C "$unpack" -xzf "$dir/$name"

tops=0
top=""
for d in "$unpack"/*; do
  if [ ! -d "$d" ]; then
    die "archive entry is not a directory: $d"
  fi
  tops=$((tops + 1))
  top=$d
done
if [ "$tops" -ne 1 ]; then
  die "archive must contain one directory"
fi

bin="$top/openmind-desktop"
html="$top/console.html"
note="$top/LOCAL-ARTIFACT.txt"
readme="$top/README.md"
license="$top/LICENSE"

if [ ! -f "$bin" ]; then
  die "binary missing"
fi
if [ ! -f "$html" ]; then
  die "console.html missing"
fi
if [ ! -f "$note" ]; then
  die "LOCAL-ARTIFACT.txt missing"
fi
if [ ! -f "$readme" ]; then
  die "README.md missing"
fi
if [ ! -f "$license" ]; then
  die "LICENSE missing"
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
repo_license="$root/LICENSE"
repo_last=$(sed -n '4p' "$repo_license")
got_last=$(sed -n '4p' "$license")
if [ -z "$repo_last" ] || [ "$got_last" != "$repo_last" ]; then
  die "last brand line does not match"
fi
if ! cmp -s "$repo_license" "$license"; then
  die "LICENSE does not match the repository LICENSE"
fi

magic=$(od -An -t x1 -N 4 -- "$bin" | tr -d ' \n')
if [ "$magic" != "7f454c46" ]; then
  die "openmind-desktop is not an ELF binary"
fi

grep -q 'data-desktop="control-console"' "$html" || die "console.html is not the control console"
grep -q '432 Hz' "$html" || die "console.html does not name 432 Hz"
grep -q 'not a GitHub Release' "$note" || die "notice does not say this is not a release"
grep -q 'not published' "$note" || die "notice does not say this was not published"

printf 'desktop-verify: ok %s\n' "$dir/$name"
