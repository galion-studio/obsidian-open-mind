# openmind-nerve self-eval (galion-nerve)

- time: 2026-10-20T19:06:05+02:00 (Europe/Zagreb)
- cycle: 3, mode: observe-only
- result: **FAIL**, score **21/100 (grade F)**, 6 error(s), 7 warning(s)
- system: **failing** (worst single component: degrading)
- verdict: failing: 5 open incident(s) (1 critical, 2 error), 3 vulnerability finding(s), 4 bleeding point(s). [critical] request flood on tuner (stream tuner) | [error] edge: tunnel routes 2 public hostname(s) to 2 closed origin port(s) :3011 :4001: 4.0 failed requests/min over 25 min | [error] galion-one: galion-one-mouth degrading: health probe HTTP 503 -- stuck loading for 1.9 d [health body {"status":"loading","model":"chatterbox","loaded":false,"ok":false,"provider":"local"}] | [warn] scanner/exploit probes against console | +1 more

## Score: what cost points

| points | what | why |
|---|---|---|
| -25 | critical incident: request flood on tuner (stream tuner) | 1 signal(s) from tuner correlated into one cause `ddos:tuner`; open since 2026-10-20T19:05:35+02:00. request flood: tuner: 20.0 req/s over the last 60 s vs baseline 0.10 req/s (threshold 10.0, x200); unknown (counter only), 0 IPs; 0% 4xx/5xx |
| -15 | error incident: edge: tunnel routes 2 public hostname(s) to 2 closed origin port(s) :3011 :4001: 4.0 failed requests/min over 25 min | 2 signal(s) from cloudflared correlated into one cause `edge`; open since 2026-10-20T19:05:05+02:00. public hostnames answer with Cloudflare error pages; every failed request is a visitor or bot turned away, and the error counters hide real faults |
| -15 | error incident: galion-one: galion-one-mouth degrading: health probe HTTP 503 -- stuck loading for 1.9 d [health body {"status":"loading","model":"chatterbox","loaded":false,"ok":false,"provider":"local"}] | 1 signal(s) from galion-one-mouth correlated into one cause `galion-one`; open since 2026-10-20T19:05:05+02:00. 1 signal(s) in group galion-one (galion-one:healthy, galion-one-mouth:degrading): one cause, one incident |
| -10 | vulnerability (error): port 9999 listens on 0.0.0.0 (not loopback) | LISTEN 0.0.0.0:9999 |
| -5 | warn incident: scanner/exploit probes against console | 1 signal(s) from console correlated into one cause `scan:console`; open since 2026-10-20T19:05:05+02:00. 3 scanner request(s): /.env, /phpinfo.php, /x/.git/config |
| -5 | warn incident: galion-desk: degrading: 2 restart(s) in the last hour | 1 signal(s) from galion-desk correlated into one cause `galion-desk`; open since 2026-10-20T19:05:35+02:00. 1 signal(s) in group galion-desk (galion-desk:degrading): one cause, one incident |
| -4 | vulnerability (warn): 2 loopback listener(s) no monitored unit owns: :8652 :8653 | not a component health/metrics port and not in NERVE_KNOWN_PORTS; typical cause: manual shadow/test processes left running (no unit, no restart policy, no patching) |

## Incidents (correlated)

### inc-3051083e [critical] request flood on tuner (stream tuner)

- open since 2026-10-20T19:05:35+02:00 (2 cycle(s)), components: tuner
- why: request flood: tuner: 20.0 req/s over the last 60 s vs baseline 0.10 req/s (threshold 10.0, x200); unknown (counter only), 0 IPs; 0% 4xx/5xx
- next: check the top source IPs; block or rate-limit them at the edge (Cloudflare WAF rule / rate limit) -- owner approval; the nerve never changes the edge
- signals:
  - critical tuner: request flood: tuner: 20.0 req/s over the last 60 s vs baseline 0.10 req/s (threshold 10.0, x200); unknown (counter only), 0 IPs; 0% 4xx/5xx

### inc-0b21ec15 [error] edge: tunnel routes 2 public hostname(s) to 2 closed origin port(s) :3011 :4001: 4.0 failed requests/min over 25 min

- open since 2026-10-20T19:05:05+02:00 (3 cycle(s)), components: cloudflared
- why: public hostnames answer with Cloudflare error pages; every failed request is a visitor or bot turned away, and the error counters hide real faults
- next: start the services behind the refused ports, or remove their ingress rules/DNS records (cloudflared config + DNS: owner approval)
- signals:
  - error cloudflared: tunnel routes 2 public hostname(s) to 2 closed origin port(s) :3011 :4001: 4.0 failed requests/min over 25 min
  - warn cloudflared: degrading: tunnel origin errors 30/40 requests in 1 min; degrading: 19 errors in 5 min (last: )

### inc-f8ae4b8b [error] galion-one: galion-one-mouth degrading: health probe HTTP 503 -- stuck loading for 1.9 d [health body {"status":"loading","model":"chatterbox","loaded":false,"ok":false,"provider":"local"}]

- open since 2026-10-20T19:05:05+02:00 (3 cycle(s)), components: galion-one-mouth
- why: 1 signal(s) in group galion-one (galion-one:healthy, galion-one-mouth:degrading): one cause, one incident
- next: the dependency never finished loading: read its logs, check the model files, then restart that one service (owner approval)
- signals:
  - error galion-one-mouth: degrading: health probe HTTP 503 -- stuck loading for 1.9 d [health body {"status":"loading","model":"chatterbox","loaded":false,"ok":false,"provider":"local"}]

### inc-4f012246 [warn] scanner/exploit probes against console

- open since 2026-10-20T19:05:05+02:00 (3 cycle(s)), components: console
- why: 3 scanner request(s): /.env, /phpinfo.php, /x/.git/config
- next: deny dotfiles and admin paths at the edge, serve production builds only (owner approval)
- signals:
  - warn console: 3 scanner request(s): /.env, /phpinfo.php, /x/.git/config

### inc-1e7c712c [warn] galion-desk: degrading: 2 restart(s) in the last hour

- open since 2026-10-20T19:05:35+02:00 (2 cycle(s)), components: galion-desk
- why: 1 signal(s) in group galion-desk (galion-desk:degrading): one cause, one incident
- next: read the unit journal; the reflex restarts allowlisted units within budget
- signals:
  - warn galion-desk: degrading: 2 restart(s) in the last hour

## Cortex

### Vulnerabilities

- **error** host: port 9999 listens on 0.0.0.0 (not loopback)
  - LISTEN 0.0.0.0:9999
  - fix: bind it to 127.0.0.1 and publish through the tunnel, or firewall it (owner approval)
- **error** console: 3 scanner/exploit request(s) reached console (3 distinct paths)
  - /.env x1
  - /phpinfo.php x1
  - /x/.git/config x1
  - the unit prints Vite dev-server messages: a dev server is answering internet traffic
  - fix: serve a production build (B61) instead of the dev server, and deny dotfiles (.env, .git) at the edge
- **warn** host: 2 loopback listener(s) no monitored unit owns: :8652 :8653
  - not a component health/metrics port and not in NERVE_KNOWN_PORTS
  - typical cause: manual shadow/test processes left running (no unit, no restart policy, no patching)
  - fix: identify the owners (ss -ltnp), then stop them or give them a unit (owner approval; the nerve never kills processes)

### Bleeding

- **error** cloudflared: tunnel routes 2 public hostname(s) to 2 closed origin port(s) :3011 :4001: 4.0 failed requests/min over 25 min
  - origin ? (port 3011) refused 50 dial(s), 50 failed request(s); hosts: two.example.test
  - origin ? (port 4001) refused 50 dial(s), 50 failed request(s); hosts: one.example.test
  - none of these ports has a listener on the host: the routes point at services that no longer run
  - cloudflared counters since its start: 48590 of 81940 requests failed (59%)
  - fix: either start the services behind those ports or remove the ingress rules / DNS for those hostnames (cloudflared config + DNS: owner approval; the nerve never touches them)
- **error** galion-one-mouth: galion-one-mouth has answered 503 "loading" for 1.9 d (since 2026-10-18T22:35:04+02:00)
  - health :8002/health -> 503 {"status":"loading","model":"chatterbox","loaded":false,"ok":false,"provider":"local"}
  - a model load takes minutes, not 1.9 d: the loader is stuck or the model is missing
  - fix: check the voice/model process logs and the model files, then restart that one service (owner approval)
- **warn** cloudflared: cloudflared: degrading: 19 errors in 5 min (last: )
  - fix: read the unit journal
- **warn** galion-desk: galion-desk restarted 2 time(s) in the last hour
  - systemd NRestarts=2
  - fix: read the unit journal before the restart
- **info** console: console holds 683.6 MiB
  - largest resident service on the host
  - fix: expected for a Node dev server; a production build would use a fraction

### DDoS (request-rate anomaly)

firing: **true**

- [normal] edge-cloudflared: 0.7 req/s over the last 60 s vs baseline 0.00 req/s (threshold 10.0, x0); unknown (counter only), 0 IPs; 75% 4xx/5xx
- [normal] edge-tuner-quick: 0.0 req/s over the last 60 s vs baseline 0.00 req/s (threshold 10.0, x0); unknown (counter only), 0 IPs; 0% 4xx/5xx
- [flood] tuner: 20.0 req/s over the last 60 s vs baseline 0.10 req/s (threshold 10.0, x200); unknown (counter only), 0 IPs; 0% 4xx/5xx

## Components

| component | Gaia | status | unit | http | latency | mem | cpu | restarts 1h | log errors | reasons |
|---|---|---|---|---|---|---|---|---|---|---|
| openmind-memory | APOLLO | healthy | active | 200 | - | 2.9 MiB | - | 0 | 0 | - |
| hermes-rs | ARTEMIS | healthy | active | 200 | - | 3.9 MiB | - | 0 | 0 | - |
| link-watch-rs | APOLLO | healthy | active | - | - | 2.0 MiB | - | 0 | 0 | - |
| galion-desk | HEPHAESTUS | degrading | active | 200 | - | 4.9 MiB | - | 2 | 0 | degrading: 2 restart(s) in the last hour |
| tuner | DEMETER | healthy | active | 200 | - | 5.9 MiB | - | 0 | 0 | - |
| galion-one | ELEUTHIA | healthy | active | 200 | - | 6.8 MiB | - | 0 | 0 | - |
| galion-one-mouth | AETHER | degrading | - | 503 | - | - | - | 0 | 0 | degrading: health probe HTTP 503 (model loading) |
| console | HEPHAESTUS | healthy | active | 302 | - | 683.6 MiB | - | 0 | 0 | - |
| galion-health | DEMETER | healthy | active | 200 | - | 1000 KiB | - | 0 | 0 | - |
| cloudflared | AETHER | degrading | active | - | - | - | - | 0 | 0 | degrading: tunnel origin errors 30/40 requests in 1 min; degrading: 19 errors in 5 min (last: ) (4 edge conns, 81940 req, 48590 req errors) |
| tuner-quick | AETHER | healthy | active | - | - | - | - | 0 | 0 | (1 edge conns, 21 req, 0 req errors) |
| voice-worker | AETHER | absent | - | - | - | - | - | 0 | 0 | absent: ws/voice KittenTTS worker; not deployed yet |
| gaia | GAIA | absent | - | - | - | - | - | 0 | 0 | absent: ws/gaia `gaia status`; absent until the gaia CLI is installed |
| lxc-host | POSEIDON | healthy | - | - | - | - | - | 0 | 0 | (disk / 25% used (75 KiB free of 100 KiB), RAM 90 KiB available of 100 KiB, load1 0.5) |
| galion-nerve | AETHER | healthy | - | - | - | 2.9 MiB | - | 0 | 0 | - |

## Gaia functions

| function | status | components |
|---|---|---|
| GAIA | absent | gaia:absent |
| HADES | no-live-systems | (rollback: nerve rollback reflex via galion-deploy, 1 per unit per day) |
| MINERVA | no-live-systems | (alerting: nerve alerts to journald + alerts.json) |
| APOLLO | healthy | openmind-memory:healthy, link-watch-rs:healthy |
| HEPHAESTUS | degrading | galion-desk:degrading, console:healthy |
| ARTEMIS | healthy | hermes-rs:healthy |
| POSEIDON | healthy | lxc-host:healthy |
| AETHER | degrading | galion-one-mouth:degrading, cloudflared:degrading, tuner-quick:healthy, voice-worker:absent, galion-nerve:healthy (nerve: this hub (signals in, state out)) |
| DEMETER | healthy | tuner:healthy, galion-health:healthy (boot-units: nerve restart reflex) |
| ELEUTHIA | healthy | galion-one:healthy |

## Findings

- error `incident` ddos:tuner: inc-3051083e [critical] request flood on tuner (stream tuner) -- request flood: tuner: 20.0 req/s over the last 60 s vs baseline 0.10 req/s (threshold 10.0, x200); unknown (counter only), 0 IPs; 0% 4xx/5xx
- error `incident` edge: inc-0b21ec15 [error] edge: tunnel routes 2 public hostname(s) to 2 closed origin port(s) :3011 :4001: 4.0 failed requests/min over 25 min -- public hostnames answer with Cloudflare error pages; every failed request is a visitor or bot turned away, and the error counters hide real faults
- error `incident` galion-one: inc-f8ae4b8b [error] galion-one: galion-one-mouth degrading: health probe HTTP 503 -- stuck loading for 1.9 d [health body {\"status\":\"loading\",\"model\":\"chatterbox\",\"loaded\":false,\"ok\":false,\"provider\":\"local\"}] -- 1 signal(s) in group galion-one (galion-one:healthy, galion-one-mouth:degrading): one cause, one incident
- warn `incident` scan:console: inc-4f012246 [warn] scanner/exploit probes against console -- 3 scanner request(s): /.env, /phpinfo.php, /x/.git/config
- warn `incident` galion-desk: inc-1e7c712c [warn] galion-desk: degrading: 2 restart(s) in the last hour -- 1 signal(s) in group galion-desk (galion-desk:degrading): one cause, one incident
- error `vulnerability` host: port 9999 listens on 0.0.0.0 (not loopback) (LISTEN 0.0.0.0:9999)
- error `vulnerability` console: 3 scanner/exploit request(s) reached console (3 distinct paths) (/.env x1; /phpinfo.php x1; /x/.git/config x1; the unit prints Vite dev-server messages: a dev server is answering internet traffic)
- warn `vulnerability` host: 2 loopback listener(s) no monitored unit owns: :8652 :8653 (not a component health/metrics port and not in NERVE_KNOWN_PORTS; typical cause: manual shadow/test processes left running (no unit, no restart policy, no patching))
- warn `bleeding` cloudflared: tunnel routes 2 public hostname(s) to 2 closed origin port(s) :3011 :4001: 4.0 failed requests/min over 25 min
- warn `bleeding` galion-one-mouth: galion-one-mouth has answered 503 \"loading\" for 1.9 d (since 2026-10-18T22:35:04+02:00)
- warn `bleeding` cloudflared: cloudflared: degrading: 19 errors in 5 min (last: )
- warn `bleeding` galion-desk: galion-desk restarted 2 time(s) in the last hour
- info `bleeding` console: console holds 683.6 MiB
- error `ddos` tuner: tuner: 20.0 req/s over the last 60 s vs baseline 0.10 req/s (threshold 10.0, x200); unknown (counter only), 0 IPs; 0% 4xx/5xx
- info `component` galion-desk: degrading: degrading: 2 restart(s) in the last hour
- info `component` galion-one-mouth: degrading: degrading: health probe HTTP 503
- info `component` cloudflared: degrading: degrading: tunnel origin errors 30/40 requests in 1 min; degrading: 19 errors in 5 min (last: )
- info `component` voice-worker: absent: absent: ws/voice KittenTTS worker; not deployed yet
- info `component` gaia: absent: absent: ws/gaia `gaia status`; absent until the gaia CLI is installed
- info `push` galion-nerve: no push sources live; all components on pull probes
- info `alerts-channel` galion-nerve: journald + alerts.json; external: none configured (no Telegram/Hermes alert channel found on the host)
- info `mode` galion-nerve: reflex mode observe-only

## Reflex actions since last report

none
