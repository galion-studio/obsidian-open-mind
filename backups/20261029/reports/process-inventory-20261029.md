# Process inventory: 2026-10-29, 11:58 Europe/Zagreb (read-only)

VPS: up 6 days 13h, load 19.7/20.7/20.1, RAM 455 MB used of 16384 MB (no swap), disk / 30G of 100G used (30%).
The load is high, but ps shows about 0% CPU per process, which points to container/host-level load, not Galion.
CPU = cumulative CPUUsageNSec. Uptime = since ActiveEnter. All units are listed by `systemctl list-units --state=running`.

## VPS services
| name | status | PID | CPU (cum) | memory | port | notes |
|---|---|---|---|---|---|---|
| console (Rust fallback) | active | 2000253 | 1.39 s | 1.0 MB | 127.0.0.1:8080 | galion-console-serve-dff00cbe, since 29 Oct 11:02 |
| console-rust-primary | active | 1998684 | 0.33 s | 1.7 MB | :18080 | same binary dff00cbe (description says sandbox 2d846fb8), since 29 Oct 11:01 |
| gaia-cortex | active | 1876527 | 0.15 s | 1.9 MB | :8674 | gaia-cortex-4b036bc-llm, since 29 Oct 08:21 |
| galion-nerve (openmind-nerve) | active | 1887891 | 4.8 s | 9.3 MB | :8670 | openmind-nerve-f66974c, since 29 Oct 08:34; galion-nerve-shadow disabled |
| hermes-rs | active | 255539 | 57 s | 1.6 MB | :8642 | hermes-gateway-b7fbbc78, since 27 Oct 19:46 |
| galion-stt | active | 1922609 | 69 s | 302 MB | :8651 | Moonshine Tiny, since 29 Oct 09:17 |
| openmind-memory | active | 291 | 6.9 s | 0.6 MB | :8420 | openmind-memory-aa77864, since 22 Oct 23:46 |
| openmind-queue | active | 294 | 12.8 s | 0.5 MB | :19120 | /usr/local/bin/openmind-queue, since 22 Oct |
| galion-desk | active | 264677 | 7.1 s | 0.3 MB | :8646 | since 27 Oct 19:56 |
| galion-one | active | 304 | 10669 s (~2.96 h) | 1.7 MB | :8091, :8002 | top CPU consumer; since 22 Oct |
| galion-health | active | 284 | 30.6 s | 0.3 MB | :8660 | since 22 Oct |
| tuner | active | 313 | 0.34 s | 0.1 MB | :8650 | since 22 Oct |
| tanga-proto | active | 311 | 0.02 s | 0.3 MB | :8671 | static mock prototype |
| link-watch-rs | active | 293 | 86 s | 0.2 MB | n/a | Rust, since 22 Oct |
| cloudflared | active | 272 | 1526 s | 26.5 MB | :20241 (metrics) | since 22 Oct |
| sshd (laptop reverse tunnel) | active | 1989334 | n/a | 7 MB RSS | 127.0.0.1:11434 | Ollama tunnel from the laptop |
| system: fail2ban, postfix (:25), resolved (:53), rsyslog, cron, journald, ssh (:22), networkd, networkd-dispatcher, gettys | active | n/a | n/a | n/a | n/a | standard |

Timers: guardian-tick (every minute, last run 11:58), galion-backup (last 07:17, next 13:18), sysstat, apt-daily, logrotate, man-db, e2scrub, dpkg-db-backup, motd-news, tmpfiles-clean.
Path unit: gaia-hermes-handoff.path is enabled (its service is inactive).

## Laptop GALION-STUDIO (seen from the VPS only)
There is no command channel, so processes can't be listed.
| name | status | PID | CPU | memory | port | notes |
|---|---|---|---|---|---|---|
| Ollama (via reverse tunnel) | up | n/a | n/a | n/a | VPS 127.0.0.1:11434 | /api/version 0.35.1; /api/ps: no models loaded |
| laptop.json | 11:48 | n/a | i7-13620H | 63.7 GB RAM, 53.2 GB free | n/a | RTX 4060 Laptop 8 GB, user Gigabyte |
| link.json | 11:58 | n/a | n/a | n/a | n/a | laptop_port true, model galion:latest (idle, 170 ms), hermes true, france false. Recent events: up at 10:51, ran qwen3.5-abliterated:9b at 10:52, back to idle at 10:57 (UTC times in the file +1h) |

## Box
| name | status | PID | CPU | memory | port | notes |
|---|---|---|---|---|---|---|
| node sand-host (host-main.cjs) | running | 1392381 | 29.1% | 9.3% | n/a | agent host runtime, up 7h47m |
| diff-policy.sh | short-lived | 2929713 | 17.8% | 0% | n/a | transient |
| node (platform daemons) | running | 243, 257, 227, 242, 275, 313, 3183 | ≤2.2% | ≤0.5% each | n/a | up ~1d5h |
| Xvfb / x11vnc / websockify | running | various | <1% | <1% | n/a | desktops |
| cargo builds in /workspace/swarm | none | n/a | n/a | n/a | n/a | no cargo or rustc processes |

## Failed units
None (`systemctl --failed` is empty).

## Python/Node leftovers on the VPS
Only distro tooling: fail2ban-server and networkd-dispatcher (python3). No Galion Python or Node processes. console.service now runs Rust in place of Node srvx.

## Overnight loop
No unit, timer or cron for an overnight loop was found (grep "overnight" in systemd, cron.d and spool: no hits; no swarm or sandbox units).
The scheduled jobs that do exist are guardian-tick (every minute, last run 11:58) and galion-backup (last run 07:17).
Either the loop runs from somewhere else (laptop or box), or it isn't deployed.
