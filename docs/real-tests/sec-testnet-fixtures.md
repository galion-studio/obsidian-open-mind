<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: sec-testnet-fixtures (wave-4 slot 225, Security S5.1 + Docs D16)

Base integration-5 `0d68f46`, branch `swarm/sec-testnet-fixtures`. Box only; the VPS
was probed READ-ONLY (HTTP GET on 127.0.0.1, `systemctl is-active`, `journalctl` reads).
No real address is printed here or in any log: every check against the VPS address is
count-only (the address is held in a mode-600 temp file on the box and removed after).

## What changed

| Area | Before | After |
|---|---|---|
| openmind-discovery `admit`, `routing`, `probe`, `cmd` tests | public DNS literal (`8.8.8.x`) as the "not local" case | `203.0.113.8` / `203.0.113.0/24` (TEST-NET-3) |
| `scripts/real/lan-discovery.sh`, `docs/real-tests/lan-discovery.md`, `docs/merges/integration-2.md` | same literal | TEST-NET-3; the old recorded output line now reads `<public /24>`, plus a dated re-run block |
| openmind-nerve `logscan`, `replay`, `cortex` fixtures (`ip=` field of cloudflared lines) | Cloudflare edge / `1.2.3.4` / `1.1.1.1` | `198.51.100.13` / `203.0.113.4` / `203.0.113.1` |
| galion-guard `nft`, `ipnet`, `filter` tests | `1.2.3.4`, `5.6.7.8`, `1.2.3.4567` | `192.0.2.4`, `198.51.100.8`, `192.0.2.4567` |
| `stack/hermes/SOUL.md`, `TEACHER/QUEUE.md` | old France server IPv4 | `<France server IP, redacted>` |
| `docs/TERMINAL-LOG.md` (3 lines) | box egress IPv4 | `<box egress IP, redacted>` |

`// repo-check: allow` markers that only covered the old public literal were removed.
On the three nerve lines that also carry a UTC (`Z`) timestamp the marker was kept,
with the reason changed to "UTC input fixture in cloudflared log format", because the
`Z` is the real cloudflared log format the parser must read.

### Left on purpose (5 gate-10 hits, each one is the point of its test)

| File | Literal | Why it stays |
|---|---|---|
| `crates/hermes-tools/tests/fixtures/ticket/escape.rs` | `1.1.1.1:443` | sandbox egress probe: it is really dialled and must be reachable on the open internet, so the sandbox block is what makes it fail. A TEST-NET target would fail anyway and the test would prove nothing. |
| `crates/galion-guard/src/ipnet.rs` | `128.0.0.1` | first address after `127.0.0.0/8`: boundary case of `contains` |
| `crates/openmind-discovery/src/probe.rs` | `100.64.0.0/24` | CGNAT (RFC 6598) must be refused by the /24 probe |
| `crates/repo-check/tests/checks.rs` | `100.101.102.103` | CGNAT must NOT count as public for repo-check |
| `crates/repo-check/tests/checks.rs` | `1.2.3.4.5` | five-octet string must not parse as an address |

Gate 10 greps any non-RFC1918/non-documentation literal, so CGNAT counts there while
repo-check (`is_public`) treats 100.64.0.0/10 as non-public. Aligning gate 10 with
repo-check would drop the count to 3; not done here (owner decision, see below).

## Public-export gate 10 (same grep/awk as `scripts/public-export.sh` step 10, over `git archive`)

| Ref | public IPv4 literals | quick-tunnel hostnames |
|---|---|---|
| base `0d68f46` | 27 | 1 |
| this branch | 5 | 1 |

The one quick-tunnel hostname (`docs/session-2026-10-18-evening.md:45`) is the open
SEC-5 owner decision and out of scope here. VPS address in the working tree, any form
(dotted, dashed, reversed): 0 files before and after (survival-heal.md was already
redacted by integration-4 42de672). It is still in 11 historical commits; `git archive`
exports the tree only, so the export is clean, but publishing full history would not be.

## Tests (box, `nice -n 10`, `-j 4`, own target dir), 2026-10-21 23:50 .. 2026-10-22 00:05 +02:00

- `cargo fmt --all --check`: ok
- `cargo clippy --all-targets -p openmind-discovery -p openmind-nerve -p galion-guard -p repo-check -- -D warnings`: clean
- `cargo test -p openmind-discovery -p openmind-nerve -p galion-guard -p repo-check`: 221 passed, 0 failed
  (incl. `routing::tests::operator_rows_must_be_local_http`, `cmd::tests::argument_errors_are_usage_and_touch_nothing`,
  `nft::tests::parses_set_json`, `ipnet::tests::cidr`, `filter::tests::address_must_end_at_a_boundary`, nerve logscan/replay/cortex fixtures)
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`: 1241 passed, 0 failed, 6 ignored
  (hermes-tools excluded: its escape tests are sandboxed, `sandbox_vps_only`; hermes-tools was not changed)
- `scripts/no-python.sh`: ok
- `repo-check` over `git archive`: base 10 errors / 48 warnings, branch 10 errors / 48 warnings,
  identical lists (the 10 errors are the known vendored candle-core main-merge blocker 0); `--only hosts`: 0 / 0 PASS

## Real binaries

1. `openmind-discovery find --no-mdns --probe <net> --registry <tmp>` (release), 2026-10-21T23:50:21+02:00:
   TEST-NET-3, TEST-NET-2, TEST-NET-1 and the old public /24 all give
   `refusing to probe <net>: only loopback, RFC 1918 and link-local /24s are allowed`, exit 2, no registry file written.
   The new fixture exercises exactly the same refusal path.
2. Live-data A/B through the real nerve (`openmind-nerve replay DIR --state`, release), 2026-10-21 ~23:53 +02:00:
   capture = live nerve `/state` (cycle 9, 23:41:56+02:00) + 2 h of live `cloudflared` journal
   (3114 lines, 1557 with `ip=`, all in the Cloudflare edge range). Copy B had every `ip=` rewritten to `198.51.100.13`.
   Result A vs B: both score 70/C, 3 incidents, exit 1; `jq -S` diff only in two `us` (microsecond timing) fields.
   So the `ip=` value is incidental to the nerve and TEST-NET fixtures behave like real lines.
   Captures deleted after the run.

## Live VPS observation (read-only, 2026-10-21 ~23:47 +02:00)

- nerve `127.0.0.1:8670/state`: mode live, cycle 9 (restarted shortly before), score 66 / D,
  healthy 11, degrading 1, failing-soon 1, absent 1, 2 open incidents; VPS address literal in /state: 0.
- cortex `127.0.0.1:8672/healthz`: 200; `gaia-cortex.service`, `galion-nerve.service`,
  `galion-nerve-shadow.service` active.
- `journalctl -u cloudflared` last 6 h: 6677 `ERR Request failed ... ip=` lines, all `ip=198.41.x`.

## Owner decisions

- D-225-1: align public-export gate 10 with repo-check and skip CGNAT 100.64.0.0/10 (5 -> 3 hits)?
- D-225-2: the VPS address is in 11 historical commits; publish tree-only (`git archive`, as now) or rewrite history before any public mirror (needs a force-push, not allowed for agents).
- D-225-3: `stack/hermes/SOUL.md` keeps a dated note about the France server (date 15 October has passed): drop the paragraph?
