//! rskj's `BridgeSupportTest` tests of who a peg-in is from, as rskj reads
//! its first input (`when_registerBtcTransaction_*`), of the instructions it
//! carries (`registerBtcTransaction_*_rskip_170_*`), and of the refunds
//! `registerPegIn` makes (`registerPegIn_*`).
//!
//! rskj's tests mock the sender a peg-in is read to be from; here it is the
//! input the peg-in spends: a P2PKH spend, a P2SH-P2WPKH one, a P2SH multisig
//! one, a P2SH-P2WSH multisig one, or one rskj reads no sender from. Before
//! RSKIP143 rskj takes a P2PKH sender only; from it, a P2SH-P2WPKH one too,
//! and refunds the others (`BridgeUtils.txIsProcessableInLegacyVersion`,
//! `BridgeSupport.isTxLockableForLegacyVersion`). rskj's "before fork" mocks
//! RSKIP143 off and its "after fork" RSKIP143 on alone; here they run on
//! Wasabi100 and on Papyrus200, which brings RSKIP143 with RSKIP134 and
//! RSKIP146.
//!
//! rskj's `registerPegIn_*` call `registerPegIn` on a mocked storage, which
//! keeps no processed mark, so they find none; here the peg-ins are
//! registered, and the mark rskj's `processPegInVersionLegacy` sets
//! (`BridgeSupport.java:682`) is held.

use alloy_primitives::{Address as RskAddress, U256};
use bitcoin::{OutPoint, ScriptBuf, Transaction, Witness};

use crate::bridge::constants::BridgeConstants;
use crate::bridge::storage;
use crate::hardfork::{RskNetworkUpgrade, RSK_REGTEST_CHAIN_ID};
use crate::precompiles::BRIDGE_ADDR;

use super::super::support::*;

use RskNetworkUpgrade::{Iris300, Papyrus200, Wasabi100};

/// The Bitcoin height a peg-in is registered at.
const HEIGHT: u32 = 100;

/// A locking cap no peg-in here reaches.
const NO_CAP: u64 = 1_000 * COIN;

/// What a peg-in sends.
const AMOUNT: u64 = 5 * COIN;

/// The P2PKH key the peg-ins are sent from.
fn sender() -> [u8; 33] {
    key_from_seed("sender")
}

/// The multisig redeem script the multisig peg-ins are sent from.
fn multisig_redeem() -> Vec<u8> {
    standard_redeem(&[
        key_from_seed("m1"),
        key_from_seed("m2"),
        key_from_seed("m3"),
    ])
}

/// The P2SH hash of the P2SH-P2WPKH output of `key`: rskj's address of the
/// sender (`P2shP2wpkhBtcLockSender`).
fn p2sh_p2wpkh_hash(key: &[u8; 33]) -> [u8; 20] {
    let mut program = vec![0x00, 20];
    program.extend_from_slice(&hash160(key));
    hash160(&program)
}

/// Instructions rskj cannot read: `RSKT`, version 1, and too few bytes for an
/// RSK destination.
fn unreadable_instructions() -> ScriptBuf {
    op_return(&[b"RSKT".as_slice(), &[1], &[0xd7; 5]].concat())
}

/// [`AMOUNT`] from `input` to rskj regtest's federation, with the output
/// `instructions` beside it, if any.
fn pegin(input: (ScriptBuf, Witness), instructions: Option<ScriptBuf>) -> Transaction {
    let mut outputs = vec![(
        AMOUNT,
        genesis_federation_output(&BridgeConstants::rskj_regtest()),
    )];
    outputs.extend(instructions.map(|script| (0, script)));
    tx(vec![input], &outputs)
}

/// `pegin` registered at RSK block 10 of rskj regtest's ladder up to `last`,
/// with a locking cap of `cap`, `whitelisted` on the whitelist once for
/// [`AMOUNT`] if any: what it answered, and the Bridge after it.
fn registered(
    pegin: &Transaction,
    last: RskNetworkUpgrade,
    cap: u64,
    whitelisted: Option<[u8; 20]>,
) -> (Registered, Ctx) {
    let config = BridgeConstants {
        initial_locking_cap: cap,
        ..BridgeConstants::rskj_regtest()
    };
    let cfg = up_to(last, RSK_REGTEST_CHAIN_ID);
    let mut ctx = bridge_holding(pegin, HEIGHT, 10, &config, &cfg);
    set_fee_per_kb(&mut ctx, MILLICOIN);
    if let Some(hash) = whitelisted {
        whitelist_once(&mut ctx, hash, AMOUNT);
    }
    let registered = register(&mut ctx, pegin, i64::from(HEIGHT), &config, &cfg);
    (registered, ctx)
}

/// Nothing credited, nothing refunded, and not processed.
fn assert_left_alone(registered: &Registered, ctx: &mut Ctx) {
    assert!(!registered.processed, "not processed");
    assert_eq!(balance(ctx, BRIDGE_ADDR), wei(MAX_RBTC), "nothing credited");
    assert!(waiting_pegouts(ctx).is_empty(), "nothing refunded");
}

/// `to` credited [`AMOUNT`], the federation holding the peg-in's output, and
/// processed.
fn assert_credited(
    registered: &Registered,
    ctx: &mut Ctx,
    to: RskAddress,
    last: RskNetworkUpgrade,
) {
    assert!(registered.processed, "processed");
    assert_eq!(balance(ctx, to), wei(AMOUNT), "credited");
    assert_eq!(
        balance(ctx, BRIDGE_ADDR),
        wei(MAX_RBTC - AMOUNT),
        "paid by the Bridge"
    );
    assert!(waiting_pegouts(ctx).is_empty(), "nothing refunded");
    let utxos = storage::load_federation_utxos(ctx, &up_to(last, RSK_REGTEST_CHAIN_ID), 10);
    let values: Vec<u64> = utxos.iter().map(|utxo| utxo.value_satoshis).collect();
    assert_eq!(values, [AMOUNT], "the federation holds it");
}

/// One refund to `to`, of the peg-in's output to the federation less a fee
/// within a millicoin, nothing credited, and processed.
fn assert_refunded(registered: &Registered, ctx: &mut Ctx, pegin: &Transaction, to: &ScriptBuf) {
    assert!(registered.processed, "processed");
    assert_eq!(balance(ctx, BRIDGE_ADDR), wei(MAX_RBTC), "nothing credited");
    let refunds = waiting_pegouts(ctx);
    assert_eq!(refunds.len(), 1, "a refund");
    let refund = &refunds[0];
    assert_eq!(refund.output.len(), 1);
    assert_eq!(&refund.output[0].script_pubkey, to, "where the refund goes");
    assert!(
        AMOUNT - refund.output[0].value.to_sat() <= MILLICOIN,
        "its fee"
    );
    let spent: Vec<OutPoint> = refund
        .input
        .iter()
        .map(|input| input.previous_output)
        .collect();
    assert_eq!(
        spent,
        [OutPoint {
            txid: pegin.compute_txid(),
            vout: 0
        }]
    );
}

/// `when_registerBtcTransaction_sender_not_recognized_before_rskip170_no_lock_and_no_refund`,
/// at Papyrus200: RSKIP143 on, RSKIP170 not yet.
#[test]
fn before_rskip170_a_peg_in_without_a_sender_is_left_alone() {
    let pegin = pegin(no_sender(), None);
    let (registered, mut ctx) = registered(&pegin, Papyrus200, NO_CAP, None);
    assert_left_alone(&registered, &mut ctx);
}

/// `when_registerBtcTransaction_sender_not_recognized_after_rskip170_lock`.
#[test]
fn from_rskip170_a_peg_in_without_a_sender_is_credited_where_its_instructions_say() {
    let destination = RskAddress::repeat_byte(0xd5);
    let pegin = pegin(no_sender(), Some(rskt(1, destination, None)));
    let (registered, mut ctx) = registered(&pegin, Iris300, NO_CAP, None);
    assert_credited(&registered, &mut ctx, destination, Iris300);
}

/// `when_registerBtcTransaction_usesLegacyType_beforeFork_lock_and_no_refund`.
#[test]
fn before_rskip143_a_whitelisted_p2pkh_sender_is_credited() {
    let key = sender();
    let pegin = pegin(p2pkh_spend(&key), None);
    let (registered, mut ctx) = registered(&pegin, Wasabi100, NO_CAP, Some(hash160(&key)));
    assert_credited(&registered, &mut ctx, rsk_address_of(&key), Wasabi100);
    assert!(
        !whitelisted_once(&mut ctx, &hash160(&key)),
        "the entry is used up"
    );
}

/// `when_registerBtcTransaction_usesLegacyType_afterFork_notWhitelisted_no_lock_and_refund`.
#[test]
fn from_rskip143_a_p2pkh_sender_not_whitelisted_is_refunded() {
    let key = sender();
    let pegin = pegin(p2pkh_spend(&key), None);
    let (registered, mut ctx) = registered(&pegin, Papyrus200, NO_CAP, None);
    assert_refunded(&registered, &mut ctx, &pegin, &p2pkh(&hash160(&key)));
    assert_eq!(balance(&mut ctx, rsk_address_of(&key)), U256::ZERO);
}

/// `when_registerBtcTransaction_usesLegacyType_afterFork_lock_and_no_refund`.
#[test]
fn from_rskip143_a_whitelisted_p2pkh_sender_is_credited() {
    let key = sender();
    let pegin = pegin(p2pkh_spend(&key), None);
    let (registered, mut ctx) = registered(&pegin, Papyrus200, NO_CAP, Some(hash160(&key)));
    assert_credited(&registered, &mut ctx, rsk_address_of(&key), Papyrus200);
    assert!(
        !whitelisted_once(&mut ctx, &hash160(&key)),
        "the entry is used up"
    );
}

/// `when_registerBtcTransaction_usesSegCompatibilityType_beforeFork_no_lock_and_no_refund`.
#[test]
fn before_rskip143_a_p2sh_p2wpkh_sender_is_left_alone() {
    let key = sender();
    let pegin = pegin(p2sh_p2wpkh_spend(&key), None);
    let (registered, mut ctx) = registered(&pegin, Wasabi100, NO_CAP, Some(p2sh_p2wpkh_hash(&key)));
    assert_left_alone(&registered, &mut ctx);
}

/// `when_registerBtcTransaction_usesSegCompatibilityType_afterFork_lock_and_no_refund`.
#[test]
fn from_rskip143_a_whitelisted_p2sh_p2wpkh_sender_is_credited() {
    let key = sender();
    let pegin = pegin(p2sh_p2wpkh_spend(&key), None);
    let (registered, mut ctx) =
        registered(&pegin, Papyrus200, NO_CAP, Some(p2sh_p2wpkh_hash(&key)));
    assert_credited(&registered, &mut ctx, rsk_address_of(&key), Papyrus200);
    assert!(
        !whitelisted_once(&mut ctx, &p2sh_p2wpkh_hash(&key)),
        "the entry is used up"
    );
}

/// `when_registerBtcTransaction_usesSegCompatibilityType_afterFork_notWhitelisted_no_lock_and_refund`:
/// refunded to the sender's P2SH address.
#[test]
fn from_rskip143_a_p2sh_p2wpkh_sender_not_whitelisted_is_refunded() {
    let key = sender();
    let pegin = pegin(p2sh_p2wpkh_spend(&key), None);
    let (registered, mut ctx) = registered(&pegin, Papyrus200, NO_CAP, None);
    assert_refunded(
        &registered,
        &mut ctx,
        &pegin,
        &p2sh(&p2sh_p2wpkh_hash(&key)),
    );
}

/// `when_registerBtcTransaction_usesMultisigType_beforeFork_no_lock_and_no_refund`.
#[test]
fn before_rskip143_a_multisig_sender_is_left_alone() {
    let pegin = pegin(p2sh_multisig_spend(&multisig_redeem(), 2), None);
    let (registered, mut ctx) = registered(&pegin, Wasabi100, NO_CAP, None);
    assert_left_alone(&registered, &mut ctx);
}

/// `when_registerBtcTransaction_usesMultisigType_afterFork_no_lock_and_refund`
/// and `registerPegIn_version0_tx_no_lockable_by_invalid_sender`: refunded to
/// the multisig's P2SH address.
#[test]
fn from_rskip143_a_multisig_sender_is_refunded() {
    let redeem = multisig_redeem();
    let pegin = pegin(p2sh_multisig_spend(&redeem, 2), None);
    let (registered, mut ctx) = registered(&pegin, Papyrus200, NO_CAP, None);
    assert_refunded(&registered, &mut ctx, &pegin, &p2sh_of(&redeem));
}

/// `when_registerBtcTransaction_usesMultisigWithWitnessType_beforeFork_no_lock_and_no_refund`.
#[test]
fn before_rskip143_a_p2sh_p2wsh_multisig_sender_is_left_alone() {
    let pegin = pegin(p2sh_p2wsh_spend(&multisig_redeem(), 2), None);
    let (registered, mut ctx) = registered(&pegin, Wasabi100, NO_CAP, None);
    assert_left_alone(&registered, &mut ctx);
}

/// `when_registerBtcTransaction_usesMultisigWithWitnessType_afterFork_no_lock_and_refund`:
/// refunded to the multisig's P2SH-P2WSH address.
#[test]
fn from_rskip143_a_p2sh_p2wsh_multisig_sender_is_refunded() {
    let redeem = multisig_redeem();
    let pegin = pegin(p2sh_p2wsh_spend(&redeem, 2), None);
    let (registered, mut ctx) = registered(&pegin, Papyrus200, NO_CAP, None);
    assert_refunded(&registered, &mut ctx, &pegin, &p2sh_p2wsh_of(&redeem));
}

/// `registerBtcTransaction_accepts_lock_tx_version1_after_rskip_170_activation`.
#[test]
fn from_rskip170_a_peg_in_is_credited_where_its_instructions_say() {
    let key = sender();
    let destination = RskAddress::repeat_byte(0xd1);
    let pegin = pegin(p2pkh_spend(&key), Some(rskt(1, destination, None)));
    let (registered, mut ctx) = registered(&pegin, Iris300, NO_CAP, None);
    assert_credited(&registered, &mut ctx, destination, Iris300);
    assert_eq!(balance(&mut ctx, rsk_address_of(&key)), U256::ZERO);
}

/// `registerBtcTransaction_ignores_pegin_instructions_before_rskip_170_activation`.
#[test]
fn before_rskip170_a_peg_in_is_credited_to_its_sender_whatever_its_instructions_say() {
    let key = sender();
    let destination = RskAddress::repeat_byte(0xd2);
    let pegin = pegin(p2pkh_spend(&key), Some(rskt(1, destination, None)));
    let (registered, mut ctx) = registered(&pegin, Papyrus200, NO_CAP, Some(hash160(&key)));
    assert_credited(&registered, &mut ctx, rsk_address_of(&key), Papyrus200);
    assert_eq!(balance(&mut ctx, destination), U256::ZERO);
}

/// `when_registerBtcTransaction_invalidPeginProtocolVersion_afterFork_no_lock_and_refund`:
/// instructions of a version rskj does not know, 2, are refunded to the
/// sender.
#[test]
fn from_rskip170_a_peg_in_of_an_unknown_version_is_refunded() {
    let key = sender();
    let pegin = pegin(
        p2pkh_spend(&key),
        Some(rskt(2, RskAddress::repeat_byte(0xd3), None)),
    );
    let (registered, mut ctx) = registered(&pegin, Iris300, NO_CAP, None);
    assert_refunded(&registered, &mut ctx, &pegin, &p2pkh(&hash160(&key)));
}

/// `registerPegIn_version0_tx_no_lockable_by_surpassing_locking_cap`, at
/// Papyrus200, which brings RSKIP134.
#[test]
fn from_rskip134_a_legacy_peg_in_past_the_locking_cap_is_refunded() {
    let key = sender();
    let pegin = pegin(p2pkh_spend(&key), None);
    let (registered, mut ctx) = registered(&pegin, Papyrus200, COIN, Some(hash160(&key)));
    assert_refunded(&registered, &mut ctx, &pegin, &p2pkh(&hash160(&key)));
}

/// `registerPegIn_version1_tx_no_lockable_by_surpassing_locking_cap`: with no
/// refund address in the instructions, refunded to the sender.
#[test]
fn from_rskip170_a_peg_in_past_the_locking_cap_is_refunded_to_its_sender() {
    let key = sender();
    let pegin = pegin(
        p2pkh_spend(&key),
        Some(rskt(1, RskAddress::repeat_byte(0xd4), None)),
    );
    let (registered, mut ctx) = registered(&pegin, Iris300, COIN, None);
    assert_refunded(&registered, &mut ctx, &pegin, &p2pkh(&hash160(&key)));
}

/// `registerPegIn_version1_tx_no_lockable_by_surpassing_locking_cap_unknown_sender_with_refund_address`.
#[test]
fn from_rskip170_a_peg_in_without_a_sender_past_the_locking_cap_is_refunded_where_it_says() {
    let refund_to = [0x7a; 20];
    let pegin = pegin(
        no_sender(),
        Some(rskt(
            1,
            RskAddress::repeat_byte(0xd6),
            Some(Refund::P2pkh(refund_to)),
        )),
    );
    let (registered, mut ctx) = registered(&pegin, Iris300, COIN, None);
    assert_refunded(&registered, &mut ctx, &pegin, &p2pkh(&refund_to));
}

/// `registerPegIn_version1_tx_no_lockable_by_surpassing_locking_cap_unknown_sender_without_refund_address`:
/// nowhere to refund to.
#[test]
fn from_rskip170_a_peg_in_without_a_sender_or_refund_address_past_the_locking_cap_is_not_refunded()
{
    let pegin = pegin(
        no_sender(),
        Some(rskt(1, RskAddress::repeat_byte(0xd6), None)),
    );
    let (_, mut ctx) = registered(&pegin, Iris300, COIN, None);
    assert!(waiting_pegouts(&mut ctx).is_empty(), "no refund");
    assert_eq!(
        balance(&mut ctx, BRIDGE_ADDR),
        wei(MAX_RBTC),
        "nothing credited"
    );
}

/// `registerPegIn_errorParsingPeginInstructions_beforeRskip170_dontRefundSender`:
/// before RSKIP170 the instructions are not read, and with no sender rskj
/// reads, the peg-in is left alone.
#[test]
fn before_rskip170_unreadable_instructions_are_not_refunded() {
    let pegin = pegin(no_sender(), Some(unreadable_instructions()));
    let (registered, mut ctx) = registered(&pegin, Papyrus200, NO_CAP, None);
    assert_left_alone(&registered, &mut ctx);
}

/// `registerPegIn_errorParsingPeginInstructions_afterRskip170_refundSender`.
#[test]
fn from_rskip170_unreadable_instructions_are_refunded_to_the_sender() {
    let key = sender();
    let pegin = pegin(p2pkh_spend(&key), Some(unreadable_instructions()));
    let (registered, mut ctx) = registered(&pegin, Iris300, NO_CAP, None);
    assert_refunded(&registered, &mut ctx, &pegin, &p2pkh(&hash160(&key)));
}
