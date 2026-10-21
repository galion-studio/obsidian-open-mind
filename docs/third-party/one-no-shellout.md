<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party record: swarm/one-no-shellout (wave 4 slot 148)

Written 2026-10-21 (Europe/Zagreb, +02:00).

## New runtime dependencies: none

`galion-one` still has zero crates.io dependencies (`cargo tree -p
galion-one` = the crate alone). Everything new is std-only Rust written for
this branch from public specifications, not ported from other code:

| file | written from | checked against (test-only oracle, never shipped) |
|---|---|---|
| `src/inflate.rs` | RFC 1950 (zlib), RFC 1951 (DEFLATE) | `gzip` 1.13 on the box: 60 inputs x levels -1/-9 = 120 streams decoded byte-for-byte; 4 MB zero bomb stops at the 64 KB cap; 4000 garbage inputs |
| `src/pdf_crypt.rs` MD5 | RFC 1321 (incl. its appendix A.5 test suite) | the RFC vectors + `md5sum` (coreutils) on generated inputs |
| `src/pdf_crypt.rs` RC4 | the published RC4 algorithm (Schneier, "Applied Cryptography") | the classic Key/Plaintext, Wiki/pedia, Secret/Attack at dawn vectors |
| `src/pdf_crypt.rs` key derivation | ISO 32000-1:2008 §7.6.3.3 algorithms 2, 4, 5 (Standard security handler, rev 2/3) | ghostscript-encrypted fixtures |
| `src/pdf_text.rs` | ISO 32000-1:2008 (syntax §7, filters §7.4, object streams §7.5.7, text §9, fonts/CMaps §9.6-9.10, annotations §12.5, forms §12.7); Adobe glyph list names for `/Differences` (only the Latin names we map) | real `pdftotext` (poppler 25.03.0 on the box) on 44 PDFs, see docs/real-tests/one-no-shellout.md |
| `src/http_plain.rs` | RFC 9112 (HTTP/1.1 message syntax, chunked coding) | std `TcpListener` stand-ins in the unit and real tests |

RC4 and MD5 are broken as general cryptography. Here they only *read*
"protected" PDFs whose key is derived from the empty user password, which is
exactly what PDF viewers do; nothing is protected by them.

## Test fixtures

`crates/galion-one/tests/fixtures/pdf/`: five PDFs made by ghostscript
10.05.1 (`pdfwrite`) from our own `fixture.ps`, with
`-dEmbedAllFonts=false`, so no font program (no third-party font data) is in
them. The output of a GPL program is not covered by its licence; the files
are ours (MIT). Recipe in the fixtures README. Chrome / LibreOffice outputs
were tried and **not** committed, because they embed font subsets (OFL /
other licences).

The 39-file comparison corpus (pdf.js, pypdf, Apache PDFBox sample files,
arXiv 1706.03762, IRS fw9, etc.) was downloaded to the box only
(`GALION_PDF_CORPUS=<dir>` points the real test at it) and is **not**
committed: mixed and partly unknown licences.

## Candidates evaluated and rejected

| need | candidate | licence | why not |
|---|---|---|---|
| PDF text | lopdf 0.45.0 | MIT | hard deps on `aes`, `cbc`, `ecb`, `md-5`, `sha2`, `rand`, `getrandom`...: RustCrypto runtime-dispatches to AES-NI/SHA-NI (see one-hardening record) |
| PDF text | pdf-extract 0.12.1 | MIT | built on lopdf, plus ~20 more crates |
| HTTPS | rustls 0.23 + ring / aws-lc-rs | Apache/ISC/MIT, OpenSSL-ish for aws-lc | assembly / C + assembly crypto |
| HTTPS | rustls 0.23.46 + rustls-rustcrypto 0.0.2-alpha + webpki-roots 1.0.10 | MIT/Apache-2.0/BSD-3/ISC/Unicode-3.0; **webpki-roots = CDLA-Permissive-2.0** | works (spike below) but: alpha + unaudited provider (last release 2024-04-24), ~60 linked crates (106 in Cargo.lock incl. ring 0.17.14 and two rustls-webpki versions 0.102.8 + 0.103.16), needs 5 cfg flags + `sha2/force-soft` to drop AES-NI/SHA-NI/AVX, 2.56 MB binary, CDLA not on our list (use `/etc/ssl/certs` instead) |

## TLS spike (box only, not committed as a crate)

`Cargo.toml`:

```toml
[package]
name = "tls-spike"
version = "0.0.0"
edition = "2021"
publish = false

[workspace]

[dependencies]
rustls = { version = "0.23", default-features = false, features = ["std", "tls12"] }
rustls-rustcrypto = "0.0.2-alpha"
webpki-roots = "1"
sha2 = { version = "0.10", features = ["force-soft"] }
```

`src/main.rs`:

```rust
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(rustls_rustcrypto::provider()))
        .with_safe_default_protocol_versions().unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let cfg = Arc::new(cfg);
    for host in std::env::args().skip(1) {
        let t = Instant::now();
        let r = (|| -> Result<String, String> {
            let name = host.clone().try_into().map_err(|e| format!("{e:?}"))?;
            let mut conn = rustls::ClientConnection::new(cfg.clone(), name).map_err(|e| e.to_string())?;
            let mut sock = TcpStream::connect((host.as_str(), 443)).map_err(|e| e.to_string())?;
            sock.set_read_timeout(Some(Duration::from_secs(3))).ok();
            let mut tls = rustls::Stream::new(&mut conn, &mut sock);
            write!(tls, "GET / HTTP/1.1\r\nHost: {host}\r\nUser-Agent: galion-nerve\r\nConnection: close\r\n\r\n").map_err(|e| e.to_string())?;
            let mut buf = [0u8; 64];
            let n = tls.read(&mut buf).map_err(|e| e.to_string())?;
            let line = String::from_utf8_lossy(&buf[..n]).into_owned();
            let suite = conn.negotiated_cipher_suite().map(|s| format!("{:?}", s.suite())).unwrap_or_default();
            Ok(format!("{} {}", line.split_whitespace().nth(1).unwrap_or("?"), suite))
        })();
        println!("{host} {:?} {}ms", r, t.elapsed().as_millis());
    }
}
```

Build without SIMD crypto (this full set was tested; we did not test which subsets suffice):

```sh
RUSTFLAGS='--cfg aes_force_soft --cfg polyval_force_soft --cfg chacha20_force_soft --cfg poly1305_force_soft --cfg curve25519_dalek_backend="serial"' \
  cargo build --release   # plus sha2 feature "force-soft" in Cargo.toml
objdump -d target/release/tls-spike | grep -cE 'aesenc|sha256rnds2|pclmulqdq|%ymm|%zmm'
# default build: 408 aesenc, 32 sha256rnds2, 3427 ymm; soft build: 0 / 0 / 0
```

Result per host: docs/one-no-shellout.md (same HTTP status as curl 5/5).
