//! rskj `co.rsk.validators.BlockDifficultyValidationRuleTest`: the
//! difficulty a block must carry, from regtest's `DifficultyCalculator`.

use super::support::header;
use crate::config::ChainConfig;
use crate::validation::{DifficultyRule, ParentHeaderValidator, ValidationError};
use alloy_primitives::U256;
use std::sync::Arc;

/// rskj `testDifficulty`: a block ten seconds after a parent of difficulty
/// 2,048, naming one uncle, carries 2,049. rskj gives a block
/// `(1 + uncleCount) * duration` seconds before it counts as late, so the
/// uncle makes ten seconds early on regtest's ten-second duration: the
/// difficulty rises by 2,048 / 2,048. With no uncle the same ten seconds are
/// on time, and the difficulty stays at 2,048.
#[test]
fn an_uncle_doubles_the_time_a_block_has_before_it_is_late() {
    let rule = DifficultyRule {
        config: Arc::new(ChainConfig::regtest()),
    };
    let mut parent = header(0, 0);
    parent.difficulty = U256::from(2_048);
    let child = |uncle_count: u64| {
        let mut child = header(1, 10);
        child.uncle_count = uncle_count;
        child.difficulty = U256::from(2_049);
        rule.validate_with_parent(&child, &parent)
    };
    assert_eq!(child(1), Ok(()));
    assert_eq!(
        child(0),
        Err(ValidationError::DifficultyMismatch {
            expected: U256::from(2_048),
            got: U256::from(2_049),
        })
    );
}
