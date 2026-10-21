<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: swarm/one-no-shellout (wave 4 slot 148)

All times Europe/Zagreb (+02:00). Box = the swarm build box; VPS =
slavik-vps, read-only (GET, `systemctl show`, `/proc` reads). Nothing was
deployed, restarted or built on the VPS. The cortex is not live; the nerve
(VPS 127.0.0.1:8670 `/state`) was used.

## 1. Real galion-one binaries, slot ports 20180-20189 (box)

```sh
GALION_REAL=1 GALION_PDF_CORPUS=/workspace/swarm/scratch-148/corpus \
  cargo test --release -p galion-one --test real_one_no_shellout -- --nocapture --test-threads=1
```

Run 2026-10-21 07:34:39 -> 07:34:52 +02:00 (release build): **2 passed, 0
failed**. Earlier debug runs 07:1x-07:24 +02:00 found two test bugs (the curl
marker came from the edge probe, and the fake mind also gets `/health`); both
fixed in the test, not by weakening the asserts.

What the test does: three real `galion-one` processes (A default, B
`ONE_PDF_TEXT=rust+pdftotext`, C `ONE_PDF_TEXT=pdftotext`) with a fake ear
(20182), a capturing fake mind (20183) and an OpenRouter stand-in (20188,
chunked reply). `PATH` contains only a fake `pdftotext` and a fake `curl`
that log their arguments to a marker file, so "no subprocess" is observed.
PDFs go through the real routes (`POST /files`, then `POST /reply` with
`X-Files`); the text is read from the chat request the mind received.

Output (trimmed to 200 chars per line):

```
A gs-plain.pdf: 1 ms: a pdf named gs-plain.pdf. First page: Galion One PDF test Dobar dan, Zagreb! first page text. Second line: 12345 hermes nerve cortex.
A gs-objstm.pdf: 0 ms: a pdf named gs-objstm.pdf. First page: Galion One PDF test Dobar dan, Zagreb! first page text. Second line: 12345 hermes nerve cortex.
A gs-rc4-r2.pdf: 0 ms: a pdf named gs-rc4-r2.pdf. First page: Galion One PDF test Dobar dan, Zagreb! first page text. Second line: 12345 hermes nerve cortex.
A gs-rc4-r3.pdf: 0 ms: a pdf named gs-rc4-r3.pdf. First page: Galion One PDF test Dobar dan, Zagreb! first page text. Second line: 12345 hermes nerve cortex.
A gs-userpw.pdf: a pdf named gs-userpw.pdf, 4559 bytes
A corpus: 39 PDFs through /files + /reply
A: models refreshed in-process (1 request to the stand-in); curl calls: 0, all edge probes: {}
A:\ models\ refreshed\ in\-process\ (1\ request\ to\ the\ stand\-in)\;\ curl\ calls:\ 0\,\ all\ edge\ probes:\ {}
A /models: 200 [{"id": "alpha/model-a", "name": "alpha: Model \u00e9 A"}, {"id": "noname/model-c", "name": "noname/model-c"}, {"id": "zeta/model-b", "name": "Zeta: Model B"}]
B gs-userpw.pdf: a pdf named gs-userpw.pdf. First page: FAKE PDFTOTEXT OUTPUT
C gs-plain.pdf: a pdf named gs-plain.pdf. First page: FAKE PDFTOTEXT OUTPUT
```

Checked by the test: A = exact expected first-page text for the 4 readable
fixtures (assert_eq on the whole note, so page 2 cannot sneak in), password PDF falls back to "name, size";
A's fake pdftotext marker never written; `/models` refreshed in-process
with the bearer header and 0 curl calls mentioning models/openrouter;
B runs pdftotext only for gs-userpw; C runs it as before; the private
temp dir is empty afterwards. 39 corpus PDFs also went through A's routes
(no crash, 0-52 ms each).

## 2. Built-in reader vs real poppler `pdftotext` (box, poppler 25.03.0)

Same run, test `real_pdf_corpus_matches_pdftotext`: word recall = share of
pdftotext's first-page words that our first-page output also contains
(multiset). Gate: every scored file >= 85 %, total >= 95 %. RTL is not
scored (we keep logical order).

```
002-trivial-libre-office-writer.pdf          pdftotext   100 rust   100 common   100 recall 100.0%    1 ms 
1706.03762.pdf                               pdftotext   398 rust   396 common   391 recall  98.2%   24 ms 
GeoTopo.pdf                                  pdftotext    13 rust    13 common    13 recall 100.0%   47 ms 
TAMReview.pdf                                pdftotext   229 rust   229 common   229 recall 100.0%    6 ms 
alphatrans.pdf                               pdftotext    17 rust    17 common    17 recall 100.0%    1 ms 
annotated_pdf.pdf                            pdftotext     8 rust     8 common     8 recall 100.0%    1 ms 
asciihexdecode.pdf                           pdftotext     2 rust     2 common     2 recall 100.0%    0 ms 
c4611_sample_explain.pdf                     pdftotext   189 rust   189 common   189 recall 100.0%    2 ms 
chrome-skia.pdf                              pdftotext    24 rust    24 common    24 recall 100.0%    1 ms 
cmyk-image.pdf                               pdftotext     0 rust     0 common     0 recall 100.0%    4 ms pdf-text: NoText
compressed.tracemonkey-pldi-09.pdf           pdftotext   723 rust   724 common   669 recall  92.5%   11 ms 
crazyones-pdfa.pdf                           pdftotext   170 rust   170 common   170 recall 100.0%    1 ms 
devicen.pdf                                  pdftotext     0 rust     0 common     0 recall 100.0%    1 ms pdf-text: NoText
fw9.pdf                                      pdftotext   933 rust   933 common   933 recall 100.0%    4 ms 
google-doc-document.pdf                      pdftotext   178 rust   176 common   172 recall  96.6%    2 ms 
grayscale-image.pdf                          pdftotext     0 rust     0 common     0 recall 100.0%    1 ms pdf-text: NoText
gs-encrypted.pdf                             pdftotext    16 rust    16 common    16 recall 100.0%    1 ms 
gs-objstm.pdf                                pdftotext    16 rust    16 common    16 recall 100.0%    0 ms 
gs-plain.pdf                                 pdftotext    16 rust    16 common    16 recall 100.0%    0 ms 
habibi.pdf                                   pdftotext     2 rust     3 common     0 recall   0.0%    1 ms  (RTL: order differs, not scored)
inline-image.pdf                             pdftotext     1 rust     1 common     1 recall 100.0%    0 ms 
issue7101.pdf                                pdftotext     3 rust     3 common     3 recall 100.0%    1 ms 
libre-office-link.pdf                        pdftotext     8 rust     8 common     8 recall 100.0%    0 ms 
libreoffice-form.pdf                         pdftotext    20 rust    20 common    20 recall 100.0%    1 ms 
libreoffice-txt.pdf                          pdftotext    19 rust    19 common    19 recall 100.0%    1 ms 
libreoffice-writer-password.pdf              pdftotext     0 rust     0 common     0 recall 100.0%    1 ms pdf-text: Encrypted
minimal-document.pdf                         pdftotext   101 rust   102 common   100 recall  99.0%    1 ms 
multicolumn.pdf                              pdftotext   508 rust   524 common   491 recall  96.7%    2 ms 
output_with_metadata_pymupdf.pdf             pdftotext     2 rust     2 common     2 recall 100.0%    0 ms 
pdf-sample.pdf                               pdftotext   173 rust   173 common   173 recall 100.0%    1 ms 
pdfkit.pdf                                   pdftotext     5 rust     5 common     5 recall 100.0%    1 ms 
pdflatex-4-pages.pdf                         pdftotext   710 rust   710 common   710 recall 100.0%    2 ms 
pdflatex-image.pdf                           pdftotext   104 rust   104 common   104 recall 100.0%    1 ms 
pdflatex-outline.pdf                         pdftotext    29 rust    29 common    29 recall 100.0%    1 ms 
reportlab-overlay.pdf                        pdftotext     7 rust     7 common     7 recall 100.0%    1 ms 
sample.pdf                                   pdftotext   416 rust   416 common   415 recall  99.8%    1 ms 
table.pdf                                    pdftotext    65 rust    65 common    65 recall 100.0%    2 ms 
unreadablemetadata.pdf                       pdftotext     0 rust     0 common     0 recall 100.0%   20 ms pdf-text: NoPage
with-attachment.pdf                          pdftotext   101 rust   102 common   100 recall  99.0%    1 ms 
gs-objstm.pdf                                pdftotext    16 rust    16 common    16 recall 100.0%    1 ms 
gs-plain.pdf                                 pdftotext    16 rust    16 common    16 recall 100.0%    0 ms 
gs-rc4-r2.pdf                                pdftotext    16 rust    16 common    16 recall 100.0%    1 ms 
gs-rc4-r3.pdf                                pdftotext    16 rust    16 common    16 recall 100.0%    1 ms 
gs-userpw.pdf                                pdftotext     0 rust     0 common     0 recall 100.0%    1 ms pdf-text: Encrypted
corpus: 44 files, 34 scored, 5276/5363 words = 98.38% recall; worst 92.5% (compressed.tracemonkey-pldi-09.pdf), slowest 47 ms (GeoTopo.pdf)
```

Misses explained: tracemonkey (92.5 %) = tokens split differently at
hyphenated line ends and around commas; google-doc (96.6 %) = emoji flags;
multicolumn (96.7 %) = column interleave differs (content-stream order).
NoText files are scanned/image-only (pdftotext also gives nothing);
unreadablemetadata gives NoPage (pdftotext also gives 0 words).
Encrypted = real user password (correct: poppler needs it too).

## 3. Unit tests and checks (box)

| check | result |
|---|---|
| `cargo test -p galion-one --bin galion-one` | 78 passed, 0 failed (40/40 repeated runs of the test binary green after the ETXTBSY fix below) |
| `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect` | 520 passed, 0 failed, 116 test targets (final run 07:36 +02:00) |
| `cargo clippy --workspace --all-targets ... -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| `scripts/no-python.sh` / `scripts/spdx-scan.sh` | ok / 0 missing |
| mutation fuzz (`mutated_inputs_never_panic`) | 7 500 mutants of 5 seeds, no panic |
| x86_64-unknown-linux-musl release | 963 904 B (base fed3de4: 841 024 B) |
| aarch64-unknown-linux-musl release (linker rust-lld; the box `cc`/ld rejects `--fix-cortex-a53-843419`) | 843 360 B; under qemu-aarch64 (no panic on a closed pipe) it extracts gs-rc4-r3.pdf and arXiv 1706.03762 correctly |
| `cargo check` aarch64-linux-android, armv7-unknown-linux-musleabihf | ok |
| `objdump -d` x86_64 musl | 0 aesenc, 0 sha256rnds2, 0 pclmulqdq, 0 ymm, 0 zmm |

Flaky test found and fixed: the pre-existing
`fake_nvidia_smi_is_read_and_a_hung_one_is_killed` failed 1 of 4 runs with
the new spawning tests (a parallel fork briefly holds the freshly written
script open -> `ETXTBSY`). Tests that write-then-exec or spawn oracles now
share `EXEC_TEST_LOCK`; 40/40 runs green afterwards.

## 4. VPS (read-only), 2026-10-21

| time (+02:00) | probe | result |
|---|---|---|
| 07:24-07:26 | `pdftotext -v`, `curl --version`, `nvidia-smi` | poppler 24.02.0, curl 8.5.0, no nvidia-smi |
| 07:35:12 | `systemctl show galion-one` | active, MainPID 1373612, since 2026-10-18 22:13:56 +02:00, binary /opt/galion/openmind/dist/bin-ws/galion-one (not touched) |
| 07:2x (30 s sampling) and 07:35:12 | `/proc/1373612/task/*/children` | 0 child processes |
| 07:35:05 | nerve `/state` cycle 6841, mode live | 11/15 healthy; galion-one healthy (HTTP 200, rss 676 KB, 0 restarts 1 h); not healthy: galion-one-mouth degrading (503 "model loading"), cloudflared degrading, voice-worker absent, gaia absent |
| 07:24-07:26 | nerve cycle 6822 | 11/15 healthy, galion-one healthy (rss 664 KB) |
| 07:2x | `curl` edge hosts from the VPS | 403 / 403 / 403 / 502 / 502 (one, console, obsidian, galion.studio, merlinelement); from the box 200 / 302 / 200 / 502 / 502. The 403 from the VPS's own IP matches an earlier record note; it is the same for curl, so not a TLS-stack question |

Disclosure: at 07:2x one probe wrote a temp file `/tmp/.n148` (nerve JSON
for jq) on the VPS and removed it right after. No user PDFs on the VPS were
read; the corpus is public sample files on the box.

## 5. Pure-Rust TLS spike vs curl (box), 2026-10-21 07:29:47 +02:00

```
one.galion.app Ok("200 TLS13_AES_128_GCM_SHA256") 674ms
console.galion.app Ok("302 TLS13_AES_128_GCM_SHA256") 737ms
obsidian.galion.app Ok("200 TLS13_AES_128_GCM_SHA256") 598ms
galion.studio Ok("502 TLS13_AES_128_GCM_SHA256") 664ms
merlinelement.com Ok("502 TLS13_AES_128_GCM_SHA256") 611ms
one.galion.app curl 200 1.031758s
console.galion.app curl 302 0.674790s
obsidian.galion.app curl 200 0.685671s
galion.studio curl 502 0.642739s
merlinelement.com curl 502 0.695544s
```

Soft-crypto build (0 AES-NI/SHA-NI/AVX instructions), TLS 1.3, certificate chain verified. Details in docs/one-no-shellout.md.
