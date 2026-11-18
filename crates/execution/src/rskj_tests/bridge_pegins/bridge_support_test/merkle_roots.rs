//! rskj's `BridgeSupportTest` tests of what proves a Bitcoin transaction to
//! the Bridge (`isBlockMerkleRootValid_*`, `getBtcTransactionConfirmations_*`,
//! `registerBtcTransaction_*_rskip_143_*`): the root its merkle branch climbs
//! to must be its block's merkle root or, from RSKIP143, the witness merkle
//! root registered with the block's coinbase.
//!
//! rskj's `isBlockMerkleRootValid` is a method of its own, which its five
//! tests call; rustock checks the same inside `getBtcTransactionConfirmations`
//! and `registerBtcTransaction`, so those tests are held through the
//! confirmations `getBtcTransactionConfirmations` answers: their count where
//! the root is valid, -5 where it is not. rskj's tests mock RSKIP143 off or on
//! alone; here they run on Wasabi100 and on Papyrus200, which brings it.

use bitcoin::block::Header;
use bitcoin::{Transaction, Witness};

use crate::abi::encode::{self, Arg};
use crate::bridge::constants::BridgeConstants;
use crate::bridge::{btc_chain, storage, tx as bridge_tx};
use crate::hardfork::{RskHardforkConfig, RskNetworkUpgrade, RSK_REGTEST_CHAIN_ID};
use crate::precompiles::BRIDGE_ADDR;

use super::super::support::*;

use RskNetworkUpgrade::{Papyrus200, Wasabi100};

/// Where rskj's tests hold the transaction's block, and the chain's head.
const HEIGHT: u32 = 50;
const HEAD: u32 = 132;

/// The confirmations of a transaction in the block at [`HEIGHT`].
const CONFIRMED: i64 = (HEAD - HEIGHT + 1) as i64;

/// `getBtcTransactionConfirmations`' answer for a branch that climbs to
/// neither root (`BTC_TRANSACTION_CONFIRMATION_INVALID_MERKLE_BRANCH_ERROR_CODE`).
const INVALID_MERKLE_BRANCH: i64 = -5;

/// What the transactions here send.
const AMOUNT: u64 = 10 * COIN;

/// A transaction from the P2PKH key `sender` to rskj regtest's federation,
/// its input's witness `witness`.
fn transaction(sender: &[u8; 33], witness: Witness) -> Transaction {
    let (script_sig, _) = p2pkh_spend(sender);
    tx(
        vec![(script_sig, witness)],
        &[(
            AMOUNT,
            genesis_federation_output(&BridgeConstants::rskj_regtest()),
        )],
    )
}

/// rskj's tests' witness: one empty push.
fn a_witness() -> Witness {
    let mut witness = Witness::new();
    witness.push([0u8; 0]);
    witness
}

/// The hash of the transaction's bytes, with its witness if it has one
/// (rskj's `getHash(true)`).
fn hash_of(tx: &Transaction) -> [u8; 32] {
    bridge_tx::calculate_btc_tx_hash(&bitcoin::consensus::serialize(tx))
}

/// The transaction's id, of its bytes without its witness (rskj's
/// `getHash()`).
fn id_of(tx: &Transaction) -> [u8; 32] {
    let mut stripped = tx.clone();
    for input in &mut stripped.input {
        input.witness = Witness::new();
    }
    hash_of(&stripped)
}

/// A block's hash, as the Bridge's methods take it.
fn block_hash(header: &Header) -> [u8; 32] {
    btc_chain::bitcoin_hash_to_b256(&header.block_hash()).0
}

/// A Bridge at RSK block 10 of rskj regtest's ladder up to `last`, whose
/// Bitcoin store holds a block at [`HEIGHT`] with the merkle root `root` and
/// the chain on it up to [`HEAD`], and, if any, `witness_root` registered as
/// the witness root of the block's coinbase.
fn holding(
    last: RskNetworkUpgrade,
    root: [u8; 32],
    witness_root: Option<[u8; 32]>,
) -> (Ctx, BridgeConstants, RskHardforkConfig, Header) {
    let config = BridgeConstants {
        initial_locking_cap: 1_000 * COIN,
        ..BridgeConstants::rskj_regtest()
    };
    let cfg = up_to(last, RSK_REGTEST_CHAIN_ID);
    let mut ctx = ctx_at(10);
    let header = hold_with_root(&mut ctx, root, HEIGHT, HEAD, &cfg);
    if let Some(mut witness_root) = witness_root {
        // Kept as `registerBtcCoinbaseTransaction` keeps it, as it is displayed.
        witness_root.reverse();
        bridge_tx::set_coinbase_information(&mut ctx, &block_hash(&header), &witness_root);
    }
    fund_bridge_but(&mut ctx, 0);
    (ctx, config, cfg, header)
}

/// `getBtcTransactionConfirmations` of the transaction whose hash is `hash`,
/// in the block `header`, by a branch of no hashes.
fn confirmations(
    ctx: &mut Ctx,
    config: &BridgeConstants,
    cfg: &RskHardforkConfig,
    hash: [u8; 32],
    header: &Header,
) -> i64 {
    let mut tx_hash = hash;
    tx_hash.reverse();
    let args = encode::args(&[
        Arg::Word(tx_hash),
        Arg::Word(block_hash(header)),
        Arg::Word(encode::int(0)),
        Arg::Words(&[]),
    ]);
    let use_v2 = cfg.has_stored_block_v2(block_of(ctx));
    let answer = bridge_tx::get_btc_transaction_confirmations(ctx, &args, 0, config, use_v2, cfg)
        .expect("an answer");
    i64::from_be_bytes(answer.bytes[24..32].try_into().expect("eight bytes"))
}

/// `getBtcTransactionConfirmations_rejects_tx_with_witness_before_rskip_143`.
#[test]
fn before_rskip143_a_branch_from_the_witness_hash_proves_nothing() {
    let tx = transaction(&key_from_seed("k1"), a_witness());
    let (mut ctx, config, cfg, header) = holding(Wasabi100, id_of(&tx), None);
    let answer = confirmations(&mut ctx, &config, &cfg, hash_of(&tx), &header);
    assert_eq!(answer, INVALID_MERKLE_BRANCH);
}

/// rskj's `isBlockMerkleRootValid` reads the witness root registered for a
/// block only from RSKIP143: before it, a branch to the root kept with the
/// block's coinbase proves nothing, and from it the transaction is proven
/// (`from_rskip143_a_branch_to_the_registered_witness_root_proves_the_transaction`).
/// No chain shows it, since a coinbase can be registered only from RSKIP143;
/// the root is put in place as `registerBtcCoinbaseTransaction` keeps it.
#[test]
fn before_rskip143_a_registered_witness_root_proves_nothing() {
    let tx = transaction(&key_from_seed("k1"), a_witness());
    let (mut ctx, config, cfg, header) = holding(Wasabi100, id_of(&tx), Some(hash_of(&tx)));
    let answer = confirmations(&mut ctx, &config, &cfg, hash_of(&tx), &header);
    assert_eq!(answer, INVALID_MERKLE_BRANCH);
}

/// `getBtcTransactionConfirmations_accepts_tx_with_witness_after_rskip_143`
/// and `isBlockMerkleRootValid_coinbase_information_not_null_and_equal_mroots_after_rskip_143`.
#[test]
fn from_rskip143_a_branch_to_the_registered_witness_root_proves_the_transaction() {
    let tx = transaction(&key_from_seed("k2"), a_witness());
    let (mut ctx, config, cfg, header) = holding(Papyrus200, id_of(&tx), Some(hash_of(&tx)));
    assert_eq!(
        confirmations(&mut ctx, &config, &cfg, hash_of(&tx), &header),
        CONFIRMED
    );
}

/// `getBtcTransactionConfirmations_unregistered_coinbase_after_rskip_143` and
/// `isBlockMerkleRootValid_coinbase_information_null_after_rskip_143`.
#[test]
fn from_rskip143_a_witness_root_no_coinbase_registered_proves_nothing() {
    let tx = transaction(&key_from_seed("k3"), a_witness());
    let (mut ctx, config, cfg, header) = holding(Papyrus200, id_of(&tx), None);
    let answer = confirmations(&mut ctx, &config, &cfg, hash_of(&tx), &header);
    assert_eq!(answer, INVALID_MERKLE_BRANCH);
}

/// `getBtcTransactionConfirmations_registered_coinbase_unequal_witnessroot_after_rskip_143`
/// and `isBlockMerkleRootValid_coinbase_information_not_null_and_unequal_mroots_after_rskip_143`.
#[test]
fn from_rskip143_a_witness_root_other_than_the_registered_one_proves_nothing() {
    let tx = transaction(&key_from_seed("k4"), a_witness());
    let (mut ctx, config, cfg, header) = holding(Papyrus200, id_of(&tx), Some([0x3c; 32]));
    let answer = confirmations(&mut ctx, &config, &cfg, hash_of(&tx), &header);
    assert_eq!(answer, INVALID_MERKLE_BRANCH);
}

/// `getBtcTransactionConfirmations_tx_without_witness_unequal_roots_after_rskip_143`
/// and `isBlockMerkleRootValid_unequal_merkle_roots_before_rskip_143`: a root
/// other than the block's proves nothing, before RSKIP143 or from it.
#[test]
fn a_root_other_than_the_blocks_proves_nothing() {
    for last in [Wasabi100, Papyrus200] {
        let tx = transaction(&key_from_seed("k5"), Witness::new());
        let (mut ctx, config, cfg, header) = holding(last, [0x22; 32], None);
        let answer = confirmations(&mut ctx, &config, &cfg, hash_of(&tx), &header);
        assert_eq!(answer, INVALID_MERKLE_BRANCH, "{last:?}");
    }
}

/// `getBtcTransactionConfirmations_accepts_tx_without_witness_after_rskip_143`
/// and `isBlockMerkleRootValid_equal_merkle_roots`: the block's merkle root
/// proves the transaction, before RSKIP143 and from it.
#[test]
fn the_blocks_merkle_root_proves_the_transaction() {
    for last in [Wasabi100, Papyrus200] {
        let tx = transaction(&key_from_seed("k6"), Witness::new());
        let (mut ctx, config, cfg, header) = holding(last, hash_of(&tx), None);
        let answer = confirmations(&mut ctx, &config, &cfg, hash_of(&tx), &header);
        assert_eq!(answer, CONFIRMED, "{last:?}");
    }
}

/// [`holding`] the transaction `tx` of the whitelisted P2PKH key `sender`,
/// registered at [`HEIGHT`] by a tree of it alone.
fn registered(
    last: RskNetworkUpgrade,
    sender: &[u8; 33],
    tx: &Transaction,
    root: [u8; 32],
    witness_root: Option<[u8; 32]>,
) -> (Registered, Ctx, RskHardforkConfig) {
    let (mut ctx, config, cfg, _) = holding(last, root, witness_root);
    whitelist_once(&mut ctx, hash160(sender), AMOUNT);
    let registered = register(&mut ctx, tx, i64::from(HEIGHT), &config, &cfg);
    (registered, ctx, cfg)
}

/// Proven, so credited to the sender, the federation holding it, and marked
/// processed by its id.
fn assert_credited(
    registered: &Registered,
    ctx: &mut Ctx,
    cfg: &RskHardforkConfig,
    sender: &[u8; 33],
) {
    assert!(registered.processed, "processed");
    assert_eq!(
        balance(ctx, rsk_address_of(sender)),
        wei(AMOUNT),
        "credited"
    );
    let utxos = storage::load_federation_utxos(ctx, cfg, 10);
    let values: Vec<u64> = utxos.iter().map(|utxo| utxo.value_satoshis).collect();
    assert_eq!(values, [AMOUNT], "the federation holds it");
}

/// Not proven: the registration returns, and nothing is credited or marked.
fn assert_not_proven(registered: &Registered, ctx: &mut Ctx) {
    assert!(registered.returned, "it returns");
    assert!(!registered.processed, "not processed");
    assert_eq!(balance(ctx, BRIDGE_ADDR), wei(MAX_RBTC), "nothing credited");
}

/// `registerBtcTransaction_rejects_tx_with_witness_before_rskip_143_activation`:
/// before RSKIP143 rskj reads a segwit transaction's marker as a count of no
/// inputs and throws.
#[test]
fn before_rskip143_a_segwit_transaction_fails_its_registration() {
    let sender = key_from_seed("h1");
    let tx = transaction(&sender, a_witness());
    let (registered, mut ctx, _) = registered(Wasabi100, &sender, &tx, hash_of(&tx), None);
    assert!(!registered.returned, "it throws");
    assert!(!registered.processed);
    assert_eq!(balance(&mut ctx, BRIDGE_ADDR), wei(MAX_RBTC));
}

/// `registerBtcTransaction_accepts_lock_tx_with_witness_after_rskip_143_activation`.
#[test]
fn from_rskip143_a_segwit_peg_in_proven_by_its_coinbase_is_credited() {
    let sender = key_from_seed("h2");
    let tx = transaction(&sender, a_witness());
    let (registered, mut ctx, cfg) =
        registered(Papyrus200, &sender, &tx, id_of(&tx), Some(hash_of(&tx)));
    assert_credited(&registered, &mut ctx, &cfg, &sender);
}

/// `registerBtcTransaction_rejects_tx_with_witness_and_unregistered_coinbase_after_rskip_143_activation`.
#[test]
fn from_rskip143_a_segwit_transaction_no_coinbase_proves_is_not_registered() {
    let sender = key_from_seed("h3");
    let tx = transaction(&sender, a_witness());
    let (registered, mut ctx, _) = registered(Papyrus200, &sender, &tx, id_of(&tx), None);
    assert_not_proven(&registered, &mut ctx);
}

/// `registerBtcTransaction_rejects_tx_with_witness_and_unqual_witness_root_after_rskip_143_activation`.
#[test]
fn from_rskip143_a_segwit_transaction_of_another_witness_root_is_not_registered() {
    let sender = key_from_seed("h4");
    let tx = transaction(&sender, a_witness());
    let (registered, mut ctx, _) =
        registered(Papyrus200, &sender, &tx, id_of(&tx), Some([0x3c; 32]));
    assert_not_proven(&registered, &mut ctx);
}

/// `registerBtcTransaction_rejects_tx_without_witness_unequal_roots_after_rskip_143`.
#[test]
fn from_rskip143_a_transaction_of_another_root_is_not_registered() {
    let sender = key_from_seed("h5");
    let tx = transaction(&sender, Witness::new());
    let (registered, mut ctx, _) = registered(Papyrus200, &sender, &tx, [0x22; 32], None);
    assert_not_proven(&registered, &mut ctx);
}

/// `registerBtcTransaction_accepts_lock_tx_without_witness_after_rskip_143_activation`.
#[test]
fn from_rskip143_a_peg_in_proven_by_its_blocks_root_is_credited() {
    let sender = key_from_seed("h6");
    let tx = transaction(&sender, Witness::new());
    let (registered, mut ctx, cfg) = registered(Papyrus200, &sender, &tx, hash_of(&tx), None);
    assert_credited(&registered, &mut ctx, &cfg, &sender);
}
