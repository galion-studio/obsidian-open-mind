<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: bench-banking-adv (wave-4 slot 131)

No new third-party dependency and no copied code. `Cargo.lock` is unchanged against base 457ef74.
The normaliser, IBAN finder, look-alike skeleton, edit distance (optimal string alignment) and amount parser in
`crates/bench-banking/src/guard.rs` are written here, std-only.

The IBANs in the adversarial data are the published example IBANs from the IBAN registry / bank documentation
(GB82WEST12345698765432, NL91ABNA0417164300, FR1420041010050500013M02606, AT611904300234573201,
CH9300762011623852957, DE02120300000000202051, HR1723600001101234565); they are documentation/test examples (mod-97 valid), used here only as data; none is used against any real bank.
