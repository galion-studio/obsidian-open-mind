<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: cortex-t212-edgerate (slot 107, ports 19770-19779)

- Box run: 2026-10-21 01:22:17 - 01:22:56 CEST (+02:00), release build of `openmind-nerve` (x86_64), branch
  `swarm/cortex-t212-edgerate`. Full log on the box: `/workspace/swarm/evidence/cortex-t212-edgerate/real-run.txt`.
- Command: `GALION_REAL=1 cargo-slot test -j 2 --release -p openmind-nerve --test real_edge_rate --test real_cortex_sensor -- --nocapture`
- The Rust test builds the capture, starts the real binary, probes it and kills it (no scripts). All started processes
  were gone afterwards (`pgrep -f "openmind-nerve serve"` = 0, no listener on 1977x).

## VPS probe (read-only, 2026-10-21 01:10:22 CEST)

`journalctl -u cloudflared --since -1h -o cat` on the VPS (counts only, nothing written there):

| what | value |
|---|---|
| cloudflared lines, last hour | 1036 |
| `Request failed ... dest=` lines | 518 (each failure = this line + one `ERR  error=` line without hostname) |
| hostnames | merlinelement.com 267, galion.studio 237, galion.app 3, www.maivirtue.com 2, www.galion.studio 2, www.galion.app 2, slaviknomad.com 2, ws3d.app 1, workspace3d.app 1, maivirtue.com 1 |
| debug request lines (`DBG GET https://...`) | 0 (cloudflared runs at info level) |
| units | `cloudflared.service` (named tunnel), `tuner-quick.service` (quick tunnel) |

## Box output (release binary)

```
replay fixture: score 21 grade "F" pass false (= pre-trait golden)
replay1 live capture: score 61 grade D (unchanged)
serve :19430 cycle 1: sensors ["listeners", "scanners", "edge", "components", "ddos", "edge-rate"] us [3, 0, 0, 0, 0, 0], cycle_ms 22, rss 3188 KiB; event {"ts":"2026-10-21T01:22:34+02:00","source":"listeners","kind":"vulnerability","severity":"warn","data":{"findings":1,"ids":["unmanaged-listeners"],"top":{"cause":null,"component":"host","evidence_n":2,"fix":"identify the owners (ss -ltnp), then stop them or give them a unit (owner approval; the nerve never kills processes)","id":"unmanaged-listeners","title":"1 loopback listener(s) no monitored unit owns: :19433"}}}
serve :19430 cycle 2: stray gone, finding cleared
serve :19434 NERVE_SENSORS_OFF=listeners cycle 2: sensors ["scanners", "edge", "components", "ddos", "edge-rate"], stray :19435 not reported
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 11.05s
replay flood capture (3096 cf lines, 27.584095ms): host streams [("edge-host:hot.example.test", "flood", 2748), ("edge-host:one.example.test", "normal", 50), ("edge-host:quiet.example.test", "normal", 348), ("edge-host:two.example.test", "normal", 50)]
finding: ["critical"] 1 public hostname(s) under a request flood at the edge: worst 40.0 req/s over 60 s vs baseline 0.20 req/s, 100% failed at the origin
incident: "inc-83ba59b1" ["critical"] "request flood on cloudflared (stream edge-host:*, per public hostname)"
verdict: score 0 grade "F"
NERVE_EDGE_HOSTS=0: no host streams, score 21 (on: 0)
replay1 live capture (10.843395ms): 14 host streams, 419 failed requests, all normal, score 61 grade D (unchanged)
  edge-host:adriaticpadel.com normal total=1
  edge-host:galion.app normal total=4
  edge-host:galion.studio normal total=178
  edge-host:machine.galion.app normal total=3
  edge-host:maivirtue.com normal total=1
  edge-host:merlinelement.com normal total=180
  edge-host:slaviknomad.com normal total=2
  edge-host:tactical-belts.galion.app normal total=1
  edge-host:torchbearer.galion.app normal total=3
  edge-host:voice.galion.app normal total=2
  edge-host:ws3d.app normal total=40
  edge-host:www.galion.studio normal total=2
  edge-host:www.workspace3d.app normal total=1
  edge-host:www.ws3d.app normal total=1
serve :19770 cycle 2: sensors ["listeners", "scanners", "edge", "components", "ddos", "edge-rate"], edge_hosts {"folded_into_other":0,"hosts":0,"lines_counted":0,"max_hosts":16}, edge-rate us Some(0), cycle_ms 22
serve :19771 NERVE_SENSORS_OFF=edge-rate cycle 2: sensors ["listeners", "scanners", "edge", "components", "ddos"], edge_hosts null
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 11.09s
```

## What it shows

| check | result |
|---|---|
| flood on one hostname (40 req/s for 60 s vs 0.2 baseline) | `edge-host:hot.example.test` = flood; quiet host and the fixture's two dead-origin hosts = normal |
| one finding, no hostname in its title | `edge-host-rate` (ddos, critical, source `edge-rate`), host only in evidence |
| one incident, under the tunnel cause | `ddos:edge-cloudflared`, critical, title "request flood on cloudflared (stream edge-host:*, per public hostname)"; no `ddos:edge-host:*` incident |
| switch off (`NERVE_EDGE_HOSTS=0`) | no host streams, no finding, fixture verdict 21/F as before |
| real VPS capture (`nerve-capture/replay1`, 2026-10-20 19:25 CEST) | 14 hostname streams, 419 failed requests, all normal, verdict still **61/D** |
| serve :19770 | `edge-rate` registered after the 5 built-ins, `edge_hosts` in `/api/v1/state`, cycle 22 ms, sensor < 1 us |
| serve :19771 `NERVE_SENSORS_OFF=edge-rate` | sensor gone, `edge_hosts` null (feeder off too) |
| pre-trait golden (`real_cortex_sensor`) | unchanged once the additive `edge-host:*` streams are stripped; replay1 61/D |
| speed | 3096 cloudflared lines through a full 3-snapshot replay in 30 ms |

Not covered on the box: live journald (the box has no journald, so serve mode sees no cloudflared lines; the
journal path is exercised through `replay`, which runs the same `Engine::cycle_with`). No VPS deploy (planning phase).
