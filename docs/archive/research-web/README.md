<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Archived research console prompt

The live web interface is galion-studio/hermes-research, for research.galion.app.

`console-prompt.html` is the prompt the Rust console room used to draw: a large field, a file plus (`ask-plus`), and a voice button (`ask-voice`). The form posted to `/hermes/attach`. The room no longer emits that markup. `POST /hermes/attach` and `POST /research/attach` still store a file on this machine and do not dial the research engine.
