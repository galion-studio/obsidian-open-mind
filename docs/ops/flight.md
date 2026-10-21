<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# galion-flight: pre/postflight go/no-go per service (ops plan S3.2)

Written 2026-10-21 16:10 +02:00 by wave-4 slot 195 (vps-preflight-scripts), base integration-4 42de672.

## What it is

`galion-flight preflight <svc>` answers "is the live service exactly in the state the change
assumes, and is it a safe moment?" `galion-flight postflight <svc>` answers "did the change land
as tested and is the service healthy?". Six services ship with a manifest:
`nerve`, `memory`, `hermes-rs`, `galion-one`, `tuner`, `cloudflared`
(`config/ops/flight/<svc>.conf`, also compiled into the binary).

The ops plan named the deliverable `scripts/ops/{preflight,postflight}-<svc>.sh`. It ships as
one Rust binary with the same 12 entry points instead, because of hard rule 1 / the 20:41
Rust-only rule (the swarm has been porting every `scripts/real/*.sh` and CI bash step to Rust).
Mapping: `scripts/ops/preflight-nerve.sh` = `galion-flight preflight nerve`, and so on.
Whether thin `.sh` wrappers are still wanted is decision D-195-1.

## Guarantees (enforced in code and pinned by tests)

- **One read per check.** Every check is exactly one of: `systemctl show -p …` (fixed property
  list), `readlink /proc/PID/exe`, `sha256sum PATH`, `stat -c …` / `stat -f …`,
  `ss -ltnH 'sport = :P'`, or an HTTP GET (`curl -sS -m 5`, no `-X`, no data). The VPS command
  set is a closed Rust enum (`target::Remote`); every argument is validated against a strict
  character set and single-quoted. A unit test checks that no rendered command contains a shell
  operator, `-X`, `--data`, `POST`, an output file or `Environment`.
- **No loops.** No retry, no polling, no "wait until healthy". A check that fails, fails.
  (Waiting is the deploy tool's job; galion-flight judges a moment.)
- **No writes, no POST, no restart.** Over ssh nothing is copied to the VPS; the ssh control
  socket lives on the box and is closed at the end.
- **No secrets.** `Environment=` and `ExecStart=` are never read (a tunnel token can live there).
  Response bodies are judged, never printed (only length and the expected value). The
  cloudflared `/ready` body (connector id) is judged by status only.
- **Fail closed.** An unreadable value is FAIL, never "not scheduled"/"unknown = fine".
- Exit code: `0` GO, `1` NO-GO, `2` usage or manifest error.

## Checks

| Check id | Read | Pre | Post | NO-GO when |
|---|---|---|---|---|
| `unit.state` | systemctl show | ✓ | ✓ | not `active/running`, Result ≠ success |
| `unit.restarts` | (same read) | ✓ | ✓ | NRestarts > `restarts_max` (0) |
| `unit.enabled` | (same read) | ✓ | ✓ | UnitFileState ≠ `enabled` |
| `unit.dropins` | (same read) | ✓ | ✓ | drop-in set differs (someone added/removed a drop-in) |
| `unit.restarted` | (same read) | | with `--since-unix` | ExecMainStartTimestamp before the change time (it never restarted) |
| `exe.running` | readlink /proc/PID/exe | ✓ | ✓ | the process runs another path |
| `exe.running-sha256` | sha256sum /proc/PID/exe | ✓ | ✓ | running image ≠ expected sha (catches "file swapped, not restarted") |
| `exe.file-sha256` | sha256sum exe | ✓ | ✓ | file on disk ≠ expected sha (what the next restart would run) |
| `exe.stat` | stat | ✓ | ✓ | mode/owner differ |
| `port.listen` | ss | ✓ | ✓ | nothing listening |
| `http.get` | GET | ✓ | ✓ | status or body (exact / contains / json key) differ |
| `nerve.view` | GET nerve /state | ✓ | ✓ | the nerve's own status for the component is not in `nerve_ok` (default `healthy`) |
| `unit.state/restarts` of `also_unit` | systemctl show | ✓ | ✓ | companion unit (tuner-quick) not running |
| `file.sha256` | sha256sum | ✓ | | rollback binary / deploy tool / config missing or changed |
| `file.exists` | stat | ✓ | | unit backup missing |
| `timer.quiet` | systemctl show timer | ✓ | | `galion-backup.timer` fires within 300 s |
| `disk.free` | stat -f | ✓ | | < 2048 MiB free on /opt/galion |
| `public.get` | GET from the box | ✓ | ✓ | public URL through Cloudflare not 200/expected body |

## Per-service go/no-go (manifests as of 2026-10-21 15:56 +02:00)

| Service | Unit / port | Expected running image | Rollback target checked | Extra |
|---|---|---|---|---|
| nerve | galion-nerve.service / 8670 | openmind-nerve-aa77864 `f0994b6d…` + drop-ins 50-rust-bin, 60-nerve-eventlog | galion-nerve `3b150dea…`, unit backup -094354 | /healthz `ok`, /state service=galion-nerve, mode=live |
| memory | openmind-memory.service / 8420 | openmind-memory-aa77864 `18e35ab4…` + 50-rust-bin, 60-nerve-url | openmind-memory `473ae4a3…`, unit backup -094509 | /health status=ok |
| hermes-rs | hermes-rs.service / 8642 | hermes-gateway-aa77864 `eb691b29…` + 50-rust-bin, 60-nerve-url | hermes-gateway `cd344b16…`, unit backup -094621 | /health status=ok, implementation=hermes-rs |
| galion-one | galion-one.service / 8091 | /opt/galion/rust/bin/galion-one `24cb4976…` + 50-rust-bin | galion-one.old `090c15d0…` | /health `ok`; mouth :8002 503 status=loading (known stub); public one.galion.app/health |
| tuner | tuner.service / 8650 | /opt/galion/tuner/bin/tuner `89c7fcbc…`, no drop-ins | galion-deploy backup tuner-20261021-043232+0200.bin `ecad72a4…` | /health `{"ok":true}`, / contains `<html` |
| cloudflared | cloudflared.service / 20241 | /opt/galion/tooling/bin/cloudflared `03f1f25d…` mode 775 | ingress config `e3825ce4…`, config backup -063215 | /ready 200 (status only), tuner-quick active + :20242/ready, public one.galion.app/health and galion.app/ |

All six also check: galion-deploy `2a60e1ac…` present, backup timer quiet for 5 min, 2 GiB free.

Note: `docs/cutover/tuner.md` 3.2 still says the live tuner "must still be ecad72a4…dd25". Since
the 04:32 +02:00 deploy the live sha is `89c7fcbc…2b12` and `ecad72a4…` is the rollback copy;
`docs/cutover/nerve.md` section 1 still describes `ExecStart=/opt/galion/rust/bin/galion-nerve`
without the slot-180 drop-ins. The manifests here carry the current state.

## Use

```
# before a change (from the box; nothing is copied to the VPS)
galion-flight preflight nerve                       # default --target vps --host slavik-vps
galion-flight preflight tuner --show-commands       # also print every remote read sent

# after deploying build X at time T
galion-flight postflight nerve \
  --set exe=/opt/galion/rust/bin/openmind-nerve-X --set sha256=<sha of X> \
  --since-unix T

# box instance (no unit): checks by pid
galion-flight postflight tuner --target local --pid 1234 --set unit= --set port=20654 \
  --set exe=/path/tuner --set sha256=… --set owner=1000:1000 --set nerve=
```

`--set key=value` overrides one manifest key; the first `--set` of a list key (`get`, `file`,
`exists`, `also_unit`, `public`) replaces that list. `--manifest FILE` uses another manifest.
`--no-public` skips the box→Cloudflare GETs. A manifest must be updated in the same change
that moves the live state (new sha, new drop-in); until then preflight says NO-GO, on purpose.

Runtime: 6-10 s per service over one multiplexed ssh connection (8-15 remote reads).

## Owner decisions (Slavik)

- **D-195-1 Rust vs .sh.** The row asked for 12 `.sh` scripts; delivered as one Rust binary
  (Rust-only rule). Options: (a) keep Rust only (recommended); (b) add 12 one-line `.sh` wrappers
  calling it (adds shell back).
- **D-195-2 cloudflared is NO-GO today.** The nerve rates `cloudflared` `failing-soon` since
  09:43 +02:00 (tunnel routes 16 public hostnames to 6 closed origin ports, ~90 origin errors
  per 5 min). Everything else about cloudflared passes. Options: (a) keep strict: no cloudflared
  change until the ingress is fixed (ingress-lint branch, D-T32-1) (recommended); (b) accept
  `failing-soon` for cloudflared only (`nerve_ok = healthy,failing-soon` with the reason).
- **D-195-3 cloudflared binary is mode 775** (group-writable, root:root). Tightening to 755 is a
  live permission change (not done; this task is read-only). Options: (a) chmod 755 in the next
  approved cloudflared window; (b) keep and pin 775 (current manifest).
- **D-195-4 Pinning known-bad states.** The galion-one manifest pins the mouth's hard-coded
  `503 loading`. A 42de672 build answers `503 unavailable` (honest), so a galion-one deploy from
  integration-4 will be NO-GO on that line until the manifest moves with it (seen in the box
  test). Options: (a) keep pinning exact known states (recommended: any drift is visible);
  (b) only check that the mouth answers 503.
- **D-195-5 Where it runs.** Today: from the box over ssh (nothing installed on the VPS).
  Options: (a) keep; (b) install the static musl build (582,320 B, sha256 `4688c530…`) as
  /opt/galion/rust/bin/galion-flight so Gaia/galion-deploy can call it on the VPS (live install,
  needs approval).
- **D-195-6 Gate galion-deploy on it.** galion-deploy could refuse to start unless preflight is
  GO and finish with a postflight. Not implemented here (galion-deploy change + approval).
