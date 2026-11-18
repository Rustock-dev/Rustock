//! rskj's `registerBtcTransaction_{before,after}_RSKIP134_activation_sends_
//! {above,exactly,below}_lockingcap` and the `assertLockingCap` they call: a
//! legacy peg-in of a whitelisted P2PKH sender to both federations, the
//! retiring one first, with what the federations already hold, against a
//! locking cap. RSKIP134 is off below Papyrus200 and on from it.
//!
//! rskj's `assertLockingCap` mocks a federation activation age of 0 and
//! registers at RSK block 10. Here mainnet's constants hold, so the new
//! federation, created at block 0, is active from block 18,500, and the
//! peg-ins are registered at block 20,000. What the federations hold is what
//! the Bridge has paid out, as rskj's cap reads it
//! (`BridgeSupport.verifyLockDoesNotSurpassLockingCap`); rskj also puts it in
//! the federations' UTXOs, which neither the cap nor the refund reads. rskj
//! asserts some of its rows twice or three times over; each row is here once.

use bitcoin::OutPoint;

use crate::bridge::constants::BridgeConstants;
use crate::hardfork::{RskNetworkUpgrade, RSK_MAINNET_CHAIN_ID};
use crate::precompiles::BRIDGE_ADDR;

use super::super::support::*;

use RskNetworkUpgrade::{Papyrus200, Wasabi100};

/// One bitcoin.
const C: u64 = COIN;

/// One satoshi (bitcoinj's `Coin.SATOSHI`).
const SAT: u64 = 1;

/// The RSK block the peg-ins are registered at.
const BLOCK: u64 = 20_000;

/// `assertLockingCap(shouldLock, isLockingCapEnabled, lockingCap,
/// amountSentToNewFed, amountSentToOldFed, amountInNewFed, amountInOldFed)`.
fn assert_locking_cap(
    should_lock: bool,
    enabled: bool,
    cap: u64,
    sent_to_new: u64,
    sent_to_old: u64,
    in_new: u64,
    in_old: u64,
) {
    let case =
        format!("{should_lock} {enabled} {cap} {sent_to_new} {sent_to_old} {in_new} {in_old}");
    let config = BridgeConstants {
        minimum_pegin_tx_value: SAT,
        legacy_minimum_pegin_tx_value: SAT,
        initial_locking_cap: cap,
        btc2rsk_minimum_acceptable_confirmations: 1,
        ..BridgeConstants::mainnet()
    };
    let cfg = up_to(
        if enabled { Papyrus200 } else { Wasabi100 },
        RSK_MAINNET_CHAIN_ID,
    );
    // `PegTestUtils.createSimpleActiveFederation`, and the new federation of
    // the keys fb01, fb02 and fb03.
    let old_keys = [public_key(&[0xfa, 0x01]), public_key(&[0xfa, 0x02])];
    let new_keys = [
        public_key(&[0xfb, 0x01]),
        public_key(&[0xfb, 0x02]),
        public_key(&[0xfb, 0x03]),
    ];
    let sender = key_from_seed("assertLockingCap");
    let pegin = tx(
        vec![p2pkh_spend(&sender)],
        &[
            (sent_to_old, p2sh_of(&standard_redeem(&old_keys))),
            (sent_to_new, p2sh_of(&standard_redeem(&new_keys))),
        ],
    );
    let lock_value = sent_to_new + sent_to_old;
    let funds = in_new + in_old;

    let mut ctx = bridge_holding(&pegin, 1, BLOCK, &config, &cfg);
    fund_bridge_but(&mut ctx, funds);
    store_federation(&mut ctx, OLD_FEDERATION, &old_keys, 0, 1000);
    store_federation(&mut ctx, NEW_FEDERATION, &new_keys, 0, 1000);
    whitelist_once(&mut ctx, hash160(&sender), lock_value);
    set_fee_per_kb(&mut ctx, MILLICOIN);

    let registered = register(&mut ctx, &pegin, 1, &config, &cfg);

    assert!(registered.processed, "processed: {case}");
    // rskj's whitelist uses the entry up before the cap is looked at
    // (`WhitelistSupportImpl.verifyLockSenderIsWhitelisted`), locked or not.
    assert!(
        !whitelisted_once(&mut ctx, &hash160(&sender)),
        "the entry is used up: {case}"
    );
    let locked = if should_lock { lock_value } else { 0 };
    assert_eq!(
        balance(&mut ctx, rsk_address_of(&sender)),
        wei(locked),
        "credited: {case}"
    );
    assert_eq!(
        balance(&mut ctx, BRIDGE_ADDR),
        wei(MAX_RBTC - funds - locked),
        "the Bridge: {case}"
    );
    let refunds = waiting_pegouts(&mut ctx);
    if should_lock {
        assert!(refunds.is_empty(), "no refund: {case}");
        return;
    }
    assert_eq!(refunds.len(), 1, "a refund: {case}");
    let refund = &refunds[0];
    assert_eq!(refund.output.len(), 1, "{case}");
    assert_eq!(
        refund.output[0].script_pubkey,
        p2pkh(&hash160(&sender)),
        "{case}"
    );
    // It spends the peg-in's outputs and nothing else, so what it pays and its
    // fee are what the peg-in sent.
    assert!(refund.output[0].value.to_sat() < lock_value, "{case}");
    let txid = pegin.compute_txid();
    let spent: Vec<OutPoint> = refund
        .input
        .iter()
        .map(|input| input.previous_output)
        .collect();
    assert_eq!(
        spent,
        [OutPoint { txid, vout: 0 }, OutPoint { txid, vout: 1 }],
        "{case}"
    );
}

/// `registerBtcTransaction_before_RSKIP134_activation_sends_above_lockingcap`.
#[test]
fn before_rskip134_a_peg_in_above_the_locking_cap_is_credited() {
    for (cap, to_new, to_old, in_new, in_old) in [
        (3 * C, C, C, C, C),
        (3 * C, 0, 2 * C, C, C),
        (3 * C, 2 * C, 0, C, C),
        (3 * C, 2 * C, 0, 2 * C, 0),
        (3 * C, 2 * C, 0, 0, 2 * C),
        (5 * C, 5 * C, 0, 0, SAT),
        (5 * C, 5 * C, 0, SAT, 0),
    ] {
        assert_locking_cap(true, false, cap, to_new, to_old, in_new, in_old);
    }
}

/// `registerBtcTransaction_before_RSKIP134_activation_sends_exactly_lockingcap`.
#[test]
fn before_rskip134_a_peg_in_up_to_the_locking_cap_is_credited() {
    for (to_new, to_old, in_new, in_old) in [
        (0, C, C, C),
        (C, 0, C, C),
        (C, C, C, C),
        (C, C, 0, C),
        (C, C, C, 0),
    ] {
        assert_locking_cap(true, false, 5 * C, to_new, to_old, in_new, in_old);
    }
}

/// `registerBtcTransaction_before_RSKIP134_activation_sends_below_lockingcap`.
#[test]
fn before_rskip134_a_peg_in_below_the_locking_cap_is_credited() {
    for (to_new, to_old, in_new, in_old) in [
        (0, C, C, C),
        (C, 0, C, C),
        (C, C, C, C),
        (C, C, C, 0),
        (C, C, 0, C),
    ] {
        assert_locking_cap(true, false, 6 * C, to_new, to_old, in_new, in_old);
    }
}

/// `registerBtcTransaction_after_RSKIP134_activation_sends_above_lockingcap`.
#[test]
fn from_rskip134_a_peg_in_above_the_locking_cap_is_refunded() {
    for (cap, to_new, to_old, in_new, in_old) in [
        (4 * C, C, 2 * C, C, C),
        (4 * C, 0, 3 * C, C, C),
        (4 * C, 3 * C, 0, C, C),
        (4 * C, 3 * C, 0, 2 * C, 0),
        (4 * C, 3 * C, 0, 0, 2 * C),
        (5 * C, 5 * C, 0, 0, SAT),
        (5 * C, 5 * C, 0, SAT, 0),
        (5 * C, 5 * C, SAT, 0, 0),
        (5 * C, 5 * C + SAT, 0, 0, 0),
    ] {
        assert_locking_cap(false, true, cap, to_new, to_old, in_new, in_old);
    }
}

/// `registerBtcTransaction_after_RSKIP134_activation_sends_exactly_lockingcap`.
#[test]
fn from_rskip134_a_peg_in_up_to_the_locking_cap_is_credited() {
    for (to_new, to_old, in_new, in_old) in [
        (0, C, C, C),
        (C, 0, C, C),
        (C, C, C, C),
        (C, C, 0, C),
        (C, C, C, 0),
    ] {
        assert_locking_cap(true, true, 5 * C, to_new, to_old, in_new, in_old);
    }
}

/// `registerBtcTransaction_after_RSKIP134_activation_sends_below_lockingcap`.
#[test]
fn from_rskip134_a_peg_in_below_the_locking_cap_is_credited() {
    for (to_new, to_old, in_new, in_old) in [
        (0, C, C, C),
        (C, 0, C, C),
        (C, C, C, C),
        (C, C, C, 0),
        (C, C, 0, C),
    ] {
        assert_locking_cap(true, true, 6 * C, to_new, to_old, in_new, in_old);
    }
}
