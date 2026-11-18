//! rskj's `BridgeSupportRegisterBtcTransactionTest`: how `registerBtcTransaction`
//! takes a peg-in, a peg-out and a migration, by the legacy rules before
//! RSKIP379 and below the pegout index's start, by the index from RSKIP379 for
//! a Bitcoin block at its start or above, and what it does with a peg-in it
//! turns down, which RSKIP459 marks processed and RSKIP551 no longer does.
//!
//! rskj's tests mock the block store, the storage provider and the event
//! logger, and run under `fingerroot500`, `arrowhead600` and `all`
//! activations. Here mainnet's own Bridge runs at a block of each of those
//! upgrades, and of lovell700, where a refused peg-in is marked processed. Its
//! Bitcoin store holds the transaction's block and the confirmations above it,
//! the pegout index holds what rskj's tests mock it to hold, and the ports read
//! what the call leaves: whether the transaction was marked processed, the
//! events, the UTXOs of each federation, and the refunds.
//!
//! The federations here are standard multisig ones, where rskj's tests use
//! P2SH ERP federations: these rules read a federation's address and its
//! standard redeem script, which for a standard federation are its own.

use super::support::*;

/// A peg-in taken: `pegin_btc` of `amount` and protocol 0, the transaction
/// processed, and `utxos` UTXOs to the active federation.
fn assert_taken(outcome: &Outcome, run: Run, amount: u64, utxos: usize) {
    assert!(outcome.returned, "{run:?}");
    assert_eq!(outcome.events, vec![Event::PeginBtc(amount, 0)], "{run:?}");
    assert_eq!(outcome.processed_at, Some(run.block), "{run:?}");
    assert_eq!(outcome.active_utxos.len(), utxos, "{run:?}");
    assert!(outcome.retiring_utxos.is_empty(), "{run:?}");
}

/// rskj's `assertInvalidPeginIsRejectedWithInvalidAmountReason` by the index
/// and `assertInvalidPeginIsIgnored` by the legacy rules: by the index a
/// peg-in that sends the federation too little is turned down,
/// `rejected_pegin` INVALID_AMOUNT (5) and `unrefundable_pegin`
/// INVALID_AMOUNT (3), and marked processed between RSKIP459 and RSKIP551
/// (`assertInvalidPeginMarkedAsProcessed`); by the legacy rules it is no
/// peg-in, and is left alone.
fn assert_too_little(outcome: &Outcome, run: Run) {
    assert!(outcome.returned, "{run:?}");
    if run.indexed() {
        assert_eq!(
            outcome.events,
            vec![Event::Rejected(5), Event::Unrefundable(3)],
            "{run:?}"
        );
        assert_eq!(
            outcome.processed_at,
            run.marks_refused().then_some(run.block),
            "{run:?}"
        );
    } else {
        assert!(outcome.events.is_empty(), "{run:?}");
        assert_eq!(outcome.processed_at, None, "{run:?}");
    }
    assert!(outcome.active_utxos.is_empty(), "{run:?}");
    assert!(outcome.retiring_utxos.is_empty(), "{run:?}");
    assert_eq!(outcome.refunds, 0, "{run:?}");
}

/// A peg-in rskj can't refund for want of a sender: before RSKIP379
/// (`assertLegacyUndeterminedSenderPeginIsRejectedAsPeginV1InvalidPayloadBeforeRSKIP379`)
/// `rejected_pegin` PEGIN_V1_INVALID_PAYLOAD (4) whatever the reason, and
/// processed; from it `rejected_pegin` with `reason`, and processed between
/// RSKIP459 and RSKIP551. Either way `unrefundable_pegin`
/// LEGACY_PEGIN_UNDETERMINED_SENDER (1), no UTXO and no refund.
fn assert_not_refundable(outcome: &Outcome, run: Run, reason: u64) {
    assert!(outcome.returned, "{run:?}");
    let (rejected, processed) = if run.rskip379() {
        (reason, run.marks_refused().then_some(run.block))
    } else {
        (4, Some(run.block))
    };
    assert_eq!(
        outcome.events,
        vec![Event::Rejected(rejected), Event::Unrefundable(1)],
        "{run:?}"
    );
    assert_eq!(outcome.processed_at, processed, "{run:?}");
    assert!(outcome.active_utxos.is_empty(), "{run:?}");
    assert!(outcome.retiring_utxos.is_empty(), "{run:?}");
    assert_eq!(outcome.refunds, 0, "{run:?}");
}

/// rskj's `assertPeginIsRejectedAndRefunded`: `rejected_pegin` with
/// `reason`, the refund of `amount` requested and, from RSKIP428, the
/// refund's `pegout_transaction_created`; the transaction processed, no UTXO
/// kept, and the refund waiting for confirmations.
fn assert_refunded(outcome: &Outcome, run: Run, reason: u64, amount: u64) {
    assert!(outcome.returned, "{run:?}");
    let mut events = vec![Event::Rejected(reason), Event::ReleaseRequested(amount)];
    if RskHardforkConfig::mainnet().has_rskip428(run.block) {
        events.push(Event::PegoutTransactionCreated);
    }
    assert_eq!(outcome.events, events, "{run:?}");
    assert_eq!(outcome.processed_at, Some(run.block), "{run:?}");
    assert!(outcome.active_utxos.is_empty(), "{run:?}");
    assert!(outcome.retiring_utxos.is_empty(), "{run:?}");
    assert_eq!(outcome.refunds, 1, "{run:?}");
}

/// rskj's `pegin_legacy_to_active_fed`: a bitcoin to the active federation
/// from a P2PKH sender is taken in every run, with a retiring federation or
/// without.
#[test]
fn a_legacy_peg_in_to_the_active_federation_is_taken() {
    let tx = from_a_user(&[(BTC, address_of(&active()))]);
    for run in RUNS {
        for with_retiring in [false, true] {
            let outcome = run.bridge(with_retiring).register(&tx, run.height);
            assert_taken(&outcome, run, BTC, 1);
        }
    }
}

/// rskj's `pegin_multiple_outputs_to_active_fed`: ten outputs of the minimum
/// to the active federation are one peg-in of ten minimums, and ten UTXOs.
#[test]
fn a_peg_in_of_many_outputs_to_the_active_federation_is_taken_whole() {
    let outputs = vec![(minimum_pegin(), address_of(&active())); 10];
    let tx = from_a_user(&outputs);
    for run in RUNS {
        for with_retiring in [false, true] {
            let outcome = run.bridge(with_retiring).register(&tx, run.height);
            assert_taken(&outcome, run, 10 * minimum_pegin(), 10);
        }
    }
}

/// rskj's `pegin_to_active_fed_with_bech32_output`: an output to a bech32
/// address beside the one to the federation is not counted.
#[test]
fn a_peg_in_with_an_output_to_a_bech32_address_counts_the_federations_alone() {
    let tx = from_a_user(&[(BTC, address_of(&active())), (BTC, p2wpkh(&[0xbe; 20]))]);
    for run in RUNS {
        for with_retiring in [false, true] {
            let outcome = run.bridge(with_retiring).register(&tx, run.height);
            assert_taken(&outcome, run, BTC, 1);
        }
    }
}

/// rskj's `pegin_to_active_fed_equal_to_minimum_with_other_random_outputs_below_minimum`:
/// the minimum to the federation is a peg-in, whatever goes elsewhere below
/// the minimum.
#[test]
fn a_peg_in_of_the_minimum_is_taken_beside_smaller_outputs_elsewhere() {
    let below = minimum_pegin() - 1;
    let tx = from_a_user(&[
        (minimum_pegin(), address_of(&active())),
        (below, p2wpkh(&[0xbe; 20])),
        (below, user(0x0a)),
    ]);
    for run in RUNS {
        for with_retiring in [false, true] {
            let outcome = run.bridge(with_retiring).register(&tx, run.height);
            assert_taken(&outcome, run, minimum_pegin(), 1);
        }
    }
}

/// rskj's `pegin_to_active_fed_below_minimum`,
/// `pegin_to_active_fed_below_and_above_minimum` and
/// `pegin_multiple_outputs_to_active_fed_sum_amount_equal_to_minimum_pegin`:
/// an output to the federation below the minimum makes the transaction no
/// peg-in by the legacy rules, and a peg-in of an invalid amount by the index,
/// even where the outputs together come to the minimum.
#[test]
fn a_peg_in_with_an_output_below_the_minimum_is_refused() {
    let below = minimum_pegin() - 1;
    let to_the_federation = |value: u64| (value, address_of(&active()));
    let txs = [
        from_a_user(&[to_the_federation(below)]),
        from_a_user(&[to_the_federation(minimum_pegin()), to_the_federation(below)]),
        from_a_user(&vec![to_the_federation(minimum_pegin() / 10); 10]),
    ];
    for tx in &txs {
        for run in RUNS {
            for with_retiring in [false, true] {
                let outcome = run.bridge(with_retiring).register(tx, run.height);
                assert_too_little(&outcome, run);
            }
        }
    }
}

/// rskj's `pegin_to_active_fed_below_minimum_and_retiring_above_minimum`:
/// the minimum to the retiring federation doesn't make up for too little to
/// the active one.
#[test]
fn a_peg_in_below_the_minimum_to_the_active_federation_is_refused_whatever_the_retiring_one_gets() {
    let tx = from_a_user(&[
        (minimum_pegin() - 1, address_of(&active())),
        (minimum_pegin(), address_of(&retiring())),
    ]);
    for run in RUNS {
        let outcome = run.bridge(true).register(&tx, run.height);
        assert_too_little(&outcome, run);
    }
}

/// rskj's `pegin_v1_to_active_fed_with_invalid_payload_and_unknown_sender_cannot_be_processed`:
/// a peg-in v1 whose payload is empty, from a sender rskj can't name, can be
/// neither registered nor refunded: PEGIN_V1_INVALID_PAYLOAD. rskj's test
/// mocks the sender out of a multisig spend; here the input carries no
/// scriptSig, which no sender rskj knows reads.
#[test]
fn a_peg_in_v1_with_an_empty_payload_and_no_sender_is_not_refundable() {
    let empty_payload = [&[0x6a, 5][..], b"RSKT", &[1]].concat();
    let tx = btc_transaction(
        vec![input(1, vec![])],
        &[(BTC, address_of(&active())), (0, empty_payload)],
    );
    for run in RUNS {
        let outcome = run.bridge(false).register(&tx, run.height);
        assert_not_refundable(&outcome, run, 4);
    }
}

/// rskj's `pegin_to_retiring_fed_cannot_be_processed`: a peg-in to the
/// retiring federation from a sender rskj can't name is not refundable:
/// LEGACY_PEGIN_UNDETERMINED_SENDER (3) from RSKIP379. As above, the
/// sender is one with no scriptSig, where rskj's test mocks it.
#[test]
fn a_peg_in_to_the_retiring_federation_from_an_unknown_sender_is_not_refundable() {
    let tx = btc_transaction(vec![input(1, vec![])], &[(BTC, address_of(&retiring()))]);
    for run in RUNS {
        let outcome = run.bridge(true).register(&tx, run.height);
        assert_not_refundable(&outcome, run, 3);
    }
}

/// rskj's `pegin_legacy_from_segwit_to_active_fed_cannot_be_processed`: a
/// legacy peg-in from a segwit sender, here a P2WPKH spend, its signature and
/// key in the witness, is not refundable: rskj can't name the sender.
#[test]
fn a_legacy_peg_in_from_a_segwit_sender_is_not_refundable() {
    let mut spend = input(1, vec![]);
    spend.witness = bitcoin::Witness::from_slice(&[vec![0x30; 71], public_key(&[0x77]).to_vec()]);
    let tx = btc_transaction(vec![spend], &[(BTC, address_of(&active()))]);
    for run in RUNS {
        let outcome = run.bridge(false).register(&tx, run.height);
        assert_not_refundable(&outcome, run, 3);
    }
}

/// rskj's `pegout_no_change_output`: a peg-out paying a user alone is
/// processed, and nothing is registered.
#[test]
fn a_pegout_with_no_change_is_processed_and_registers_nothing() {
    for run in RUNS {
        for with_retiring in [false, true] {
            let outcome = register_spend(run, with_retiring, &active(), 1, &[(BTC, user(1))]);
            assert_moved(&outcome, run, 0);
        }
    }
}

/// rskj's `pegout_many_outputs_and_inputs_with_change_output` and
/// `pegout_many_outputs_and_one_input`: a peg-out of fifty inputs, or one, to
/// forty users registers its change to the federation, a quarter of the
/// minimum peg-out, whatever its value.
#[test]
fn a_pegout_to_many_users_registers_its_change() {
    let mut outputs = forty_outputs(|n| user(n + 1));
    outputs.push((minimum_pegout() / 4, address_of(&active())));
    for inputs in [50, 1] {
        for run in RUNS {
            for with_retiring in [false, true] {
                let outcome = register_spend(run, with_retiring, &active(), inputs, &outputs);
                assert_moved(&outcome, run, 1);
                assert_eq!(outcome.active_utxos, vec![minimum_pegout() / 4]);
            }
        }
    }
}

/// rskj's `pegout_one_output_and_many_input`: fifty inputs to one user is a
/// peg-out, and nothing is registered.
#[test]
fn a_pegout_of_many_inputs_to_one_user_registers_nothing() {
    for run in RUNS {
        for with_retiring in [false, true] {
            let outputs = [(minimum_pegout(), user(1))];
            let outcome = register_spend(run, with_retiring, &active(), 50, &outputs);
            assert_moved(&outcome, run, 0);
        }
    }
}

/// rskj's `migration_ok`, `migration_one_outputs_and_many_input`,
/// `migration_many_outputs_and_inputs` and
/// `migration_many_outputs_and_one_input`: a spend of the retiring federation
/// to the active one is processed, and each of its outputs registered to the
/// active federation, whatever their values.
#[test]
fn a_migration_registers_every_output_to_the_active_federation() {
    let to_active = |n: u8| (BTC, address_of(&active()), n);
    let cases: Vec<(u8, Vec<(u64, Vec<u8>)>)> = vec![
        (1, vec![(to_active(0).0, to_active(0).1)]),
        (50, vec![(minimum_pegout(), address_of(&active()))]),
        (50, forty_outputs(|_| address_of(&active()))),
        (1, forty_outputs(|_| address_of(&active()))),
    ];
    for (inputs, outputs) in &cases {
        for run in RUNS {
            let outcome = register_spend(run, true, &retiring(), *inputs, outputs);
            assert_moved(&outcome, run, outputs.len());
        }
    }
}

/// rskj's `flyover_segwit_as_migration_utxo` and
/// `flyover_segwit_as_migration_utxo_with_many_outputs_and_inputs`, and
/// `PegUtilsGetTransactionTypeTest`'s `flyover_segwit_migration`,
/// `flyover_segwit_with_other_inputs_migration` and
/// `flyover_segwit_with_many_inputs_and_outputs_migration`: a migration whose
/// first input, spending a Flyover output, carries the retiring federation's
/// redeem script in its witness is a migration, read from the witness, and is
/// proven by its block's witness root. With nine more inputs spending the
/// retiring federation or none, to one output or to ten, its outputs to the
/// active federation are all registered.
#[test]
fn a_segwit_migration_spending_a_flyover_output_registers_its_outputs() {
    use bitcoin::hashes::Hash;
    let redeem = standard_redeem(&retiring());
    let flyover = [
        &[0x20][..],
        &[0xd1; 32],
        &[0x75],
        &standard_redeem(&active()),
    ]
    .concat();
    let funding = from_a_user(&[
        (minimum_pegin(), p2sh(&hash160(&flyover))),
        (minimum_pegin(), p2wpkh(&[0xbe; 20])),
    ]);
    // rskj `createBaseWitnessThatSpendsFromErpRedeemScript`: OP_0, a push for
    // each signature, one for OP_NOTIF, and the redeem script.
    let threshold = usize::from(redeem[0] - 0x50);
    let mut witness: Vec<Vec<u8>> = vec![vec![]; threshold + 2];
    witness.push(redeem.clone());
    // Each case: the inputs beside the first, the first output's value, and
    // the outputs of a bitcoin beside it.
    for (others, first_output, more_outputs) in [
        (0u8, 10 * BTC, 0u8),
        (9, 10 * BTC, 9),
        (0, BTC, 0),
        (9, 10 * BTC, 0),
        (9, BTC, 9),
    ] {
        let mut first = input(0, multisig_spend(&redeem));
        first.previous_output = bitcoin::OutPoint {
            txid: bitcoin::Txid::from_byte_array(txid(&funding)),
            vout: 0,
        };
        first.witness = bitcoin::Witness::from_slice(&witness);
        let inputs = std::iter::once(first)
            .chain((1..=others).map(|tag| input(tag, multisig_spend(&redeem))))
            .collect();
        let outputs: Vec<_> = std::iter::once((first_output, address_of(&active())))
            .chain((0..more_outputs).map(|_| (BTC, address_of(&active()))))
            .collect();
        let tx = btc_transaction(inputs, &outputs);
        for run in RUNS {
            let mut bridge = run.bridge(true);
            if run.rskip379() {
                index_pegout(&mut bridge.ctx, &index_sighash(&tx, &redeem));
            }
            let outcome = bridge.register(&tx, run.height);
            assert_moved(&outcome, run, outputs.len());
        }
    }
}

/// A spend of the last retired federation to the active one, its sighash in
/// the index from RSKIP379, registered in `run` with the last retired
/// federation's P2SH script stored when `stored`.
fn register_retired_spend(run: Run, stored: bool) -> Outcome {
    let redeem = standard_redeem(&retired());
    let tx = btc_transaction(
        vec![input(1, multisig_spend(&redeem))],
        &[(BTC, address_of(&active()))],
    );
    let mut bridge = run.bridge(false);
    if run.rskip379() {
        index_pegout(&mut bridge.ctx, &index_sighash(&tx, &redeem));
    }
    if stored {
        put_named(
            &mut bridge.ctx,
            "lastRetiredFedP2SHScript",
            &rlp_encode_list(&[rlp_encode_element(&address_of(&retired()))]),
        );
    }
    bridge.register(&tx, run.height)
}

/// rskj's `last_retired_fed_to_active_fed`: a spend of the last retired
/// federation to the active one is a migration in every run.
#[test]
fn a_spend_of_the_last_retired_federation_is_a_migration() {
    for run in RUNS {
        assert_moved(&register_retired_spend(run, true), run, 1);
    }
}

/// rskj's `no_last_retired_fed_in_storage_sending_funds_to_active_fed`: with
/// no last retired federation stored, the index still knows the spend for a
/// migration; the legacy rules take it for a peg-in from a multisig sender,
/// and refund it (`assertPeginIsRejectedAndRefunded`):
/// LEGACY_PEGIN_MULTISIG_SENDER (2) and the refund of the bitcoin.
#[test]
fn with_no_last_retired_federation_a_spend_of_it_is_refunded_by_the_legacy_rules() {
    for run in RUNS {
        let outcome = register_retired_spend(run, false);
        if run.indexed() {
            assert_moved(&outcome, run, 1);
        } else {
            assert_refunded(&outcome, run, 2, BTC);
        }
    }
}

/// An unknown federation's members, a multisig sender.
fn unknown() -> Vec<[u8; 33]> {
    keys(20, 3)
}

/// rskj's `pegin_from_multisig_to_retiring_fed_can_be_refunded`: a peg-in to
/// the retiring federation from a multisig sender is refunded:
/// LEGACY_PEGIN_MULTISIG_SENDER (2).
#[test]
fn a_peg_in_from_a_multisig_sender_to_the_retiring_federation_is_refunded() {
    let tx = btc_transaction(
        vec![input(1, multisig_spend(&standard_redeem(&unknown())))],
        &[(BTC, address_of(&retiring()))],
    );
    for run in RUNS {
        assert_refunded(&run.bridge(true).register(&tx, run.height), run, 2, BTC);
    }
}

/// rskj's `invalid_pegin_v1_from_multisig_to_active_fed_can_be_refunded`: a
/// peg-in v1 with an empty payload from a multisig sender, whose address
/// takes a refund, is refunded: PEGIN_V1_INVALID_PAYLOAD (4).
#[test]
fn a_peg_in_v1_with_an_empty_payload_from_a_multisig_sender_is_refunded() {
    let empty_payload = [&[0x6a, 5][..], b"RSKT", &[1]].concat();
    let tx = btc_transaction(
        vec![input(1, multisig_spend(&standard_redeem(&unknown())))],
        &[(BTC, address_of(&active())), (0, empty_payload)],
    );
    for run in RUNS {
        assert_refunded(&run.bridge(false).register(&tx, run.height), run, 4, BTC);
    }
}

/// rskj's `pegout_sighash_no_exists_in_provider` and
/// `migration_sighash_no_exists_in_provider`: by the index, a spend of the
/// active or the retiring federation whose sighash the index doesn't hold is
/// no peg-out or migration but a peg-in from a multisig sender, and is
/// refunded what it sends the federation: LEGACY_PEGIN_MULTISIG_SENDER (2).
#[test]
fn by_the_index_a_federation_spend_it_doesnt_hold_is_refunded_as_a_peg_in() {
    let pegout = btc_transaction(
        vec![input(1, multisig_spend(&standard_redeem(&active())))],
        &[(BTC, user(1)), (BTC, address_of(&active()))],
    );
    let migration = btc_transaction(
        vec![input(1, multisig_spend(&standard_redeem(&retiring())))],
        &[(BTC, address_of(&active()))],
    );
    for tx in [pegout, migration] {
        for run in RUNS.into_iter().filter(|run| run.indexed()) {
            assert_refunded(&run.bridge(true).register(&tx, run.height), run, 2, BTC);
        }
    }
}

/// What a peg-in rskj's `PeginFromUserWithDifferentScriptPubKey` tests build
/// sends: from an input whose last push is the public key `03 77a6..2de9`,
/// which reads as a script, legacy or v1.
fn pegins_with_a_key_that_reads_as_a_script() -> [bitcoin::Transaction; 2] {
    let key = alloy_primitives::hex::decode(
        "0377a6c71c43d9fac4343f87538cd2880cf5ebefd3dd1d9aabdbbf454bca162de9",
    )
    .expect("hex");
    let script_sig = [vec![0x00], push(&key)].concat();
    let v1 = [&[0x6a, 25][..], b"RSKT", &[1], &[0x5e; 20]].concat();
    [
        btc_transaction(
            vec![input(1, script_sig.clone())],
            &[(BTC, address_of(&active()))],
        ),
        btc_transaction(
            vec![input(1, script_sig)],
            &[(0, v1), (BTC, address_of(&active()))],
        ),
    ]
}

/// rskj's `registerBtcTx_legacyPeginWithParseableScriptPubKey_beforeRSKIP305_withSVPOnGoing_shouldThrowISE`
/// and `registerBtcTx_peginV1WithParseableScriptPubKey_beforeRSKIP305_withSVPOnGoing_shouldThrowISE`:
/// while an SVP spend transaction is awaited, every transaction the index
/// reads has its signatures stripped to be compared with it. Before RSKIP305
/// the stripping takes each input's last push for a redeem script, a key
/// that reads as a script too, and throws `IllegalStateException` on one that
/// is no multisig: the call fails, and the peg-in is not processed. From
/// RSKIP305 such an input is no multisig spend and the peg-in is taken
/// (`registerBtcTx_legacyPeginWithParseableScriptPubKey_withSVPOnGoing_shouldRegisterPegIn`).
#[test]
fn before_rskip305_an_awaited_svp_spend_fails_a_peg_in_whose_key_reads_as_a_script() {
    for tx in pegins_with_a_key_that_reads_as_a_script() {
        for (block, taken) in [(MAINNET_LOVELL700, false), (MAINNET_REED800, true)] {
            let mut bridge = Registration::mainnet(block);
            bridge.federation(NEW_FEDERATION, &active());
            put_named(
                &mut bridge.ctx,
                "svpSpendTxHashUnsigned",
                &rlp_encode_element(&[0; 32]),
            );
            let outcome = bridge.register(&tx, INDEX_START);
            assert_eq!(outcome.returned, taken, "block {block}");
            assert_eq!(
                outcome.processed_at,
                taken.then_some(block),
                "block {block}"
            );
            assert_eq!(
                outcome.active_utxos.len(),
                usize::from(taken),
                "block {block}"
            );
        }
    }
}

/// Testnet's peg-in v1 77a135b5...3866a6 as rskj's test holds it.
const TESTNET_REAL_PEGIN: &str = "0200000002d5dfff21c0e1f0b02dcbcda77a56ffd6412b79d2081bc4bc0466cf3f0913b297010000006a47304402201dc13fabe4b29d0a596a84f6f15b3d2d636625a8aa02acb8a4635038322040e10220421d22271ca64b7c02b496dc916f092ea84a74e811f81a328f2f9d888aaee59b01210342e7b7961475e1fcb0e604ed34fc554f14ce7f931373646e98e463ae52a4b564fdffffffd5dfff21c0e1f0b02dcbcda77a56ffd6412b79d2081bc4bc0466cf3f0913b297020000006a47304402207e0add6292ac318db6657aeeb717818136f0f4d6e4efef35302ce72a79731fba0220297c3259eed72cd15fae7e0b9a63d2321e415eb6bd36c023bd179455f236147301210342e7b7961475e1fcb0e604ed34fc554f14ce7f931373646e98e463ae52a4b564fdffffff030000000000000000446a4252534b540147bc43b214c418c101b976f8bbb5101ed262a069011ae302de6607907116810e598b83897b00f764d5c0eff2d4911d78c411bf873de759e10d3b2eeaba04bc0300000000001976a914dfc505d84d81d346563fe9726a76c28e9ea8454588ac20a107000000000017a91405804450706addc3c6df3a400a22397ecaafe2d687cfba3d00";

/// rskj's `registerBtcTx_testnetRealPeginV1_withSVPOnGoing_beforeRSKIP305_shouldThrowISE`:
/// testnet's peg-in v1 77a135b5...3866a6, whose P2PKH inputs' key reads as a
/// script, fails at testnet's lovell700 while an SVP spend transaction is
/// awaited, and is not processed.
#[test]
fn before_rskip305_testnets_real_peg_in_fails_while_an_svp_spend_is_awaited() {
    let raw = alloy_primitives::hex::decode(TESTNET_REAL_PEGIN).expect("hex");
    let tx: bitcoin::Transaction = bitcoin::consensus::deserialize(&raw).expect("a transaction");
    let constants = BridgeConstants::testnet();
    let height = constants.btc_height_when_pegout_tx_index_activates
        + constants.pegout_tx_index_grace_period_in_btc_blocks;
    let mut bridge = Registration::new(RskHardforkConfig::testnet(), constants, TESTNET_LOVELL700);
    put_named(
        &mut bridge.ctx,
        "svpSpendTxHashUnsigned",
        &rlp_encode_element(&[0; 32]),
    );
    let outcome = bridge.register(&tx, height);
    assert!(!outcome.returned);
    assert_eq!(outcome.processed_at, None);
}
