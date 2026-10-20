<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Cutover approval

- DESK-ID: openmind.AUDIT-APPROVAL
- Written: 2026-10-19T22:29:16+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Read only: release/TEACHER/AWAITING-APPROVAL.md, TEACHER/STAGED.md, docs/next.md
- Those three files were not edited. No box in them was marked checked. No box in this file is checked.
- This note does not install, enable, start, stop, or replace a live process.

The stamp ends in a numeric offset. A trailing UTC designator is not used.

## Approved

No. Slavik has not approved a live replacement of galion-one, galion-desk, link-watch, tuner, the console on 8080, or memory on 8420.

## Heading

Quoted from release/TEACHER/AWAITING-APPROVAL.md, the heading that says approval is still required:

> # AWAITING APPROVAL (Slavik)

The line under that heading says nothing there is done, and that each line needs explicit approval.

A second heading in TEACHER/STAGED.md says the same for the staged behaviour change:

> ## Behaviour awaiting approval

## Each name

| name | live replacement approved by Slavik |
| --- | --- |
| galion-one | no |
| galion-desk | no |
| link-watch | no |
| tuner | no |
| console on 8080 | no |
| memory on 8420 | no |

### galion-one

TEACHER/STAGED.md lists a staged galion-one unit and, later, the B14 retire of the legacy worker. Both stay proposals. The B14 close says production bins and units were not switched. docs/next.md says not to copy a new binary over a live pid until Slavik says so, and not to dial a chosen row from galion-one. No live replacement is approved.

### galion-desk

TEACHER/STAGED.md stages deploy/galion-desk.service with no install section, so it cannot be enabled. The survival drop-in for galion-desk sits under the survival units that still await Slavik. docs/next.md does not approve a desk switch. No live replacement is approved.

### link-watch

TEACHER/STAGED.md has a link-watch heading that says the France probe is config-only and is awaiting Slavik's approval. The same note says the staged unit is not installed, not enabled, and not switched. No live replacement is approved.

### tuner

TEACHER/STAGED.md lists deploy/survival/tuner.d/10-restart.conf with the survival units that still await Slavik. docs/next.md names tuner only as a smaller candidate after Slavik approves one binary. That sentence is not an approval. No live replacement is approved.

### console on 8080

TEACHER/STAGED.md records the console production build as staged, not switched. The unit that would serve it on 127.0.0.1:8080 is not enabled and has no install section. docs/next.md says not to free port 8080, and that Vite stays until the console switch is approved. No live replacement is approved.

### memory on 8420

docs/next.md says not to point memory clients at a new process on 8420, and that the live gateway stays. A shadow run, if any, must bind 127.0.0.1:0 and must not switch the client. No live replacement is approved.

## Open boxes

Copied open from release/TEACHER/AWAITING-APPROVAL.md. Not checked:

- [ ] Create public GitHub repo (name TBD) and push the workspace + release/repo files
- [ ] Publish release/site/ (hosting TBD)
- [ ] Post launch announcement and short posts (channels TBD)

## Not done

No source doc was edited. No box was marked checked. Nothing was committed. Nothing was pushed. systemctl was not used. No secret was written here.
