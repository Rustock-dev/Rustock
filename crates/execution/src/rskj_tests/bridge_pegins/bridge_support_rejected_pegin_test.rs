//! rskj's `BridgeSupportRejectedPeginTest`
//! (`co/rsk/peg/BridgeSupportRejectedPeginTest.java`): from RSKIP379, peg-ins
//! rskj turns down and can't refund, and transactions it leaves alone.
//!
//! Each runs at arrowhead600, reed800 and vetiver900 on mainnet, with rskj's
//! test's federations: an active P2SH ERP federation of five members
//! (`fa07` to `fa11`) created at block 2, and for one test a P2SH ERP
//! federation of three (`fa04` to `fa06`) created at block 1 before it, both
//! with mainnet's emergency keys. The Bridge runs at block 3, so a newer
//! federation with an older one beside it isn't active yet. A peg-in is
//! registered at mainnet's pegout index's height, with one more confirmation
//! than it needs. Between RSKIP459 (lovell700) and RSKIP551 (vetiver900) a
//! peg-in turned down without a refund is marked processed; outside it, it
//! isn't (`BridgeSupportTestUtil.shouldMarkRejectedPeginAsProcessed`).
//!
//! Read in full: its two other tests, of a Flyover payment and of a
//! transaction that pays no federation, are ported too.

use alloy_primitives::U256;
use bitcoin::{ScriptBuf, Transaction, Witness};

use crate::bridge::constants::BridgeConstants;
use crate::bridge::storage;
use crate::hardfork::{RskHardforkConfig, RskNetworkUpgrade, RSK_MAINNET_CHAIN_ID};

use super::support::*;

/// The ladders rskj's `activationsProvider` gives: arrowhead600, reed800 and
/// every upgrade.
fn ladders() -> [RskHardforkConfig; 3] {
    [
        RskNetworkUpgrade::Arrowhead600,
        RskNetworkUpgrade::Reed800,
        RskNetworkUpgrade::Vetiver900,
    ]
    .map(|last| up_to(last, RSK_MAINNET_CHAIN_ID))
}

/// rskj's `shouldMarkRejectedPeginAsProcessed`: RSKIP459 and not RSKIP551.
fn marks(cfg: &RskHardforkConfig) -> bool {
    cfg.has_rskip459(3) && !cfg.has_rskip551(3)
}

/// The members of a federation, from rskj's seeds.
fn members(seeds: &[&str]) -> Vec<[u8; 33]> {
    seeds.iter().map(|seed| key_from_seed(seed)).collect()
}

/// The active federation's members.
fn active() -> Vec<[u8; 33]> {
    members(&["fa07", "fa08", "fa09", "fa10", "fa11"])
}

/// The older federation's members.
fn older() -> Vec<[u8; 33]> {
    members(&["fa04", "fa05", "fa06"])
}

/// The redeem script of mainnet's P2SH ERP federation of `members`.
fn redeem(members: &[[u8; 33]]) -> Vec<u8> {
    let config = BridgeConstants::mainnet();
    erp_redeem(
        members,
        &keys_of(config.erp_fed_pubkeys),
        config.erp_fed_activation_delay,
    )
}

/// The output a P2SH ERP federation of `members` is paid at.
fn output(members: &[[u8; 33]]) -> ScriptBuf {
    p2sh_of(&redeem(members))
}

/// The Bitcoin height a peg-in is registered at: where mainnet starts reading
/// the pegout index.
fn index_height() -> u32 {
    let config = BridgeConstants::mainnet();
    config.btc_height_when_pegout_tx_index_activates
        + config.pegout_tx_index_grace_period_in_btc_blocks
}

/// `tx` registered at RSK block 3 of `cfg` on mainnet, the active federation
/// stored and, with `with_older`, the older one too: what it answered and left
/// behind, and the Bridge after it.
fn registered(tx: &Transaction, cfg: &RskHardforkConfig, with_older: bool) -> (Registered, Ctx) {
    let config = BridgeConstants::mainnet();
    let height = index_height();
    let mut ctx = ctx_at(3);
    let raw = bitcoin::consensus::serialize(tx);
    hold(
        &mut ctx,
        &raw,
        height,
        height + config.btc2rsk_minimum_acceptable_confirmations,
        cfg,
    );
    fund_bridge_but(&mut ctx, 0);
    store_federation(&mut ctx, NEW_FEDERATION, &active(), 2, 3000);
    if with_older {
        store_federation(&mut ctx, OLD_FEDERATION, &older(), 1, 3000);
    }
    let registered = register(&mut ctx, tx, i64::from(height), &config, cfg);
    (registered, ctx)
}

/// No UTXO of either federation's is registered.
fn no_utxos(ctx: &mut Ctx, cfg: &RskHardforkConfig) {
    assert!(storage::load_federation_utxos(ctx, cfg, 3).is_empty());
    assert!(storage::load_old_federation_utxos(ctx).is_empty());
}

/// A spend from an input rskj names no sender for.
fn from_no_one() -> (ScriptBuf, Witness) {
    no_sender()
}

/// rskj `registerBtcTransaction_whenBelowTheMinimum_shouldRejectPegin`: a
/// peg-in of a satoshi under the minimum is turned down as INVALID_AMOUNT (5),
/// with no refund (3), nothing credited or registered.
#[test]
fn a_peg_in_below_the_minimum_is_turned_down_without_a_refund() {
    let minimum = BridgeConstants::mainnet().minimum_pegin_tx_value;
    let pegin = tx(vec![from_no_one()], &[(minimum - 1, output(&active()))]);
    for cfg in ladders() {
        let (refused, mut ctx) = registered(&pegin, &cfg, false);
        assert_eq!(
            refused,
            Registered {
                returned: true,
                processed: marks(&cfg),
                logs: vec![rejected_pegin(&pegin, 5), unrefundable_pegin(&pegin, 3)],
            }
        );
        no_utxos(&mut ctx, &cfg);
    }
}

/// rskj `registerBtcTransaction_whenUndeterminedSender_shouldRejectPegin`: a
/// peg-in from no sender rskj can name, with no instructions, is turned down
/// as LEGACY_PEGIN_UNDETERMINED_SENDER (3) with no refund (1): no refund is
/// built, nothing credited or registered.
#[test]
fn a_peg_in_from_an_undetermined_sender_is_turned_down_without_a_refund() {
    let pegin = tx(vec![from_no_one()], &[(COIN, output(&active()))]);
    for cfg in ladders() {
        let (refused, mut ctx) = registered(&pegin, &cfg, false);
        assert_eq!(
            refused,
            Registered {
                returned: true,
                processed: marks(&cfg),
                logs: vec![rejected_pegin(&pegin, 3), unrefundable_pegin(&pegin, 1)],
            }
        );
        no_utxos(&mut ctx, &cfg);
        assert!(waiting_pegouts(&mut ctx).is_empty());
    }
}

/// rskj `registerBtcTransaction_whenPeginV1WithInvalidPayloadAndUnderminedSender_shouldRejectPegin`:
/// instructions of version 1 with nothing after the version don't parse: from
/// no sender rskj can name the peg-in is turned down as
/// PEGIN_V1_INVALID_PAYLOAD (4) with no refund, LEGACY_PEGIN_UNDETERMINED_SENDER
/// (1) since the version read is still 0.
#[test]
fn a_peg_in_whose_instructions_do_not_parse_from_no_sender_is_not_refunded() {
    let pegin = tx(
        vec![from_no_one()],
        &[(COIN, output(&active())), (0, op_return(b"RSKT\x01"))],
    );
    for cfg in ladders() {
        let (refused, mut ctx) = registered(&pegin, &cfg, false);
        assert_eq!(
            refused,
            Registered {
                returned: true,
                processed: marks(&cfg),
                logs: vec![rejected_pegin(&pegin, 4), unrefundable_pegin(&pegin, 1)],
            }
        );
        no_utxos(&mut ctx, &cfg);
        assert!(waiting_pegouts(&mut ctx).is_empty());
    }
}

/// rskj `registerBtcTransaction_whenUtxoToActiveFedBelowMinimumAndUtxoToRetiringFedAboveMinimum_shouldRejectPegin`:
/// with a newer federation stored beside an older one, and not yet old enough
/// to be active, the older one is the active one and the newer one no live
/// federation. A UTXO under the minimum to the newer one doesn't count, and
/// the minimum to the older one does: the peg-in is no invalid amount but
/// one from an undetermined sender (3, then 1).
#[test]
fn only_what_live_federations_are_paid_counts_toward_the_minimum() {
    let minimum = BridgeConstants::mainnet().minimum_pegin_tx_value;
    let pegin = tx(
        vec![from_no_one()],
        &[
            (minimum - 1, output(&active())),
            (minimum, output(&older())),
        ],
    );
    for cfg in ladders() {
        let (refused, mut ctx) = registered(&pegin, &cfg, true);
        assert_eq!(
            refused,
            Registered {
                returned: true,
                processed: marks(&cfg),
                logs: vec![rejected_pegin(&pegin, 3), unrefundable_pegin(&pegin, 1)],
            }
        );
        no_utxos(&mut ctx, &cfg);
    }
}

/// rskj `registerBtcTransaction_whenAttemptToRegisterFlyoverPegin_shouldIgnorePegin`:
/// a payment to a Flyover address of the active federation (`<hash> OP_DROP`
/// before its redeem script) pays no live federation:
/// `registerBtcTransaction` leaves it alone, logging nothing and not marking
/// it processed.
#[test]
fn a_payment_to_a_flyover_address_is_left_alone() {
    let mut flyover = push(&[0x33; 32]);
    flyover.push(0x75);
    flyover.extend(redeem(&active()));
    let payment = tx(vec![from_no_one()], &[(COIN, p2sh_of(&flyover))]);
    for cfg in ladders() {
        let (ignored, mut ctx) = registered(&payment, &cfg, false);
        assert_eq!(
            ignored,
            Registered {
                returned: true,
                processed: false,
                logs: vec![],
            }
        );
        no_utxos(&mut ctx, &cfg);
    }
}

/// rskj `registerBtcTransaction_whenNoUtxoToFed_shouldIgnorePegin`: a
/// transaction that pays no federation is left alone.
#[test]
fn a_transaction_that_pays_no_federation_is_left_alone() {
    let minimum = BridgeConstants::mainnet().minimum_pegin_tx_value;
    let user = key_from_seed("userAddress");
    let payment = tx(vec![from_no_one()], &[(minimum, p2pkh(&hash160(&user)))]);
    for cfg in ladders() {
        let (ignored, mut ctx) = registered(&payment, &cfg, false);
        assert_eq!(
            ignored,
            Registered {
                returned: true,
                processed: false,
                logs: vec![],
            }
        );
        no_utxos(&mut ctx, &cfg);
        assert_eq!(balance(&mut ctx, rsk_address_of(&user)), U256::ZERO);
    }
}
