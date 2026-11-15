#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Build a local tarball of openmind-desktop.
# Writes the tarball and SHA256SUMS on this machine.
# Does not publish, upload, or create a release.
set -euo pipefail

usage() {
  cat << 'EOF'
usage: scripts/desktop/package-local.sh [--out DIR] [--target TRIPLE] [--skip-build]

Build openmind-desktop in release mode and write a local tarball plus
SHA256SUMS. The default directory is target/openmind-desktop-dist under
the repository root (target/ is gitignored). It does not write into dist/.

This script never publishes. --publish, --upload, --gh-release and
--cargo-publish are refused. It does not call gh, curl, wget, or
cargo publish.

--out DIR        directory for the tarball and SHA256SUMS
--target TRIPLE  pass --target TRIPLE to cargo (the toolchain must already have it)
--skip-build     package the release binary already in the cargo target dir
--help           print this text

exit 0  tarball and SHA256SUMS written
exit 2  refused publish flag, or bad arguments
exit 1  build or package failed
EOF
}

refuse() {
  printf 'desktop-package: refusing to publish or upload (%s)\n' "$1" >&2
  printf 'desktop-package: local artifacts only; no release is created\n' >&2
  exit 2
}

die() {
  printf 'desktop-package: %s\n' "$1" >&2
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

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
cd "$root"

out="$root/target/openmind-desktop-dist"
triple=""
skip_build=0

while [ $# -gt 0 ]; do
  case "$1" in
    --help|-h)
      usage
      exit 0
      ;;
    --out)
      if [ $# -lt 2 ]; then
        usage >&2
        exit 2
      fi
      out=$2
      shift 2
      ;;
    --target)
      if [ $# -lt 2 ]; then
        usage >&2
        exit 2
      fi
      triple=$2
      shift 2
      ;;
    --skip-build)
      skip_build=1
      shift
      ;;
    *)
      printf 'desktop-package: unknown argument %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

case "$out" in
  *://*)
    refuse "output path looks like a URL"
    ;;
esac

if [ -z "$out" ]; then
  die "empty --out"
fi

mkdir -p -- "$out"
out=$(CDPATH= cd -- "$out" && pwd)

if [ "$out" = "$root" ]; then
  die "--out must not be the repository root"
fi

if [ "$skip_build" -eq 0 ]; then
  if [ -n "$triple" ]; then
    nice -n 19 cargo build -j 2 -p openmind-desktop --release --locked --target "$triple"
  else
    nice -n 19 cargo build -j 2 -p openmind-desktop --release --locked
  fi
fi

target_dir=${CARGO_TARGET_DIR:-$root/target}
# Read all of `rustc -vV`. An awk `exit` closes the pipe early, and
# pipefail then turns rustc's SIGPIPE (141) into a failed package.
host=$(rustc -vV | sed -n 's/^host: //p')
host=${host%%[!A-Za-z0-9_.-]*}
if [ -z "$host" ]; then
  die "rustc did not report a host"
fi

if [ -n "$triple" ]; then
  bin="$target_dir/$triple/release/openmind-desktop"
  artifact_host=$triple
else
  bin="$target_dir/release/openmind-desktop"
  artifact_host=$host
fi

if [ ! -f "$bin" ]; then
  die "release binary missing: $bin"
fi

ver=$(awk '
  $0 == "[workspace.package]" { in_pkg = 1; next }
  in_pkg && $0 ~ /^\[/ { in_pkg = 0 }
  in_pkg && $1 == "version" {
    gsub(/"/, "", $3)
    print $3
    exit
  }
' "$root/Cargo.toml")
if [ -z "$ver" ]; then
  die "workspace version not found"
fi

name="openmind-desktop-${ver}-${artifact_host}"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
pkg="$stage/$name"
mkdir -p -- "$pkg"

cp -- "$bin" "$pkg/openmind-desktop"
chmod 755 "$pkg/openmind-desktop"
# Line 4 is the last of the four brand lines. Copy the repository file
# as it is. Do not append those lines a second time.
if [ -z "$(sed -n '4p' "$root/LICENSE")" ]; then
  die "LICENSE is missing the last brand line"
fi
cp -- "$root/LICENSE" "$pkg/LICENSE"
cp -- "$root/docs/desktop/README.md" "$pkg/README.md"

# Page mode prints HTML and exits. It does not listen.
"$pkg/openmind-desktop" > "$pkg/console.html"
grep -q 'data-desktop="control-console"' "$pkg/console.html"
grep -q '432 Hz' "$pkg/console.html"

cat > "$pkg/LOCAL-ARTIFACT.txt" << EOF
openmind-desktop local artifact
version: ${ver}
host: ${artifact_host}
This file is a local build. It is not a GitHub Release and it was not published.
Nothing in this directory was uploaded.
EOF

rm -f "$out"/openmind-desktop-*.tar.gz "$out"/SHA256SUMS
tar -C "$stage" -czf "$out/$name.tar.gz" "$name"
(
  cd "$out"
  sha256sum -- "$name.tar.gz" > SHA256SUMS
)

printf 'desktop-package: local artifact %s\n' "$out/$name.tar.gz"
printf 'desktop-package: checksums %s\n' "$out/SHA256SUMS"
printf 'desktop-package: not a release; nothing was published\n'
