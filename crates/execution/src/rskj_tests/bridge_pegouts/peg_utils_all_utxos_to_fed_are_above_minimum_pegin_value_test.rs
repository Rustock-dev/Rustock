//! rskj's `PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest`: whether every
//! output a transaction sends the live federations is worth the minimum
//! peg-in. Before RSKIP379 rskj answers it as
//! `PegUtilsLegacy.isAnyUTXOAmountBelowMinimum` does, true where no output
//! goes to a federation; from RSKIP379 it answers false there.
//!
//! rskj's tests ask the function. Here its answer is read in what
//! `registerBtcTransaction` does with the transaction, sent by a P2PKH user.
//! Before RSKIP379 the answer is the legacy rules' minimum (`isValidPegInTx`,
//! from RSKIP293): with it the transaction is a peg-in, taken for what it
//! sends the federations, nothing where it sends them nothing; without it the
//! transaction is no peg-in and is left alone. From RSKIP379 `evaluatePegin`
//! asks it of every peg-in: with it the peg-in is taken, without it the peg-in
//! is turned down, INVALID_AMOUNT. A transaction that pays a federation is
//! asked by the pegout index; one that pays none only below the index's
//! start, where the legacy rules first take it for a peg-in.

use super::support::*;

/// Where an output goes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum To {
    Active,
    Retiring,
    /// A random user's address.
    Elsewhere,
}

fn script(to: To) -> Vec<u8> {
    match to {
        To::Active => address_of(&active()),
        To::Retiring => address_of(&retiring()),
        To::Elsewhere => user(0xad),
    }
}

/// What registering a user's transaction paying `outputs` leaves in `run`,
/// the retiring federation live where an output goes to it.
fn registered(run: Run, outputs: &[(u64, To)]) -> Outcome {
    let paid: Vec<(u64, Vec<u8>)> = outputs
        .iter()
        .map(|(value, to)| (*value, script(*to)))
        .collect();
    let with_retiring = outputs.iter().any(|(_, to)| *to == To::Retiring);
    run.bridge(with_retiring)
        .register(&from_a_user(&paid), run.height)
}

/// The peg-in of `outputs` taken: `pegin_btc` of what they send the
/// federations, the transaction processed, each output to a federation its
/// UTXO.
fn assert_taken(outcome: &Outcome, run: Run, outputs: &[(u64, To)]) {
    let sent: u64 = outputs
        .iter()
        .filter(|(_, to)| *to != To::Elsewhere)
        .map(|(value, _)| value)
        .sum();
    let count = |which: To| outputs.iter().filter(|(_, to)| *to == which).count();
    assert_eq!(outcome.events, vec![Event::PeginBtc(sent, 0)]);
    assert_eq!(outcome.processed_at, Some(run.block));
    assert_eq!(outcome.active_utxos.len(), count(To::Active));
    assert_eq!(outcome.retiring_utxos.len(), count(To::Retiring));
}

/// rskj's answer before RSKIP379 for `outputs`, held: with it the
/// transaction is taken for a peg-in; without it it is left alone.
fn holds_before_rskip379(outputs: &[(u64, To)], answer: bool) {
    let outcome = registered(BEFORE_RSKIP379, outputs);
    assert!(outcome.returned);
    if answer {
        assert_taken(&outcome, BEFORE_RSKIP379, outputs);
    } else {
        assert!(outcome.events.is_empty());
        assert_eq!(outcome.processed_at, None);
        assert!(outcome.active_utxos.is_empty() && outcome.retiring_utxos.is_empty());
    }
}

/// rskj's answer from RSKIP379 for `outputs`, held: with it the peg-in is
/// taken; without it it is turned down, `rejected_pegin` INVALID_AMOUNT (5)
/// and `unrefundable_pegin` INVALID_AMOUNT (3), and at arrowhead600, before
/// RSKIP459, not marked processed.
fn holds_from_rskip379(outputs: &[(u64, To)], answer: bool) {
    let pays_a_federation = outputs.iter().any(|(_, to)| *to != To::Elsewhere);
    let run = if pays_a_federation {
        RSKIP379_INDEXED
    } else {
        RSKIP379_BELOW_INDEX
    };
    let outcome = registered(run, outputs);
    assert!(outcome.returned);
    if answer {
        assert_taken(&outcome, run, outputs);
    } else {
        assert_eq!(
            outcome.events,
            vec![Event::Rejected(5), Event::Unrefundable(3)]
        );
        assert_eq!(outcome.processed_at, None);
        assert!(outcome.active_utxos.is_empty() && outcome.retiring_utxos.is_empty());
    }
}

fn min() -> u64 {
    minimum_pegin()
}

/// rskj's `..._preRSKIP379WithManyOutputsBelowMinimumButNoneForFed_true` and
/// `..._postRSKIP379WithManyOutputsBelowMinimumButNoneForFed_false`.
#[test]
fn outputs_below_the_minimum_to_no_federation_pass_before_rskip379_only() {
    let outputs = [(min() - 1, To::Elsewhere); 3];
    holds_before_rskip379(&outputs, true);
    holds_from_rskip379(&outputs, false);
}

/// rskj's `..._preRSKIP379WithManyOutputsEqualAndAboveMinimumButNoneForFed_true`
/// and `..._postRSKIP379WithManyOutputsEqualAndAboveMinimumButNoneForFed_false`.
#[test]
fn outputs_of_the_minimum_to_no_federation_pass_before_rskip379_only() {
    let outputs = [
        (min(), To::Elsewhere),
        (min() + 1, To::Elsewhere),
        (min() + 1, To::Elsewhere),
    ];
    holds_before_rskip379(&outputs, true);
    holds_from_rskip379(&outputs, false);
}

/// rskj's `..._preRSKIP379WithManyOutputsAboveMinimumForFedAndManyOutputsBelowMinimumForRandomAddresses_true`
/// and its `post` counterpart: outputs elsewhere below the minimum don't count.
#[test]
fn outputs_to_the_federation_of_the_minimum_pass_beside_smaller_ones_elsewhere() {
    let elsewhere = [(min() - 1, To::Elsewhere); 3];
    let before = [elsewhere.as_slice(), &[(min() + 1, To::Active); 3]].concat();
    holds_before_rskip379(&before, true);
    let after = [
        elsewhere.as_slice(),
        &[
            (min(), To::Active),
            (min() + 1, To::Active),
            (min() + 1, To::Active),
        ],
    ]
    .concat();
    holds_from_rskip379(&after, true);
}

/// rskj's `..._preRSKIP379WithManyOutputsForFedButEachBelowMinimumAndSumAboveMinimum_false`
/// and its `post` counterpart: each output must be worth the minimum, not
/// their sum.
#[test]
fn outputs_to_the_federation_each_below_the_minimum_fail_whatever_their_sum() {
    let outputs = [(min() - 1, To::Active); 3];
    holds_before_rskip379(&outputs, false);
    holds_from_rskip379(&outputs, false);
}

/// rskj's `..._preRSKIP379WithManyOutputsForFedAboveMinimumAndOneBelowMinimum_false`
/// and its `post` counterpart.
#[test]
fn one_output_to_the_federation_below_the_minimum_fails_them_all() {
    let outputs = [
        (min(), To::Active),
        (min() + 1, To::Active),
        (min() - 1, To::Active),
    ];
    holds_before_rskip379(&outputs, false);
    holds_from_rskip379(&outputs, false);
}

/// rskj's `..._preRSKIP379WithManyOutputsForFedAllEqualOrAboveMinimum_true`
/// and its `post` counterpart.
#[test]
fn outputs_to_the_federation_all_of_the_minimum_or_more_pass() {
    let outputs = [
        (min(), To::Active),
        (min() + 1, To::Active),
        (min() + 1, To::Active),
    ];
    holds_before_rskip379(&outputs, true);
    holds_from_rskip379(&outputs, true);
}

/// rskj's `..._preRSKIP379UtxosToRetiringFedAboveMinimumAndToActiveFedBelowMinimum_false`
/// and its `post` counterpart: the minimum to the retiring federation
/// doesn't make up for an output below it to the active one.
#[test]
fn an_output_to_the_active_federation_below_the_minimum_fails_those_to_the_retiring_one() {
    let outputs = [
        (min() - 1, To::Active),
        (min(), To::Active),
        (min() + 1, To::Retiring),
        (min() + 1, To::Retiring),
    ];
    holds_before_rskip379(&outputs, false);
    holds_from_rskip379(&outputs, false);
}

/// rskj's `..._preRSKIP379UtxosToRetiringAndActiveFedEqualToAndAboveMinimum_true`
/// and its `post` counterpart.
#[test]
fn outputs_of_the_minimum_or_more_to_both_federations_pass() {
    let outputs = [
        (min(), To::Active),
        (min() + 1, To::Active),
        (min(), To::Retiring),
        (min() + 1, To::Retiring),
    ];
    holds_before_rskip379(&outputs, true);
    holds_from_rskip379(&outputs, true);
}

/// rskj's `..._preRSKIP379Exactly1UtxoToFedWithMinimumValue_true` and
/// `..._postRSKIP379Exactly1UtxoToFedWithMinimumValue_true`, whose `post`
/// test runs under fingerroot500 again; here it runs from RSKIP379.
#[test]
fn one_output_of_exactly_the_minimum_passes() {
    let outputs = [(min(), To::Active)];
    holds_before_rskip379(&outputs, true);
    holds_from_rskip379(&outputs, true);
}

/// rskj's `..._preRSKIP379UtxosBelowMinimumToBothRetiringAndActiveFed_false`
/// and its `post` counterpart.
#[test]
fn outputs_below_the_minimum_to_both_federations_fail() {
    let outputs = [
        (min() - 1, To::Active),
        (min() - 1, To::Active),
        (min() - 1, To::Retiring),
        (min() - 1, To::Retiring),
    ];
    holds_before_rskip379(&outputs, false);
    holds_from_rskip379(&outputs, false);
}
