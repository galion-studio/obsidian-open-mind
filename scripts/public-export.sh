#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# public-export.sh: build the PUBLIC tree of this repository and verify it.
#
# It never pushes, publishes, or changes repository visibility. It only reads
# the git object store and writes into --out.
#
#   1. `git archive <ref>` -> <out>/tree   (honours .gitattributes export-ignore)
#   2. path policy      : no contrib/, *.redacted, stack/kms/, stack/INDEX.md,
#                         key/env/cookie/token files                     [FAIL]
#   3. binaries         : no ELF / executable binaries in the tree         [FAIL]
#   4. no Python        : no *.py file                                     [FAIL]
#   5. built-in secrets : regex scan for known token shapes and redaction
#                         markers; prints file:line only, never the value  [FAIL]
#   6. gitleaks dir     : gitleaks (MIT) over the tree, values redacted     [FAIL]
#   7. REUSE lint       : comply (Rust, MIT OR Apache-2.0) over the tree   [FAIL]
#   8. cargo deny       : licences/bans/sources with deny.toml, for the
#                         workspace and edge/openmind-node                  [FAIL]
#   9. SBOM             : cargo-cyclonedx (Apache-2.0) CycloneDX 1.5 JSON per
#                         crate, from Cargo.lock, into <out>/sbom          [FAIL]
#  10. disclosure warn  : public IPv4 addresses, quick-tunnel hostnames    [WARN]
#
# Usage: scripts/public-export.sh [--ref REF] [--out DIR] [--online]
#                                 [--allow-missing-tools] [--keep-going]
#   --ref REF              git ref to export (default HEAD; uncommitted changes
#                          are NOT exported)
#   --out DIR              output directory (default
#                          ${TMPDIR:-/tmp}/obsidian-open-mind-public); it is
#                          wiped first, so it must not be the repo itself
#   --online               also run `cargo deny check advisories` (fetches the
#                          RustSec advisory DB)
#   --allow-missing-tools  a missing gitleaks/comply/cargo-deny/cargo-cyclonedx
#                          is a SKIP instead of a FAIL
# Tools are looked up in $GALION_TOOLS_BIN first, then $PATH.
# Exit 0 = every FAIL-level gate passed. Exit 1 = at least one failed.
# No Python. Bash, git, tar, grep, sha256sum, file(1) optional.
set -u -o pipefail
export LC_ALL=C

die() { printf 'public-export: %s\n' "$*" >&2; exit 2; }

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH='' cd -- "$script_dir/.." && pwd)

ref=HEAD
out="${TMPDIR:-/tmp}/obsidian-open-mind-public"
online=0
allow_missing=0
while [ $# -gt 0 ]; do
  case "$1" in
    --ref) [ $# -ge 2 ] || die "--ref needs a value"; ref=$2; shift 2 ;;
    --out) [ $# -ge 2 ] || die "--out needs a value"; out=$2; shift 2 ;;
    --online) online=1; shift ;;
    --allow-missing-tools) allow_missing=1; shift ;;
    --keep-going) shift ;; # kept for compatibility; all gates always run
    -h|--help) sed -n '2,40p' "$0"; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done

if [ -n "${GALION_TOOLS_BIN:-}" ]; then
  PATH="$GALION_TOOLS_BIN:$PATH"
fi
command -v git >/dev/null || die "git not found"
command -v tar >/dev/null || die "tar not found"

out_abs=$(mkdir -p -- "$out" && CDPATH='' cd -- "$out" && pwd) || die "cannot create $out"
case "$out_abs" in
  "$root"|"$root"/*) die "--out must be outside the repository ($root)" ;;
  /|/root|/home|/workspace|/opt*|/etc*) die "refusing to wipe $out_abs" ;;
esac

ts() { TZ=Europe/Zagreb date -Iseconds; }
commit=$(git -C "$root" rev-parse --verify "$ref^{commit}") || die "bad ref: $ref"

rm -rf -- "$out_abs/tree" "$out_abs/sbom" "$out_abs/logs"
rm -f -- "$out_abs/REPORT.md" "$out_abs/MANIFEST.sha256" "$out_abs/gitleaks.json"
mkdir -p -- "$out_abs/tree" "$out_abs/sbom" "$out_abs/logs"
tree="$out_abs/tree"
report="$out_abs/REPORT.md"
fails=0
warns=0

line() { printf '%s\n' "$*" | tee -a "$report"; }
gate() { # gate NAME STATUS DETAIL
  line "| $1 | $2 | $3 |"
  case "$2" in FAIL) fails=$((fails + 1)) ;; WARN) warns=$((warns + 1)) ;; esac
}
need_tool() { # need_tool NAME -> 0 found, 1 missing
  command -v "$1" >/dev/null 2>&1
}
missing_status() { if [ "$allow_missing" = 1 ]; then echo SKIP; else echo FAIL; fi; }

: > "$report"
line "<!-- SPDX-License-Identifier: MIT -->"
line "<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->"
line "# Public export report"
line ""
line "- started: $(ts)"
line "- ref: \`$ref\` = \`$commit\`"
line "- tree: \`$tree\`"
if [ -n "$(git -C "$root" status --porcelain 2>/dev/null)" ]; then
  line "- note: the working tree has uncommitted changes; they are NOT in this export"
fi
line ""
line "| gate | result | detail |"
line "|---|---|---|"

# 1. export ---------------------------------------------------------------
if git -C "$root" archive --format=tar "$commit" | tar -x -C "$tree"; then
  nfiles=$(find "$tree" -type f | wc -l | tr -d ' ')
  ntracked=$(git -C "$root" ls-tree -r --name-only "$commit" | wc -l | tr -d ' ')
  gate "1 git archive" PASS "$nfiles files exported of $ntracked tracked ($((ntracked - nfiles)) export-ignored)"
else
  gate "1 git archive" FAIL "git archive failed"
  line ""; line "- finished: $(ts) (aborted)"; exit 1
fi
( cd "$tree" && find . -type f -print0 | sort -z | xargs -0 sha256sum ) > "$out_abs/MANIFEST.sha256"

# 2. path policy ------------------------------------------------------------
bad_paths=$(cd "$tree" && find . -type f \( \
    -path './contrib/*' -o -path './stack/kms/*' -o -path './stack/INDEX.md' \
    -o -name '*.redacted' -o -name '.env' -o -name '.env.*' -o -name '*.env' \
    -o -name '*.pem' -o -name '*.key' -o -name '*.p12' -o -name '*.pfx' \
    -o -name 'id_rsa*' -o -name 'id_ed25519*' -o -name '*_ed25519' -o -name '*.sealed' \
    -o -name 'auth.json' -o -name '*.cookie' -o -name '*.token' -o -name '*.pin' \
    -o -name 'credentials*.json' -o -name '*.kdbx' \) \
    ! -name '*.env.example' -print | sed 's#^\./##' | sort)
if [ -z "$bad_paths" ]; then
  gate "2 path policy" PASS "no contrib/, *.redacted, stack/kms/, stack/INDEX.md, key/env/cookie/token files"
else
  printf '%s\n' "$bad_paths" > "$out_abs/logs/bad-paths.txt"
  gate "2 path policy" FAIL "$(printf '%s\n' "$bad_paths" | wc -l | tr -d ' ') forbidden paths, see logs/bad-paths.txt"
fi

# 3. binaries ---------------------------------------------------------------
: > "$out_abs/logs/binaries.txt"
while IFS= read -r -d '' f; do
  magic=$(head -c 4 -- "$f" | od -An -tx1 | tr -d ' \n')
  case "$magic" in
    7f454c46|4d5a*|cafebabe|feedface|feedfacf|cffaedfe|cefaedfe)
      printf '%s\n' "${f#"$tree"/}" >> "$out_abs/logs/binaries.txt" ;;
  esac
done < <(find "$tree" -type f -size +3c -print0)
nbin=$(wc -l < "$out_abs/logs/binaries.txt" | tr -d ' ')
if [ "$nbin" = 0 ]; then
  gate "3 no binaries" PASS "no ELF/PE/Mach-O/class files"
else
  gate "3 no binaries" FAIL "$nbin executable binaries, see logs/binaries.txt"
fi

# 4. no Python --------------------------------------------------------------
npy=$(find "$tree" -type f -name '*.py' | wc -l | tr -d ' ')
if [ "$npy" = 0 ]; then gate "4 no Python" PASS "0 *.py"; else gate "4 no Python" FAIL "$npy *.py files"; fi

# 5. built-in secret patterns -----------------------------------------------
# Each entry: NAME<TAB>ERE. Shapes need the real body length, so this file's
# own pattern text never matches itself. Output is file:line only.
patterns=$(cat << 'PAT'
openrouter-key	sk-or-v1-[0-9a-f]{40,}
anthropic-key	sk-ant-[A-Za-z0-9_-]{30,}
openai-key	sk-(proj-)?[A-Za-z0-9]{40,}
github-pat	(ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{36}
github-fine-pat	github_pat_[A-Za-z0-9_]{60,}
aws-access-key	(AKIA|ASIA)[0-9A-Z]{16}
private-key	-----BEGIN [A-Z ]*PRIVATE KEY( BLOCK)?-----
slack-token	xox[abprs]-[A-Za-z0-9-]{10,}
google-api-key	AIza[0-9A-Za-z_-]{35}
huggingface	hf_[A-Za-z0-9]{34,}
replicate	r8_[A-Za-z0-9]{37}
cloudflare-tunnel-secret	"TunnelSecret"[[:space:]]*:[[:space:]]*"[A-Za-z0-9+/=]{30,}"
redaction-marker	<REDACTED:[A-Za-z0-9_.-]+>
jwt	eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}
PAT
)
: > "$out_abs/logs/secret-patterns.txt"
while IFS="$(printf '\t')" read -r name re; do
  [ -n "$name" ] || continue
  grep -rEnI --binary-files=without-match -e "$re" "$tree" 2>/dev/null \
    | cut -d: -f1,2 | sed "s#^$tree/##; s#\$# $name#" >> "$out_abs/logs/secret-patterns.txt" || true
done <<< "$patterns"
nsec=$(wc -l < "$out_abs/logs/secret-patterns.txt" | tr -d ' ')
if [ "$nsec" = 0 ]; then
  gate "5 secret patterns (built-in)" PASS "0 hits over $(printf '%s\n' "$patterns" | grep -c .) token shapes"
else
  gate "5 secret patterns (built-in)" FAIL "$nsec hits (file:line rule in logs/secret-patterns.txt, values not printed)"
fi

# 6. gitleaks ---------------------------------------------------------------
if need_tool gitleaks; then
  glv=$(gitleaks version 2>/dev/null | head -1)
  if nice -n 10 gitleaks dir --redact --no-banner --exit-code 1 \
      --report-format json --report-path "$out_abs/gitleaks.json" "$tree" \
      > "$out_abs/logs/gitleaks.log" 2>&1; then
    gate "6 gitleaks dir" PASS "gitleaks $glv: no leaks"
  else
    n=$(grep -c '"RuleID"' "$out_abs/gitleaks.json" 2>/dev/null || echo '?')
    gate "6 gitleaks dir" FAIL "gitleaks $glv: $n findings (redacted) in gitleaks.json"
  fi
else
  gate "6 gitleaks dir" "$(missing_status)" "gitleaks not installed"
fi

# 7. REUSE lint -------------------------------------------------------------
if need_tool comply; then
  if comply lint --lines "$tree" > "$out_abs/logs/reuse-lint.txt" 2>&1; then
    gate "7 REUSE lint (comply)" PASS "$(comply lint "$tree" 2>/dev/null | grep -E 'total' | head -1)"
  else
    gate "7 REUSE lint (comply)" FAIL "$(grep -c . "$out_abs/logs/reuse-lint.txt") problems, see logs/reuse-lint.txt"
  fi
else
  gate "7 REUSE lint (comply)" "$(missing_status)" "comply not installed"
fi

# Cargo steps run on the exported tree with their own target dir.
export CARGO_TARGET_DIR="$out_abs/cargo-target"

# 8. cargo deny -------------------------------------------------------------
if need_tool cargo-deny; then
  checks="licenses bans sources"
  [ "$online" = 1 ] && checks="$checks advisories"
  offline_flag=--offline
  [ "$online" = 1 ] && offline_flag=
  ok=1; detail=""
  for m in Cargo.toml edge/openmind-node/Cargo.toml; do
    [ -f "$tree/$m" ] || continue
    log="$out_abs/logs/cargo-deny-$(echo "$m" | tr '/' '_').log"
    # shellcheck disable=SC2086
    if (cd "$tree" && nice -n 10 cargo-deny $offline_flag --manifest-path "$m" \
          --config "$tree/deny.toml" check $checks) > "$log" 2>&1; then
      detail="$detail $m: $(tail -1 "$log");"
    else
      ok=0; detail="$detail $m: $(tail -1 "$log");"
    fi
  done
  if [ "$ok" = 1 ]; then gate "8 cargo deny" PASS "$detail"; else gate "8 cargo deny" FAIL "$detail"; fi
else
  gate "8 cargo deny" "$(missing_status)" "cargo-deny not installed"
fi

# 9. SBOM -------------------------------------------------------------------
if need_tool cargo-cyclonedx; then
  # cargo-cyclonedx writes <crate>.cdx.json next to each Cargo.toml, so run
  # it in a scratch copy and collect the files; the exported tree stays clean.
  scratch="$out_abs/sbom-scratch"
  rm -rf -- "$scratch"; cp -a -- "$tree" "$scratch"
  ok=1
  for m in Cargo.toml edge/openmind-node/Cargo.toml; do
    [ -f "$scratch/$m" ] || continue
    if ! (cd "$scratch" && nice -n 10 cargo-cyclonedx cyclonedx --manifest-path "$m" \
            --format json --spec-version 1.5 --all --target all --license-strict -q) \
            >> "$out_abs/logs/sbom.log" 2>&1; then
      ok=0
    fi
  done
  (cd "$scratch" && find . -name '*.cdx.json' -print0 | xargs -0 -I{} cp {} "$out_abs/sbom/") 2>/dev/null
  rm -rf -- "$scratch"
  nsbom=$(find "$out_abs/sbom" -name '*.cdx.json' | wc -l | tr -d ' ')
  if [ "$ok" = 1 ] && [ "$nsbom" -gt 0 ]; then
    ncomp=""
    if command -v jq >/dev/null; then
      ncomp=$(jq -s '[.[].components[]?.purl] | unique | length' "$out_abs"/sbom/*.cdx.json)
      nolic=$(jq -s '[.[].components[]? | select(.type=="library") | select((.licenses // []) | length == 0) | .purl] | unique | length' "$out_abs"/sbom/*.cdx.json)
      ncomp="; $ncomp unique components, $nolic without licence data"
    fi
    gate "9 SBOM (CycloneDX 1.5)" PASS "$nsbom files in sbom/$ncomp"
  else
    gate "9 SBOM (CycloneDX 1.5)" FAIL "cargo-cyclonedx failed or produced nothing, see logs/sbom.log"
  fi
else
  gate "9 SBOM (CycloneDX 1.5)" "$(missing_status)" "cargo-cyclonedx not installed"
fi
rm -rf -- "$out_abs/cargo-target"

# 10. disclosure warnings ---------------------------------------------------
# Public (non-RFC1918, non-loopback, non-documentation) IPv4 literals and
# quick-tunnel hostnames are not secrets, but they map live infrastructure.
grep -rEnoI '\b([0-9]{1,3}\.){3}[0-9]{1,3}\b' "$tree" 2>/dev/null \
  | sed "s#^$tree/##" \
  | grep -Ev ':(127\.|0\.|10\.|192\.168\.|172\.(1[6-9]|2[0-9]|3[01])\.|169\.254\.|192\.0\.2\.|198\.51\.100\.|203\.0\.113\.|22[4-9]\.|23[0-9]\.|24[0-9]\.|25[0-5]\.)' \
  > "$out_abs/logs/public-ipv4.txt" || true
# keep only real-looking addresses (each octet <= 255)
awk -F: '{split($3,o,"."); if (o[1]<=255 && o[2]<=255 && o[3]<=255 && o[4]<=255) print}' \
  "$out_abs/logs/public-ipv4.txt" > "$out_abs/logs/public-ipv4.tmp" && mv "$out_abs/logs/public-ipv4.tmp" "$out_abs/logs/public-ipv4.txt"
nip=$(wc -l < "$out_abs/logs/public-ipv4.txt" | tr -d ' ')
grep -rEnI 'trycloudflare\.com' "$tree" 2>/dev/null | cut -d: -f1,2 | sed "s#^$tree/##" > "$out_abs/logs/quick-tunnels.txt" || true
ntun=$(wc -l < "$out_abs/logs/quick-tunnels.txt" | tr -d ' ')
if [ "$nip" = 0 ] && [ "$ntun" = 0 ]; then
  gate "10 infra disclosure" PASS "no public IPv4 literals, no quick-tunnel hostnames"
else
  gate "10 infra disclosure" WARN "$nip public IPv4 literals (logs/public-ipv4.txt), $ntun trycloudflare.com mentions (logs/quick-tunnels.txt); owner decides"
fi

line ""
line "- finished: $(ts)"
line "- result: $fails FAIL, $warns WARN"
line "- manifest: \`MANIFEST.sha256\` ($(wc -l < "$out_abs/MANIFEST.sha256" | tr -d ' ') files)"
line ""
line "Nothing was pushed or published. The tree is for review; creating the public"
line "repository from it is the owner's action (docs/launch/PUBLIC-EXPORT.md)."
[ "$fails" = 0 ]
