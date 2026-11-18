//! rskj `co.rsk.validators.BlockTxsValidationRuleTest`: each transaction of
//! a block carries its sender's next nonce, counting from the parent's state
//! up through the block (`BlockTxsValidationRule`). rskj refuses a block
//! that doesn't before it runs; rustock refuses it as it runs, when the
//! transaction's nonce isn't its sender's.

use super::support::run_block_on_mainnet;
use crate::executor::ExecutionError;
use alloy_primitives::Address;

/// A block at mainnet's #8,000,000.
const NUMBER: u64 = 8_000_000;

/// Whether a block of `transactions` runs where `accounts` hold their
/// nonces; a refusal must be the nonce's.
fn runs(accounts: &[(Address, u64)], transactions: &[(Address, u64)]) -> bool {
    match run_block_on_mainnet(NUMBER, accounts, transactions) {
        Ok(()) => true,
        Err(ExecutionError::Evm(why)) if why.to_lowercase().contains("nonce") => false,
        Err(other) => panic!("refused for another reason: {other}"),
    }
}

const SENDER: Address = Address::repeat_byte(0x5e);

/// rskj `validNonceSame`: a sender at 42 sends at 42.
#[test]
fn a_transaction_at_its_senders_nonce_is_valid() {
    assert!(runs(&[(SENDER, 42)], &[(SENDER, 42)]));
}

/// rskj `invalidNonceLower` and `invalidNonceHigher`: at 41 or 43 the
/// block is refused.
#[test]
fn a_transaction_below_or_above_its_senders_nonce_is_refused() {
    assert!(!runs(&[(SENDER, 42)], &[(SENDER, 41)]));
    assert!(!runs(&[(SENDER, 42)], &[(SENDER, 43)]));
}

/// rskj `validTransactionsWithSameNonceAndDifferentSenders`: two senders
/// at 64 each send at 64.
#[test]
fn two_senders_may_send_at_the_same_nonce() {
    let other = Address::repeat_byte(0x6f);
    assert!(runs(
        &[(SENDER, 64), (other, 64)],
        &[(SENDER, 64), (other, 64)]
    ));
}

/// rskj `validConsecutiveTransactionsFromSameSender`,
/// `invalidTransactionsFromSameSenderWithSkippedNonce` and
/// `invalidTransactionsWithSameNonceAndSameSender`: a sender at 42 sends at
/// 42 and 43, and not at 42 and 44, nor twice at 42.
#[test]
fn a_senders_nonce_counts_up_through_the_block() {
    assert!(runs(&[(SENDER, 42)], &[(SENDER, 42), (SENDER, 43)]));
    assert!(!runs(&[(SENDER, 42)], &[(SENDER, 42), (SENDER, 44)]));
    assert!(!runs(&[(SENDER, 42)], &[(SENDER, 42), (SENDER, 42)]));
}
