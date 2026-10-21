<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# console-port-4: `/md/$name`, `/memory`, `/mesh` in Rust

Wave 4 slot 138, branch `swarm/console-port-4`, base `landing-overview` 669dc18. Written 2026-10-21 05:55 CEST (+02:00).

## Which routes
The 17 console routes still in TypeScript at 669dc18, alphabetical, split 4/4/3/3/3:
analytics, arena, bank, cache | context, gateway, guardian, hermes | instances, keys, markers | **md.$name, memory, mesh** |
node, security, session. This branch ports the fourth fifth. `PORTED` goes from 13 to 16 (`/md/user` stands for `/md/$name`;
`/md/persona` and any other `/md/*` are answered too).

Source: the live console-ide tree read 2026-10-20 21:25 CEST (`/workspace/swarm/artifacts/console-ui/live-src-20261020-2125`):
`routes/_console/{md.$name,memory,mesh}.tsx`, `routes/api/mesh.ts`, the `/md/user` / `/md/persona` branch of `start.ts`,
`lib/desk-foundation.ts`.

## What each room does now (no JavaScript, GET/HEAD only, CSP unchanged)

| room | data | differences from the TSX |
|---|---|---|
| `/memory` | host files, like the TSX server function (Node `fs`): counts under `--memory-root` (live `/var/lib/memory-tencentdb`) with the memory.tsx rules (L0-L2 files at any depth, L3 = bytes of `pyramid/l3/persona.md`, scenes = `*.md` directly in `scene_blocks`, raw tool refs = `*.md` directly in every `refs` folder under `offload`, canvases = `canvas.mmd` under `offload`); model from `--model-file`; first 400 UTF-16 units of `persona.md` | no 4 s poll (Refresh link); says "not configured" / "could not be read: …" instead of zeros; walk capped at 250 000 entries and depth 32 (symlink loops end; the page says when counts are lower bounds); a link to the whole persona |
| `/mesh` | `GET /api/mesh` on the console API (live: Node runs `mesh.py status`, read-only; the Rust side runs nothing) | no 4 s poll; when the call fails the page says `Mesh status did not load: HTTP 500 (mesh down)` instead of only "No live line yet."; max 50 lines × 2 000 chars |
| `/md/$name` | `user` -> `--user-md` (live `/opt/galion/hermes/memories/USER.md`), `persona` -> `<memory-root>/persona.md`; nothing from the URL reaches the file system; max 1 MiB shown | mtime on the Zagreb clock with the zone named (TSX: UTC); runs behind the same gate as every room; **fix**: on a loopback Host the TSX middleware skipped the file and showed the stub route (live today: SPA shell), here loopback gets the file; unknown names get a 404 page with the two tabs; missing file 404, unreadable 500, both without the host path |

Code: `crates/console-ui/src/port4.rs` (handlers, file walk, `HostFiles`), `pages/{md,memory,mesh,p4}.rs`, a `PORT4` block in
`css.rs` (own `p4-` class names), flags `--memory-root`, `--model-file`, `--user-md` (absolute paths, no `..`) in
`console-static/src/config.rs`. Shared hunks with the other console-port branches: `lib.rs` (PORTED, `mod port4`,
`UiConfig.files`, `owns`, two route arms), `pages/mod.rs`, `css.rs` (sheet/defined_classes chain), `config.rs`/`server.rs`,
`tests/ui.rs` (fake `/api/mesh`, `--user-md` for the PORTED loop, not-ported 16 -> 14).

## Tests
- Unit (console-ui, 44 total, 18 new): p4 (Hinnant round trip, Zagreb DST edges 2026-03-29 / 2026-10-25 / 2027-03-28, JS
  `slice` on UTF-16 incl. surrogate pairs), memory (live shape, unconfigured, unreadable, capped), mesh (live shape, phases,
  caps), md (fixed names incl. `../etc/passwd`, escaping, Zagreb time, errors), port4 (every memory.tsx counting rule on a temp
  tree, missing root, symlink loop capped, `/md` reads only the two files, 1 MiB cap, every emitted class styled).
- Config: `host_file_flags` (absolute only, no `..`, missing value refused).
- End-to-end, fake upstream (`crates/console-static/tests/ui_port4.rs`, 7 tests): counts from a host tree, persona escaped
  and cut at 400 units, HEAD, no host path on any page, only `GET /api/v1/health` upstream for `/memory`; `/mesh` live shape,
  500, not-JSON, upstream gone; `/md` fixed names, 7 traversal / case / suffix / NUL tries -> 404/400 with nothing read;
  not configured / missing / unreadable; gate (public host unpaired -> pairing, paired -> content, admin cookie forwarded,
  non-console host -> Soon, POST -> 405 on all three); `owns` with an SPA fallback.
- Real-life against the live VPS, read-only (`tests/real_port4.rs`, `GALION_REAL=1`, ports 20080-20082): see
  `docs/real-tests/console-port-4.md`.

## Owner decisions (Slavik)
1. **Host files read by the console process.** `/memory` and `/md/*` read Hermes memory files directly (as the Node console
   does today). For the cut-over unit: pass `--memory-root /var/lib/memory-tencentdb --model-file
   /opt/galion/galion-light/one/model --user-md /opt/galion/hermes/memories/USER.md`, and the console user must be able to
   read them. Live, `/opt/galion/hermes/memories` is `drwx------ root`: a non-root console shows `USER.md is not shown:
   permission denied`. Options: (a) run as root like Node does now; (b) a read-only group on those files (permission change =
   your call); (c) drop `/md/user` from the Rust console.
2. **Show `USER.md` / persona on loopback.** The TSX never served them on a loopback Host (stub); the Rust room does (loopback
   peer + loopback Host only). Keep, or hide `/md/*` on loopback.
3. **Refresh for `/memory` and `/mesh`** (TSX polled every 4 s): Refresh link now; `<meta http-equiv=refresh>` or a script
   island per the JS ruling (decision 1 of `docs/console-ui-decision.md`).
4. **`/api/mesh` still runs `mesh.py`** on the Node side. The Rust room only reads it; porting the mesh status
   (mesh.py state file + `state/eval/survival.txt`) to Rust is a separate task if you want Python gone there.
