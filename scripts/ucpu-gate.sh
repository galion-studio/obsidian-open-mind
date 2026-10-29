#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
# DESK-ID: openmind.UCPU-1
#
# Universal-CPU gate for every binary (UNIVERSAL-CPU.md).
#
# Stages (default: all, in this order):
#   grep    no inline asm, arch intrinsics, target-cpu / target-feature flags
#   build   release builds, tier 1: x86_64 + aarch64 musl, workspace + omnode
#   static  every built ELF has no PT_INTERP and no DT_NEEDED, right e_machine
#   size    scripts/size-budget.sh (200 MB for every binary, stripped) per tier-1 target
#   check   cargo check, tier 2: armv7-unknown-linux-musleabihf, aarch64-linux-android
#   clippy  (not in all) clippy -D warnings per tier-1 target
#
# C code: five workspace crates pull rusqlite "bundled" (desk-hermes,
# hermes-rs, hermes-gateway, one-defence, openmind-memory), so the build
# scripts compile sqlite3.c for the target. `zig cc` is the one C cross
# compiler for every target (no aarch64/arm gcc, no NDK needed for the gate).
# For aarch64-linux-android the C side is compiled against zig's musl headers:
# `cargo check` never links, so the object only has to compile. A real Android
# *link* needs the NDK (bionic crt + libc/libm/libdl); see docs/ucpu.md.
#
# Linker for musl: rust-lld (from rustup). No gcc cross toolchain.
#
# Environment:
#   ZIG               path to zig (default: `zig` on PATH)
#   CARGO_TARGET_DIR  honoured; default <repo>/target
#   UCPU_JOBS         cargo -j (default 4)
#   UCPU_NICE         nice level (default 10)
#   UCPU_LOCKED       set to 0 to drop --locked (default 1)
#   UCPU_TIER1        space-separated build targets (default: both musl)
#   UCPU_TIER2        space-separated check targets (default: armv7, android)
#
# Exit 0 when every requested stage passes, 1 otherwise. Bash only.
set -u

usage() {
  sed -n '6,35p' "$0" | sed 's/^# \{0,1\}//'
  printf 'usage: ucpu-gate.sh [grep|build|static|size|check|clippy|all]...\n'
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$script_dir/.." && pwd)
cd "$root" || exit 1
export LC_ALL=C

# shellcheck disable=SC2206
TIER1=(${UCPU_TIER1:-x86_64-unknown-linux-musl aarch64-unknown-linux-musl})
# shellcheck disable=SC2206
TIER2=(${UCPU_TIER2:-armv7-unknown-linux-musleabihf aarch64-linux-android})
JOBS=${UCPU_JOBS:-4}
NICE=${UCPU_NICE:-10}
LOCKED=--locked
[ "${UCPU_LOCKED:-1}" = 0 ] && LOCKED=

case "${CARGO_TARGET_DIR:-}" in
  "") TD=$root/target ;;
  /*) TD=$CARGO_TARGET_DIR ;;
  *) TD=$root/$CARGO_TARGET_DIR ;;
esac
export CARGO_TARGET_DIR=$TD
# omnode is its own workspace; give it its own target dir next to ours.
OMNODE_TD=$TD/omnode

say() { printf 'ucpu-gate: %s\n' "$*"; }
fail=0
bad() { printf 'ucpu-gate: FAIL %s\n' "$*" >&2; fail=1; }

for t in cargo readelf; do
  command -v "$t" >/dev/null 2>&1 || { [ -x "$HOME/.cargo/bin/$t" ] && PATH=$HOME/.cargo/bin:$PATH; }
done

# ---------------------------------------------------------------- grep gate
stage_grep() {
  say "stage grep: asm / intrinsics / target-cpu / target-feature"
  local hits n=0
  # Rust sources: inline/global asm, arch intrinsics, runtime feature probes,
  # #[target_feature], cfg(target_feature = ...).
  hits=$(git ls-files -z -- '*.rs' ':!contrib/**' |
    xargs -0 grep -nE \
      '(^|[^A-Za-z_])(asm!|global_asm!|naked_asm!)|(std|core)::arch::|#!?\[target_feature|target_feature *=|is_(x86|aarch64|arm)_feature_detected!|(^|[^A-Za-z_])_mm(256|512)?_[a-z]|(^|[^A-Za-z_])v(ld|st)[1-4]q?_[a-z0-9]' \
      2>/dev/null |
    grep -vE 'ucpu-gate: allow' || true)
  # repo-check's own pattern table names these strings on purpose.
  hits=$(printf '%s\n' "$hits" | grep -vE '^crates/repo-check/(src|tests)/' | sed '/^$/d')
  if [ -n "$hits" ]; then printf '%s\n' "$hits" >&2; n=$((n + $(printf '%s\n' "$hits" | wc -l))); fi
  # Build flags: cargo config, manifests, build scripts, workflows, scripts.
  hits=$(git ls-files -z -- '*.toml' '.cargo/config' '*.rs' '*.sh' '*.yml' '*.yaml' 'Makefile' '*.mk' ':!contrib/**' ':!scripts/ucpu-gate.sh' ':!crates/repo-check/**' |
    xargs -0 grep -nE 'target-cpu|target-feature|-C *target|-mcpu=|-march=native|-mtune=native' 2>/dev/null |
    grep -vE 'ucpu-gate: allow|UNIVERSAL-CPU|no target-cpu|no `target' | sed '/^$/d' || true)
  if [ -n "$hits" ]; then printf '%s\n' "$hits" >&2; n=$((n + $(printf '%s\n' "$hits" | wc -l))); fi
  # Environment of this run must not carry them either.
  case "${RUSTFLAGS:-} ${CARGO_ENCODED_RUSTFLAGS:-}" in
    *target-cpu*|*target-feature*) bad "RUSTFLAGS sets target-cpu/target-feature"; n=$((n + 1)) ;;
  esac
  # openssl-sys is a C dep per target; UNIVERSAL-CPU.md forbids it.
  if grep -qE '^name = "openssl-sys"' Cargo.lock edge/openmind-node/Cargo.lock 2>/dev/null; then
    bad "openssl-sys in a Cargo.lock"; n=$((n + 1))
  fi
  if [ "$n" -gt 0 ]; then bad "grep gate: $n hit(s)"; else say "grep ok"; fi
}

# ---------------------------------------------------------------- zig cc
zig_bin=""
setup_zig() {
  [ -n "$zig_bin" ] && return 0
  zig_bin=${ZIG:-$(command -v zig 2>/dev/null || true)}
  [ -n "$zig_bin" ] && [ -x "$zig_bin" ] || { bad "zig not found (set ZIG=/path/to/zig)"; return 1; }
  say "zig $("$zig_bin" version) at $zig_bin"
  local w=$TD/ucpu-zig
  mkdir -p "$w" "$TD/zig-cache"
  export ZIG_GLOBAL_CACHE_DIR=${ZIG_GLOBAL_CACHE_DIR:-$TD/zig-cache}
  export ZIG_LOCAL_CACHE_DIR=${ZIG_LOCAL_CACHE_DIR:-$TD/zig-cache}
  # rust triple -> zig triple. Android: zig ships no bionic headers, so the C
  # side compiles against musl headers (check only, never linked).
  local pair rt zt v
  for pair in \
    x86_64-unknown-linux-musl:x86_64-linux-musl \
    aarch64-unknown-linux-musl:aarch64-linux-musl \
    armv7-unknown-linux-musleabihf:arm-linux-musleabihf \
    aarch64-linux-android:aarch64-linux-musl; do
    rt=${pair%%:*}; zt=${pair#*:}
    # cc-rs adds --target=<rust triple>; zig wants its own -target, so drop it.
    cat > "$w/cc-$rt" << WRAP
#!/bin/sh
for a do shift; case "\$a" in --target=*) continue ;; esac; set -- "\$@" "\$a"; done
exec "$zig_bin" cc -target $zt "\$@"
WRAP
    chmod +x "$w/cc-$rt"
    v=$(printf '%s' "$rt" | tr '-' '_')
    export "CC_$v=$w/cc-$rt"
    export "AR_$v=$w/ar"
  done
  printf '#!/bin/sh\nexec "%s" ar "$@"\n' "$zig_bin" > "$w/ar"
  chmod +x "$w/ar"
  export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld
  export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld
  export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_LINKER=rust-lld
}

need_targets() {
  local t have
  have=$(rustup target list --installed 2>/dev/null || true)
  for t in "$@"; do
    printf '%s\n' "$have" | grep -qx "$t" || { bad "rust target $t not installed (rustup target add $t)"; return 1; }
  done
}

cargo_n() { nice -n "$NICE" cargo "$@"; }

# ---------------------------------------------------------------- build
stage_build() {
  say "stage build: ${TIER1[*]}"
  setup_zig || return
  need_targets "${TIER1[@]}" || return
  local t
  for t in "${TIER1[@]}"; do
    say "build workspace --target $t"
    cargo_n build -j "$JOBS" --release --workspace $LOCKED --target "$t" || bad "build workspace $t"
    say "build omnode --target $t"
    CARGO_TARGET_DIR=$OMNODE_TD cargo_n build -j "$JOBS" --release $LOCKED \
      --manifest-path edge/openmind-node/Cargo.toml --target "$t" || bad "build omnode $t"
  done
}

# Workspace + omnode release ELF executables for one triple.
bins_for() {
  local t=$1 d f
  for d in "$TD/$t/release" "$OMNODE_TD/$t/release"; do
    [ -d "$d" ] || continue
    for f in "$d"/*; do
      [ -f "$f" ] && [ -x "$f" ] || continue
      case "$f" in *.so|*.d|*.rlib|*.a) continue ;; esac
      [ "$(head -c 4 -- "$f" | od -An -c | tr -d ' ')" = '177ELF' ] || continue
      printf '%s\n' "$f"
    done
  done
}

machine_for() {
  case "$1" in
    x86_64-*) echo 'Advanced Micro Devices X86-64' ;;
    aarch64-*) echo 'AArch64' ;;
    armv7-*) echo 'ARM' ;;
  esac
}

# ---------------------------------------------------------------- static
stage_static() {
  say "stage static: no PT_INTERP, no DT_NEEDED, e_machine matches"
  local t f n m want
  for t in "${TIER1[@]}"; do
    want=$(machine_for "$t")
    n=0
    while IFS= read -r f; do
      [ -n "$f" ] || continue
      n=$((n + 1))
      m=$(readelf -hW -- "$f" | sed -n 's/^ *Machine: *//p')
      [ "$m" = "$want" ] || bad "$t $(basename "$f"): machine '$m' != '$want'"
      if readelf -lW -- "$f" | grep -q 'INTERP'; then bad "$t $(basename "$f"): has PT_INTERP (dynamic loader)"; fi
      if readelf -dW -- "$f" | grep -q '(NEEDED)'; then
        bad "$t $(basename "$f"): DT_NEEDED $(readelf -dW -- "$f" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' | tr '\n' ' ')"
      fi
      printf '  %-34s %-22s static\n' "$(basename "$f")" "$t"
    done < <(bins_for "$t")
    [ "$n" -gt 0 ] || bad "$t: no binaries found (run the build stage)"
  done
}

# ---------------------------------------------------------------- size
stage_size() {
  say "stage size: scripts/size-budget.sh per tier-1 target"
  local t
  for t in "${TIER1[@]}"; do
    "$root/scripts/size-budget.sh" --check-dir "$TD/$t/release" || bad "size budget $t (workspace)"
    if [ -d "$OMNODE_TD/$t/release" ]; then
      "$root/scripts/size-budget.sh" --check-dir "$OMNODE_TD/$t/release" || bad "size budget $t (omnode)"
    fi
  done
}

# ---------------------------------------------------------------- check
stage_check() {
  say "stage check: ${TIER2[*]}"
  setup_zig || return
  need_targets "${TIER2[@]}" || return
  local t
  for t in "${TIER2[@]}"; do
    say "check workspace --target $t"
    cargo_n check -j "$JOBS" --workspace --all-targets $LOCKED --target "$t" || bad "check workspace $t"
    say "check omnode --target $t"
    CARGO_TARGET_DIR=$OMNODE_TD cargo_n check -j "$JOBS" --all-targets $LOCKED \
      --manifest-path edge/openmind-node/Cargo.toml --target "$t" || bad "check omnode $t"
  done
}

# ---------------------------------------------------------------- clippy
# Optional (not in "all"): clippy -D warnings per tier-1 target, so cfg paths
# that only exist on musl/aarch64 are linted too. Used by CI.
stage_clippy() {
  say "stage clippy: ${TIER1[*]}"
  setup_zig || return
  need_targets "${TIER1[@]}" || return
  local t
  for t in "${TIER1[@]}"; do
    cargo_n clippy -j "$JOBS" --workspace --all-targets $LOCKED --target "$t" -- -D warnings || bad "clippy $t"
  done
}

stages=("$@")
[ ${#stages[@]} -eq 0 ] && stages=(all)
say "time $(TZ=Europe/Zagreb date '+%Y-%m-%dT%H:%M:%S%:z') target-dir $TD"
for s in "${stages[@]}"; do
  case "$s" in
    grep) stage_grep ;;
    build) stage_build ;;
    static) stage_static ;;
    size) stage_size ;;
    check) stage_check ;;
    clippy) stage_clippy ;;
    all) stage_grep; stage_build; stage_static; stage_size; stage_check ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 1 ;;
  esac
done

if [ "$fail" -ne 0 ]; then say "RESULT FAIL"; exit 1; fi
say "RESULT ok"
exit 0
