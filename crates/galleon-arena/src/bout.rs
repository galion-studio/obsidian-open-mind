// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

use crate::paper::{score_match, Board, ScoreError, SeatReport};

/// Why a match record was rejected. Nothing was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoutError {
    EmptyId,
    BadStamp(String),
    BaseBelowOne(i64),
    EmptyTask,
    EmptySeats,
    DuplicateSeat(String),
    EmptySeatId,
}

impl std::fmt::Display for BoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BoutError::EmptyId => write!(f, "match id is empty"),
            BoutError::BadStamp(at) => {
                write!(
                    f,
                    "timestamp is not Europe/Zagreb RFC 3339 with a numeric offset: {at}"
                )
            }
            BoutError::BaseBelowOne(base) => {
                write!(f, "paper base must be >= 1, got {base}")
            }
            BoutError::EmptyTask => write!(f, "match task is empty"),
            BoutError::EmptySeats => write!(f, "match has no seats"),
            BoutError::DuplicateSeat(id) => write!(f, "duplicate seat {id}"),
            BoutError::EmptySeatId => write!(f, "seat id is empty"),
        }
    }
}

impl std::error::Error for BoutError {}

/// One paper bout. `at` is Europe/Zagreb RFC 3339 with `+01:00` or `+02:00`.
///
/// `base` is a paper integer, not a sum of money. `rounds` is board text, not
/// a live session. The winner is not stored here; [`Match::score`] returns it
/// on the [`Board`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub id: String,
    pub at: String,
    pub base: i64,
    pub task: String,
    pub seats: Vec<String>,
    pub rounds: Vec<String>,
}

impl Match {
    /// Build a match after checking the stamp, the base, and the seat ids.
    ///
    /// # Errors
    ///
    /// Empty id, stamp that is not Europe/Zagreb RFC 3339, `base < 1`, empty
    /// task, empty seat list, empty seat id, or a repeated seat id.
    pub fn try_new(
        id: impl Into<String>,
        at: impl Into<String>,
        base: i64,
        task: impl Into<String>,
        seats: Vec<String>,
        rounds: Vec<String>,
    ) -> Result<Self, BoutError> {
        let id = id.into();
        if id.is_empty() {
            return Err(BoutError::EmptyId);
        }
        let at = at.into();
        if !zagreb_stamp(&at) {
            return Err(BoutError::BadStamp(at));
        }
        if base < 1 {
            return Err(BoutError::BaseBelowOne(base));
        }
        let task = task.into();
        if task.trim().is_empty() {
            return Err(BoutError::EmptyTask);
        }
        if seats.is_empty() {
            return Err(BoutError::EmptySeats);
        }
        let mut seen = Vec::with_capacity(seats.len());
        for seat in &seats {
            if seat.is_empty() {
                return Err(BoutError::EmptySeatId);
            }
            if seen.iter().any(|id| id == seat) {
                return Err(BoutError::DuplicateSeat(seat.clone()));
            }
            seen.push(seat.clone());
        }
        Ok(Self {
            id,
            at,
            base,
            task,
            seats,
            rounds,
        })
    }

    /// Score the judge's bits. Same base and same bits always return the same
    /// numbers. Does not read files and does not touch the network.
    ///
    /// # Errors
    ///
    /// A seat id is empty, `passed > n`, a seat is repeated, a non-team seat
    /// is not on this match, or the arithmetic overflows.
    pub fn score(&self, seats: &[SeatReport]) -> Result<Board, ScoreError> {
        score_match(self.base, &self.seats, seats)
    }
}

/// `YYYY-MM-DDTHH:MM:SS` with an optional fraction, then `+01:00` or `+02:00`.
/// Rejects `Z` and `+00:00`.
pub(crate) fn zagreb_stamp(at: &str) -> bool {
    if at.contains('Z') || at.contains('z') || at.contains("+00:00") {
        return false;
    }
    let Some((date, rest)) = at.split_once('T') else {
        return false;
    };
    if !date_part(date) {
        return false;
    }
    let offset = if rest.ends_with("+02:00") {
        "+02:00"
    } else if rest.ends_with("+01:00") {
        "+01:00"
    } else {
        return false;
    };
    let time = &rest[..rest.len() - offset.len()];
    time_part(time)
}

fn date_part(date: &str) -> bool {
    let bytes = date.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    bytes.iter().enumerate().all(|(index, byte)| {
        if index == 4 || index == 7 {
            *byte == b'-'
        } else {
            byte.is_ascii_digit()
        }
    })
}

fn time_part(time: &str) -> bool {
    let (hms, frac) = match time.split_once('.') {
        Some((hms, frac)) => (hms, Some(frac)),
        None => (time, None),
    };
    let bytes = hms.as_bytes();
    if bytes.len() != 8 || bytes[2] != b':' || bytes[5] != b':' {
        return false;
    }
    if !bytes.iter().enumerate().all(|(index, byte)| {
        if index == 2 || index == 5 {
            *byte == b':'
        } else {
            byte.is_ascii_digit()
        }
    }) {
        return false;
    }
    match frac {
        None => true,
        Some(frac) => !frac.is_empty() && frac.bytes().all(|byte| byte.is_ascii_digit()),
    }
}
