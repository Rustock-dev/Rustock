//! rskj's `BridgeSupportTest` tests of the event a credited legacy peg-in
//! logs (`eventLoggerLogLockBtc_*`, `eventLoggerLogPeginBtc_*`), of peg-ins
//! from a sender the whitelist does not name
//! (`registerBtcTransactionLockTxNotWhitelisted_*`), and of a segwit peg-in
//! registered twice (`registerBtcTransaction_sending_segwit_tx_twice_locks_just_once`).

use alloy_primitives::U256;
use bitcoin::{OutPoint, Transaction, Witness};

use crate::bridge::constants::BridgeConstants;
use crate::bridge::{btc_chain, peg, storage, tx as bridge_tx};
use crate::hardfork::{RskNetworkUpgrade, RSK_REGTEST_CHAIN_ID};
use crate::precompiles::BRIDGE_ADDR;

use super::super::support::*;
use super::waiting_in;

use RskNetworkUpgrade::{Iris300, Papyrus200, Wasabi100};

/// The Bitcoin height a peg-in is registered at.
const HEIGHT: u32 = 100;

const LOCK_BTC: &str = "lock_btc(address,bytes32,string,int256)";
const PEGIN_BTC: &str = "pegin_btc(address,bytes32,int256,int256)";
const RELEASE_REQUESTED: &str = "release_requested(bytes32,bytes32,uint256)";

/// rskj's regtest constants, with a locking cap no peg-in here reaches.
fn regtest() -> BridgeConstants {
    BridgeConstants {
        initial_locking_cap: 1_000 * COIN,
        ..BridgeConstants::rskj_regtest()
    }
}

/// A bitcoin from the P2PKH key `sender`, whitelisted for it, to rskj
/// regtest's federation, registered at RSK block 10 of the regtest ladder up
/// to `last`: the peg-in, what its registration answered, and the Bridge.
fn whitelisted_pegin(sender: &[u8; 33], last: RskNetworkUpgrade) -> (Transaction, Registered, Ctx) {
    let config = regtest();
    let cfg = up_to(last, RSK_REGTEST_CHAIN_ID);
    let pegin = tx(
        vec![p2pkh_spend(sender)],
        &[(COIN, genesis_federation_output(&config))],
    );
    let mut ctx = bridge_holding(&pegin, HEIGHT, 10, &config, &cfg);
    whitelist_once(&mut ctx, hash160(sender), COIN);
    let registered = register(&mut ctx, &pegin, i64::from(HEIGHT), &config, &cfg);
    (pegin, registered, ctx)
}

/// `eventLoggerLogLockBtc_before_rskip_146_activation`: before RSKIP146 a
/// credited peg-in logs no `lock_btc`.
#[test]
fn before_rskip146_a_credited_peg_in_logs_no_lock_btc() {
    let sender = key_from_seed("seed");
    let (_, registered, mut ctx) = whitelisted_pegin(&sender, Wasabi100);
    assert!(registered.processed);
    assert_eq!(balance(&mut ctx, rsk_address_of(&sender)), wei(COIN));
    assert_eq!(count_of(&registered.logs, LOCK_BTC), 0);
}

/// `eventLoggerLogLockBtc_after_rskip_146_activation` and
/// `eventLoggerLogPeginBtc_before_rskip_170_activation`: from RSKIP146, and
/// before RSKIP170, a credited peg-in logs `lock_btc` and not `pegin_btc`.
#[test]
fn from_rskip146_to_rskip170_a_credited_peg_in_logs_lock_btc() {
    let sender = key_from_seed("seed");
    let (pegin, registered, _) = whitelisted_pegin(&sender, Papyrus200);
    let sender_address = base58(111, &hash160(&sender));
    assert_eq!(
        registered.logs,
        [lock_btc(
            rsk_address_of(&sender),
            &pegin,
            &sender_address,
            COIN
        )]
    );
    assert_eq!(count_of(&registered.logs, PEGIN_BTC), 0);
}

/// `eventLoggerLogPeginBtc_after_rskip_170_activation`: from RSKIP170 it
/// logs `pegin_btc`, of protocol version 0, and not `lock_btc`.
#[test]
fn from_rskip170_a_credited_peg_in_logs_pegin_btc() {
    let sender = key_from_seed("seed");
    let (pegin, registered, _) = whitelisted_pegin(&sender, Iris300);
    assert_eq!(
        registered.logs,
        [pegin_btc(rsk_address_of(&sender), &pegin, COIN, 0)]
    );
    assert_eq!(count_of(&registered.logs, LOCK_BTC), 0);
}

/// rskj's `registerBtcTransactionLockTxNotWhitelisted_*`: three peg-ins of a
/// P2PKH sender the whitelist does not name, one to the active federation,
/// one to the retiring one and one to both, each refunded from what it sent
/// to the sender.
fn not_whitelisted(last: RskNetworkUpgrade) {
    let config = regtest();
    let cfg = up_to(last, RSK_REGTEST_CHAIN_ID);
    // Past the activation age, so that the federation created at block 0 is
    // active and the one before it retiring.
    let block = config.federation_activation_age_legacy + 10;
    let retiring = [
        public_key(&[0xfa, 0x03]),
        public_key(&[0xfa, 0x04]),
        public_key(&[0xfa, 0x05]),
    ];
    let active = [
        public_key(&[0xfb, 0x03]),
        public_key(&[0xfb, 0x04]),
        public_key(&[0xfb, 0x05]),
    ];
    let to_active = p2sh_of(&standard_redeem(&active));
    let to_retiring = p2sh_of(&standard_redeem(&retiring));
    let sender = key_from_seed("not whitelisted");
    let pegins = [
        tx(vec![p2pkh_spend(&sender)], &[(5 * COIN, to_active.clone())]),
        tx(
            vec![p2pkh_spend(&sender)],
            &[(10 * COIN, to_retiring.clone())],
        ),
        tx(
            vec![p2pkh_spend(&sender)],
            &[(3 * COIN, to_active), (4 * COIN, to_retiring)],
        ),
    ];
    let roots: Vec<[u8; 32]> = pegins
        .iter()
        .map(|pegin| bridge_tx::calculate_btc_tx_hash(&bitcoin::consensus::serialize(pegin)))
        .collect();
    let mut ctx = ctx_at(block);
    let head = HEIGHT + 2 + config.btc2rsk_minimum_acceptable_confirmations - 1;
    hold_roots(&mut ctx, &roots, HEIGHT, head, &cfg);
    fund_bridge_but(&mut ctx, 0);
    store_federation(&mut ctx, OLD_FEDERATION, &retiring, 0, 1000);
    store_federation(&mut ctx, NEW_FEDERATION, &active, 0, 1000);
    set_fee_per_kb(&mut ctx, MILLICOIN);

    let rskip146 = cfg.has_rskip146(block);
    for (height, pegin) in (HEIGHT..).zip(&pegins) {
        let registered = register(&mut ctx, pegin, i64::from(height), &config, &cfg);
        assert!(registered.processed);
        // rskj's `logReleaseBtcRequested`, from RSKIP146.
        assert_eq!(
            count_of(&registered.logs, RELEASE_REQUESTED),
            usize::from(rskip146)
        );
    }
    assert_eq!(balance(&mut ctx, rsk_address_of(&sender)), U256::ZERO);
    assert_eq!(balance(&mut ctx, BRIDGE_ADDR), wei(MAX_RBTC));
    // Before RSKIP146 a refund waits without the hash of the transaction that
    // made it, and from RSKIP146 with it.
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
    let (refunds, none) = if rskip146 {
        (with_hash, without_hash)
    } else {
        (without_hash, with_hash)
    };
    assert!(none.is_empty());
    assert_eq!(refunds.len(), 3);
    for pegin in &pegins {
        let txid = pegin.compute_txid();
        let refund = refunds
            .iter()
            .find(|refund| refund.input[0].previous_output.txid == txid)
            .expect("its refund");
        assert_eq!(refund.output.len(), 1);
        assert_eq!(refund.output[0].script_pubkey, p2pkh(&hash160(&sender)));
        let sent: u64 = pegin
            .output
            .iter()
            .map(|output| output.value.to_sat())
            .sum();
        assert!(sent - refund.output[0].value.to_sat() <= MILLICOIN);
        let spent: Vec<OutPoint> = refund
            .input
            .iter()
            .map(|input| input.previous_output)
            .collect();
        let outputs: Vec<OutPoint> = (0..pegin.output.len() as u32)
            .map(|vout| OutPoint { txid, vout })
            .collect();
        assert_eq!(spent, outputs);
    }
    let for_signatures = peg::deserialize_rsk_txs_waiting_for_signatures(
        &storage::bridge_load_bytes_named(&mut ctx, storage::PEGOUTS_WAITING_FOR_SIGNATURES_KEY),
    );
    assert!(for_signatures.is_empty());
}

/// `registerBtcTransactionLockTxNotWhitelisted_before_rskip_146_activation`.
#[test]
fn before_rskip146_peg_ins_of_a_sender_not_whitelisted_are_refunded() {
    not_whitelisted(Wasabi100);
}

/// `registerBtcTransactionLockTxNotWhitelisted_after_rskip_146_activation`,
/// at Papyrus200, which brings RSKIP146.
#[test]
fn from_rskip146_peg_ins_of_a_sender_not_whitelisted_are_refunded() {
    not_whitelisted(Papyrus200);
}

/// `registerBtcTransaction_sending_segwit_tx_twice_locks_just_once`, at
/// Papyrus200, which brings RSKIP143: a segwit peg-in registered with its
/// witness, proven by the witness root its block's coinbase registered, then
/// without it, proven by the block's merkle root. It is marked processed by
/// its id without the witness, so the second registration credits nothing.
#[test]
fn a_segwit_peg_in_registered_twice_is_credited_once() {
    let config = regtest();
    let cfg = up_to(Papyrus200, RSK_REGTEST_CHAIN_ID);
    let sender = key_from_seed("segwit twice");
    let pegin = tx(
        vec![p2sh_p2wpkh_spend(&sender)],
        &[(5 * COIN, genesis_federation_output(&config))],
    );
    let mut stripped = pegin.clone();
    stripped.input[0].witness = Witness::new();
    let with_witness = bitcoin::consensus::serialize(&pegin);
    let without_witness = bitcoin::consensus::serialize(&stripped);

    let mut ctx = ctx_at(10);
    let head = HEIGHT + config.btc2rsk_minimum_acceptable_confirmations - 1;
    let root = bridge_tx::calculate_btc_tx_hash(&without_witness);
    let header = hold_with_root(&mut ctx, root, HEIGHT, head, &cfg);
    // Kept as `registerBtcCoinbaseTransaction` keeps it, as it is displayed.
    let mut witness_root = bridge_tx::calculate_btc_tx_hash(&with_witness);
    witness_root.reverse();
    let block_hash = btc_chain::bitcoin_hash_to_b256(&header.block_hash()).0;
    bridge_tx::set_coinbase_information(&mut ctx, &block_hash, &witness_root);
    fund_bridge_but(&mut ctx, 0);
    // rskj's whitelist here is off from height 0.
    storage::store_one_off_whitelist(&mut ctx, &[], 0);

    let first = register_bytes(
        &mut ctx,
        &with_witness,
        &pegin,
        i64::from(HEIGHT),
        &config,
        &cfg,
    );
    assert!(first.processed);
    assert_eq!(balance(&mut ctx, rsk_address_of(&sender)), wei(5 * COIN));
    let second = register_bytes(
        &mut ctx,
        &without_witness,
        &stripped,
        i64::from(HEIGHT),
        &config,
        &cfg,
    );
    assert!(second.processed);
    assert!(second.logs.is_empty());
    assert_eq!(balance(&mut ctx, rsk_address_of(&sender)), wei(5 * COIN));
    assert_eq!(storage::load_federation_utxos(&mut ctx, &cfg, 10).len(), 1);
}
