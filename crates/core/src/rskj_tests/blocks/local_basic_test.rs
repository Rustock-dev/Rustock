//! rskj `co.rsk.jsontestsuite.LocalBasicTest`: the difficulty cases of
//! rskj's `json/BasicTests/difficulty*.json`, each a parent, a child's
//! timestamp and number, and the difficulty rskj gives the child
//! (`DifficultyCalculator.calcDifficulty`).

use super::support::header;
use crate::config::{ActivationHeights, ChainConfig};
use crate::validation::DifficultyRule;
use alloy_primitives::U256;
use std::sync::Arc;

/// The cases of `difficulty.json` and `difficultyBeforeRSKIP156.json`,
/// which share them: the parent's timestamp and difficulty, and the child's
/// timestamp and number.
#[rustfmt::skip]
const CASES: [(&str, u64, u64, u64, u64); 10] = [
    ("preExpDiffIncrease", 42, 10_000_000_000_000_000, 43, 42),
    ("preExpDiffDecrease", 42, 10_000_000_000_000_000, 60, 42),
    ("DifficultyTest1", 0xff6f9889aec4a8ab, 0xe5f6f17a6cfb97a9, 0xff6f9889aec4a8ab, 0x01),
    ("DifficultyTest2", 0x4807463f1b55475e, 0xa9c932bb020082a4, 0x4807463f1b55475e, 0x61a9),
    ("DifficultyTest3", 0x39636370b8492dde, 0x24a37089ce303762, 0x39636370b8492dde, 0xc351),
    ("DifficultyTest4", 0x894395be53be575d, 0xd21e04041ea258f6, 0x894395be53be575d, 0x0124f9),
    ("DifficultyTest5", 0x85d238e2ee51c214, 0x734df3bd1e36dc09, 0x85d238e2ee51c214, 0x0186a1),
    ("DifficultyTest6", 0x4c9b2b0a61e65adb, 0xb0e35f896c8e0280, 0x4c9b2b0a61e65adb, 0x01e849),
    ("DifficultyTest7", 0x701109b82158d260, 0xf9e6aa1a79f7bc1d, 0x701109b82158d260, 0x0249f1),
    ("DifficultyTest8", 0x711f963840eae450, 0x12b73b0b5b0cf756, 0x711f963840eae450, 0x02ab99),
];

/// What `difficultyBeforeRSKIP156.json` expects of them: the parent's
/// difficulty and a fiftieth of it.
const BEFORE_RSKIP156: [u64; 10] = [
    10_200_000_000_000_000,
    9_800_000_000_000_000,
    0xEA905CB531B8F1BB,
    0xAD2E808B8C483355,
    0x255F0744E1AC0F87,
    0xD651D0E57B68278A,
    0x759C4FA74CE60965,
    0xB46D0A68552035C0,
    0xFEE62862AF9B68D5,
    0x13170E252ECAAA5C,
];

/// What `difficulty.json` expects of them: the parent's difficulty and a
/// four-hundredth of it, from RSKIP156.
const FROM_RSKIP156: [u64; 10] = [
    10_025_000_000_000_000,
    9_975_000_000_000_000,
    0xe68a1ee1c59342eb,
    0xaa35dc75134978ba,
    0x24bae361309fb266,
    0xd2a47da04a3b12c8,
    0x7397bf3a640cc1b4,
    0xb15494e549a048e8,
    0xfa8699e380ac31b4,
    0x12c3356e9584adb6,
];

/// `chain` with RSKIP97 on from genesis, as every case has it, and RSKIP156
/// and RSKIP290 on from genesis or never.
fn chain(chain: ChainConfig, rskip156: bool, rskip290: bool) -> ChainConfig {
    ChainConfig {
        activation_heights: ActivationHeights {
            rskip97: Some(0),
            papyrus200: if rskip156 { 0 } else { u64::MAX },
            rskip290: rskip290.then_some(0),
            ..chain.activation_heights.clone()
        },
        ..chain
    }
}

/// The difficulty `chain` gives a child at `number` and `timestamp` of a
/// parent at `parent_timestamp` of `parent_difficulty`, without uncles.
fn calculated(
    chain: &Arc<ChainConfig>,
    parent_timestamp: u64,
    parent_difficulty: u64,
    timestamp: u64,
    number: u64,
) -> U256 {
    let mut parent = header(number - 1, parent_timestamp);
    parent.difficulty = U256::from(parent_difficulty);
    let rule = DifficultyRule {
        config: chain.clone(),
    };
    rule.difficulty_for_child(&parent, number, timestamp, 0)
}

fn run(chain: ChainConfig, expected: &[u64; 10]) {
    let chain = Arc::new(chain);
    for ((name, parent_timestamp, parent_difficulty, timestamp, number), expected) in
        CASES.iter().zip(expected)
    {
        assert_eq!(
            calculated(
                &chain,
                *parent_timestamp,
                *parent_difficulty,
                *timestamp,
                *number
            ),
            U256::from(*expected),
            "{name}"
        );
    }
}

/// rskj `runDifficultyTestBeforeRSKIP156`: under mainnet's constants and
/// without RSKIP156, a child a second after its parent rises by a fiftieth
/// of the parent's difficulty, and one 18 seconds after falls by as much;
/// children at their parent's timestamp, which the cases put past 2^63,
/// rise.
#[test]
fn before_rskip156_the_difficulty_moves_by_a_fiftieth() {
    run(chain(ChainConfig::mainnet(), false, true), &BEFORE_RSKIP156);
}

/// rskj `runDifficultyTestBeforeRSKIP290`: under testnet's constants, with
/// RSKIP156 and without RSKIP290, it moves by a four-hundredth.
#[test]
fn from_rskip156_the_difficulty_moves_by_a_four_hundredth() {
    run(chain(ChainConfig::testnet(), true, false), &FROM_RSKIP156);
}

/// rskj `runDifficultyTest`, from the rest of the class: so it does under
/// mainnet's constants with every rule on.
#[test]
fn on_mainnet_from_rskip156_the_difficulty_moves_by_a_four_hundredth() {
    run(chain(ChainConfig::mainnet(), true, true), &FROM_RSKIP156);
}

/// rskj `runDifficultyTestAfterRSKIP290`: on testnet from RSKIP290 a child
/// whose difficulty would rise to 218,359,490 is raised to the floor of
/// 550,000,000 (`difficulty1.json`).
#[test]
fn from_rskip290_testnets_difficulty_is_floored_at_550_million() {
    let testnet = Arc::new(chain(ChainConfig::testnet(), true, true));
    let at_floor = calculated(
        &testnet,
        0xff6f9889aec4a8ab,
        0xcfb97a9,
        0xff6f9889aec4a8ab,
        1,
    );
    assert_eq!(at_floor, U256::from(0x20C85580u64));
    assert_eq!(at_floor, U256::from(550_000_000u64));
}
