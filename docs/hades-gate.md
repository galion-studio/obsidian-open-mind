<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# hades-gate

Pure approval gate. Written 2026-10-19T17:12:17+02:00 (Europe/Zagreb).

`decide(action, token)` returns `DryRun` unless `token` is exactly `approved-test` and `action` is `heal-apply`, `switch`, or `rollback`. Those three then return `WouldApply`. Nothing is executed. `kill` and `rotate` always return `Refused`. Default mode is dry-run. No process is spawned and no network is used.
