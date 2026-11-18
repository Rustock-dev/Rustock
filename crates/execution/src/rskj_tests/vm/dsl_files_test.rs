//! rskj's `co.rsk.test.DslFilesTest`, its test of the gas of a creation
//! before RSKIP438 (lovell700) and from it.

use alloy_primitives::hex;

use super::harness::{create, Chain, ONE_RBTC, SENDER};

/// lovell700 on mainnet: RSKIP438.
const LOVELL: u64 = 7_338_024;

/// The init code of rskj's `create01.txt`, solc's empty contract: 80 bytes,
/// three words.
fn empty_contract() -> Vec<u8> {
    let code = [
        "60606040523415600e57600080fd5b603580601b6000396000f3006060604052600080fd00",
        "a165627a7a72305820b25edb28bec763685838b8044760e105b5385638276b4768c8045237b8fc",
        "6bf10029",
    ];
    hex::decode(code.concat()).expect("hex")
}

/// The gas the creation of rskj's empty contract uses in block `number` of
/// mainnet.
fn creation_gas(number: u64) -> u64 {
    let mut chain = Chain::mainnet(number);
    chain.put(SENDER, 0, ONE_RBTC, &[]);
    let ran = chain.run(SENDER, create(0, 500_000, &empty_contract()));
    assert!(ran.success, "the creation runs");
    ran.gas_used
}

/// The creation of rskj's empty contract uses 64,857 gas before RSKIP438,
/// and 64,863 from it: 2 more for each of its init code's three words.
#[test]
fn a_creation_of_rskjs_empty_contract_uses_6_gas_more_from_rskip438() {
    let test = "runCreate01ResourceBeforeActivationOfRSKIP438";
    assert_eq!(creation_gas(LOVELL - 1), 64_857, "{test}");
    assert_eq!(creation_gas(LOVELL), 64_863, "runCreate01Resource");
}
