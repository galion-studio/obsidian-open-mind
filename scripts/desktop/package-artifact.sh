#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Write a local openmind-desktop workflow artifact.
# Does not upload, publish, sign, or create a release.
set -euo pipefail

usage() {
  cat << 'EOF'
usage: scripts/desktop/package-artifact.sh [--out DIR] [--target TRIPLE] [--skip-build] [--runner CMD]

Build openmind-desktop in release mode and write a directory of local
files: LICENSE, README.md, SHA256SUMS, and one archive. Linux triples
also get a .deb, and that .deb is inside the tarball. A Windows triple
is a .zip. A macOS triple is a .tar.gz.

LICENSE is the repository MIT text plus the four Galion Labs lines.
README.md is the short artifact readme plus this build's version and triple.

The default directory is target/openmind-desktop-ci under the repository
root (target/ is gitignored). The directory must be under target/, /tmp,
TMPDIR, or the runner temp directory.

This script never publishes or signs. --publish, --upload, --gh-release,
--cargo-publish, --sign and --signed are refused. It does not upload the
directory. A workflow may upload the directory afterwards as an artifact.

--out DIR        directory for the artifact files
--target TRIPLE  pass --target TRIPLE to cargo (the toolchain must already have it)
--skip-build     package the release binary already in the cargo target dir
--runner CMD     command that runs a cross binary when writing console.html
--help           print this text

exit 0  artifact directory written
exit 2  refused publish or sign flag, or bad arguments
exit 1  build or package failed
EOF
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=common.sh
. "$script_dir/common.sh"
desktop_prefix=desktop-package
desktop_init "$0"
desktop_refuse_publishing "$@"

out="$desktop_root/target/openmind-desktop-ci"
explicit_target=""
skip_build=0
runner=""

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
      explicit_target=$2
      shift 2
      ;;
    --skip-build)
      skip_build=1
      shift
      ;;
    --runner)
      if [ $# -lt 2 ]; then
        usage >&2
        exit 2
      fi
      runner=$2
      shift 2
      ;;
    *)
      desktop_usage_fail "unknown argument $1"
      ;;
  esac
done

case "$out" in
  *://*)
    printf '%s: refusing to publish, upload, or sign (output path looks like a URL)\n' "$desktop_prefix" >&2
    exit 2
    ;;
esac

if [ -z "$out" ]; then
  desktop_usage_fail "empty --out"
fi

case "$runner" in
  "") ;;
  *[!A-Za-z0-9_./-]*)
    desktop_usage_fail "runner path has unexpected characters"
    ;;
esac

cd "$desktop_root"

if [ "$skip_build" -eq 0 ]; then
  if [ -n "$explicit_target" ]; then
    if command -v nice >/dev/null 2>&1; then
      nice -n 19 cargo build -j 2 -p openmind-desktop --release --locked --target "$explicit_target"
    else
      cargo build -j 2 -p openmind-desktop --release --locked --target "$explicit_target"
    fi
  else
    if command -v nice >/dev/null 2>&1; then
      nice -n 19 cargo build -j 2 -p openmind-desktop --release --locked
    else
      cargo build -j 2 -p openmind-desktop --release --locked
    fi
  fi
fi

if [ -n "$explicit_target" ]; then
  triple=$explicit_target
else
  # Read all of `rustc -vV`. An awk `exit` closes the pipe early, and
  # pipefail then turns rustc's SIGPIPE into a failed package.
  triple=$(rustc -vV | sed -n 's/^host: //p')
  triple=${triple%%[!A-Za-z0-9_.-]*}
fi

format=$(desktop_format "$triple") || desktop_die "unsupported target $triple"
ver=$(desktop_version)
bin_name=$(desktop_bin_name "$triple")
stem="openmind-desktop-${ver}-${triple}"
deb_name=""
deb_arch=""
case "$format" in
  zip) archive_name="${stem}.zip" ;;
  linux|tar) archive_name="${stem}.tar.gz" ;;
esac
if [ "$format" = "linux" ]; then
  deb_arch=$(desktop_deb_arch "$triple") || desktop_die "no Debian architecture for $triple"
  deb_name="openmind-desktop_${ver}_${deb_arch}.deb"
fi

target_dir=${CARGO_TARGET_DIR:-$desktop_root/target}
if [ -n "$explicit_target" ]; then
  bin="$target_dir/$triple/release/$bin_name"
else
  bin="$target_dir/release/$bin_name"
fi
if [ ! -f "$bin" ]; then
  desktop_die "release binary missing: $bin"
fi

mkdir -p -- "$out"
out=$(CDPATH= cd -- "$out" && pwd)
case "$out" in
  "$desktop_root"/target/*) ;;
  /tmp|/tmp/*) ;;
  *)
    allowed=0
    if [ -n "${TMPDIR:-}" ]; then
      case "$out" in
        "$TMPDIR"|"$TMPDIR"/*) allowed=1 ;;
      esac
    fi
    if [ -n "${RUNNER_TEMP:-}" ]; then
      case "$out" in
        "$RUNNER_TEMP"|"$RUNNER_TEMP"/*) allowed=1 ;;
      esac
    fi
    if [ "$allowed" -ne 1 ]; then
      desktop_die "--out must be under target/, /tmp, TMPDIR, or the runner temp directory"
    fi
    ;;
esac
if [ "$out" = "$desktop_root" ] || [ "$out" = "$desktop_root/target" ]; then
  desktop_die "--out must be a directory under target/, not the repository root or target/ itself"
fi

# Generated names in the output directory, replaced on the next run.
rm -f -- \
  "$out"/SHA256SUMS \
  "$out"/LICENSE \
  "$out"/README.md \
  "$out"/openmind-desktop-*.tar.gz \
  "$out"/openmind-desktop-*.zip \
  "$out"/openmind-desktop_*.deb

desktop_assemble_license "$out/LICENSE"
desktop_assemble_readme "$out/README.md" "$ver" "$triple"
# The short README is a fixed template plus four trailer lines.
readme_lines=$(wc -l < "$out/README.md")
readme_lines=${readme_lines// /}
if [ "$readme_lines" -gt 80 ]; then
  desktop_die "README.md is $readme_lines lines; the artifact readme stays at most 80"
fi

work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT

if [ "$format" = "linux" ]; then
  if ! command -v dpkg-deb >/dev/null 2>&1; then
    desktop_die "dpkg-deb is required to build the Linux package"
  fi
  deps=$(desktop_deb_depends "$bin")
  deb_root="$work/deb"
  mkdir -p -- \
    "$deb_root/DEBIAN" \
    "$deb_root/usr/bin" \
    "$deb_root/usr/share/doc/openmind-desktop"
  cp -- "$bin" "$deb_root/usr/bin/openmind-desktop"
  chmod 755 -- "$deb_root/usr/bin/openmind-desktop"
  cp -- "$out/LICENSE" "$deb_root/usr/share/doc/openmind-desktop/LICENSE"
  cp -- "$out/README.md" "$deb_root/usr/share/doc/openmind-desktop/README.md"
  chmod 644 -- \
    "$deb_root/usr/share/doc/openmind-desktop/LICENSE" \
    "$deb_root/usr/share/doc/openmind-desktop/README.md"
  {
    printf 'Package: openmind-desktop\n'
    printf 'Version: %s\n' "$ver"
    printf 'Architecture: %s\n' "$deb_arch"
    printf 'Maintainer: 3000 Galion / galion-studio\n'
    printf 'Section: utils\n'
    printf 'Priority: optional\n'
    if [ -n "$deps" ]; then
      printf 'Depends: %s\n' "$deps"
    fi
    printf 'Description: Local control console for the Open Mind desktop\n'
    printf ' Loopback-only page. Tones are integer ratios of 432 Hz.\n'
    printf ' Workflow artifact, not a signed GitHub Release.\n'
  } > "$deb_root/DEBIAN/control"
  chmod 644 -- "$deb_root/DEBIAN/control"
  dpkg-deb --root-owner-group --build "$deb_root" "$out/$deb_name"
fi

stage="$work/stage"
mkdir -p -- "$stage"
pkg="$stage/$stem"
mkdir -p -- "$pkg"
cp -- "$bin" "$pkg/$bin_name"
chmod 755 -- "$pkg/$bin_name"
cp -- "$out/LICENSE" "$pkg/LICENSE"
cp -- "$out/README.md" "$pkg/README.md"
if [ -n "$deb_name" ]; then
  cp -- "$out/$deb_name" "$pkg/$deb_name"
fi

if [ -n "$runner" ]; then
  if [ ! -x "$runner" ]; then
    desktop_die "runner is not executable: $runner"
  fi
  "$runner" "$pkg/$bin_name" > "$pkg/console.html"
else
  "$pkg/$bin_name" > "$pkg/console.html"
fi
grep -q 'data-desktop="control-console"' "$pkg/console.html" \
  || desktop_die "console.html is not the control console"
grep -q '432 Hz' "$pkg/console.html" \
  || desktop_die "console.html does not name 432 Hz"
desktop_write_note "$pkg/LOCAL-ARTIFACT.txt" "$ver" "$triple" "$archive_name" "$deb_name"

case "$format" in
  zip)
    if command -v zip >/dev/null 2>&1; then
      (cd "$stage" && zip -r -X "$out/$archive_name" "$stem")
    else
      tar -a -cf "$out/$archive_name" -C "$stage" "$stem"
    fi
    ;;
  linux|tar)
    tar -C "$stage" -czf "$out/$archive_name" "$stem"
    ;;
esac

if [ -n "$deb_name" ]; then
  desktop_write_sums "$out" \
    "$out/LICENSE" \
    "$out/README.md" \
    "$out/$archive_name" \
    "$out/$deb_name"
else
  desktop_write_sums "$out" \
    "$out/LICENSE" \
    "$out/README.md" \
    "$out/$archive_name"
fi

printf '%s: local artifact %s\n' "$desktop_prefix" "$out/$archive_name"
if [ -n "$deb_name" ]; then
  printf '%s: deb %s\n' "$desktop_prefix" "$out/$deb_name"
fi
printf '%s: checksums %s\n' "$desktop_prefix" "$out/SHA256SUMS"
printf '%s: not a release; nothing was published or signed\n' "$desktop_prefix"
