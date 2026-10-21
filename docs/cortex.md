# Cortex

- DESK-ID: openmind.B41
- Written: 2026-10-19T16:12:35+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Host: ip50
- Scope: design only. No code, no listener, no unit, no crate edit, no commit.

The cortex judges. It does not act. Real money is not approved. This design has no payment rail, no balance, and no spend.

## 1. Place in the stack

`docs/main.md` section 3 asks the stack to know, in real time, about vulnerabilities, where it is bleeding, and about DDoS, and how it could change its own environment. This page specifies only the knowing. Changing sshd, the firewall, a unit, or a ban list is out of scope.

Two tickets own the pipe. This page does not implement either.

- B33 adds `NerveEvent` to `openmind-schema`: `ts`, `source`, `kind`, `severity`, `data`. As of this writing that struct is not in `crates/openmind-schema/src/lib.rs`. The types there are `Machine`, `Provider`, `ProviderKind`, `KeySlot`, `Surface`, and `Stack`.
- B40 is the append-only JSONL writer: Zagreb timestamps, size rotation, configurable path. This page does not choose the path and does not start the writer.

The in-memory 1 Hz buffer inside `galion-one` is not this cortex. The separate `galion-nerve` process on `127.0.0.1:8670` is not this cortex. Neither is called, restarted, or given a new socket here.

`openmind-survival` already classifies host memory, load, and disk at 85 percent warn and 95 percent crit, and it only suggests. Those probes stay there. They are not duplicated.

## 2. Nerve event shape

One firing is one JSON object, one line, matching the B33 field list. `ts` is Europe/Zagreb RFC 3339 with a numeric offset, produced by `zagreb-time`. Never `Z`, and never a raw auth.log stamp (those stamps are `+00:00`).

On 2026-10-25 at 03:00 local, Europe/Zagreb leaves CEST (`+02:00`) for CET (`+01:00`). The cortex does not format that itself. `zagreb-time` already owns the fold.

| field | type | rule |
| --- | --- | --- |
| ts | string | Zagreb RFC 3339, offset required |
| source | string | `auth.log` or `memory-gateway` |
| kind | string | `vulnerability`, `bleeding`, or `ddos` |
| severity | string | `warn`, `high`, or `crit` |
| data | object | counts and short tokens only |

`data` never holds a raw log line, an address, an account name, a key, a cookie, or a password. A share is a percent. A status is a number. The highest severity wins inside one `(source, kind)` pair per window, so a flood writes one line, not three.

Shape only. The first object is the ddos warn edge. The second is the bleeding crit edge. Neither is a live firing.

```json
{"ts":"2026-10-19T16:12:35+02:00","source":"auth.log","kind":"ddos","severity":"warn","data":{"window_s":300,"rule":"failed_password","failed_password":30,"invalid_user":0,"accepted_password":0,"accepted_publickey_root":0,"accepted_publickey_other":0,"distinct_fail_sources":0,"top_fail_share_pct":0,"max_auth_attempts":0,"bytes_added":0}}
```

```json
{"ts":"2026-10-19T16:12:35+02:00","source":"memory-gateway","kind":"bleeding","severity":"crit","data":{"window_s":10,"rule":"rate_per_s","rate_per_s":50,"sample_s":10,"status":200,"connect":"ok","implementation":"openmind-memory"}}
```

Severity sits next to survival, it does not restart anything:

| cortex severity | survival escalation | incident severity in openmind-survival |
| --- | --- | --- |
| warn | Warn | low |
| high | Warn | high |
| crit | Warn | high |

Crit is still words. No `SuggestRestart`, no ban, no reload.

## 3. Read-only sources

Two sources. Both already exist. The collector that would read them is not built here. When it is built it is a short-lived Rust read, not a daemon, not a listen socket, and not Python.

### 3.1 auth.log counts

File: `/var/log/auth.log` (the live file). The rotated file `/var/log/auth.log.1` is context for the baseline, not a second source.

A sample counts lines whose stamp falls inside the last 300 seconds. It does not `tail -f`. It does not keep a file handle open across the day. Line counts, not parsed messages stored:

| data key | line test |
| --- | --- |
| failed_password | `Failed password` |
| invalid_user | `Invalid user` |
| accepted_password | `Accepted password` |
| accepted_publickey_root | `Accepted publickey for root` |
| accepted_publickey_other | `Accepted publickey` and not `for root` |
| max_auth_attempts | `maximum authentication attempts` |
| distinct_fail_sources | count of distinct `from` addresses on `Failed password` lines |
| top_fail_share_pct | largest of those addresses, as a percent of `failed_password` |
| bytes_added | bytes appended to the file during the window |

`Failed password for invalid user` is already inside `failed_password`. The two keys are not added together.

Addresses are counted, then dropped. They are not written into `data`.

### 3.2 memory-gateway /health rate

Logical source name: `memory-gateway`. It is the loopback health port the ticket names, TCP `127.0.0.1:8420`, path `/health`.

Measured this hour: `memory-gateway.service` is inactive and disabled. `openmind-memory.service` is active and is the process bound to that port. The public Internet cannot open it. A high rate is a local loop burning CPU, not an external DDoS. External DDoS is the auth.log signal, because ssh is the reachable door (ufw is active, default deny incoming, `22/tcp` allowed).

Rate, without a listener and without a packet capture:

1. Over 10 seconds, once per cortex cycle, read the kernel socket table for local port 8420 (`ss`, no bind).
2. `rate_per_s` is the growth of ESTAB plus TIME-WAIT, divided by 10. A flat TIME-WAIT count means zero new connections.
3. After that sample, one `GET /health` records `status` and `connect`. That single GET is not added to the rate.
4. Do not call `/metrics`, `/ready`, `/status`, or `/`. They answered 401. Do not send a cookie or a key to get past that.

`/health` today does not include a request counter or an RSS field. Until it does, the socket-table delta is the rate. Do not restart the unit to turn on access logs.

## 4. Signals and thresholds

Window for auth.log rules: 300 seconds. Window for the health rate: 10 seconds, checked once per 300-second cycle. Under every threshold: write nothing.

### 4.1 Vulnerability (source `auth.log`)

Policy the counts are compared with, read from `sshd -T` and not changed: `passwordauthentication no`, `permitrootlogin without-password`, `pubkeyauthentication yes`, `maxauthtries 3`.

| severity | rule | why |
| --- | --- | --- |
| crit | `accepted_password >= 1` | Password login is off. One success is a policy break. |
| high | `accepted_publickey_other >= 5` | A burst of successful non-root logins. |
| warn | `accepted_publickey_other >= 1` | Any successful non-root login. Names are not stored. |
| warn | `invalid_user >= 20` (was 30; tightened by T3.1, docs/cortex/calibration.md) | Account enumeration, not the credential flood. |
| warn | `max_auth_attempts >= 1` | A client passed the three-try cap. |

`accepted_publickey_root` is recorded in `data` and is not a threshold. The desk itself logs in as root with a key. Treating it as an attack would fire on this work.

### 4.2 DDoS (source `auth.log`)

Credential flood and spread. This is the public surface.

| severity | rule |
| --- | --- |
| crit | `failed_password >= 300`, or (`failed_password >= 100` and `distinct_fail_sources >= 40`) |
| high | `failed_password >= 100`, or `distinct_fail_sources >= 25`, or (`top_fail_share_pct >= 80` and `failed_password >= 50`) |
| warn | `failed_password >= 30`, or `distinct_fail_sources >= 10` |

Warn at 30 failures in 300 seconds is 360 per hour if it holds. That is above a quiet hour and below the worst hour already on disk (section 5). It will not sit in alarm all day, and it will see a morning like 2026-10-18.

A high `invalid_user` count does not by itself raise `ddos`. It raises `vulnerability`. Both may fire in the same window. They stay two events.

### 4.3 Bleeding (two sources, one kind)

Disk bleed from the auth log, and CPU bleed from the health port.

| source | severity | rule |
| --- | --- | --- |
| auth.log | high | `bytes_added >= 1048576` (1 MiB in 300 s) |
| auth.log | warn | `bytes_added >= 262144` (256 KiB in 300 s) |
| memory-gateway | crit | `rate_per_s >= 50` |
| memory-gateway | high | `rate_per_s >= 10`, or `connect` is `fail`, or `status` is outside 200-399 |
| memory-gateway | warn | `rate_per_s >= 1` |

50 per second is the crit line because a note on 2026-10-18 recorded about 67 hits per second on this path (section 5). One hit per second is already more than a 30-to-60-second health probe. The port is loopback, so this kind stays `bleeding`, not `ddos`.

Host RSS, load, and disk percent stay on the survival probes (85 / 95). The old Node gateway's size is context, not a live field: this design does not open `/proc` as a third source.

## 5. Measured baseline

Reads were read-only. No unit was started or stopped. No listen socket was opened. The live auth file grew a little during the reads because root key logins were happening, including this session. Figures below are tied to the read that produced them.

### 5.1 Current auth.log

File span of the stamps: `2026-10-18T00:00:04+00:00` through `2026-10-19T14:06:30+00:00`, which is `2026-10-18T02:00:04+02:00` through `2026-10-19T16:06:30+02:00`. At `2026-10-19T16:06:31+02:00` the file was 45672 lines and 5504452 bytes.

Hourly `Failed password` counts, 39 UTC hours, read at `2026-10-19T16:08:18+02:00`:

| | |
| --- | ---: |
| sum | 6370 |
| hours at 0 | 20 |
| median hour | 0 |
| 36th of 39, sorted ascending | 684 |
| max hour | 1244 |

The max hour is `2026-10-18T04:00+00:00`, that is `2026-10-18T06:00+02:00`. The next four hours were 982, 759, 684, and 607. Thresholds in section 4.2 sit under that cluster and over the 20 empty hours. They were set from hourly totals. Five-minute peaks inside a moderate hour were not measured. A later collector may tighten the numbers after it has stored real 300-second windows. It must not loosen them in code on this page.

T3.1 update (2026-10-21 +02:00, swarm/cortex-t31-calibrate): 933 real 300-second windows from 6 VPS captures (2026-10-21 08:24 to 19:51 +02:00) were measured with `openmind-nerve calibrate`. Highest `invalid_user` window: 8. `failed_password` and `distinct_fail_sources`: 0 in every window (password auth is off, so these lines never appear). The `invalid_user` warn line was tightened from 30 to 20; the section 4.2 lines stay as they are (no real window to calibrate them on). Details: docs/cortex/calibration.md.

Same file, split read just after that hour table:

| count | value |
| --- | ---: |
| Failed password for invalid user | 4451 |
| Failed password for any other name | 1919 |
| Invalid user lines | 4608 |
| Accepted password | 1 |
| Accepted publickey for root | 1736 |
| Accepted publickey for any other name | 47 |
| distinct Failed-password sources | 65 |
| largest source share | 23.0 percent |
| Failed password from 127.0.0.1 | 0 |

The one `Accepted password` is a past line in this file. The running daemon refuses password auth. The rule is still crit, so a new line is not missed.

Hour ending `2026-10-19T16:08:50+02:00`: `Failed password` 0, `Accepted publickey` for root 513, for any other name 0, from one non-loopback source. That hour is the desk working. It is why root key accepts are not a signal. `Invalid user` in the hour ending `2026-10-19T16:08:19+02:00` was 3.

### 5.2 Rotated auth.log.1

Stamps `2026-10-11T00:01:08+00:00` through `2026-10-18T00:00:03+00:00` (`2026-10-11T02:01:08+02:00` through `2026-10-18T02:00:03+02:00`). 34741846 bytes.

| count | value |
| --- | ---: |
| Failed password | 54605 |
| Invalid user | 40537 |
| Accepted publickey | 4586 |
| Accepted password | 0 |

54605 in seven days is the same order as the earlier "~55,000 a week" note. It is the background this design treats as real, not as a reason to ignore ssh.

### 5.3 Health rate

At `2026-10-19T16:06:31+02:00`:

| check | result |
| --- | --- |
| memory-gateway.service | inactive, disabled |
| listener | `127.0.0.1:8420`, process `openmind-memory` |
| one GET `/health` | HTTP 200 in 0.000376 s |
| body | `status` ok, `version` 0.1.0, `uptime` 64340, `embeddingService` false, `implementation` openmind-memory |
| cgroup MemoryCurrent | 3076096 bytes |
| VmRSS | 4028 kB |
| GET `/metrics`, `/ready`, `/status`, `/` | 401, not opened further |

Ten socket-table reads, one per second, starting `2026-10-19T16:08:18+02:00`: ESTAB stayed 0, TIME-WAIT stayed 5. New connections in that sample: 0 per second. One snapshot cannot prove the rest of the hour. It does show that the old ~67/s figure is not happening now.

That ~67/s figure is the sentence in `docs/session-2026-10-18-evening.md`. It was not recomputed from a log: this process does not log each hit. It is the crit anchor, not a live value.

The retired Node gateway was reported at about 614 MB RSS in the foundation study. That process is not running, so it was not re-measured. The replacement on the same port is about 4 MB RSS. Bleed on this source is the request rate, not a return of that 614 MB figure, unless a later ticket adds a counter to `/health`.

### 5.4 Neighbors, not sources

Recorded so this design does not "turn them on" as if they were off. The 2026-10-18 evening note that said fail2ban was absent and ufw was off is stale.

| neighbor | state at this writing | cortex uses it? |
| --- | --- | --- |
| fail2ban sshd jail | active; currently failed 0; total failed 554; currently banned 1; total banned 31 | no |
| ufw | active; default deny incoming | no |
| sshd password auth | no | compared, not edited |

Ban lists and rule addresses are not copied here.

## 6. What a later collector does, and what it never does

Not built in B41.

Does:

- Runs once per cycle, then exits. `nice` if it is ever on a timer. No listen, no `tail -f`, no capture device.
- Reads auth.log counts for 300 seconds and the 10-second socket delta, then one GET.
- Converts `ts` with `zagreb-time`.
- Emits at most one `NerveEvent` per `(source, kind)` per cycle, through the B40 writer when that writer exists.
- Leaves `data` as counts and the tokens in section 3.

Never:

- Binds a port, or sits on 8420, 8670, or 8091.
- Restarts, reloads, bans, or edits sshd, fail2ban, ufw, or `openmind-memory`.
- Follows a `SuggestRestart` line. Survival may print one. Cortex does not.
- Calls a model, spends money, or names a payment rail.
- Writes secrets, raw log lines, or addresses into the JSONL.
- Uses Python.

Until B33 and B40 land, the shape in section 2 is the contract and the file is not created by this ticket.
