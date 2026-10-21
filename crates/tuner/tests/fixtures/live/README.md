<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Live tuner responses (golden fixtures)

Raw HTTP responses of the live tuner (unit `tuner.service`, pid 1196273,
`/opt/galion/tuner/bin/tuner`, sha256 `ecad72a4368a3c91...`) on VPS
`127.0.0.1:8650`, captured 2026-10-20T19:06:06+02:00 (Europe/Zagreb)
with `scripts/real/tuner-capture.sh 8650` over ssh. One file per line of
`scripts/real/tuner-requests.txt`; GET requests only. The responses carry no
Date header and no secrets. The bytes are exact (CRLF kept, see .gitattributes).
