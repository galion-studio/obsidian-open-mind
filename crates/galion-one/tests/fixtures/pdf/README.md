<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# PDF fixtures (swarm/one-no-shellout)

All five PDFs are ghostscript 10.05.1 `pdfwrite` output of `fixture.ps`
(our own text, two pages; page 2 says "PAGE TWO must not appear").
`-dEmbedAllFonts=false`: no font program is embedded, so the files carry
no third-party font data. Regenerate (byte content differs per run because
of dates and /ID, the extracted text does not):

```sh
G="gs -q -dSAFER -dBATCH -dNOPAUSE -sDEVICE=pdfwrite -dEmbedAllFonts=false"
E="-sOwnerPassword=owner148 -dPermissions=-4"
$G -dCompatibilityLevel=1.4 -sOutputFile=gs-plain.pdf fixture.ps
$G -sOutputFile=gs-objstm.pdf fixture.ps            # PDF 1.7: /ObjStm + /XRef
$G -dCompatibilityLevel=1.4 $E -dEncryptionR=2 -dKeyLength=40  -sOutputFile=gs-rc4-r2.pdf fixture.ps
$G -dCompatibilityLevel=1.4 $E -dEncryptionR=3 -dKeyLength=128 -sOutputFile=gs-rc4-r3.pdf fixture.ps
$G -dCompatibilityLevel=1.4 $E -sUserPassword=galion -dEncryptionR=3 -dKeyLength=128 -sOutputFile=gs-userpw.pdf fixture.ps
```

These passwords are test values only, not secrets.
