//! rskj's `BridgeSupportGetEstimatedFeesTest`: what
//! `getEstimatedFeesForNextPegOutEvent` answers, in satoshis, under each
//! era's rules. Before RSKIP271 (hop400), and before RSKIP385
//! (fingerroot500) with no request queued, nothing. Before RSKIP305 (reed800)
//! the fee of a peg-out of two inputs and an output for each request queued
//! and two more, sized by the active federation's format. From it the fee of
//! a batch built of the requests queued and one more, of a bitcoin before
//! RSKIP540 (vetiver900) and of the minimum peg-out value from it, or, where
//! that batch can't be built, the former.
//!
//! rskj's tests store the active federation, its UTXOs and a fee per
//! kilobyte of 8,000 satoshis under each era's activations; the ports store
//! the same in mainnet's Bridge at the block of that era's upgrade. The
//! federations are rskj's test builders': the nine members `member01` to
//! `member09`, standard multisig or P2SH ERP, or twenty P2SH-P2WSH ERP, the
//! ERP ones with mainnet's emergency keys and delay, made at block 1.

use alloy_primitives::U256;

use crate::bridge::peg;

use super::support::*;

/// rskj's `FEE_PER_KB`.
const FEE_PER_KB: u64 = 8_000;

/// Eight and ten bitcoins.
const EIGHT: u64 = 8 * BTC;
const TEN: u64 = 10 * BTC;

/// Mainnet's minimum peg-out value, rskj's `MIN_PEGOUT_TX_VALUE`.
const MIN: u64 = 400_000;

/// rskj `BitcoinTestUtils.getBtcEcKeyFromSeed(seed)`'s public key.
fn key_of(seed: &str) -> [u8; 33] {
    use sha3::Digest;
    let private = sha3::Keccak256::digest(seed.as_bytes());
    k256::ecdsa::SigningKey::from_slice(&private)
        .expect("a key")
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .try_into()
        .expect("33 bytes")
}

/// The members of rskj's builder for `format`, sorted: nine, or twenty for
/// P2SH-P2WSH ERP.
fn members(format: u64) -> Vec<[u8; 33]> {
    let count = if format == P2SH_P2WSH_ERP { 20 } else { 9 };
    let mut keys: Vec<[u8; 33]> = (1..=count)
        .map(|n| key_of(&format!("member{n:02}")))
        .collect();
    keys.sort();
    keys
}

/// `value` as rskj's `serializeCoin` writes it.
fn coin(value: u64) -> Vec<u8> {
    let bytes = value.to_be_bytes();
    let first = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    rlp_encode_element(&bytes[first..])
}

/// One of rskj's cases: its test, the era's upgrade block, the federation's
/// format, the values of its UTXOs, the requests queued, each group `count`
/// requests of `amount` (`addPegoutRequests`), and the fee per kilobyte.
struct Case {
    test: &'static str,
    number: u64,
    format: u64,
    utxos: &'static [u64],
    requests: &'static [(usize, u64)],
    fee_per_kb: u64,
}

impl Case {
    /// What `getEstimatedFeesForNextPegOutEvent` answers for this case.
    fn estimated(&self) -> u64 {
        let mut bridge = Bridge::mainnet_at(self.number);
        put_named(&mut bridge.ctx, "feePerKb", &coin(self.fee_per_kb));
        let keys = members(self.format);
        store_federation(&mut bridge.ctx, NEW_FEDERATION, &keys, 1, self.format);
        let redeem = peg::federation_redeem_for_format_pub(
            &keys,
            self.format,
            &bridge.constants,
            &bridge.cfg,
            1,
        );
        let script = p2sh(&peg::federation_output_hash160(&redeem, self.format));
        let utxos: Vec<_> = self
            .utxos
            .iter()
            .zip(1u8..)
            .map(|(&value, tag)| utxo(tag, value, script.clone()))
            .collect();
        let number = bridge.number();
        crate::bridge::storage::store_federation_utxos(
            &mut bridge.ctx,
            &bridge.cfg,
            number,
            &utxos,
        );
        let queued: Vec<([u8; 20], u64)> = self
            .requests
            .iter()
            .flat_map(|&(count, amount)| {
                (0..count).map(move |i| (hash160(&key_of(&format!("receiver{i}"))), amount))
            })
            .collect();
        put_named(
            &mut bridge.ctx,
            "releaseRequestQueue",
            &legacy_queue(&queued),
        );
        let out = peg::get_estimated_fees_for_next_pegout(
            &mut bridge.ctx,
            0,
            &bridge.constants,
            &bridge.cfg,
        )
        .expect("it answers");
        U256::from_be_slice(&out.bytes).to::<u64>()
    }
}

/// Runs each case and compares its answer with rskj's.
fn assert_estimates(cases: &[(Case, u64)]) {
    for (case, expected) in cases {
        assert_eq!(case.estimated(), *expected, "{}", case.test);
    }
}

/// A case at `number` with the fee per kilobyte of rskj's tests.
const fn case(
    test: &'static str,
    number: u64,
    format: u64,
    utxos: &'static [u64],
    requests: &'static [(usize, u64)],
) -> Case {
    Case {
        test,
        number,
        format,
        utxos,
        requests,
        fee_per_kb: FEE_PER_KB,
    }
}

/// The same case at twice the fee per kilobyte.
const fn doubled(case: Case) -> Case {
    Case {
        fee_per_kb: 2 * FEE_PER_KB,
        ..case
    }
}

/// `PreHop400Activation.getEstimatedFeesForNextPegOutEvent_preHop_shouldReturnZeroFees`,
/// for none, one and 150 requests: before RSKIP271 nothing is estimated.
#[test]
fn before_rskip271_no_fee_is_estimated() {
    let utxos = &[EIGHT, EIGHT, EIGHT];
    let at = MAINNET_IRIS300;
    let test = "preHop_shouldReturnZeroFees";
    assert_estimates(&[
        (case(test, at, STANDARD_MULTISIG, utxos, &[]), 0),
        (case(test, at, STANDARD_MULTISIG, utxos, &[(1, BTC)]), 0),
        (case(test, at, STANDARD_MULTISIG, utxos, &[(150, BTC)]), 0),
    ]);
}

/// `PostHop400PreFingerrootActivations`: from RSKIP271 and before RSKIP385,
/// nothing with no request queued, and the fee of two inputs and an output
/// for each request and two more with some.
#[test]
fn from_rskip271_the_fee_is_estimated_only_with_a_request_queued() {
    let at = MAINNET_HOP400;
    let three = &[EIGHT, EIGHT, EIGHT];
    let one = &[EIGHT];
    assert_estimates(&[
        (
            case("withStandardMultisigFederation_withNoPegoutRequests_shouldEstimateZeroFees", at, STANDARD_MULTISIG, three, &[]),
            0,
        ),
        (
            case("withStandardMultisigFederation_withOnePegoutRequest_shouldEstimateFeesFromInputAndOutputCount", at, STANDARD_MULTISIG, three, &[(1, BTC)]),
            12_240,
        ),
        (
            case("withStandardMultisigFederation_withManyPegoutRequests_shouldEstimateFeesFromInputAndOutputCount", at, STANDARD_MULTISIG, three, &[(150, BTC)]),
            50_384,
        ),
        (
            doubled(case("withStandardMultisigFederation_withManyPegoutRequests_shouldEstimateFeesFromInputAndOutputCount, doubled", at, STANDARD_MULTISIG, three, &[(150, BTC)])),
            100_768,
        ),
        (
            case("withP2shErpFederation_withNoPegoutRequests_shouldEstimateZeroFees", at, P2SH_ERP, one, &[]),
            0,
        ),
        (
            case("withP2shErpFederation_withOnePegoutRequest_shouldEstimateFeesFromInputAndOutputCount", at, P2SH_ERP, one, &[(1, BTC)]),
            14_608,
        ),
        (
            case("withP2shErpFederation_withManyPegoutRequests_shouldEstimateFeesFromInputAndOutputCount", at, P2SH_ERP, one, &[(150, BTC)]),
            52_752,
        ),
        (
            doubled(case("withP2shErpFederation_withManyPegoutRequests_shouldEstimateFeesFromInputAndOutputCount, doubled", at, P2SH_ERP, one, &[(150, BTC)])),
            105_504,
        ),
    ]);
}

/// `PostFingerrootPreReedActivations`: from RSKIP385, the fee of two inputs
/// and an output for each request and two more, with no request queued too.
#[test]
fn from_rskip385_the_fee_is_estimated_with_no_request_queued_too() {
    let at = MAINNET_FINGERROOT500;
    let one = &[EIGHT];
    assert_estimates(&[
        (
            case("withP2shErpFederation_withNoPegoutRequests_shouldEstimateFeesFromInputAndOutputCount", at, P2SH_ERP, one, &[]),
            14_352,
        ),
        (
            case("withP2shErpFederation_withOnePegoutRequest_shouldEstimateFeesFromInputAndOutputCount", at, P2SH_ERP, one, &[(1, BTC)]),
            14_608,
        ),
        (
            case("withP2shErpFederation_withManyPegoutRequests_shouldEstimateFeesFromInputAndOutputCount", at, P2SH_ERP, one, &[(150, BTC)]),
            52_752,
        ),
        (
            doubled(case("withP2shErpFederation_withManyPegoutRequests_shouldEstimateFeesFromInputAndOutputCount, doubled", at, P2SH_ERP, one, &[(150, BTC)])),
            105_504,
        ),
    ]);
}

/// The cases rskj runs from RSKIP305 both before RSKIP540
/// (`PostReedPreVetiverActivations`) and from it (`PostVetiverActivation`):
/// each a P2SH-P2WSH ERP federation's, and what it is estimated before
/// RSKIP540 and from it.
fn from_rskip305(at: u64, from_rskip540: bool) -> Vec<(Case, u64)> {
    let f = P2SH_P2WSH_ERP;
    let two_of_eight = &[EIGHT, EIGHT];
    let four_of_one = &[BTC, BTC, BTC, BTC];
    let two_of_one = &[BTC, BTC];
    let one_and_min = &[BTC, MIN];
    let either = |before: u64, from: u64| if from_rskip540 { from } else { before };
    let mut cases = vec![
        (
            case("withP2shP2wshErpFederation_withNoPegoutRequests_shouldEstimateFeesFromTransactionSimulation", at, f, two_of_eight, &[]),
            4_536,
        ),
        (
            case("withP2shP2wshErpFederation_withNoUtxos_withNoPegoutRequests_shouldFallBackToEstimateFeesFromInputAndOutputCount", at, f, &[], &[]),
            7_664,
        ),
        (
            case("withP2shP2wshErpFederation_withNoUtxos_withOnePegoutRequest_shouldFallBackToEstimateFeesFromInputAndOutputCount", at, f, &[], &[(1, BTC)]),
            7_920,
        ),
        (
            case("withP2shP2wshErpFederation_withMinPegoutValueRequests_shouldEstimateFeesFromTransactionSimulation, 1", at, f, four_of_one, &[(1, MIN)]),
            either(8_736, 4_808),
        ),
        (
            case("withP2shP2wshErpFederation_withMinPegoutValueRequests_shouldEstimateFeesFromTransactionSimulation, 2", at, f, four_of_one, &[(2, MIN)]),
            either(9_008, 5_080),
        ),
        (
            case("withP2shP2wshErpFederation_withMinPegoutValueRequests_shouldEstimateFeesFromTransactionSimulation, 3", at, f, four_of_one, &[(3, MIN)]),
            either(9_280, 5_352),
        ),
        (
            case("withP2shP2wshErpFederation_withTwoBTCs_withPegoutRequestOfOneBtc", at, f, two_of_one, &[(1, BTC)]),
            either(8_480, 8_736),
        ),
        (
            case("withP2shP2wshErpFederation_withTwoBTCs_withPegoutRequestGreaterThanOneBtc", at, f, two_of_one, &[(1, BTC + 1)]),
            either(7_920, 8_736),
        ),
        (
            case("withP2shP2wshErpFederation_withTwoUtxos_withTwoPegoutRequests_shouldEstimateFeesFromTransactionSimulation", at, f, two_of_eight, &[(1, BTC + 1), (1, TEN)]),
            9_008,
        ),
        (
            case("withP2shP2wshErpFederation_withTwoUtxos_withThreePegoutRequests_shouldEstimateFeesFromTransactionSimulation", at, f, two_of_eight, &[(2, BTC), (1, TEN)]),
            9_280,
        ),
        (
            doubled(case("withP2shP2wshErpFederation_withTwoUtxos_withThreePegoutRequests_shouldEstimateFeesFromTransactionSimulation, doubled", at, f, two_of_eight, &[(2, BTC), (1, TEN)])),
            18_560,
        ),
    ];
    if from_rskip540 {
        cases.push((
            case("withP2shP2wshErpFederationHavingOneBTCAndMinPegoutValue_withOneBTCPegoutRequest_shouldEstimateFeesFromTransactionSimulation", at, f, one_and_min, &[(1, BTC)]),
            8_480,
        ));
        cases.push((
            case("withP2shP2wshErpFederationHavingOneBTCAndMinPegoutValue_withPegoutRequestGreaterThanOneBtc_shouldFallBackToEstimateFeesFromInputAndOutputCount", at, f, one_and_min, &[(1, BTC + 1)]),
            7_920,
        ));
    }
    cases
}

/// `PostReedPreVetiverActivations`: from RSKIP305 the fee of a batch built of
/// the requests and one more of a bitcoin, or, where it can't be built, of
/// two inputs and an output for each request and two more, sized as segwit.
#[test]
fn from_rskip305_the_fee_is_that_of_a_batch_with_one_more_bitcoin() {
    assert_estimates(&from_rskip305(MAINNET_REED800, false));
}

/// `PostVetiverActivation`: from RSKIP540 the request added is of the minimum
/// peg-out value.
#[test]
fn from_rskip540_the_batch_has_one_more_minimum_pegout() {
    assert_estimates(&from_rskip305(MAINNET_VETIVER900, true));
}
