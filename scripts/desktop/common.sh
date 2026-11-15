# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Shared helpers for the openmind-desktop artifact scripts.
# Source this file. Do not execute it.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  printf 'desktop: source this file; do not execute it\n' >&2
  exit 2
fi

desktop_die() {
  printf '%s: %s\n' "$desktop_prefix" "$1" >&2
  exit 1
}

desktop_usage_fail() {
  printf '%s: %s\n' "$desktop_prefix" "$1" >&2
  exit 2
}

# Refuse before any output directory is created.
desktop_refuse_publishing() {
  local arg
  for arg in "$@"; do
    case "$arg" in
      --publish|--upload|--gh-release|--cargo-publish|--sign|--signed|publish|upload|sign)
        printf '%s: refusing to publish, upload, or sign (%s)\n' "$desktop_prefix" "$arg" >&2
        printf '%s: local artifacts only; no release is created\n' "$desktop_prefix" >&2
        exit 2
        ;;
    esac
  done
  if [ "${OPENMIND_DESKTOP_PUBLISH:-}" = "1" ]; then
    printf '%s: refusing to publish, upload, or sign (OPENMIND_DESKTOP_PUBLISH)\n' "$desktop_prefix" >&2
    exit 2
  fi
  if [ "${OPENMIND_DESKTOP_SIGN:-}" = "1" ]; then
    printf '%s: refusing to publish, upload, or sign (OPENMIND_DESKTOP_SIGN)\n' "$desktop_prefix" >&2
    exit 2
  fi
}

desktop_init() {
  local src=$1
  desktop_script_dir=$(CDPATH= cd -- "$(dirname -- "$src")" && pwd)
  desktop_root=$(CDPATH= cd -- "$desktop_script_dir/../.." && pwd)
}

# workspace.package version in the root Cargo.toml.
desktop_version() {
  local ver
  ver=$(awk '
    $0 == "[workspace.package]" { in_pkg = 1; next }
    in_pkg && $0 ~ /^\[/ { in_pkg = 0 }
    in_pkg && $1 == "version" {
      gsub(/"/, "", $3)
      print $3
      exit
    }
  ' "$desktop_root/Cargo.toml")
  if ! printf '%s' "$ver" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    desktop_die "workspace version is not major.minor.patch"
  fi
  printf '%s' "$ver"
}

# linux, zip, or tar. Empty output and status 1 when the triple is unknown.
desktop_format() {
  case "$1" in
    *-unknown-linux-gnu|*-unknown-linux-musl) printf 'linux' ;;
    *-pc-windows-msvc) printf 'zip' ;;
    *-apple-darwin) printf 'tar' ;;
    *) return 1 ;;
  esac
}

desktop_deb_arch() {
  case "$1" in
    x86_64-*-linux-*) printf 'amd64' ;;
    aarch64-*-linux-*) printf 'arm64' ;;
    *) return 1 ;;
  esac
}

desktop_bin_name() {
  case "$1" in
    *-pc-windows-*) printf 'openmind-desktop.exe' ;;
    *) printf 'openmind-desktop' ;;
  esac
}

desktop_readelf() {
  if command -v readelf >/dev/null 2>&1; then
    command -v readelf
    return 0
  fi
  if command -v llvm-readelf >/dev/null 2>&1; then
    command -v llvm-readelf
    return 0
  fi
  return 1
}

# Debian Depends value for a Linux binary, or empty when it is static.
# Unknown NEEDED entries are a package error: the mapping is not guessed.
desktop_deb_depends() {
  local bin=$1
  local tool sonames soname libc=0 gcc=0 deps=
  tool=$(desktop_readelf) || desktop_die "readelf is required to read NEEDED entries"
  sonames=$("$tool" -d "$bin" 2>/dev/null | sed -n 's/.*Shared library: \[\([^]]*\)\]/\1/p' || true)
  if [ -n "$sonames" ]; then
    while IFS= read -r soname; do
      [ -z "$soname" ] && continue
      case "$soname" in
        libc.so.6|libm.so.6|libdl.so.2|libpthread.so.0|ld-linux-*.so.*)
          libc=1
          ;;
        libgcc_s.so.1)
          gcc=1
          ;;
        *)
          desktop_die "shared library $soname has no Debian package mapping"
          ;;
      esac
    done << EOF
$sonames
EOF
  fi
  if [ "$libc" -eq 1 ]; then
    deps="libc6"
  fi
  if [ "$gcc" -eq 1 ]; then
    if [ -n "$deps" ]; then
      deps="$deps, libgcc-s1"
    else
      deps="libgcc-s1"
    fi
  fi
  printf '%s' "$deps"
}

# Root MIT text, one blank line, then scripts/desktop/brand-lines.txt.
# Those four lines are BRAND_LINES in crates/openmind-desktop/src/page.rs.
desktop_assemble_license() {
  local dest=$1
  cat -- "$desktop_root/LICENSE" > "$dest"
  if [ -n "$(tail -c 1 "$dest")" ]; then
    printf '\n' >> "$dest"
  fi
  printf '\n' >> "$dest"
  cat -- "$desktop_script_dir/brand-lines.txt" >> "$dest"
}

# Short README plus the version and triple of this build.
desktop_assemble_readme() {
  local dest=$1 ver=$2 triple=$3
  cat -- "$desktop_script_dir/artifact-readme.md" > "$dest"
  if [ -n "$(tail -c 1 "$dest")" ]; then
    printf '\n' >> "$dest"
  fi
  printf '\n## This build\n\nversion: %s\ntarget: %s\n' "$ver" "$triple" >> "$dest"
}

desktop_write_note() {
  local dest=$1 ver=$2 triple=$3 archive=$4 deb=$5
  {
    printf 'openmind-desktop workflow artifact\n'
    printf 'version: %s\n' "$ver"
    printf 'target: %s\n' "$triple"
    printf 'archive: %s\n' "$archive"
    if [ -n "$deb" ]; then
      printf 'deb: %s\n' "$deb"
    fi
    printf 'This file is a workflow artifact. It is not a GitHub Release and it was not published.\n'
    printf 'It is not signed.\n'
  } > "$dest"
}

desktop_sha256() {
  local hash
  if command -v sha256sum >/dev/null 2>&1; then
    hash=$(sha256sum -- "$1" | awk '{print $1}')
  elif command -v shasum >/dev/null 2>&1; then
    hash=$(shasum -a 256 -- "$1" | awk '{print $1}')
  else
    desktop_die "sha256sum or shasum is required"
  fi
  if ! printf '%s' "$hash" | grep -Eq '^[0-9a-f]{64}$'; then
    desktop_die "hash of $(basename -- "$1") is not 64 lowercase hex digits"
  fi
  printf '%s' "$hash"
}

# SHA256SUMS in the directory, two spaces, sorted by file name.
# Arguments are files inside that directory. The sums file is not an argument.
desktop_write_sums() {
  local dir=$1
  shift
  local tmp f base hash
  tmp=$(mktemp)
  : > "$tmp"
  for f in "$@"; do
    base=$(basename -- "$f")
    case "$base" in
      *[!A-Za-z0-9._-]*)
        desktop_die "checksum name has unexpected characters: $base"
        ;;
    esac
    hash=$(desktop_sha256 "$f")
    printf '%s  %s\n' "$hash" "$base" >> "$tmp"
  done
  LC_ALL=C sort -k 2,2 -o "$dir/SHA256SUMS" "$tmp"
  rm -f -- "$tmp"
}

desktop_cmp() {
  local what=$1 want=$2 got=$3
  # No "--": macOS cmp and diff treat it as a file name.
  if ! cmp -s "$want" "$got"; then
    diff -u "$want" "$got" >&2 || true
    desktop_die "$what does not match"
  fi
}

# True when the file starts with a local-file zip signature.
desktop_zip_magic() {
  local magic
  if [ ! -f "$1" ]; then
    return 1
  fi
  if ! command -v od >/dev/null 2>&1; then
    desktop_die "od is required"
  fi
  magic=$(od -An -t x1 -N 4 "$1" | tr -d ' \t\n' | tr 'A-F' 'a-f')
  case "$magic" in
    504b0304|504b0506|504b0708) return 0 ;;
    *) return 1 ;;
  esac
}

# Write a zip of $stem inside $stage to $dest. GNU tar -a names a tar ".zip",
# so a ".zip" suffix is not enough. The result must start with a PK signature.
desktop_write_zip() {
  local dest=$1 stage=$2 stem=$3
  rm -f "$dest"
  if command -v zip >/dev/null 2>&1; then
    (cd "$stage" && zip -r -X "$dest" "$stem") || desktop_die "zip failed"
    if desktop_zip_magic "$dest"; then
      return 0
    fi
    rm -f "$dest"
  fi
  # bsdtar. GNU tar rejects --format=zip and must not leave a tar behind.
  if tar --format=zip -cf "$dest" -C "$stage" "$stem" 2>/dev/null && desktop_zip_magic "$dest"; then
    return 0
  fi
  rm -f "$dest"
  if desktop_try_powershell_zip "$dest" "$stage/$stem" && desktop_zip_magic "$dest"; then
    return 0
  fi
  desktop_die "could not write a zip archive"
}

# Compress-Archive stores backslashes. Info-ZIP then warns and exits 1, and a
# Linux unzip keeps the backslash in the name. Entry names are written with
# forward slashes instead.
desktop_try_powershell_zip() {
  local dest=$1 src=$2 ps="" win_dest win_src win_ps1 status
  local work ps1
  if command -v powershell.exe >/dev/null 2>&1; then
    ps=powershell.exe
  elif command -v pwsh >/dev/null 2>&1; then
    ps=pwsh
  else
    return 1
  fi
  work=$(mktemp -d)
  ps1="$work/zip.ps1"
  cat > "$ps1" << 'EOF'
param(
  [Parameter(Mandatory = $true)][string]$Dest,
  [Parameter(Mandatory = $true)][string]$Src
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
if (Test-Path -LiteralPath $Dest) {
  Remove-Item -LiteralPath $Dest
}
$zip = [System.IO.Compression.ZipFile]::Open(
  $Dest,
  [System.IO.Compression.ZipArchiveMode]::Create
)
try {
  $srcFull = [System.IO.Path]::GetFullPath($Src).TrimEnd('\')
  $parent = [System.IO.Path]::GetDirectoryName($srcFull)
  $files = @(Get-ChildItem -LiteralPath $srcFull -Recurse -File)
  if ($files.Count -eq 0) {
    throw "no files under $Src"
  }
  foreach ($file in $files) {
    $rel = $file.FullName.Substring($parent.Length).TrimStart('\')
    $rel = $rel.Replace('\', '/')
    if ($rel -match '\\' -or $rel.StartsWith('/') -or $rel.Contains('..')) {
      throw "refusing zip entry $rel"
    }
    [void][System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
      $zip,
      $file.FullName,
      $rel,
      [System.IO.Compression.CompressionLevel]::Optimal
    )
  }
} finally {
  $zip.Dispose()
}
EOF
  if command -v cygpath >/dev/null 2>&1; then
    win_dest=$(cygpath -w "$dest")
    win_src=$(cygpath -w "$src")
    win_ps1=$(cygpath -w "$ps1")
  else
    win_dest=$dest
    win_src=$src
    win_ps1=$ps1
  fi
  "$ps" -NoProfile -NonInteractive -File "$win_ps1" -Dest "$win_dest" -Src "$win_src"
  status=$?
  rm -rf "$work"
  return "$status"
}
