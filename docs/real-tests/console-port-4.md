<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: console-port-4 (`/md/$name`, `/memory`, `/mesh`) against the live VPS

Test: `crates/console-static/tests/real_port4.rs`, run as
`GALION_REAL=1 cargo test --release -p console-static --test real_port4 -- --ignored --nocapture` on the box.
Ports (slot 138): binary 127.0.0.1:20080, ssh forward 20081 -> VPS 8080 (console API), 20082 -> 8670 (galion-nerve).

## What touches the VPS (read-only)
- One ssh probe: `find -L` / `stat` / `TZ=Europe/Zagreb date` / `head` / `od` under `/var/lib/memory-tencentdb`, `stat` of
  `/opt/galion/hermes/memories/USER.md`, the 7-byte model file. Nothing is written there.
- One `ssh -N` forward; through it only GETs: `/api/v1/health`, `/api/mesh` (Node runs `mesh.py status`, which only reads
  its state), `/api/v1/admin` (gate check for a public host, no cookie), `/md/user` (Node, for parity), nerve `/state`.
- No service touched, no POST, no paid model, no secrets read. `USER.md` text is never copied (stat only). The box mirror
  holds anonymised names, empty files and the first 1600 bytes of `persona.md` (for the 400-unit excerpt check) in a 0700
  temp folder, removed at the end of every run (`removed the mirror (true)`). Persona text and memory file names are not
  printed or committed; only a hash of the excerpt.
- While writing the test (05:46 CEST) one manual probe wrote and removed a scratch file `/tmp/.p4probe` on the VPS (curl
  output); the committed test does not do that.

## Runs (2026-10-21, CEST +02:00), all PASS
| run | start | duration |
|---|---|---|
| 1 | 05:49:26 | 14.7 s |
| 2 | 05:51:55 | 14.3 s |
| 3 | 05:52:17 | 14.6 s |
| 4 (after final edits) | 05:53:45 | 14.4 s |

Same results in all four runs:

```
real: VPS probe 2977 ms: 1799 tree entries; find counts l0 249 l1 215 l2 249 l3 34907 B scenes 66 refs 940 canvases 17
real: mirror built: 42 dirs, 1757 files (names anonymised), persona 34907 B head 1600 B, USER.md placeholder 1280 B
real: nerve before: cycle 6635 mode live components 15 -> ["hermes-rs:8642 healthy", "galion-desk:8646 healthy", "galion-one:8091 healthy", "console:8080 healthy"]
real: /memory 200 15787 B in 612 ms
real: /memory = VPS find on all 7 rows; model "galion"; excerpt 400 UTF-16 units, fnv 59fc4d94b4cc5d82 (text not printed)
real: /md/persona shows "persona.md · 34907 bytes · 2026-10-18 21:40 CEST (+02:00)" (= VPS stat + TZ=Europe/Zagreb date)
real: /md/user shows "USER.md · 1280 bytes · 2026-10-18 23:41 CEST (+02:00)" (= VPS stat + TZ=Europe/Zagreb date)
real: live Node /md/user on a loopback Host: 200 3697 B, SPA shell, no file (the TSX stub route; the middleware skips the file on loopback)
real: /mesh 200 14505 B in 842 ms; live phase "idle", 1 score line(s)
real: /mesh lines + footer "Cold. Nothing is spending." = live
real: public host unpaired -> pairing page for all 3 rooms (live /api/v1/admin)
real: nerve after: cycle 6635 mode live components 15 -> [... all 4 healthy]
real: removed the mirror (true)
```

(Run 1 printed the live `/md/user` check with `Accept: application/json`: Node answered `500 {"error":"Only HTML requests are
supported here"}`; the test now sends `Accept: text/html`, as a browser does.)

## What it shows
- The Rust walk of memory.tsx's rules gives the same 7 numbers as GNU `find` on the live tree (1799 entries): L0 249, L1 215,
  L2 249, L3 34 907 B, scenes 66, raw tool refs 940, canvases 17. The model line is `galion`.
- `/md/*` sizes and times match the VPS `stat`, and the Rust Zagreb clock matches the VPS `TZ=Europe/Zagreb date` to the
  minute.
- `/mesh` shows exactly the live score line and the idle footer.
- An unpaired public host gets the pairing page for all three rooms (decided by the live `/api/v1/admin`); no counts, no file.
- The live Node console never shows the file on a loopback Host (SPA shell for the stub route); the Rust room does (owner
  decision 2 in `docs/console-port-4.md`).
- Page times (~0.6 s `/memory`, ~0.85 s `/mesh`) include the header-dot `/api/v1/health` call and, for `/mesh`, the live
  `mesh.py` run, both over the ssh forward.
- Nerve (127.0.0.1:8670 `/state`): 15 components, console/desk/hermes-rs/galion-one healthy before and after. The cortex
  is not deployed, so the nerve is the only live view.
