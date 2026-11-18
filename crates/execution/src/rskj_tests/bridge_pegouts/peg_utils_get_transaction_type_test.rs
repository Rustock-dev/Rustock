//! rskj's `PegUtilsGetTransactionTypeTest`: how `PegUtils.getTransactionType`
//! classifies a transaction, by the legacy rules before RSKIP379 and below
//! the pegout index's start, by the index from it.
//!
//! rskj's tests ask the classifier; here the class is read in what
//! `registerBtcTransaction` does, in the runs of the ports of
//! `BridgeSupportRegisterBtcTransactionTest`: a peg-in is taken or turned
//! down, a peg-out or a migration is processed with no event and its outputs
//! to the active federation registered. Most of its listed peg-out and
//! migration cases are the transactions those ports register; the rest are
//! here. As there, the federations are standard multisig ones.

use super::support::*;

/// rskj's `many_outputs_and_inputs_pegout` and
/// `many_outputs_and_one_input_pegout`: a spend of the active federation, by
/// fifty inputs or by one, to forty users and no change, is a peg-out:
/// processed, and nothing registered.
#[test]
fn a_spend_of_the_active_federation_to_forty_users_is_a_peg_out() {
    let outputs = forty_outputs(|n| user(n + 1));
    for inputs in [50, 1] {
        for run in RUNS {
            let outcome = register_spend(run, false, &active(), inputs, &outputs);
            assert_moved(&outcome, run, 0);
        }
    }
}

/// rskj's `anyAddressToAnyAddress_pegin_before_RSKIP379`: before RSKIP379 a
/// transaction that pays the minimum to an empty script, from an input with
/// none either, is a peg-in by the legacy rules, none of its outputs to a
/// federation being below the minimum. It is registered as one: its sender
/// can't be named, so it can't be refunded, and it is processed (the legacy
/// path's `rejected_pegin` PEGIN_V1_INVALID_PAYLOAD and `unrefundable_pegin`
/// LEGACY_PEGIN_UNDETERMINED_SENDER). By the index
/// (`unknown_anyAddressToAnyAddress_unknown_after_RSIP379_and_using_pegout_tx_index`)
/// it is no peg-in, and is left alone.
#[test]
fn before_rskip379_a_transaction_between_any_addresses_is_a_peg_in() {
    let tx = btc_transaction(vec![input(1, vec![])], &[(minimum_pegin(), vec![])]);
    let outcome = BEFORE_RSKIP379
        .bridge(false)
        .register(&tx, BEFORE_RSKIP379.height);
    assert!(outcome.returned);
    assert_eq!(
        outcome.events,
        vec![Event::Rejected(4), Event::Unrefundable(1)]
    );
    assert_eq!(outcome.processed_at, Some(BEFORE_RSKIP379.block));

    let outcome = RSKIP379_INDEXED
        .bridge(false)
        .register(&tx, RSKIP379_INDEXED.height);
    assert!(outcome.returned);
    assert!(outcome.events.is_empty());
    assert_eq!(outcome.processed_at, None);
}
