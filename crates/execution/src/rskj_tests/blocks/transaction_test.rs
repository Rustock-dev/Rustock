//! rskj `co.rsk.core.TransactionTest`: what a transaction costs before it
//! runs (`Transaction.transactionCost`), which a transaction's gas must
//! cover and its block pays.

use super::support::{creation, run_on_mainnet, LOVELL700};

/// rskj `testTransactionCostForContractCreationWithRSKIP438Disabled`,
/// `testTransactionCostForContractCreationWithRSKIP438Enabled` and
/// `testTransactionCostForContractCreationWithNullDataAndRSKIP438Enabled`:
/// a creation costs 53,000 and its data; from RSKIP438 2 more for each 32
/// bytes of its initcode, the last counted whole, and nothing more for
/// none (`InitcodeCostCalculator`). rskj's test prices one byte of 0xff
/// with RSKIP400 off, which no network has where RSKIP438 is on; here the
/// initcode is zero bytes, 4 each on either side of RSKIP400, which run as
/// STOP and leave nothing to pay for running or deploying.
#[test]
fn from_rskip438_a_creation_pays_for_its_initcode_by_the_word() {
    let cost = |number: u64, init_code: Vec<u8>| {
        let result = run_on_mainnet(number, &creation(init_code)).unwrap();
        assert!(result.success, "block {number}");
        result.gas_used
    };
    // One word.
    assert_eq!(cost(LOVELL700 - 1, vec![0]), 53_004);
    assert_eq!(cost(LOVELL700, vec![0]), 53_006);
    // Thirty-three bytes: two words.
    assert_eq!(cost(LOVELL700 - 1, vec![0; 33]), 53_132);
    assert_eq!(cost(LOVELL700, vec![0; 33]), 53_136);
    // None.
    assert_eq!(cost(LOVELL700 - 1, Vec::new()), 53_000);
    assert_eq!(cost(LOVELL700, Vec::new()), 53_000);
}
