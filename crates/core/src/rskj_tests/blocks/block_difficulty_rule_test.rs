//! rskj `co.rsk.validators.BlockDifficultyRuleTest`: a block's difficulty
//! is the one `DifficultyCalculator` gives for it and its parent, and no
//! other (`BlockDifficultyRule`).

use super::support::header;
use crate::config::ChainConfig;
use crate::validation::{DifficultyRule, ParentHeaderValidator, ValidationError};
use alloy_primitives::U256;
use std::sync::Arc;

/// rskj `invalidWhenCalculatedDifficultyDoesntMatch`: on regtest a block
/// five seconds after a parent of difficulty 20,480 must carry 20,490
/// (+1/2048, inside the ten-second duration); one more or one less is
/// refused.
#[test]
fn a_difficulty_other_than_the_calculated_one_is_refused() {
    let rule = DifficultyRule {
        config: Arc::new(ChainConfig::regtest()),
    };
    let mut parent = header(10, 1_000);
    parent.difficulty = U256::from(20_480);
    let child = |difficulty: u64| {
        let mut child = header(11, 1_005);
        child.difficulty = U256::from(difficulty);
        rule.validate_with_parent(&child, &parent)
    };
    assert_eq!(child(20_490), Ok(()));
    for got in [20_491, 20_489] {
        assert_eq!(
            child(got),
            Err(ValidationError::DifficultyMismatch {
                expected: U256::from(20_490),
                got: U256::from(got),
            })
        );
    }
}
