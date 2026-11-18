//! rskj's `PeginInformationTest` (`co/rsk/peg/PeginInformationTest.java`):
//! what rskj reads of a peg-in, its version, RSK destination and refund
//! address, from its sender and, from RSKIP170, from its instructions
//! (`PeginInformation.parse`).
//!
//! rustock reads the same inside `registerBtcTransaction`, so each port
//! registers a peg-in and holds what rskj's reading leads to: who is
//! credited, the version its event carries, and, where the locking cap turns
//! it down, where its refund goes. They run on rskj's regtest, as rskj's
//! test does.
//!
//! Read in full. Its other test, `parse_fromBtcLockSender`, is a legacy peg-in
//! credited to its sender, which every other port here and in
//! `bridge_support_test` holds.

use alloy_primitives::U256;
use bitcoin::{ScriptBuf, Transaction, Witness};

use crate::bridge::constants::BridgeConstants;
use crate::hardfork::{RskNetworkUpgrade, RSK_REGTEST_CHAIN_ID};

use super::support::*;

use RskNetworkUpgrade::{Arrowhead600, Fingerroot500, Iris300, Papyrus200};

/// The Bitcoin height a peg-in is registered at: below regtest's pegout
/// index.
const HEIGHT: u32 = 100;

/// A locking cap no peg-in here reaches.
const NO_CAP: u64 = 1_000 * COIN;

/// A peg-in of one bitcoin to rskj regtest's federation from `input`, with
/// the output `instructions` beside it, if any.
fn pegin(input: (ScriptBuf, Witness), instructions: Option<ScriptBuf>) -> Transaction {
    let mut outputs = vec![(
        COIN,
        genesis_federation_output(&BridgeConstants::rskj_regtest()),
    )];
    outputs.extend(instructions.map(|script| (0, script)));
    tx(vec![input], &outputs)
}

/// `tx` registered at RSK block 10 of rskj's regtest ladder up to `last`,
/// at Bitcoin height `height`, with a locking cap of `cap` satoshis: what it
/// answered and left behind, and the Bridge after it.
fn registered_at(
    tx: &Transaction,
    last: RskNetworkUpgrade,
    height: u32,
    cap: u64,
) -> (Registered, Ctx) {
    let config = BridgeConstants {
        initial_locking_cap: cap,
        ..BridgeConstants::rskj_regtest()
    };
    let cfg = up_to(last, RSK_REGTEST_CHAIN_ID);
    let mut ctx = bridge_holding(tx, height, 10, &config, &cfg);
    let registered = register(&mut ctx, tx, i64::from(height), &config, &cfg);
    (registered, ctx)
}

/// [`registered_at`] at [`HEIGHT`].
fn registered(tx: &Transaction, last: RskNetworkUpgrade, cap: u64) -> (Registered, Ctx) {
    registered_at(tx, last, HEIGHT, cap)
}

/// The one refund the Bridge waits to send, and where it pays.
fn refund(ctx: &mut Ctx) -> (Transaction, ScriptBuf) {
    let refunds = waiting_pegouts(ctx);
    assert_eq!(refunds.len(), 1, "one refund");
    assert_eq!(refunds[0].output.len(), 1, "one output");
    let to = refunds[0].output[0].script_pubkey.clone();
    (refunds[0].clone(), to)
}

/// rskj `parse_fromPeginInstructions`: from RSKIP170 a peg-in's instructions
/// override what its sender gives, with version 1, their destination and
/// their refund address. Taken, it credits their destination, not the
/// sender's, and logs version 1; turned down by the locking cap, it is
/// refunded to their refund address.
#[test]
fn from_rskip170_the_instructions_give_the_destination_and_the_refund_address() {
    let (sender, them) = (public_key(&[0x51]), public_key(&[0x52]));
    let destination = rsk_address_of(&them);
    let instructions = rskt(1, destination, Some(Refund::P2pkh(hash160(&them))));
    let pegin = pegin(p2pkh_spend(&sender), Some(instructions));

    let (taken, mut ctx) = registered(&pegin, Iris300, NO_CAP);
    assert_eq!(
        taken,
        Registered {
            returned: true,
            processed: true,
            logs: vec![pegin_btc(destination, &pegin, COIN, 1)],
        }
    );
    assert_eq!(balance(&mut ctx, destination), wei(COIN));
    assert_eq!(balance(&mut ctx, rsk_address_of(&sender)), U256::ZERO);

    let (refused, mut ctx) = registered(&pegin, Iris300, COIN / 2);
    let (refund, to) = refund(&mut ctx);
    assert_eq!(to, p2pkh(&hash160(&them)));
    assert_eq!(
        refused,
        Registered {
            returned: true,
            processed: true,
            logs: vec![rejected_pegin(&pegin, 1), release_requested(&refund, COIN)],
        }
    );
    assert_eq!(balance(&mut ctx, destination), U256::ZERO);
}

/// rskj `parse_fromPeginInstructions_withoutBtcLockSender`: with no sender
/// rskj can name, the instructions still give version 1, the destination and
/// the refund address.
#[test]
fn with_no_sender_the_instructions_give_the_destination_and_the_refund_address() {
    let them = public_key(&[0x52]);
    let destination = rsk_address_of(&them);
    let instructions = rskt(1, destination, Some(Refund::P2sh(hash160(&them))));
    let pegin = pegin(p2wpkh_spend(&public_key(&[0x51])), Some(instructions));

    let (taken, mut ctx) = registered(&pegin, Iris300, NO_CAP);
    assert_eq!(taken.logs, vec![pegin_btc(destination, &pegin, COIN, 1)]);
    assert!(taken.returned && taken.processed);
    assert_eq!(balance(&mut ctx, destination), wei(COIN));

    let (refused, mut ctx) = registered(&pegin, Iris300, COIN / 2);
    let (refund, to) = refund(&mut ctx);
    assert_eq!(to, p2sh(&hash160(&them)));
    assert_eq!(
        refused.logs,
        vec![rejected_pegin(&pegin, 1), release_requested(&refund, COIN)]
    );
    assert!(refused.processed);
}

/// rskj `parse_fromPeginInstructions_withoutBtcRefundAddress`: instructions
/// without a refund address leave the sender's in place, so a peg-in the
/// locking cap turns down goes back to its sender, while one taken credits
/// the instructions' destination.
#[test]
fn instructions_without_a_refund_address_leave_the_senders() {
    let (sender, them) = (public_key(&[0x51]), public_key(&[0x52]));
    let destination = rsk_address_of(&them);
    let pegin = pegin(p2pkh_spend(&sender), Some(rskt(1, destination, None)));

    let (taken, mut ctx) = registered(&pegin, Iris300, NO_CAP);
    assert_eq!(taken.logs, vec![pegin_btc(destination, &pegin, COIN, 1)]);
    assert_eq!(balance(&mut ctx, destination), wei(COIN));
    assert_eq!(balance(&mut ctx, rsk_address_of(&sender)), U256::ZERO);

    let (refused, mut ctx) = registered(&pegin, Iris300, COIN / 2);
    let (refund, to) = refund(&mut ctx);
    assert_eq!(to, p2pkh(&hash160(&sender)));
    assert_eq!(
        refused.logs,
        vec![rejected_pegin(&pegin, 1), release_requested(&refund, COIN)]
    );
}

/// rskj `parse_fromPeginInstructions_withoutBtcLockSender_withoutBtcRefundAddress`:
/// with no sender and no refund address in the instructions there is none:
/// a peg-in the locking cap turns down can't be refunded, which from RSKIP181
/// is logged as PEGIN_V1_REFUND_ADDRESS_NOT_SET (2), and it is marked
/// processed.
#[test]
fn with_no_sender_and_no_refund_address_in_the_instructions_there_is_none() {
    let them = public_key(&[0x52]);
    let destination = rsk_address_of(&them);
    let pegin = pegin(
        p2wpkh_spend(&public_key(&[0x51])),
        Some(rskt(1, destination, None)),
    );

    let (taken, mut ctx) = registered(&pegin, Iris300, NO_CAP);
    assert_eq!(taken.logs, vec![pegin_btc(destination, &pegin, COIN, 1)]);
    assert_eq!(balance(&mut ctx, destination), wei(COIN));

    let (refused, mut ctx) = registered(&pegin, Iris300, COIN / 2);
    assert_eq!(
        refused,
        Registered {
            returned: true,
            processed: true,
            logs: vec![rejected_pegin(&pegin, 1), unrefundable_pegin(&pegin, 2)],
        }
    );
    assert!(waiting_pegouts(&mut ctx).is_empty());
    assert_eq!(balance(&mut ctx, destination), U256::ZERO);
}

/// rskj `parse_fromPeginInstructions_invalidProtocolVersion`: instructions of
/// a version other than 1 throw (`PeginInformation.parseFromPeginInstructions`,
/// and before it `PeginInstructionsProvider.buildPeginInstructions`). A
/// peg-in of version 0 is then an invalid payload: from RSKIP181 logged as
/// PEGIN_V1_INVALID_PAYLOAD (4), refunded to its sender, whose address was
/// read before the throw, and marked processed, before RSKIP379 and after it.
#[test]
fn instructions_of_a_version_other_than_1_are_an_invalid_payload() {
    let (sender, them) = (public_key(&[0x51]), public_key(&[0x52]));
    let destination = rsk_address_of(&them);
    let pegin = pegin(
        p2pkh_spend(&sender),
        Some(rskt(0, destination, Some(Refund::P2pkh(hash160(&them))))),
    );
    for last in [Iris300, Arrowhead600] {
        let (refused, mut ctx) = registered(&pegin, last, NO_CAP);
        let (refund, to) = refund(&mut ctx);
        assert_eq!(to, p2pkh(&hash160(&sender)), "{last:?}");
        assert_eq!(
            refused,
            Registered {
                returned: true,
                processed: true,
                logs: vec![rejected_pegin(&pegin, 4), release_requested(&refund, COIN)],
            },
            "{last:?}"
        );
        assert_eq!(balance(&mut ctx, destination), U256::ZERO);
        assert_eq!(balance(&mut ctx, rsk_address_of(&sender)), U256::ZERO);
    }
}

/// rskj `parse_withoutBtcLockSender_withoutPeginInstructions`: before RSKIP379
/// a peg-in with neither a sender rskj can name nor instructions throws.
/// From RSKIP170 (with RSKIP181) that logs PEGIN_V1_INVALID_PAYLOAD (4), then,
/// with nowhere to refund to and the version still 0,
/// LEGACY_PEGIN_UNDETERMINED_SENDER (1), and marks it processed; before
/// RSKIP170 nothing is kept.
#[test]
fn before_rskip379_a_peg_in_with_neither_a_sender_nor_instructions_is_refused() {
    let pegin = pegin(p2wpkh_spend(&public_key(&[0x51])), None);
    let (refused, mut ctx) = registered(&pegin, Iris300, NO_CAP);
    assert_eq!(
        refused,
        Registered {
            returned: true,
            processed: true,
            logs: vec![rejected_pegin(&pegin, 4), unrefundable_pegin(&pegin, 1)],
        }
    );
    assert!(waiting_pegouts(&mut ctx).is_empty());

    let (ignored, _) = registered(&pegin, Papyrus200, NO_CAP);
    assert_eq!(
        ignored,
        Registered {
            returned: true,
            processed: false,
            logs: vec![],
        }
    );
}

/// rskj `parse_withBtcLockSender_withPeginInstructions_preIris`: before
/// RSKIP170 the instructions aren't read: the peg-in is version 0, credited
/// to its sender's RSK address and, from RSKIP146, logged as `lock_btc` with
/// its sender's address; the instructions' destination gets nothing.
#[test]
fn before_rskip170_the_instructions_are_not_read() {
    let (sender, them) = (public_key(&[0x51]), public_key(&[0x52]));
    let destination = rsk_address_of(&them);
    let instructions = rskt(1, destination, Some(Refund::P2pkh(hash160(&them))));
    let pegin = pegin(p2pkh_spend(&sender), Some(instructions));
    let config = BridgeConstants::rskj_regtest();
    let cfg = up_to(Papyrus200, RSK_REGTEST_CHAIN_ID);
    assert!(!cfg.has_rskip170(10) && cfg.has_rskip146(10));
    let mut ctx = bridge_holding(&pegin, HEIGHT, 10, &config, &cfg);
    whitelist_once(&mut ctx, hash160(&sender), COIN);

    let taken = register(&mut ctx, &pegin, i64::from(HEIGHT), &config, &cfg);
    let to = rsk_address_of(&sender);
    assert_eq!(
        taken,
        Registered {
            returned: true,
            processed: true,
            logs: vec![lock_btc(to, &pegin, &base58(111, &hash160(&sender)), COIN)],
        }
    );
    assert_eq!(balance(&mut ctx, to), wei(COIN));
    assert_eq!(balance(&mut ctx, destination), U256::ZERO);
}

/// rskj `parse_withoutBtcLockSender_withPeginInstructions_preIris`: before
/// RSKIP170 a peg-in from no sender rskj can name throws, its instructions
/// unread: nothing of it is kept, and it isn't marked processed.
#[test]
fn before_rskip170_instructions_do_not_stand_in_for_a_sender() {
    let them = public_key(&[0x52]);
    let destination = rsk_address_of(&them);
    let instructions = rskt(1, destination, Some(Refund::P2pkh(hash160(&them))));
    let pegin = pegin(p2wpkh_spend(&public_key(&[0x51])), Some(instructions));

    let (ignored, mut ctx) = registered(&pegin, Papyrus200, NO_CAP);
    assert_eq!(
        ignored,
        Registered {
            returned: true,
            processed: false,
            logs: vec![],
        }
    );
    assert_eq!(balance(&mut ctx, destination), U256::ZERO);
}

/// rskj `parse_bech32_withPeginInstructions`, a testnet peg-in from a native
/// segwit (bech32) sender with no instructions: before RSKIP379 its reading
/// throws, an invalid payload with no one to refund (4, then 1), marked
/// processed; from RSKIP379 it doesn't, and the peg-in is a legacy one from
/// an undetermined sender (3, then 1), not marked at arrowhead600, below the
/// pegout index's height and at it. rskj's transaction pays testnet's
/// federation of the day; this one is as it is, paying regtest's.
#[test]
fn a_peg_in_from_a_bech32_sender_is_read_as_rskj_reads_it_on_each_side_of_rskip379() {
    let config = BridgeConstants::rskj_regtest();
    let sender = public_key(&[0x53]);
    let mut pegin = tx(
        vec![p2wpkh_spend(&sender)],
        &[
            (550_000, genesis_federation_output(&config)),
            (97_803_500, p2wpkh(&hash160(&sender))),
        ],
    );
    pegin.version = bitcoin::transaction::Version::TWO;

    let (refused, _) = registered(&pegin, Fingerroot500, NO_CAP);
    assert_eq!(
        refused,
        Registered {
            returned: true,
            processed: true,
            logs: vec![rejected_pegin(&pegin, 4), unrefundable_pegin(&pegin, 1)],
        }
    );

    let index = config.btc_height_when_pegout_tx_index_activates
        + config.pegout_tx_index_grace_period_in_btc_blocks;
    for height in [HEIGHT, index] {
        let (refused, mut ctx) = registered_at(&pegin, Arrowhead600, height, NO_CAP);
        assert_eq!(
            refused,
            Registered {
                returned: true,
                processed: false,
                logs: vec![rejected_pegin(&pegin, 3), unrefundable_pegin(&pegin, 1)],
            },
            "at {height}"
        );
        assert_eq!(balance(&mut ctx, rsk_address_of(&sender)), U256::ZERO);
    }
}
