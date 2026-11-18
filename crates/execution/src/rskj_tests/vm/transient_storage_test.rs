//! rskj's `co.rsk.vm.opcode.TransientStorageTest`: TLOAD and TSTORE (RSKIP446)
//! and the stack, run from lovell700 on mainnet.

use alloy_primitives::U256;

use super::harness::{compile, hex_word, return_top, run_code, words, GAS, RETURN_ONE_WORD_GAS};

/// lovell700 on mainnet: RSKIP446.
const LOVELL: u64 = 7_338_024;

/// TLOAD with nothing on the stack fails, with all of the call's gas: rskj's
/// `StackTooSmallException`.
#[test]
fn tload_with_an_empty_stack_fails_with_all_the_gas() {
    let ran = run_code(LOVELL, &compile("TLOAD"));
    assert!(!ran.success, "testTLoadDynamicExecutionContextUnderflow");
    assert_eq!(ran.gas_used, GAS);
}

/// TLOAD of a key nothing was stored at pushes zero, for 100 gas.
#[test]
fn tload_of_a_key_never_stored_pushes_zero() {
    let one = hex_word(U256::from(1));
    let code = [compile(&format!("PUSH32 {one} TLOAD")), return_top(1)].concat();
    let ran = run_code(LOVELL, &code);
    assert!(ran.success, "testTLoadDynamicExecutionContextWorksFine");
    assert_eq!(words(&ran.output), [U256::ZERO]);
    assert_eq!(ran.gas_used, 21_000 + 3 + 100 + RETURN_ONE_WORD_GAS);
}

/// TSTORE with fewer than its two words on the stack fails, with all of the
/// call's gas.
#[test]
fn tstore_short_of_its_two_words_fails_with_all_the_gas() {
    for code in ["TSTORE", "PUSH1 0x01 TSTORE"] {
        let ran = run_code(LOVELL, &compile(code));
        let test = "testTStoreDynamicExecutionContextUnderflow";
        assert!(!ran.success, "{test}: {code}");
        assert_eq!(ran.gas_used, GAS, "{code}");
    }
}

/// TSTORE takes its key and its value off the stack and runs on, for 100
/// gas: the word pushed before them is the top one after it.
#[test]
fn tstore_takes_its_key_and_its_value_off_the_stack() {
    let (value, key) = (hex_word(U256::from(0x420)), hex_word(U256::from(1)));
    let code = compile(&format!("PUSH1 0x2a PUSH32 {value} PUSH32 {key} TSTORE"));
    let ran = run_code(LOVELL, &[code, return_top(1)].concat());
    assert!(ran.success, "testTStoreDynamicExecutionContextWorksFine");
    assert_eq!(words(&ran.output), [U256::from(0x2a)]);
    assert_eq!(ran.gas_used, 21_000 + 9 + 100 + RETURN_ONE_WORD_GAS);
}

/// TLOAD of a key pushes what TSTORE stored there.
#[test]
fn tload_pushes_what_tstore_stored_at_the_key() {
    let (value, key) = (hex_word(U256::from(0x420)), hex_word(U256::from(1)));
    let stored = format!("PUSH32 {value} PUSH32 {key} TSTORE");
    let code = compile(&format!("{stored} PUSH32 {key} TLOAD"));
    let ran = run_code(LOVELL, &[code, return_top(1)].concat());
    let test = "testTStoreTloadDynamicExecutionContextWorksFine";
    assert!(ran.success, "{test}");
    assert_eq!(words(&ran.output), [U256::from(0x420)]);
}
