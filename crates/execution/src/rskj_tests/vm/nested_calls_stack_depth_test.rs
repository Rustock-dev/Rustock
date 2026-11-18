//! rskj's `org.ethereum.core.NestedCallsStackDepthTest`: the Environment
//! contract (RSKIP203, arrowhead600) answers a contract's call of its
//! getCallStackDepth with how deep the call is, and before the upgrade no
//! contract is at its address.
//!
//! rskj's test of the second leaves arrowhead600 unscheduled on a regtest
//! whose later upgrades are at block 0, a schedule rustock refuses
//! (`RskHardforkConfig::with_rskj_overrides`). Here both run on mainnet, in
//! the upgrade's first block and the block before.

use alloy_primitives::{hex, keccak256, Address, U256};

use super::harness::{
    call, compile, push_address, return_top, run_code, words, Chain, CONTRACT, GAS, ONE_RBTC,
    SENDER,
};
use crate::precompiles::ENVIRONMENT_ADDR;

/// arrowhead600 on mainnet: RSKIP203.
const ARROWHEAD: u64 = 6_223_700;

/// Code that calls the Environment contract's getCallStackDepth with
/// 100,000 gas, for an answer of one word written to memory at 0x20.
fn ask_depth() -> String {
    let selector = hex::encode(&keccak256("getCallStackDepth()")[..4]);
    format!(
        "PUSH4 0x{selector} PUSH1 0x00 MSTORE PUSH1 0x20 PUSH1 0x20 PUSH1 0x04 PUSH1 0x1c \
         PUSH1 0x00 {} PUSH3 0x0186a0 CALL",
        push_address(ENVIRONMENT_ADDR)
    )
}

/// What a contract gets from calling the Environment contract's
/// getCallStackDepth in block `number` of mainnet: the word it was
/// answered, the size of the answer, and whether the call succeeded.
fn call_stack_depth_at(number: u64) -> [U256; 3] {
    let mut code = compile(&format!(
        "{} RETURNDATASIZE PUSH1 0x20 MLOAD",
        ask_depth()
    ));
    code.extend(return_top(3));
    let ran = run_code(number, &code);
    assert!(ran.success, "the contract runs at #{number}");
    let answer = words(&ran.output);
    [answer[0], answer[1], answer[2]]
}

/// Before RSKIP203 no contract is at the Environment contract's address: a
/// contract's call of it succeeds and gets nothing back, where rskj's
/// Solidity caller reverts for want of an answer. From RSKIP203 it answers
/// one word, the depth of the call: 1 from a contract a transaction calls.
#[test]
fn the_environment_contract_answers_a_contracts_call_from_rskip203() {
    let (none, one, word) = (U256::ZERO, U256::from(1), U256::from(32));
    let test = "testContractConsensusInactive";
    assert_eq!(call_stack_depth_at(ARROWHEAD - 1), [none, none, one], "{test}");
    let test = "testContractCallsGetCallStackDepth";
    assert_eq!(call_stack_depth_at(ARROWHEAD), [one, word, one], "{test}");
}

/// A call three contracts deep is answered 3: the contract a transaction
/// calls calls a second, which calls a third, which asks.
#[test]
fn a_call_three_contracts_deep_is_answered_3() {
    let (second, third) = (Address::repeat_byte(0xc2), Address::repeat_byte(0xc3));
    // Calls `next` with `gas`, and returns the word it answers.
    let relay = |next: Address, gas: u32| {
        compile(&format!(
            "PUSH1 0x20 PUSH1 0x00 PUSH1 0x00 PUSH1 0x00 PUSH1 0x00 {} PUSH4 0x{gas:08x} CALL \
             POP PUSH1 0x20 PUSH1 0x00 RETURN",
            push_address(next)
        ))
    };
    let asks = compile(&format!(
        "{} POP PUSH1 0x20 PUSH1 0x20 RETURN",
        ask_depth()
    ));
    let mut chain = Chain::mainnet(ARROWHEAD);
    chain.put(SENDER, 0, ONE_RBTC, &[]);
    chain.put(CONTRACT, 1, U256::ZERO, &relay(second, 600_000));
    chain.put(second, 1, U256::ZERO, &relay(third, 300_000));
    chain.put(third, 1, U256::ZERO, &asks);
    let ran = chain.run(SENDER, call(0, CONTRACT, U256::ZERO, GAS, &[]));
    let test = "testNestedContractCallsGetCallStackDepth";
    assert!(ran.success, "{test}");
    assert_eq!(words(&ran.output), [U256::from(3)], "{test}");
}
