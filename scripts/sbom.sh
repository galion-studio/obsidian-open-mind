#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# DESK-ID: openmind.B44
# cargo metadata --format-version 1 --offline -> dist/sbom-cargo.json,
# then fail if any package license is outside the allowlist.
# Bash and jq only. No Python. No cargo install.
set -eu -o pipefail

ALLOW_WORDS="MIT Apache-2.0 BSD-2-Clause BSD-3-Clause ISC Zlib Unicode-3.0"

usage() {
  cat << 'HELP'
usage: sbom.sh [--check FILE]

Default:
  nice -n 19 cargo metadata --format-version 1 --offline
  writes <repo>/dist/sbom-cargo.json
  prints every package license
  exits 1 if any license is outside the allowlist

--check FILE
  Do not run cargo. Do not write dist/sbom-cargo.json.
  FILE is cargo metadata --format-version 1 JSON (a small fixture is enough).
  Prints the same license report.

Allowlist:
  MIT Apache-2.0 BSD-2-Clause BSD-3-Clause ISC Zlib Unicode-3.0

An SPDX expression passes when it can be satisfied by that list.
OR passes if one side passes. A slash between license ids is treated as OR.
AND passes only if every side passes. WITH passes only if both ids are
on the allowlist. A missing, empty, or unparsable license fails.
Identifiers are case-sensitive.

exit 0  every package license is allowed
exit 1  disallowed license, empty graph, or bad arguments
HELP
}

die() {
  printf 'sbom: %s\n' "$*" >&2
  exit 1
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$script_dir/.." && pwd)

check_file=""
while [ $# -gt 0 ]; do
  case "$1" in
    --check)
      [ $# -ge 2 ] || die "--check needs a JSON file"
      check_file=$2
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

export LC_ALL=C
command -v jq >/dev/null 2>&1 || die "jq not found"

ts=$(TZ=Europe/Zagreb date -Iseconds)

raw=""
out_tmp=""
cleanup() {
  if [ -n "${raw}" ]; then
    rm -f -- "$raw"
  fi
  if [ -n "${out_tmp}" ]; then
    rm -f -- "$out_tmp"
  fi
}
trap cleanup EXIT

if [ -n "$check_file" ]; then
  [ -f "$check_file" ] || die "not a file: $check_file"
  meta=$check_file
  mode=check
else
  if ! command -v cargo >/dev/null 2>&1; then
    if [ -x /root/.cargo/bin/cargo ]; then
      PATH="/root/.cargo/bin:${PATH}"
      export PATH
    else
      die "cargo not found"
    fi
  fi
  raw=$(mktemp)
  meta=$raw
  mode=write
  cd "$root"
  # Exact command. --offline so Cargo.lock is not rewritten.
  nice -n 19 cargo metadata --format-version 1 --offline > "$raw"
fi

if ! jq -e '(.packages | type == "array") and ((.version // 1) == 1)' "$meta" >/dev/null; then
  die "need cargo metadata format-version 1 with a packages array"
fi

filter=$(cat << 'JQ'
def allow: ["MIT","Apache-2.0","BSD-2-Clause","BSD-3-Clause","ISC","Zlib","Unicode-3.0"];
def allowmap:
  {
    "MIT": true,
    "Apache-2.0": true,
    "BSD-2-Clause": true,
    "BSD-3-Clause": true,
    "ISC": true,
    "Zlib": true,
    "Unicode-3.0": true
  };

def lex($s):
  ($s | tostring | gsub("\\s*/\\s*"; " OR ")) as $n
  | [ $n | scan("\\(|\\)|AND|OR|WITH|[A-Za-z0-9][A-Za-z0-9.+-]*") ];

def judge($expr):
  def parse_or($t; $i):
    def parse_primary($t; $i):
      if $t[$i] == null then
        {ok: false, i: $i, err: "truncated"}
      elif $t[$i] == "(" then
        parse_or($t; $i + 1) as $inner
        | if $t[$inner.i] != ")" then
            {ok: false, i: $inner.i, err: "missing paren"}
          else
            {ok: $inner.ok, i: ($inner.i + 1), err: $inner.err}
          end
      elif ($t[$i] == "WITH" or $t[$i] == "AND" or $t[$i] == "OR" or $t[$i] == ")") then
        {ok: false, i: $i, err: "bad token"}
      else
        ($t[$i]) as $id
        | if $t[$i + 1] == "WITH" then
            ($t[$i + 2]) as $ex
            | if ($ex == null or $ex == "AND" or $ex == "OR" or $ex == "WITH" or $ex == "(" or $ex == ")") then
                {ok: false, i: ($i + 1), err: "bad exception"}
              else
                {
                  ok: ((allowmap[$id] == true) and (allowmap[$ex] == true)),
                  i: ($i + 3)
                }
              end
          else
            {ok: (allowmap[$id] == true), i: ($i + 1)}
          end
      end;
    def parse_and($t; $i):
      parse_primary($t; $i) as $a
      | if $t[$a.i] == "AND" then
          parse_and($t; $a.i + 1) as $b
          | {ok: ($a.ok and $b.ok), i: $b.i, err: ($a.err // $b.err)}
        else
          $a
        end;
    parse_and($t; $i) as $a
    | if $t[$a.i] == "OR" then
        parse_or($t; $a.i + 1) as $b
        | {ok: ($a.ok or $b.ok), i: $b.i, err: ($a.err // $b.err)}
      else
        $a
      end;
  if ($expr == null) or (($expr | type) != "string") or (($expr | gsub("[[:space:]]+"; "")) == "") then
    {ok: false, reason: "missing"}
  else
    (lex($expr)) as $t
    | if ($t | length) == 0 then
        {ok: false, reason: "missing"}
      else
        parse_or($t; 0) as $r
        | if $r.err != null then
            {ok: false, reason: $r.err}
          elif $r.i != ($t | length) then
            {ok: false, reason: "trailing"}
          elif $r.ok then
            {ok: true, reason: "ok"}
          else
            {ok: false, reason: "disallowed"}
          end
      end
  end;

. as $meta
| (([$meta.workspace_members[]?] | map({(.): true}) | add) // {}) as $members
| [
    $meta.packages[]
    | . as $p
    | (judge($p.license)) as $j
    | {
        name: $p.name,
        version: $p.version,
        id: $p.id,
        license: $p.license,
        license_file: $p.license_file,
        workspace_member: ($members[$p.id] == true),
        license_ok: $j.ok,
        reason: $j.reason
      }
  ]
| sort_by(.name, .version, .id) as $components
| ($components | map(select(.license_ok | not)) | length) as $failed
| ($components | length) as $n
| {
    bomFormat: "openmind-cargo-sbom",
    specVersion: "1",
    desk_id: $desk,
    generated: $ts,
    source: "cargo metadata --format-version 1 --offline",
    mode: $mode,
    workspace_root: $meta.workspace_root,
    allowlist: allow,
    license_rule: "SPDX expression must be satisfiable from the allowlist. OR passes when one side passes. A slash between license ids is treated as OR. AND passes only when every side passes. WITH passes only when both the license id and the exception id are on the allowlist. Missing, empty, or unparsable licenses fail. Identifiers are case-sensitive.",
    components: $components,
    summary: {
      packages: $n,
      failed: $failed,
      ok: ($n > 0 and $failed == 0)
    }
  }
JQ
)

out_tmp=$(mktemp)
if ! jq --arg ts "$ts" --arg mode "$mode" --arg desk "openmind.B44" \
  "$filter" "$meta" > "$out_tmp"; then
  die "jq could not build the SBOM"
fi

printf 'sbom: DESK-ID openmind.B44\n'
printf 'sbom: time %s\n' "$ts"
printf 'sbom: mode %s\n' "$mode"
printf 'sbom: allow %s\n' "$ALLOW_WORDS"
printf '%s\t%s\t%s\t%s\t%s\t%s\n' status kind name version license reason
jq -r '
  .components[]
  | [
      (if .license_ok then "ok" else "FAIL" end),
      (if .workspace_member then "workspace" else "dep" end),
      (.name // "-"),
      (.version // "-"),
      (if (.license == null or .license == "") then "-" else .license end),
      .reason
    ]
  | @tsv
' "$out_tmp"

packages=$(jq -r '.summary.packages' "$out_tmp")
failed=$(jq -r '.summary.failed' "$out_tmp")
ok=$(jq -r '.summary.ok' "$out_tmp")

if [ "$mode" = write ]; then
  mkdir -p -- "$root/dist"
  cp -- "$out_tmp" "$root/dist/sbom-cargo.json"
  printf 'sbom: wrote %s\n' "$root/dist/sbom-cargo.json"
fi

if [ "$ok" != true ]; then
  printf 'sbom: FAIL (%s packages, %s disallowed)\n' "$packages" "$failed"
  exit 1
fi
printf 'sbom: ok (%s packages)\n' "$packages"
exit 0
