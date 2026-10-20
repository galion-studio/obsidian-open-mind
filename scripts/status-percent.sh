#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Compute the backlog percentage from the one status table in docs/STATUS.md.
# Rows look like: | ID | Size | Status | Evidence |, Status is D, P, N or O.
# Credit: D = 1, P = 0.5, N and O = 0. Weighted uses Size (S = 1, M = 2).
# Read-only. Bash and awk only.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
FILE=${1:-"$ROOT/docs/STATUS.md"}
awk -F'|' '
  /^\| [A-Z0-9][A-Z0-9-]* +\| [0-9]+ +\| [DPNO] +\|/ {
    s = $3 + 0; st = $4; gsub(/ /, "", st)
    n++; w += s; c[st]++; cw[st] += s
  }
  END {
    if (n == 0) { print "status-percent: no rows"; exit 1 }
    printf "status-percent: items %d (D %d, P %d, N %d, O %d), weight %d\n", n, c["D"], c["P"], c["N"], c["O"], w
    printf "done: %.1f %% unweighted, %.1f %% weighted\n", 100 * c["D"] / n, 100 * cw["D"] / w
    printf "done + half partial: %.1f %% unweighted, %.1f %% weighted\n", 100 * (c["D"] + c["P"] / 2) / n, 100 * (cw["D"] + cw["P"] / 2) / w
    printf "owner-blocked: %.1f %% of items\n", 100 * c["O"] / n
  }' "$FILE"
