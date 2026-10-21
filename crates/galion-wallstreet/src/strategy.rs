// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Strategy scratchpad (the agent's working notes, kept with the ticket and
//! printed in the report) and a simple rule-based strategy for backtests.

use crate::broker::Side;
use crate::compliance::{self, Verdict};
use crate::market::Bar;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Note {
    pub seq: u32,
    pub date: String,
    pub ticket: String,
    pub text: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Scratchpad {
    pub notes: Vec<Note>,
}

impl Scratchpad {
    /// Add a note. Notes go through the compliance screen like everything else:
    /// a scratchpad is not a place to park an insider tip.
    pub fn add(
        &mut self,
        date: &str,
        ticket: &str,
        text: &str,
        tags: &[String],
    ) -> Result<&Note, Verdict> {
        let v = compliance::screen(text);
        if v.is_refusal() {
            return Err(v);
        }
        let seq = self.notes.len() as u32 + 1;
        self.notes.push(Note {
            seq,
            date: date.into(),
            ticket: ticket.into(),
            text: text.trim().into(),
            tags: tags.to_vec(),
        });
        Ok(self.notes.last().expect("just pushed"))
    }

    pub fn for_ticket(&self, ticket: &str) -> Vec<&Note> {
        self.notes.iter().filter(|n| n.ticket == ticket).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signal {
    pub side: Side,
    pub qty: i64,
}

pub trait Strategy {
    fn name(&self) -> String;
    /// Called once per revealed bar with the visible history only.
    fn on_bar(&mut self, history: &[Bar], position: i64) -> Option<Signal>;
}

/// Long-only simple moving average crossover on closes.
#[derive(Debug, Clone, Copy)]
pub struct SmaCross {
    pub fast: usize,
    pub slow: usize,
    pub qty: i64,
}

fn sma(h: &[Bar], n: usize) -> Option<i64> {
    if h.len() < n || n == 0 {
        return None;
    }
    Some(h[h.len() - n..].iter().map(|b| b.close).sum::<i64>() / n as i64)
}

impl Strategy for SmaCross {
    fn name(&self) -> String {
        format!("sma_cross({}/{}, {} sh)", self.fast, self.slow, self.qty)
    }
    fn on_bar(&mut self, h: &[Bar], position: i64) -> Option<Signal> {
        let (f, s) = (sma(h, self.fast)?, sma(h, self.slow)?);
        if f > s && position == 0 {
            Some(Signal {
                side: Side::Buy,
                qty: self.qty,
            })
        } else if f < s && position > 0 {
            Some(Signal {
                side: Side::Sell,
                qty: position,
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scratchpad_screens_notes() {
        let mut s = Scratchpad::default();
        assert!(s
            .add("d", "T", "momentum after public earnings beat", &[])
            .is_ok());
        assert!(s.add("d", "T", "inside information says buy", &[]).is_err());
        assert_eq!(s.for_ticket("T").len(), 1);
        assert_eq!(s.notes[0].seq, 1);
    }
}
