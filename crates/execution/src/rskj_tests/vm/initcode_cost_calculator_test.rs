//! rskj's `org.ethereum.cost.InitcodeCostCalculatorTest`: RSKIP438's price
//! of init code, 2 gas for each 32-byte word or part of one
//! (`InitcodeCostCalculator.calculateCost`), seen in the gas a transaction
//! that creates a contract must carry from lovell700 on.

use alloy_primitives::hex;

use super::harness::{create, Chain, ONE_RBTC, SENDER};

/// lovell700 on mainnet: RSKIP438.
const LOVELL: u64 = 7_338_024;

/// A creation's gas before RSKIP438: 53,000, 4 for each zero byte of its
/// init code, and 16 for each other (RSKIP400).
fn intrinsic(data: &[u8]) -> u64 {
    let bytes = data.iter().map(|byte| if *byte == 0 { 4 } else { 16 });
    53_000 + bytes.sum::<u64>()
}

/// Whether the miner takes the creation of `data`, carrying `gas`, into
/// block `number` of mainnet.
fn taken(number: u64, data: &[u8], gas: u64) -> bool {
    let mut chain = Chain::mainnet(number);
    chain.put(SENDER, 0, ONE_RBTC, &[]);
    let mined = chain.mine(&[(create(0, gas, data), SENDER)]);
    let (_, drops) = mined.expect("the miner builds the block");
    drops.invalid.is_empty()
}

/// rskj's three init codes, of 4, 40 and 71 bytes, cost 2, 4 and 6 gas from
/// RSKIP438: a creation of each needs that much more than before it.
#[test]
fn from_rskip438_a_creation_carries_2_gas_for_each_word_of_init_code() {
    let cases = [
        (2, "00010203".to_string()),
        (
            4,
            [
                "fd5fa123fd5fa123a31231000076890afd5fa123a31231000076890a",
                "fd5fa123a31231000076890a",
            ]
            .concat(),
        ),
        (
            6,
            [
                "fd5fa123a31231000076890abcfff41239123912323123fd5fa123afd5fa12fd5fa123a3",
                "1231000076890afd5fa123a31231000076890a3a31231000076890a31231000076890a",
            ]
            .concat(),
        ),
    ];
    let test = "testTransactionInitCodeCostCalculation";
    for (cost, data) in cases {
        let data = hex::decode(data).expect("hex");
        let gas = intrinsic(&data);
        assert!(taken(LOVELL - 1, &data, gas), "{test}: {cost}, before");
        assert!(
            !taken(LOVELL, &data, gas + cost - 1),
            "{test}: {cost}, a gas short"
        );
        assert!(taken(LOVELL, &data, gas + cost), "{test}: {cost}");
    }
}
