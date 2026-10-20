#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# SUPERSEDED (2026-10-20, swarm/gaia-hades-tokens): this script used to run
# `gaia heal-tick` with a per-run random TEST token (--trust-file /
# --approval-file). That token path is gone: heal-tick now needs a hades-gate
# Ed25519-signed, expiring, single-use token scoped to heal-apply + service +
# plan hash. The real-life test of the same path (nerve -> GAIA -> DEMETER ->
# HADES -> approval -> restart, 3/hour cap, nerve events) now lives in
# scripts/real/gaia-hades-tokens.sh (slot 23 ports 19230-19239); this wrapper
# runs it. The original run's evidence stays in docs/real-tests/gaia-live.md.
set -euo pipefail
exec "$(dirname "$0")/gaia-hades-tokens.sh" "$@"
