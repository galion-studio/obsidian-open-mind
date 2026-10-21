# Galion OS — master plan
**PROJECT 42 / Gaia · admin interface**  
Written 2026-10-20 23:05 CEST (+02:00). One document. No build starts without Slavik’s yes on §11.

Galion OS is the **main admin interface**: a secure, private, anonymous ARM sandbox that lives **beside** Slavik’s normal Android apps and desktop — it does not replace them. It is the home of the HADES admin key, the phone link to Hermes, the Hermes Rust V2 redesign surface, and the banking / Wall Street / vending benchmarks (mocks and paper only).

> Discover the universe together with humans. Leave with more than you came in with. Rust only.

---

## 1. Design goals (first-class)

| Goal | Meaning in Galion OS |
|---|---|
| **Secure** | Hardened access control: HADES private key never leaves the device; admin bridge allowlists peers; every remote input is audited; sandbox default-deny network; root-hide so banking apps keep working. |
| **Private** | No data leakage: no secrets in git; no `adb pull` of keystore; gold-image personal blob encrypted on VPS only; agents never print tokens; Revolut/session material stays on-device. |
| **Anonymous** | No identifying metadata on the wire: admin tunnel strips device name/advertising IDs where possible; Hermes sees a signed **node id**, not a phone number or Google account; logs use Zagreb time and node ids, not personal names; public docs never carry IMEI/serial. |
| **Coexist** | Daily UI uninterrupted: cgroup CPU/RAM caps; sandbox off by default after reboot; no launcher replacement; no wakelock without an explicit toggle; thermal/jank tests before any “done”. |

These four beat convenience. If a feature needs to break one, it needs a written owner exception.

---

## 2. What was merged (sources) and how conflicts were resolved

| Source | Kept | Dropped / overridden |
|---|---|---|
| Galion OS sketch (t122u) | Coexisting ARM sandbox; root → terminal → gold image → Linux/Rust userspace; admin bridge | Full-ROM Linux phone (postmarketOS as daily driver) — **rejected** (breaks coexist) |
| Phone link (t118–t119) | Wireless debugging → VPS reverse tunnel → `adb`/`scrcpy` from Grok Bot cloud box (no USB) | “30 agents on the phone” — **rejected** (measured limit ~18–20) |
| Benchmarks (t121u, `plans/benchmarks.md`) | Banking / Wall Street / vending → 100% on **mocks/paper/sim**; Hermes tool sketches | Live money, live broker, live Revolut writes — **forbidden** until separate approval |
| Hermes Rust V2 (legacy Python → hermes-rs / hermes-core / desk-hermes) | Revolut **in scope** for redesign (ban lifted 22:56 CEST); tools talk to Galion OS mocks first | Revolut standing ban (old RULES §5) — **lifted for this redesign only**; “act on Slavik’s behalf” still banned |
| Swarm ceiling | Auto-accept on; queue + auto-restart; `swarm/settings-snapshot` | Spawning past ~18–20 — **rejected** |
| Gaia / cutover / console work | Continues in parallel; Galion OS does not block `integration-2` | Galion OS agents do not flash the VPS or merge `main` |

**Revolut (recorded):** Slavik typed (Mate relayed) that Revolut is allowed on the Hermes Rust V2 redesign. Recorded in RULES.md §5, `config/swarm/swarm-settings.toml` `[revolut]`, `docs/decisions/revolut-lift.md`. Still forbidden: any transfer, order, top-up, withdrawal, buy/sell.

---

## 3. Architecture

```
                    ┌─ secure · private · anonymous ─┐
                    ▼                                │
┌───────────────────────────────────────────────────┐│
│  Stock Android (MIUI / HyperOS) — daily life      ││
│  Chat, camera, Revolut UI, … never replaced       ││
├───────────────────────────────────────────────────┤│
│  Galion OS sandbox (on-demand, capped, audited)   ││
│                                                   ││
│  galion-term    galion-box (Linux aarch64 + Rust) ││
│       │              │                            ││
│       │         openmind-sandbox policies         ││
│       │         default-deny net · cgroup caps    ││
│       ▼              ▼                            ││
│  galion-admin ── admin bridge ── HADES keystore   ││
│   (Rust CLI)     (tunnel+adb)   (private stays)   ││
│                      │                            ││
│         benchmarks: bank mock · paper broker ·    ││
│                     vending sim (Hermes tools)    ││
└──────────────────────┼────────────────────────────┘│
                       │ reverse tunnel (VPS only)   │
                       ▼                             │
              VPS edge ── Hermes (boss) ── swarm     │
                       └── Grok Bot box (adb/scrcpy)─� (boss) ── swarm     │
                       └── Grok Bot box (adb/scrcpy)─┘
```

**Anonymous on the wire:** the bridge presents `node=<ed25519>` and a rotating session id. No IMEI, serial, Google id, or phone number in Hermes logs or nerve events.

---

## 4. Handset gate (must clear before any flash)

Mate named **Redmi 13 Pro** or **Note 9 Pro**. These are different trees (unlock, recovery, root).

| Candidate | Action |
|---|---|
| Redmi Note 13 Pro (4G/5G SKUs differ) | Confirm codename (`emerald` / `garnet` / …) |
| Redmi Note 9 Pro (`joyeuse`, SD720G) | Older Magisk path |
| “Redmi 13 Pro” without Note | Treat as third device until proven |

**Gate:** one About-phone photo, or `getprop ro.product.device` + `ro.product.model` + Android version. Wrong recovery = brick risk. Phase 1 does not start without this.

---

## 5. Layers

### L0 — Device trust
Bootloader unlock (Xiaomi; **wipes userdata** → backup first) → preferred **APatch / KernelSU**, Magisk+DenyList fallback → root-hide proven with Revolut still opening.

### L1 — Terminal + admin CLI
Rust `galion-admin` (aarch64-android / armv7 check): sandbox start/stop, tunnel, HADES public enroll, audit-log tail. `su` only for Galion paths.

### L2 — Gold image
After stable root: package list + modules + public flashable skeleton in git; **private** encrypted snapshot on VPS `<vps-path>` (never git). Used to build an emulator twin so agents test without touching the daily phone.

### L3 — Sandbox userspace (coexist)
**v1 = proot / chroot Debian aarch64** under a Galion app (least invasive). **v2 research** = crosvm/Firecracker only if the kernel allows — never a daily-driver Linux ROM. Import omnode / edge-boot, openmind-sandbox, hermes tool-jail patterns. Network default deny; allowlist = VPS tunnel.

### L4 — Admin bridge + Hermes Rust V2
1. Wireless debugging → VPS reverse tunnel → `adb` from Grok Bot.  
2. Screen steer (`scrcpy` now; Rust later), every injection audited.  
3. omnode pairing: phone = signed edge node.  
4. HADES keygen on device; public half to VPS (clears hermes-core G9).  
5. Hermes Rust V2 tools call **mocks first** (banking / paper / vending); live Revolut reads only under later approval; live writes need their own written approval.

---

## 6. Benchmarks (inside Galion OS)

Full scoring detail remains in `plans/benchmarks.md`; summary baked here.

| Bench | 100% means | Never |
|---|---|---|
| **Banking** | All axes green against a local Revolut-shaped mock; submit/confirm counters stay 0 | Live transfers |
| **Wall Street** | Paper broker + fixtures; kill switch blocks oversized orders | Live brokerage |
| **Vending** | Seeded ≥30-day sim; solvent; no silent inventory lies; deterministic seed | Real payments |

Scoreboard: `docs/bench/SCOREBOARD.md`. Hermes V2 tool sketches land next to each bench.

---

## 7. Phases

| Phase | Name | Owner present? | Agents | Exit criteria |
|---|---|---|---|---|
| **0** | Research & model confirm | No | 1–2 | `docs/galion-os/00-device.md` with one root path |
| **1** | Backup & unlock | **Yes** | 0 (checklist only) | Device daily-usable after wipe restore; banking apps login |
| **2** | Root + terminal + gold image | **Yes** | 2 | Root-hide ok; image checksummed on VPS; Croatian mini-checklist done |
| **3** | Sandbox v1 (proot + Rust) | Light | 2–3 | `cargo check` inside sandbox while Maps/Revolut used outside; jank/thermal pass |
| **4** | Admin bridge | **Yes** for first real-phone steer | 2 | Emulator demo first, then real phone; HADES public enrolled |
| **5** | Harden + anonymity review | No | 1–2 | Threat model doc; metadata audit; sandbox off-by-default |

Parallel (do not wait on Phase 0): Hermes V2 mock tools, benchmark crates, gold-image *format* design, tunnel runbook drafts.

---

## 8. Security / privacy / anonymity checklist

- [ ] HADES private key: device only; backup policy decided by Slavik  
- [ ] Admin tunnel: VPS allowlist, short-lived pairing codes, mutual auth  
- [ ] Audit log: every remote tap/key; retained; no secret values  
- [ ] Gold image: public skeleton ≠ personal blob  
- [ ] Nerve/Hermes events: node id only  
- [ ] No IMEI/serial/GAID in repo or CALL-RECORD  
- [ ] Revolut: scope ≠ authority to spend  
- [ ] Sandbox: seccomp + cgroup + default-deny net  

Full threat model and test list: [../security/threat-model.md](../security/threat-model.md) (2026-10-21).

---

## 9. Risks

| Risk | Severity | Mitigation |
|---|---|---|
| Wrong model / wrong recovery | Critical | Phase 0 gate |
| Unlock wipe loses 2FA / keys | Critical | Backup checklist; HADES after stable root |
| Root breaks banking | High | Root-hide; test every change; keep unrooted twin |
| Sandbox jank / heat | High | Caps; off by default; Phase 3 test |
| Key exfil via adb backup | Critical | Exclude keystore; never pull secrets |
| Always-on remote steer | High | No unsupervised scrcpy; owner present for real phone |
| “Revolut allowed” → money move | High | RULES + settings forbidden list |
| Agent stampede (>20) | High | Queue; auto-restart; this plan’s sizing |
| Anonymity leak via verbose logs | Medium | Metadata audit in Phase 5 |

---

## 10. Agent sizing & ceiling

- Measured concurrent limit: **~18–20** (request limit); configured ceiling 250 with auto-accept.  
- Galion OS steady budget: **2–3 agents** in Phases 0–4, burst to **5** only when headroom exists.  
- Does not steal slots from `integration-2`, cutover-prep, or HADES adversarial until those report.

---

## 11. Acceptance (Slavik)

1. Exact model from About phone?  
2. Accept unlock wipe after backup?  
3. APatch/KernelSU preferred, Magisk fallback?  
4. Sandbox off by default after reboot? (recommend **yes**)  
5. Private gold image on VPS `<vps-path>`?  
6. First steer on **emulator**, then real phone with you watching?  
7. Confirm secure / private / anonymous as non-negotiable for v1?  

**Build agents start only after answers to 1 and 7 (minimum).**

---

## 12. Pointers

| Doc | Role |
|---|---|
| `plans/benchmarks.md` | Full bench axes & workstreams |
| `plans/gaia.md` … `vps-ops.md` | Other sectors (unchanged) |
| `docs/decisions/revolut-lift.md` | Revolut ban lift record |
| `docs/SLAVIK-TODO.md` (`swarm/slavik-todo`) | HADES key, OpenRouter, HTTP strict |
| `swarm/hermes-core` `45b7980` | Coordinator; G9 needs admin key |
| `RULES.md` §5 | Money / Revolut / paid APIs |

---

*End of master plan. Beautiful work goes into the sandbox that stays out of the way — and into the admin face that stays quiet on the wire.*
