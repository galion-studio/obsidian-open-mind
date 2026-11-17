<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# openmind-desktop

Local control console for this machine. With no flags it prints the page and exits, and it opens no socket. `--listen` binds a loopback address only. The default port is 18181.

This directory is a workflow artifact. It is not a GitHub Release and it was not published. It is not signed.

## Files

`SHA256SUMS` lists this file, `LICENSE`, the archive, and the Linux `.deb` when that file is present. The archive holds the binary, `LICENSE`, this file, `console.html` (the page, written with no socket), and `LOCAL-ARTIFACT.txt`. A Linux archive also holds the `.deb`.

`LICENSE` is the MIT text plus the four Galion Labs lines.

## Run

From the unpacked directory on Unix or macOS:

    ./openmind-desktop

On Windows:

    .\openmind-desktop.exe

On Debian or Ubuntu, `dpkg -i` on the Linux `.deb` installs the `openmind-desktop` binary under `/usr/bin`.

## Sound

Tones in the page are integer ratios of 432 Hz.
