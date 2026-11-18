//! rskj's `co.rsk.vm.precompiles.blake2b.Blake2fNullDataTest`: a
//! transaction's own call of BLAKE2F with no data, which rskj reads as null,
//! before RSKIP552 (vetiver900) and from it.

use alloy_primitives::{Address, U256};
use rustock_core::Transaction;

use crate::rskj_tests::vm::harness::{call, Chain, ONE_RBTC, SENDER};

/// vetiver900 on mainnet: RSKIP552.
const VETIVER: u64 = 8_804_200;
/// BLAKE2F.
const BLAKE2F: Address = Address::with_last_byte(9);
/// The sender of the transaction that follows the call of BLAKE2F.
const OTHER_SENDER: Address = Address::repeat_byte(0xa2);

/// rskj's two transactions, from funded senders on a chain whose next block
/// is `number`: a call of BLAKE2F with no data, and a transfer of 1 wei from
/// another sender.
fn calls(number: u64) -> (Chain, [(Transaction, Address); 2]) {
    let mut chain = Chain::mainnet(number);
    chain.put(SENDER, 0, ONE_RBTC, &[]);
    chain.put(OTHER_SENDER, 0, ONE_RBTC, &[]);
    let to = Address::with_last_byte(2);
    let transactions = [
        (call(0, BLAKE2F, U256::ZERO, 30_000, &[]), SENDER),
        (call(0, to, U256::from(1), 30_000, &[]), OTHER_SENDER),
    ];
    (chain, transactions)
}

/// Before RSKIP552 rskj's miner leaves such a call out of the block it
/// builds, as `BlockExecutor` drops a transaction whose execution throws
/// (here BLAKE2F's NullPointerException), and the block holds the rest.
#[test]
fn before_rskip552_the_miner_leaves_out_a_transactions_call_of_blake2f_with_no_data() {
    let (chain, transactions) = calls(VETIVER - 1);
    let mined = chain.mine(&transactions);
    let (result, drops) = mined.expect("the miner builds the block");
    let test = "blake2fWithNullData_beforeRskip552_blockExecutorHandlesUnexpectedException";
    assert_eq!(drops.invalid, [0], "{test}");
    assert_eq!(result.tx_results.len(), 1, "{test}");
    assert!(result.tx_results[0].success, "{test}");
}

/// From RSKIP552 the call is refused for nothing, and the block holds it and
/// the transfer after it, which succeeds.
#[test]
fn from_rskip552_a_transactions_call_of_blake2f_with_no_data_leaves_its_block_valid() {
    let (mut chain, transactions) = calls(VETIVER);
    let block = chain.block(&transactions);
    let test = "blake2fWithNullData_shouldNotInvalidateBlock";
    assert_eq!(block.tx_results.len(), 2, "{test}");
    assert!(block.tx_results[1].success, "{test}");
}
