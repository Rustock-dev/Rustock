//! What the ported tests of the precompiled contracts share: a contract that
//! calls a precompiled contract and returns what the call left it.

use alloy_primitives::{Address, U256};

use crate::rskj_tests::vm::harness::{
    call, compile, push_address, Chain, CONTRACT, GAS, ONE_RBTC, SENDER,
};

/// What a contract's CALL of `precompile`, given `gas` and `input`, comes
/// to in block `number` of mainnet: whether the call succeeded, and the
/// return data it left. The input reaches the contract as its transaction's
/// data, which it copies to memory and passes on whole.
pub(super) fn call_precompile(
    number: u64,
    precompile: Address,
    gas: u32,
    input: &[u8],
) -> (bool, Vec<u8>) {
    let code = compile(&format!(
        "CALLDATASIZE PUSH1 0x00 PUSH2 0x0100 CALLDATACOPY
         PUSH1 0x00 PUSH1 0x00 CALLDATASIZE PUSH2 0x0100 PUSH1 0x00 {} PUSH4 0x{gas:08x} CALL
         PUSH1 0x00 MSTORE
         RETURNDATASIZE PUSH1 0x00 PUSH1 0x20 RETURNDATACOPY
         RETURNDATASIZE PUSH1 0x20 ADD PUSH1 0x00 RETURN",
        push_address(precompile)
    ));
    let mut chain = Chain::mainnet(number);
    chain.put(SENDER, 0, ONE_RBTC, &[]);
    chain.put(CONTRACT, 1, U256::ZERO, &code);
    let ran = chain.run(SENDER, call(0, CONTRACT, U256::ZERO, GAS, input));
    assert!(ran.success, "the calling contract runs");
    let (succeeded, data) = ran.output.split_at(32);
    (succeeded[31] == 1, data.to_vec())
}
