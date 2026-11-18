//! A federation member whose key is the point at infinity, the one byte
//! `00`, as a peg-out and the SVP spend transaction are signed.
//!
//! Not a port: no test of rskj's has such a member. rskj's `addSignature`
//! finds the member by that key (`Federation.hasBtcPublicKey`,
//! `getMemberByBtcPublicKey`), as any other by its own, and then verifies a
//! signature with it: `BtcECKey.verify` hands the point to Spongy Castle,
//! whose `ECPublicKeyParameters` refuses it with an
//! `IllegalArgumentException` ("point at infinity"), which nothing on the way
//! to the Bridge catches, so the call fails. Another member's first signature
//! of an input goes in; but once an input holds one, `getSigInsertionIndex`
//! looks it up with `findSigInRedeem`, which verifies it with the redeem
//! script's keys in order, the point at infinity first, and the call fails
//! alike. A member whose RSK key is that point is logged at its address.

use alloy_primitives::Address;

use super::support::*;
use crate::bridge::federation::rsk_address_from_public_key;

/// The point at infinity's encoding.
const INFINITY: &[u8] = &[0x00];

/// The point at infinity's RSK address.
const INFINITY_ADDRESS: Address =
    alloy_primitives::address!("dcc703c0e500b653ca82273b7bfad8045d85a470");

/// The hash of the RSK transaction that made the SVP spend transaction.
const SPEND_HASH: [u8; 32] = [0x5e; 32];

/// `serializeFederationMember` as an element of a federation's list: `btc`
/// its BTC and MST keys, `rsk` its RSK key, each as rskj encodes it.
fn member(btc: &[u8], rsk: &[u8]) -> Vec<u8> {
    rlp_encode_element(&rlp_encode_list(&[
        rlp_encode_element(btc),
        rlp_encode_element(rsk),
        rlp_encode_element(btc),
    ]))
}

/// `serializeFederation` of `members`, given sorted, made at `block`.
fn federation(members: &[Vec<u8>], block: u64) -> Vec<u8> {
    rlp_encode_list(&[
        serialize_long(0),
        serialize_long(block),
        rlp_encode_list(members),
    ])
}

/// `ScriptBuilder.createRedeemScript`: `size / 2 + 1` of `keys`, given
/// sorted and as rskj encodes them, each pushed by `ScriptBuilder.data`, the
/// point at infinity's one byte as a one-byte push (`01 00`).
fn multisig_of(keys: &[&[u8]]) -> Vec<u8> {
    let mut script = vec![0x50 + (keys.len() / 2 + 1) as u8];
    for key in keys {
        script.push(key.len() as u8);
        script.extend_from_slice(key);
    }
    script.push(0x50 + keys.len() as u8);
    script.push(0xae);
    script
}

/// `P2shErpRedeemScriptBuilder` on mainnet: the members' multisig, and after
/// 52,560 blocks (`signedLongToByteArrayLE`: `50 cd 00`) the emergency
/// keys'.
fn p2sh_erp_of(keys: &[&[u8]], constants: &BridgeConstants) -> Vec<u8> {
    assert_eq!(constants.erp_fed_activation_delay, 52_560);
    let emergency: Vec<[u8; 33]> = constants
        .erp_fed_pubkeys
        .iter()
        .map(|key| {
            alloy_primitives::hex::decode(key)
                .unwrap()
                .try_into()
                .unwrap()
        })
        .collect();
    let mut script = vec![0x64];
    script.extend(multisig_of(keys));
    script.extend_from_slice(&[0x67, 0x03, 0x50, 0xcd, 0x00, 0xb2, 0x75]);
    script.extend(standard_redeem(&emergency));
    script.push(0x68);
    script
}

/// The call data of `addSignature(key, signatures, hash)`.
fn add_signature(key: &[u8], signatures: &[Vec<u8>], hash: &[u8; 32]) -> Vec<u8> {
    use crate::abi::encode::{args, Arg};
    let sent: Vec<&[u8]> = signatures.iter().map(Vec::as_slice).collect();
    [
        crate::abi::selector("addSignature(bytes,bytes[],bytes)").to_vec(),
        args(&[Arg::Bytes(key), Arg::BytesArray(&sent), Arg::Bytes(hash)]),
    ]
    .concat()
}

/// The DER signature of `sighash` by the private key `private`.
fn signature(private: u8, sighash: &[u8; 32]) -> Vec<u8> {
    use k256::ecdsa::signature::hazmat::PrehashSigner;
    let mut scalar = [0u8; 32];
    scalar[31] = private;
    let key = k256::ecdsa::SigningKey::from_slice(&scalar).expect("a key");
    let signature: k256::ecdsa::Signature = key.sign_prehash(sighash).expect("signed");
    signature.to_der().as_bytes().to_vec()
}

/// The RSK address of the private key `private`'s public key.
fn address_of_private(private: u8) -> Address {
    rsk_address_from_public_key(&public_key(&[private])).expect("an address")
}

/// Whether `tx`'s first input holds `signature` as bitcoinj writes it, in
/// its witness or its scriptSig.
fn holds(tx: &bitcoin::Transaction, signature: &[u8]) -> bool {
    let written = [signature, &[0x01]].concat();
    let input = &tx.input[0];
    input.witness.iter().any(|push| push == written.as_slice())
        || input.script_sig.instructions().any(|instruction| {
            matches!(instruction, Ok(bitcoin::script::Instruction::PushBytes(bytes))
                if bytes.as_bytes() == written.as_slice())
        })
}

/// A spending of a bitcoin of `redeem`'s, `threshold` of its keys, with no
/// signature yet (`addSpendingFederationBaseScript`): from its witness where
/// `segwit`, else from its scriptSig.
fn spending(redeem: &[u8], threshold: usize, segwit: bool) -> bitcoin::Transaction {
    use crate::bridge::release_tx::{placeholder_scriptsig, segwit_base_input};
    let mut tx = btc_transaction(vec![input(1, Vec::new())], &[(BTC, user(9))]);
    if segwit {
        let (script_sig, witness) = segwit_base_input(redeem, threshold);
        tx.input[0].script_sig = script_sig;
        tx.input[0].witness = witness;
    } else {
        let script_sig = placeholder_scriptsig(redeem, threshold);
        tx.input[0].script_sig = bitcoin::ScriptBuf::from_bytes(script_sig);
    }
    tx
}

/// A Bridge at block `MAINNET_VETIVER900` of mainnet whose active federation
/// is of `members`, in format 4000 where `segwit` and 1000 otherwise, and
/// waiting for its signatures under [`RELEASE_HASH`] a peg-out of a bitcoin
/// of `redeem`'s, `threshold` of its keys, the value its input spends kept
/// where `segwit`, as the Bridge keeps it from RSKIP305. With what the input
/// is signed over.
fn waiting(
    members: &[Vec<u8>],
    redeem: &[u8],
    threshold: usize,
    segwit: bool,
) -> (Bridge, [u8; 32]) {
    use crate::bridge::release_tx::{legacy_sighash_all, segwit_sighash_all};
    let mut bridge = Bridge::mainnet_at(MAINNET_VETIVER900);
    put_named(&mut bridge.ctx, NEW_FEDERATION.0, &federation(members, 1));
    let format = if segwit {
        P2SH_P2WSH_ERP
    } else {
        STANDARD_MULTISIG
    };
    put_named(&mut bridge.ctx, NEW_FEDERATION.1, &serialize_long(format));
    let tx = spending(redeem, threshold, segwit);
    put_named(
        &mut bridge.ctx,
        "rskTxsWaitingFS",
        &rlp_encode_list(&[
            rlp_encode_element(&RELEASE_HASH),
            rlp_encode_element(&bitcoin::consensus::serialize(&tx)),
        ]),
    );
    if !segwit {
        return (bridge, legacy_sighash_all(&tx, 0, redeem));
    }
    use bitcoin::hashes::Hash;
    let mut hash = tx.compute_txid().to_raw_hash().to_byte_array();
    hash.reverse();
    let hash = crate::bridge::tx::to_hex(&hash);
    let key = crate::bridge::storage::compound_key("releasesOutpointsValues", "-", &hash);
    // One bitcoin as bitcoinj's `VarInt` writes it.
    let value = vec![0xfe, 0x00, 0xe1, 0xf5, 0x05];
    crate::bridge::storage::bridge_store_raw(&mut bridge.ctx, key, Some(value));
    (bridge, segwit_sighash_all(&tx, 0, redeem, BTC))
}

/// A federation of the point at infinity and the keys of the private keys
/// 1 and 2, a member each, two of them to sign: its members, and its redeem
/// script, a P2SH ERP one where `segwit` and a standard multisig otherwise.
fn with_the_point_at_infinity(segwit: bool) -> (Vec<Vec<u8>>, Vec<u8>) {
    let mut keys = [public_key(&[1]), public_key(&[2])];
    keys.sort();
    let members = vec![
        member(INFINITY, INFINITY),
        member(&keys[0], &keys[0]),
        member(&keys[1], &keys[1]),
    ];
    let ordered: [&[u8]; 3] = [INFINITY, &keys[0], &keys[1]];
    let redeem = if segwit {
        p2sh_erp_of(&ordered, &BridgeConstants::mainnet())
    } else {
        multisig_of(&ordered)
    };
    (members, redeem)
}

/// The peg-out as it waits.
fn waiting_pegout(bridge: &mut Bridge) -> bitcoin::Transaction {
    let entries = crate::bridge::peg::deserialize_rsk_txs_waiting_for_signatures(
        &named(&mut bridge.ctx, "rskTxsWaitingFS").expect("waiting"),
    );
    bitcoin::consensus::deserialize(&entries[&RELEASE_HASH]).expect("a transaction")
}

/// A signature given for the point at infinity: rskj returns at the count
/// of signatures, one per input, before it looks the key up; with one, the
/// member is found, its signature is verified with the point at infinity,
/// and the call fails. Nothing is signed.
#[test]
fn a_signature_for_the_point_at_infinity_fails_the_call() {
    for segwit in [false, true] {
        let (members, redeem) = with_the_point_at_infinity(segwit);
        let (mut bridge, sighash) = waiting(&members, &redeem, 2, segwit);
        let before = named(&mut bridge.ctx, "rskTxsWaitingFS");
        bridge.caller = address_of_private(1);
        let signed = signature(1, &sighash);
        let two = add_signature(INFINITY, &[signed.clone(), signed.clone()], &RELEASE_HASH);
        assert!(
            bridge.execute(&two).is_ok(),
            "two signatures, segwit: {segwit}"
        );
        let one = add_signature(INFINITY, &[signed], &RELEASE_HASH);
        assert!(
            bridge.execute(&one).is_err(),
            "one signature, segwit: {segwit}"
        );
        assert_eq!(
            named(&mut bridge.ctx, "rskTxsWaitingFS"),
            before,
            "segwit: {segwit}"
        );
    }
}

/// A member's first signature of an input goes in, its key found in the
/// redeem script by its bytes (`findKeyInRedeem`). Another member's then
/// fails the call: the signature held is looked up (`findSigInRedeem`) with
/// the point at infinity first. The peg-out waits as the first left it.
#[test]
fn once_an_input_holds_a_signature_another_members_fails_the_call() {
    for segwit in [false, true] {
        let (members, redeem) = with_the_point_at_infinity(segwit);
        let (mut bridge, sighash) = waiting(&members, &redeem, 2, segwit);
        bridge.caller = address_of_private(1);
        let first = signature(1, &sighash);
        let call = add_signature(
            &public_key(&[1]),
            std::slice::from_ref(&first),
            &RELEASE_HASH,
        );
        bridge.execute(&call).expect("the first signature goes in");
        assert!(
            holds(&waiting_pegout(&mut bridge), &first),
            "segwit: {segwit}"
        );
        let signed_once = named(&mut bridge.ctx, "rskTxsWaitingFS");
        bridge.caller = address_of_private(2);
        let second = signature(2, &sighash);
        let call = add_signature(&public_key(&[2]), &[second], &RELEASE_HASH);
        assert!(bridge.execute(&call).is_err(), "segwit: {segwit}");
        assert_eq!(
            named(&mut bridge.ctx, "rskTxsWaitingFS"),
            signed_once,
            "segwit: {segwit}"
        );
    }
}

/// From RSKIP415 rskj logs a signing member at its RSK key's address
/// (`BridgeEventLoggerImpl.logAddSignature`): a member whose RSK key is the
/// point at infinity signs in with its BTC key, and is logged at that
/// point's address, the other member sending the transaction.
#[test]
fn a_member_whose_rsk_key_is_the_point_at_infinity_is_logged_at_its_address() {
    let (one, two) = (public_key(&[1]), public_key(&[2]));
    let mut keys = [one, two];
    keys.sort();
    let members: Vec<Vec<u8>> = keys
        .iter()
        .map(|key| member(key, if *key == two { INFINITY } else { key }))
        .collect();
    let redeem = multisig_of(&[&keys[0], &keys[1]]);
    let (mut bridge, sighash) = waiting(&members, &redeem, 2, false);
    let _ = logs(&mut bridge.ctx);
    bridge.caller = address_of_private(1);
    let call = add_signature(&two, &[signature(2, &sighash)], &RELEASE_HASH);
    bridge.execute(&call).expect("signed in");
    let add = event_topic("add_signature(bytes32,address,bytes)");
    let logged: Vec<_> = logs(&mut bridge.ctx)
        .into_iter()
        .filter(|log| topics(log)[0] == add)
        .collect();
    assert_eq!(logged.len(), 1);
    assert_eq!(topics(&logged[0])[2][12..], INFINITY_ADDRESS.0);
}

/// The SVP spend transaction, signed by the proposed federation's members
/// (`addSvpSpendTxSignatures`): the member whose BTC key is the point at
/// infinity is found by it, so two signatures for one input return at their
/// count, and one fails the call at its verification; the member whose RSK
/// key is that point signs in, logged at its address. A third member, of
/// neither, sends the transactions.
#[test]
fn the_svp_spend_transaction_takes_the_point_at_infinity_as_a_proposed_members_key() {
    let (one, three) = (public_key(&[1]), public_key(&[3]));
    let members = [member(INFINITY, &three), member(&one, INFINITY)];
    let redeem = multisig_of(&[INFINITY, &one]);
    let mut bridge = Bridge::mainnet_at(MAINNET_VETIVER900);
    let creation = MAINNET_VETIVER900 - 1;
    put_named(
        &mut bridge.ctx,
        "proposedFederation",
        &federation(&members, creation),
    );
    put_named(
        &mut bridge.ctx,
        "proposedFederationFormatVersion",
        &serialize_long(P2SH_P2WSH_ERP),
    );
    let spend = spending(&redeem, 2, false);
    put_named(
        &mut bridge.ctx,
        "svpSpendTxWaitingForSignatures",
        &rlp_encode_list(&[
            rlp_encode_element(&SPEND_HASH),
            rlp_encode_element(&bitcoin::consensus::serialize(&spend)),
        ]),
    );
    let sighash = crate::bridge::release_tx::legacy_sighash_all(&spend, 0, &redeem);
    let _ = logs(&mut bridge.ctx);
    bridge.caller = address_of_private(3);

    let signed = signature(1, &sighash);
    let two_signatures = add_signature(INFINITY, &[signed.clone(), signed.clone()], &SPEND_HASH);
    assert!(
        bridge.execute(&two_signatures).is_ok(),
        "two signatures for one input"
    );
    let one_signature = add_signature(INFINITY, std::slice::from_ref(&signed), &SPEND_HASH);
    assert!(
        bridge.execute(&one_signature).is_err(),
        "verified with the point at infinity"
    );

    let call = add_signature(&one, std::slice::from_ref(&signed), &SPEND_HASH);
    bridge.execute(&call).expect("signed in");
    let entry = named(&mut bridge.ctx, "svpSpendTxWaitingForSignatures").expect("waiting");
    let items = crate::bridge::serialization::rlp_decode_list(&entry).expect("a list");
    let waiting: bitcoin::Transaction =
        bitcoin::consensus::deserialize(&items[1]).expect("a transaction");
    assert!(holds(&waiting, &signed));
    let add = event_topic("add_signature(bytes32,address,bytes)");
    let logged: Vec<_> = logs(&mut bridge.ctx)
        .into_iter()
        .filter(|log| topics(log)[0] == add)
        .collect();
    assert_eq!(logged.len(), 1);
    assert_eq!(topics(&logged[0])[2][12..], INFINITY_ADDRESS.0);
}
