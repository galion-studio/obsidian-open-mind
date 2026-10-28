#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# Real-life test for the launch gates (slot 15, slug launch-gates).
# No network service is started (ports 19150-19159 are not needed); the "real
# binary" here is the public-export pipeline and the third-party scanners.
#
#   A. Export HEAD for real and run every gate. Expect 0 FAIL.
#   B. `cargo check --workspace --locked` on the EXPORTED tree (proves the
#      public tree builds without contrib/, stack/kms/, dist/bin/ ...).
#   C. Canary run: a throw-away clone gets planted violations (a generated
#      fake AWS-shaped key, a GPL path crate, an ELF binary, a GPL SPDX header,
#      the contrib/ export rule removed, a new *.redacted file). Expect gates
#      2, 3, 5, 6, 7, 8 to FAIL and the *.redacted canary to stay out.
#
# Gated: runs only with GALION_REAL=1. Tools from $GALION_TOOLS_BIN or PATH.
# Output under ${LG_OUT:-${TMPDIR:-/tmp}/launch-gates-real}.
set -u -o pipefail
export LC_ALL=C
if [ "${GALION_REAL:-0}" != 1 ]; then
  echo "launch-gates real test: set GALION_REAL=1 to run"; exit 0
fi
root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
out=${LG_OUT:-${TMPDIR:-/tmp}/launch-gates-real}
mkdir -p -- "$out"
pass=0; fail=0
ok()  { echo "  ok   $*"; pass=$((pass + 1)); }
bad() { echo "  FAIL $*"; fail=$((fail + 1)); }
ts() { TZ=Europe/Zagreb date -Iseconds; }

echo "== A. real export of HEAD ($(ts))"
if "$root/scripts/public-export.sh" --out "$out/A" > "$out/A.log" 2>&1; then
  ok "public-export HEAD exit 0"
else
  bad "public-export HEAD failed (see $out/A.log)"
fi
grep -E '^\| [0-9]+ ' "$out/A/REPORT.md" | sed 's/^/     /'

echo "== B. cargo check of the exported tree ($(ts))"
if [ "${LG_SKIP_BUILD:-0}" = 1 ]; then
  echo "  skip (LG_SKIP_BUILD=1)"
elif (cd "$out/A/tree" && CARGO_TARGET_DIR="$out/B-target" nice -n 10 \
        cargo check --workspace --locked --offline -j 4 -q) > "$out/B.log" 2>&1; then
  ok "exported workspace: cargo check --workspace --locked"
else
  bad "exported workspace does not build (see $out/B.log)"
fi
rm -rf -- "$out/B-target"

echo "== C. canary run ($(ts))"
canary="$out/C-repo"
rm -rf -- "$canary"
git clone -q --no-hardlinks "$root" "$canary"
(
  cd "$canary" || exit 1
  git checkout -q --detach "$(git -C "$root" rev-parse HEAD)"
  # 1. secret: a random AWS-shaped access key id, built at run time so this
  #    script never contains one.
  key="AKIA$(head -c 4096 /dev/urandom | tr -dc 'A-Z2-7' | head -c 16)"
  sec="$(head -c 4096 /dev/urandom | tr -dc 'A-Za-z0-9' | head -c 40)"
  printf 'aws_access_key_id = %s\naws_secret_access_key = %s\n' "$key" "$sec" > docs/canary-leak.txt
  # REUSE-IgnoreStart (the canary below writes a GPL header into the clone)
  # 2. GPL dependency inside the workspace
  mkdir -p crates/zz-gpl-canary/src
  printf '[package]\nname = "zz-gpl-canary"\nversion = "0.1.0"\nedition = "2021"\nlicense = "GPL-3.0-only"\npublish = false\n' > crates/zz-gpl-canary/Cargo.toml
  printf '// SPDX-License-Identifier: GPL-3.0-only\n// SPDX-FileCopyrightText: 3000 Canary\n' > crates/zz-gpl-canary/src/lib.rs
  # REUSE-IgnoreEnd
  # 3. an ELF binary
  cp /bin/true docs/canary-elf
  # 4. contrib/ export rule removed
  sed -i '/^\/contrib export-ignore/d' .gitattributes
  # 5. a new redacted inventory file (must stay out via *.redacted)
  printf 'TOKEN=REDACTED\n' > stack/hermes/canary.env.redacted
  git add -A -f
  git -c user.name=canary -c user.email=canary@invalid commit -q -m canary
) || bad "could not build the canary repo"
# cargo deny needs Cargo.lock to know the new member; refresh it offline.
(cd "$canary" && cargo generate-lockfile --offline -q 2>/dev/null; git add -A; \
  git -c user.name=canary -c user.email=canary@invalid commit -q -m lock) || true
"$canary/scripts/public-export.sh" --out "$out/C" > "$out/C.log" 2>&1
rc=$?
grep -E '^\| [0-9]+ ' "$out/C/REPORT.md" | sed 's/^/     /'
[ "$rc" != 0 ] && ok "canary export exits non-zero ($rc)" || bad "canary export passed"
for g in "2 path policy" "3 no binaries" "5 secret patterns" "6 gitleaks dir" "7 REUSE lint" "8 cargo deny"; do
  if grep -E "^\| $g[^|]*\| FAIL \|" "$out/C/REPORT.md" >/dev/null; then ok "canary caught by gate $g"; else bad "gate $g did not catch its canary"; fi
done
if [ ! -e "$out/C/tree/stack/hermes/canary.env.redacted" ]; then ok "*.redacted canary export-ignored"; else bad "*.redacted canary leaked into the tree"; fi
if [ -e "$out/C/tree/contrib" ]; then ok "contrib/ appears once its rule is removed (gate 2 is the backstop)"; fi
rm -rf -- "$canary" "$out/C/sbom"

echo "== result ($(ts)): $pass ok, $fail failed"
[ "$fail" = 0 ]
