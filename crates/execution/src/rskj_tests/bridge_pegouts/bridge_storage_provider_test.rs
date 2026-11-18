//! rskj's `BridgeStorageProviderTest`: which of the Bridge's cells its
//! storage provider reads and writes, on each side of the upgrade that
//! brings each one.
//!
//! rskj's tests hand the provider a mocked repository and check which keys
//! it asks for and which it writes. rustock has no provider: the Bridge's
//! methods read and write the cells themselves, each with the switch of the
//! rule that brings the cell. So each port runs the method, or the function
//! the methods share, at a block of mainnet on either side of the rule's
//! upgrade, and reads the cells it leaves, written as rskj writes them.

use super::support::*;
use crate::bridge::peg::{
    deserialize_rsk_txs_waiting_for_signatures, load_pegout_confirmation_set,
    load_release_request_queue, store_pegout_confirmation_set, PegoutWaitingForConfirmations,
};

/// One bitcoin, in satoshis.
const BTC: u64 = 100_000_000;

/// A Bitcoin transaction of one input, spending the output 0 of `tag`'s
/// transaction.
fn btc_tx(tag: u8) -> Vec<u8> {
    use bitcoin::hashes::Hash;
    bitcoin::consensus::serialize(&bitcoin::Transaction {
        version: bitcoin::transaction::Version::ONE,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![bitcoin::TxIn {
            previous_output: bitcoin::OutPoint {
                txid: bitcoin::Txid::from_byte_array([tag; 32]),
                vout: 0,
            },
            script_sig: bitcoin::ScriptBuf::new(),
            sequence: bitcoin::Sequence::MAX,
            witness: bitcoin::Witness::new(),
        }],
        output: vec![],
    })
}

/// rskj `serializePegoutsWaitingForConfirmations`: each transaction and the
/// block that made it, in the order of the transactions' bytes.
fn legacy_set(entries: &[(Vec<u8>, u64)]) -> Vec<u8> {
    let mut entries = entries.to_vec();
    entries.sort();
    let items: Vec<Vec<u8>> = entries
        .iter()
        .flat_map(|(tx, height)| [rlp_encode_element(tx), serialize_long(*height)])
        .collect();
    rlp_encode_list(&items)
}

/// rskj `serializePegoutsWaitingForConfirmationsWithTxHash`: each
/// transaction, the block that made it and the hash of the transaction that
/// did, in the order of the transactions' bytes.
fn hashed_set(entries: &[(Vec<u8>, u64, [u8; 32])]) -> Vec<u8> {
    let mut entries = entries.to_vec();
    entries.sort();
    let items: Vec<Vec<u8>> = entries
        .iter()
        .flat_map(|(tx, height, hash)| {
            [
                rlp_encode_element(tx),
                serialize_long(*height),
                rlp_encode_element(hash),
            ]
        })
        .collect();
    rlp_encode_list(&items)
}

fn waiting(tx: Vec<u8>, height: u64, hash: Option<[u8; 32]>) -> PegoutWaitingForConfirmations {
    PegoutWaitingForConfirmations {
        btc_tx_raw: tx,
        rsk_block_height: height,
        rsk_tx_hash: hash,
    }
}

// ---------------------------------------------------------------------------
// RSKIP146: the peg-out requests and the peg-outs waiting for confirmations
// keep the hash of the transaction that made them, in cells of their own.
// ---------------------------------------------------------------------------

/// rskj's `saveReleaseRequestQueue_before_rskip_146_activation`: before
/// RSKIP146 a release request carries no transaction hash, and the queue is
/// written to `releaseRequestQueue` alone; `releaseRequestQueueWithTxHash` is
/// never written.
#[test]
fn before_rskip146_a_peg_out_request_is_written_to_the_legacy_queue_alone() {
    let mut bridge = Bridge::mainnet_at(MAINNET_PAPYRUS200 - 1);
    assert!(!bridge.cfg.has_rskip146(bridge.number()));
    bridge.release(BTC);
    let destination = bridge.tx.btc_sender_hash160;
    assert_eq!(
        named(&mut bridge.ctx, "releaseRequestQueue"),
        Some(legacy_queue(&[(destination, BTC)]))
    );
    assert_eq!(
        named(&mut bridge.ctx, "releaseRequestQueueWithTxHash"),
        None
    );
}

/// rskj's `getReleaseRequestQueue_after_rskip_146_activation` and
/// `saveReleaseRequestQueue_after_rskip_146_activation`: from RSKIP146 the
/// queue is the entries of `releaseRequestQueue` followed by those of
/// `releaseRequestQueueWithTxHash`. A new request carries its transaction's
/// hash and goes to the second; the first keeps its entries.
#[test]
fn from_rskip146_a_peg_out_request_is_queued_with_its_hash_after_the_legacy_ones() {
    let mut bridge = Bridge::mainnet_at(MAINNET_PAPYRUS200);
    assert!(bridge.cfg.has_rskip146(bridge.number()));
    let old = ([0x11; 20], 2 * BTC);
    put_named(
        &mut bridge.ctx,
        "releaseRequestQueue",
        &legacy_queue(&[old]),
    );
    bridge.release(BTC);
    let new = (bridge.tx.btc_sender_hash160, BTC, bridge.tx.rsk_tx_hash);
    assert_eq!(
        named(&mut bridge.ctx, "releaseRequestQueue"),
        Some(legacy_queue(&[old]))
    );
    assert_eq!(
        named(&mut bridge.ctx, "releaseRequestQueueWithTxHash"),
        Some(hashed_queue(&[new]))
    );
    let queue: Vec<_> = load_release_request_queue(&mut bridge.ctx, true)
        .into_iter()
        .map(|entry| {
            (
                entry.btc_dest_hash160,
                entry.amount_satoshis,
                entry.rsk_tx_hash,
            )
        })
        .collect();
    assert_eq!(
        queue,
        vec![(old.0, old.1, None), (new.0, new.1, Some(new.2))]
    );
}

/// rskj's `saveReleaseRequestQueue_after_rskip_146_activation` asserts that
/// from RSKIP146 saving the queue writes `releaseRequestQueue` as well as
/// `releaseRequestQueueWithTxHash`. rskj saves the queue at the end of every
/// Bridge call that loaded it (`Bridge.execute`, `teardown`,
/// `BridgeSupport.save`, `BridgeStorageProvider.saveReleaseRequestQueue`),
/// and `releaseBtc` loads it to add the request. Where `releaseRequestQueue`
/// never held anything, it is written as the empty list.
#[test]
fn from_rskip146_a_peg_out_request_writes_the_legacy_queue_back_too() {
    let mut bridge = Bridge::mainnet_at(MAINNET_PAPYRUS200);
    bridge.release(BTC);
    assert!(named(&mut bridge.ctx, "releaseRequestQueueWithTxHash").is_some());
    assert_eq!(
        named(&mut bridge.ctx, "releaseRequestQueue"),
        Some(vec![0xc0]),
        "the empty list, RLP.encodeList()"
    );
}

/// The getters that read the queue from RSKIP271, `getQueuedPegoutsCount`
/// and `getEstimatedFeesForNextPegOutEvent`, end as every Bridge method that
/// returns does in rskj: `teardown` saves what they read, both of the
/// queue's cells, the empty list where they held nothing.
#[test]
fn from_rskip271_the_getters_that_read_the_queue_write_it_back() {
    for method in [
        "getQueuedPegoutsCount()",
        "getEstimatedFeesForNextPegOutEvent()",
    ] {
        let mut bridge = Bridge::mainnet_at(MAINNET_HOP400);
        bridge
            .execute(&crate::abi::selector(method))
            .expect("the getter answers");
        for cell in ["releaseRequestQueue", "releaseRequestQueueWithTxHash"] {
            assert_eq!(
                named(&mut bridge.ctx, cell),
                Some(vec![0xc0]),
                "{method} writes {cell} back"
            );
        }
    }
}

/// From RSKIP540 `getEstimatedFeesForPegOutAmount` simulates the peg-out
/// over the queue it reads, and when it answers, `teardown` writes the
/// queue back as the other getters do.
#[test]
fn from_rskip540_the_estimate_for_an_amount_writes_the_queue_back() {
    let mut collections = Collections::mainnet_at(MAINNET_VETIVER900);
    collections.active(1, 10);
    let wei = alloy_primitives::U256::from(BTC) * alloy_primitives::U256::from(10_000_000_000u64);
    let data = [
        crate::abi::selector("getEstimatedFeesForPegOutAmount(uint256)").to_vec(),
        wei.to_be_bytes::<32>().to_vec(),
    ]
    .concat();
    collections
        .bridge
        .execute(&data)
        .expect("the estimate answers");
    for cell in ["releaseRequestQueue", "releaseRequestQueueWithTxHash"] {
        assert_eq!(
            named(&mut collections.bridge.ctx, cell),
            Some(vec![0xc0]),
            "{cell} is written back"
        );
    }
}

/// rskj's `getPegoutsWaitingForConfirmations_before_rskip_146_activation`
/// and `savePegoutsWaitingForConfirmations_before_rskip_146_activations`:
/// before RSKIP146 the set is what `releaseTransactionSet` holds, and
/// `releaseTransactionSetWithTxHash` is neither read nor written.
#[test]
fn before_rskip146_the_pegouts_waiting_for_confirmations_are_the_legacy_cell_alone() {
    let block = MAINNET_PAPYRUS200 - 1;
    let rskip146 = RskHardforkConfig::mainnet().has_rskip146(block);
    assert!(!rskip146);

    let mut ctx = ctx_at(block);
    put_named(
        &mut ctx,
        "releaseTransactionSet",
        &legacy_set(&[(btc_tx(1), 1)]),
    );
    put_named(
        &mut ctx,
        "releaseTransactionSetWithTxHash",
        &hashed_set(&[(btc_tx(2), 2, [0x22; 32])]),
    );
    let set = load_pegout_confirmation_set(&mut ctx, rskip146);
    assert_eq!(set.len(), 1);
    assert_eq!(
        (set[0].btc_tx_raw.clone(), set[0].rsk_block_height),
        (btc_tx(1), 1)
    );

    let mut ctx = ctx_at(block);
    store_pegout_confirmation_set(
        &mut ctx,
        &[waiting(btc_tx(1), 1, None), waiting(btc_tx(3), 3, None)],
        rskip146,
    );
    assert_eq!(
        named(&mut ctx, "releaseTransactionSet"),
        Some(legacy_set(&[(btc_tx(1), 1), (btc_tx(3), 3)]))
    );
    assert_eq!(named(&mut ctx, "releaseTransactionSetWithTxHash"), None);
}

/// rskj's `getPegoutsWaitingForConfirmations_after_rskip_146_activation` and
/// `savePegoutsWaitingForConfirmations_after_rskip_146_activations`: from
/// RSKIP146 the set is what both cells hold, and it is written back split by
/// whether an entry carries the hash of the transaction that made it.
#[test]
fn from_rskip146_the_pegouts_waiting_for_confirmations_span_both_cells() {
    let rskip146 = RskHardforkConfig::mainnet().has_rskip146(MAINNET_PAPYRUS200);
    assert!(rskip146);

    let mut ctx = ctx_at(MAINNET_PAPYRUS200);
    put_named(
        &mut ctx,
        "releaseTransactionSet",
        &legacy_set(&[(btc_tx(1), 1)]),
    );
    put_named(
        &mut ctx,
        "releaseTransactionSetWithTxHash",
        &hashed_set(&[(btc_tx(2), 2, [0x22; 32])]),
    );
    let set = load_pegout_confirmation_set(&mut ctx, rskip146);
    let read: Vec<_> = set
        .iter()
        .map(|entry| {
            (
                entry.btc_tx_raw.clone(),
                entry.rsk_block_height,
                entry.rsk_tx_hash,
            )
        })
        .collect();
    assert_eq!(
        read,
        vec![(btc_tx(1), 1, None), (btc_tx(2), 2, Some([0x22; 32]))]
    );

    let mut ctx = ctx_at(MAINNET_PAPYRUS200);
    store_pegout_confirmation_set(
        &mut ctx,
        &[
            waiting(btc_tx(1), 1, None),
            waiting(btc_tx(3), 1, Some([0x33; 32])),
        ],
        rskip146,
    );
    assert_eq!(
        named(&mut ctx, "releaseTransactionSet"),
        Some(legacy_set(&[(btc_tx(1), 1)]))
    );
    assert_eq!(
        named(&mut ctx, "releaseTransactionSetWithTxHash"),
        Some(hashed_set(&[(btc_tx(3), 1, [0x33; 32])]))
    );
    assert_eq!(load_pegout_confirmation_set(&mut ctx, rskip146).len(), 2);
}

/// rskj's `getReleaseTransaction_after_rskip_146_activations`: three
/// peg-outs saved with the hashes of the transactions that made them read
/// back as three, and none of them waits for signatures.
#[test]
fn from_rskip146_saved_pegouts_read_back_and_none_waits_for_signatures() {
    let mut ctx = ctx_at(MAINNET_PAPYRUS200);
    let pegouts: Vec<_> = (1..=3u8)
        .map(|n| waiting(btc_tx(n), u64::from(n), Some([n; 32])))
        .collect();
    store_pegout_confirmation_set(&mut ctx, &pegouts, true);
    assert_eq!(load_pegout_confirmation_set(&mut ctx, true).len(), 3);
    let signing = named(&mut ctx, "rskTxsWaitingFS").unwrap_or_default();
    assert!(deserialize_rsk_txs_waiting_for_signatures(&signing).is_empty());
}

// ---------------------------------------------------------------------------
// RSKIP134: a processed peg-in's hash gets a cell of its own, where before it
// went into one map of them all.
// ---------------------------------------------------------------------------

/// A Bitcoin transaction's hash in its internal byte order, and as bitcoinj's
/// `Sha256Hash` of it reads, which is the other way round.
fn btc_hash(tag: u8) -> ([u8; 32], [u8; 32]) {
    let mut internal = [tag; 32];
    internal[0] = 0x01;
    let mut display = internal;
    display.reverse();
    (internal, display)
}

/// rskj `serializeMapOfHashesToLong` of a map of one hash.
fn processed_map(display: &[u8; 32], height: u64) -> Vec<u8> {
    rlp_encode_list(&[rlp_encode_element(display), serialize_long(height)])
}

/// rskj's `BTC_TX_HASH_AP.getCompoundKey("-", hash.toString())`.
fn processed_cell(display: &[u8; 32]) -> alloy_primitives::U256 {
    compound("btcTxHashAP", &alloy_primitives::hex::encode(display))
}

/// rskj's `setHeightBtcTxhashAlreadyProcessed_before_RSKIP134_does_not_use_new_storage`
/// and `getHeightIfBtcTxhashIsAlreadyProcessed_before_RSKIP134_does_not_use_new_storage`:
/// before RSKIP134 a processed peg-in's hash goes into `btcTxHashesAP`, the
/// map of them all, and is read from it with its height.
#[test]
fn before_rskip134_a_processed_peg_in_is_kept_in_the_map_of_them_all() {
    let block = MAINNET_PAPYRUS200 - 1;
    let rskip134 = RskHardforkConfig::mainnet().has_rskip134(block);
    assert!(!rskip134);
    let (internal, display) = btc_hash(0xab);
    let mut ctx = ctx_at(block);
    crate::bridge::tx::set_btc_tx_processed(&mut ctx, &internal, 1, rskip134);
    assert_eq!(
        named(&mut ctx, "btcTxHashesAP"),
        Some(processed_map(&display, 1))
    );
    assert_eq!(cell(&mut ctx, processed_cell(&display)), None);
    assert_eq!(
        crate::bridge::tx::get_btc_tx_processed_height(&mut ctx, &internal),
        Some(1)
    );
}

/// rskj's `setHeightBtcTxhashAlreadyProcessed_before_RSKIP134_uses_new_storage`,
/// whose activations are all of them: from RSKIP134 a processed peg-in's
/// hash gets a cell of its own, holding its height, and the map is not
/// touched.
#[test]
fn from_rskip134_a_processed_peg_in_is_kept_in_a_cell_of_its_own() {
    let rskip134 = RskHardforkConfig::mainnet().has_rskip134(MAINNET_PAPYRUS200);
    assert!(rskip134);
    let (internal, display) = btc_hash(0xab);
    let mut ctx = ctx_at(MAINNET_PAPYRUS200);
    crate::bridge::tx::set_btc_tx_processed(&mut ctx, &internal, 1, rskip134);
    assert_eq!(
        cell(&mut ctx, processed_cell(&display)),
        Some(serialize_long(1))
    );
    assert_eq!(named(&mut ctx, "btcTxHashesAP"), None);
    assert_eq!(
        crate::bridge::tx::get_btc_tx_processed_height(&mut ctx, &internal),
        Some(1)
    );
}

/// rskj's `getHeightIfBtcTxhashIsAlreadyProcessed_after_RSKIP134_uses_new_storage`:
/// from RSKIP134 a hash the map holds reads from the map, and one the map
/// doesn't hold from its own cell, every time it is asked.
#[test]
fn from_rskip134_a_processed_peg_in_is_read_from_the_map_or_its_own_cell() {
    let (in_map, in_map_display) = btc_hash(0x01);
    let (in_cell, in_cell_display) = btc_hash(0x02);
    let mut ctx = ctx_at(MAINNET_PAPYRUS200);
    put_named(
        &mut ctx,
        "btcTxHashesAP",
        &processed_map(&in_map_display, 1),
    );
    crate::bridge::storage::bridge_store_raw(
        &mut ctx,
        processed_cell(&in_cell_display),
        Some(serialize_long(2)),
    );
    let height = |ctx: &mut Ctx, hash| crate::bridge::tx::get_btc_tx_processed_height(ctx, hash);
    assert_eq!(height(&mut ctx, &in_map), Some(1));
    assert_eq!(height(&mut ctx, &in_cell), Some(2));
    assert_eq!(height(&mut ctx, &in_cell), Some(2));
}

// ---------------------------------------------------------------------------
// RSKIP271: peg-outs are made in batches, the next batch not before the
// block `nextPegoutHeight` names.
// ---------------------------------------------------------------------------

/// The Bridge at `block` of mainnet, its genesis federation holding ten
/// bitcoins, a peg-out request of one queued, and the next batch named for
/// a block far ahead.
fn a_request_and_a_batch_far_ahead(block: u64) -> Bridge {
    let mut bridge = Bridge::mainnet_at(block);
    let (constants, cfg) = (bridge.constants.clone(), bridge.cfg.clone());
    fund_genesis_federation(&mut bridge.ctx, &constants, &cfg, block, 10 * BTC);
    put_named(
        &mut bridge.ctx,
        "releaseRequestQueueWithTxHash",
        &hashed_queue(&[([0x44; 20], BTC, [0x45; 32])]),
    );
    put_named(
        &mut bridge.ctx,
        "nextPegoutHeight",
        &serialize_long(MAINNET_HOP400 + 10_000),
    );
    bridge
}

/// rskj's `getNextPegoutHeight_before_RSKIP271_activation` and
/// `saveNextPegoutHeight_before_RSKIP271`: before RSKIP271 the provider
/// answers no next height without reading the cell and never writes it, so
/// `updateCollections` makes a peg-out of every request whatever the cell
/// holds. From RSKIP271 the same request waits for the block it names.
#[test]
fn before_rskip271_the_next_pegout_height_is_neither_read_nor_written() {
    let mut bridge = a_request_and_a_batch_far_ahead(MAINNET_HOP400 - 1);
    assert!(!bridge.cfg.has_rskip271(bridge.number()));
    bridge.update_collections();
    let queue = load_release_request_queue(&mut bridge.ctx, true);
    assert!(queue.is_empty(), "the request was made a peg-out");
    assert_eq!(load_pegout_confirmation_set(&mut bridge.ctx, true).len(), 1);
    assert_eq!(
        named(&mut bridge.ctx, "nextPegoutHeight"),
        Some(serialize_long(MAINNET_HOP400 + 10_000))
    );

    let mut bridge = a_request_and_a_batch_far_ahead(MAINNET_HOP400);
    assert!(bridge.cfg.has_rskip271(bridge.number()));
    bridge.update_collections();
    assert_eq!(load_release_request_queue(&mut bridge.ctx, true).len(), 1);
    assert!(load_pegout_confirmation_set(&mut bridge.ctx, true).is_empty());
}

/// rskj's `saveNextPegoutHeight_after_RSKIP271` and
/// `setNextPegoutHeightAndGetNextPegoutHeight_after_RSKIP271_activation`:
/// from RSKIP271 `updateCollections` with no request queued names the next
/// batch's block, `nextPegoutHeight` holds it as rskj's `serializeLong`
/// writes it, and `getNextPegoutCreationBlockNumber` reads it back. Before
/// it the cell is never written.
#[test]
fn from_rskip271_the_next_pegout_height_is_written_and_read_back() {
    let mut bridge = Bridge::mainnet_at(MAINNET_HOP400 - 1);
    bridge.update_collections();
    assert_eq!(named(&mut bridge.ctx, "nextPegoutHeight"), None);

    let mut bridge = Bridge::mainnet_at(MAINNET_HOP400);
    bridge.update_collections();
    let next = MAINNET_HOP400 + bridge.constants.number_of_blocks_between_pegouts;
    assert_eq!(
        named(&mut bridge.ctx, "nextPegoutHeight"),
        Some(serialize_long(next))
    );
    let answer = crate::bridge::peg::get_next_pegout_creation_block_number(&mut bridge.ctx, 0)
        .expect("getNextPegoutCreationBlockNumber answers");
    assert_eq!(
        alloy_primitives::U256::from_be_slice(&answer.bytes),
        alloy_primitives::U256::from(next)
    );
}

// ---------------------------------------------------------------------------
// RSKIP143: a Bitcoin block's coinbase information, the root of its
// transactions' witnesses, registered by `registerBtcCoinbaseTransaction`.
// ---------------------------------------------------------------------------

/// rskj's `getCoinBaseInformation_before_RSKIP143` and
/// `getCoinBaseInformation_after_RSKIP143`: the coinbase information of a
/// Bitcoin block is read from RSKIP143, and before it the provider answers
/// none without reading it. rskj reads it where a peg-in's tree doesn't come
/// to the block's merkle root (`isBlockMerkleRootValid`): from RSKIP143 a
/// tree that comes to the block's witness root proves the peg-in, which is
/// taken; before it the peg-in is left alone.
#[test]
fn from_rskip143_a_blocks_coinbase_information_proves_a_peg_in() {
    use RskNetworkUpgrade::{Papyrus200, TwoToThree};
    let federation = keys(1, 3);
    let tx = tx_paying(&[(BTC, p2sh(&hash160(&standard_redeem(&federation))))]);
    let raw = bitcoin::consensus::serialize(&tx);
    let hash = sha256d(&raw);
    for (last, taken) in [(TwoToThree, false), (Papyrus200, true)] {
        let cfg = up_to(last, RSK_REGTEST_CHAIN_ID);
        let mut ctx = ctx_at(1_000);
        let headers = btc_chain(&mut ctx, &cfg, |_| [0x11; 32]);
        store_federation(&mut ctx, NEW_FEDERATION, &federation, 1, 1000);
        fund_bridge(&mut ctx);
        // registerBtcCoinbaseTransaction's witness root, as it stores the
        // argument: display order.
        let block = block_hash(&headers[PEGIN_HEIGHT as usize - 1]);
        crate::bridge::storage::bridge_store_raw(
            &mut ctx,
            compound("coinbaseInformation", &alloy_primitives::hex::encode(block)),
            Some(rlp_encode_list(&[rlp_encode_element(&reversed(hash))])),
        );
        let tree = one_leaf_tree(&hash);
        let tx_ctx = BridgeTxContext::default();
        register(&mut ctx, &cfg, &tx_ctx, &raw, PEGIN_HEIGHT, &tree).expect("it answers");
        assert_eq!(
            crate::bridge::tx::is_btc_tx_processed(&mut ctx, &hash),
            taken,
            "up to {last:?}"
        );
    }
}

/// rskj's `setCoinBaseInformation_after_RSKIP143` and
/// `saveCoinBaseInformation_after_RSKIP143`: the coinbase information of a
/// block is stored under `coinbaseInformation-<block hash>` as rskj's
/// `serializeCoinbaseInformation` writes it, a list of the witness root, and
/// read back.
#[test]
fn a_blocks_coinbase_information_is_stored_as_rskj_serializes_it() {
    let (block, root) = ([0x0b; 32], [0x5f; 32]);
    let mut ctx = ctx_at(MAINNET_PAPYRUS200);
    crate::bridge::tx::set_coinbase_information(&mut ctx, &block, &root);
    assert_eq!(
        cell(
            &mut ctx,
            compound("coinbaseInformation", &alloy_primitives::hex::encode(block))
        ),
        Some(rlp_encode_list(&[rlp_encode_element(&root)]))
    );
    assert_eq!(
        crate::bridge::tx::get_coinbase_information(&mut ctx, &block),
        Some(root)
    );
}

// ---------------------------------------------------------------------------
// RSKIP199: the Bitcoin main chain's blocks indexed by their heights.
// ---------------------------------------------------------------------------

/// rskj's `saveBtcBlocksIndex_beforeRskip199` and
/// `saveBtcBlocksIndex_afterRskip199`: from RSKIP199 each block that becomes
/// the head of the Bitcoin chain is indexed by its height, the cell
/// `btcBlockHeight-<height>` holding its hash as rskj's `serializeSha256Hash`
/// writes it; before it nothing is indexed.
#[test]
fn from_rskip199_each_block_the_chain_takes_is_indexed_by_its_height() {
    use RskNetworkUpgrade::{Iris300, Papyrus200};
    let headers = regtest_headers(3, |_| [0x11; 32]);
    for (last, indexed) in [(Papyrus200, false), (Iris300, true)] {
        let cfg = up_to(last, RSK_REGTEST_CHAIN_ID);
        let mut ctx = ctx_at(1);
        receive_headers(&mut ctx, &headers, &cfg);
        for (height, header) in (1u32..).zip(&headers) {
            assert_eq!(
                cell(&mut ctx, compound("btcBlockHeight", &height.to_string())),
                indexed.then(|| rlp_encode_element(&block_hash(header))),
                "height {height} up to {last:?}"
            );
        }
    }
}

/// rskj's `getBtcBestBlockHashByHeight_beforeRskip199`,
/// `getBtcBestBlockHashByHeight_afterRskip199` and
/// `getBtcBestBlockHashByHeight_afterRskip199_hashNotFound`: from RSKIP199 the
/// main chain's block at a height is the one its index names, and where the
/// index names none the chain is walked back from its head; before it the
/// index is never read and the chain is walked.
#[test]
fn from_rskip199_the_main_chain_block_at_a_height_is_the_one_its_index_names() {
    use crate::bridge::btc_chain::{main_chain_block_at_height, MainChainLookup};
    let cfg = up_to(RskNetworkUpgrade::Iris300, RSK_REGTEST_CHAIN_ID);
    let headers = regtest_headers(3, |_| [0x11; 32]);
    let mut ctx = ctx_at(1);
    receive_headers(&mut ctx, &headers, &cfg);
    // A block beside the main chain's second, stored, with less work than
    // the head: it doesn't become the head.
    let beside = mine(headers[0].block_hash(), [0xf1; 32]);
    receive_headers(&mut ctx, &[beside], &cfg);
    let index = compound("btcBlockHeight", "2");
    crate::bridge::storage::bridge_store_raw(
        &mut ctx,
        index,
        Some(rlp_encode_element(&block_hash(&beside))),
    );
    let at_2 = |ctx: &mut Ctx, rskip199| match main_chain_block_at_height(ctx, 2, rskip199, None) {
        MainChainLookup::Found(block) => Some(block.header.block_hash()),
        _ => None,
    };
    assert_eq!(at_2(&mut ctx, false), Some(headers[1].block_hash()));
    assert_eq!(at_2(&mut ctx, true), Some(beside.block_hash()));
    crate::bridge::storage::bridge_store_raw(&mut ctx, index, None);
    assert_eq!(at_2(&mut ctx, true), Some(headers[1].block_hash()));
}

// ---------------------------------------------------------------------------
// RSKIP176: Flyover's peg-ins, each marked by its transaction and its
// derivation hash, the federation's Flyover address described by its hash.
// ---------------------------------------------------------------------------

/// The contract that registers Flyover payments.
const LBC: alloy_primitives::Address = alloy_primitives::Address::repeat_byte(0x42);

/// The user's and the liquidity provider's address: a regtest P2PKH address,
/// its version and its hash.
const USER: [u8; 21] = {
    let mut address = [0x51; 21];
    address[0] = 0x6f;
    address
};

/// The hash of the payment's derivation arguments.
const ARGUMENTS: [u8; 32] = [0x11; 32];

/// rskj `PegUtils.getFlyoverDerivationHash` from RSKIP284: the Keccak-256 of
/// the arguments' hash, the user's refund address, the LBC and the liquidity
/// provider's address.
fn derivation_hash() -> [u8; 32] {
    use sha3::Digest;
    sha3::Keccak256::digest([&ARGUMENTS[..], &USER, LBC.as_slice(), &USER].concat()).into()
}

/// rskj `FlyoverRedeemScriptBuilderImpl.of`: the derivation hash pushed and
/// dropped before the federation's redeem script.
fn flyover_redeem(federation: &[[u8; 33]]) -> Vec<u8> {
    [
        &[0x20][..],
        &derivation_hash(),
        &[0x75],
        &standard_redeem(federation),
    ]
    .concat()
}

/// The output script of the Flyover address: the P2SH of its redeem script.
fn flyover_script(federation: &[[u8; 33]]) -> Vec<u8> {
    p2sh(&hash160(&flyover_redeem(federation)))
}

/// rskj's `fastBridgeHashUsedInBtcTx-<btc tx hash><derivation hash>`, the hashes
/// as bitcoinj's `Sha256Hash` and rskj's `Keccak256` print them.
fn used_mark(btc_tx_hash: &[u8; 32]) -> alloy_primitives::U256 {
    compound(
        "fastBridgeHashUsedInBtcTx",
        &format!(
            "{}{}",
            alloy_primitives::hex::encode(reversed(*btc_tx_hash)),
            alloy_primitives::hex::encode(derivation_hash())
        ),
    )
}

/// What the LBC's registration of a Flyover payment of a bitcoin to the
/// federation of `keys(1, 3)` is answered with, at block 1,000 of rskj's
/// regtest, with the cell of its mark holding `mark`; the Bridge after it,
/// and the payment's hash.
fn register_flyover(mark: Option<Vec<u8>>) -> (i128, Ctx, [u8; 32]) {
    use crate::abi::encode::{self, Arg};
    let cfg = RskHardforkConfig::rskj_regtest();
    let federation = keys(1, 3);
    let tx = tx_paying(&[(BTC, flyover_script(&federation))]);
    let raw = bitcoin::consensus::serialize(&tx);
    let hash = sha256d(&raw);
    let mut ctx = ctx_at(1_000);
    btc_chain(&mut ctx, &cfg, |_| hash);
    store_federation(&mut ctx, NEW_FEDERATION, &federation, 1, 1000);
    fund_bridge(&mut ctx);
    if let Some(mark) = mark {
        crate::bridge::storage::bridge_store_raw(&mut ctx, used_mark(&hash), Some(mark));
    }
    let mut lbc = [0u8; 32];
    lbc[12..].copy_from_slice(LBC.as_slice());
    let tree = one_leaf_tree(&hash);
    let args = encode::args(&[
        Arg::Bytes(&raw),
        Arg::Word(encode::int(i64::from(PEGIN_HEIGHT))),
        Arg::Bytes(&tree),
        Arg::Word(ARGUMENTS),
        Arg::Bytes(&USER),
        Arg::Word(lbc),
        Arg::Bytes(&USER),
        Arg::Word([0; 32]),
    ]);
    let answer = crate::bridge::peg::register_fast_bridge_btc_transaction(
        &mut ctx,
        &args,
        0,
        &BridgeConstants::rskj_regtest(),
        &cfg,
        &BridgeTxContext::default(),
        LBC,
        2,
    )
    .expect("registerFastBridgeBtcTransaction answers");
    (int256(&answer.bytes), ctx, hash)
}

/// rskj's `saveDerivationArgumentsScriptHash_afterRSKIP176_ok` and
/// `isFlyoverFederationDerivationHashUsed_afterRSKIP176_returnTrue`: a
/// registered payment's derivation hash is marked used in its cell with rskj's
/// one true byte, and with it marked the payment is answered
/// UNPROCESSABLE_TX_ALREADY_PROCESSED_ERROR, -302.
#[test]
fn a_registered_flyover_payment_is_marked_with_one_true_byte() {
    let (answer, mut ctx, hash) = register_flyover(None);
    assert_eq!(answer, i128::from(BTC) * i128::from(WEI_PER_SATOSHI));
    assert_eq!(cell(&mut ctx, used_mark(&hash)), Some(vec![1]));
    let (answer, _, _) = register_flyover(Some(vec![1]));
    assert_eq!(answer, -302);
}

/// rskj's `isFlyoverFederationDerivationHashUsed_storageReturnsNull_returnFalse`,
/// `_storageReturnsEmpty_returnFalse` and `_storageReturnsWrongValue_returnFalse`:
/// a payment is marked used only by its cell holding the one byte 1. With the
/// cell holding nothing, or any other bytes, it is registered. (A cell can't
/// hold no bytes: writing none clears it.)
#[test]
fn a_flyover_mark_of_anything_but_one_true_byte_is_no_mark() {
    let locked = i128::from(BTC) * i128::from(WEI_PER_SATOSHI);
    for mark in [
        None,
        Some(vec![]),
        Some(vec![0]),
        Some(vec![2]),
        Some(vec![1, 1]),
    ] {
        let (answer, _, _) = register_flyover(mark.clone());
        assert_eq!(answer, locked, "marked {mark:?}");
    }
}

// ---------------------------------------------------------------------------
// RSKIP200: `receiveHeader` keeps the time of the last header it took.
// ---------------------------------------------------------------------------

/// rskj `receiveHeader` of `header` in a block of time `timestamp`: its answer.
fn receive_header(
    ctx: &mut Ctx,
    header: &bitcoin::block::Header,
    cfg: &RskHardforkConfig,
    timestamp: u64,
) -> i128 {
    use crate::abi::encode::{self, Arg};
    ctx.block.timestamp = alloy_primitives::U256::from(timestamp);
    let args = encode::args(&[Arg::Bytes(&bitcoin::consensus::serialize(header))]);
    let answer = crate::bridge::btc_chain::receive_header(
        ctx,
        &args,
        0,
        &BridgeConstants::rskj_regtest(),
        false,
        cfg,
    )
    .expect("receiveHeader answers");
    int256(&answer.bytes)
}

/// rskj's `getReceiveHeadersLastTimestamp_after_RSKIP200`,
/// `saveReceiveHeadersLastTimestamp_after_RSKIP200`,
/// `getReceiveHeadersLastTimestamp_not_in_repository` and
/// `saveReceiveHeadersLastTimestamp_not_set`: from RSKIP200 `receiveHeader`
/// keeps the time of the block that took a header, `receiveHeadersLastTimestamp`
/// holding it as `serializeLong` writes it, and reads it back: a header within
/// `minSecondsBetweenCallsToReceiveHeader` of it, 300 on rskj's regtest, is
/// answered RECEIVE_HEADER_CALLED_TOO_SOON, -1, and the time is left as it
/// was. With no time kept a header is taken; `receiveHeaders` keeps no time.
#[test]
fn from_rskip200_receive_header_keeps_and_reads_the_time_of_the_last_header() {
    let cfg = up_to(RskNetworkUpgrade::Iris300, RSK_REGTEST_CHAIN_ID);
    let headers = regtest_headers(5, |_| [0x11; 32]);
    let mut ctx = ctx_at(1);
    receive_headers(&mut ctx, &headers[..3], &cfg);
    assert_eq!(named(&mut ctx, "receiveHeadersLastTimestamp"), None);

    let time = 1_700_000_000;
    assert_eq!(receive_header(&mut ctx, &headers[3], &cfg, time), 0);
    assert_eq!(
        named(&mut ctx, "receiveHeadersLastTimestamp"),
        Some(serialize_long(time))
    );
    assert_eq!(receive_header(&mut ctx, &headers[4], &cfg, time + 299), -1);
    assert_eq!(
        named(&mut ctx, "receiveHeadersLastTimestamp"),
        Some(serialize_long(time))
    );
    assert_eq!(receive_header(&mut ctx, &headers[4], &cfg, time + 300), 0);
    assert_eq!(
        named(&mut ctx, "receiveHeadersLastTimestamp"),
        Some(serialize_long(time + 300))
    );
}

/// The data the last push of `script` pushes.
fn last_push(script: &bitcoin::ScriptBuf) -> Vec<u8> {
    script
        .instructions()
        .filter_map(
            |instruction| match instruction.expect("a script that parses") {
                bitcoin::script::Instruction::PushBytes(data) => Some(data.as_bytes().to_vec()),
                bitcoin::script::Instruction::Op(_) => None,
            },
        )
        .last()
        .expect("a push")
}

/// rskj's `getFlyoverFederationInformation_afterRSKIP176_ok` and
/// `getFlyoverFederationInformation_notFound`: the Flyover information stored
/// under the hash of a Flyover address's redeem script is read when a peg-out
/// spends a UTXO paid to that address (`FlyoverCompatibleBtcWalletWithStorage
/// .findRedeemDataFromScriptHash`), and the input is made to be signed with
/// the redeem script it names: the derivation hash, dropped, before the redeem
/// script of the federation it names. For a UTXO paid to the federation's own
/// address no information is found, and its input is made with the
/// federation's redeem script.
#[test]
fn a_pegout_spends_a_flyover_utxo_with_the_redeem_script_its_information_names() {
    use bitcoin::hashes::Hash;
    let cfg = RskHardforkConfig::rskj_regtest();
    let federation = keys(1, 3);
    let redeem = standard_redeem(&federation);
    let mut bridge = Bridge::new(cfg.clone(), BridgeConstants::rskj_regtest(), 1_000);
    store_federation(&mut bridge.ctx, NEW_FEDERATION, &federation, 1, 1000);
    let utxo = |tag: u8, script: Vec<u8>| crate::bridge::storage::BridgeUtxo {
        tx_hash: [tag; 32],
        vout: 0,
        value_satoshis: BTC,
        height: 1,
        script,
        coinbase: false,
    };
    crate::bridge::storage::store_federation_utxos(
        &mut bridge.ctx,
        &cfg,
        1_000,
        &[
            utxo(0xf1, flyover_script(&federation)),
            utxo(0xfe, p2sh(&hash160(&redeem))),
        ],
    );
    crate::bridge::storage::bridge_store_raw(
        &mut bridge.ctx,
        compound(
            "fastBridgeFederationInformation",
            &alloy_primitives::hex::encode(hash160(&flyover_redeem(&federation))),
        ),
        Some(rlp_encode_list(&[
            rlp_encode_element(&derivation_hash()),
            rlp_encode_element(&hash160(&redeem)),
        ])),
    );
    put_named(
        &mut bridge.ctx,
        "releaseRequestQueueWithTxHash",
        &hashed_queue(&[([0x44; 20], 3 * BTC / 2, [0x45; 32])]),
    );
    bridge.update_collections();

    let set = load_pegout_confirmation_set(&mut bridge.ctx, true);
    assert_eq!(set.len(), 1, "one peg-out");
    let pegout: bitcoin::Transaction =
        bitcoin::consensus::deserialize(&set[0].btc_tx_raw).expect("a transaction");
    let mut spent: Vec<(u8, Vec<u8>)> = pegout
        .input
        .iter()
        .map(|input| {
            let tag = input.previous_output.txid.to_byte_array()[0];
            (tag, last_push(&input.script_sig))
        })
        .collect();
    spent.sort();
    assert_eq!(
        spent,
        vec![(0xf1, flyover_redeem(&federation)), (0xfe, redeem)]
    );
}
