//! rskj's `org.ethereum.core.ContractCreatingDslRollbackTest`, its tests of
//! RSKIP453 (lovell700): before it a contract's CREATE whose new code is
//! past 24,576 bytes leaves the new contract with no code and the creating
//! contract going on (`Program.exceedsMaxContractSize`, then
//! `finalizeContractCreation`); from it the creation is rolled back.

use alloy_primitives::U256;

use super::harness::{call, compile, return_top, Chain, CONTRACT, ONE_RBTC, SENDER};

/// lovell700 on mainnet: RSKIP453.
const LOVELL: u64 = 7_338_024;
/// The gas of the transaction: enough to pay for the new code's bytes.
const CREATION_GAS: u64 = 6_000_000;

/// What a contract's CREATE of 24,577 zero bytes of code does in block
/// `number` of mainnet: whether its transaction succeeded, and the code and
/// account it left at the new address.
fn create_past_the_limit(number: u64) -> (bool, Vec<u8>, bool) {
    // Init code: PUSH2 24,577, PUSH1 0, RETURN.
    let init = "PUSH6 0x6160016000f3 PUSH1 0x00 MSTORE PUSH1 0x06 PUSH1 0x1a PUSH1 0x00 CREATE";
    let mut code = compile(init);
    code.extend(return_top(1));
    let mut chain = Chain::mainnet(number);
    chain.put(SENDER, 0, ONE_RBTC, &[]);
    chain.put(CONTRACT, 1, U256::ZERO, &code);
    let ran = chain.run(SENDER, call(0, CONTRACT, U256::ZERO, CREATION_GAS, &[]));
    let created = CONTRACT.create(1);
    let exists = chain.account(created).is_some();
    (ran.success, chain.code(created), exists)
}

/// Before RSKIP453 the transaction succeeds, and the new contract is there
/// with no code.
#[test]
fn before_rskip453_code_past_the_limit_leaves_its_contract_empty() {
    let (success, code, exists) = create_past_the_limit(LOVELL - 1);
    let test = "createExecutionExceedingCodeSizeDoesNotRollbackBefore453";
    assert!(success, "{test}");
    assert!(code.is_empty() && exists, "{test}");
}

/// From RSKIP453 the creation is rolled back, and the transaction fails.
#[test]
fn from_rskip453_code_past_the_limit_fails_the_creation() {
    let (success, _, exists) = create_past_the_limit(LOVELL);
    let test = "createExecutionExceedingCodeSizeDoesRollbackAfter453";
    assert!(!success, "{test}");
    assert!(!exists, "{test}");
}
