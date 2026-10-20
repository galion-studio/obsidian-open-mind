// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! NIST SHA-256 example vectors (FIPS 180-2 Appendix B and the NIST CSRC
//! "SHA256.pdf" / "SHA2_Additional.pdf" examples).

use galion_common::sha256::{self, Sha256};

#[test]
fn fips_180_2_appendix_b() {
    // B.1 one block, B.2 two blocks, B.3 one million 'a'.
    assert_eq!(
        sha256::hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256::hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    let mut s = Sha256::new();
    let block = [b'a'; 1000];
    for _ in 0..1000 {
        s.update(&block);
    }
    assert_eq!(
        galion_common::hex::encode(&s.finalize()),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
}

#[test]
fn nist_additional_vectors() {
    assert_eq!(
        sha256::hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256::hex(&[0xbd]),
        "68325720aabd7c82f30f554b313d0570c95accbb7dc4b5aae11204c08ffe732b"
    );
    assert_eq!(
        sha256::hex(&[0xc9, 0x8c, 0x8e, 0x55]),
        "7abc22c0ae5af26ce93dbb94433a0e0b2e119d014f8e7f65bd56c61ccccd9504"
    );
    assert_eq!(
        sha256::hex(&[0u8; 55]),
        "02779466cdec163811d078815c633f21901413081449002f24aa3e80f0b88ef7"
    );
    assert_eq!(
        sha256::hex(&[0u8; 56]),
        "d4817aa5497628e7c77e6b606107042bbba3130888c5f47a375e6179be789fbb"
    );
    assert_eq!(
        sha256::hex(&[0u8; 57]),
        "65a16cb7861335d5ace3c60718b5052e44660726da4cd13bb745381b235a1785"
    );
    assert_eq!(
        sha256::hex(&[0u8; 64]),
        "f5a5fd42d16a20302798ef6ed309979b43003d2320d9f0e8ea9831a92759fb4b"
    );
    assert_eq!(
        sha256::hex(&[0u8; 1000]),
        "541b3e9daa09b20bf85fa273e5cbd3e80185aa4ec298e765db87742b70138a53"
    );
    assert_eq!(
        sha256::hex(&[0x41u8; 1000]),
        "c2e686823489ced2017f6059b8b239318b6364f6dcd835d0a519105a1eadd6e4"
    );
    assert_eq!(
        sha256::hex(&[0x55u8; 1005]),
        "f4d62ddec0f3dd90ea1380fa16a5ff8dc4c54b21740650f24afc4120903552b0"
    );
}

#[test]
fn old_copies_agree() {
    // The 1000 x 'a' vector the galion-one copy carried.
    assert_eq!(
        sha256::hex(&[b'a'; 1000]),
        "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
    );
}
