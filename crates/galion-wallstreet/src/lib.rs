// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! # galion-wallstreet: B2 "Wall Street" bench (PAPER ONLY)
//!
//! A Rust paper broker on delayed, recorded fixture market data. It measures
//! whether Hermes can research and paper-trade under risk limits, not whether
//! it can manage real capital.
//!
//! Hard guarantees, by construction:
//! - No network client code; the only socket is the optional loopback HTTP
//!   *server*. No broker SDK, no credentials, no account ids.
//! - All money is integer cents; every report carries a reproducible
//!   FNV-1a fingerprint.
//! - An order over `max_order_notional` is refused and trips the kill switch;
//!   with the switch engaged only position-reducing orders pass.
//!
//! Modules: [`market`] (ingest + delayed feed), [`universe`], [`broker`]
//! (paper order book), [`risk`], [`strategy`] (scratchpad + SMA), [`backtest`],
//! [`news`] (event tagging), [`compliance`], [`report`], [`ticket`] (scripted
//! Hermes tickets), [`score`] (the scoreboard), [`http`].

pub mod adversarial;
pub mod backtest;
pub mod broker;
pub mod compliance;
pub mod http;
pub mod intent;
pub mod market;
pub mod money;
pub mod news;
pub mod report;
pub mod risk;
pub mod score;
pub mod strategy;
pub mod ticket;
pub mod universe;
