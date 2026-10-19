#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 2026 Slavik / Galion Studio
# DESK-ID: openmind.B43
# Self-check for the OpenMind workspace. Bash only. No Python.
#
# Default (SELF_CHECK_FULL is not 1):
#   C6 UTC grep, C7 CJK grep, secret scan, scripts/no-python.sh.
#   Does not invoke cargo and does not compile the workspace.
# SELF_CHECK_FULL=1 also runs:
#   cargo fmt --all -- --check
#   nice -n 19 cargo clippy -j 2 --workspace --all-targets -- -D warnings
#   nice -n 19 cargo test -j 2 --workspace
#   SPDX MIT header scan on crates/**/*.rs
#   scripts/size-budget.sh (nice -n 19 cargo build -j 2 --release)
#
# Secret hits are printed as file:line only. The match text is never printed.
set -u

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

export PATH="/root/.cargo/bin:${PATH:-/usr/bin:/bin}"

# file:line from an "rg -n -H" line. Never prints the remainder (the match).
# Returns 1 if the line cannot be split safely.
emit_file_line() {
  local tag=$1 line=$2 file rest lineno
  file=${line%%:*}
  rest=${line#*:}
  lineno=${rest%%:*}
  case "$lineno" in
    ""|*[!0-9]*)
      printf "%s: parse-error\n" "$tag" >&2
      return 1
      ;;
  esac
  printf "%s: %s:%s\n" "$tag" "$file" "$lineno"
  return 0
}

# CHECKS C6: list Utc::now / SystemTime::now outside zagreb-time.
# Utc::now outside that crate fails. SystemTime::now is listed only.
# The DST proof is cargo test -p zagreb-time, which is full mode (C3).
run_c6() {
  local line hits utc bad
  hits=0
  utc=0
  bad=0
  while IFS= read -r line || [ -n "$line" ]; do
    [ -n "$line" ] || continue
    hits=$((hits + 1))
    emit_file_line c6 "$line" || bad=1
    case "$line" in
      *"Utc::now"*) utc=1 ;;
    esac
  done < <(rg -n -H --no-heading --color never \
    "Utc::now\\(\\)|SystemTime::now\\(\\)" crates \
    | rg -v --color never "zagreb-time" || true)

  if [ "$utc" -ne 0 ] || [ "$bad" -ne 0 ]; then
    printf "c6: fail\n"
    return 1
  fi
  printf "c6: ok (%s listed)\n" "$hits"
  return 0
}

# CHECKS C7. No match is success. Hits are file:line only (no CJK text).
run_c7() {
  local line hits bad
  hits=0
  bad=0
  while IFS= read -r line || [ -n "$line" ]; do
    [ -n "$line" ] || continue
    hits=$((hits + 1))
    emit_file_line c7 "$line" || bad=1
  done < <(rg -n -H --no-heading --color never \
    "[\p{Han}\p{Hiragana}\p{Katakana}\p{Hangul}]" . \
    --glob "!target" --glob "!stage/console-src/**" || true)

  if [ "$hits" -ne 0 ] || [ "$bad" -ne 0 ]; then
    printf "c7: fail\n"
    return 1
  fi
  printf "c7: ok\n"
  return 0
}

# Boss secret needles, with a token body so the documented regex in
# TEACHER/BOSS.md is not itself a hit. Prefixes are split in this file
# so the source does not contain a contiguous key prefix.
# Prints file:line only. Never prints the matched text.
run_secrets() {
  local or_pre or_mid ant_mid akia pem_begin pem_end re
  local line hits bad
  or_pre="sk-"
  or_mid="or-"
  ant_mid="ant-"
  akia="AKIA"
  pem_begin="BEGIN "
  pem_end="PRIVATE KEY"
  re="${or_pre}${or_mid}[A-Za-z0-9][A-Za-z0-9_-]{8,}|${or_pre}${ant_mid}[A-Za-z0-9][A-Za-z0-9_-]{8,}|${akia}[0-9A-Z]{16}|${pem_begin}[A-Z ]*${pem_end}"

  hits=0
  bad=0
  while IFS= read -r line || [ -n "$line" ]; do
    [ -n "$line" ] || continue
    hits=$((hits + 1))
    emit_file_line secret "$line" || bad=1
  done < <(rg -n -i -H --no-heading --color never --no-ignore \
    --glob "!.git/**" --glob "!target/**" --glob "!stage/**" \
    -e "$re" "$ROOT" || true)

  if [ "$hits" -ne 0 ] || [ "$bad" -ne 0 ]; then
    printf "secret: fail\n"
    return 1
  fi
  printf "secret: ok\n"
  return 0
}

# B42 accept: every crates/**/*.rs carries the MIT SPDX header.
run_spdx() {
  local f missing
  missing=0
  while IFS= read -r -d "" f; do
    if ! grep -q "SPDX-License-Identifier: MIT" "$f"; then
      printf "spdx: %s:1\n" "$f"
      missing=1
    fi
  done < <(find crates -type f -name "*.rs" -print0)
  if [ "$missing" -ne 0 ]; then
    printf "spdx: fail\n"
    return 1
  fi
  printf "spdx: ok\n"
  return 0
}

fail=0
mode=light
if [ "${SELF_CHECK_FULL:-}" = 1 ]; then
  mode=full
fi

ts=$(TZ=Europe/Zagreb date -Iseconds)
printf "self-check: DESK-ID openmind.B43\n"
printf "self-check: time %s\n" "$ts"
printf "self-check: mode %s\n" "$mode"

run_c6 || fail=1
run_c7 || fail=1
run_secrets || fail=1
scripts/no-python.sh || fail=1

if [ "$mode" = full ]; then
  run_spdx || fail=1
  printf "self-check: cargo fmt\n"
  if ! nice -n 19 cargo fmt --all -- --check; then
    printf "self-check: fmt fail\n"
    fail=1
  fi
  printf "self-check: cargo clippy\n"
  if ! nice -n 19 cargo clippy -j 2 --workspace --all-targets -- -D warnings; then
    printf "self-check: clippy fail\n"
    fail=1
  fi
  printf "self-check: cargo test\n"
  if ! nice -n 19 cargo test -j 2 --workspace; then
    printf "self-check: test fail\n"
    fail=1
  fi
  printf "self-check: size-budget\n"
  if ! scripts/size-budget.sh; then
    printf "self-check: size-budget fail\n"
    fail=1
  fi
else
  printf "self-check: cargo skipped (set SELF_CHECK_FULL=1 for fmt, clippy, test, spdx, size-budget)\n"
fi

if [ "$fail" -ne 0 ]; then
  printf "self-check: fail\n"
  exit 1
fi
printf "self-check: ok\n"
exit 0
