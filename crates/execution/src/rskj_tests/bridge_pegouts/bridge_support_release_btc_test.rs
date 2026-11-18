//! rskj's `BridgeSupportReleaseBtcTest`: here its tests of what RSKIP185
//! changes in `releaseBtc`, of what RSKIP427 changes in a refund and in the
//! amount a turned-down request is logged with, and of the batches RSKIP271
//! builds in `updateCollections`. rskj's tests run `BridgeSupport` under
//! papyrus200, arrowhead631 and every rule; the ports run mainnet's Bridge
//! at the block before iris300, which brings RSKIP185, at iris300, at
//! hop400, which brings RSKIP271, and at the block before lovell700, which
//! brings RSKIP427, and at lovell700.

use super::support::*;
use alloy_primitives::{Address, U256};
use revm::precompile::{PrecompileError, PrecompileOutput};

const BEFORE_RSKIP185: u64 = MAINNET_IRIS300 - 1;
const FROM_RSKIP185: u64 = MAINNET_IRIS300;

/// A request below every minimum, in satoshis.
const LOW: u64 = 1_000;

/// A contract that calls the Bridge.
const CONTRACT: Address = Address::repeat_byte(0xc0);

/// `satoshis` in wei.
fn wei(satoshis: u64) -> U256 {
    U256::from(satoshis) * U256::from(WEI_PER_SATOSHI)
}

/// The requests queued.
fn queued(bridge: &mut Bridge) -> usize {
    crate::bridge::peg::load_release_request_queue(&mut bridge.ctx, true).len()
}

/// An address as an indexed topic holds it.
fn as_topic(address: Address) -> [u8; 32] {
    let mut topic = [0u8; 32];
    topic[12..].copy_from_slice(address.as_slice());
    topic
}

/// `releaseBtc` of `satoshis` reached from [`CONTRACT`], a call from its
/// code, the value already at the Bridge.
fn release_from_a_contract(
    bridge: &mut Bridge,
    satoshis: u64,
) -> Result<PrecompileOutput, PrecompileError> {
    bridge.caller = CONTRACT;
    let bridge_address = crate::precompiles::BRIDGE_ADDR;
    let held = bridge.balance(bridge_address);
    bridge.set_balance(bridge_address, held + wei(satoshis));
    let tx = BridgeTxContext {
        rsk_sender: CONTRACT,
        ..bridge.tx.clone()
    };
    crate::bridge::peg::release_btc(
        &mut bridge.ctx,
        0,
        &bridge.constants,
        &bridge.cfg,
        &tx,
        CONTRACT,
        2,
        wei(satoshis),
    )
}

/// rskj's `release_after_papyrus_before_iris` and
/// `release_after_iris_before_fingerroot`: a request that is accepted is
/// queued on both sides, and from RSKIP185 logged, `release_request_received`.
#[test]
fn from_rskip185_an_accepted_request_is_logged() {
    let received = event_topic("release_request_received(address,bytes,uint256)");
    for (number, expected) in [(BEFORE_RSKIP185, vec![]), (FROM_RSKIP185, vec![received])] {
        let mut bridge = Bridge::mainnet_at(number);
        bridge.release(BTC);
        assert_eq!(queued(&mut bridge), 1, "at {number}");
        let logs = logs(&mut bridge.ctx);
        let firsts: Vec<[u8; 32]> = logs.iter().map(|log| topics(log)[0]).collect();
        assert_eq!(firsts, expected, "at {number}");
    }
}

/// rskj's `release_after_papyrus_before_iris_rejected_lowAmount` and
/// `release_after_iris_rejected_lowAmount`: a request below the minimum is
/// not queued. Before RSKIP185 the Bridge keeps its value and logs nothing;
/// from it the value goes back to the sender, and `release_request_rejected`
/// logs it with the reason LOW_AMOUNT. As in rskj's test, the fee per
/// kilobyte is 5,000 satoshis, which leaves the minimum the larger bound;
/// at mainnet's genesis fee per kilobyte the fee would be, and the reason
/// FEE_ABOVE_VALUE.
#[test]
fn from_rskip185_a_request_below_the_minimum_is_refunded_and_logged() {
    let rejected = event_topic("release_request_rejected(address,uint256,int256)");
    let bridge_address = crate::precompiles::BRIDGE_ADDR;
    let mut bridge = Bridge::mainnet_at(BEFORE_RSKIP185);
    bridge.release(LOW);
    assert_eq!(queued(&mut bridge), 0);
    assert_eq!(bridge.balance(STRANGER), U256::ZERO);
    assert_eq!(bridge.balance(bridge_address), wei(LOW));
    assert!(logs(&mut bridge.ctx).is_empty());

    let mut bridge = Bridge::mainnet_at(FROM_RSKIP185);
    put_named(
        &mut bridge.ctx,
        "feePerKb",
        &rlp_encode_element(&[0x13, 0x88]),
    );
    bridge.release(LOW);
    assert_eq!(queued(&mut bridge), 0);
    assert_eq!(bridge.balance(STRANGER), wei(LOW));
    assert_eq!(bridge.balance(bridge_address), U256::ZERO);
    let logs = logs(&mut bridge.ctx);
    assert_eq!(logs.len(), 1);
    assert_eq!(topics(&logs[0]), [rejected, as_topic(STRANGER)]);
    assert_eq!(word(&logs[0].data.data, 0), U256::from(LOW));
    assert_eq!(word(&logs[0].data.data, 1), U256::from(1), "LOW_AMOUNT");
}

/// From RSKIP271 rskj sizes the peg-out whose fee a request must cover by
/// the active federation's format (`BridgeUtils.calculatePegoutTxSize`): a
/// P2SH-P2WSH ERP federation's spend weighs its witness at a quarter, a P2SH
/// ERP federation's counts it whole. At 0.02 BTC a kilobyte, a request of
/// 0.03 BTC covers the first and not the second, which turns it down with
/// the reason FEE_ABOVE_VALUE.
#[test]
fn from_rskip271_a_segwit_federations_pegout_is_sized_by_its_weight() {
    let rejected = event_topic("release_request_rejected(address,uint256,int256)");
    for (format, accepted) in [(P2SH_ERP, false), (P2SH_P2WSH_ERP, true)] {
        let mut bridge = Bridge::mainnet_at(MAINNET_VETIVER900);
        store_federation(&mut bridge.ctx, NEW_FEDERATION, &active(), 1, format);
        put_named(
            &mut bridge.ctx,
            "feePerKb",
            &rlp_encode_element(&[0x1e, 0x84, 0x80]),
        );
        bridge.release(3 * CENT);
        assert_eq!(
            queued(&mut bridge),
            usize::from(accepted),
            "format {format}"
        );
        let logs = logs(&mut bridge.ctx);
        let turned_down: Vec<_> = logs
            .iter()
            .filter(|log| topics(log)[0] == rejected)
            .collect();
        if accepted {
            assert!(turned_down.is_empty());
        } else {
            assert_eq!(turned_down.len(), 1);
            assert_eq!(
                word(&turned_down[0].data.data, 1),
                U256::from(3),
                "FEE_ABOVE_VALUE"
            );
        }
    }
}

/// rskj's `release_after_papyrus_before_iris_rejected_contractCaller_throws_exception`
/// and `release_after_iris_rejected_contractCaller_emits_rejection_event`: a
/// contract's request throws before RSKIP185, rskj's `OutOfGasException`,
/// which `Bridge.execute` wraps as any other throw (not a want of gas); from
/// it the call returns, the value is not sent back, and
/// `release_request_rejected` logs the contract with the reason
/// CALLER_CONTRACT.
#[test]
fn from_rskip185_a_contracts_request_is_logged_and_not_refunded() {
    let mut bridge = Bridge::mainnet_at(BEFORE_RSKIP185);
    let thrown = release_from_a_contract(&mut bridge, BTC).expect_err("releaseBtc throws");
    assert!(!thrown.is_oog(), "a throw, not a want of gas: {thrown:?}");

    let rejected = event_topic("release_request_rejected(address,uint256,int256)");
    let mut bridge = Bridge::mainnet_at(FROM_RSKIP185);
    release_from_a_contract(&mut bridge, BTC).expect("releaseBtc answers");
    assert_eq!(queued(&mut bridge), 0);
    assert_eq!(bridge.balance(CONTRACT), U256::ZERO);
    assert_eq!(bridge.balance(crate::precompiles::BRIDGE_ADDR), wei(BTC));
    let logs = logs(&mut bridge.ctx);
    assert_eq!(logs.len(), 1);
    assert_eq!(topics(&logs[0]), [rejected, as_topic(CONTRACT)]);
    assert_eq!(
        word(&logs[0].data.data, 1),
        U256::from(2),
        "CALLER_CONTRACT"
    );
}

/// The block before RSKIP427 and the block that brings it.
const AROUND_RSKIP427: [(u64, bool); 2] =
    [(MAINNET_LOVELL700 - 1, false), (MAINNET_LOVELL700, true)];

/// What rskj's tests add to a request's satoshis: a satoshi less one wei.
fn under_a_satoshi() -> U256 {
    U256::from(WEI_PER_SATOSHI - 1)
}

/// `releaseBtc` of `value` wei asked for by `caller`, from the transaction
/// itself, or with `from_a_contract` from a contract's code; the value
/// already at the Bridge.
fn release_wei(
    bridge: &mut Bridge,
    caller: Address,
    from_a_contract: bool,
    value: U256,
) -> Result<PrecompileOutput, PrecompileError> {
    let bridge_address = crate::precompiles::BRIDGE_ADDR;
    let held = bridge.balance(bridge_address);
    bridge.set_balance(bridge_address, held + value);
    let tx = BridgeTxContext {
        rsk_sender: caller,
        ..bridge.tx.clone()
    };
    crate::bridge::peg::release_btc(
        &mut bridge.ctx,
        0,
        &bridge.constants,
        &bridge.cfg,
        &tx,
        caller,
        if from_a_contract { 2 } else { 1 },
        value,
    )
}

/// A request of `satoshis` and a satoshi less one wei, turned down for
/// `reason`, with the fee per kilobyte `fee_per_kb`: from the sender it is
/// refunded, from a contract it is not. Before RSKIP427 the refund and the
/// amount `release_request_rejected` logs are its whole satoshis; from it
/// the wei it sent.
fn turned_down_around_rskip427(
    satoshis: u64,
    fee_per_kb: &[u8],
    from_a_contract: bool,
    reason: u64,
) {
    let rejected = event_topic("release_request_rejected(address,uint256,int256)");
    let bridge_address = crate::precompiles::BRIDGE_ADDR;
    let value = wei(satoshis) + under_a_satoshi();
    let caller = if from_a_contract { CONTRACT } else { STRANGER };
    for (number, rskip427) in AROUND_RSKIP427 {
        let mut bridge = Bridge::mainnet_at(number);
        put_named(&mut bridge.ctx, "feePerKb", &rlp_encode_element(fee_per_kb));
        release_wei(&mut bridge, caller, from_a_contract, value).expect("releaseBtc answers");
        assert_eq!(queued(&mut bridge), 0, "{number}");
        let refunded = match (from_a_contract, rskip427) {
            (true, _) => U256::ZERO,
            (false, true) => value,
            (false, false) => wei(satoshis),
        };
        assert_eq!(bridge.balance(caller), refunded, "{number}");
        assert_eq!(bridge.balance(bridge_address), value - refunded, "{number}");
        let logs = logs(&mut bridge.ctx);
        assert_eq!(logs.len(), 1, "{number}");
        assert_eq!(topics(&logs[0]), [rejected, as_topic(caller)], "{number}");
        let amount = if rskip427 {
            value
        } else {
            U256::from(satoshis)
        };
        assert_eq!(word(&logs[0].data.data, 0), amount, "{number}");
        assert_eq!(word(&logs[0].data.data, 1), U256::from(reason), "{number}");
    }
}

/// rskj's `low_amount_release_request_rejected_before_lovell` and
/// `low_amount_release_request_rejected_after_lovell`: a satoshi below the
/// minimum, LOW_AMOUNT. The fee per kilobyte is 5,000 satoshis, which
/// leaves the minimum the larger bound.
#[test]
fn a_request_below_the_minimum_is_refunded_its_satoshis_before_rskip427_and_its_wei_from_it() {
    let minimum = BridgeConstants::mainnet().minimum_pegout_tx_value;
    turned_down_around_rskip427(minimum - 1, &[0x13, 0x88], false, 1);
}

/// rskj's `contract_caller_release_request_rejected_before_lovell` and
/// `contract_caller_release_request_rejected_after_lovell`: the minimum,
/// from a contract, CALLER_CONTRACT, nothing refunded.
#[test]
fn a_contracts_request_is_logged_in_satoshis_before_rskip427_and_in_wei_from_it() {
    let minimum = BridgeConstants::mainnet().minimum_pegout_tx_value;
    turned_down_around_rskip427(minimum, &[0x13, 0x88], true, 2);
}

/// rskj's `fee_above_value_release_request_rejected_before_lovell` and
/// `fee_above_value_release_request_rejected_after_lovell`: a fee per
/// kilobyte of a bitcoin, which puts the fee a peg-out must cover above a
/// bitcoin, and a request of a bitcoin, FEE_ABOVE_VALUE.
#[test]
fn a_request_below_its_fee_is_refunded_its_satoshis_before_rskip427_and_its_wei_from_it() {
    turned_down_around_rskip427(BTC, &[0x05, 0xf5, 0xe1, 0x00], false, 3);
}

/// Where the Bridge burns what it holds and no one is owed.
const BURN: Address = Address::repeat_byte(0xff);

/// `count` UTXOs of `satoshis` each, paid to the federation
/// [`Collections::active`] makes, each from a transaction of its own.
fn many_utxos(count: u16, satoshis: u64) -> Vec<crate::bridge::storage::BridgeUtxo> {
    let script = address_of(&active());
    (1..=count)
        .map(|i| {
            let mut tx_hash = [0u8; 32];
            tx_hash[..2].copy_from_slice(&i.to_be_bytes());
            crate::bridge::storage::BridgeUtxo {
                tx_hash,
                vout: 0,
                value_satoshis: satoshis,
                height: 1,
                script: script.clone(),
                coinbase: false,
            }
        })
        .collect()
}

/// Mainnet's Bridge at hop400, about to batch: the active federation holding
/// `utxos`, the requests of `amounts` queued, the request `i` (from 1) for
/// the user `i` with the hash `[i; 32]`, and the Bridge holding 100 BTC.
fn batching(utxos: &[crate::bridge::storage::BridgeUtxo], amounts: &[u64]) -> Collections {
    let entries: Vec<([u8; 20], u64, [u8; 32])> = amounts
        .iter()
        .zip(1u8..)
        .map(|(&amount, i)| ([i; 20], amount, [i; 32]))
        .collect();
    batching_queue(utxos, &entries)
}

/// [`batching`], with the queue's `(destination, amount, hash)` entries.
fn batching_queue(
    utxos: &[crate::bridge::storage::BridgeUtxo],
    entries: &[([u8; 20], u64, [u8; 32])],
) -> Collections {
    let mut collections = Collections::mainnet_at(MAINNET_HOP400);
    collections.active(1, 0);
    let number = collections.bridge.number();
    crate::bridge::storage::store_federation_utxos(
        &mut collections.bridge.ctx,
        &collections.bridge.cfg,
        number,
        utxos,
    );
    put_named(
        &mut collections.bridge.ctx,
        "releaseRequestQueueWithTxHash",
        &hashed_queue(entries),
    );
    collections
        .bridge
        .set_balance(crate::precompiles::BRIDGE_ADDR, wei(100 * BTC));
    collections
}

/// The hashes of the requests still queued.
fn still_queued(collections: &mut Collections) -> Vec<Option<[u8; 32]>> {
    crate::bridge::peg::load_release_request_queue(&mut collections.bridge.ctx, true)
        .iter()
        .map(|request| request.rsk_tx_hash)
        .collect()
}

/// rskj takes a batch's requests off the queue with `List.removeAll`, by
/// `ReleaseRequestQueue.Entry.equals`: every request equal to one the batch
/// pays goes, a request being equal to another of the same destination,
/// amount and creation hash. Here the queue, too large for one batch, repeats
/// its first request, whose copy goes with it though the halved batch pays
/// one, and holds a request of the same destination and amount with another
/// hash, which stays.
#[test]
fn from_rskip271_a_batch_takes_off_the_queue_every_request_equal_to_one_it_pays() {
    let first = ([1; 20], 3 * BTC / 2, [1; 32]);
    let second = ([2; 20], 3 * BTC / 2, [2; 32]);
    let other_hash = ([1; 20], 3 * BTC / 2, [3; 32]);
    let mut collections = batching_queue(
        &many_utxos(700, BTC / 100),
        &[first, second, first, other_hash],
    );
    collections.update();
    assert_eq!(collections.waiting().len(), 1);
    assert_eq!(still_queued(&mut collections), [Some([3; 32])]);
}

/// rskj's `processPegoutsInBatch_after_hop_divide_transaction_when_max_size_exceeded`:
/// four requests of 1.5 BTC need 600 inputs of 0.01 BTC, over bitcoinj's
/// 100,000 bytes, and the batch is halved: the first two, 300 inputs, are
/// paid, the other two stay queued, and the next call pays them. rskj settles
/// the first batch with all four's 6 BTC: `release_requested` logs it, and
/// the federation having spent 3 BTC, `adjustBalancesIfChangeOutputWasDust`
/// burns the other 3. `batch_pegout_created` lists the two paid.
#[test]
fn from_rskip271_a_batch_over_the_size_is_halved() {
    let mut collections = batching(&many_utxos(700, BTC / 100), &[3 * BTC / 2; 4]);
    let logs = collections.update();
    assert_eq!(
        still_queued(&mut collections),
        [Some([3; 32]), Some([4; 32])]
    );
    let waiting = collections.waiting();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].input.len(), 300);
    let paid: Vec<Vec<u8>> = waiting[0]
        .output
        .iter()
        .map(|output| output.script_pubkey.to_bytes())
        .collect();
    assert_eq!(paid, [user(1), user(2)]);

    let requested = event_topic("release_requested(bytes32,bytes32,uint256)");
    let batch = event_topic("batch_pegout_created(bytes32,bytes)");
    let log_of = |topic: [u8; 32]| {
        logs.iter()
            .find(|log| topics(log)[0] == topic)
            .expect("logged")
    };
    assert_eq!(word(&log_of(requested).data.data, 0), U256::from(6 * BTC));
    let hashes = &log_of(batch).data.data;
    assert_eq!(word(hashes, 1), U256::from(64));
    assert_eq!(&hashes[64..128], [[1u8; 32], [2u8; 32]].concat());
    assert_eq!(collections.bridge.balance(BURN), wei(3 * BTC));

    collections.update();
    assert!(still_queued(&mut collections).is_empty());
    assert_eq!(collections.waiting().len(), 2);
}

/// rskj's `processPegoutsInBatch_after_hop_when_max_size_exceeded_for_one_pegout`:
/// one request of 7 BTC needs 700 inputs of 0.01 BTC, over the size, and
/// cannot be halved: nothing is paid, and the request stays queued.
#[test]
fn from_rskip271_one_request_over_the_size_stays_queued() {
    let mut collections = batching(&many_utxos(700, BTC / 100), &[7 * BTC]);
    collections.update();
    assert_eq!(still_queued(&mut collections), [Some([1; 32])]);
    assert!(collections.waiting().is_empty());
}

/// rskj's `processPegoutsInBatch_after_hop_when_max_size_exceeded_for_two_pegout`:
/// two requests of 7 BTC are over the size, and so is the first alone:
/// nothing is paid, and both stay queued.
#[test]
fn from_rskip271_two_requests_each_over_the_size_stay_queued() {
    let mut collections = batching(&many_utxos(1400, BTC / 100), &[7 * BTC, 7 * BTC]);
    collections.update();
    assert_eq!(
        still_queued(&mut collections),
        [Some([1; 32]), Some([2; 32])]
    );
    assert!(collections.waiting().is_empty());
}

/// A request queued before RSKIP146 has no creation hash, and when it is
/// batched rskj's `logBatchPegoutCreated` cannot serialize it
/// (`serializeRskTxHashes`): the call throws. No network reaches it, as every
/// network queues requests with their hashes well before RSKIP271.
#[test]
fn from_rskip271_a_batched_request_with_no_hash_fails_the_call() {
    let mut collections = batching(&many_utxos(10, BTC), &[BTC]);
    put_named(
        &mut collections.bridge.ctx,
        "releaseRequestQueue",
        &legacy_queue(&[([9; 20], BTC)]),
    );
    let bridge = &mut collections.bridge;
    let tx = BridgeTxContext {
        rsk_sender: bridge.caller,
        ..bridge.tx.clone()
    };
    assert!(crate::bridge::peg::update_collections(
        &mut bridge.ctx,
        0,
        &bridge.constants,
        &bridge.cfg,
        &tx
    )
    .is_err());
}

/// From RSKIP271 rskj reads the next peg-out height in every
/// `updateCollections` (`getNextPegoutHeight`, 0 where the cell holds
/// nothing) and in `getNextPegoutCreationBlockNumber`, and writes back what
/// it read where it set nothing new: a request that cannot be paid leaves
/// the height unset, and the cell holds 0, `RLP.encodeBigInteger(0)`.
/// Before RSKIP271 the cell is neither read nor written.
#[test]
fn from_rskip271_the_next_pegout_height_read_is_written_back() {
    for (number, expected) in [
        (MAINNET_HOP400 - 1, None),
        (MAINNET_HOP400, Some(vec![0x80])),
    ] {
        let mut collections = Collections::mainnet_at(number);
        collections.active(1, 0);
        collections.requests(1);
        collections.update();
        assert_eq!(
            named(&mut collections.bridge.ctx, "nextPegoutHeight"),
            expected,
            "at {number}"
        );
    }

    let mut bridge = Bridge::mainnet_at(MAINNET_HOP400);
    bridge
        .execute(&crate::abi::selector("getNextPegoutCreationBlockNumber()"))
        .expect("the getter answers");
    assert_eq!(named(&mut bridge.ctx, "nextPegoutHeight"), Some(vec![0x80]));
}
