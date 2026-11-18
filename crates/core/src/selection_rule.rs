//! Which of two blocks is the best: rskj's `SelectionRule.shouldWeAddThisBlock`.
//!
//! rskj asks it of every block it connects, its own mined blocks included,
//! against the best block it holds (`BlockChainImpl.internalTryToConnect`,
//! VETIVER-9.0.4). The heavier chain wins. Two chains of the same total
//! difficulty are told apart by their blocks' fees and then their hashes, so
//! that nodes take the same block whichever of the two reached them first: the
//! newcomer wins if it paid more than twice what the best block paid, or else
//! if the best block paid less than twice what the newcomer paid and the
//! newcomer's hash is the lower. Two blocks that paid nothing keep the one held
//! first.
//!
//! REMASC holds miners to the same rule after the fact
//! (`SelectionRule.isBrokenSelectionRule`, in `rustock_execution::remasc`): a
//! block built on one that the rule wouldn't have chosen has its reward cut.

use alloy_primitives::{B256, U256};
use std::cmp::Ordering;

/// What the rule weighs of a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Contender {
    /// Its total difficulty: its parent's, its own difficulty and its uncles'.
    pub total_difficulty: U256,
    /// The fees its header says it paid.
    pub paid_fees: U256,
    /// Its hash.
    pub hash: B256,
}

/// Whether `block`, just connected, takes the best block's place from
/// `current`: rskj's `SelectionRule.shouldWeAddThisBlock`.
pub fn should_we_add_this_block(block: &Contender, current: &Contender) -> bool {
    match block.total_difficulty.cmp(&current.total_difficulty) {
        Ordering::Greater => return true,
        Ordering::Less => return false,
        Ordering::Equal => {}
    }
    // rskj: "fees over PAID_FEES_MULTIPLIER_CRITERIA times higher".
    if more_than_twice(block.paid_fees, current.paid_fees) {
        return true;
    }
    // rskj: "As a last resort, choose the block with the lower hash. We ask
    // that the fees are at least bigger than the half of current block."
    less_than_twice(current.paid_fees, block.paid_fees)
        && is_this_block_hash_smaller(&block.hash, &current.hash)
}

/// rskj's `SelectionRule.isThisBlockHashSmaller`: the 32 bytes compared first
/// to last, each as an unsigned byte (`FastByteComparisons.compareTo`).
pub fn is_this_block_hash_smaller(this: &B256, other: &B256) -> bool {
    this.as_slice() < other.as_slice()
}

/// `fees > 2 * of`, as rskj's `Coin` computes it, which has no width: a
/// doubling past 256 bits is more than any fees are.
fn more_than_twice(fees: U256, of: U256) -> bool {
    of.checked_mul(U256::from(2))
        .is_some_and(|twice| fees > twice)
}

/// `fees < 2 * of`, likewise.
fn less_than_twice(fees: U256, of: U256) -> bool {
    of.checked_mul(U256::from(2))
        .is_none_or(|twice| fees < twice)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(total_difficulty: u64, paid_fees: u64, hash: B256) -> Contender {
        Contender {
            total_difficulty: U256::from(total_difficulty),
            paid_fees: U256::from(paid_fees),
            hash,
        }
    }

    /// Two hashes, the first the lower.
    fn low_and_high() -> (B256, B256) {
        (B256::repeat_byte(0x11), B256::repeat_byte(0xEE))
    }

    /// rskj `SelectionRuleTest.smallerBlockHashTest`, and the bytes read
    /// unsigned: a first byte of 0x80 is above 0x7F, not below it.
    #[test]
    fn a_hash_is_smaller_by_its_bytes_read_unsigned() {
        let mut one = B256::ZERO;
        one[0] = 1;
        assert!(is_this_block_hash_smaller(&B256::ZERO, &one));
        assert!(!is_this_block_hash_smaller(&one, &B256::ZERO));

        let mut high_bit = B256::ZERO;
        high_bit[0] = 0x80;
        let below = B256::repeat_byte(0x7F);
        assert!(is_this_block_hash_smaller(&below, &high_bit));
        assert!(!is_this_block_hash_smaller(&high_bit, &below));
        assert!(
            !is_this_block_hash_smaller(&one, &one),
            "a hash isn't smaller than itself"
        );
    }

    /// rskj `SelectionRuleTest.addBlockTest`: the heavier chain wins whatever
    /// the fees and the hashes, and on a tie a block with fees takes the place
    /// of one without.
    #[test]
    fn the_heavier_chain_wins_and_a_tie_goes_to_fees() {
        let (low, high) = low_and_high();
        assert!(!should_we_add_this_block(
            &block(1, 1_000, low),
            &block(5, 0, high)
        ));
        assert!(should_we_add_this_block(
            &block(5, 0, high),
            &block(1, 1_000, low)
        ));
        assert!(should_we_add_this_block(
            &block(5, 10, high),
            &block(5, 0, low)
        ));
    }

    /// On a tie, with fees within twice each other's, the lower hash wins,
    /// whichever block arrived first.
    #[test]
    fn a_tie_with_fees_alike_goes_to_the_lower_hash() {
        let (low, high) = low_and_high();
        assert!(should_we_add_this_block(
            &block(7, 100, low),
            &block(7, 100, high)
        ));
        assert!(!should_we_add_this_block(
            &block(7, 100, high),
            &block(7, 100, low)
        ));
        assert!(should_we_add_this_block(
            &block(7, 60, low),
            &block(7, 100, high)
        ));
        assert!(!should_we_add_this_block(
            &block(7, 60, high),
            &block(7, 100, low)
        ));
    }

    /// The two bounds at twice the fees. More than twice the best block's
    /// fees wins whatever the hash; exactly twice falls to the hashes. The
    /// best block keeps its place against a lower hash once it paid twice the
    /// newcomer's fees or more.
    #[test]
    fn twice_the_fees_is_the_bound_both_ways() {
        let (low, high) = low_and_high();
        assert!(should_we_add_this_block(
            &block(7, 201, high),
            &block(7, 100, low)
        ));
        assert!(!should_we_add_this_block(
            &block(7, 200, high),
            &block(7, 100, low)
        ));
        assert!(should_we_add_this_block(
            &block(7, 200, low),
            &block(7, 100, high)
        ));

        assert!(!should_we_add_this_block(
            &block(7, 100, low),
            &block(7, 200, high)
        ));
        assert!(!should_we_add_this_block(
            &block(7, 100, low),
            &block(7, 250, high)
        ));
        assert!(should_we_add_this_block(
            &block(7, 100, low),
            &block(7, 199, high)
        ));
    }

    /// Two blocks that paid nothing: the one held first keeps its place,
    /// however the hashes fall. A newcomer that paid anything beats one that
    /// paid nothing.
    #[test]
    fn without_fees_a_tie_keeps_the_block_held_first() {
        let (low, high) = low_and_high();
        assert!(!should_we_add_this_block(
            &block(7, 0, low),
            &block(7, 0, high)
        ));
        assert!(!should_we_add_this_block(
            &block(7, 0, low),
            &block(7, 1, high)
        ));
        assert!(should_we_add_this_block(
            &block(7, 1, high),
            &block(7, 0, low)
        ));
    }

    /// rskj's `Coin` doubles without a width, so a doubling past 256 bits is
    /// more than any fees: neither comparison may wrap.
    #[test]
    fn a_doubling_past_256_bits_does_not_wrap() {
        let (low, high) = low_and_high();
        let half = U256::from(1) << 255;
        let at = |paid_fees: U256, hash: B256| Contender {
            total_difficulty: U256::from(7),
            paid_fees,
            hash,
        };
        // The best block paid 2^255, and twice that is 2^256, which no fees
        // are more than: the newcomer's `MAX` doesn't win by its fees, and
        // the hashes decide.
        assert!(!should_we_add_this_block(
            &at(U256::MAX, high),
            &at(half, low)
        ));
        assert!(should_we_add_this_block(
            &at(U256::MAX, low),
            &at(half, high)
        ));
        // The newcomer paid 2^255: the best block's `MAX` is less than twice
        // that, so the hashes decide again.
        assert!(should_we_add_this_block(
            &at(half, low),
            &at(U256::MAX, high)
        ));
        assert!(!should_we_add_this_block(
            &at(half, high),
            &at(U256::MAX, low)
        ));
    }

    /// The rule isn't transitive once fees differ by twice or more: with fees
    /// of 25, 10 and 15, and the cheapest block's hash the lowest and the
    /// first's the highest, the block that paid 10 loses to the one that paid
    /// 25 and beats the one that paid 15, which beats the first. So a header
    /// weighed again after the head moved can take a head that rskj, weighing
    /// each block once, would keep (the catalogue's §99).
    #[test]
    fn the_rule_is_not_transitive() {
        let first = block(7, 25, B256::repeat_byte(0x30));
        let cheapest = block(7, 10, B256::repeat_byte(0x10));
        let third = block(7, 15, B256::repeat_byte(0x20));
        assert!(!should_we_add_this_block(&cheapest, &first));
        assert!(should_we_add_this_block(&third, &first));
        assert!(should_we_add_this_block(&cheapest, &third));
    }
}
