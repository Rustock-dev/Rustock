//! What the ported tests of this folder build on: a header holding only the
//! fields a test sets, and a header merge-mined through a coinbase and a
//! merkle proof of the test's choosing, as rskj's `BlockMiner` mines one.

use crate::config::{ActivationHeights, ChainConfig};
use crate::types::header::Header;
use crate::validation::merged_mining::{self, MergedMiningRule};
use alloy_primitives::{Address, Bytes, B256, U256};
use bitcoin::hashes::Hash;
use std::sync::Arc;

/// A header at `number` and `timestamp`, every other field empty or zero.
pub(super) fn header(number: u64, timestamp: u64) -> Header {
    Header {
        parent_hash: B256::ZERO,
        ommers_hash: B256::ZERO,
        beneficiary: Address::ZERO,
        state_root: B256::ZERO,
        transactions_root: B256::ZERO,
        receipts_root: B256::ZERO,
        logs_bloom: Default::default(),
        extension_data: None,
        difficulty: U256::ZERO,
        number,
        gas_limit: U256::from(10_000_000),
        gas_used: 0,
        timestamp,
        extra_data: Bytes::new(),
        paid_fees: U256::ZERO,
        minimum_gas_price: U256::ZERO,
        uncle_count: 0,
        umm_root: None,
        version_fields: None,
        bitcoin_merged_mining_header: None,
        bitcoin_merged_mining_merkle_proof: None,
        bitcoin_merged_mining_coinbase_transaction: None,
        cached_hash: None,
        cached_hash_for_merged_mining: None,
        kept: None,
    }
}

/// Regtest's rules with iris300, where RSKIP179 and RSKIP180 arrive, at
/// `iris300`: regtest has every upgrade from genesis, so a test of a height
/// window moves the one it is about.
pub(super) fn regtest_with_iris300_at(iris300: u64) -> ChainConfig {
    ChainConfig {
        activation_heights: ActivationHeights {
            iris300,
            ..ActivationHeights::regtest()
        },
        ..ChainConfig::regtest()
    }
}

pub(super) fn merged_mining_rule(config: ChainConfig) -> MergedMiningRule {
    MergedMiningRule {
        config: Arc::new(config),
    }
}

/// A header at `number` and difficulty 1, where one Bitcoin hash in three
/// meets rskj's target, 2^256 / 3: [`mined_through`] rolls the nonce until
/// one does, so only the coinbase and the merkle proof are in question.
pub(super) fn minable_header(number: u64) -> Header {
    let mut header = header(number, 1_000);
    header.difficulty = U256::from(1);
    header
}

/// A coinbase in the compressed form a header carries: SHA-256's initial
/// state as its midstate, claiming `absorbed` bytes already hashed, then
/// `tail`.
pub(super) fn compressed_coinbase(absorbed: u64, tail: &[u8]) -> Vec<u8> {
    let mut out = absorbed.to_be_bytes().to_vec();
    for word in merged_mining::SHA256_INITIAL_STATE {
        out.extend_from_slice(&word.to_be_bytes());
    }
    out.extend_from_slice(tail);
    out
}

/// A coinbase tail: `before` bytes, the RSK tag, `committed`, then `after`
/// bytes.
pub(super) fn tail_committing_to(committed: &[u8], before: usize, after: usize) -> Vec<u8> {
    let mut tail = vec![0x11; before];
    tail.extend_from_slice(merged_mining::RSK_TAG);
    tail.extend_from_slice(committed);
    tail.resize(tail.len() + after, 0x22);
    tail
}

/// The compressed coinbase of a Bitcoin block that commits to `header`'s
/// whole merged-mining hash, right after its tag, with `after` bytes after it.
pub(super) fn coinbase_committing_to(header: &Header, after: usize) -> Vec<u8> {
    let hash = header.hash_for_merged_mining();
    compressed_coinbase(0, &tail_committing_to(hash.as_slice(), 0, after))
}

/// A root folded in bitcoinj's display order, as a Bitcoin header holds it:
/// reversed.
pub(super) fn wire(display: [u8; 32]) -> [u8; 32] {
    let mut out = display;
    out.reverse();
    out
}

/// Roll `btc`'s nonce until its hash meets rskj's target for `difficulty`,
/// as a miner does.
pub(super) fn mine(btc: &mut bitcoin::block::Header, difficulty: U256) {
    let target = merged_mining::difficulty_to_target(difficulty);
    for _ in 0..1_000_000 {
        if U256::from_le_slice(btc.block_hash().as_byte_array()) <= target {
            return;
        }
        btc.nonce += 1;
    }
    panic!("no nonce meets the target for difficulty {difficulty}");
}

/// A mined Bitcoin header holding `wire_root` as its merkle root.
pub(super) fn bitcoin_header_with_root(
    wire_root: [u8; 32],
    difficulty: U256,
) -> bitcoin::block::Header {
    let mut btc = bitcoin::block::Header {
        version: bitcoin::block::Version::from_consensus(1),
        prev_blockhash: bitcoin::BlockHash::all_zeros(),
        merkle_root: bitcoin::TxMerkleNode::from_byte_array(wire_root),
        time: 1_000,
        bits: bitcoin::CompactTarget::from_consensus(0x207f_ffff),
        nonce: 0,
    };
    mine(&mut btc, difficulty);
    btc
}

/// `header` merge-mined through `coinbase` and `proof`, by a Bitcoin header
/// holding `wire_root` as its merkle root.
pub(super) fn mined_through(
    mut header: Header,
    coinbase: Vec<u8>,
    proof: Vec<u8>,
    wire_root: [u8; 32],
) -> Header {
    let btc = bitcoin_header_with_root(wire_root, header.difficulty);
    header.bitcoin_merged_mining_header = Some(Bytes::from(bitcoin::consensus::serialize(&btc)));
    header.bitcoin_merged_mining_coinbase_transaction = Some(Bytes::from(coinbase));
    header.bitcoin_merged_mining_merkle_proof = Some(Bytes::from(proof));
    header
}

/// The root RSKIP92's proof folds to: the coinbase's hash, combined on the
/// left with each sibling in turn.
pub(super) fn folded_root(coinbase_hash: [u8; 32], siblings: &[[u8; 32]]) -> [u8; 32] {
    siblings.iter().fold(coinbase_hash, |node, sibling| {
        merged_mining::combine_left_right(&node, sibling)
    })
}

/// `header` merge-mined through `coinbase` and an RSKIP92 proof of
/// `siblings`, by a Bitcoin header holding the root they fold to.
pub(super) fn mined_with_siblings(
    header: Header,
    coinbase: Vec<u8>,
    siblings: &[[u8; 32]],
) -> Header {
    let root = folded_root(merged_mining::compute_coinbase_hash(&coinbase), siblings);
    mined_through(header, coinbase, siblings.concat(), wire(root))
}

/// `count` distinct hashes, none of which a test means anything by.
pub(super) fn hashes(count: usize) -> Vec<[u8; 32]> {
    (0..count)
        .map(|i| alloy_primitives::keccak256((i as u64).to_be_bytes()).0)
        .collect()
}
