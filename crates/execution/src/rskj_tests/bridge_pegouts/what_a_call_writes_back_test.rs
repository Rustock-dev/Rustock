//! What a Bridge call that returns leaves in the cells it read.
//!
//! Not a port: rskj's tests mock the storage providers and check that `save`
//! is called, not what the cells hold after a call. rskj's Bridge saves after
//! every call that returns (`Bridge.execute` ends with `teardown`, which is
//! `BridgeSupport.save`), and its providers write back what the call read as
//! well as what it set: the federation stored as the new one, from RSKIP123
//! with all three keys of each member and its format version
//! (`FederationStorageProviderImpl.saveNewFederation`), each federation's
//! UTXOs (`saveNewFederationBtcUTXOs`, `saveOldFederationBtcUTXOs`), and the
//! sets the state getters read. Before RSKIP88 a call that throws a
//! `BridgeIllegalArgumentException` is answered with null and saves nothing.

use revm::context_interface::ContextTr;

use super::support::*;
use crate::abi::encode::{self, Arg};
use crate::bridge::federation::rsk_address_from_public_key;

/// Where these tests schedule wasabi100 on rskj's regtest, and every upgrade
/// after it.
const WASABI: u64 = 10;

/// A block of mainnet's before orchid's RSKIP88, at #729,000.
const BEFORE_RSKIP88: u64 = 700_000;

/// rskj's regtest with wasabi100 and the upgrades after it at [`WASABI`], at
/// `block`: RSKIP123 off below it and on from it. Upgrades are scheduled in
/// their order, as this node takes an rskj schedule only so.
fn regtest_with_wasabi_at(block: u64) -> Bridge {
    use RskNetworkUpgrade::*;
    let cfg = [
        Wasabi100,
        TwoToThree,
        Papyrus200,
        Iris300,
        Hop400,
        Fingerroot500,
        Arrowhead600,
        Lovell700,
        Reed800,
        Reed810,
        Vetiver900,
    ]
    .into_iter()
    .fold(RskHardforkConfig::rskj_regtest(), |cfg, upgrade| {
        cfg.with_hardfork(upgrade, Some(WASABI))
    });
    assert!(!cfg.has_rskip123(WASABI - 1) && cfg.has_rskip123(WASABI));
    Bridge::new(cfg, BridgeConstants::rskj_regtest(), block)
}

/// The federation's members' keys, in order.
fn members() -> Vec<[u8; 33]> {
    let mut members = keys(1, 3);
    members.sort();
    members
}

/// `serializeFederationOnlyBtcKeys` of a federation of `keys`, given sorted,
/// made at block 1: how one committed before RSKIP123 is kept.
fn only_btc_keys(keys: &[[u8; 33]]) -> Vec<u8> {
    let members: Vec<Vec<u8>> = keys.iter().map(|key| rlp_encode_element(key)).collect();
    rlp_encode_list(&[
        serialize_long(0),
        serialize_long(1),
        rlp_encode_list(&members),
    ])
}

/// `serializeFederation` of the same federation: each member's BTC, RSK and
/// MST keys, its BTC key all three for a member that had only that one.
fn all_three_keys(keys: &[[u8; 33]]) -> Vec<u8> {
    let members: Vec<Vec<u8>> = keys
        .iter()
        .map(|key| {
            rlp_encode_element(&rlp_encode_list(&[
                rlp_encode_element(key),
                rlp_encode_element(key),
                rlp_encode_element(key),
            ]))
        })
        .collect();
    rlp_encode_list(&[
        serialize_long(0),
        serialize_long(1),
        rlp_encode_list(&members),
    ])
}

/// That regtest at `block`, its new federation committed before RSKIP123,
/// called by the federation's first member.
fn with_a_federation_of_btc_keys(block: u64) -> Bridge {
    let mut bridge = regtest_with_wasabi_at(block);
    put_named(
        &mut bridge.ctx,
        NEW_FEDERATION.0,
        &only_btc_keys(&members()),
    );
    bridge.caller = rsk_address_from_public_key(&members()[0]).expect("an address");
    bridge
}

/// `receiveHeaders` of no headers, a call only the federation may make from
/// RSKIP200: rskj reads the federation to know the caller.
fn receive_no_headers() -> Vec<u8> {
    let mut data = selector("receiveHeaders(bytes[])");
    data.extend(encode::args(&[Arg::BytesArray(&[])]));
    data
}

/// A call of `signature`, a method of no arguments.
fn selector(signature: &str) -> Vec<u8> {
    crate::abi::selector(signature).to_vec()
}

/// The empty list, as rskj writes a set it read where its cell held nothing.
fn empty_list() -> Option<Vec<u8>> {
    Some(rlp_encode_list(&[]))
}

/// From RSKIP123 a federator's call, which reads the federation to know the
/// caller, saves it with all three keys of each member and the format
/// version of a standard multisig federation.
#[test]
fn from_rskip123_a_call_that_read_the_federation_saves_it_with_all_three_keys() {
    let mut bridge = with_a_federation_of_btc_keys(WASABI);
    bridge
        .execute(&receive_no_headers())
        .expect("a federator's call");
    assert_eq!(
        named(&mut bridge.ctx, NEW_FEDERATION.0),
        Some(all_three_keys(&members()))
    );
    assert_eq!(
        named(&mut bridge.ctx, NEW_FEDERATION.1),
        Some(serialize_long(1000))
    );
}

/// Before RSKIP123 the same call writes the federation back as it was kept.
#[test]
fn before_rskip123_the_call_leaves_the_federation_as_it_was() {
    let mut bridge = with_a_federation_of_btc_keys(WASABI - 1);
    bridge
        .execute(&receive_no_headers())
        .expect("a federator's call");
    assert_eq!(
        named(&mut bridge.ctx, NEW_FEDERATION.0),
        Some(only_btc_keys(&members()))
    );
    assert_eq!(named(&mut bridge.ctx, NEW_FEDERATION.1), None);
}

/// A call that reads no federation saves none, though a call before it that
/// failed read it.
#[test]
fn a_call_that_read_no_federation_leaves_it_as_it_was() {
    let mut bridge = with_a_federation_of_btc_keys(WASABI);
    bridge.caller = STRANGER;
    assert!(
        bridge.execute(&receive_no_headers()).is_err(),
        "a stranger's call fails"
    );
    bridge
        .execute(&selector("getQueuedPegoutsCount()"))
        .expect("a count");
    assert_eq!(
        named(&mut bridge.ctx, NEW_FEDERATION.0),
        Some(only_btc_keys(&members()))
    );
    assert_eq!(named(&mut bridge.ctx, NEW_FEDERATION.1), None);
}

/// Nor does a call that reads another federation, the proposed one.
#[test]
fn a_call_that_read_another_federation_leaves_the_new_one_as_it_was() {
    let mut bridge = with_a_federation_of_btc_keys(WASABI);
    bridge.tx.local_call = true;
    bridge
        .execute(&selector("getProposedFederationSize()"))
        .expect("a size");
    assert_eq!(
        named(&mut bridge.ctx, NEW_FEDERATION.0),
        Some(only_btc_keys(&members()))
    );
}

/// `registerBtcTransaction` of a transaction in the Bitcoin block at
/// `height`, with `tree` for its tree, on mainnet before RSKIP88. Whether it
/// returned.
fn register_before_rskip88(bridge: &mut Registration, height: u32, tree: &[u8]) -> bool {
    let raw = bitcoin::consensus::serialize(&tx_paying(&[(BTC, user(9))]));
    let args = encode::args(&[
        Arg::Bytes(&raw),
        Arg::Word(encode::int(i64::from(height))),
        Arg::Bytes(tree),
    ]);
    let returned = crate::bridge::peg::register_btc_transaction(
        &mut bridge.ctx,
        &args,
        0,
        &bridge.constants,
        &bridge.cfg,
        &BridgeTxContext::default(),
    )
    .is_ok();
    if returned {
        bridge.ctx.chain_mut().raw_storage.commit_call();
    } else {
        bridge.ctx.chain_mut().raw_storage.discard_call();
    }
    returned
}

/// Before RSKIP88 a transaction whose tree is not of a tree's size, confirmed
/// enough, throws a `BridgeIllegalArgumentException`, which `Bridge.execute`
/// answers with null: the call returns and saves nothing, so the processed
/// transactions' map it read is left as it was.
#[test]
fn before_rskip88_a_call_answered_with_null_leaves_the_processed_map_unwritten() {
    let mut bridge = Registration::mainnet(BEFORE_RSKIP88);
    bridge.store_chain(1_000, [0x22; 32]);
    assert!(register_before_rskip88(&mut bridge, 1_000, &[0x01]), "null");
    assert_eq!(named(&mut bridge.ctx, "btcTxHashesAP"), None);
}

/// One turned down quietly, from a block above the chain's head, which no
/// block confirms, saves the map.
#[test]
fn before_rskip88_a_call_turned_down_quietly_writes_the_processed_map_back() {
    let mut bridge = Registration::mainnet(BEFORE_RSKIP88);
    assert!(
        register_before_rskip88(&mut bridge, 2_000_000_000, &[0x01]),
        "turned down"
    );
    assert_eq!(named(&mut bridge.ctx, "btcTxHashesAP"), empty_list());
}

/// The fee estimate builds a peg-out of the active federation's UTXOs, and
/// writes them back as it returns: on a new regtest, where nothing wrote
/// them, the empty list.
#[test]
fn the_fee_estimate_writes_back_the_utxos_it_read() {
    let mut bridge = Bridge::new(
        RskHardforkConfig::rskj_regtest(),
        BridgeConstants::rskj_regtest(),
        1,
    );
    bridge
        .execute(&selector("getEstimatedFeesForNextPegOutEvent()"))
        .expect("an estimate");
    assert_eq!(
        named(&mut bridge.ctx, "newFederationBtcUTXOs"),
        empty_list()
    );
}

/// A peg-in turned down for its amount: `registerPegIn` first computes what
/// the transaction sent, which reads the active federation's UTXOs and the
/// retiring one's, and the call writes both back.
#[test]
fn a_peg_in_turned_down_writes_back_the_utxos_it_read() {
    let mut bridge = RSKIP379_INDEXED.bridge(true);
    let tx = tx_paying(&[(minimum_pegin() - 1, address_of(&active()))]);
    let outcome = bridge.register(&tx, RSKIP379_INDEXED.height);
    assert!(outcome.returned);
    assert!(outcome.active_utxos.is_empty());
    assert_eq!(
        named(&mut bridge.ctx, "newFederationBtcUTXOs"),
        empty_list()
    );
    assert_eq!(
        named(&mut bridge.ctx, "oldFederationBtcUTXOs"),
        empty_list()
    );
}

/// A transaction the legacy rules take for no peg-in, paying the federation
/// too little before RSKIP379, is left alone: nothing reads the UTXOs.
#[test]
fn a_transaction_that_is_no_peg_in_leaves_the_utxos_unwritten() {
    let mut bridge = BEFORE_RSKIP379.bridge(true);
    let tx = tx_paying(&[(minimum_pegin() - 1, address_of(&active()))]);
    let outcome = bridge.register(&tx, BEFORE_RSKIP379.height);
    assert!(outcome.returned);
    assert_eq!(named(&mut bridge.ctx, "newFederationBtcUTXOs"), None);
    assert_eq!(named(&mut bridge.ctx, "oldFederationBtcUTXOs"), None);
}

/// A peg-out paying neither federation: `registerNewUtxos` reads both
/// federations' UTXOs whatever the transaction pays them, and the call
/// writes both back.
#[test]
fn a_peg_out_paying_neither_federation_writes_back_the_utxos_it_read() {
    let redeem = standard_redeem(&active());
    let tx = btc_transaction(vec![input(1, multisig_spend(&redeem))], &[(BTC, user(9))]);
    let mut bridge = RSKIP379_INDEXED.bridge(true);
    index_pegout(&mut bridge.ctx, &index_sighash(&tx, &redeem));
    let outcome = bridge.register(&tx, RSKIP379_INDEXED.height);
    assert!(outcome.returned);
    assert_eq!(
        named(&mut bridge.ctx, "newFederationBtcUTXOs"),
        empty_list()
    );
    assert_eq!(
        named(&mut bridge.ctx, "oldFederationBtcUTXOs"),
        empty_list()
    );
}

/// Before RSKIP88 a transaction may ask for the state the federators sign
/// from, which reads the peg-outs waiting for signatures; the call writes
/// them back.
#[test]
fn before_rskip88_a_call_for_the_signing_state_writes_back_what_it_read() {
    let mut bridge = Bridge::mainnet_at(BEFORE_RSKIP88);
    bridge
        .execute(&selector("getStateForBtcReleaseClient()"))
        .expect("the state");
    assert_eq!(named(&mut bridge.ctx, "rskTxsWaitingFS"), empty_list());
}

/// And for the whole state, which reads the new federation's UTXOs, the
/// peg-outs waiting for signatures, the release request queue and the
/// peg-outs waiting for confirmations, each written back; the next peg-out
/// height is read only from RSKIP271.
#[test]
fn before_rskip88_a_call_for_the_whole_state_writes_back_what_it_read() {
    let mut bridge = Bridge::mainnet_at(BEFORE_RSKIP88);
    bridge
        .execute(&selector("getStateForDebugging()"))
        .expect("the state");
    for cell in [
        "newFederationBtcUTXOs",
        "rskTxsWaitingFS",
        "releaseRequestQueue",
        "releaseTransactionSet",
    ] {
        assert_eq!(named(&mut bridge.ctx, cell), empty_list(), "{cell}");
    }
    assert_eq!(named(&mut bridge.ctx, "nextPegoutHeight"), None);
}
