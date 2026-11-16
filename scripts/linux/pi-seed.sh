#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
#
# Static musl seed of the release binaries (config/release/size-budget.tsv,
# every named row except `default`, including openmind-desktop) for
# aarch64-unknown-linux-musl and x86_64-unknown-linux-musl.
#
# Profile: cargo --profile pi-seed (opt-level 3, lto, codegen-units 1,
# panic abort, strip). The workspace [profile.release] stays opt-level "z"
# for the 2 MiB budget. This script does not call size-budget.sh.
#
# Linker: rust-lld. CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER and the
# x86_64 twin are exported below. .cargo/config.toml sets the same linker.
# Bundled SQLite uses the zig cc wrappers from scripts/linux/build-musl.sh.
# The -fno-inline size knob in that config is not applied here.
#
# verify runs each binary's --help / --version probes. aarch64 runs under
# qemu-user. That is not a Raspberry Pi 5 boot. pack writes
# obsidian-pi5-seed-<sha>.tar.gz (bin/, LICENSE, SHA256SUMS, and systemd
# user units when the tree has any). It does not publish.
set -euo pipefail

# One probe line per named size-budget binary. Flags are only --help or
# --version, and each one must exit 0 without binding a socket.
# crates/galion-realtest/tests/pi_seed.rs reads the lines and requires the
# names to match config/release/size-budget.tsv.
# probe: desk-hermes --help --version
# probe: gaia-cortex --version
# probe: galion-backup --help --version
# probe: galion-console-serve --help
# probe: galion-console-static --help
# probe: galion-desk --help --version
# probe: galion-health --version
# probe: galion-one --version
# probe: hermes-gateway --help --version
# probe: link-watch --help --version
# probe: openmind-desktop --help --version
# probe: openmind-memory --version
# probe: openmind-nerve --help --version
# probe: openmind-queue --help --version
# probe: tuner --help --version

usage() {
  cat << 'EOF'
usage: scripts/linux/pi-seed.sh build TARGET
       scripts/linux/pi-seed.sh verify TARGET [QEMU]
       scripts/linux/pi-seed.sh pack SHA AARCH64_DIR X86_64_DIR OUT_DIR

TARGET is aarch64-unknown-linux-musl or x86_64-unknown-linux-musl.

build    cargo --profile pi-seed --locked, then copy binaries to
         target/pi-seed-out/TARGET/
verify   static ELF check and the --help/--version probes. aarch64 uses
         QEMU, or qemu-aarch64-static / qemu-aarch64 on PATH.
pack     write OUT_DIR/obsidian-pi5-seed-SHA.tar.gz

exit 0  the subcommand finished
exit 2  bad arguments
exit 1  build, check, smoke, or pack failed
EOF
}

die() {
  printf 'pi-seed: %s\n' "$1" >&2
  exit 1
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
cd "$root"

budget=$root/config/release/size-budget.tsv

bins() {
  awk '
    NF < 2 { next }
    $1 ~ /^#/ { next }
    $1 == "default" { next }
    { print $1 }
  ' "$budget"
}

flags_for() {
  awk -v name="$1" '
    $1 == "#" && $2 == "probe:" && $3 == name {
      for (i = 4; i <= NF; i++) print $i
    }
  ' "$0"
}

pkg_for() {
  case "$1" in
    galion-console-static|galion-console-serve) printf '%s\n' console-static ;;
    *) printf '%s\n' "$1" ;;
  esac
}

require_target() {
  case "$1" in
    x86_64-unknown-linux-musl|aarch64-unknown-linux-musl) ;;
    *)
      usage >&2
      exit 2
      ;;
  esac
}

setup_linker_path() {
  local sysroot host lld_dir
  sysroot=$(rustc --print sysroot)
  host=$(rustc -vV | sed -n 's/^host: //p')
  host=${host%%[!A-Za-z0-9_.-]*}
  lld_dir=$sysroot/lib/rustlib/$host/bin
  if [ -x "$lld_dir/rust-lld" ]; then
    PATH=$lld_dir:$PATH
    export PATH
  fi
  if ! command -v rust-lld >/dev/null 2>&1; then
    die "rust-lld not found (rustup toolchain rustlib bin)"
  fi
  export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld
  export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld
}

setup_zig() {
  local target=$1 zig_bin zig_target wrap triple_us cflags
  case "$target" in
    x86_64-unknown-linux-musl) zig_target=x86_64-linux-musl ;;
    aarch64-unknown-linux-musl) zig_target=aarch64-linux-musl ;;
    *) die "no zig triple for $target" ;;
  esac
  zig_bin=$("$root/scripts/install-zig.sh" "$root/target/pi-seed-zig")
  wrap=$root/target/pi-seed-zig-wrap
  mkdir -p "$wrap"
  cat > "$wrap/cc-$target" << WRAP
#!/bin/sh
for a do shift; case "\$a" in --target=*) continue ;; esac; set -- "\$@" "\$a"; done
exec "$zig_bin" cc -target $zig_target "\$@"
WRAP
  chmod +x "$wrap/cc-$target"
  printf '#!/bin/sh\nexec "%s" ar "$@"\n' "$zig_bin" > "$wrap/ar"
  chmod +x "$wrap/ar"
  triple_us=$(printf '%s' "$target" | tr '-' '_')
  export "CC_$triple_us=$wrap/cc-$target"
  export "AR_$triple_us=$wrap/ar"
  # .cargo/config.toml sets -fno-inline on these targets so the opt-level "z"
  # hermes-gateway stays under 2 MiB. Cargo [env] does not override a variable
  # that is already set. This seed is opt-level 3 and is not size-budgeted.
  # Unwind tables stay off (panic = abort). SQLite may inline.
  cflags="-fno-asynchronous-unwind-tables -fno-unwind-tables"
  export "CFLAGS_$triple_us=$cflags"
  printf 'pi-seed: zig %s cc %s linker %s\n' "$("$zig_bin" version)" "$wrap/cc-$target" "$(command -v rust-lld)"
}

out_dir() {
  printf '%s\n' "$root/target/pi-seed-out/$1"
}

cmd_build() {
  local target=$1 jobs nice_level rel out bin pkg seen
  require_target "$target"
  if ! rustup target list --installed | grep -qx "$target"; then
    rustup target add "$target"
  fi
  setup_linker_path
  setup_zig "$target"
  jobs=${PI_SEED_JOBS:-2}
  nice_level=${PI_SEED_NICE:-19}
  local -a args=()
  seen=" "
  while IFS= read -r bin; do
    [ -n "$bin" ] || continue
    pkg=$(pkg_for "$bin")
    case "$seen" in
      *" $pkg "*) ;;
      *)
        args+=(-p "$pkg")
        seen="$seen$pkg "
        ;;
    esac
  done < <(bins)
  [ "${#args[@]}" -gt 0 ] || die "no binaries in $budget"
  printf 'pi-seed: cargo build --profile pi-seed --locked --target %s %s\n' "$target" "${args[*]}"
  nice -n "$nice_level" cargo build -j "$jobs" --profile pi-seed --locked --target "$target" "${args[@]}"
  rel=$root/target/$target/pi-seed
  out=$(out_dir "$target")
  mkdir -p "$out"
  while IFS= read -r bin; do
    [ -n "$bin" ] || continue
    [ -f "$rel/$bin" ] || die "missing $rel/$bin"
    cp -- "$rel/$bin" "$out/$bin"
    chmod 755 "$out/$bin"
    printf 'pi-seed: staged %s bytes=%s\n' "$out/$bin" "$(stat -c '%s' -- "$out/$bin")"
  done < <(bins)
}

elf_ok() {
  local path=$1 target=$2 machine expect interp needed stripped
  machine=$(readelf -hW -- "$path" | sed -n 's/^ *Machine: *//p')
  case "$target" in
    aarch64-unknown-linux-musl) expect=AArch64 ;;
    x86_64-unknown-linux-musl) expect=X86-64 ;;
    *) die "no machine check for $target" ;;
  esac
  case "$machine" in
    *"$expect"*) ;;
    *) die "$path machine '$machine' is not $expect" ;;
  esac
  interp=no
  if readelf -lW -- "$path" | grep -q 'INTERP'; then
    interp=yes
  fi
  needed=no
  if readelf -dW -- "$path" | grep -q '(NEEDED)'; then
    needed=yes
  fi
  stripped=yes
  if readelf -SW -- "$path" | grep -q '] \.symtab '; then
    stripped=no
  fi
  printf 'pi-seed: binary=%s target=%s bytes=%s machine=%s interp=%s needed=%s stripped=%s\n' \
    "$(basename -- "$path")" "$target" "$(stat -c '%s' -- "$path")" "$machine" "$interp" "$needed" "$stripped"
  if [ "$interp" = yes ] || [ "$needed" = yes ]; then
    die "$path is not a static musl binary"
  fi
  if [ "$stripped" != yes ]; then
    die "$path is not stripped"
  fi
}

pick_qemu() {
  local given=${1:-}
  if [ -n "$given" ]; then
    if [ ! -x "$given" ]; then
      die "qemu is not executable: $given"
    fi
    printf '%s\n' "$given"
    return 0
  fi
  if command -v qemu-aarch64-static >/dev/null 2>&1; then
    command -v qemu-aarch64-static
    return 0
  fi
  if command -v qemu-aarch64 >/dev/null 2>&1; then
    command -v qemu-aarch64
    return 0
  fi
  die "aarch64 smoke needs qemu-aarch64-static or qemu-aarch64"
}

run_probe() {
  local bin=$1 flag=$2 runner=$3 out err status
  case "$flag" in
    --help|--version) ;;
    *) die "probe flag must be --help or --version ($bin $flag)" ;;
  esac
  out=$(mktemp)
  err=$(mktemp)
  set +e
  if [ -n "$runner" ]; then
    timeout 20 "$runner" "$bin" "$flag" >"$out" 2>"$err"
  else
    timeout 20 "$bin" "$flag" >"$out" 2>"$err"
  fi
  status=$?
  set -e
  if [ "$status" -ne 0 ]; then
    printf 'pi-seed: probe failed %s %s status=%s\n' "$(basename -- "$bin")" "$flag" "$status" >&2
    cat "$err" >&2 || true
    rm -f "$out" "$err"
    die "probe failed"
  fi
  if ! grep -q . "$out"; then
    rm -f "$out" "$err"
    die "$(basename -- "$bin") $flag printed nothing"
  fi
  printf 'pi-seed: probe ok %s %s\n' "$(basename -- "$bin")" "$flag"
  rm -f "$out" "$err"
}

cmd_verify() {
  local target=$1 qemu_arg=${2:-} out bin flag runner saw
  require_target "$target"
  command -v readelf >/dev/null 2>&1 || die "readelf is required"
  command -v timeout >/dev/null 2>&1 || die "timeout is required"
  out=$(out_dir "$target")
  runner=
  if [ "$target" = aarch64-unknown-linux-musl ]; then
    runner=$(pick_qemu "$qemu_arg")
    printf 'pi-seed: qemu %s\n' "$runner"
  fi
  while IFS= read -r bin; do
    [ -n "$bin" ] || continue
    [ -f "$out/$bin" ] || die "missing $out/$bin (run build first)"
    elf_ok "$out/$bin" "$target"
    saw=0
    while IFS= read -r flag; do
      [ -n "$flag" ] || continue
      saw=1
      run_probe "$out/$bin" "$flag" "$runner"
    done < <(flags_for "$bin")
    [ "$saw" -eq 1 ] || die "no --help/--version probe for $bin"
  done < <(bins)
}

user_units() {
  find "$root" \
    \( -path "$root/target" -o -path "$root/.git" -o -path "$root/contrib" \
       -o -path "$root/vendor" -o -path "$root/release" \) -prune \
    -o -type f \( -name '*.service' -o -name '*.timer' \) -print \
    | LC_ALL=C sort \
    | while IFS= read -r file; do
        rel=${file#"$root"/}
        case "$rel" in
          */systemd/user/*|*/user/*)
            printf '%s\n' "$rel"
            ;;
          *)
            if grep -q '^WantedBy=default\.target$' "$file"; then
              printf '%s\n' "$rel"
            fi
            ;;
        esac
      done
}

cmd_pack() {
  local sha=$1 aarch=$2 x86=$3 outdir=$4 name pkg bin rel n=0
  case "$sha" in
    *[!A-Za-z0-9._-]*|"")
      printf 'pi-seed: sha must be a single path token\n' >&2
      exit 2
      ;;
  esac
  [ -d "$aarch" ] || die "missing aarch64 dir $aarch"
  [ -d "$x86" ] || die "missing x86_64 dir $x86"
  [ -f "$root/LICENSE" ] || die "missing LICENSE"
  mkdir -p -- "$outdir"
  outdir=$(CDPATH= cd -- "$outdir" && pwd)
  name=obsidian-pi5-seed-$sha
  # Global so the EXIT trap still sees it if this function returns first.
  stage=$(mktemp -d)
  trap 'rm -rf "$stage"' EXIT
  pkg=$stage/$name
  mkdir -p "$pkg/bin/aarch64-unknown-linux-musl" "$pkg/bin/x86_64-unknown-linux-musl"
  while IFS= read -r bin; do
    [ -n "$bin" ] || continue
    [ -f "$aarch/$bin" ] || die "pack missing $aarch/$bin"
    [ -f "$x86/$bin" ] || die "pack missing $x86/$bin"
    cp -- "$aarch/$bin" "$pkg/bin/aarch64-unknown-linux-musl/$bin"
    cp -- "$x86/$bin" "$pkg/bin/x86_64-unknown-linux-musl/$bin"
    chmod 755 "$pkg/bin/aarch64-unknown-linux-musl/$bin" "$pkg/bin/x86_64-unknown-linux-musl/$bin"
  done < <(bins)
  for dir in "$aarch" "$x86"; do
    for file in "$dir"/*; do
      [ -e "$file" ] || continue
      base=$(basename -- "$file")
      if ! bins | grep -qx "$base"; then
        die "unexpected file in seed dir: $file"
      fi
    done
  done
  while IFS= read -r rel; do
    [ -n "$rel" ] || continue
    n=$((n + 1))
    mkdir -p "$pkg/systemd/user/$(dirname -- "$rel")"
    cp -- "$root/$rel" "$pkg/systemd/user/$rel"
  done < <(user_units)
  if [ "$n" -eq 0 ]; then
    printf 'pi-seed: no systemd user units in the tree\n'
  else
    printf 'pi-seed: systemd user units=%s\n' "$n"
  fi
  cp -- "$root/LICENSE" "$pkg/LICENSE"
  cmp -s -- "$root/LICENSE" "$pkg/LICENSE" || die "LICENSE bytes changed while packing"
  (
    cd "$pkg"
    find . -type f ! -name SHA256SUMS -print | sed 's|^\./||' | LC_ALL=C sort | while IFS= read -r file; do
      sha256sum -- "$file"
    done > SHA256SUMS
  )
  rm -f "$outdir/$name.tar.gz"
  tar -C "$stage" -czf "$outdir/$name.tar.gz" "$name"
  printf 'pi-seed: artifact %s\n' "$outdir/$name.tar.gz"
  tar -tzf "$outdir/$name.tar.gz" | LC_ALL=C sort
  printf 'pi-seed: sizes\n'
  tar -xOf "$outdir/$name.tar.gz" "$name/SHA256SUMS"
  find "$pkg/bin" -type f -print | LC_ALL=C sort | while IFS= read -r file; do
    printf 'pi-seed: size %s %s\n' "$(stat -c '%s' -- "$file")" "${file#"$pkg"/}"
  done
  rm -rf "$stage"
  stage=
  trap - EXIT
}

if [ "$#" -lt 1 ]; then
  usage >&2
  exit 2
fi

case "$1" in
  -h|--help|help)
    usage
    exit 0
    ;;
  build)
    [ "$#" -eq 2 ] || { usage >&2; exit 2; }
    cmd_build "$2"
    ;;
  verify)
    [ "$#" -eq 2 ] || [ "$#" -eq 3 ] || { usage >&2; exit 2; }
    cmd_verify "$2" "${3:-}"
    ;;
  pack)
    [ "$#" -eq 5 ] || { usage >&2; exit 2; }
    cmd_pack "$2" "$3" "$4" "$5"
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
