# Obsidian Open Mind: goals and living standards

This is a living standards file that sits alongside [main.md](main.md). **main.md is the base of everything**; if anything here conflicts with main.md, main.md wins.

> **Priority #1 (set 2026-10-18 19:01 CEST (+02:00)):** the GitHub repository and automatic publishing of the OpenMind stack after it self-evaluates (passes its own checks: fmt, clippy, tests, license/secret/no-Python/CJK/Zagreb-timestamp scans). **Deadline: October 31, 2026.** Creating the public repo and the first publish need the user's approval and a GitHub login on the build machine (not set up yet).

**Last updated:** 2026-10-18 19:01 CEST (+02:00)

## Standards

Follow the rules in main.md, in short:

- Rust-only, CPU-light foundation (extra compute is added externally, never by making the VPS heavy).
- MIT license for the Open Mind stack.
- All timestamps in Europe/Zagreb time with explicit offset (never server time; the VPS is UTC).
- No real-money access: connecting Revolut / giving Hermes real money is **NOT approved**.
- Milestone: publish everything live, fact-checked and timestamped by **October 31, 2026**.

## Goals

- **Priority #1:** GitHub repository and self-evaluating automatic publishing of the OpenMind stack (deadline October 31, 2026). See Priority #1 above.
- **Galleon Voice to Voice:** full-duplex, multi-persona voice front end for the stack. See *Project: Galleon Voice to Voice* below.
- **Galleon Arena:** arena of gladiators (personas, workers, teams); Hermes coding sessions on Opus 5.5 via OpenRouter. See *Project: Galleon Arena* below.
- **Universal engine:** Linux + Rust only; all agent code in the GitHub open-source library under Obsidian Open Mind Stack (MIT). See below.
- **Automatic recognition as fully open-source and secure:** OSI MIT, OpenSSF Scorecard / Best Practices, REUSE, SBOM, signed releases, SECURITY.md, CI secret scanning. See below.
- **Local network instances (plug-in interface):** add devices on the local Wi-Fi as compute providers. See below.
- **Survival Skills:** self-preservation and resilience (auto-recovery, self-healing services, attack detection, graceful degradation). See below.

### Universal engine

Based on **Linux and Rust architecture only**. All agent code lives in the GitHub open-source library under the **Obsidian Open Mind Stack** branding (MIT). No other OS or language foundations for the engine core.

**Status:** Proposed (as of 2026-10-18 19:01 CEST (+02:00))

- [ ] 2026-10-18 19:01 CEST (+02:00): Define the Universal engine crate layout and Linux/Rust-only boundaries under Obsidian Open Mind Stack (MIT).

### Automatic recognition as fully open-source and secure

Meaning: automatically recognized and verified as a fully open-source and secure project — e.g. OSI-approved MIT LICENSE in every repo, OpenSSF Scorecard / Best Practices badge, REUSE compliance, SBOM, signed releases, SECURITY.md, CI secret scanning.

**Status:** Proposed (as of 2026-10-18 19:01 CEST (+02:00))

- [ ] 2026-10-18 19:01 CEST (+02:00): Inventory every Open Mind repo for LICENSE, SECURITY.md, REUSE, SBOM, signed releases, OpenSSF Scorecard / Best Practices, and CI secret scanning.

### Local network instances (plug-in interface)

A better interface that lets the user add any device or compute instance on the current local Wi-Fi network (laptops, Macs, NVIDIA/AMD GPU boxes, phones) into the Obsidian Open Mind stack. Devices are discovered via mDNS/LAN scan and registered as an OpenMind Provider row (fits the foundation study: llama.cpp `rpc-server` for NVIDIA/AMD, [exo](https://github.com/exo-explore/exo) for Apple Silicon), and made reachable from the VPS via a tunnel. Must stay CPU-light, and **no device joins without explicit user approval**.

**Status:** Proposed (as of 2026-10-18 18:58 CEST (+02:00))

- [ ] 2026-10-18 18:58 CEST (+02:00): Design the LAN device discovery and approval interface (mDNS scan, register as OpenMind Provider, tunnel to VPS).

### Survival Skills

The self-preservation and resilience capabilities of the Obsidian Open Mind stack:

- Automatic recovery from crashes and failures.
- Self-healing services that restart on their own: a systemd unit with `Restart=` for every service (Galion One currently has none).
- Detecting attacks (DDoS, vulnerabilities, "bleeding" resources) and adapting in real time, tied to the nerve/cortex in main.md.
- Graceful degradation when compute or network is lost (e.g. fall back from GPU/laptop providers to CPU-light paths).
- Keeping the stack alive across reboots and outages.

An existing `galion/survival` skill was found among the imported VPS skills and should be reviewed.

**Status:** Proposed (as of 2026-10-18 18:59 CEST (+02:00))

- [ ] 2026-10-18 18:59 CEST (+02:00): Audit every service for auto-restart and reboot survival, and review the existing galion/survival skill.

## Project: Galleon Arena

Galleon Arena is an arena of gladiators where the gladiators are the personas, workers, and teams inside the system. Hermes is the base of the coding sessions, running on `anthropic/claude-opus-5.5` through OpenRouter.

**Honest note (checked 2026-10-18 19:01 CEST (+02:00)):** Hermes's OpenRouter connection is currently broken — the OpenRouter API key is not present in `/opt/galion/hermes/.env`. Fix is pending approval.

**Starting point on the box:** existing test agent homes `/opt/galion/arena/homes/g2-0` .. `g2-3` already exist, each with `SOUL.md` + `TASK.md`.

**Status:** Proposed (as of 2026-10-18 19:01 CEST (+02:00))

### To-do

- [ ] 2026-10-18 19:01 CEST (+02:00): Fix Hermes OpenRouter on anthropic/claude-opus-5.5 (key missing from /opt/galion/hermes/.env); requires approval.
- [ ] 2026-10-18 19:01 CEST (+02:00): Define gladiator roles from personas, workers, and teams for Galleon Arena.
- [ ] 2026-10-18 19:01 CEST (+02:00): Design arena rules and scoring linked to the vending machine benchmark.
- [ ] 2026-10-18 19:01 CEST (+02:00): Reuse existing arena/homes g2-0..3 (SOUL.md + TASK.md) as the starting point for Galleon Arena.

## Project: Galleon Voice to Voice

Galleon Voice to Voice is the planned multi-persona, multiplexed voice/persona system for the stack, based on the Persona Plex concept from the Moshi architecture (Kyutai). Goal: bring full-duplex voice with switchable voices and personas into the stack, most likely via the Galion One voice front end and Hermes personas.

**Verified facts (checked 2026-10-18):**

- The underlying model is NVIDIA's **PersonaPlex** (`nvidia/personaplex-7b-v1`, released 2026-01-15): a 7B real-time, full-duplex speech-to-speech model built on Kyutai's Moshi architecture, fine-tuned from Moshi (Moshiko) weights. Persona control comes from a text role prompt plus an audio voice prompt (18 pre-packaged voices). English only.
- A small clarification, for accuracy: it is an NVIDIA model built *on* Moshi, not a component Kyutai ships inside Moshi. It runs one persona per session (set by prompt), rather than several personas at once; "multiplexed" fits in the full-duplex sense (it listens and speaks at the same time).
- **Licenses:** code is MIT; weights are under the NVIDIA Open Model License (commercial use and derivatives allowed; gated on Hugging Face, requires attribution notice on redistribution, Trustworthy AI terms, revocable if guardrails are bypassed). Base Moshiko weights are CC-BY-4.0.
- **License fit vs MIT:** our integration code can stay MIT; the weights are *not* MIT and must not be redistributed as part of the stack. Users fetch them themselves after accepting NVIDIA's license.
- **Hardware:** PyTorch on Linux, NVIDIA GPU (supported: Ampere A100, Hopper H100; tested on A100 80 GB). A `--cpu-offload` flag and CPU-only offline evaluation exist, but they are not suited to real-time use.
- **Placement:** heavy inference must run on an external GPU provider (RunPod or similar), never on the CPU-light VPS. The VPS only relays and orchestrates.
- Sources: [repo](https://github.com/NVIDIA/personaplex), [paper (arXiv 2602.06053)](https://arxiv.org/abs/2602.06053), [model card](https://huggingface.co/nvidia/personaplex-7b-v1), [demo](https://research.nvidia.com/labs/adlr/personaplex/), [NVIDIA Open Model License](https://www.nvidia.com/en-us/agreements/enterprise-software/nvidia-open-model-license/), [Moshi](https://github.com/kyutai-labs/moshi).

**Status:** Proposed, evaluation not started (as of 2026-10-18 18:58 CEST (+02:00))

### To-do

- [ ] 2026-10-18 18:58 CEST (+02:00): Verify the Galleon Voice to Voice license fit (MIT code vs NVIDIA Open Model License weights) and GPU needs.
- [ ] 2026-10-18 18:58 CEST (+02:00): Choose a GPU provider path for Galleon Voice to Voice (RunPod/external, not the VPS) and estimate cost.
- [ ] 2026-10-18 18:58 CEST (+02:00): Design the integration of Galleon Voice to Voice with Galion One (voice front end) and Hermes personas.
- [ ] 2026-10-18 18:58 CEST (+02:00): Build a minimal Galleon Voice to Voice prototype on the external GPU, relayed through the VPS.
- [ ] 2026-10-18 18:58 CEST (+02:00): Fact-check and timestamp all Galleon Voice to Voice claims before the Oct 31, 2026 publish.

## To-do

- [ ] 2026-10-18 19:01 CEST (+02:00): Priority #1: set up the GitHub repository and self-evaluating auto-publish pipeline (deadline 2026-10-31).
- [ ] 2026-10-18 19:01 CEST (+02:00): Galleon Arena: fix Hermes OpenRouter, define gladiator roles, design rules/scoring, reuse arena/homes (see project to-do).
- [ ] 2026-10-18 19:01 CEST (+02:00): Universal engine: define Linux/Rust-only crate layout under Obsidian Open Mind Stack MIT (see goal above).
- [ ] 2026-10-18 19:01 CEST (+02:00): Automatic recognition as fully open-source and secure: inventory LICENSE, SECURITY.md, REUSE, SBOM, signed releases, OpenSSF, CI secret scanning (see goal above).
- [ ] 2026-10-18 18:58 CEST (+02:00): Evaluate Galleon Voice to Voice for Galion One: verify license, GPU needs, and a minimal integration path (see project to-do).
- [ ] 2026-10-18 18:58 CEST (+02:00): Local network instances: design the LAN device discovery and approval interface (see goal above).
- [ ] 2026-10-18 18:59 CEST (+02:00): Survival Skills: audit every service for auto-restart and reboot survival, and review the galion/survival skill (see goal above).
