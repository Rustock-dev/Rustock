//! rskj's `MinerClockTest`, for the clock a development chain's miner stamps
//! its blocks with: the wall clock plus what `evm_increaseTime` has added.
//! The offset is one value for the whole process, so these run in a test
//! binary of their own, where no other test's blocks read it, and in one test
//! function, so that none of them reads another's.

use rustock_execution::mining::dev::{increase_time, now_with_offset, reset_time, time_offset};

fn wall_clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// rskj `MinerClockTest.adjustTimeIfNotRegtest` and `clearTimeIncrease`: an
/// increase of 5,392 seconds puts the clock a child block is stamped by that
/// far ahead of the wall clock, and clearing it puts it back. Between them,
/// rskj's `MinerClock.increaseTime`: a move of no seconds or fewer changes
/// nothing and answers the offset as it stands.
#[test]
fn the_miners_clock_runs_ahead_by_what_was_added() {
    assert_eq!(time_offset(), 0);
    assert_eq!(increase_time(5392), 5392);
    let before = wall_clock();
    let clock = now_with_offset();
    let after = wall_clock();
    assert!(
        (before + 5392..=after + 5392).contains(&clock),
        "{clock} against the wall clock {before}..={after}"
    );

    assert_eq!(increase_time(0), 5392);
    assert_eq!(increase_time(-5392), 5392);
    assert_eq!(increase_time(i64::MIN), 5392);
    assert_eq!(time_offset(), 5392);

    reset_time();
    assert_eq!(time_offset(), 0);
    let before = wall_clock();
    let clock = now_with_offset();
    let after = wall_clock();
    assert!((before..=after).contains(&clock), "{clock} against the wall clock {before}..={after}");
}
