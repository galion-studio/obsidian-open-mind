<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: nerve-xt-pin-refresh (slot 376)

No new dependency and no copied code. The live-pin rebuild uses `git archive`
and `tar` (already on the box) through `std::process::Command`; SHA-256 stays
the in-test FIPS 180-4 implementation from slot 217. Cargo.lock unchanged.
