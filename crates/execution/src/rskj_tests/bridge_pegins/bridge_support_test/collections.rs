//! rskj's `BridgeSupportTest` tests of what `updateCollections` moves: a
//! retiring federation's funds (`callProcessFundsMigration_*`), the
//! federations' creation heights (`updateFederationCreationBlockHeights_*`),
//! and a confirmed peg-out into the set waiting for signatures
//! (`rskTxWaitingForSignature_*`); and of the RSKIP271 getter of the next
//! peg-out's block (`getNextPegoutCreationBlockNumber_after_RSKIP271_activation`).
//!
//! rskj's tests mock the rules they turn on, alone; here each runs on the
//! ladder up to the upgrade that brings it: RSKIP146 Papyrus200, RSKIP176
//! and RSKIP186 Iris300, RSKIP326 and RSKIP375 Fingerroot500. rskj gives a
//! peg-out the creation hash `Keccak256.ZERO_HASH`; here it is another, as no
//! transaction hashes to zero.

use std::collections::BTreeMap;

use alloy_primitives::U256;
use bitcoin::Transaction;

use crate::bridge::constants::BridgeConstants;
use crate::bridge::peg::{self, PegoutWaitingForConfirmations};
use crate::bridge::serialization::rlp_encode_u64;
use crate::bridge::storage::{self, BridgeUtxo};
use crate::hardfork::{
    RskHardforkConfig, RskNetworkUpgrade, RSK_MAINNET_CHAIN_ID, RSK_REGTEST_CHAIN_ID,
    RSK_TESTNET_CHAIN_ID,
};

use super::super::support::*;
use super::waiting_in;

use RskNetworkUpgrade::{Fingerroot500, Hop400, Iris300, Papyrus200, Wasabi100};

const RELEASE_REQUESTED: &str = "release_requested(bytes32,bytes32,uint256)";
const PEGOUT_CONFIRMED: &str = "pegout_confirmed(bytes32,uint256)";

/// The RSK transaction a peg-out here was created in.
const CREATION: [u8; 32] = [0x33; 32];

/// `updateCollections` in the transaction [`RSK_TX_HASH`]: whether it
/// returned, and the events it logged.
fn update(ctx: &mut Ctx, config: &BridgeConstants, cfg: &RskHardforkConfig) -> (bool, Vec<Event>) {
    let returned = in_a_call(ctx, |ctx| {
        peg::update_collections(ctx, 0, config, cfg, &tx_context())
    })
    .is_ok();
    (returned, take_logs(ctx))
}

/// rskj's funds-migration tests: mainnet at RSK block 18,510, the genesis
/// federation retiring with a UTXO of one bitcoin, and a one-member
/// federation created at block 5, so active from block 18,505 and inside its
/// migration window.
fn migrating(last: RskNetworkUpgrade) -> (Ctx, BridgeConstants, RskHardforkConfig) {
    let config = BridgeConstants::mainnet();
    let cfg = up_to(last, RSK_MAINNET_CHAIN_ID);
    let mut ctx = ctx_at(18_510);
    fund_bridge_but(&mut ctx, 0);
    set_fee_per_kb(&mut ctx, MILLICOIN);
    let genesis = keys_of(config.genesis_federation_public_keys);
    store_federation(&mut ctx, OLD_FEDERATION, &genesis, 1, 1000);
    // `FederationTestUtils.getFederationMembers(1)`: the key of private key 100.
    store_federation(&mut ctx, NEW_FEDERATION, &[public_key(&[100])], 5, 1000);
    storage::store_old_federation_utxos(
        &mut ctx,
        &[BridgeUtxo {
            tx_hash: [0x01; 32],
            vout: 0,
            value_satoshis: COIN,
            height: 10,
            script: p2sh_of(&standard_redeem(&genesis)).to_bytes(),
            coinbase: false,
        }],
    );
    (ctx, config, cfg)
}

/// The migration `updateCollections` makes: waiting without the hash of the
/// transaction that made it before RSKIP146, and with it, logged as
/// requested, from RSKIP146.
fn migrated(last: RskNetworkUpgrade) {
    let (mut ctx, config, cfg) = migrating(last);
    let (returned, logs) = update(&mut ctx, &config, &cfg);
    assert!(returned);
    let without_hash = waiting_in(
        &mut ctx,
        storage::PEGOUTS_WAITING_FOR_CONFIRMATIONS_KEY,
        false,
    );
    let with_hash = waiting_in(
        &mut ctx,
        storage::PEGOUTS_WAITING_FOR_CONFIRMATIONS_WITH_TXHASH_KEY,
        true,
    );
    if cfg.has_rskip146(18_510) {
        assert_eq!((without_hash.len(), with_hash.len()), (0, 1));
        assert!(
            logs.contains(&release_requested(&with_hash[0], COIN)),
            "{logs:?}"
        );
    } else {
        assert_eq!((without_hash.len(), with_hash.len()), (1, 0));
        assert_eq!(count_of(&logs, RELEASE_REQUESTED), 0);
    }
}

/// `callProcessFundsMigration_is_migrating_before_rskip_146_activation` and
/// `callProcessFundsMigration_is_migrated_before_rskip_146_activation`, which
/// rskj sets up alike.
#[test]
fn before_rskip146_a_migration_waits_without_its_transactions_hash() {
    migrated(Wasabi100);
}

/// `callProcessFundsMigration_is_migrating_after_rskip_146_activation` and
/// `callProcessFundsMigration_is_migrated_after_rskip_146_activation`.
#[test]
fn from_rskip146_a_migration_waits_with_its_transactions_hash_and_is_logged() {
    migrated(Papyrus200);
}

/// The active and the next federation's creation heights, as kept.
fn creation_heights(ctx: &mut Ctx) -> (Vec<u8>, Vec<u8>) {
    (
        storage::bridge_load_bytes_named(ctx, storage::ACTIVE_FEDERATION_CREATION_BLOCK_HEIGHT_KEY),
        storage::bridge_load_bytes_named(ctx, storage::NEXT_FEDERATION_CREATION_BLOCK_HEIGHT_KEY),
    )
}

/// Keeps 1 as the next federation's creation height.
fn keep_next_creation_height(ctx: &mut Ctx) {
    storage::bridge_store_bytes_named(
        ctx,
        storage::NEXT_FEDERATION_CREATION_BLOCK_HEIGHT_KEY,
        &rlp_encode_u64(1),
    );
}

/// `updateFederationCreationBlockHeights_before_rskip_186_activation`, at
/// Papyrus200: `updateCollections` leaves the heights as they are.
#[test]
fn before_rskip186_update_collections_leaves_the_creation_heights_alone() {
    let (mut ctx, config, cfg) = migrating(Papyrus200);
    keep_next_creation_height(&mut ctx);
    assert!(update(&mut ctx, &config, &cfg).0);
    assert_eq!(creation_heights(&mut ctx), (vec![], rlp_encode_u64(1)));
}

/// `updateFederationCreationBlockHeights_after_rskip_186_activation`: with no
/// next height nothing changes, and a next height whose federation is active
/// becomes the active one, and is cleared.
#[test]
fn from_rskip186_update_collections_makes_the_next_creation_height_the_active_one() {
    let (mut ctx, config, cfg) = migrating(Iris300);
    assert!(update(&mut ctx, &config, &cfg).0);
    assert_eq!(creation_heights(&mut ctx), (vec![], vec![]));
    keep_next_creation_height(&mut ctx);
    assert!(update(&mut ctx, &config, &cfg).0);
    assert_eq!(creation_heights(&mut ctx), (rlp_encode_u64(1), vec![]));
}

/// A peg-out, told apart by `seed`.
fn pegout(seed: u8) -> Transaction {
    tx(vec![no_sender()], &[(COIN, p2pkh(&[seed; 20]))])
}

/// The set waiting for signatures: each peg-out by the RSK transaction hash
/// it is kept under.
fn waiting_for_signatures(ctx: &mut Ctx) -> BTreeMap<[u8; 32], Vec<u8>> {
    peg::deserialize_rsk_txs_waiting_for_signatures(&storage::bridge_load_bytes_named(
        ctx,
        storage::PEGOUTS_WAITING_FOR_SIGNATURES_KEY,
    ))
}

/// A Bridge at RSK block `at` with `pegout`, created at block `created` in
/// the transaction `creation`, if known, waiting for confirmations, and
/// `signing` waiting for signatures.
fn confirming(
    cfg: &RskHardforkConfig,
    at: u64,
    (pegout, created, creation): (&Transaction, u64, Option<[u8; 32]>),
    signing: &BTreeMap<[u8; 32], Vec<u8>>,
) -> Ctx {
    let mut ctx = ctx_at(at);
    fund_bridge_but(&mut ctx, 0);
    let entry = PegoutWaitingForConfirmations {
        btc_tx_raw: bitcoin::consensus::serialize(pegout),
        rsk_block_height: created,
        rsk_tx_hash: creation,
    };
    peg::store_pegout_confirmation_set(&mut ctx, &[entry], cfg.has_rskip146(at));
    storage::bridge_store_bytes_named(
        &mut ctx,
        storage::PEGOUTS_WAITING_FOR_SIGNATURES_KEY,
        &peg::serialize_rsk_txs_waiting_for_signatures(signing),
    );
    ctx
}

/// rskj's regtest constants, a peg-out confirmed after one block, as rskj's
/// tests have it.
fn regtest_one_confirmation() -> BridgeConstants {
    BridgeConstants {
        rsk2btc_minimum_acceptable_confirmations: 1,
        ..BridgeConstants::rskj_regtest()
    }
}

/// The networks rskj's parameterized tests run on, with their own
/// confirmations.
fn networks() -> [(BridgeConstants, u64); 3] {
    [
        (BridgeConstants::rskj_regtest(), RSK_REGTEST_CHAIN_ID),
        (BridgeConstants::testnet(), RSK_TESTNET_CHAIN_ID),
        (BridgeConstants::mainnet(), RSK_MAINNET_CHAIN_ID),
    ]
}

/// The key a peg-out created at block 1 in `creation`, if known, confirmed by
/// `updateCollections` at block 2 of rskj regtest's ladder up to `last`, is
/// kept under, waiting for signatures.
fn kept_under(last: RskNetworkUpgrade, creation: Option<[u8; 32]>) -> [u8; 32] {
    let config = regtest_one_confirmation();
    let cfg = up_to(last, RSK_REGTEST_CHAIN_ID);
    let pegout = pegout(0x91);
    let mut ctx = confirming(&cfg, 2, (&pegout, 1, creation), &BTreeMap::new());
    assert!(update(&mut ctx, &config, &cfg).0);
    assert!(
        waiting_pegouts(&mut ctx).is_empty(),
        "no longer waiting for confirmations"
    );
    let signing = waiting_for_signatures(&mut ctx);
    assert_eq!(signing.len(), 1);
    let (key, raw) = signing.into_iter().next().expect("one");
    assert_eq!(raw, bitcoin::consensus::serialize(&pegout));
    key
}

/// `rskTxWaitingForSignature_uses_updateCollection_rskTxHash_before_rskip_146_activation`.
#[test]
fn before_rskip146_a_confirmed_pegout_is_kept_under_the_update_transaction() {
    assert_eq!(kept_under(Wasabi100, None), RSK_TX_HASH);
}

/// `rskTxWaitingForSignature_uses_updateCollection_rskTxHash_after_rskip_146_activation_if_release_transaction_doesnt_have_rstTxHash`.
#[test]
fn from_rskip146_a_confirmed_pegout_of_no_creation_hash_is_kept_under_the_update_transaction() {
    assert_eq!(kept_under(Papyrus200, None), RSK_TX_HASH);
}

/// `rskTxWaitingForSignature_uses_release_transaction_rstTxHash_after_rskip_146_activation`.
#[test]
fn from_rskip146_a_confirmed_pegout_is_kept_under_its_creation_transaction() {
    assert_eq!(kept_under(Papyrus200, Some(CREATION)), CREATION);
}

/// `rskTxWaitingForSignature_uses_updateCollection_rskTxHash_after_rskip_176_activation`.
#[test]
fn from_rskip176_a_confirmed_pegout_is_kept_under_the_update_transaction_again() {
    assert_eq!(kept_under(Iris300, Some(CREATION)), RSK_TX_HASH);
}

/// The events `updateCollections` at block `at` of rskj regtest's ladder up
/// to `last` logs, with a peg-out created at block `created` waiting for
/// confirmations, and whether the peg-out moved to the set waiting for
/// signatures.
fn confirmation_logged(last: RskNetworkUpgrade, created: u64, at: u64) -> (Vec<Event>, bool) {
    let config = regtest_one_confirmation();
    let cfg = up_to(last, RSK_REGTEST_CHAIN_ID);
    let pegout = pegout(0x91);
    let mut ctx = confirming(
        &cfg,
        at,
        (&pegout, created, Some(CREATION)),
        &BTreeMap::new(),
    );
    let (returned, logs) = update(&mut ctx, &config, &cfg);
    assert!(returned);
    (logs, !waiting_for_signatures(&mut ctx).is_empty())
}

/// `rskTxWaitingForSignature_postRSKIP326_emitNewPegoutConfirmedEvent`.
#[test]
fn from_rskip326_a_confirmed_pegout_is_logged() {
    let (logs, moved) = confirmation_logged(Fingerroot500, 1, 2);
    assert!(moved);
    assert!(
        logs.contains(&pegout_confirmed(&pegout(0x91), 1)),
        "{logs:?}"
    );
}

/// `rskTxWaitingForSignature_preRSKIP326_noPegoutConfirmedEventEmitted`.
#[test]
fn before_rskip326_a_confirmed_pegout_is_not_logged() {
    let (logs, moved) = confirmation_logged(Hop400, 1, 2);
    assert!(moved);
    assert_eq!(count_of(&logs, PEGOUT_CONFIRMED), 0);
}

/// `rskTxWaitingForSignature_postRSKIP326NoTxWithEnoughConfirmation_pegoutConfirmedEventNotEmitted`.
#[test]
fn from_rskip326_a_pegout_not_yet_confirmed_is_not_logged() {
    let (logs, moved) = confirmation_logged(Fingerroot500, 2, 2);
    assert!(!moved);
    assert_eq!(count_of(&logs, PEGOUT_CONFIRMED), 0);
}

/// `rskTxWaitingForSignature_uses_pegoutCreation_rskTxHash_after_rskip_375_activation`,
/// on each network.
#[test]
fn from_rskip375_a_confirmed_pegout_is_kept_under_its_creation_transaction() {
    for (config, chain_id) in networks() {
        let cfg = up_to(Fingerroot500, chain_id);
        let at = 2 + u64::from(config.rsk2btc_minimum_acceptable_confirmations);
        let pegout = pegout(0x91);
        let mut ctx = confirming(&cfg, at, (&pegout, 1, Some(CREATION)), &BTreeMap::new());
        assert!(update(&mut ctx, &config, &cfg).0);
        let kept = BTreeMap::from([(CREATION, bitcoin::consensus::serialize(&pegout))]);
        assert_eq!(waiting_for_signatures(&mut ctx), kept, "{chain_id}");
        assert!(waiting_pegouts(&mut ctx).is_empty());
    }
}

/// `rskTxWaitingForSignature_override_entry_is_allowed_before_rskip_375_activation`,
/// on each network, at Hop400: RSKIP146 and RSKIP176 on, RSKIP375 not yet.
/// The confirmed peg-out takes the key of the update transaction, which a
/// peg-out already had.
#[test]
fn before_rskip375_a_confirmed_pegout_takes_the_place_of_one_under_its_key() {
    for (config, chain_id) in networks() {
        let cfg = up_to(Hop400, chain_id);
        let at = 2 + u64::from(config.rsk2btc_minimum_acceptable_confirmations);
        let (confirmed, existing) = (pegout(0x91), pegout(0x92));
        let signing = BTreeMap::from([(RSK_TX_HASH, bitcoin::consensus::serialize(&existing))]);
        let mut ctx = confirming(&cfg, at, (&confirmed, 1, Some(CREATION)), &signing);
        assert!(update(&mut ctx, &config, &cfg).0);
        let kept = BTreeMap::from([(RSK_TX_HASH, bitcoin::consensus::serialize(&confirmed))]);
        assert_eq!(waiting_for_signatures(&mut ctx), kept, "{chain_id}");
        assert!(waiting_pegouts(&mut ctx).is_empty());
    }
}

/// `rskTxWaitingForSignature_override_entry_attempt_after_rskip_375_activation`:
/// from RSKIP375 rskj refuses to keep a confirmed peg-out under a key the
/// set waiting for signatures already has
/// (`BridgeSupport.checkIfEntryExistsInPegoutsWaitingForSignatures`), and
/// `updateCollections` throws: both sets stay as they were.
#[test]
fn from_rskip375_a_confirmed_pegout_under_a_key_already_kept_fails_the_update() {
    let config = regtest_one_confirmation();
    let cfg = up_to(Fingerroot500, RSK_REGTEST_CHAIN_ID);
    let pegout = pegout(0x91);
    let signing = BTreeMap::from([(CREATION, bitcoin::consensus::serialize(&pegout))]);
    let mut ctx = confirming(&cfg, 2, (&pegout, 1, Some(CREATION)), &signing);
    assert!(
        !update(&mut ctx, &config, &cfg).0,
        "updateCollections throws"
    );
    assert_eq!(waiting_for_signatures(&mut ctx), signing);
    assert_eq!(waiting_pegouts(&mut ctx), [pegout]);
}

/// `rskTxWaitingForSignature_fail_adding_an_already_existing_key_after_rskip_375`:
/// two peg-outs made in one transaction, so of one creation hash, the first
/// waiting for signatures under it; the second, confirmed, fails
/// `updateCollections` and both sets stay as they were, until the first,
/// signed, leaves the set, when the second takes its key.
#[test]
fn from_rskip375_a_second_pegout_of_one_creation_waits_until_the_first_is_signed() {
    let config = regtest_one_confirmation();
    let cfg = up_to(Fingerroot500, RSK_REGTEST_CHAIN_ID);
    let (first, second) = (pegout(0x92), pegout(0x91));
    let signing = BTreeMap::from([(CREATION, bitcoin::consensus::serialize(&first))]);
    let mut ctx = confirming(&cfg, 2, (&second, 1, Some(CREATION)), &signing);
    assert!(
        !update(&mut ctx, &config, &cfg).0,
        "updateCollections throws"
    );
    assert_eq!(waiting_for_signatures(&mut ctx), signing);
    assert_eq!(waiting_pegouts(&mut ctx), std::slice::from_ref(&second));
    // The first is signed, and leaves the set.
    storage::bridge_store_bytes_named(
        &mut ctx,
        storage::PEGOUTS_WAITING_FOR_SIGNATURES_KEY,
        &peg::serialize_rsk_txs_waiting_for_signatures(&BTreeMap::new()),
    );
    assert!(update(&mut ctx, &config, &cfg).0);
    let kept = BTreeMap::from([(CREATION, bitcoin::consensus::serialize(&second))]);
    assert_eq!(waiting_for_signatures(&mut ctx), kept);
    assert!(waiting_pegouts(&mut ctx).is_empty());
}

/// From RSKIP375 rskj keys a confirmed peg-out by its creation hash, and a
/// peg-out of none, made before RSKIP146, is looked up under null in its
/// `TreeMap`, which throws: `updateCollections` fails, and both sets stay as
/// they were. Before RSKIP375 such a peg-out is kept under the update
/// transaction. No network reaches it: a peg-out waits for its confirmations
/// some thousands of blocks, and RSKIP146 came long before RSKIP375.
#[test]
fn from_rskip375_a_confirmed_pegout_of_no_creation_hash_fails_the_update() {
    assert_eq!(kept_under(Hop400, None), RSK_TX_HASH);

    let config = regtest_one_confirmation();
    let cfg = up_to(Fingerroot500, RSK_REGTEST_CHAIN_ID);
    let pegout = pegout(0x91);
    let mut ctx = confirming(&cfg, 2, (&pegout, 1, None), &BTreeMap::new());
    assert!(
        !update(&mut ctx, &config, &cfg).0,
        "updateCollections throws"
    );
    assert!(waiting_for_signatures(&mut ctx).is_empty());
    assert_eq!(waiting_pegouts(&mut ctx), [pegout]);
}

/// `getNextPegoutCreationBlockNumber_after_RSKIP271_activation`: the height
/// kept is the one answered.
#[test]
fn the_next_pegout_creation_block_answered_is_the_one_kept() {
    let mut ctx = ctx_at(10);
    storage::bridge_store_u256(&mut ctx, storage::NEXT_PEGOUT_HEIGHT_KEY, U256::from(10u64));
    let answer = peg::get_next_pegout_creation_block_number(&mut ctx, 0).expect("an answer");
    assert_eq!(U256::from_be_slice(&answer.bytes), U256::from(10u64));
}
