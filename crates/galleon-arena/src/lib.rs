// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Paper arena for the seed homes `g2-0` through `g2-3`.
//!
//! A [`Gladiator`] is loaded by reading `SOUL.md` and `TASK.md`. A [`Match`]
//! holds the paper base and the seats. A [`Score`] is the unreduced result of
//! those bits. The scorer adds integers. It does not call a model, open a
//! socket, or transfer a value. "Profitable" here means the paper numerator is
//! strictly above zero. It is not money.

#![forbid(unsafe_code)]

mod bout;
mod cli;
mod gladiator;
mod load;
mod paper;

pub use bout::{BoutError, Match};
pub use cli::{dispatch, read_zagreb_now, render_list, CliError, CliOut, ListError, USAGE};
pub use gladiator::{Gladiator, Kind};
pub use load::{load_seed, redacted_homes, LoadError, SEED_IDS, SEED_TEAM_ID};
pub use paper::{score_match, Board, Score, ScoreError, SeatReport};
