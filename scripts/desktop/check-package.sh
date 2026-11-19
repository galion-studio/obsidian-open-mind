#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Check a local openmind-desktop workflow artifact directory.
# Does not upload, publish, sign, or create a release.
set -euo pipefail

usage() {
  cat << 'EOF'
usage: scripts/desktop/check-package.sh [--runner CMD] DIR

DIR is the directory written by scripts/desktop/package-artifact.sh.
It must contain SHA256SUMS, LICENSE, README.md, and one archive.
A Linux archive's directory also contains one .deb.

Checks the checksums, the MIT text plus the four Galion Labs lines,
the short README, and the archive. Linux also checks the .deb: root
ownership, no maintainer scripts, no signature member, and the same
binary, LICENSE, and README. The packaged binary is run and its page
must match console.html. Pass --runner for a cross binary.

This script never publishes or signs. --publish, --upload, --gh-release,
--cargo-publish, --sign and --signed are refused.

exit 0  the artifact directory matches
exit 2  refused publish or sign flag, or bad arguments
exit 1  checksum or contents failed
EOF
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=common.sh
. "$script_dir/common.sh"
desktop_prefix=desktop-check
desktop_init "$0"
desktop_refuse_publishing "$@"

runner=""
dir=""

check_magic() {
  local bin=$1 triple=$2
  local ident machine tool interp pe_hex pe_off size sig
  case "$triple" in
    *-linux-*)
      ident=$(desktop_hex_at "$bin" 0 6)
      case "$ident" in
        7f454c460201*) ;;
        *) desktop_die "binary is not a 64-bit little-endian ELF" ;;
      esac
      machine=$(desktop_hex_at "$bin" 18 2)
      case "$triple" in
        x86_64-*)
          [ "$machine" = "3e00" ] || desktop_die "ELF machine is not x86_64"
          ;;
        aarch64-*)
          [ "$machine" = "b700" ] || desktop_die "ELF machine is not aarch64"
          ;;
      esac
      tool=$(desktop_readelf) || desktop_die "readelf is required to check the Linux binary"
      if "$tool" -l "$bin" | grep -q 'INTERP'; then
        interp=1
      else
        interp=0
      fi
      case "$triple" in
        *linux-musl*)
          [ "$interp" -eq 0 ] || desktop_die "musl triple but the binary has an interpreter"
          ;;
        *linux-gnu*)
          [ "$interp" -eq 1 ] || desktop_die "gnu triple but the binary is static"
          ;;
      esac
      ;;
    *-pc-windows-msvc)
      ident=$(desktop_hex_at "$bin" 0 2)
      [ "$ident" = "4d5a" ] || desktop_die "binary is not a PE executable"
      pe_hex=$(desktop_hex_at "$bin" 60 4)
      pe_off=$((0x${pe_hex:0:2} + 0x${pe_hex:2:2} * 256 + 0x${pe_hex:4:2} * 65536 + 0x${pe_hex:6:2} * 16777216))
      size=$(wc -c < "$bin")
      size=${size// /}
      if [ "$pe_off" -lt 64 ] || [ "$pe_off" -gt $((size - 6)) ]; then
        desktop_die "PE header offset is out of range"
      fi
      sig=$(desktop_hex_at "$bin" "$pe_off" 4)
      [ "$sig" = "50450000" ] || desktop_die "PE signature is missing"
      machine=$(desktop_hex_at "$bin" $((pe_off + 4)) 2)
      case "$triple" in
        x86_64-*)
          [ "$machine" = "6486" ] || desktop_die "PE machine is not x86_64"
          ;;
        aarch64-*)
          [ "$machine" = "64aa" ] || desktop_die "PE machine is not aarch64"
          ;;
      esac
      ;;
    *-apple-darwin)
      ident=$(desktop_hex_at "$bin" 0 4)
      [ "$ident" = "cffaedfe" ] || desktop_die "binary is not a little-endian 64-bit Mach-O"
      machine=$(desktop_hex_at "$bin" 4 4)
      case "$triple" in
        aarch64-*)
          [ "$machine" = "0c000001" ] || desktop_die "Mach-O cputype is not arm64"
          ;;
        x86_64-*)
          [ "$machine" = "07000001" ] || desktop_die "Mach-O cputype is not x86_64"
          ;;
      esac
      ;;
  esac
}

desktop_hex_at() {
  local file=$1 off=$2 len=$3
  if ! command -v od >/dev/null 2>&1; then
    desktop_die "od is required"
  fi
  od -An -t x1 -j "$off" -N "$len" "$file" | tr -d ' \t\n' | tr 'A-F' 'a-f'
}

check_deb() {
  local deb=$1 triple=$2 ver=$3 deb_arch=$4 bin=$5
  local member field got ctrl data
  if ! command -v dpkg-deb >/dev/null 2>&1; then
    desktop_die "dpkg-deb is required to check the deb"
  fi
  if ! command -v ar >/dev/null 2>&1; then
    desktop_die "ar is required to check the deb"
  fi
  members=$(ar t "$deb")
  printf '%s\n' "$members" | grep -Eq '^debian-binary$' || desktop_die "deb has no debian-binary member"
  printf '%s\n' "$members" | grep -Eq '^control\.tar\.' || desktop_die "deb has no control tar"
  printf '%s\n' "$members" | grep -Eq '^data\.tar\.' || desktop_die "deb has no data tar"
  count=$(printf '%s\n' "$members" | grep -c .)
  if [ "$count" -ne 3 ]; then
    desktop_die "deb has extra archive members"
  fi
  if printf '%s\n' "$members" | grep -Eq 'sign|_gpg|asc$'; then
    desktop_die "deb looks signed"
  fi

  field() {
    dpkg-deb -f "$deb" "$1" | tr -d '\r\n'
  }
  [ "$(field Package)" = "openmind-desktop" ] || desktop_die "deb Package is not openmind-desktop"
  [ "$(field Version)" = "$ver" ] || desktop_die "deb Version does not match the archive"
  [ "$(field Architecture)" = "$deb_arch" ] || desktop_die "deb Architecture does not match the target"
  [ "$(field Maintainer)" = "3000 Galion / galion-studio" ] || desktop_die "deb Maintainer changed"
  [ "$(field Section)" = "utils" ] || desktop_die "deb Section is not utils"
  [ "$(field Priority)" = "optional" ] || desktop_die "deb Priority is not optional"
  got=$(field Depends || true)
  want=$(desktop_deb_depends "$bin")
  [ "$got" = "$want" ] || desktop_die "deb Depends is '$got', binary needs '$want'"

  ctrl="$unpack/deb-control"
  data="$unpack/deb-data"
  mkdir -p -- "$ctrl" "$data"
  dpkg-deb -e "$deb" "$ctrl"
  for member in "$ctrl"/*; do
    case "$(basename -- "$member")" in
      control|md5sums) ;;
      *) desktop_die "deb has a maintainer script or extra control file: $(basename -- "$member")" ;;
    esac
  done
  if grep -E 'https?://|([0-9]{1,3}\.){3}[0-9]{1,3}' "$ctrl/control" >/dev/null; then
    desktop_die "deb control contains a URL or an IPv4 address"
  fi
  grep -q '432 Hz' "$ctrl/control" || desktop_die "deb description does not name 432 Hz"
  grep -q 'not a signed GitHub Release' "$ctrl/control" || desktop_die "deb description does not say this is unsigned"

  dpkg-deb -c "$deb" > "$unpack/deb-list"
  if grep -v 'root/root' "$unpack/deb-list" | grep -q .; then
    desktop_die "deb entry is not owned by root"
  fi
  grep -E 'rwxr-xr-x .* \./usr/bin/openmind-desktop$' "$unpack/deb-list" >/dev/null \
    || desktop_die "deb binary is not mode 755 at /usr/bin/openmind-desktop"
  grep -E 'rw-r--r-- .* \./usr/share/doc/openmind-desktop/LICENSE$' "$unpack/deb-list" >/dev/null \
    || desktop_die "deb LICENSE is missing or not mode 644"
  grep -E 'rw-r--r-- .* \./usr/share/doc/openmind-desktop/README.md$' "$unpack/deb-list" >/dev/null \
    || desktop_die "deb README is missing or not mode 644"

  paths="$unpack/deb-paths"
  awk '{print $NF}' "$unpack/deb-list" | LC_ALL=C sort > "$paths"
  while IFS= read -r path; do
    case "$path" in
      ./|./usr|./usr/|./usr/bin|./usr/bin/|./usr/share|./usr/share/|./usr/share/doc|./usr/share/doc/|./usr/share/doc/openmind-desktop|./usr/share/doc/openmind-desktop/)
        ;;
      ./usr/bin/openmind-desktop|./usr/share/doc/openmind-desktop/LICENSE|./usr/share/doc/openmind-desktop/README.md)
        ;;
      *)
        desktop_die "unexpected deb path: $path"
        ;;
    esac
  done < "$paths"

  dpkg-deb -x "$deb" "$data"
  desktop_cmp "deb binary" "$bin" "$data/usr/bin/openmind-desktop"
  desktop_cmp "deb LICENSE" "$dir/LICENSE" "$data/usr/share/doc/openmind-desktop/LICENSE"
  desktop_cmp "deb README" "$dir/README.md" "$data/usr/share/doc/openmind-desktop/README.md"
}


while [ $# -gt 0 ]; do
  case "$1" in
    --help|-h)
      usage
      exit 0
      ;;
    --runner)
      if [ $# -lt 2 ]; then
        usage >&2
        exit 2
      fi
      runner=$2
      shift 2
      ;;
    --)
      shift
      break
      ;;
    -*)
      desktop_usage_fail "unknown argument $1"
      ;;
    *)
      if [ -n "$dir" ]; then
        desktop_usage_fail "unexpected argument $1"
      fi
      dir=$1
      shift
      ;;
  esac
done

if [ $# -gt 0 ]; then
  desktop_usage_fail "unexpected argument $1"
fi

case "$runner" in
  "") ;;
  *[!A-Za-z0-9_./-]*)
    desktop_usage_fail "runner path has unexpected characters"
    ;;
esac

if [ -z "$dir" ]; then
  usage >&2
  exit 2
fi
if [ ! -d "$dir" ]; then
  desktop_die "not a directory: $dir"
fi
dir=$(CDPATH= cd -- "$dir" && pwd)

shopt -s nullglob
saw_license=0
saw_readme=0
saw_sums=0
archive=""
deb=""
for f in "$dir"/*; do
  if [ -L "$f" ] || [ ! -f "$f" ]; then
    desktop_die "not a regular file: $(basename -- "$f")"
  fi
  base=$(basename -- "$f")
  case "$base" in
    LICENSE) saw_license=1 ;;
    README.md) saw_readme=1 ;;
    SHA256SUMS) saw_sums=1 ;;
    openmind-desktop-*.tar.gz|openmind-desktop-*.zip)
      if [ -n "$archive" ]; then
        desktop_die "more than one archive"
      fi
      archive=$f
      ;;
    openmind-desktop_*.deb)
      if [ -n "$deb" ]; then
        desktop_die "more than one deb"
      fi
      deb=$f
      ;;
    *)
      desktop_die "unexpected file $base"
      ;;
  esac
done
shopt -u nullglob

if [ "$saw_license" -ne 1 ]; then
  desktop_die "missing LICENSE"
fi
if [ "$saw_readme" -ne 1 ]; then
  desktop_die "missing README.md"
fi
if [ "$saw_sums" -ne 1 ]; then
  desktop_die "missing SHA256SUMS"
fi
if [ -z "$archive" ]; then
  desktop_die "missing archive"
fi

base=$(basename -- "$archive")
case "$base" in
  *.tar.gz) stem=${base%.tar.gz} ;;
  *.zip) stem=${base%.zip} ;;
  *) desktop_die "archive name is not a tarball or zip" ;;
esac
rest=${stem#openmind-desktop-}
if [ "$rest" = "$stem" ]; then
  desktop_die "archive name does not start with openmind-desktop-"
fi
ver=${rest%%-*}
triple=${rest#*-}
if [ "$ver" = "$rest" ] || [ -z "$triple" ] || [ "$triple" = "$rest" ]; then
  desktop_die "archive name has no version and target"
fi
if ! printf '%s' "$ver" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  desktop_die "archive version is not major.minor.patch"
fi
format=$(desktop_format "$triple") || desktop_die "unsupported target in archive name: $triple"
case "$format" in
  zip)
    case "$base" in
      *.zip) ;;
      *) desktop_die "Windows target is not a zip" ;;
    esac
    if [ -n "$deb" ]; then
      desktop_die "zip artifact must not contain a deb"
    fi
    ;;
  linux)
    case "$base" in
      *.tar.gz) ;;
      *) desktop_die "Linux target is not a tarball" ;;
    esac
    if [ -z "$deb" ]; then
      desktop_die "Linux artifact is missing a deb"
    fi
    deb_arch=$(desktop_deb_arch "$triple") || desktop_die "no Debian architecture for $triple"
    deb_base=$(basename -- "$deb")
    if [ "$deb_base" != "openmind-desktop_${ver}_${deb_arch}.deb" ]; then
      desktop_die "deb name does not match version and architecture"
    fi
    ;;
  tar)
    case "$base" in
      *.tar.gz) ;;
      *) desktop_die "macOS target is not a tarball" ;;
    esac
    if [ -n "$deb" ]; then
      desktop_die "macOS artifact must not contain a deb"
    fi
    ;;
esac

unpack=$(mktemp -d)
expected="$unpack/expected-files"
trap 'rm -rf -- "$unpack"' EXIT
{
  printf '%s\n' LICENSE README.md "$base"
  if [ -n "$deb" ]; then
    printf '%s\n' "$(basename -- "$deb")"
  fi
} | LC_ALL=C sort > "$expected"

listed="$unpack/listed"
: > "$listed"
while IFS= read -r line || [ -n "$line" ]; do
  if [ -z "$line" ]; then
    continue
  fi
  if ! printf '%s\n' "$line" | grep -Eq '^[0-9a-f]{64}  [A-Za-z0-9._-]+$'; then
    desktop_die "SHA256SUMS line is not a 64-digit hash and a local file name"
  fi
  name=${line#*  }
  case "$name" in
    SHA256SUMS|*/*|../*|*/../*|*://*)
      desktop_die "checksum path is not a local file name"
      ;;
  esac
  if [ ! -f "$dir/$name" ]; then
    desktop_die "SHA256SUMS lists a missing file: $name"
  fi
  printf '%s\n' "$name" >> "$listed"
done < "$dir/SHA256SUMS"
LC_ALL=C sort -o "$listed" "$listed"
desktop_cmp "SHA256SUMS file list" "$expected" "$listed"

(
  cd "$dir"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c SHA256SUMS
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c SHA256SUMS
  else
    desktop_die "sha256sum or shasum is required"
  fi
)

want_license="$unpack/LICENSE"
want_readme="$unpack/README.md"
desktop_assemble_license "$want_license"
desktop_assemble_readme "$want_readme" "$ver" "$triple"
desktop_cmp "LICENSE" "$want_license" "$dir/LICENSE"
desktop_cmp "README.md" "$want_readme" "$dir/README.md"
readme_lines=$(wc -l < "$dir/README.md")
readme_lines=${readme_lines// /}
if [ "$readme_lines" -gt 80 ]; then
  desktop_die "README.md is $readme_lines lines; the artifact readme stays at most 80"
fi
grep -q 'not a GitHub Release' "$dir/README.md" || desktop_die "README does not say this is not a release"
grep -q 'not signed' "$dir/README.md" || desktop_die "README does not say this is not signed"
grep -q '432 Hz' "$dir/README.md" || desktop_die "README does not name 432 Hz"
if grep -E 'https?://|([0-9]{1,3}\.){3}[0-9]{1,3}' "$dir/README.md" "$dir/LICENSE" >/dev/null; then
  desktop_die "LICENSE or README contains a URL or an IPv4 address"
fi

top="$unpack/tree"
mkdir -p -- "$top"
case "$base" in
  *.tar.gz)
    while IFS= read -r entry; do
      case "$entry" in
        ""|./) continue ;;
        /*|*..*) desktop_die "archive path is not local: $entry" ;;
      esac
    done << EOF
$(tar -tzf "$dir/$base")
EOF
    if tar -tvzf "$dir/$base" | grep -q '^l'; then
      desktop_die "archive contains a symlink"
    fi
    tar -C "$top" -xzf "$dir/$base"
    ;;
  *.zip)
    if ! desktop_zip_magic "$dir/$base"; then
      desktop_die "archive is not a zip"
    fi
    names=""
    if command -v zipinfo >/dev/null 2>&1; then
      names=$(zipinfo -1 "$dir/$base" 2>/dev/null || true)
    fi
    if [ -z "$names" ] && command -v unzip >/dev/null 2>&1; then
      names=$(unzip -Z1 "$dir/$base" 2>/dev/null || true)
    fi
    if [ -z "$names" ]; then
      names=$(tar -tf "$dir/$base" 2>/dev/null || true)
    fi
    if [ -z "$names" ]; then
      desktop_die "could not list the zip"
    fi
    while IFS= read -r entry; do
      [ -z "$entry" ] && continue
      case "$entry" in
        *\\*) desktop_die "zip entry uses a backslash: $entry" ;;
        /*|*..*) desktop_die "archive path is not local: $entry" ;;
      esac
    done << EOF
$names
EOF
    if command -v unzip >/dev/null 2>&1; then
      # Info-ZIP uses exit 1 for a warning. A real failure is 2 or higher.
      set +e
      unzip -q "$dir/$base" -d "$top"
      status=$?
      set -e
      if [ "$status" -gt 1 ]; then
        desktop_die "unzip failed ($status)"
      fi
    else
      tar -xf "$dir/$base" -C "$top"
    fi
    ;;
esac

if [ ! -d "$top/$stem" ]; then
  desktop_die "archive does not contain $stem"
fi
pkg="$top/$stem"
shopt -s nullglob dotglob
got_names="$unpack/names"
: > "$got_names"
for entry in "$pkg"/*; do
  name=$(basename -- "$entry")
  if [ "$name" = "." ] || [ "$name" = ".." ]; then
    continue
  fi
  if [ -L "$entry" ] || [ ! -f "$entry" ]; then
    desktop_die "archive entry is not a regular file: $name"
  fi
  printf '%s\n' "$name" >> "$got_names"
done
shopt -u nullglob dotglob
if [ -n "$(find "$pkg" -mindepth 2 -print)" ]; then
  desktop_die "archive has nested paths"
fi

bin_name=$(desktop_bin_name "$triple")
want_names="$unpack/want-names"
{
  printf '%s\n' "$bin_name" LICENSE README.md console.html LOCAL-ARTIFACT.txt
  if [ -n "$deb" ]; then
    printf '%s\n' "$(basename -- "$deb")"
  fi
} | LC_ALL=C sort > "$want_names"
LC_ALL=C sort -o "$got_names" "$got_names"
desktop_cmp "archive file list" "$want_names" "$got_names"

desktop_cmp "archived LICENSE" "$dir/LICENSE" "$pkg/LICENSE"
desktop_cmp "archived README.md" "$dir/README.md" "$pkg/README.md"
want_note="$unpack/LOCAL-ARTIFACT.txt"
if [ -n "$deb" ]; then
  desktop_write_note "$want_note" "$ver" "$triple" "$base" "$(basename -- "$deb")"
else
  desktop_write_note "$want_note" "$ver" "$triple" "$base" ""
fi
desktop_cmp "LOCAL-ARTIFACT.txt" "$want_note" "$pkg/LOCAL-ARTIFACT.txt"
grep -q 'data-desktop="control-console"' "$pkg/console.html" || desktop_die "console.html is not the control console"
grep -q '432 Hz' "$pkg/console.html" || desktop_die "console.html does not name 432 Hz"

bin="$pkg/$bin_name"
check_magic "$bin" "$triple"

host_os=$(uname -s)
host_mach=$(uname -m)
case "$host_mach" in
  arm64) host_mach=aarch64 ;;
esac
native=0
case "$triple" in
  x86_64-*) arch=x86_64 ;;
  aarch64-*) arch=aarch64 ;;
  *) desktop_die "no machine for $triple" ;;
esac
if [ "$host_mach" = "$arch" ]; then
  case "$triple" in
    *-linux-*)
      [ "$host_os" = "Linux" ] && native=1
      ;;
    *-apple-darwin)
      [ "$host_os" = "Darwin" ] && native=1
      ;;
    *-pc-windows-msvc)
      case "$host_os" in
        MINGW*|MSYS*|CYGWIN*) native=1 ;;
      esac
      ;;
  esac
fi

page="$unpack/page.html"
chmod +x "$bin" 2>/dev/null || true
if [ "$native" -eq 1 ]; then
  "$bin" > "$page"
elif [ -n "$runner" ]; then
  if [ ! -x "$runner" ]; then
    desktop_die "runner is not executable: $runner"
  fi
  "$runner" "$bin" > "$page"
else
  desktop_die "cannot run $triple on $host_os $host_mach; pass --runner"
fi
desktop_cmp "console.html" "$page" "$pkg/console.html"

if [ "$format" = "linux" ]; then
  desktop_cmp "deb inside the tarball" "$deb" "$pkg/$(basename -- "$deb")"
  check_deb "$deb" "$triple" "$ver" "$deb_arch" "$pkg/$bin_name"
fi

printf '%s: ok %s\n' "$desktop_prefix" "$dir"

