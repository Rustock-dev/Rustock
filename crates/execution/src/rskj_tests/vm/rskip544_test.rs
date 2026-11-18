//! rskj's `co.rsk.vm.RSKIP544Test`, its test of RSKIP544's activation
//! (vetiver900, EIP-3541): from it CREATE deploys no code that starts with
//! 0xEF (`Program.hasInvalidCodePrefix`); before it, and for code that
//! starts with another byte, it deploys.

use alloy_primitives::U256;

use super::harness::{call, compile, Chain, CONTRACT, GAS, ONE_RBTC, SENDER};

/// vetiver900 on mainnet: RSKIP544.
const VETIVER: u64 = 8_804_200;

/// The code a contract's CREATE leaves at the new address, from rskj's
/// `initcode` of `size` bytes, in block `number` of mainnet. RSK gives a
/// CREATE all the gas left, so the contract stops right after it.
fn deployed(number: u64, initcode: &str, size: u8) -> Vec<u8> {
    let code = compile(&format!(
        "PUSH32 0x{initcode} PUSH1 0x00 MSTORE PUSH1 0x{size:02x} PUSH1 0x00 PUSH1 0x00 CREATE STOP"
    ));
    let mut chain = Chain::mainnet(number);
    chain.put(SENDER, 0, ONE_RBTC, &[]);
    chain.put(CONTRACT, 1, U256::ZERO, &code);
    let ran = chain.run(SENDER, call(0, CONTRACT, U256::ZERO, GAS, &[]));
    assert!(ran.success, "the creating contract runs");
    chain.code(CONTRACT.create(1))
}

/// rskj's scenarios: code of 0xEF and of 0xEF00 is deployed before RSKIP544
/// and not from it; code of 0xFE is deployed on both sides.
#[test]
fn from_rskip544_create_deploys_no_code_that_starts_with_0xef() {
    let cases = [
        (
            "60ef60005360016000f300000000000000000000000000000000000000000000",
            0x0a,
            vec![0xef],
            false,
        ),
        (
            "61ef006000526002601ef3000000000000000000000000000000000000000000",
            0x0b,
            vec![0xef, 0x00],
            false,
        ),
        (
            "60fe60005360016000f300000000000000000000000000000000000000000000",
            0x0a,
            vec![0xfe],
            true,
        ),
    ];
    let test = "testActivationBoundary_Parameterized";
    for (initcode, size, code, deployed_from) in cases {
        let before = deployed(VETIVER - 1, initcode, size);
        assert_eq!(before, code, "{test}: {initcode}");
        let from = deployed(VETIVER, initcode, size);
        assert_eq!(from == code, deployed_from, "{test}: {initcode}");
    }
}
