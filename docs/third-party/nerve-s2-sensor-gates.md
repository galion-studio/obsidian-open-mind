<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: nerve-s2-sensor-gates (slot 404)

No new dependency, no copied code, no dependency made optional. The seven
gated S2 sensors (sshd, conn-rate, proc-owner, surface, hades, event-feed,
edge-rate) are std-only; their features (`sensor-sshd`, `sensor-connrate`,
`sensor-proc-owner`, `sensor-surface`, `sensor-hades`, `sensor-event-feed`,
`sensor-edgerate`) pull nothing. Hermes / OpenRouter not used.
Cargo.lock unchanged. Sizes measured with A/B release builds only.
