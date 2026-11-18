//! rskj's `org.ethereum.core.TransactionExecutorInitCodeSizeDslTest`: from
//! RSKIP438 (lovell700) a transaction that creates a contract carries 2 gas
//! for each word of its init code on top of its intrinsic gas
//! (`Transaction.transactionCost`), and one of more than 49,152 bytes of it
//! is invalid (`Transaction.isInitCodeSizeInvalidForTx`): the miner leaves
//! it out of its block, and a block that holds it is refused.

use alloy_primitives::U256;
use rustock_core::Transaction;

use super::harness::{create, Chain, GAS, ONE_RBTC, SENDER};
use crate::hardfork::RskHardforkConfig;

/// lovell700 on mainnet: RSKIP438.
const LOVELL: u64 = 7_338_024;
/// The most init code RSKIP438 lets a transaction carry.
const MAX_INITCODE: usize = 49_152;
/// The gas of a creation of `MAX_INITCODE` zero bytes before RSKIP438: 53,000
/// and 4 for each byte. Its init code is a STOP, which runs for nothing.
const MAX_CREATION_GAS: u64 = 53_000 + 4 * MAX_INITCODE as u64;

/// A chain on `config`'s rules, at block `number`, where the sender holds
/// `balance`.
fn chain(config: RskHardforkConfig, number: u64, balance: U256) -> Chain {
    let mut chain = Chain::new(config, number);
    chain.put(SENDER, 0, balance, &[]);
    chain
}

/// A creation of `size` zero bytes of init code, carrying `gas`.
fn creation(size: usize, gas: u64) -> [(Transaction, alloy_primitives::Address); 1] {
    [(create(0, gas, &vec![0; size]), SENDER)]
}

/// From RSKIP438 a creation pays 2 gas for each word of its init code:
/// 3,072 more for 49,152 bytes.
#[test]
fn from_rskip438_a_creation_pays_2_gas_a_word_of_its_init_code() {
    let test = "testInitCodeSizeTransactionCreationValidationSuccessWithRSKIPActivated";
    for (number, gas) in [
        (LOVELL - 1, MAX_CREATION_GAS),
        (LOVELL, MAX_CREATION_GAS + 3_072),
    ] {
        let mut chain = chain(RskHardforkConfig::mainnet(), number, ONE_RBTC);
        let ran = chain
            .block(&creation(MAX_INITCODE, GAS))
            .tx_results
            .remove(0);
        assert!(ran.success, "{test} #{number}");
        assert_eq!(ran.gas_used, gas, "{test} #{number}");
    }
}

/// A creation whose sender cannot pay for the gas it carries, at its price,
/// is invalid: rskj's scenario gives the sender 4,322,017 for 4,323,189 gas
/// at a price of 1, and the miner leaves it out of the block.
#[test]
fn a_creation_whose_sender_cannot_pay_for_its_gas_is_left_out_of_the_block() {
    let chain = chain(RskHardforkConfig::mainnet(), LOVELL, U256::from(4_322_017));
    let [(transaction, sender)] = creation(1, 4_323_189);
    let priced = Transaction {
        gas_price: U256::from(1),
        ..transaction
    };
    let mined = chain.mine(&[(priced, sender)]);
    let (result, drops) = mined.expect("the miner builds the block");
    let test = "testInitCodeSizeTransactionCreationValidationFailsNotEnoughGas";
    assert_eq!(drops.invalid, [0], "{test}");
    assert!(result.tx_results.is_empty(), "{test}");
}

/// From RSKIP438 a creation of more than 49,152 bytes of init code is
/// invalid: the miner leaves it out, and a block that holds it is refused.
#[test]
fn from_rskip438_a_creation_of_more_than_49152_bytes_of_init_code_is_invalid() {
    let test = "testInitCodeSizeTransactionCreationValidationFailsWithRSKIPActiveWithInitcodeSizeMaxReached";
    let mut chain = chain(RskHardforkConfig::mainnet(), LOVELL, ONE_RBTC);
    let past = creation(MAX_INITCODE + 1, GAS);
    let (_, drops) = chain.mine(&past).expect("the miner builds the block");
    assert_eq!(drops.invalid, [0], "{test}");
    assert!(chain.try_block(&past).is_err(), "{test}");
    assert!(
        chain.block(&creation(MAX_INITCODE, GAS)).tx_results[0].success,
        "{test}"
    );
}

/// Before RSKIP438 a creation carries init code of any size, and no gas for
/// it: one of 49,152 bytes needs no more than its bytes' gas, and one byte
/// more is valid.
#[test]
fn before_rskip438_a_creation_carries_init_code_of_any_size_and_no_gas_for_it() {
    let run = |size, gas| {
        let mut chain = chain(RskHardforkConfig::mainnet(), LOVELL - 1, ONE_RBTC);
        chain.block(&creation(size, gas)).tx_results.remove(0)
    };
    let exact = run(MAX_INITCODE, MAX_CREATION_GAS);
    assert!(exact.success);
    assert_eq!(exact.gas_used, MAX_CREATION_GAS);
    let past = run(MAX_INITCODE + 1, GAS);
    assert!(past.success);
    assert_eq!(past.gas_used, MAX_CREATION_GAS + 4);
}
