//! rskj `org.ethereum.validator.ProofOfWorkRuleTest`: the merged-mining
//! proof `ProofOfWorkRule` checks.
//!
//! From RSKIP98 (orchid) the field that carries the Bitcoin header holds a
//! Bitcoin header of 80 bytes and nothing else (`BtcHeaderSizeRule`, and
//! `BlockFactory.validateBitcoinMergedMiningHeader` when a block is read).
//! The coinbase carries the RSK tag and the block's merged-mining hash, the
//! tag within its first 64 bytes after the midstate, no later tag after it,
//! and no more than 128 bytes after the hash.

use super::support::{
    bitcoin_header_with_root, coinbase_committing_to, compressed_coinbase, header,
    merged_mining_rule, minable_header, mined_with_siblings, regtest_with_iris300_at,
    tail_committing_to,
};
use crate::config::{ActivationHeights, ChainConfig};
use crate::types::header::Header;
use crate::validation::merged_mining::RSK_TAG;
use crate::validation::{HeaderValidator, ValidationError};
use alloy_primitives::{Bytes, U256};
use alloy_rlp::Encodable;
use bitcoin::hashes::Hash;

/// A Bitcoin coinbase transaction whose output carries `tag_and_hash`, as a
/// merge-mining pool's does.
fn bitcoin_coinbase(tag_and_hash: &[u8]) -> bitcoin::Transaction {
    let push: &bitcoin::script::PushBytes = tag_and_hash.try_into().unwrap();
    bitcoin::Transaction {
        version: bitcoin::transaction::Version::ONE,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![bitcoin::TxIn {
            previous_output: bitcoin::OutPoint::null(),
            script_sig: bitcoin::ScriptBuf::from_bytes(vec![0x51]),
            sequence: bitcoin::Sequence::MAX,
            witness: bitcoin::Witness::default(),
        }],
        output: vec![bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(50),
            script_pubkey: bitcoin::ScriptBuf::new_op_return(push),
        }],
    }
}

/// `header` merge-mined in a one-transaction Bitcoin block, and that block
/// whole: its header, the count of its transactions, and its coinbase.
fn mined_in_a_whole_block(mut header: Header) -> (Header, Vec<u8>) {
    let committed = [RSK_TAG, header.hash_for_merged_mining().as_slice()].concat();
    let coinbase = bitcoin_coinbase(&committed);
    let coinbase_bytes = bitcoin::consensus::serialize(&coinbase);
    // A one-transaction block's merkle root is its coinbase's id.
    let btc = bitcoin_header_with_root(coinbase.compute_txid().to_byte_array(), header.difficulty);
    let header_bytes = bitcoin::consensus::serialize(&btc);
    header.bitcoin_merged_mining_header = Some(Bytes::from(header_bytes.clone()));
    header.bitcoin_merged_mining_coinbase_transaction =
        Some(Bytes::from(compressed_coinbase(0, &coinbase_bytes)));
    header.bitcoin_merged_mining_merkle_proof = Some(Bytes::new());
    let whole = [header_bytes, vec![1], coinbase_bytes].concat();
    (header, whole)
}

/// rskj `test_fullBtcBlockAsHeader_rejectedByRskj` (RSKIP98): a whole Bitcoin
/// block in place of its header proves the same work and the same merkle
/// root, and leaves the merged-mining hash alone, which leaves out the
/// merged-mining fields; from RSKIP98 the rule refuses it, and so does
/// reading the block. Mainnet's orchid, where RSKIP98 arrives.
#[test]
fn from_rskip98_a_whole_bitcoin_block_is_no_merged_mining_header() {
    let rule = merged_mining_rule(ChainConfig::mainnet());
    let heights = ActivationHeights::mainnet();
    let mut at_orchid = header(heights.orchid, 1_000);
    at_orchid.difficulty = U256::from(1);
    let (mined, whole) = mined_in_a_whole_block(at_orchid);
    assert!(whole.len() > 80);
    assert_eq!(rule.validate(&mined), Ok(()));

    let mut with_block = mined.clone();
    with_block.bitcoin_merged_mining_header = Some(Bytes::from(whole));
    assert_eq!(
        with_block.hash_for_merged_mining(),
        mined.hash_for_merged_mining()
    );
    assert_eq!(
        rule.validate(&with_block),
        Err(ValidationError::BitcoinHeaderDecodeError)
    );

    let read = |header: &Header| {
        let mut bytes = Vec::new();
        header.encode(&mut bytes);
        Header::decode_with_activations(&mut bytes.as_slice(), &heights)
    };
    assert!(read(&mined).is_ok());
    assert!(read(&with_block).is_err());
}

/// rskj `test_noRSKTagInCoinbaseTransaction`: a coinbase that carries the
/// merged-mining hash but no tag commits to nothing.
#[test]
fn a_coinbase_without_the_rsk_tag_is_refused() {
    let rule = merged_mining_rule(regtest_with_iris300_at(0));
    let header = minable_header(10);
    let hash = header.hash_for_merged_mining();
    let untagged = compressed_coinbase(0, &[&[0u8; 60][..], hash.as_slice()].concat());
    let mined = mined_with_siblings(header.clone(), untagged, &[]);
    assert_eq!(
        rule.validate(&mined),
        Err(ValidationError::BitcoinCoinbaseTagInvalid)
    );

    let tagged = coinbase_committing_to(&header, 30);
    assert_eq!(
        rule.validate(&mined_with_siblings(header, tagged, &[])),
        Ok(())
    );
}

/// rskj `test_RSKTagInCoinbaseTransactionTooFar`: the tag must start within
/// the coinbase's first 64 bytes after its midstate, so that two midstates
/// of the same coinbase can't prove two blocks. 63 bytes before it pass;
/// 64 don't.
#[test]
fn the_rsk_tag_must_start_within_the_first_64_bytes_of_the_tail() {
    let rule = merged_mining_rule(regtest_with_iris300_at(0));
    let header = minable_header(10);
    let hash = header.hash_for_merged_mining();
    let judge = |before: usize| {
        let coinbase = compressed_coinbase(0, &tail_committing_to(hash.as_slice(), before, 0));
        rule.validate(&mined_with_siblings(header.clone(), coinbase, &[]))
    };
    assert_eq!(judge(63), Ok(()));
    assert_eq!(judge(64), Err(ValidationError::BitcoinCoinbaseTagInvalid));
    assert_eq!(judge(960), Err(ValidationError::BitcoinCoinbaseTagInvalid));
}

/// rskj `bytesAfterMergedMiningHashAreLessThan128` and
/// `bytesAfterMergedMiningHashAreMoreThan128`: a coinbase may carry up to
/// 128 bytes after the merged-mining hash, and no more (a pool can't add
/// three outputs of 36 bytes after it).
#[test]
fn at_most_128_bytes_may_follow_the_merged_mining_hash() {
    let rule = merged_mining_rule(regtest_with_iris300_at(0));
    let header = minable_header(10);
    let judge = |after: usize| {
        let coinbase = coinbase_committing_to(&header, after);
        rule.validate(&mined_with_siblings(header.clone(), coinbase, &[]))
    };
    assert_eq!(judge(128), Ok(()));
    assert_eq!(judge(129), Err(ValidationError::BitcoinCoinbaseTagInvalid));
}

/// Not in rskj's suite: `ProofOfWorkRule` also refuses a coinbase whose tag
/// committing to the block is not its last, so that no later tag can claim
/// the same work.
#[test]
fn the_tag_committing_to_the_block_must_be_the_last_tag() {
    let rule = merged_mining_rule(regtest_with_iris300_at(0));
    let header = minable_header(10);
    let hash = header.hash_for_merged_mining();
    let judge = |later_tag: bool| {
        let mut tail = tail_committing_to(hash.as_slice(), 0, 30);
        if later_tag {
            tail.extend_from_slice(RSK_TAG);
        }
        let coinbase = compressed_coinbase(0, &tail);
        rule.validate(&mined_with_siblings(header.clone(), coinbase, &[]))
    };
    assert_eq!(judge(false), Ok(()));
    assert_eq!(judge(true), Err(ValidationError::BitcoinCoinbaseTagInvalid));
}
