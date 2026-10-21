<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Why galion-one's mouth (127.0.0.1:8002) says 503 "loading" for 2.4+ days, and fix options

Written 2026-10-21 09:05 CEST (+02:00) by swarm slot 181 (`swarm/mouth-loading-diag`, base integration-4 42de672).
**The VPS was only read**: `systemctl show/cat`, `ss -ltnp`, `journalctl -u galion-one`, `grep -c`/`tail` of
`/var/log/galion-one.log`, `ls`/`du`/`find` for model files, HTTP GETs on loopback, nerve `GET :8670/state`, and one
`scp` copy of the live binary *to* the box for a local test. Nothing on the VPS was changed, restarted or written
(the copy was a read). No secret values were read; for the unit, only environment variable **names** were listed.
Builds on slot 06's `docs/voice-mouth-503.md` (2026-10-20 19:25 +02:00) and updates it for the binary that has been
live since 2026-10-21 08:39 +02:00.

## 1. Short answer
The mouth is **not loading anything and never will.** In every galion-one binary since B14 (2026-10-19), the
`:8002` listener is backed by `speech::local()`, which returns the `Unavailable` stub (`ready()` is always false).
In the live code line, `/health` answers that state with a **hard-coded** body that says `"status":"loading"`,
`"model":"chatterbox"`. So the "loading" 503 is a constant string, not a warm-up. No model path is read, nothing is
loaded, and nothing can fail to load. That is why the journal shows no load errors. Before B14 the Python
Chatterbox worker *was* spawned and crashed every time (`No module named 'torch'`, 7,780 times). The 08:39 deploy
of 40454f9 kept the same constant, so the 503 continued unchanged after it.

## 2. Evidence (VPS, read-only, 2026-10-21 08:56-09:00 +02:00)
| What | Observed |
|---|---|
| Unit | `galion-one.service` active, MainPID 29782, NRestarts 0, started 08:39:24 +02:00; drop-in `50-rust-bin.conf` -> `ExecStart=/opt/galion/rust/bin/galion-one` (sha256 prefix `24cb4976`, = swarm/one-no-shellout 40454f9); `MemoryMax=128M`; stdout+stderr appended to `/var/log/galion-one.log` |
| Env var **names** (unit and /proc, values not read) | `GALION_HOME`, `ONE_BIND`, `ONE_MOUTH_BIND`, `ONE_DEVICE_STORE`, `ONE_HERMES_DIR`, `ONE_HERMES_ENV` (unit also names `OPENROUTER_KEY_FILE`; it is not in the process environment). **No `GALION_MOUTH`, no `GALION_EAR`, no `ONE_NO_WARM`.** |
| Listeners | 8091 and 8002 both galion-one pid 29782 (fd 4 / fd 3). Nothing on 8003 (the planned galion-voice-serve) or 8643 (ear) |
| `GET :8002/health` | **503** `{"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}` in 0.8 ms; same body at 08:56:53 and 09:00:33 |
| `GET :8091/health` | 200 `ok` |
| journalctl -u galion-one since 2026-10-18 | only systemd lines: start 2026-10-18 22:13:56 +02:00, stop/start 2026-10-21 08:39:24 +02:00 (old process "Consumed 1h 3min 9.7s CPU, 17.1M memory peak"). No app lines (they go to the log file) |
| `/var/log/galion-one.log` (1.97 MB, 50,873 lines at 08:56) | 7,780 Python tracebacks `tts/worker.py line 16: import torch -> ModuleNotFoundError: No module named 'torch'` and 7,780 `voice: failed to fill whole buffer`, last one at line 40,365. These all come from binaries before B14 (path `/workspace/galion-light/tts/worker.py`). 5 `voice: listening` starts, the last one at line 50,821 (08:39 deploy). Since then only `warm rust skipped mouth=0 mind=402` (10,602 -> 10,613 between 08:56 and 09:00, one every ~20 s) |
| Model files | `/var/lib/galion/voice-models` (the galion-voice-serve default) **does not exist**. Chatterbox weights **are present**: `/opt/galion/tts/models/models--ResembleAI--chatterbox` snapshot 5bb1f6ee (`ve`, `s3gen`, `t3_cfg` .safetensors, `conds.pt`, tokenizer), 3.0 GB, dated 2026-09-23. Python venv `/opt/galion/tts/venv` (277 MB) has `chatterbox_tts 0.1.7`, `transformers 5.2.0`, `librosa 0.11.0`, but **no torch**. `worker.py` sets `HF_HOME=/workspace/tts/models` (the old path, not `/opt/galion/tts/models`) and loads `cuda` if available, else CPU |
| GPU | none (`nvidia-smi` absent, no `/dev/nvidia*`) |
| Memory / CPU | galion-one RSS 680 kB (VmHWM 2.4 MB), 4 threads, 0.4 % CPU, cgroup MemoryCurrent ~0.9 MB of 128 MB. Host: AMD Ryzen 7 3800X, 8 vCPU, 16 GB, 14.7 GB available, no swap, `/` 26 G of 100 G. Load average ~11 is container-wide (no galion process is hot: top is galion-one at 0.4 %). **No resource problem.** |
| Nerve (`:8670/state`, mode live, cycles 7004 -> 7011) | `galion-one-mouth`: `degrading`, reason `health probe HTTP 503`, `status_since 2026-10-18T22:35:04+02:00`. Its note says "503 while the model loads", which is wrong. `voice-worker` (unit `galion-voice.service`): `absent`, "ws/voice KittenTTS worker; not deployed yet". `galion-one`: degrading, "1 restart(s) in the last hour" (the planned 08:39 deploy). Cortex: not live (8680/8690 no answer), so the nerve was used |

Duration: degrading since 2026-10-18 22:35:04 +02:00 (process start 22:13:56) -> 2.4+ days at 09:00 on 2026-10-21.
Before that the same 503 came from the crashing Python worker.

## 3. Root cause, ranked
1. **No speech backend exists on the VPS.** B14 removed the Python Chatterbox spawn; `speech::local()` is the
   `Unavailable` stub; no `GALION_MOUTH` is set; nothing listens on 8003; no Rust voice models are installed.
   Chatterbox never worked here either (no torch, no GPU, and a stale `HF_HOME` path).
2. **`/health` lies.** The live code line (one-probe b44cd4f -> one-hardening fed3de4 -> one-no-shellout 40454f9)
   still has the hard-coded `"loading"/"chatterbox"` body in `tts_http`. So everyone (nerve, people reading probes)
   reads "wait, it is loading" instead of "there is no backend".
3. **A remote mouth alone would not clear it on the live binary.** Shown on the box with the live binary
   (case L2 below): with `GALION_MOUTH` pointing at a working mouth, `POST /synthesize` returns 200 audio/wav, but
   `/health` stays 503 "loading", because it only asks the local stub.
4. **The honest `/health` fix exists but is on a different line.** `swarm/voice-mouth` 5d776c1 (merged in
   integration-4 42de672 and integration-5 c16639f) answers 503 `"unavailable"` + reason, passes a remote
   `/health` through, or says `"unreachable"`. **No branch has both** that fix and the live 40454f9 changes.
   `git merge-tree 42de672 40454f9` has conflicts only in `Cargo.lock` and `crates/galion-one/Cargo.toml`
   (integration-4 adds the `galion-common` dependency); `main.rs`, `cli.rs` and `models_refresh.rs` auto-merge.
5. Side issue, not the 503: every ~20 s the keep-warm loop pings the mind (Hermes 8642) and gets **402** (payment
   required): `mind=402`. Even with a mouth, a full voice turn would fail at the mind step until the credit
   question is settled. Hermes's paid model was **not** touched.

## 4. Real-test proof on the box (Rust, `crates/galion-one/tests/real_mouth_loading.rs`)
See `docs/real-tests/mouth-loading-diag.md`: 3 runs, all pass. Branch binary: A no mouth -> 503 "unavailable"
(constant over 6 s), B remote up -> 200 passthrough + WAV, C remote down -> 503 "unreachable". Live binary copy
(sha256 `24cb4976`): L1 no mouth -> 503 "loading" constant over 6 s; L2 remote up -> **/health still 503 "loading"**
while /synthesize returns 200 audio/wav.

## 5. Fix options for Slavik (none done; each needs your OK)
| # | Option | What changes on the VPS | Result | Cost / risk |
|---|---|---|---|---|
| **1** | **Make the next galion-one deploy carry the honest `/health`.** A merge branch = one-no-shellout 40454f9 + integration-4/5 (resolve 2 Cargo files), tests green, then the normal drop-in install like t179 | 1 binary swap + 1 restart of galion-one (~1 s) | 8002 says 503 `"unavailable"` + "set GALION_MOUTH ..." instead of "loading". Truthful, but the nerve row stays `degrading` | Low. Same pattern as the 08:39 deploy; rollback = previous binary (backup in /var/backups/galion-one-20261021) |
| **1b** | With 1: change the nerve probe so `galion-one-mouth` treats 503 `"unavailable"` as `absent` (like `voice-worker`) and fix its note "503 while the model loads" | nerve config/binary update (separate change) | The nerve stops flagging a known, expected gap; real regressions (8002 down, 5xx other than unavailable) still flagged | Low. Needs a nerve change and its own deploy |
| **2** | **Real CPU voice, Rust only**: `galion-voice-serve` (crate galion-voice-cpu, features `whisper,kitten`) as new `galion-voice.service` on 127.0.0.1:8003 + ~215 MB models in `/var/lib/galion/voice-models` + drop-in `GALION_MOUTH=127.0.0.1:8003` (and `GALION_EAR`) on galion-one. **Needs option 1 first** (proved by L2: the live binary would stay 503 even with a working mouth) | new unit + models + galion-one drop-in + restart | 8002 /health 200 with the voice service's JSON; nerve `voice-worker` and `galion-one-mouth` healthy; `/synthesize` returns real speech (slot 06 measured TTS 3.1 s for 5.8 s audio, peak RSS ~450 MB on the box) | Medium. Needs decision A from docs/voice-mouth-503.md: the **SIMD exception** for candle's runtime-dispatched gemm (RULES say no SIMD). Must stay a separate service: galion-one's `MemoryMax=128M` cannot hold the model. Voice turns still fail at the mind step while Hermes answers 402 |
| 3 | Revive Chatterbox (Python): install torch into `/opt/galion/tts/venv`, fix HF_HOME, restore a worker | pip install of torch (~GBs), Python service | Maybe slow CPU speech | **Not recommended**: breaks the Rust-only rule, no GPU, and B14 removed the spawn on purpose |
| 4 | Accept and document: leave 8002 as is | nothing | Status quo: the nerve keeps flagging a "loading" mouth forever | Zero change, but a misleading signal stays live |
| 5 | Housekeeping (any time, separate OK): the Chatterbox weights (3.0 GB, `/opt/galion/tts/models`) and the torch-less venv (277 MB) are dead weight while option 3 is rejected. Archive or delete | delete/move files | +3.3 GB free on `/` | Irreversible if deleted; archive first |
| 6 | Side issue (402 noise): set `ONE_NO_WARM=1` on galion-one until the Hermes credit question is settled, **or** leave as is. Do **not** turn on the paid model to fix it | 1 env line + restart (can ride with option 1) | Stops ~4,300 `mind=402` pings/day to Hermes and the log growth (~1 line per 20 s) | Low. Loses the keep-warm, which does nothing useful while the answer is 402 |

**Recommendation:** 1 + 1b now (cheap, truthful, removes a false "degrading"); 2 once you decide the SIMD
exception; 5 and 6 as you like. The deploy agent should build the merge of 40454f9 with integration-4/5, not
one-no-shellout alone, so the honest health check is not lost again.

## 6. Commands used (read-only, for repeat)
```
systemctl show galion-one -p ActiveState,SubState,MainPID,NRestarts,ExecMainStartTimestamp,MemoryCurrent,CPUUsageNSec,DropInPaths
systemctl cat galion-one | grep -E '^Environment=' | ... | sed 's/=.*//'     # names only
tr '\0' '\n' < /proc/<pid>/environ | sed 's/=.*//'                             # names only
ss -ltnp | grep -E ':(8002|8003|8091|8643|8642)\b'
curl -s -w '%{http_code}' http://127.0.0.1:8002/health
journalctl -u galion-one --since 2026-10-18 --no-pager -o short-iso
grep -c "No module named 'torch'" /var/log/galion-one.log ; tail /var/log/galion-one.log
du -sh /opt/galion/tts/models ; ls /opt/galion/tts/venv/lib/python3.11/site-packages | grep -i torch
curl -s http://127.0.0.1:8670/state
```
