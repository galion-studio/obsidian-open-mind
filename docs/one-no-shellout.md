<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# galion-one without shell-outs: what is possible (swarm/one-no-shellout)

Wave 4 slot 148, 2026-10-21 (CEST +02:00). Base: swarm/one-hardening
fed3de4. Research + prototype; nothing deployed, nothing on the VPS changed.

## Bottom line

| shell-out (fed3de4) | where | status on this branch |
|---|---|---|
| `pdftotext` (poppler) | `pdf_clip` -> "They included a file" note | **Replaced by default.** Pure-Rust, std-only, zero-crate reader (`src/pdf_text.rs`, `src/inflate.rs`, `src/pdf_crypt.rs`). 98.38 % word recall vs poppler on 44 real PDFs, 0-55 ms each. `pdftotext` stays only as an opt-in fallback. |
| `curl` for the OpenRouter model list | `models_refresh.rs` | **Replaced for loopback `http://`** (`src/http_plain.rs`, std only): local proxy/stand-in needs no curl. The default `https://openrouter.ai` still uses curl. |
| `curl` for the 5 HTTPS edge probes | `main.rs load_edge` | **Kept.** Pure-Rust HTTPS is possible (spike below: same codes as curl 5/5) but costs ~60 crates, an alpha unaudited crypto provider, 5 cfg flags and one non-allowed licence. Owner decision D2. |
| `nvidia-smi` | GPU sample | Kept (NVML is a closed C library); out of scope, already optional. |

## 1. PDF text in pure Rust

`galion-one` decides per `ONE_PDF_TEXT`:

| `ONE_PDF_TEXT` | behaviour |
|---|---|
| unset / `rust` (default) | built-in reader only, no subprocess at all |
| `rust+pdftotext` / `fallback` | Rust first; `pdftotext` only if Rust found no text (password-protected, AES, CID font without ToUnicode, scanned page) |
| `pdftotext` | old behaviour (subprocess only) |
| `off` / `0` / `none` / `false` | no PDF text |
| anything else | treated as `rust` (nothing gets executed) |

`ONE_PDFTOTEXT` keeps naming the poppler binary (and `off` there disables
just the binary). New operator command: `galion-one pdf-text <file.pdf>
[--content]` prints the first-page text (or the decoded content stream).

What the reader covers (all written from the public specs, see
docs/third-party/one-no-shellout.md):

* objects found by scanning `N G obj` (broken/missing xref still works;
  later copies win = incremental updates), `/ObjStm` object streams, all
  `trailer`s and `/XRef` stream dictionaries;
* filters: FlateDecode (own RFC 1950/1951 inflate, checked byte-for-byte
  against gzip(1) on 240 streams), LZWDecode (PDF spec example vector),
  ASCIIHex, ASCII85, RunLength; chains of them;
* first leaf of the page tree with inherited `/Resources`, `/Contents`
  arrays, Form XObjects (`Do`, with matrices), visible annotation
  appearances and empty-appearance form field values (`/V`);
* fonts: `/ToUnicode` CMaps (bfchar, bfrange incl. arrays, codespaces),
  simple-font encodings (WinAnsi, Standard, `/Differences` with glyph names,
  `uniXXXX`), glyph widths (`/Widths`, CID `/W` `/DW`) for word gaps and
  line breaks from the text matrix and CTM;
* encryption: Standard security handler RC4 rev 2 and 3 with the empty user
  password ("protected" PDFs that only restrict printing/copying), own
  MD5 (RFC 1321 vectors + md5sum cross-check) and RC4 (published vectors).
  AES (`/V 4`, `/V 5`) and real user passwords -> "encrypted", left to the
  optional fallback.

Not covered (honest list): AES-encrypted PDFs, CID fonts with no ToUnicode,
reading order by position (we emit content-stream order; for the
480-character clip that is what matters), right-to-left visual order
(we keep logical order, which is arguably better), OCR.

Safety on hostile uploads: no `unsafe`, every index checked, recursion depth
capped (48 object levels, 8 form levels, 64 page-tree levels, cycle sets),
decoded bytes capped (16 MB per stream, 32 MB total), 2 M content tokens,
4000 output characters, plus a 5 s worker-thread fence in `host.rs`. Tested
with 7 500 random mutations of the 4 fixture PDFs and a synthetic
Type0/form/CMap PDF (no panic) and 200 000-deep `[` and `<<` nesting.

Cost: x86_64 musl static binary 841 024 -> 963 904 bytes (+120 KB). Still
zero crates.io dependencies. `objdump`: 0 AES-NI / SHA-NI / PCLMUL / AVX
(ymm/zmm) instructions.

## 2. curl

### Done: loopback plain HTTP in-process

`OPENROUTER_BASE_URL=http://127.0.0.1:<port>/api/v1` (a local egress proxy,
cache or test stand-in) is now fetched with std (`http_plain.rs`:
Content-Length, chunked, read-to-close, 8 MB cap, 20 s). The bearer key is
put in a header in-process (never argv). **Non-loopback `http://` is
refused before connecting**, so the key can never cross a network in clear
text. `https://` keeps the curl path unchanged.

### Researched: pure-Rust HTTPS (not merged into galion-one)

Spike (box only, not committed; source in docs/third-party/one-no-shellout.md):
rustls 0.23.46 + rustls-rustcrypto 0.0.2-alpha + webpki-roots 1.0.10,
soft-crypto release build, run 2026-10-21 07:29:47 +02:00 from the box:

| host | spike (pure Rust TLS 1.3, AES-128-GCM) | curl on the box |
|---|---|---|
| one.galion.app | 200, 674 ms | 200, 1.03 s |
| console.galion.app | 302, 737 ms | 302, 0.67 s |
| obsidian.galion.app | 200, 598 ms | 200, 0.69 s |
| galion.studio | 502, 664 ms | 502, 0.64 s |
| merlinelement.com | 502, 611 ms | 502, 0.70 s |

So it works (certificate chain verified against the roots, same status
codes 5/5). But:

* default build: 408 `aesenc`, 32 `sha256rnds2`, 3 427 AVX `ymm`
  instructions (RustCrypto picks AES-NI/SHA-NI/AVX2 at run time). Only with
  `RUSTFLAGS='--cfg aes_force_soft --cfg polyval_force_soft --cfg
  chacha20_force_soft --cfg poly1305_force_soft --cfg
  curve25519_dalek_backend="serial"'` plus `sha2/force-soft` does it reach
  0/0/0 (still works, same codes);
* rustls-rustcrypto is `0.0.2-alpha`, last release 2024-04-24, says it is
  not audited; it drags a second rustls-webpki (0.102) next to 0.103;
* 106 packages in the spike Cargo.lock (~60 linked); `ring` 0.17 sits in the
  Cargo.lock (not linked for this target);
* webpki-roots is **CDLA-Permissive-2.0** (not on the allowed list); reading
  `/etc/ssl/certs` instead would avoid it;
* 2.56 MB static binary for the spike alone (galion-one is 0.96 MB).

Writing our own TLS 1.3 client (X25519, AES-GCM/ChaCha20-Poly1305, HKDF,
P-256/RSA signature checks, X.509 path validation) is possible in principle
but is security-critical code we should not hand-roll for a status probe.

## Owner decisions (Slavik)

* **D1 PDF default.** (a) `rust` only (this branch's default: no subprocess
  ever, AES/password PDFs give just "a pdf named X, N bytes"); (b)
  `rust+pdftotext` (Rust first, poppler for the rest; poppler is installed on
  the VPS: 24.02.0); (c) keep `pdftotext`. Evidence: 98.38 % recall,
  34/34 scored files >= 92.5 %.
* **D2 HTTPS edge probes / OpenRouter.** (a) keep curl (today; curl 8.5.0 on
  the VPS); (b) adopt rustls + rustls-rustcrypto with the soft cfgs in
  `.cargo/config.toml` (~60 crates, alpha provider, replace webpki-roots by
  system roots); (c) move the edge probe out of galion-one (the nerve/cortex
  already watch the edge) and drop it here; (d) route OpenRouter through a
  loopback proxy so the new std client covers it. Recommendation: (a) now,
  (c) or (d) later; not (b) until rustls-rustcrypto has a stable, audited
  release.
* **D3 env change.** With this branch `ONE_PDFTOTEXT=off` alone no longer
  turns PDF text off (the Rust reader still runs); use `ONE_PDF_TEXT=off`.
  Before deploying, check whether the unit sets `ONE_PDFTOTEXT=off`
  (not inspected here: the unit environment may hold secrets).

## Where the evidence is

docs/real-tests/one-no-shellout.md (commands, outputs, Zagreb times),
docs/third-party/one-no-shellout.md (specs, oracles, the TLS spike).
