// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

#![forbid(unsafe_code)]

use galleon_arena::{Board, BoutError, Match, Score, ScoreError, SeatReport};

fn stamp() -> &'static str {
    "2026-10-19T16:21:00+02:00"
}

fn bout(base: i64, seats: &[&str]) -> Match {
    Match::try_new(
        "paper-1",
        stamp(),
        base,
        "Sealed task. Answer from this home only.",
        seats.iter().map(|id| (*id).to_string()).collect(),
        Vec::new(),
    )
    .unwrap()
}

fn seat(id: &str, n: u32, passed: u32) -> SeatReport {
    SeatReport {
        gladiator_id: id.to_string(),
        n,
        passed,
        disqualified: false,
        silent: false,
        agreed: 0,
        team: false,
    }
}

fn line<'a>(board: &'a Board, id: &str) -> &'a Score {
    board
        .lines
        .iter()
        .find(|line| line.gladiator_id == id)
        .unwrap_or_else(|| panic!("missing line {id}"))
}

#[test]
fn design_table_base_100_solo_n_7() {
    let cases = [
        (6_u32, 1200_i64, 500_i64, true),
        (4, 800, 100, true),
        (3, 600, -100, false),
    ];
    for (passed, result_num, paper_num, profitable) in cases {
        let board = bout(100, &["g2-0"])
            .score(&[seat("g2-0", 7, passed)])
            .unwrap();
        let row = line(&board, "g2-0");
        assert_eq!(row.result_num, result_num, "passed {passed}");
        assert_eq!(row.result_den, 7, "passed {passed}");
        assert_eq!(row.paper_num, paper_num, "passed {passed}");
        assert_eq!(row.paper_den, 7, "passed {passed}");
        assert_eq!(row.profitable, profitable, "passed {passed}");
        assert_eq!(board.winner.as_deref(), Some("g2-0"));
        assert_eq!(board.profitable, profitable);
    }
}

#[test]
fn base_size_does_not_flip_the_bit_and_fraction_is_not_truncated() {
    let small = bout(1, &["g2-0"]).score(&[seat("g2-0", 7, 4)]).unwrap();
    let row = line(&small, "g2-0");
    assert_eq!(
        (row.result_num, row.result_den, row.paper_num, row.paper_den),
        (8, 7, 1, 7)
    );
    assert!(row.profitable);
    assert!(small.profitable);
    let large = bout(100, &["g2-0"]).score(&[seat("g2-0", 7, 4)]).unwrap();
    assert_eq!(line(&large, "g2-0").profitable, row.profitable);
}

#[test]
fn break_even_is_not_a_success() {
    let board = bout(100, &["g2-0"]).score(&[seat("g2-0", 4, 2)]).unwrap();
    let row = line(&board, "g2-0");
    assert_eq!(row.paper_num, 0);
    assert_eq!(row.paper_den, 4);
    assert!(!row.profitable);
    assert_eq!(board.winner.as_deref(), Some("g2-0"));
    assert!(!board.profitable);
}

#[test]
fn tie_on_bits_uses_the_smaller_id() {
    let mut first = seat("g2-0", 7, 4);
    let mut second = seat("g2-2", 7, 4);
    first.agreed = 0;
    second.agreed = 0;
    let board = bout(100, &["g2-2", "g2-0"])
        .score(&[second, first])
        .unwrap();
    assert_eq!(board.winner.as_deref(), Some("g2-0"));
    assert!(line(&board, "g2-0").profitable);
    assert!(line(&board, "g2-2").profitable);
    assert!(board.profitable);
}

#[test]
fn tie_on_bits_prefers_the_seat_others_agreed_with() {
    let mut plain = seat("g2-0", 7, 4);
    let mut steam = seat("g2-2", 7, 4);
    plain.agreed = 0;
    steam.agreed = 2;
    let board = bout(100, &["g2-0", "g2-2"]).score(&[plain, steam]).unwrap();
    assert_eq!(board.winner.as_deref(), Some("g2-2"));
}

#[test]
fn rank_key_is_not_raw_paper_numerator() {
    let low_n = seat("g2-0", 4, 3);
    let high_n = seat("g2-1", 7, 5);
    let board = bout(100, &["g2-0", "g2-1"])
        .score(&[low_n, high_n])
        .unwrap();
    assert_eq!(line(&board, "g2-0").paper_num, 200);
    assert_eq!(line(&board, "g2-1").paper_num, 300);
    assert_eq!(board.winner.as_deref(), Some("g2-0"));
}

#[test]
fn disqualified_is_a_total_paper_loss_and_cannot_win() {
    let mut bad = seat("g2-1", 7, 7);
    bad.disqualified = true;
    let good = seat("g2-0", 7, 4);
    let board = bout(100, &["g2-0", "g2-1"]).score(&[bad, good]).unwrap();
    let lost = line(&board, "g2-1");
    assert_eq!(
        (
            lost.result_num,
            lost.result_den,
            lost.paper_num,
            lost.paper_den,
            lost.profitable
        ),
        (0, 1, -100, 1, false)
    );
    assert_eq!(board.winner.as_deref(), Some("g2-0"));
    let mut only_bad = seat("g2-1", 7, 7);
    only_bad.disqualified = true;
    let only = bout(100, &["g2-1"]).score(&[only_bad]).unwrap();
    assert!(only.winner.is_none());
    assert!(!only.profitable);
}

#[test]
fn silent_seat_cannot_win_and_is_not_disqualified() {
    let mut quiet = seat("g2-3", 7, 7);
    quiet.silent = true;
    let board = bout(100, &["g2-0", "g2-3"])
        .score(&[quiet, seat("g2-0", 7, 4)])
        .unwrap();
    let row = line(&board, "g2-3");
    assert!(row.silent);
    assert!(!row.disqualified);
    assert!(row.profitable);
    assert_eq!(board.winner.as_deref(), Some("g2-0"));
}

#[test]
fn team_line_is_recorded_and_not_ranked() {
    let mut team = seat("arena-compare", 5, 5);
    team.team = true;
    let board = bout(100, &["g2-0", "g2-1"])
        .score(&[team, seat("g2-0", 7, 4), seat("g2-1", 7, 3)])
        .unwrap();
    assert!(line(&board, "arena-compare").profitable);
    assert_eq!(board.lines.len(), 3);
    assert_eq!(board.winner.as_deref(), Some("g2-0"));
}

#[test]
fn zero_checks_are_left_out_of_the_ranking() {
    let board = bout(100, &["g2-0", "g2-1"])
        .score(&[seat("g2-0", 0, 0), seat("g2-1", 7, 4)])
        .unwrap();
    assert_eq!(board.lines.len(), 1);
    assert_eq!(board.winner.as_deref(), Some("g2-1"));
}

#[test]
fn same_bits_are_deterministic() {
    let reports = [seat("g2-0", 7, 6), seat("g2-2", 7, 4)];
    let first = bout(100, &["g2-0", "g2-2"]).score(&reports).unwrap();
    let second = bout(100, &["g2-0", "g2-2"]).score(&reports).unwrap();
    assert_eq!(first, second);
}

#[test]
fn no_eligible_seat_has_no_winner() {
    let mut silent = seat("g2-0", 7, 7);
    silent.silent = true;
    let mut lost = seat("g2-1", 7, 7);
    lost.disqualified = true;
    let board = bout(100, &["g2-0", "g2-1"]).score(&[silent, lost]).unwrap();
    assert!(board.winner.is_none());
    assert!(!board.profitable);
}

#[test]
fn rejects_bad_base_stamp_and_bits() {
    let err = Match::try_new(
        "m",
        stamp(),
        0,
        "task",
        vec!["g2-0".to_string()],
        Vec::new(),
    );
    assert!(matches!(err, Err(BoutError::BaseBelowOne(0))));
    let err = Match::try_new(
        "m",
        "2026-10-19T16:21:00Z",
        100,
        "task",
        vec!["g2-0".to_string()],
        Vec::new(),
    );
    assert!(matches!(err, Err(BoutError::BadStamp(_))));
    let err = Match::try_new(
        "m",
        "2026-10-19T16:21:00+00:00",
        100,
        "task",
        vec!["g2-0".to_string()],
        Vec::new(),
    );
    assert!(matches!(err, Err(BoutError::BadStamp(_))));
    let winter = Match::try_new(
        "m",
        "2026-01-15T08:00:00+01:00",
        1,
        "task",
        vec!["g2-0".to_string()],
        Vec::new(),
    );
    assert!(winter.is_ok());
    let err = bout(100, &["g2-0"]).score(&[seat("g2-0", 7, 8)]);
    assert!(matches!(
        err,
        Err(ScoreError::PassedAboveN {
            passed: 8,
            n: 7,
            ..
        })
    ));
    let err = bout(100, &["g2-0"]).score(&[seat("g2-1", 7, 4)]);
    assert!(matches!(err, Err(ScoreError::UnknownSeat(_))));
    let err = bout(i64::MAX, &["g2-0"]).score(&[seat("g2-0", 3, 2)]);
    assert!(matches!(err, Err(ScoreError::Overflow)));
}
