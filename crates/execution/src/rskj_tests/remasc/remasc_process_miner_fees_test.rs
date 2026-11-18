//! rskj's `co.rsk.remasc.RemascProcessMinerFeesTest`, its tests that turn
//! RSKIP85 (orchid) off: what REMASC pays at block 16 for block 6 and its
//! siblings. Block 16 takes 21,000 of the reward balance's 105,000: 4,200
//! to RSK Labs, 168 to the federation, a tenth of the 16,632 left to the
//! miners that included the siblings, and the rest to block 6's miner and
//! the siblings' miners, a sibling losing a twentieth of its share for each
//! block it came in late (`Remasc.payWithSiblings`).

use alloy_primitives::U256;

use super::runner::{
    burned_balance, coinbase, reward_balance, run_without_rskip85, sibling_coinbase,
};
use crate::precompiles::REMASC_ADDR;

/// A sibling of block 6 that block 8 includes, a block later than it
/// could, loses a twentieth of its share.
#[test]
fn a_sibling_included_a_block_late_loses_a_twentieth_of_its_share() {
    let test = "siblingIncludedOneBlockLater";
    let chain = run_without_rskip85(&[(6, 8)]);
    assert_eq!(chain.balance(coinbase(8)), U256::from(1_663), "{test}");
    assert_eq!(chain.balance(coinbase(6)), U256::from(7_484), "{test}");
    let sibling = chain.balance(sibling_coinbase(0));
    assert_eq!(sibling, U256::from(7_110), "{test}");
    assert_eq!(reward_balance(&chain), U256::from(84_000), "{test}");
    assert_eq!(burned_balance(&chain), U256::from(375), "{test}");
}

/// Siblings of block 6 that blocks 7 and 8 include share the miners'
/// reward three ways, and the miners that included them the publishers'.
#[test]
fn siblings_included_by_two_blocks_share_the_reward_with_their_block() {
    let test = "oneSiblingIncludedOneBlockLaterAndAnotherIncludedRightAfter";
    let chain = run_without_rskip85(&[(6, 7), (6, 8)]);
    assert_eq!(chain.balance(coinbase(7)), U256::from(831), "{test}");
    assert_eq!(chain.balance(coinbase(8)), U256::from(831), "{test}");
    assert_eq!(chain.balance(coinbase(6)), U256::from(4_989), "{test}");
    let siblings = [0, 1].map(|index| chain.balance(sibling_coinbase(index)));
    assert_eq!(siblings, [4_989u64, 4_740].map(U256::from), "{test}");
    assert_eq!(reward_balance(&chain), U256::from(84_000), "{test}");
    assert_eq!(burned_balance(&chain), U256::from(252), "{test}");
}

/// A sibling of block 6 that block 13 includes loses six twentieths of its
/// share, which REMASC keeps as burned.
#[test]
fn a_sibling_included_six_blocks_late_loses_six_twentieths_of_its_share() {
    let test = "siblingIncludedSevenBlocksLater";
    let chain = run_without_rskip85(&[(6, 13)]);
    assert_eq!(chain.balance(coinbase(13)), U256::from(1_663), "{test}");
    assert_eq!(chain.balance(coinbase(6)), U256::from(7_484), "{test}");
    let sibling = chain.balance(sibling_coinbase(0));
    assert_eq!(sibling, U256::from(5_239), "{test}");
    assert_eq!(chain.balance(REMASC_ADDR), U256::from(296_246), "{test}");
    assert_eq!(reward_balance(&chain), U256::from(84_000), "{test}");
    assert_eq!(burned_balance(&chain), U256::from(2_246), "{test}");
}

/// Nine siblings of block 6 that block 7 includes: each share is rounded
/// down, and what the rounding leaves is burned. Block 8's miner, paid
/// nothing yet, has no account.
#[test]
fn nine_siblings_shares_are_rounded_down_and_the_surplus_burned() {
    let test = "siblingsFeeForMiningBlockMustBeRoundedAndTheRoundedSurplusBurned";
    let chain = run_without_rskip85(&[(6, 7); 9]);
    assert_eq!(chain.balance(coinbase(7)), U256::from(1_656), "{test}");
    assert!(chain.account(coinbase(8)).is_none(), "{test}");
    assert_eq!(chain.balance(coinbase(6)), U256::from(1_496), "{test}");
    let siblings = [0, 1].map(|index| chain.balance(sibling_coinbase(index)));
    assert_eq!(siblings, [U256::from(1_496); 2], "{test}");
    assert_eq!(reward_balance(&chain), U256::from(84_000), "{test}");
    assert_eq!(burned_balance(&chain), U256::from(16), "{test}");
}

/// Ten siblings of block 6 that block 7 includes: the publishers' shares
/// are rounded down too, and their surplus burned.
#[test]
fn ten_siblings_publishing_shares_are_rounded_down_and_the_surplus_burned() {
    let test = "unclesPublishingFeeMustBeRoundedAndTheRoundedSurplusBurned";
    let chain = run_without_rskip85(&[(6, 7); 10]);
    assert_eq!(chain.balance(coinbase(7)), U256::from(1_660), "{test}");
    assert_eq!(chain.balance(coinbase(6)), U256::from(1_360), "{test}");
    for index in 0..3 {
        let sibling = chain.balance(sibling_coinbase(index));
        assert_eq!(sibling, U256::from(1_360), "{test}");
    }
    assert_eq!(reward_balance(&chain), U256::from(84_000), "{test}");
    assert_eq!(burned_balance(&chain), U256::from(12), "{test}");
}
