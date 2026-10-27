#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# CPU-only gate for Galion Rust services (Mate t276u / swarm/cpu-only-rust).
# Confirms nerve, cortex, hermes, memory, console (and the workspace lock)
# cannot pull CUDA / OpenCL / Metal / MKL / Accelerate crates, and that
# vendored candle-core no longer exposes those Cargo features.
#
# Stages:
#   candle   vendor/candle-core features are CPU-only
#   lock     Cargo.lock has none of the banned GPU crates
#   tree     cargo tree for each present service package has no GPU crates
#   resolve  `cargo check --manifest-path vendor/candle-core/Cargo.toml --features cuda` fails (feature gone)
#   all      (default) run every stage
#
# Environment: CARGO_TARGET_DIR, UCPU_JOBS (default 4), UCPU_NICE (default 10).
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root" || exit 1
export LC_ALL=C
JOBS=${UCPU_JOBS:-4}
NICE=${UCPU_NICE:-10}
case "${CARGO_TARGET_DIR:-}" in
  "") export CARGO_TARGET_DIR=$root/target ;;
  /*) ;;
  *) export CARGO_TARGET_DIR=$root/$CARGO_TARGET_DIR ;;
esac

say() { printf 'cpu-only-gate: %s\n' "$*"; }
fail=0
bad() { printf 'cpu-only-gate: FAIL %s\n' "$*" >&2; fail=1; }

BANNED='cudarc|candle-kernels|candle-metal-kernels|accelerate-src|intel-mkl-src|opencl3|^ocl$|cl-sys|objc2-metal'

stage_candle() {
  say "stage candle: vendor/candle-core has no GPU feature arms"
  local f=vendor/candle-core/Cargo.toml
  [ -f "$f" ] || { bad "missing $f"; return; }
  if rg -n '^(cuda|cudnn|nccl|metal|mkl|accelerate)\s*=' "$f" >/dev/null; then
    bad "$f still declares a GPU/accelerator feature arm"
  fi
  if rg -n '^\[dependencies\.(cudarc|candle-kernels|candle-metal-kernels|accelerate-src|intel-mkl-src|objc2-metal)\]' "$f" >/dev/null; then
    bad "$f still lists a GPU optional dependency"
  fi
  if ! rg -n 'Galion CPU-only' "$f" >/dev/null; then
    bad "$f missing Galion CPU-only marker comment"
  fi
}

stage_lock() {
  say "stage lock: Cargo.lock has no banned GPU crates"
  [ -f Cargo.lock ] || { bad "missing Cargo.lock"; return; }
  # Match exact package name lines only.
  local hits
  hits=$(rg -n '^name = "(cudarc|candle-kernels|candle-metal-kernels|accelerate-src|intel-mkl-src|opencl3|ocl|cl-sys|objc2-metal)"$' Cargo.lock || true)
  if [ -n "$hits" ]; then
    printf '%s\n' "$hits" >&2
    bad "Cargo.lock contains a banned GPU/accelerator crate"
  fi
}

stage_tree() {
  say "stage tree: service package dependency trees are CPU-only"
  local pkgs=(openmind-nerve hermes-gateway openmind-memory console-static)
  # cortex lives on cortex branches; include when present in this checkout.
  if [ -f crates/gaia-cortex/Cargo.toml ]; then
    pkgs+=(gaia-cortex)
  else
    say "note: gaia-cortex not in this checkout (verify on swarm/cortex-live-merge-* )"
  fi
  local p tree
  for p in "${pkgs[@]}"; do
    if ! rg -n "^name = \"$p\"$" Cargo.lock >/dev/null 2>&1 && [ ! -f "crates/$p/Cargo.toml" ]; then
      bad "package $p missing"
      continue
    fi
    say "  cargo tree -p $p"
    tree=$(nice -n "$NICE" cargo tree -p "$p" 2>&1) || {
      bad "cargo tree -p $p failed"
      printf '%s\n' "$tree" | tail -20 >&2
      continue
    }
    if printf '%s\n' "$tree" | rg -n "($BANNED)|candle-core|candle-nn" >/dev/null; then
      # candle in a *service* tree is unexpected for nerve/cortex/hermes/memory/console
      if printf '%s\n' "$tree" | rg -n "($BANNED)" >/dev/null; then
        printf '%s\n' "$tree" | rg -n "($BANNED)" >&2
        bad "$p dependency tree contains a banned GPU crate"
      fi
      if printf '%s\n' "$tree" | rg -n 'candle-core|candle-nn' >/dev/null; then
        printf '%s\n' "$tree" | rg -n 'candle-core|candle-nn' >&2
        bad "$p unexpectedly depends on candle (voice-only); services must stay CPU/IO only"
      fi
    fi
  done
}

stage_resolve() {
  say "stage resolve: vendor candle-core --features cuda must be rejected"
  local err
  if err=$(nice -n "$NICE" cargo check --manifest-path vendor/candle-core/Cargo.toml --features cuda -j "$JOBS" 2>&1); then
    bad "candle-core --features cuda unexpectedly succeeded"
  else
    if printf '%s\n' "$err" | rg -qi 'does not contain this feature: cuda'; then
      say "  ok: cuda feature rejected by cargo"
    else
      printf '%s\n' "$err" | tail -5 >&2
      bad "candle-core --features cuda failed for an unexpected reason"
    fi
  fi
}

stages=${*:-all}
[ "$stages" = all ] && stages="candle lock tree resolve"
for s in $stages; do
  case "$s" in
    candle|lock|tree|resolve) stage_"$s" ;;
    all) ;;
    *) bad "unknown stage $s"; exit 2 ;;
  esac
done
if [ "$fail" -ne 0 ]; then
  say "RESULT fail"
  exit 1
fi
say "RESULT ok"
exit 0
