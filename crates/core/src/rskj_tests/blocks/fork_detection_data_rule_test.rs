//! rskj `co.rsk.validators.ForkDetectionDataRuleTest` (RSKIP110).
//!
//! From RSKIP110 (wasabi100) the twelve bytes after the tag and the first
//! twenty bytes of the merged-mining hash commit to the miner's view of the
//! chain: above block 449 a block is refused unless they are what the 449
//! headers below it make (`ForkDetectionDataRule.isValid`). rskj reads no such
//! bytes below block 449 (`BlockFactory` builds those headers with
//! `includeForkDetectionData` false), and compares none at 449 itself.
//! The calculator, which rskj's test mocks, is rustock's own here.

use super::support::header;
use crate::types::header::Header;
use crate::validation::fork_detection::{self, MainchainView, REQUIRED_BLOCKS};
use crate::validation::ValidationError;
use alloy_primitives::{Bytes, B256};

/// A view of the chain that holds these headers, newest first.
struct Held(Vec<Header>);

impl MainchainView for Held {
    fn headers_from(&self, _from: B256, count: u64) -> Vec<Header> {
        self.0.iter().take(count as usize).cloned().collect()
    }
}

/// `count` headers below `number`, newest first, each merge-mined in a
/// Bitcoin header of its own, as the calculation reads them.
fn chain_below(number: u64, count: u64) -> Held {
    Held(
        (1..=count)
            .map(|depth| {
                let mut header = header(number - depth, 1_000);
                let btc = bitcoin::block::Header {
                    version: bitcoin::block::Version::from_consensus(0x2000_0000),
                    prev_blockhash: bitcoin::hashes::Hash::all_zeros(),
                    merkle_root: bitcoin::hashes::Hash::all_zeros(),
                    time: depth as u32,
                    bits: bitcoin::CompactTarget::from_consensus(0x207f_ffff),
                    nonce: 0,
                };
                header.bitcoin_merged_mining_header =
                    Some(Bytes::from(bitcoin::consensus::serialize(&btc)));
                header
            })
            .collect(),
    )
}

/// What rskj's test hands its rule as fork-detection data.
const DATA: [u8; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

/// rskj `validForBlocksBeforeRskip110` and
/// `validForBlocksBeforeRskip110UsingMethodThatReceivesBlockAsParameter`:
/// before RSKIP110 the rule asks nothing of block 4,242, whatever its
/// coinbase carries and whatever the chain below it. From the activation
/// on, it does.
#[test]
fn before_rskip110_no_fork_detection_data_is_asked_for() {
    let block = header(4_242, 1_000);
    let none_below = Held(Vec::new());
    for actual in [None, Some(DATA)] {
        assert_eq!(
            fork_detection::validate(&block, actual, &none_below, 4_243),
            Ok(())
        );
    }
    assert_eq!(
        fork_detection::validate(&block, Some(DATA), &none_below, 4_242),
        Err(ValidationError::ForkDetectionDataUnreadable)
    );
}

/// rskj `validForRskip110ActiveButNoForkDetectionDataBecauseNoEnoughBlocksToCalculateIt`:
/// block 42, with RSKIP110 active and no chain below it to calculate from,
/// carries no fork-detection data and is valid.
#[test]
fn below_block_449_a_block_carries_no_fork_detection_data() {
    let block = header(42, 1_000);
    assert_eq!(
        fork_detection::validate(&block, None, &Held(Vec::new()), 0),
        Ok(())
    );
}

/// rskj `invalidForRskip110ActiveAndForkDetectionDataButMissingBlocksForCalculation`:
/// above block 449, a chain that can't give the 449 headers the data is
/// calculated from refuses the block; it isn't let through unchecked.
#[test]
fn a_chain_too_short_to_calculate_from_refuses_the_block() {
    let block = header(4_242, 1_000);
    for held in [0, REQUIRED_BLOCKS - 1] {
        assert_eq!(
            fork_detection::validate(&block, Some(DATA), &chain_below(4_242, held), 0),
            Err(ValidationError::ForkDetectionDataUnreadable),
            "{held} headers below it"
        );
    }
}

/// rskj `validForRskip110ActiveAndForkDetectionData`: data that matches
/// what the 449 headers below the block make is valid.
#[test]
fn fork_detection_data_matching_the_chain_is_valid() {
    let block = header(4_242, 1_000);
    let below = chain_below(4_242, REQUIRED_BLOCKS);
    let expected = fork_detection::calculate(&below.0).unwrap();
    assert_eq!(
        fork_detection::validate(&block, Some(expected), &below, 0),
        Ok(())
    );
}

/// rskj `invalidForRskip110ActiveAndForkDetectionDataBecauseDataDoesNotMatch`:
/// one byte other than the chain makes, and the block is refused.
#[test]
fn fork_detection_data_not_matching_the_chain_is_refused() {
    let block = header(4_242, 1_000);
    let below = chain_below(4_242, REQUIRED_BLOCKS);
    let expected = fork_detection::calculate(&below.0).unwrap();
    let mut got = expected;
    got[6] = got[6].wrapping_add(35);
    assert_eq!(
        fork_detection::validate(&block, Some(got), &below, 0),
        Err(ValidationError::ForkDetectionDataMismatch { expected, got })
    );
}
