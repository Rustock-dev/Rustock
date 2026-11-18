//! rskj `co.rsk.validators.BlockParentNumberRuleTest`: a block's number is
//! its parent's plus one (`BlockParentNumberRule`).

use super::support::header;
use crate::validation::{BlockNumberRule, ParentHeaderValidator, ValidationError};

/// rskj `isValid`: against a parent at 451, 452 is valid, and the parent's
/// own number, the one below it and one far above are not.
#[test]
fn a_block_is_numbered_one_past_its_parent() {
    let parent = header(451, 1_000);
    let child = |number: u64| BlockNumberRule.validate_with_parent(&header(number, 1_010), &parent);
    assert_eq!(child(452), Ok(()));
    for got in [451, 450, 999] {
        assert_eq!(
            child(got),
            Err(ValidationError::InvalidBlockNumber { expected: 452, got })
        );
    }
}
