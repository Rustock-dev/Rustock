//! rskj's `BridgeSupportAddSignatureTest`: what `addSignature` signs in, and
//! when it logs `add_signature`, which RSKIP326 changes. Before it the event
//! is logged for each call of a federator's, before the signature is
//! checked; from it, only when a signature is added. rskj's tests mock
//! RSKIP146 on and RSKIP326 either way; the ports run mainnet's Bridge at
//! hop400, with RSKIP146 and before RSKIP326, and at fingerroot500, which
//! brings it. rskj's tests hand `BridgeSupport` what the Bridge would turn
//! down before it (no signatures, bytes that are not DER); the ports call
//! the Bridge, as a member's transaction does. Its nested `AddSignature` and
//! `MigrationTxSigning` tests, of P2SH ERP and P2SH-P2WSH ERP federations
//! with every upgrade on, run at vetiver900. Its parameterized `addSignature`,
//! of the address the event names a federator by, which RSKIP415 changes,
//! runs on each side of arrowhead600.

use super::support::*;

const BEFORE_RSKIP326: u64 = MAINNET_HOP400;
const FROM_RSKIP326: u64 = MAINNET_FINGERROOT500;

/// The DER signature of r and s both 1: it reads, and signs nothing.
const WRONG: &[u8] = &[0x30, 0x06, 0x02, 0x01, 0x01, 0x02, 0x01, 0x01];

/// How many `add_signature` events `calls` calls of the first member's log
/// at block `number`, each with its own signature, or with [`WRONG`].
fn add_signature_events(number: u64, valid: bool, calls: usize) -> usize {
    let mut signing = Signing::mainnet_at(number);
    let signature = if valid {
        signing.signature(0)
    } else {
        WRONG.to_vec()
    };
    for _ in 0..calls {
        signing
            .add_signature(0, &signature)
            .expect("addSignature runs");
    }
    let topic = event_topic("add_signature(bytes32,address,bytes)");
    logs(&mut signing.bridge.ctx)
        .iter()
        .filter(|log| topics(log)[0] == topic)
        .count()
}

/// rskj's `addSignature_calledTwiceWithSameFederatorPreRSKIP326_emitEventTwice`
/// and `addSignature_calledTwiceWithSameFederatorPostRSKIP326_emitEventOnlyOnce`.
#[test]
fn a_federator_signing_twice_is_logged_twice_before_rskip326_and_once_from_it() {
    assert_eq!(add_signature_events(BEFORE_RSKIP326, true, 2), 2);
    assert_eq!(add_signature_events(FROM_RSKIP326, true, 2), 1);
}

/// rskj's `addSignatureCreateEventLog_preRSKIP326` and
/// `addSignature_afterRSKIP326_eventEmitted`.
#[test]
fn a_federators_signature_is_logged() {
    assert_eq!(add_signature_events(BEFORE_RSKIP326, true, 1), 1);
    assert_eq!(add_signature_events(FROM_RSKIP326, true, 1), 1);
}

/// The federator `addSignature`'s event names at block `number` when the
/// member of the private key 350 signs, in a federation of 150 and 250, each
/// stored with the RSK key of the private key after its own and the MST key
/// of the one after that. The signer is stored so too where `multikey`, and
/// with its BTC key for all three where not. One signature of the two needed
/// is kept, and logged alone, by the Bridge, with the peg-out's creation hash
/// and the federator's BTC key.
fn logged_federator(number: u64, multikey: bool) -> alloy_primitives::Address {
    use crate::abi::encode::{args, Arg};
    use crate::bridge::federation::StoredMember;
    let member = |btc: u16, rsk: u16, mst: u16| {
        let mut private = [0u8; 32];
        private[30..].copy_from_slice(&btc.to_be_bytes());
        let stored = StoredMember {
            btc: public_key(&btc.to_be_bytes()),
            rsk: public_key(&rsk.to_be_bytes()),
            mst: public_key(&mst.to_be_bytes()),
        };
        (stored, private)
    };
    let signer = if multikey {
        member(350, 351, 352)
    } else {
        member(350, 350, 350)
    };
    let members = vec![member(150, 151, 152), member(250, 251, 252), signer];
    let mut signing = Signing::of_members(number, members, 1);
    let index = (0..3)
        .find(|&i| signing.key(i) == signer.0.btc)
        .expect("the signer is a member");
    let signature = signing.signature(index);
    signing
        .add_signature(index, &signature)
        .expect("addSignature runs");
    let logs = logs(&mut signing.bridge.ctx);
    assert_eq!(logs.len(), 1, "one signature of two, logged alone");
    assert_eq!(logs[0].address, crate::precompiles::BRIDGE_ADDR);
    let topics = topics(&logs[0]);
    assert_eq!(topics.len(), 3);
    assert_eq!(
        topics[0],
        event_topic("add_signature(bytes32,address,bytes)")
    );
    assert_eq!(topics[1], RELEASE_HASH);
    assert_eq!(
        logs[0].data.data.to_vec(),
        args(&[Arg::Bytes(&signer.0.btc)]),
        "the federator's BTC key"
    );
    alloy_primitives::Address::from_slice(&topics[2][12..])
}

/// rskj's `addSignature`, `addSignatureArgProvider`'s four cases: from
/// RSKIP415 the `add_signature` event names a federator by the address of its
/// RSK key, and before it by its BTC key's, which is the same where a
/// member's keys are one. The two addresses are rskj's, of the private keys
/// 350 and 351.
#[test]
fn from_rskip415_a_federator_is_logged_at_its_rsk_keys_address() {
    let of_btc_key = alloy_primitives::address!("dbc29273d4de3d5645e308c7e629d28d4499b3d3");
    let of_rsk_key = alloy_primitives::address!("74891a05ad4d7ec87c1cffe9bd00bb4e1382b586");
    let before = MAINNET_ARROWHEAD600 - 1;
    assert_eq!(logged_federator(before, false), of_btc_key);
    assert_eq!(logged_federator(before, true), of_btc_key);
    assert_eq!(logged_federator(MAINNET_ARROWHEAD600, false), of_btc_key);
    assert_eq!(logged_federator(MAINNET_ARROWHEAD600, true), of_rsk_key);
}

/// rskj's `save()` writes back the set of peg-outs waiting for signatures
/// whenever a call read it (`BridgeStorageProvider
/// .savePegoutsWaitingForSignatures`), and `addSignature` reads it first
/// (`addReleaseSignatures`): where it was never written, a call for a
/// peg-out it does not hold leaves the empty list there. Where it is
/// written, a call that signs nothing, here by the key of no member, leaves
/// it as it was.
#[test]
fn add_signature_writes_back_the_set_it_read() {
    let mut signing = Signing::mainnet_at(FROM_RSKIP326);
    let kept = named(&mut signing.bridge.ctx, "rskTxsWaitingFS");
    assert!(kept.is_some());
    let signature = signing.signature(0);
    signing
        .add_signatures_as(0, &public_key(&[9]), std::slice::from_ref(&signature))
        .expect("addSignature runs");
    assert_eq!(named(&mut signing.bridge.ctx, "rskTxsWaitingFS"), kept);

    let mut signing = Signing::mainnet_at(FROM_RSKIP326);
    crate::bridge::storage::bridge_store_raw(
        &mut signing.bridge.ctx,
        crate::bridge::storage::bridge_storage_key("rskTxsWaitingFS"),
        None,
    );
    assert_eq!(named(&mut signing.bridge.ctx, "rskTxsWaitingFS"), None);
    signing
        .add_signature(0, &signature)
        .expect("addSignature runs");
    assert_eq!(
        named(&mut signing.bridge.ctx, "rskTxsWaitingFS"),
        Some(vec![0xc0]),
        "the empty list"
    );
    assert!(logs(&mut signing.bridge.ctx).is_empty());
}

/// rskj's `addSignature_invalidSignatureBeforeRSKIP326_eventStillEmitted` and
/// `addSignature_invalidSignatureAfterRSKIP326_noEventEmitted`. rskj's tests
/// hand `BridgeSupport` bytes that aren't DER; the Bridge turns those down
/// before `BridgeSupport` sees them, so here a DER signature that doesn't
/// verify takes their place.
#[test]
fn a_signature_that_does_not_verify_is_logged_before_rskip326_only() {
    assert_eq!(add_signature_events(BEFORE_RSKIP326, false, 1), 1);
    assert_eq!(add_signature_events(FROM_RSKIP326, false, 1), 0);
}

/// The peg-out as it waits for signatures.
fn waiting(signing: &mut Signing) -> bitcoin::Transaction {
    let kept = crate::bridge::peg::deserialize_rsk_txs_waiting_for_signatures(
        &named(&mut signing.bridge.ctx, "rskTxsWaitingFS").unwrap_or_default(),
    );
    bitcoin::consensus::deserialize(&kept[&RELEASE_HASH]).expect("a transaction")
}

/// rskj's `addSignatureNonCanonicalSignature`: a member's signature with its
/// s in the upper half of the curve's order verifies, but rskj's
/// `getTransactionSignatures` wants it canonical (`isCanonical`) and signs
/// nothing.
#[test]
fn a_signature_whose_s_is_in_the_upper_half_of_the_order_signs_nothing() {
    let mut signing = Signing::mainnet_at(FROM_RSKIP326);
    let low = k256::ecdsa::Signature::from_der(&signing.signature(0)).expect("DER");
    let (r, s) = low.split_scalars();
    let high =
        k256::ecdsa::Signature::from_scalars(r.to_bytes(), (-s).to_bytes()).expect("a signature");
    signing
        .add_signature(0, high.to_der().as_bytes())
        .expect("addSignature runs");
    assert_eq!(waiting(&mut signing), signing.unsigned);
}

/// A member's signature with its r's length in DER's long form, which
/// bitcoinj's `decodeFromDER` reads (Bouncy Castle's `ASN1InputStream`), or
/// with a byte after it, which bitcoinj reads past: each is signed in as
/// bitcoinj writes the signature, `TransactionSignature.encodeToBitcoin`,
/// not as it was sent.
#[test]
fn a_signature_bitcoinj_reads_is_signed_in_as_bitcoinj_writes_it() {
    let der = Signing::mainnet_at(FROM_RSKIP326).signature(0);
    // `30 L 02 lr r 02 ls s`, with r's length written `81 lr`.
    let long_form = [&[0x30, der[1] + 1, 0x02, 0x81][..], &der[3..]].concat();
    let trailing = [der.clone(), vec![0x00]].concat();
    for sent in [long_form, trailing] {
        let mut signing = Signing::mainnet_at(FROM_RSKIP326);
        signing.add_signature(0, &sent).expect("addSignature runs");
        assert_eq!(waiting(&mut signing), signing.signed_by(&[0]));
    }
}

/// A signature with an integer whose first byte only repeats the sign of
/// the next: Bouncy Castle refuses the integer (`ASN1Integer.isMalformed`),
/// bitcoinj's `decodeFromDER` throws, and so, from RSKIP88, does the
/// Bridge's `addSignature`. A first byte the next one's sign needs is read,
/// and so is an integer with no bytes, as 0; neither signs anything.
#[test]
fn a_signature_with_an_integer_padded_past_its_sign_fails_the_call() {
    let mut signing = Signing::mainnet_at(FROM_RSKIP326);
    let padded: [&[u8]; 2] = [
        &[0x30, 0x07, 0x02, 0x02, 0x00, 0x7f, 0x02, 0x01, 0x01],
        &[0x30, 0x07, 0x02, 0x01, 0x01, 0x02, 0x02, 0xff, 0x80],
    ];
    for sent in padded {
        assert!(signing.add_signature(0, sent).is_err(), "{sent:02x?}");
    }
    let read: [&[u8]; 3] = [
        &[0x30, 0x07, 0x02, 0x02, 0x00, 0x80, 0x02, 0x01, 0x01],
        &[0x30, 0x07, 0x02, 0x01, 0x01, 0x02, 0x02, 0xff, 0x7f],
        &[0x30, 0x04, 0x02, 0x00, 0x02, 0x00],
    ];
    for sent in read {
        assert!(signing.add_signature(0, sent).is_ok(), "{sent:02x?}");
    }
    assert_eq!(waiting(&mut signing), signing.unsigned);
}

/// The peg-outs waiting for signatures, by the hash of the transaction that
/// made each.
fn waiting_for_signatures(signing: &mut Signing) -> std::collections::BTreeMap<[u8; 32], Vec<u8>> {
    crate::bridge::peg::deserialize_rsk_txs_waiting_for_signatures(
        &named(&mut signing.bridge.ctx, "rskTxsWaitingFS").unwrap_or_default(),
    )
    .into_iter()
    .collect()
}

/// The private key of no member.
const STRANGER_KEY: [u8; 32] = [0x77; 32];

/// rskj's `addSignatureToMissingTransaction`: a member's signature for a
/// peg-out that is not waiting for signatures changes nothing, and logs
/// nothing.
#[test]
fn a_signature_for_a_pegout_not_waiting_changes_nothing() {
    let mut signing = Signing::mainnet_at(BEFORE_RSKIP326);
    let signature = signing.signature(0);
    put_named(
        &mut signing.bridge.ctx,
        "rskTxsWaitingFS",
        &rlp_encode_list(&[]),
    );
    signing
        .add_signature(0, &signature)
        .expect("addSignature runs");
    assert!(waiting_for_signatures(&mut signing).is_empty());
    assert!(logs(&mut signing.bridge.ctx).is_empty());
}

/// rskj's `addSignatureFromInvalidFederator`: a member's call with the key
/// of no member, and that key's signature, signs nothing and logs nothing.
#[test]
fn the_key_of_no_member_signs_nothing() {
    let mut signing = Signing::mainnet_at(BEFORE_RSKIP326);
    let signature = signing.signature_by(&STRANGER_KEY, 0);
    signing
        .add_signatures_as(0, &public_key(&STRANGER_KEY), &[signature])
        .expect("addSignature runs");
    assert_eq!(waiting(&mut signing), signing.unsigned);
    assert!(logs(&mut signing.bridge.ctx).is_empty());
}

/// rskj's `addSignatureWithInvalidSignature`: a member's call with the
/// signature of a key that is no member's signs nothing.
#[test]
fn a_signature_by_another_key_signs_nothing() {
    let mut signing = Signing::mainnet_at(BEFORE_RSKIP326);
    let signature = signing.signature_by(&STRANGER_KEY, 0);
    signing
        .add_signature(0, &signature)
        .expect("addSignature runs");
    assert_eq!(waiting(&mut signing), signing.unsigned);
}

/// rskj's `addSignatureWithLessSignaturesThanExpected`: no signatures for the
/// peg-out's one input. rskj's `BridgeSupport` returns; the Bridge turns an
/// empty list down before it, and from RSKIP88 fails the call.
#[test]
fn no_signatures_fail_the_call() {
    let mut signing = Signing::mainnet_at(BEFORE_RSKIP326);
    assert!(signing.add_signatures(0, &[]).is_err());
    assert_eq!(waiting(&mut signing), signing.unsigned);
}

/// rskj's `addSignatureWithMoreSignaturesThanExpected`: two signatures for
/// the peg-out's one input sign nothing.
#[test]
fn more_signatures_than_inputs_sign_nothing() {
    let mut signing = Signing::mainnet_at(BEFORE_RSKIP326);
    let signature = signing.signature(0);
    signing
        .add_signatures(0, &[signature.clone(), signature])
        .expect("addSignature runs");
    assert_eq!(waiting(&mut signing), signing.unsigned);
}

/// rskj's `addSignatureOneSignature`: one member's signature is kept, and
/// the peg-out waits for the rest.
#[test]
fn one_members_signature_is_kept() {
    let mut signing = Signing::mainnet_at(BEFORE_RSKIP326);
    signing.sign(0);
    assert_eq!(waiting(&mut signing), signing.signed_by(&[0]));
}

/// rskj's `addSignatureTwice`: a member that signed signs nothing more with
/// another signature of its own.
#[test]
fn a_member_that_signed_signs_nothing_more() {
    let mut signing = Signing::mainnet_at(BEFORE_RSKIP326);
    signing.sign(0);
    let another = signing.another_signature_of(0, 0);
    assert_ne!(another, signing.signature(0));
    signing
        .add_signature(0, &another)
        .expect("addSignature runs");
    assert_eq!(waiting(&mut signing), signing.signed_by(&[0]));
}

/// The raw transaction a `release_btc` event logs.
fn released(log: &revm::primitives::Log) -> Vec<u8> {
    let data = &log.data.data;
    let length = word(data, 1).to::<usize>();
    data[64..64 + length].to_vec()
}

/// rskj's `addSignatureTwoSignatures`: the second of the federation's two
/// needed signatures signs the peg-out. It no longer waits, and after each
/// member's `add_signature` the Bridge logs `release_btc` with it signed.
#[test]
fn two_members_sign_the_pegout_and_it_is_released() {
    let mut signing = Signing::mainnet_at(BEFORE_RSKIP326);
    signing.sign(0);
    signing.sign(1);
    assert!(waiting_for_signatures(&mut signing).is_empty());
    let logs = logs(&mut signing.bridge.ctx);
    let firsts: Vec<[u8; 32]> = logs.iter().map(|log| topics(log)[0]).collect();
    let add = event_topic("add_signature(bytes32,address,bytes)");
    let release = event_topic("release_btc(bytes32,bytes)");
    assert_eq!(firsts, [add, add, release]);
    assert_eq!(topics(&logs[2])[1], RELEASE_HASH);
    assert_eq!(
        released(&logs[2]),
        bitcoin::consensus::serialize(&signing.signed_by(&[0, 1]))
    );
}

/// rskj's `addSignatureMultipleInputsPartiallyValid`: a peg-out of three
/// inputs. A member's three signatures, the third by another key, sign none
/// of them; with a third that bitcoinj cannot read, the Bridge fails the
/// call, as rskj's test, which hands it to `BridgeSupport`, does not see;
/// with the member's own three, the three inputs are signed; and a second
/// member's three sign the peg-out, which is released.
#[test]
fn a_pegout_of_three_inputs_is_signed_only_by_signatures_for_all_three() {
    let mut signing = Signing::spending(BEFORE_RSKIP326, 3);
    let mut first: Vec<Vec<u8>> = (0..3).map(|index| signing.signature_of(0, index)).collect();
    let own = first[2].clone();
    first[2] = signing.signature_by(&STRANGER_KEY, 2);
    signing
        .add_signatures(0, &first)
        .expect("addSignature runs");
    assert_eq!(waiting(&mut signing), signing.unsigned);
    first[2] = (0..own.len() as u8).collect();
    assert!(signing.add_signatures(0, &first).is_err());
    assert_eq!(waiting(&mut signing), signing.unsigned);
    first[2] = own;
    signing
        .add_signatures(0, &first)
        .expect("addSignature runs");
    assert_eq!(waiting(&mut signing), signing.signed_by(&[0]));
    let second: Vec<Vec<u8>> = (0..3).map(|index| signing.signature_of(1, index)).collect();
    signing
        .add_signatures(1, &second)
        .expect("addSignature runs");
    assert!(waiting_for_signatures(&mut signing).is_empty());
    let logs = logs(&mut signing.bridge.ctx);
    let add = event_topic("add_signature(bytes32,address,bytes)");
    let release = event_topic("release_btc(bytes32,bytes)");
    let firsts: Vec<[u8; 32]> = logs.iter().map(|log| topics(log)[0]).collect();
    assert_eq!(firsts, [add, add, add, release]);
    assert_eq!(
        released(&logs[3]),
        bitcoin::consensus::serialize(&signing.signed_by(&[0, 1]))
    );
}

/// A key's compressed public key.
fn compressed(key: &k256::ecdsa::SigningKey) -> [u8; 33] {
    key.verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .try_into()
        .expect("33 bytes")
}

/// A federation as rskj's nested tests make one: the keys of the private keys
/// `keccak256(seed)` (`getBtcEcKeysFromSeeds(seeds, true)`), in the order of
/// their public keys, kept in `format`, P2SH ERP or P2SH-P2WSH ERP.
struct Seeded {
    signers: Vec<k256::ecdsa::SigningKey>,
    keys: Vec<[u8; 33]>,
    format: u64,
    redeem: Vec<u8>,
}

impl Seeded {
    fn of(seeds: &[String], format: u64, bridge: &Bridge) -> Self {
        use sha3::Digest;
        let mut signers: Vec<k256::ecdsa::SigningKey> = seeds
            .iter()
            .map(|seed| {
                let private = sha3::Keccak256::digest(seed.as_bytes());
                k256::ecdsa::SigningKey::from_slice(&private).expect("a key")
            })
            .collect();
        signers.sort_by_key(compressed);
        let keys: Vec<[u8; 33]> = signers.iter().map(compressed).collect();
        let redeem = crate::bridge::peg::federation_redeem_for_format_pub(
            &keys,
            format,
            &bridge.constants,
            &bridge.cfg,
            bridge.number(),
        );
        Self {
            signers,
            keys,
            format,
            redeem,
        }
    }

    fn segwit(&self) -> bool {
        self.format == P2SH_P2WSH_ERP
    }

    fn threshold(&self) -> usize {
        self.keys.len() / 2 + 1
    }

    /// The output script of its address.
    fn output(&self) -> Vec<u8> {
        p2sh(&crate::bridge::peg::federation_output_hash160(
            &self.redeem,
            self.format,
        ))
    }

    /// rskj `BitcoinUtils.addSpendingFederationBaseScript`: input `index` of
    /// `tx` as the Bridge leaves it, spending this federation's output with
    /// no signature yet.
    fn leave_unsigned(&self, tx: &mut bitcoin::Transaction, index: usize) {
        use crate::bridge::release_tx::{placeholder_scriptsig, segwit_base_input};
        if self.segwit() {
            let (script_sig, witness) = segwit_base_input(&self.redeem, self.threshold());
            tx.input[index].script_sig = script_sig;
            tx.input[index].witness = witness;
        } else {
            let script_sig = placeholder_scriptsig(&self.redeem, self.threshold());
            tx.input[index].script_sig = bitcoin::ScriptBuf::from_bytes(script_sig);
        }
    }

    /// What input `index` of `tx`, spending `value` satoshis of this
    /// federation's, is signed over: its legacy sighash, or its BIP-143 one.
    fn sighash(&self, tx: &bitcoin::Transaction, index: usize, value: u64) -> [u8; 32] {
        use crate::bridge::release_tx::{legacy_sighash_all, segwit_sighash_all};
        if self.segwit() {
            segwit_sighash_all(tx, index, &self.redeem, value)
        } else {
            legacy_sighash_all(tx, index, &self.redeem)
        }
    }
}

/// `signer`'s DER signature of `sighash`.
fn der_signature(signer: &k256::ecdsa::SigningKey, sighash: &[u8; 32]) -> Vec<u8> {
    use k256::ecdsa::signature::hazmat::PrehashSigner;
    let signature: k256::ecdsa::Signature = signer.sign_prehash(sighash).expect("signed");
    signature.to_der().as_bytes().to_vec()
}

/// Whether input `index` of `tx` holds `signer`'s signature of `sighash`, as
/// bitcoinj writes it, in its witness or its scriptSig.
fn holds_signature(
    tx: &bitcoin::Transaction,
    index: usize,
    signer: &k256::ecdsa::SigningKey,
    sighash: &[u8; 32],
) -> bool {
    let written = [der_signature(signer, sighash), vec![0x01]].concat();
    let input = &tx.input[index];
    input.witness.iter().any(|push| push == written.as_slice())
        || input.script_sig.instructions().any(|instruction| {
            matches!(instruction, Ok(bitcoin::script::Instruction::PushBytes(bytes))
                if bytes.as_bytes() == written.as_slice())
        })
}

/// A transaction waiting for signatures under [`RELEASE_HASH`], spending
/// `spent`'s output of a bitcoin, as rskj's nested tests keep one, and what
/// each of its inputs is signed over.
struct ToSign {
    bridge: Bridge,
    spent: Seeded,
    sighashes: Vec<[u8; 32]>,
}

impl ToSign {
    /// The transaction `paying` the bitcoin of `spent`'s that it spends, of
    /// `version`, waiting in `bridge`; with `keep_value`, the bitcoin its
    /// input spends kept under its hash, as from RSKIP305 the Bridge keeps it
    /// when it builds the transaction.
    fn waiting_in(
        mut bridge: Bridge,
        spent: Seeded,
        version: i32,
        paying: &[(u64, Vec<u8>)],
        keep_value: bool,
    ) -> Self {
        let funding = btc_transaction(vec![], &[(BTC, spent.output())]);
        let mut tx = btc_transaction(
            vec![bitcoin::TxIn {
                previous_output: bitcoin::OutPoint::new(funding.compute_txid(), 0),
                ..bitcoin::TxIn::default()
            }],
            paying,
        );
        tx.version = bitcoin::transaction::Version(version);
        spent.leave_unsigned(&mut tx, 0);
        use crate::bridge::serialization::{rlp_encode_element, rlp_encode_list};
        put_named(
            &mut bridge.ctx,
            "rskTxsWaitingFS",
            &rlp_encode_list(&[
                rlp_encode_element(&RELEASE_HASH),
                rlp_encode_element(&bitcoin::consensus::serialize(&tx)),
            ]),
        );
        if keep_value {
            use bitcoin::hashes::Hash;
            let mut hash = tx.compute_txid().to_raw_hash().to_byte_array();
            hash.reverse();
            let hash = crate::bridge::tx::to_hex(&hash);
            let key = crate::bridge::storage::compound_key("releasesOutpointsValues", "-", &hash);
            // One bitcoin as bitcoinj's `VarInt` writes it.
            let value = vec![0xfe, 0x00, 0xe1, 0xf5, 0x05];
            crate::bridge::storage::bridge_store_raw(&mut bridge.ctx, key, Some(value));
        }
        let sighashes = vec![spent.sighash(&tx, 0, BTC)];
        Self {
            bridge,
            spent,
            sighashes,
        }
    }

    /// rskj's nested `AddSignature` set-up, at block `number` of mainnet: a
    /// federation of the signers `signer1` to `signer<count>` active in
    /// `format`, and waiting for its signatures a version 2 peg-out of its
    /// bitcoin, the minimum to the receiver and the rest back to it.
    fn pegout(number: u64, count: usize, format: u64, keep_value: bool) -> Self {
        let mut bridge = Bridge::mainnet_at(number);
        let seeds: Vec<String> = (1..=count).map(|i| format!("signer{i}")).collect();
        let federation = Seeded::of(&seeds, format, &bridge);
        store_federation(&mut bridge.ctx, NEW_FEDERATION, &federation.keys, 1, format);
        let minimum = bridge.constants.minimum_pegout_tx_value;
        let paying = [(minimum, user(9)), (BTC - minimum, federation.output())];
        Self::waiting_in(bridge, federation, 2, &paying, keep_value)
    }

    /// At block `number` of mainnet, the SVP under way: a proposed
    /// P2SH-P2WSH ERP federation of twenty, made the block before, and its
    /// SVP spend transaction, a version 2 one of its bitcoin, waiting for its
    /// signatures (`svpSpendTxWaitingForSignatures`).
    fn svp_spend(number: u64, keep_value: bool) -> Self {
        let mut bridge = Bridge::mainnet_at(number);
        let seeds: Vec<String> = (1..=20).map(|i| format!("signer{i}")).collect();
        let proposed = Seeded::of(&seeds, P2SH_P2WSH_ERP, &bridge);
        let slot = ("proposedFederation", "proposedFederationFormatVersion");
        store_federation(
            &mut bridge.ctx,
            slot,
            &proposed.keys,
            number - 1,
            P2SH_P2WSH_ERP,
        );
        let mut to_sign = Self::waiting_in(bridge, proposed, 2, &[(BTC, user(9))], keep_value);
        let entry = named(&mut to_sign.bridge.ctx, "rskTxsWaitingFS").expect("waiting");
        put_named(
            &mut to_sign.bridge.ctx,
            "svpSpendTxWaitingForSignatures",
            &entry,
        );
        to_sign
    }

    /// `addSignature` by the member `signer` of `federation`, its signatures of
    /// [`ToSign::sighashes`], from its transaction.
    fn add_signature_by(
        &mut self,
        signer: &k256::ecdsa::SigningKey,
    ) -> Result<revm::precompile::PrecompileOutput, revm::precompile::PrecompileError> {
        use crate::abi::encode::{args, Arg};
        let key = compressed(signer);
        self.bridge.caller =
            crate::bridge::federation::rsk_address_from_public_key(&key).expect("an address");
        let signatures: Vec<Vec<u8>> = self
            .sighashes
            .iter()
            .map(|sighash| der_signature(signer, sighash))
            .collect();
        let sent: Vec<&[u8]> = signatures.iter().map(Vec::as_slice).collect();
        let data = [
            crate::abi::selector("addSignature(bytes,bytes[],bytes)").to_vec(),
            args(&[
                Arg::Bytes(&key),
                Arg::BytesArray(&sent),
                Arg::Bytes(&RELEASE_HASH),
            ]),
        ]
        .concat();
        self.bridge.execute(&data)
    }

    /// The first `count` signers of the spent federation sign, each in a call
    /// that returns.
    fn signed_by_the_first(&mut self, count: usize) {
        for at in 0..count {
            let signer = self.spent.signers[at].clone();
            self.add_signature_by(&signer).expect("addSignature runs");
        }
    }

    /// The transactions still waiting, by their creation hash.
    fn still_waiting(&mut self) -> std::collections::BTreeMap<[u8; 32], Vec<u8>> {
        crate::bridge::peg::deserialize_rsk_txs_waiting_for_signatures(
            &named(&mut self.bridge.ctx, "rskTxsWaitingFS").unwrap_or_default(),
        )
        .into_iter()
        .collect()
    }

    /// The transaction as it waits.
    fn waiting(&mut self) -> bitcoin::Transaction {
        let raw = self.still_waiting().remove(&RELEASE_HASH).expect("waiting");
        bitcoin::consensus::deserialize(&raw).expect("a transaction")
    }

    /// rskj's `assertFederatorSigning` for the first `count` signers and
    /// `assertLogReleaseBtc`: an `add_signature` event for each, by its
    /// address, then `release_btc`, of the transaction holding each one's
    /// signature, which no longer waits. The transaction released.
    fn released_signed_by_the_first(&mut self, count: usize) -> bitcoin::Transaction {
        assert!(self.still_waiting().is_empty(), "no longer waiting");
        let logs = logs(&mut self.bridge.ctx);
        let add = event_topic("add_signature(bytes32,address,bytes)");
        let release = event_topic("release_btc(bytes32,bytes)");
        let firsts: Vec<[u8; 32]> = logs.iter().map(|log| topics(log)[0]).collect();
        assert_eq!(firsts, [vec![add; count], vec![release]].concat());
        let released: bitcoin::Transaction =
            bitcoin::consensus::deserialize(&released(&logs[count])).expect("a transaction");
        for (signer, log) in self.spent.signers[..count].iter().zip(&logs) {
            let address =
                crate::bridge::federation::rsk_address_from_public_key(&compressed(signer))
                    .expect("an address");
            assert_eq!(topics(log)[2][12..], address.0);
            assert!(holds_signature(&released, 0, signer, &self.sighashes[0]));
        }
        released
    }
}

/// rskj's `AddSignature.addSignature_correctFederator_whenFedIsSegwit_shouldSign`:
/// a majority of a P2SH-P2WSH ERP federation of twenty signs its peg-out, each
/// signature over the input's BIP-143 sighash with the value kept for it, in
/// the input's witness; the peg-out is released.
#[test]
fn a_majority_of_a_segwit_federation_signs_its_pegout_and_it_is_released() {
    let mut to_sign = ToSign::pegout(MAINNET_VETIVER900, 20, P2SH_P2WSH_ERP, true);
    let majority = to_sign.spent.threshold();
    to_sign.signed_by_the_first(majority);
    let released = to_sign.released_signed_by_the_first(majority);
    assert!(
        !released.input[0].script_sig.is_empty(),
        "the witness program pushed"
    );
}

/// rskj's `AddSignature.addSignature_correctFederator_whenFedIsSegwit_txFullySigned_shouldNotAddExtraSignature`
/// and `..._whenFedIsLegacy_txFullySigned_shouldNotAddExtraSignature`: once a
/// peg-out of a P2SH-P2WSH ERP federation of twenty, or of a P2SH ERP one of
/// nine, is released, another member's signatures change nothing.
#[test]
fn a_released_pegout_takes_no_more_signatures() {
    for (count, format, keep_value) in [(20, P2SH_P2WSH_ERP, true), (9, P2SH_ERP, false)] {
        let mut to_sign = ToSign::pegout(MAINNET_VETIVER900, count, format, keep_value);
        let majority = to_sign.spent.threshold();
        to_sign.signed_by_the_first(majority);
        let released = to_sign.released_signed_by_the_first(majority);
        let extra = to_sign.spent.signers[majority + 1].clone();
        to_sign.add_signature_by(&extra).expect("addSignature runs");
        assert!(to_sign.still_waiting().is_empty(), "{format}");
        assert!(logs(&mut to_sign.bridge.ctx).is_empty(), "{format}");
        assert!(
            !holds_signature(&released, 0, &extra, &to_sign.sighashes[0]),
            "{format}"
        );
    }
}

/// rskj reads the value a segwit input spends, which its sighash covers,
/// with `getReleaseOutpointsValues`: before RSKIP305 it throws, and from it
/// it finds nothing where nothing is kept under the transaction's hash, and
/// the call throws. A member's signature of a peg-out with a witness fails
/// the call before RSKIP305 whatever is kept, and from it where nothing is,
/// and is signed in where the value is kept.
#[test]
fn a_segwit_input_is_signed_only_from_rskip305_and_with_its_value_kept() {
    let first_signs = |to_sign: &mut ToSign| {
        let first = to_sign.spent.signers[0].clone();
        to_sign.add_signature_by(&first).map(|_| ())
    };
    let mut before = ToSign::pegout(MAINNET_REED800 - 1, 20, P2SH_P2WSH_ERP, true);
    assert!(first_signs(&mut before).is_err());

    let mut nothing_kept = ToSign::pegout(MAINNET_REED800, 20, P2SH_P2WSH_ERP, false);
    assert!(first_signs(&mut nothing_kept).is_err());

    let mut kept = ToSign::pegout(MAINNET_REED800, 20, P2SH_P2WSH_ERP, true);
    first_signs(&mut kept).expect("addSignature runs");
    let signer = kept.spent.signers[0].clone();
    let sighash = kept.sighashes[0];
    assert!(holds_signature(&kept.waiting(), 0, &signer, &sighash));
}

/// rskj signs the SVP spend transaction as a peg-out (`addSvpSpendTxSignatures`
/// runs `processSigning`): a proposed member's signature of its segwit input
/// fails the call before RSKIP305, and from it where nothing is kept under
/// its hash, and is signed in where the value is kept.
#[test]
fn the_svp_spend_transaction_is_signed_only_from_rskip305_and_with_its_value_kept() {
    for (number, keep_value, signed) in [
        (MAINNET_REED800 - 1, true, false),
        (MAINNET_REED800, false, false),
        (MAINNET_REED800, true, true),
    ] {
        let mut to_sign = ToSign::svp_spend(number, keep_value);
        let first = to_sign.spent.signers[0].clone();
        let answered = to_sign.add_signature_by(&first);
        assert_eq!(answered.is_ok(), signed, "{number} {keep_value}");
        if signed {
            use crate::bridge::serialization::rlp_decode_list;
            let entry = named(&mut to_sign.bridge.ctx, "svpSpendTxWaitingForSignatures");
            let items = rlp_decode_list(&entry.expect("waiting")).expect("a list");
            let spend: bitcoin::Transaction =
                bitcoin::consensus::deserialize(&items[1]).expect("a transaction");
            assert!(holds_signature(&spend, 0, &first, &to_sign.sighashes[0]));
        }
    }
}

/// rskj's `addSvpSpendTxSignatures` looks the signer up among the proposed
/// federation's members and throws where it is none of them
/// (`getMemberByBtcPublicKey(..).orElseThrow`): a call to sign the SVP spend
/// transaction with another key fails, though a member sends it, and the
/// transaction waits as it was.
#[test]
fn a_key_of_no_proposed_member_fails_a_call_to_sign_the_svp_spend_transaction() {
    use crate::abi::encode::{args, Arg};
    let mut to_sign = ToSign::svp_spend(MAINNET_REED800, true);
    let waiting = named(&mut to_sign.bridge.ctx, "svpSpendTxWaitingForSignatures");
    let member = compressed(&to_sign.spent.signers[0]);
    to_sign.bridge.caller =
        crate::bridge::federation::rsk_address_from_public_key(&member).expect("an address");
    let stranger = k256::ecdsa::SigningKey::from_slice(&[7; 32]).expect("a key");
    let signature = der_signature(&stranger, &to_sign.sighashes[0]);
    let data = [
        crate::abi::selector("addSignature(bytes,bytes[],bytes)").to_vec(),
        args(&[
            Arg::Bytes(&compressed(&stranger)),
            Arg::BytesArray(&[signature.as_slice()]),
            Arg::Bytes(&RELEASE_HASH),
        ]),
    ]
    .concat();
    assert!(to_sign.bridge.execute(&data).is_err(), "the call fails");
    assert_eq!(
        named(&mut to_sign.bridge.ctx, "svpSpendTxWaitingForSignatures"),
        waiting
    );
    assert!(logs(&mut to_sign.bridge.ctx).is_empty());
}

/// A key in the two other forms Bouncy Castle reads: uncompressed, and
/// hybrid, whose first byte is 06 or 07 as its y is even or odd.
fn other_forms(key: &[u8; 33]) -> [Vec<u8>; 2] {
    let uncompressed = bitcoin::secp256k1::PublicKey::from_slice(key)
        .expect("a key")
        .serialize_uncompressed();
    let mut hybrid = uncompressed;
    hybrid[0] = 0x06 | (uncompressed[64] & 1);
    [uncompressed.to_vec(), hybrid.to_vec()]
}

/// rskj finds the federation a peg-out is signed for by
/// `Federation.hasBtcPublicKey`, which compares the key's bytes as the call
/// gave them with each member's compressed key: a member that gives its key
/// uncompressed or hybrid is no member's, and the call returns without
/// signing, and without the event rskj logs before signing until RSKIP326.
#[test]
fn a_peg_out_takes_no_signature_under_a_key_given_uncompressed_or_hybrid() {
    for number in [BEFORE_RSKIP326, FROM_RSKIP326] {
        for form in 0..2 {
            let mut signing = Signing::mainnet_at(number);
            let given = other_forms(&signing.key(0))[form].clone();
            let waiting = named(&mut signing.bridge.ctx, "rskTxsWaitingFS");
            let signature = signing.signature(0);
            signing
                .add_signatures_as(0, &given, &[signature])
                .expect("addSignature returns");
            let what = format!("block {number}, key {:02x}", given[0]);
            assert_eq!(
                named(&mut signing.bridge.ctx, "rskTxsWaitingFS"),
                waiting,
                "{what}"
            );
            assert!(logs(&mut signing.bridge.ctx).is_empty(), "{what}");
        }
    }
}

/// To sign the SVP spend transaction rskj finds the proposed member by
/// `getMemberByBtcPublicKey`, which reads the key again and compares the
/// points: a member that gives its key uncompressed or hybrid is found, signs
/// in, and is logged with its key as the federation keeps it, compressed
/// (`BridgeEventLoggerImpl.logAddSignature`).
#[test]
fn the_svp_spend_transaction_takes_a_signature_under_a_key_given_uncompressed_or_hybrid() {
    use crate::abi::encode::{args, Arg};
    for form in 0..2 {
        let mut to_sign = ToSign::svp_spend(MAINNET_REED800, true);
        let signer = to_sign.spent.signers[0].clone();
        let key = compressed(&signer);
        let given = other_forms(&key)[form].clone();
        to_sign.bridge.caller =
            crate::bridge::federation::rsk_address_from_public_key(&key).expect("an address");
        let signature = der_signature(&signer, &to_sign.sighashes[0]);
        let data = [
            crate::abi::selector("addSignature(bytes,bytes[],bytes)").to_vec(),
            args(&[
                Arg::Bytes(&given),
                Arg::BytesArray(&[signature.as_slice()]),
                Arg::Bytes(&RELEASE_HASH),
            ]),
        ]
        .concat();
        to_sign.bridge.execute(&data).expect("addSignature runs");
        let logs = crate::rskj_tests::bridge_pegouts::support::logs(&mut to_sign.bridge.ctx);
        assert_eq!(logs.len(), 1, "signed in, and logged: key {:02x}", given[0]);
        assert_eq!(logs[0].data.data.to_vec(), args(&[Arg::Bytes(&key)]));
    }
}

/// rskj's nested `MigrationTxSigning` set-up, at vetiver900: a retiring
/// federation in `retiring_format` and an active one in `active_format`, of
/// nine members where P2SH ERP and twenty where P2SH-P2WSH ERP, sharing
/// `sharedFederator`, the retiring one with `retiringFederator` the active
/// one hasn't; and waiting for signatures the migration of the retiring
/// one's bitcoin to the active one, the bitcoin kept under its hash where
/// the retiring one is P2SH-P2WSH. The shared member's key and the other's.
fn migrating(retiring_format: u64, active_format: u64) -> (ToSign, [u8; 33], [u8; 33]) {
    let number = MAINNET_VETIVER900;
    let mut bridge = Bridge::mainnet_at(number);
    let seeds = |second: &str, format: u64| -> Vec<String> {
        let count = if format == P2SH_P2WSH_ERP { 20 } else { 9 };
        ["sharedFederator".to_string(), second.to_string()]
            .into_iter()
            .chain((3..=count).map(|i| format!("signer{i}")))
            .collect()
    };
    let retiring = Seeded::of(
        &seeds("retiringFederator", retiring_format),
        retiring_format,
        &bridge,
    );
    let active = Seeded::of(
        &seeds("activeFederator", active_format),
        active_format,
        &bridge,
    );
    store_federation(
        &mut bridge.ctx,
        OLD_FEDERATION,
        &retiring.keys,
        1,
        retiring_format,
    );
    let creation = number - activation_age(number) - 1;
    store_federation(
        &mut bridge.ctx,
        NEW_FEDERATION,
        &active.keys,
        creation,
        active_format,
    );
    let key_of = |seed: &str| {
        use sha3::Digest;
        let private = sha3::Keccak256::digest(seed.as_bytes());
        compressed(&k256::ecdsa::SigningKey::from_slice(&private).expect("a key"))
    };
    let (shared, only_retiring) = (key_of("sharedFederator"), key_of("retiringFederator"));
    assert!(retiring.keys.contains(&shared) && active.keys.contains(&shared));
    assert!(retiring.keys.contains(&only_retiring) && !active.keys.contains(&only_retiring));
    let paying = [(BTC, active.output())];
    let keep_value = retiring_format == P2SH_P2WSH_ERP;
    let to_sign = ToSign::waiting_in(bridge, retiring, 1, &paying, keep_value);
    (to_sign, shared, only_retiring)
}

/// rskj's `MigrationTxSigning.addSignature_migrationTx_*_withOneSharedFederatorAndOneOnlyPartOfRetiringFed`,
/// for the retiring and active federations P2SH ERP and P2SH ERP, P2SH ERP
/// and P2SH-P2WSH ERP, and P2SH-P2WSH ERP and P2SH-P2WSH ERP: a member of
/// both federations signs the migration in, then a member of the retiring
/// one alone, each signature logged.
#[test]
fn members_of_the_retiring_federation_sign_its_migration() {
    for (retiring, active) in [
        (P2SH_ERP, P2SH_ERP),
        (P2SH_ERP, P2SH_P2WSH_ERP),
        (P2SH_P2WSH_ERP, P2SH_P2WSH_ERP),
    ] {
        let (mut to_sign, shared, only_retiring) = migrating(retiring, active);
        for key in [shared, only_retiring] {
            let signer = to_sign
                .spent
                .signers
                .iter()
                .find(|signer| compressed(signer) == key)
                .expect("a retiring member")
                .clone();
            to_sign
                .add_signature_by(&signer)
                .expect("addSignature runs");
            let sighash = to_sign.sighashes[0];
            assert!(
                holds_signature(&to_sign.waiting(), 0, &signer, &sighash),
                "{retiring} {active}"
            );
            let add = event_topic("add_signature(bytes32,address,bytes)");
            let logs = logs(&mut to_sign.bridge.ctx);
            let firsts: Vec<[u8; 32]> = logs.iter().map(|log| topics(log)[0]).collect();
            assert_eq!(firsts, [add], "{retiring} {active}");
        }
    }
}
