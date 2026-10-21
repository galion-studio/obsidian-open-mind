// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! galion-realtest: the platform-wide real-life test harness.
//!
//! * `stack`: build or locate release binaries, boot the whole stack on the
//!   box on spare ports under `galion-boot supervise`, run the scenario
//!   suite against real endpoints, tear down, write JUnit + Markdown.
//! * `live`: READ-ONLY probe of the live VPS (GET / `systemctl is-active` /
//!   `ss` over ssh) and the public endpoints, compared with the contract.

pub mod gaia;
pub mod http;
pub mod live;
pub mod report;
pub mod scenarios;
pub mod sha256;
pub mod stack;
