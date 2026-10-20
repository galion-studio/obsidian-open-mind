// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

/// One judge-filled row. The scorer does not read prose.
///
/// `n == 0` leaves the seat unscored. `disqualified` is set when the guard
/// check or the no-secret check failed. `silent` means a team round failed
/// the communicates check. `agreed` is how often other replies named this
/// seat's change. `team` marks the board line, which is recorded and not ranked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatReport {
    pub gladiator_id: String,
    pub n: u32,
    pub passed: u32,
    pub disqualified: bool,
    pub silent: bool,
    pub agreed: u32,
    pub team: bool,
}

/// One score line. Fractions stay unreduced. `winner` is not a field here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Score {
    pub gladiator_id: String,
    pub n: u32,
    pub passed: u32,
    pub disqualified: bool,
    pub silent: bool,
    pub result_num: i64,
    pub result_den: i64,
    pub paper_num: i64,
    pub paper_den: i64,
    pub profitable: bool,
}

/// The match after scoring. `winner` lives on the board, not on each line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    pub lines: Vec<Score>,
    /// Seat id of the winner, if any seat may win. Never a team line.
    pub winner: Option<String>,
    /// True only when there is a winner and that line has `paper_num > 0`.
    pub profitable: bool,
}

/// Why scoring stopped. No partial board is returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScoreError {
    EmptyId,
    PassedAboveN { id: String, passed: u32, n: u32 },
    DuplicateSeat(String),
    UnknownSeat(String),
    Overflow,
}

impl std::fmt::Display for ScoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScoreError::EmptyId => write!(f, "gladiator id is empty"),
            ScoreError::PassedAboveN { id, passed, n } => {
                write!(f, "seat {id} passed {passed} of {n}")
            }
            ScoreError::DuplicateSeat(id) => write!(f, "duplicate seat {id}"),
            ScoreError::UnknownSeat(id) => write!(f, "seat {id} is not on this match"),
            ScoreError::Overflow => write!(f, "paper score overflowed"),
        }
    }
}

impl std::error::Error for ScoreError {}

/// Score `reports` against `base` for the seats listed on the match.
///
/// Not disqualified and `n > 0`:
///
/// - `result_num = base * 2 * passed`, `result_den = n`
/// - `paper_num = base * (2 * passed - n)`, `paper_den = n`
/// - profitable when `paper_num > 0` (exactly when `2 * passed > n`)
///
/// Disqualified: result `0/1`, paper `-base/1`, not profitable.
///
/// Rank key is the unreduced pair `(2 * passed, n)`, compared by the sign of
/// `a * d - c * b`. Ties break by higher `agreed`, then by the smaller id.
/// Unscored, silent, disqualified, and team rows cannot win.
///
/// # Errors
///
/// See [`ScoreError`].
pub fn score_match(
    base: i64,
    match_seats: &[String],
    reports: &[SeatReport],
) -> Result<Board, ScoreError> {
    check_reports(reports)?;
    for report in reports {
        if report.team {
            continue;
        }
        if !match_seats.iter().any(|id| id == &report.gladiator_id) {
            return Err(ScoreError::UnknownSeat(report.gladiator_id.clone()));
        }
    }
    let mut lines = Vec::new();
    for report in reports {
        if report.n == 0 {
            continue;
        }
        lines.push(score_one(base, report)?);
    }
    let winner = pick_winner(reports);
    let profitable = winner.as_ref().is_some_and(|id| {
        lines
            .iter()
            .find(|line| &line.gladiator_id == id)
            .is_some_and(|line| line.paper_num > 0)
    });
    Ok(Board {
        lines,
        winner,
        profitable,
    })
}

fn check_reports(reports: &[SeatReport]) -> Result<(), ScoreError> {
    let mut seen = Vec::with_capacity(reports.len());
    for report in reports {
        if report.gladiator_id.is_empty() {
            return Err(ScoreError::EmptyId);
        }
        if report.passed > report.n {
            return Err(ScoreError::PassedAboveN {
                id: report.gladiator_id.clone(),
                passed: report.passed,
                n: report.n,
            });
        }
        if seen.iter().any(|id| id == &report.gladiator_id) {
            return Err(ScoreError::DuplicateSeat(report.gladiator_id.clone()));
        }
        seen.push(report.gladiator_id.clone());
    }
    Ok(())
}

fn score_one(base: i64, report: &SeatReport) -> Result<Score, ScoreError> {
    let (result_num, result_den, paper_num, paper_den, profitable) = if report.disqualified {
        let paper_num = base.checked_neg().ok_or(ScoreError::Overflow)?;
        (0, 1, paper_num, 1, false)
    } else {
        let n = i64::from(report.n);
        let passed = i64::from(report.passed);
        let twice = passed.checked_mul(2).ok_or(ScoreError::Overflow)?;
        let result_num = base.checked_mul(twice).ok_or(ScoreError::Overflow)?;
        let delta = twice.checked_sub(n).ok_or(ScoreError::Overflow)?;
        let paper_num = base.checked_mul(delta).ok_or(ScoreError::Overflow)?;
        let profitable = paper_num > 0;
        (result_num, n, paper_num, n, profitable)
    };
    Ok(Score {
        gladiator_id: report.gladiator_id.clone(),
        n: report.n,
        passed: report.passed,
        disqualified: report.disqualified,
        silent: report.silent,
        result_num,
        result_den,
        paper_num,
        paper_den,
        profitable,
    })
}

fn pick_winner(reports: &[SeatReport]) -> Option<String> {
    let mut best: Option<&SeatReport> = None;
    for report in reports {
        if report.n == 0 || report.disqualified || report.silent || report.team {
            continue;
        }
        best = Some(match best {
            None => report,
            Some(current) => higher(current, report),
        });
    }
    best.map(|report| report.gladiator_id.clone())
}

/// Higher rank key, then higher `agreed`, then lexicographically smaller id.
fn higher<'a>(left: &'a SeatReport, right: &'a SeatReport) -> &'a SeatReport {
    let left_num = i128::from(left.passed) * 2;
    let right_num = i128::from(right.passed) * 2;
    let cross = left_num * i128::from(right.n) - right_num * i128::from(left.n);
    if cross > 0 {
        return left;
    }
    if cross < 0 {
        return right;
    }
    match right.agreed.cmp(&left.agreed) {
        std::cmp::Ordering::Greater => right,
        std::cmp::Ordering::Less => left,
        std::cmp::Ordering::Equal => {
            if right.gladiator_id < left.gladiator_id {
                right
            } else {
                left
            }
        }
    }
}
