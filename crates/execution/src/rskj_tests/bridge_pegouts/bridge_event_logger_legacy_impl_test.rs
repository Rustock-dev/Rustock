//! rskj's `BridgeEventLoggerLegacyImplTest`: before RSKIP146 the Bridge logs
//! its events in rskj's legacy format, one topic, the event's name as
//! `DataWord.fromString` makes it, and RLP data. From RSKIP146 rskj's legacy
//! logger throws for each of them, and the Bridge logs them in the Solidity
//! format instead. rskj's tests call the logger; the ports call mainnet's
//! Bridge a block before papyrus200, which brings RSKIP146, and at it, and
//! read what it logs.

use super::support::*;

/// A block before RSKIP146.
const BEFORE_RSKIP146: u64 = MAINNET_PAPYRUS200 - 1;
/// The block that brings it.
const FROM_RSKIP146: u64 = MAINNET_PAPYRUS200;

/// The logs among `logged` whose first topic is `topic`.
fn of(logged: &[revm::primitives::Log], topic: [u8; 32]) -> Vec<&revm::primitives::Log> {
    logged
        .iter()
        .filter(|log| topics(log)[0] == topic)
        .collect()
}

/// rskj's `testLogUpdateCollectionsBeforeRskip146` and
/// `testLogUpdateCollectionsAfterRskip146`: before RSKIP146
/// `updateCollections` logs `update_collections_topic`, from the Bridge, its
/// data the sender as an RLP element; from it, `update_collections(address)`.
#[test]
fn before_rskip146_update_collections_is_logged_in_the_legacy_format() {
    let mut bridge = Bridge::mainnet_at(BEFORE_RSKIP146);
    bridge.update_collections();
    let logged = logs(&mut bridge.ctx);
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0].address, crate::precompiles::BRIDGE_ADDR);
    assert_eq!(
        topics(&logged[0]),
        vec![legacy_topic("update_collections_topic")]
    );
    assert_eq!(
        logged[0].data.data.to_vec(),
        rlp_encode_element(STRANGER.as_slice())
    );

    let mut bridge = Bridge::mainnet_at(FROM_RSKIP146);
    bridge.update_collections();
    let logged = logs(&mut bridge.ctx);
    assert_eq!(logged.len(), 1);
    assert_eq!(
        topics(&logged[0])[0],
        event_topic("update_collections(address)")
    );
}

/// rskj's `testLogAddSignatureBeforeRskip146` and
/// `testLogAddSignatureAfterRskip146`: before RSKIP146 a federator's
/// signature is logged as `add_signature_topic`, its data the RLP list of the
/// transaction's hash as a hex string, the hash160 of the federator's key and
/// the hash of the RSK transaction that made the peg-out; from it, as
/// `add_signature(bytes32,address,bytes)`.
#[test]
fn before_rskip146_a_signature_is_logged_in_the_legacy_format() {
    let mut signing = Signing::mainnet_at(BEFORE_RSKIP146);
    let hash = signing.unsigned.compute_txid().to_string();
    signing.sign(0);
    let logged = logs(&mut signing.bridge.ctx);
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0].address, crate::precompiles::BRIDGE_ADDR);
    assert_eq!(
        topics(&logged[0]),
        vec![legacy_topic("add_signature_topic")]
    );
    assert_eq!(
        logged[0].data.data.to_vec(),
        rlp_encode_list(&[
            rlp_encode_element(hash.as_bytes()),
            rlp_encode_element(&hash160(&signing.key(0))),
            rlp_encode_element(&RELEASE_HASH),
        ])
    );

    let mut signing = Signing::mainnet_at(FROM_RSKIP146);
    signing.sign(0);
    let logged = logs(&mut signing.bridge.ctx);
    assert_eq!(logged.len(), 1);
    assert_eq!(
        topics(&logged[0])[0],
        event_topic("add_signature(bytes32,address,bytes)")
    );
}

/// rskj's `testLogReleaseBtcBeforeRskip146` and
/// `testLogReleaseBtcAfterRskip146`: before RSKIP146 the signature that
/// completes a peg-out logs `release_btc_topic`, its data the RLP list of the
/// signed transaction's hash as a hex string and the transaction; from it,
/// `release_btc(bytes32,bytes)`.
#[test]
fn before_rskip146_a_pegout_signed_is_logged_in_the_legacy_format() {
    let mut signing = Signing::mainnet_at(BEFORE_RSKIP146);
    signing.sign(0);
    logs(&mut signing.bridge.ctx);
    signing.sign(1);
    let logged = logs(&mut signing.bridge.ctx);
    assert_eq!(logged.len(), 2);
    assert_eq!(
        topics(&logged[0]),
        vec![legacy_topic("add_signature_topic")]
    );
    assert_eq!(topics(&logged[1]), vec![legacy_topic("release_btc_topic")]);
    let signed = signing.signed_by(&[0, 1]);
    assert_eq!(
        logged[1].data.data.to_vec(),
        rlp_encode_list(&[
            rlp_encode_element(signed.compute_txid().to_string().as_bytes()),
            rlp_encode_element(&bitcoin::consensus::serialize(&signed)),
        ])
    );

    let mut signing = Signing::mainnet_at(FROM_RSKIP146);
    signing.sign(0);
    signing.sign(1);
    let logged = logs(&mut signing.bridge.ctx);
    assert_eq!(
        of(&logged, event_topic("release_btc(bytes32,bytes)")).len(),
        1
    );
    assert!(of(&logged, legacy_topic("release_btc_topic")).is_empty());
}

/// The data rskj's legacy logger gives a federation in
/// `commit_federation_topic`: the hash160 of its address, and its keys.
fn legacy_federation(keys: &[[u8; 33]]) -> Vec<u8> {
    let mut sorted = keys.to_vec();
    sorted.sort();
    let keys: Vec<Vec<u8>> = sorted.iter().map(|key| rlp_encode_element(key)).collect();
    rlp_encode_list(&[
        rlp_encode_element(&hash160(&standard_redeem(&sorted))),
        rlp_encode_list(&keys),
    ])
}

/// rskj's `testLogCommitFederationBeforeRskip146` and
/// `testLogCommitFederationAfterRskip146`: before RSKIP146 a federation
/// committed is logged as `commit_federation_topic`, its data the RLP list of
/// the federation it follows (here mainnet's genesis federation), the new
/// one, and the new one's activation block as a decimal string, the commit's
/// block and mainnet's activation age before RSKIP383, 18,500 blocks; from
/// it, as `commit_federation(bytes,string,bytes,string,int256)`.
#[test]
fn before_rskip146_a_federation_committed_is_logged_in_the_legacy_format() {
    let members = keys(20, 3);
    let mut bridge = Bridge::mainnet_at(BEFORE_RSKIP146);
    assert_eq!(elect(&mut bridge, &members), 1);
    let logged = logs(&mut bridge.ctx);
    let committed = of(&logged, legacy_topic("commit_federation_topic"));
    assert_eq!(committed.len(), 1);
    assert_eq!(topics(committed[0]).len(), 1);
    assert_eq!(
        committed[0].data.data.to_vec(),
        rlp_encode_list(&[
            legacy_federation(&genesis_keys(&BridgeConstants::mainnet())),
            legacy_federation(&members),
            rlp_encode_element((BEFORE_RSKIP146 + 18_500).to_string().as_bytes()),
        ])
    );

    let mut bridge = Bridge::mainnet_at(FROM_RSKIP146);
    assert_eq!(elect(&mut bridge, &members), 1);
    let logged = logs(&mut bridge.ctx);
    let signature = "commit_federation(bytes,string,bytes,string,int256)";
    assert_eq!(of(&logged, event_topic(signature)).len(), 1);
    assert!(of(&logged, legacy_topic("commit_federation_topic")).is_empty());
}
