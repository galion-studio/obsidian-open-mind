<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: hermes-rs-noloop-dropin (wave-4 slot 370)

No new dependencies and no copied code. The restart-delay model in
`crates/hermes-tools/tests/staged_dropin.rs` is written from the documented systemd
behaviour (systemd.service(5) RestartSteps=/RestartMaxDelaySec=, systemd.unit(5)
StartLimitIntervalSec=/StartLimitBurst=) and checked against measurements on the VPS;
no systemd (LGPL) source was copied.
