#!/bin/bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Copyright (c) 3000 Galion / galion-studio
#
# tuner-capture.sh <port>  (route matrix on stdin, see tuner-requests.txt)
# Sends each raw GET to 127.0.0.1:<port> with bash /dev/tcp and prints one
# line per request: "<name> <base64 of the raw response>". Writes no files,
# so it can run read-only on the VPS through `ssh host bash -s -- 8650`.
# GET only; a request that is not GET is refused here.
set -euo pipefail
port=${1:?port}
while read -r name fmt; do
  case "$name" in ''|'#'*) continue;; esac
  case "$fmt" in GET*) ;; *) echo "refusing non-GET request $name" >&2; exit 2;; esac
  exec 3<>"/dev/tcp/127.0.0.1/$port"
  # shellcheck disable=SC2059
  printf "$fmt" >&3
  out=$(timeout 5 cat <&3 | base64 -w0 || true)
  exec 3<&- 3>&-
  echo "$name $out"
done
