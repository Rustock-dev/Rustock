//! rskj's `co.rsk.vm.ExtCodeHashTest`, its test of RSKIP140 (papyrus200):
//! EXTCODEHASH is an opcode from it, and an invalid one before it. It costs
//! 400 gas, and pushes the hash of the code at an address: of nothing for
//! an account without code and for a precompile, and 0 where no account is.

use alloy_primitives::{address, keccak256, Address, B256, U256};

use super::harness::{
    call, compile, push_address, return_top, run_code, Chain, CONTRACT, GAS, ONE_RBTC,
    RETURN_ONE_WORD_GAS, SENDER,
};

/// papyrus200 on mainnet: RSKIP140.
const PAPYRUS: u64 = 2_392_700;

/// Before RSKIP140 EXTCODEHASH is an invalid opcode: the contract that runs
/// it fails with all its gas. From RSKIP140 it pushes the hash of the code
/// at the address, here the contract's own.
#[test]
fn extcodehash_is_an_invalid_opcode_before_rskip140() {
    let mut code = compile("ADDRESS EXTCODEHASH");
    code.extend(return_top(1));
    let before = run_code(PAPYRUS - 1, &code);
    let test = "testDoEXTCODEHASHWithHardForkDeactivated";
    assert!(!before.success, "{test}");
    assert_eq!(before.gas_used, GAS, "{test}");
    let from = run_code(PAPYRUS, &code);
    assert!(from.success, "{test}");
    assert_eq!(B256::from_slice(&from.output), keccak256(&code), "{test}");
}

/// What `executeExtCodeHash` runs from RSKIP140, PUSH20 of `address` and
/// EXTCODEHASH, with the word it pushes returned, where `accounts` hold
/// what they're given and no code: the word, and the gas the two took.
fn extcodehash_of(address: Address, accounts: &[(Address, U256)]) -> (B256, u64) {
    let mut chain = Chain::mainnet(PAPYRUS);
    chain.put(SENDER, 0, ONE_RBTC, &[]);
    for (account, balance) in accounts {
        chain.put(*account, 0, *balance, &[]);
    }
    let code = compile(&format!("{} EXTCODEHASH", push_address(address)));
    chain.put(CONTRACT, 1, U256::ZERO, &[code, return_top(1)].concat());
    let ran = chain.run(SENDER, call(0, CONTRACT, U256::ZERO, GAS, &[]));
    assert!(ran.success);
    let gas = ran.gas_used - 21_000 - RETURN_ONE_WORD_GAS;
    (B256::from_slice(&ran.output), gas)
}

#[test]
fn extcodehash_pushes_the_hash_of_a_contracts_code_for_403_gas() {
    let test = "testDoEXTCODEHASHToContractAndGetTheCodeHash";
    let callee = address!("471fd3ad3e9eeadeec4608b92d16ce6b500704cc");
    let mut chain = Chain::mainnet(PAPYRUS);
    chain.put(SENDER, 0, ONE_RBTC, &[]);
    chain.put(callee, 1, U256::ZERO, &[0x60, 0x0b]);
    let code = compile(&format!("{} EXTCODEHASH", push_address(callee)));
    chain.put(CONTRACT, 1, U256::ZERO, &[code, return_top(1)].concat());
    let ran = chain.run(SENDER, call(0, CONTRACT, U256::ZERO, GAS, &[]));
    assert!(ran.success, "{test}");
    assert_eq!(
        B256::from_slice(&ran.output),
        keccak256([0x60, 0x0b]),
        "{test}"
    );
    assert_eq!(ran.gas_used - 21_000 - RETURN_ONE_WORD_GAS, 403, "{test}");
}

#[test]
fn extcodehash_of_an_account_without_code_is_the_hash_of_nothing() {
    let test = "testDoEXTCODEHASHToAccountAndGetEmptyHash";
    let account = address!("cd2a3d9f938e13cd947ec05abc7fe734df8dd826");
    let pushed = extcodehash_of(account, &[(account, U256::from(1))]);
    assert_eq!(pushed, (keccak256([]), 403), "{test}");
}

#[test]
fn extcodehash_of_no_account_is_0() {
    let test = "testDoEXTCODEHASHToNonExistentAccountAndGetZero";
    let pushed = extcodehash_of(address!("1111111111222222222233333333334444444444"), &[]);
    assert_eq!(pushed, (B256::ZERO, 403), "{test}");
}

#[test]
fn extcodehash_of_a_precompile_is_the_hash_of_nothing() {
    let test = "testDoEXTCODEHASHToPrecompiledContractAndGetEmptyHash";
    let bridge = address!("0000000000000000000000000000000001000006");
    assert_eq!(extcodehash_of(bridge, &[]), (keccak256([]), 403), "{test}");
}

/// With nothing on the stack EXTCODEHASH fails the program, which spends all
/// its gas.
#[test]
fn extcodehash_of_an_empty_stack_fails_with_all_the_gas() {
    let test = "testDoEXTCODEHASHWithoutArguments";
    let ran = run_code(PAPYRUS, &compile("EXTCODEHASH"));
    assert!(!ran.success, "{test}");
    assert_eq!(ran.gas_used, GAS, "{test}");
}
