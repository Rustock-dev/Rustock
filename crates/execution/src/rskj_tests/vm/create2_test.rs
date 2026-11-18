//! rskj's `co.rsk.vm.Create2Test`, its tests of RSKIP125 (wasabi100): CREATE2
//! is an opcode from it, and an invalid one before it.

use alloy_primitives::U256;

use super::harness::{compile, return_top, run_code, GAS};

/// wasabi100 on mainnet: RSKIP125.
const WASABI: u64 = 1_591_000;

/// Before RSKIP125 CREATE2 is an invalid opcode: the contract that runs it
/// fails with all its gas. From RSKIP125 it creates the contract.
#[test]
fn create2_is_an_invalid_opcode_before_rskip125() {
    let mut code = compile("PUSH1 0x00 PUSH1 0x00 PUSH1 0x00 PUSH1 0x00 CREATE2");
    code.extend(return_top(1));
    let before = run_code(WASABI - 1, &code);
    let test = "testCREATE2ShouldFailInvalidOpcode";
    assert!(!before.success, "{test}");
    assert_eq!(before.gas_used, GAS, "{test}");
    let from = run_code(WASABI, &code);
    assert!(from.success, "{test}");
    assert_ne!(U256::from_be_slice(&from.output), U256::ZERO, "{test}");
}
