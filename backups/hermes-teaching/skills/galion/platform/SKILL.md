---
name: platform
description: The Obsidian stack and Galion One. Use when the operator talks about the hot session, uniting One with Hermes, hardware cap, the community release, or the platform plans.
---

# Platform

The brief is on the pod. Read it before you change the hot path or One.

- `/workspace/galion-light/docs/platform/RECORD.md`
- `/workspace/galion-light/docs/platform/DESIGN.md`
- `/workspace/galion-light/docs/platform/IMPLEMENTATION.md`
- `/workspace/galion-light/docs/platform/DEVELOPMENT.md`

Follow `DEVELOPMENT.md` in order. Measure first. Write the numbers into `state/prefix.json` or `state/hardware.json`. A number you did not read is not a fact.

One agent, two sessions. The desk stays hot and small. Galion One keeps `one-voice`, its own model, its own voice setting. Do not merge the transcripts. Do not open a chat to stay warm. Do not curl health and call it work.

A hundred agents is the aim. The cap is the hardware file. Read `state/hardware.json`. If `gpu` is set, the Obsidian stack uses that card. If `gpu` is null, this machine is CPU: Hermes stays on the API model and does not start a local weight.

Public release waits for the operator.

## Loop

`python3 /workspace/galion-light/loop/loop.py run --n N --task "..." --budget B`

N is clamped to the cap in `state/hardware.json`. Workers are CPU processes. Stop with `loop.py stop`. An order is `loop.py order --sentence "..."`. It stores a goal, a deadline, a budget, and a stop. It does not promise the result. The measurement is `state/loop.json`. Do not load a model onto the GPU from this loop.


## Install

`bash /workspace/galion-light/install.sh`

CPU first. Galion One must answer on 8091. `weights` is true when a GPU is present and false on a CPU machine. The measurement is `state/install.json`.
Money and the lane are `/workspace/galion-light/state/survival.json`. Read it before a paid call.
The earn pipeline is `/workspace/galion-light/docs/platform/PIPELINE.md`. The loop does not promise money.
