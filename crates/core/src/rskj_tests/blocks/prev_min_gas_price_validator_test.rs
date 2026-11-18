//! rskj `co.rsk.validators.PrevMinGasPriceValidatorTest`: a block's minimum
//! gas price moves at most 1% from its parent's (`PrevMinGasPriceRule`).

use super::support::header;
use crate::validation::{ParentHeaderValidator, PrevMinGasPriceRule, ValidationError};
use alloy_primitives::U256;

/// rskj `genesisBlock`: genesis has no parent to follow, so its minimum gas
/// price is never held to the band; the same price on block 1, against a
/// parent at 10, is out of it.
#[test]
fn genesis_is_held_to_no_parents_minimum_gas_price() {
    let mut parent = header(0, 0);
    parent.minimum_gas_price = U256::from(10);
    let at = |number: u64| {
        let mut block = header(number, 1);
        block.minimum_gas_price = U256::from(1_050);
        PrevMinGasPriceRule.validate_with_parent(&block, &parent)
    };
    assert_eq!(at(0), Ok(()));
    assert!(matches!(
        at(1),
        Err(ValidationError::MinGasPriceOutOfRange { .. })
    ));
}
