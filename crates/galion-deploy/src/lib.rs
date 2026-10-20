// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `galion-deploy <service> <new-binary> [health-url]`: backup, atomic install,
//! restart, health check, automatic rollback. Same CLI, environment, log lines and
//! on-disk layout as the bash script it replaces (kept for reference at
//! docs/reference/galion-deploy.bash), with the deploy-sim findings fixed:
//!
//! * F1 `<new> --version` runs under a timeout (default 5 s) in its own process
//!   group, which is killed as a whole when the timeout expires.
//! * F2 `reset-failed` before every restart (default on), and a refused restart is
//!   retried once after `reset-failed`, so the start limit cannot block a rollback.
//! * F3 distinct exit codes, see [`Exit`].
//! * F4 per-unit health paths (galion-nerve is `/healthz`), and the rollback is
//!   judged on the unit's own health path, not on a URL meant for the new build.
//! * F5 first install: a unit whose binary does not exist yet is installed and
//!   started; if it fails, it is stopped and the binary removed again.
//! * F6 SIGINT/SIGTERM/SIGHUP roll back instead of killing the deploy, and a
//!   rollback journal (`<backup-dir>/<unit>.journal`, also the per-unit lock) lets
//!   the next run restore the old binary after an uncatchable kill.
//!
//! `galion-deploy backup` writes the pre-cutover snapshot (see [`backup`]); its
//! data part is `galion-backup run`, the one definition of a Galion backup.

pub mod backend;
pub mod backup;
pub mod deploy;
pub mod files;
pub mod http;
pub mod journal;
pub mod sha256;
pub mod sys;
pub mod tar;
pub mod zagreb;

pub use deploy::{Config, Exit};
