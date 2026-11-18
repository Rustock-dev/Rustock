//! rskj's `BridgeSupportTest` tests of `registerBtcCoinbaseTransaction`
//! (`when_RegisterBtcCoinbaseTransaction_*`) and of
//! `hasBtcBlockCoinbaseTransactionInformation` (`hasBtcCoinbaseTransaction_*`).
//!
//! Each registration turned down is set beside the one that is kept: the
//! same coinbase, tree and block, as they should be, is kept, so that what is
//! turned down is turned down for what was changed. They run on mainnet at
//! Papyrus200, which brings RSKIP143, and the method with it.

use sha2::Digest as _;

use crate::abi::encode::{self, Arg};
use crate::bridge::constants::BridgeConstants;
use crate::bridge::{btc_chain, tx as bridge_tx};
use crate::hardfork::{RskHardforkConfig, RskNetworkUpgrade, RSK_MAINNET_CHAIN_ID};

use super::super::support::*;

/// The witness merkle root the coinbase commits to, and its reserved value.
const WITNESS_ROOT: [u8; 32] = [0x3c; 32];
const RESERVED: [u8; 32] = [0; 32];

/// The Bitcoin height the coinbase's block is held at.
const HEIGHT: u32 = 850_000;

/// A coinbase whose scriptSig is `script_sig`, committing to [`WITNESS_ROOT`]
/// with [`RESERVED`]: BIP-141's `OP_RETURN aa21a9ed <sha256d(root ||
/// reserved)>`, the root in the order rskj reverses it to
/// (`witnessMerkleRoot.getReversedBytes()`).
fn coinbase(script_sig: &[u8]) -> Vec<u8> {
    let mut root = WITNESS_ROOT;
    root.reverse();
    let commitment = sha2::Sha256::digest(sha2::Sha256::digest([root, RESERVED].concat()));
    let mut commitment_script = vec![0x6a, 0x24, 0xaa, 0x21, 0xa9, 0xed];
    commitment_script.extend_from_slice(&commitment);
    let coinbase = bitcoin::Transaction {
        version: bitcoin::transaction::Version::ONE,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![bitcoin::TxIn {
            previous_output: bitcoin::OutPoint::null(),
            script_sig: bitcoin::ScriptBuf::from_bytes(script_sig.to_vec()),
            sequence: bitcoin::Sequence::MAX,
            witness: bitcoin::Witness::new(),
        }],
        output: vec![
            bitcoin::TxOut {
                value: bitcoin::Amount::from_sat(50 * COIN),
                script_pubkey: p2pkh(&[0x77; 20]),
            },
            bitcoin::TxOut {
                value: bitcoin::Amount::ZERO,
                script_pubkey: bitcoin::ScriptBuf::from_bytes(commitment_script),
            },
        ],
    };
    bitcoin::consensus::serialize(&coinbase)
}

/// The coinbase the registrations here are of.
fn the_coinbase() -> Vec<u8> {
    coinbase(&[0x03, 0x50, 0xf8, 0x0c])
}

/// A Bridge at RSK block 10 of mainnet's ladder up to Papyrus200, whose
/// Bitcoin store holds a block for each of `roots`, from [`HEIGHT`] up: the
/// Bridge and the blocks' hashes.
fn holding(roots: &[[u8; 32]]) -> (Ctx, BridgeConstants, RskHardforkConfig, Vec<[u8; 32]>) {
    let config = BridgeConstants::mainnet();
    let cfg = up_to(RskNetworkUpgrade::Papyrus200, RSK_MAINNET_CHAIN_ID);
    let mut ctx = ctx_at(10);
    let head = HEIGHT + roots.len() as u32 - 1;
    let hashes = hold_roots(&mut ctx, roots, HEIGHT, head, &cfg)
        .iter()
        .map(|header| btc_chain::bitcoin_hash_to_b256(&header.block_hash()).0)
        .collect();
    (ctx, config, cfg, hashes)
}

/// `registerBtcCoinbaseTransaction` of `raw`, in the block `block_hash`, by
/// the tree `tree`: whether it returned.
fn registers(
    ctx: &mut Ctx,
    config: &BridgeConstants,
    cfg: &RskHardforkConfig,
    raw: &[u8],
    block_hash: [u8; 32],
    tree: &[u8],
) -> bool {
    let args = encode::args(&[
        Arg::Bytes(raw),
        Arg::Word(block_hash),
        Arg::Bytes(tree),
        Arg::Word(WITNESS_ROOT),
        Arg::Word(RESERVED),
    ]);
    in_a_call(ctx, |ctx| {
        bridge_tx::register_btc_coinbase_transaction(ctx, &args, 0, config, cfg)
    })
    .is_ok()
}

/// `when_RegisterBtcCoinbaseTransaction_MerkleTreeWrongFormat_noSent`: a tree
/// rskj cannot read fails the call, and nothing is kept.
#[test]
fn a_coinbase_by_a_tree_rskj_cannot_read_fails_its_registration() {
    let raw = the_coinbase();
    let (mut ctx, config, cfg, blocks) = holding(&[bridge_tx::calculate_btc_tx_hash(&raw)]);
    assert!(!registers(
        &mut ctx,
        &config,
        &cfg,
        &raw,
        blocks[0],
        &[0x01, 0x02, 0x03]
    ));
    assert_eq!(
        bridge_tx::get_coinbase_information(&mut ctx, &blocks[0]),
        None
    );
    assert!(registers(
        &mut ctx,
        &config,
        &cfg,
        &raw,
        blocks[0],
        &one_leaf_tree(&raw)
    ));
    assert_eq!(
        bridge_tx::get_coinbase_information(&mut ctx, &blocks[0]),
        Some(WITNESS_ROOT)
    );
}

/// `when_RegisterBtcCoinbaseTransaction_HashNotInPmt_noSent`: a tree of
/// another transaction keeps nothing, and the call returns.
#[test]
fn a_coinbase_by_a_tree_of_another_transaction_is_not_kept() {
    let raw = the_coinbase();
    let (mut ctx, config, cfg, blocks) = holding(&[bridge_tx::calculate_btc_tx_hash(&raw)]);
    let another = one_leaf_tree(&coinbase(&[0x03, 0x51, 0xf8, 0x0c]));
    assert!(registers(
        &mut ctx, &config, &cfg, &raw, blocks[0], &another
    ));
    assert_eq!(
        bridge_tx::get_coinbase_information(&mut ctx, &blocks[0]),
        None
    );
    assert!(registers(
        &mut ctx,
        &config,
        &cfg,
        &raw,
        blocks[0],
        &one_leaf_tree(&raw)
    ));
    assert_eq!(
        bridge_tx::get_coinbase_information(&mut ctx, &blocks[0]),
        Some(WITNESS_ROOT)
    );
}

/// `when_RegisterBtcCoinbaseTransaction_not_equal_merkle_root_noSent`: in a
/// block whose merkle root is not the tree's, nothing is kept, and the call
/// returns.
#[test]
fn a_coinbase_in_a_block_of_another_merkle_root_is_not_kept() {
    let raw = the_coinbase();
    let (mut ctx, config, cfg, blocks) =
        holding(&[[0x11; 32], bridge_tx::calculate_btc_tx_hash(&raw)]);
    let (other, its_block) = (blocks[0], blocks[1]);
    assert!(registers(
        &mut ctx,
        &config,
        &cfg,
        &raw,
        other,
        &one_leaf_tree(&raw)
    ));
    assert_eq!(bridge_tx::get_coinbase_information(&mut ctx, &other), None);
    assert!(registers(
        &mut ctx,
        &config,
        &cfg,
        &raw,
        its_block,
        &one_leaf_tree(&raw)
    ));
    assert_eq!(
        bridge_tx::get_coinbase_information(&mut ctx, &its_block),
        Some(WITNESS_ROOT)
    );
}

/// `when_RegisterBtcCoinbaseTransaction_null_stored_block_noSent`: a block
/// the store does not hold fails the call, and nothing is kept.
#[test]
fn a_coinbase_in_a_block_not_held_fails_its_registration() {
    let raw = the_coinbase();
    let (mut ctx, config, cfg, blocks) = holding(&[bridge_tx::calculate_btc_tx_hash(&raw)]);
    assert!(!registers(
        &mut ctx,
        &config,
        &cfg,
        &raw,
        [0x99; 32],
        &one_leaf_tree(&raw)
    ));
    assert_eq!(
        bridge_tx::get_coinbase_information(&mut ctx, &[0x99; 32]),
        None
    );
    assert!(registers(
        &mut ctx,
        &config,
        &cfg,
        &raw,
        blocks[0],
        &one_leaf_tree(&raw)
    ));
    assert_eq!(
        bridge_tx::get_coinbase_information(&mut ctx, &blocks[0]),
        Some(WITNESS_ROOT)
    );
}

/// `hasBtcCoinbaseTransaction_after_rskip_143_activation` and
/// `hasBtcCoinbaseTransaction_fails_with_null_coinbase_information_after_rskip_143_activation`:
/// a block has coinbase information once its coinbase's is kept, and not
/// otherwise.
#[test]
fn a_block_has_coinbase_information_once_it_is_kept() {
    let mut ctx = ctx_at(10);
    let (kept, other) = ([0x5b; 32], [0x5c; 32]);
    bridge_tx::set_coinbase_information(&mut ctx, &kept, &WITNESS_ROOT);
    let has = |ctx: &mut Ctx, hash: [u8; 32]| {
        let args = encode::args(&[Arg::Word(hash)]);
        let answer = bridge_tx::has_btc_block_coinbase_info(ctx, &args, 0).expect("an answer");
        answer.bytes[..] == encode::int(1)[..]
    };
    assert!(has(&mut ctx, kept));
    assert!(!has(&mut ctx, other));
}
