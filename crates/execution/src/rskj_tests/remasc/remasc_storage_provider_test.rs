//! rskj's `co.rsk.remasc.RemascStorageProviderTest`, its tests of RSKIP85
//! (orchid): without it REMASC pays every block, the federation too; with
//! it a reward under the minimum gas price times the minimum payable gas
//! waits in the reward balance, and a federator's share under the minimum
//! gas price times the federator's minimum accrues in the federation's.

use alloy_primitives::{hex, Address, B256, U256};
use rustock_core::{Header, Transaction};

use super::runner::{
    burned_balance, federation_balance, reward_balance, run, run_without_rskip85, without_rskip85,
};
use crate::bridge::constants::BridgeConstants;
use crate::bridge::federation::rsk_address_from_public_key;
use crate::hardfork::RskHardforkConfig;
use crate::precompiles::REMASC_ADDR;
use crate::remasc::RemascConfig;
use crate::rskj_tests::vm::harness::{Chain, ONE_RBTC};

/// The first federator of the federation REMASC pays, the Bridge's genesis
/// federation, and how many federators it has.
fn first_federator() -> (Address, u64) {
    let constants = BridgeConstants::mainnet();
    let mut keys: Vec<Vec<u8>> = constants
        .genesis_federation_public_keys
        .iter()
        .map(|key| hex::decode(key).expect("hex"))
        .collect();
    keys.sort();
    let first = rsk_address_from_public_key(&keys[0]).expect("a key");
    (first, keys.len() as u64)
}

/// The runner with RSKIP85 on, at `gas_price`, the minimum payable gas
/// `reward` and the federator's minimum payable gas `federator`.
fn with_rskip85(gas_price: u64, reward: u64, federator: u64) -> Chain {
    let mut remasc = RemascConfig::regtest();
    remasc.minimum_payable_gas = reward;
    remasc.federator_minimum_payable_gas = federator;
    let hardforks = RskHardforkConfig::rskj_regtest();
    run(hardforks, remasc, gas_price, &[], |_| {})
}

/// Without RSKIP85 REMASC pays at blocks 15 and 16, and after block 16 its
/// reward balance holds 84,000 and nothing is burned.
#[test]
fn without_rskip85_remasc_pays_every_block() {
    let chain = run_without_rskip85(&[]);
    let test = "alwaysPaysBeforeRFS";
    assert_eq!(reward_balance(&chain), U256::from(84_000), "{test}");
    assert_eq!(burned_balance(&chain), U256::ZERO, "{test}");
}

/// Transfers at a gas price of 0 pay no fees: with siblings of blocks 6 and
/// 11, which blocks 7 and 12 include, REMASC's reward and burned balances
/// stay at 0.
#[test]
fn without_fees_remasc_keeps_nothing_for_blocks_with_siblings() {
    let hardforks = RskHardforkConfig::rskj_regtest();
    let chain = run(hardforks, without_rskip85(), 0, &[(6, 7), (11, 12)], |_| {});
    let test = "setSaveRetrieveAndGetSiblingsBeforeRFS";
    assert_eq!(reward_balance(&chain), U256::ZERO, "{test}");
    assert_eq!(burned_balance(&chain), U256::ZERO, "{test}");
}

/// Without RSKIP85 the federation is paid its 168 at blocks 15 and 16, and
/// REMASC keeps none of it: the first federator gets its share of each.
#[test]
fn without_rskip85_the_federation_is_paid_every_block() {
    let test = "alwaysPaysFedBeforeRFS";
    let chain = run_without_rskip85(&[]);
    assert_eq!(federation_balance(&chain), U256::ZERO, "{test}");
    let (first, federators) = first_federator();
    let share = 168 / federators;
    assert_eq!(chain.balance(first), U256::from(2 * share), "{test}");
}

/// With RSKIP85 a reward under the floor waits: at a floor of 100,000 (rskj's
/// 10,000 gas at its runner's minimum gas price of 10; here 100,000 at the
/// blocks' minimum of 1) block 16's 21,000 is not paid, and the reward
/// balance holds all six blocks' fees.
#[test]
fn from_rskip85_a_reward_under_its_floor_waits_in_the_reward_balance() {
    let chain = with_rskip85(1, 100_000, 0);
    let test = "doesntPayBelowMinimumRewardAfterRFS";
    assert_eq!(reward_balance(&chain), U256::from(126_000), "{test}");
    assert_eq!(burned_balance(&chain), U256::ZERO, "{test}");
}

/// With RSKIP85 a reward at its floor is paid: at a gas price of 10 each
/// block pays 210,000, and 21,000 gas at 10 is that floor.
#[test]
fn from_rskip85_a_reward_at_its_floor_is_paid() {
    let chain = with_rskip85(10, 21_000, 10);
    let test = "paysWhenHigherThanMinimumRewardAfterRFS";
    assert_eq!(reward_balance(&chain), U256::from(840_000), "{test}");
    assert_eq!(burned_balance(&chain), U256::ZERO, "{test}");
}

/// With RSKIP85 a federator's share under its floor accrues: the
/// federation's 168 of blocks 15 and 16, against a floor of 10,000, waits
/// as 336, and the first federator has no account.
#[test]
fn from_rskip85_a_federators_share_under_its_floor_accrues() {
    let chain = with_rskip85(1, 0, 10_000);
    let test = "doesntPayFedBelowMinimumRewardAfterRFS";
    assert_eq!(federation_balance(&chain), U256::from(336), "{test}");
    assert!(chain.account(first_federator().0).is_none(), "{test}");
}

/// With RSKIP85 a federator's share over its floor is paid: at a gas price
/// of 10 the federation gets 1,680 at blocks 15 and 16, against a floor of
/// 100.
#[test]
fn from_rskip85_a_federators_share_over_its_floor_is_paid() {
    let chain = with_rskip85(10, 0, 10);
    let test = "paysFedWhenHigherThanMinimumRewardAfterRFS";
    assert_eq!(federation_balance(&chain), U256::ZERO, "{test}");
    let (first, federators) = first_federator();
    let share = 1_680 / federators;
    assert_eq!(chain.balance(first), U256::from(2 * share), "{test}");
}

/// rskj's runner to block 14 on one coinbase at a gas price of 10, then
/// blocks 15 to 18, whose transfers pay 210,000, 21,000, 210,000 and 21,000
/// in fees, and ten more with none, each at a minimum gas price of 10:
/// RSKIP85's floor is 21,000 gas at that price, 210,000. The transfers of
/// blocks 16 and 18, at a gas price of 1, are under their blocks' minimum,
/// which consensus refuses and rustock's execution too; REMASC reads no more
/// of a block than its fees, so here each block carries its fees and
/// REMASC's transaction alone. By block 25 ten blocks are paid, 166,320
/// each; block 26 holds back block 16's reward, 172,200 being under the
/// floor; block 27 pays 169,647 of 214,200; block 28 holds back 175,560.
#[test]
fn from_rskip85_only_rewards_that_reach_the_floor_are_paid() {
    let test = "paysOnlyBlocksWithEnoughBalanceAccumulatedAfterRFS";
    let mut remasc = RemascConfig::regtest();
    remasc.minimum_payable_gas = 21_000;
    let hardforks = RskHardforkConfig::rskj_regtest();
    let mut chain =
        Chain::new(hardforks, 1).map_executor(|executor| executor.with_remasc_config(remasc));
    chain.put(REMASC_ADDR, 0, ONE_RBTC, &[]);
    let coinbase = Address::repeat_byte(0xcb);
    let mut parent = B256::ZERO;
    for number in 1..=28 {
        let fees = match number {
            16 | 18 => 21_000,
            19..=28 => 0,
            _ => 210_000,
        };
        let header = Header {
            parent_hash: parent,
            beneficiary: coinbase,
            number,
            paid_fees: U256::from(fees),
            minimum_gas_price: U256::from(10),
            ..chain.header()
        };
        let remasc = [(Transaction::remasc(number), Address::ZERO)];
        chain.import(&header, &[], &remasc);
        parent = header.hash();
        let paid = match number {
            24 => 1_663_200,
            25 | 26 => 1_829_520,
            27 | 28 => 1_999_167,
            _ => continue,
        };
        let balance = chain.balance(coinbase);
        assert_eq!(balance, U256::from(paid), "{test} at block {number}");
    }
}
