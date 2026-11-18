//! rskj's `co.rsk.vm.opcode.TransientStorageDslTest`: transient storage
//! (RSKIP446, EIP-1153) across frames, contexts and transactions.
//!
//! rskj runs each case as Solidity contracts in blocks of its DSL and counts
//! the `OK` events its contracts emit, one for each check that holds. Here
//! each contract is written in opcodes and logs each word it checks (a LOG0
//! of the word), and a test holds the logged words to the values rskj's
//! contracts check them against. A frame that fails drops its logs, as rskj
//! drops its events. Each case runs from lovell700 on mainnet.

use alloy_primitives::{Address, U256};
use rustock_core::Transaction;

use super::harness::{
    call, compile, hex_word, logged, push_address, Chain, CONTRACT, LOG_TOP, ONE_RBTC, SENDER,
};
use crate::executor::TxExecutionResult;

/// lovell700 on mainnet: RSKIP446.
const LOVELL: u64 = 7_338_024;
/// A second contract with the code of the first, where a case needs one.
const OTHER: Address = Address::repeat_byte(0x0b);
/// The contract a case's contract calls.
const CALLEE: Address = Address::repeat_byte(0xce);
/// An account whose code is the init code a case's contract creates from.
const HOLDER: Address = Address::repeat_byte(0xd0);

/// A chain whose next block is `number` on mainnet, with a funded sender
/// and `contracts`.
fn chain_at(number: u64, contracts: &[(Address, &[u8])]) -> Chain {
    let mut chain = Chain::mainnet(number);
    chain.put(SENDER, 0, ONE_RBTC, &[]);
    for (address, code) in contracts {
        chain.put(*address, 1, U256::ZERO, code);
    }
    chain
}

/// [`chain_at`] lovell700.
fn chain_with(contracts: &[(Address, &[u8])]) -> Chain {
    chain_at(LOVELL, contracts)
}

/// A transaction of `gas` from the sender to [`CONTRACT`], whose input is
/// `words`.
fn sent(nonce: u64, gas: u64, words: &[U256]) -> (Transaction, Address) {
    let input: Vec<u8> = words.iter().flat_map(U256::to_be_bytes::<32>).collect();
    (call(nonce, CONTRACT, U256::ZERO, gas, &input), SENDER)
}

/// Runs [`sent`] alone in the chain's next block.
fn send(chain: &mut Chain, nonce: u64, gas: u64, words: &[U256]) -> TxExecutionResult {
    let (transaction, sender) = sent(nonce, gas, words);
    chain.run(sender, transaction)
}

/// The words a test expects logged.
fn values(values: &[u64]) -> Vec<U256> {
    values.iter().map(|value| U256::from(*value)).collect()
}

/// A `bytes32` of rskj's cases: these bytes, then zeros.
fn bytes32(bytes: &[u8]) -> U256 {
    let mut word = [0u8; 32];
    word[..bytes.len()].copy_from_slice(bytes);
    U256::from_be_bytes(word)
}

/// PUSH32 of `word`.
fn push(word: U256) -> String {
    format!("PUSH32 {}", hex_word(word))
}

/// `op` (CALL, CALLCODE, DELEGATECALL or STATICCALL) of the contract the
/// code `to` pushes, with the gas the code `gas` pushes, passing the word at
/// 0x80 as its input and keeping a word of its output at 0xa0. Leaves the
/// call's success on the stack.
fn calling(op: &str, to: &str, gas: &str) -> String {
    // CALL and CALLCODE take a value too: none.
    let value = match op {
        "CALL" | "CALLCODE" => "PUSH1 0x00",
        _ => "",
    };
    format!("PUSH1 0x20 PUSH1 0xa0 PUSH1 0x20 PUSH1 0x80 {value} {to} {gas} {op}")
}

/// Init code that runs `constructor`, then returns `runtime` as the code it
/// deploys.
fn init(constructor: &str, runtime: &[u8]) -> Vec<u8> {
    let constructor = compile(constructor);
    // What copies and returns the runtime is 15 bytes.
    let at = constructor.len() + 15;
    let len = runtime.len();
    let returning = compile(&format!(
        "PUSH2 0x{len:04x} PUSH2 0x{at:04x} PUSH1 0x00 CODECOPY PUSH2 0x{len:04x} PUSH1 0x00 RETURN"
    ));
    [constructor, returning, runtime.to_vec()].concat()
}

/// rskj's `TestTransientStorage`, by the first word of its input. 1 TSTOREs
/// the third word at the second as key, and logs TLOAD of the key; 2 logs
/// TLOAD of the key; 3 and 4 TSTORE as 1 does, then 3 calls this contract
/// and 4 calls [`OTHER`] with 2 and the key.
fn basic() -> Vec<u8> {
    compile(&format!(
        "PUSH1 0x00 CALLDATALOAD
         DUP1 PUSH1 0x02 EQ @load JUMPI
         PUSH1 0x40 CALLDATALOAD PUSH1 0x20 CALLDATALOAD TSTORE
         DUP1 PUSH1 0x01 EQ @load JUMPI
         PUSH1 0x02 PUSH1 0x80 MSTORE PUSH1 0x20 CALLDATALOAD PUSH1 0xa0 MSTORE
         PUSH1 0x03 EQ @itself JUMPI
         {other} @calling JUMP
         itself: ADDRESS
         calling: PUSH1 0x00 DUP1 PUSH1 0x40 PUSH1 0x80 PUSH1 0x00 DUP6 GAS CALL STOP
         load: PUSH1 0x20 CALLDATALOAD TLOAD {LOG_TOP} STOP",
        other = push_address(OTHER),
    ))
}

/// RSKIP446 (lovell700): TLOAD of a key reads what TSTORE stored there in
/// the same frame.
#[test]
fn tload_reads_what_tstore_stored_at_the_key() {
    let mut chain = chain_with(&[(CONTRACT, &basic())]);
    let cases = [(b"1", &b"2"[..]), (b"2", &b"12345"[..])];
    for (nonce, (key, value)) in (0..).zip(cases) {
        let (key, value) = (bytes32(key), bytes32(value));
        let ran = send(&mut chain, nonce, 200_000, &[U256::from(1), key, value]);
        let test = "testTransientStorageOpcodesExecutionsWithRSKIPActivated";
        assert!(ran.success, "{test}");
        assert_eq!(logged(&ran), [value], "{test}");
    }
}

/// A call a contract makes to itself reads what the contract stored before
/// the call, in the same transaction; rskj's case of it checks a TSTORE and
/// a TLOAD of its key and value in one frame.
#[test]
fn a_contracts_call_to_itself_reads_what_it_stored_before_the_call() {
    let (key, value) = (bytes32(b"10"), bytes32(b"20"));
    for selector in [1, 3] {
        let mut chain = chain_with(&[(CONTRACT, &basic())]);
        let input = [U256::from(selector), key, value];
        let ran = send(&mut chain, 0, 200_000, &input);
        let test = "testTransientStorageOpcodesShareMemorySameTransaction";
        assert!(ran.success, "{test}: {selector}");
        assert_eq!(logged(&ran), [value], "{test}: {selector}");
    }
}

/// Another contract reads none of what a contract stored: transient storage
/// is each address's own (rskj's `ERROR` event of the value loaded, zero).
#[test]
fn another_contract_reads_none_of_what_a_contract_stored() {
    let mut chain = chain_with(&[(CONTRACT, &basic()), (OTHER, &basic())]);
    let input = [U256::from(4), bytes32(b"10"), bytes32(b"20")];
    let ran = send(&mut chain, 0, 200_000, &input);
    let test = "testTransientStorageOpcodesDoesntShareMemoryFromOtherContract";
    assert!(ran.success, "{test}");
    assert_eq!(logged(&ran), [U256::ZERO], "{test}");
}

/// Before RSKIP446 TLOAD and TSTORE are no opcodes, and the call fails with
/// all of its gas.
#[test]
fn tload_and_tstore_are_invalid_opcodes_before_rskip446() {
    let input = [U256::from(1), bytes32(b"1"), bytes32(b"2")];
    let mut chain = chain_at(LOVELL - 1, &[(CONTRACT, &basic())]);
    let ran = send(&mut chain, 0, 200_000, &input);
    let test = "testTransientStorageOpcodesExecutionFailsWithRSKIPDeactivated";
    assert!(!ran.success, "{test}");
    assert_eq!(ran.gas_used, 200_000, "{test}");
}

/// The four keys of rskj's EIP-1153 cases, and four others.
fn eip1153_keys() -> [(U256, U256); 4] {
    let one = U256::from(1);
    [
        (one, U256::from(3)),
        (U256::from(2), U256::from(4)),
        (one << 128, (one << 128) + one),
        (U256::MAX, U256::MAX - one),
    ]
}

/// rskj's `TestsTransientStorageEip1153BasicScenarios`, by the first word of
/// its input, for each of four keys: 1 SSTOREs 0xff at the key and logs
/// TLOAD of it; 2 TSTOREs the key at itself and logs TLOAD of it; 3 logs
/// TLOAD of it; anything else TSTOREs 0x4d2 at it and logs TLOAD of another
/// key.
fn eip1153_basic() -> Vec<u8> {
    let each = |step: &dyn Fn(String, String) -> String| -> String {
        let keys = eip1153_keys().map(|(key, other)| step(push(key), push(other)));
        keys.join(" ")
    };
    let after_sstore = each(&|k, _| format!("PUSH1 0xff {k} SSTORE {k} TLOAD {LOG_TOP}"));
    let after_tstore = each(&|k, _| format!("{k} {k} TSTORE {k} TLOAD {LOG_TOP}"));
    let unset = each(&|k, _| format!("{k} TLOAD {LOG_TOP}"));
    let other_key = each(&|k, o| format!("PUSH2 0x04d2 {k} TSTORE {o} TLOAD {LOG_TOP}"));
    compile(&format!(
        "PUSH1 0x00 CALLDATALOAD
         DUP1 PUSH1 0x01 EQ @after_sstore JUMPI
         DUP1 PUSH1 0x02 EQ @after_tstore JUMPI
         PUSH1 0x03 EQ @unset JUMPI
         {other_key} STOP
         after_sstore: {after_sstore} STOP
         after_tstore: {after_tstore} STOP
         unset: {unset} STOP"
    ))
}

/// EIP-1153's basic cases, a transaction each, in rskj's order: TLOAD after
/// an SSTORE of the key reads zero; TLOAD after a TSTORE reads its value; a
/// key in the next transaction reads zero again; and a TSTORE leaves other
/// keys at zero. For keys 1, 2, 2^128 and 2^256 - 1.
#[test]
fn tload_reads_only_what_tstore_stored_in_the_transaction() {
    let mut chain = chain_with(&[(CONTRACT, &eip1153_basic())]);
    let keys: Vec<U256> = eip1153_keys().iter().map(|(key, _)| *key).collect();
    let zeros = vec![U256::ZERO; 4];
    for (nonce, expected) in [(0, &zeros), (1, &keys), (2, &zeros), (3, &zeros)] {
        let ran = send(&mut chain, nonce, 200_000, &[U256::from(nonce + 1)]);
        let test = "testTransientStorageTestsEip1153BasicScenarios";
        assert!(ran.success, "{test}: {nonce}");
        assert_eq!(&logged(&ran), expected, "{test}: {nonce}");
    }
}

/// rskj's `TestStorageCreateContexts`: TSTOREs 2, 3 and 4 at keys 0, 1 and
/// 2; creates a contract from the init code [`HOLDER`] holds as its code;
/// calls it, where `calls_it`; and logs TLOAD of keys 0, 1 and 2.
fn creating(init_len: usize, calls_it: bool) -> Vec<u8> {
    // A CALL of the new contract, with all the gas left and no input.
    let call_it = "PUSH1 0x00 DUP1 DUP1 DUP1 DUP1 DUP6 GAS CALL POP";
    let calls = if calls_it { call_it } else { "" };
    compile(&format!(
        "PUSH1 0x02 PUSH1 0x00 TSTORE PUSH1 0x03 PUSH1 0x01 TSTORE PUSH1 0x04 PUSH1 0x02 TSTORE
         PUSH2 0x{init_len:04x} PUSH1 0x00 PUSH2 0x0100 {holder} EXTCODECOPY
         PUSH2 0x{init_len:04x} PUSH2 0x0100 PUSH1 0x00 CREATE
         {calls} POP
         PUSH1 0x00 TLOAD {LOG_TOP} PUSH1 0x01 TLOAD {LOG_TOP} PUSH1 0x02 TLOAD {LOG_TOP}",
        holder = push_address(HOLDER),
    ))
}

/// What [`creating`] logs, creating from `init`.
fn created(init: &[u8], calls_it: bool) -> Vec<U256> {
    let creator = creating(init.len(), calls_it);
    let mut chain = chain_with(&[(CONTRACT, &creator), (HOLDER, init)]);
    let ran = send(&mut chain, 0, 1_000_000, &[]);
    assert!(ran.success);
    logged(&ran)
}

/// A constructor has the new contract's transient storage, none of its
/// creator's: it reads nothing at key 0, where its creator stored 2, and its
/// TSTORE at key 1 leaves its creator's 3 there.
#[test]
fn a_constructor_has_the_new_contracts_transient_storage() {
    let constructor = format!(
        "PUSH1 0x00 TLOAD {LOG_TOP} PUSH1 0x02 PUSH1 0x01 TSTORE PUSH1 0x01 TLOAD {LOG_TOP}"
    );
    let logs = created(&init(&constructor, &[]), false);
    assert_eq!(logs, values(&[0, 2, 2, 3, 4]), "testOnlyConstructorCode");
}

/// The new contract's code, called by its creator after the creation, has
/// the new contract's transient storage too.
#[test]
fn the_code_a_constructor_deploys_has_the_new_contracts_transient_storage() {
    let stored = (U256::from(1) << 128) + U256::from(1);
    let runtime = compile(&format!(
        "PUSH1 0x00 TLOAD {LOG_TOP} {} PUSH1 0x01 TSTORE PUSH1 0x01 TLOAD {LOG_TOP} STOP",
        push(stored)
    ));
    let constructor = format!("PUSH1 0x00 TLOAD {LOG_TOP}");
    let logs = created(&init(&constructor, &runtime), true);
    let mut expected = values(&[0, 0]);
    expected.extend([stored]);
    expected.extend(values(&[2, 3, 4]));
    assert_eq!(logs, expected, "testInConstructorAndCode");
}

/// What a constructor stores, the code it deploys reads later in the same
/// transaction: both are the new contract's (rskj's V0, and V1, whose
/// constructor also checks key 0 first).
#[test]
fn the_deployed_code_reads_what_its_constructor_stored_in_the_transaction() {
    let runtime = |stored: u64| {
        compile(&format!(
            "PUSH1 0x00 TLOAD {LOG_TOP} PUSH1 0x01 TLOAD {LOG_TOP}
             PUSH2 0x{stored:04x} PUSH1 0x02 TSTORE PUSH1 0x02 TLOAD {LOG_TOP} STOP"
        ))
    };
    let v0 = init("PUSH2 0x0111 PUSH1 0x01 TSTORE", &runtime(0x222));
    let expected = values(&[0, 0x111, 0x222, 2, 3, 4]);
    let test = "testAccrossConstructorAndCodeV0CreateContext";
    assert_eq!(created(&v0, true), expected, "{test}");
    let constructor = format!("PUSH1 0x00 TLOAD {LOG_TOP} PUSH2 0x0333 PUSH1 0x01 TSTORE");
    let v1 = init(&constructor, &runtime(0x444));
    let expected = values(&[0, 0, 0x333, 0x444, 2, 3, 4]);
    let test = "testAccrossConstructorAndCodeV1CreateContext";
    assert_eq!(created(&v1, true), expected, "{test}");
}

/// rskj's case of no constructor code: the deployed code alone, called by
/// its creator, reads nothing at key 0 and reads back its own TSTORE.
#[test]
fn code_deployed_by_an_empty_constructor_has_the_new_contracts_transient_storage() {
    let runtime = compile(&format!(
        "PUSH1 0x00 TLOAD {LOG_TOP} PUSH2 0x0555 PUSH1 0x01 TSTORE PUSH1 0x01 TLOAD {LOG_TOP} STOP"
    ));
    let logs = created(&init("", &runtime), true);
    assert_eq!(logs, values(&[0, 0x555, 2, 3, 4]), "testNoConstructorCode");
}

/// rskj's caller in its cases of the dynamic contexts: TSTOREs 420 at key 0,
/// and at key 1 too where `both`; calls [`CALLEE`] with `op` and the gas the
/// code `gas` pushes; logs the call's success; then runs `then`.
fn context_caller(op: &str, gas: &str, both: bool, then: &str) -> Vec<u8> {
    let second = if both {
        "PUSH2 0x01a4 PUSH1 0x01 TSTORE"
    } else {
        ""
    };
    let called = calling(op, &push_address(CALLEE), gas);
    compile(&format!(
        "PUSH2 0x01a4 PUSH1 0x00 TSTORE {second} {called} {LOG_TOP} {then}"
    ))
}

/// What `caller` logs, calling [`CALLEE`] whose code is `callee`, from a
/// transaction of `gas`; and the chain after it.
fn context_logs(caller: &[u8], callee: &str, gas: u64) -> (Chain, Vec<U256>) {
    let callee = compile(callee);
    let mut chain = chain_with(&[(CONTRACT, caller), (CALLEE, &callee)]);
    let ran = send(&mut chain, 0, gas, &[]);
    assert!(ran.success);
    let logs = logged(&ran);
    (chain, logs)
}

/// CALLCODE and DELEGATECALL run the callee in its caller's transient
/// storage, as in its caller's storage: the callee reads the caller's 420 at
/// key 0 and stores it in slot 2, and its TSTORE of 69 at key 1 is the
/// caller's, which it reads back into slot 3.
#[test]
fn callcode_and_delegatecall_run_the_callee_in_the_callers_transient_storage() {
    let callee = "PUSH1 0x00 TLOAD PUSH1 0x02 SSTORE PUSH1 0x45 PUSH1 0x01 TSTORE
                  PUSH1 0x01 TLOAD PUSH1 0x03 SSTORE STOP";
    let then = format!(
        "PUSH1 0x00 TLOAD {LOG_TOP} PUSH1 0x02 SLOAD {LOG_TOP}
         PUSH1 0x03 SLOAD {LOG_TOP} PUSH1 0x01 TLOAD {LOG_TOP}"
    );
    for op in ["CALLCODE", "DELEGATECALL"] {
        let caller = context_caller(op, "GAS", false, &then);
        let (_, logs) = context_logs(&caller, callee, 300_000);
        let test = "testDynamicExecutionContextSimpleScenario";
        assert_eq!(logs, values(&[1, 420, 420, 69, 69]), "{test}: {op}");
    }
}

/// A callee that fails takes back its TSTORE of 69 at key 1, called with
/// CALLCODE, DELEGATECALL or CALL and the gas the code `gas` pushes: the
/// call reports failure, and the caller reads its own 420 at keys 0 and 1.
fn failing_callee_takes_back_its_tstore(test: &str, callee: &str, gas: &str) {
    let then = format!("PUSH1 0x00 TLOAD {LOG_TOP} PUSH1 0x01 TLOAD {LOG_TOP}");
    for op in ["CALLCODE", "DELEGATECALL", "CALL"] {
        let caller = context_caller(op, gas, true, &then);
        let (_, logs) = context_logs(&caller, callee, 500_000);
        assert_eq!(logs, values(&[0, 420, 420]), "{test}: {op}");
    }
}

/// A callee that REVERTs takes back its TSTORE.
#[test]
fn a_callee_that_reverts_takes_back_its_tstore() {
    let callee = "PUSH1 0x45 PUSH1 0x01 TSTORE PUSH1 0x00 DUP1 REVERT";
    let test = "testDynamicExecutionContextWithRevert";
    failing_callee_takes_back_its_tstore(test, callee, "GAS");
}

/// A callee that runs INVALID, given 255 gas, takes back its TSTORE.
#[test]
fn a_callee_that_runs_invalid_takes_back_its_tstore() {
    let callee = "PUSH1 0x45 PUSH1 0x01 TSTORE INVALID";
    let test = "testDynamicExecutionContextWithInvalid";
    failing_callee_takes_back_its_tstore(test, callee, "PUSH1 0xff");
}

/// A callee given 0xffff gas, whose calls of itself, each a frame deeper,
/// fail at last for lack of gas, fails in turn and takes back its TSTORE.
#[test]
fn a_callee_failing_deep_in_its_own_calls_takes_back_its_tstore() {
    let callee = format!(
        "PUSH1 0x45 PUSH1 0x01 TSTORE
         PUSH1 0x00 DUP1 DUP1 DUP1 DUP1 {itself} GAS CALL @fine JUMPI
         PUSH1 0x00 DUP1 REVERT
         fine: STOP",
        itself = push_address(CALLEE)
    );
    let test = "testDynamicExecutionContextWithStackOverflow";
    failing_callee_takes_back_its_tstore(test, &callee, "PUSH2 0xffff");
}

/// A CALL runs the callee in its own transient storage: it reads none of its
/// caller's, and its TSTORE is its own. The callee logs the 69 it stored;
/// the caller logs the call's success, its own 420 at key 0, nothing at key
/// 1, and nothing in its slots 0 and 1, where the callee wrote its own.
#[test]
fn a_call_runs_the_callee_in_its_own_transient_storage() {
    let callee = format!(
        "PUSH1 0x00 TLOAD PUSH1 0x00 SSTORE PUSH1 0x45 PUSH1 0x01 TSTORE
         PUSH1 0x01 TLOAD PUSH1 0x01 SSTORE PUSH1 0x01 TLOAD {LOG_TOP} STOP"
    );
    let then = format!(
        "PUSH1 0x00 TLOAD {LOG_TOP} PUSH1 0x01 TLOAD {LOG_TOP}
         PUSH1 0x00 SLOAD {LOG_TOP} PUSH1 0x01 SLOAD {LOG_TOP}"
    );
    let caller = context_caller("CALL", "GAS", false, &then);
    let (chain, logs) = context_logs(&caller, &callee, 1_000_000);
    let test = "testDynamicExecutionCallContextSubcall";
    assert_eq!(logs, values(&[69, 1, 420, 0, 0, 0]), "{test}");
    assert_eq!(chain.storage(CALLEE, 0), U256::ZERO, "{test}");
    assert_eq!(chain.storage(CALLEE, 1), U256::from(69), "{test}");
}

/// A STATICCALL's callee cannot TSTORE: the call fails, and the caller's 420
/// at key 0 is still there.
#[test]
fn a_static_calls_callee_cannot_tstore() {
    let then = format!("PUSH1 0x00 TLOAD {LOG_TOP}");
    let caller = context_caller("STATICCALL", "PUSH2 0xffff", false, &then);
    let (_, logs) = context_logs(&caller, "PUSH1 0x45 PUSH1 0x01 TSTORE STOP", 1_000_000);
    let test = "testDynamicExecutionStaticCallSubcallCantUseTstore";
    assert_eq!(logs, values(&[0, 420]), "{test}");
}

/// rskj's contract for a TSTORE before a REVERT or an INVALID. The first call
/// sets a flag in slot 0, TSTOREs 0x100 at 0xff, logs TLOAD of it, calls
/// itself, and logs the call's success and TLOAD of 0xff after it. The call
/// it makes, finding the flag, TSTOREs 0x101 at 0xff, then REVERTs, or runs
/// INVALID where its input's first word is 1, as the first call then gives
/// it only 0xffff gas.
fn tstore_then_fail() -> Vec<u8> {
    compile(&format!(
        "PUSH1 0x00 SLOAD @inner JUMPI
         PUSH1 0x01 PUSH1 0x00 SSTORE
         PUSH2 0x0100 PUSH1 0xff TSTORE PUSH1 0xff TLOAD {LOG_TOP}
         PUSH1 0x00 CALLDATALOAD PUSH1 0x80 MSTORE
         PUSH1 0x00 CALLDATALOAD @limited JUMPI
         {all} @called JUMP
         limited: {limited}
         called: {LOG_TOP} PUSH1 0xff TLOAD {LOG_TOP} STOP
         inner: PUSH2 0x0101 PUSH1 0xff TSTORE
         PUSH1 0x00 CALLDATALOAD @invalid JUMPI
         PUSH1 0x00 DUP1 REVERT
         invalid: INVALID",
        all = calling("CALL", "ADDRESS", "GAS"),
        limited = calling("CALL", "ADDRESS", "PUSH2 0xffff"),
    ))
}

/// A contract's call to itself that TSTOREs and then REVERTs leaves the
/// contract's value as it was. The next transaction, in the same block,
/// finds the flag the first left and runs the inner branch itself, whose
/// INVALID fails the transaction with all of its gas, as rskj's case has it.
#[test]
fn a_tstore_before_a_revert_has_no_effect() {
    let mut chain = chain_with(&[(CONTRACT, &tstore_then_fail())]);
    let reverting = sent(0, 300_000, &[U256::ZERO]);
    let invalid = sent(1, 1_000_000, &[U256::from(1)]);
    let block = chain.block(&[reverting, invalid]);
    let (reverting, invalid) = (&block.tx_results[0], &block.tx_results[1]);
    let test = "testDynamicReentrancyContextsTstoreBeforeRevertOrInvalidHasNoEffect";
    assert!(reverting.success, "{test}");
    assert_eq!(logged(reverting), values(&[0x100, 0, 0x100]), "{test}");
    assert!(!invalid.success, "{test}");
    assert_eq!(invalid.gas_used, 1_000_000, "{test}");
}

/// rskj's contract for a REVERT or an INVALID that undoes all. The first call
/// sets a flag, TSTOREs 0x100 at 0xfe and 0x101 at 0xff, logs TLOAD of both,
/// calls itself, and logs the call's success and TLOAD of both after it. The
/// call it makes, finding the flag, TSTOREs at both twice, then REVERTs, or
/// runs INVALID where its input's first word is 1.
fn undoing_all() -> Vec<u8> {
    compile(&format!(
        "PUSH1 0x00 SLOAD @inner JUMPI
         PUSH1 0x01 PUSH1 0x00 SSTORE
         PUSH2 0x0100 PUSH1 0xfe TSTORE PUSH2 0x0101 PUSH1 0xff TSTORE
         PUSH1 0xfe TLOAD {LOG_TOP} PUSH1 0xff TLOAD {LOG_TOP}
         PUSH1 0x00 CALLDATALOAD PUSH1 0x80 MSTORE
         {all} {LOG_TOP} PUSH1 0xfe TLOAD {LOG_TOP} PUSH1 0xff TLOAD {LOG_TOP} STOP
         inner: PUSH2 0x0201 PUSH1 0xfe TSTORE PUSH2 0x0202 PUSH1 0xfe TSTORE
         PUSH2 0x0201 PUSH1 0xff TSTORE PUSH2 0x0202 PUSH1 0xff TSTORE
         PUSH1 0x00 CALLDATALOAD @invalid JUMPI
         PUSH1 0x00 DUP1 REVERT
         invalid: INVALID",
        all = calling("CALL", "ADDRESS", "GAS"),
    ))
}

/// A REVERT takes back every TSTORE of its frame: the contract reads its own
/// 0x100 and 0x101 after its call to itself stored others twice and
/// reverted. The next transaction finds the flag and fails with INVALID.
#[test]
fn a_revert_takes_back_every_tstore_of_its_frame() {
    let mut chain = chain_with(&[(CONTRACT, &undoing_all())]);
    let test = "testDynamicReentrancyContextsRevertOrInvalidUndoesAll";
    let ran = send(&mut chain, 0, 300_000, &[U256::ZERO]);
    assert!(ran.success, "{test}");
    let expected = values(&[0x100, 0x101, 0, 0x100, 0x101]);
    assert_eq!(logged(&ran), expected, "{test}");
    let ran = send(&mut chain, 1, 1_000_000, &[U256::from(1)]);
    assert!(!ran.success, "{test}");
}

/// rskj's contract for a REVERT or an INVALID that undoes a TSTORE made in a
/// call that succeeded, by a counter in slot 0. At 0 it counts 1, TSTOREs
/// 0x100 at 0xff, keeps TLOAD of it in slot 1, calls itself, and keeps the
/// call's success in slot 3 and TLOAD of 0xff in slot 2. At 1 it counts 2,
/// calls itself, keeps that call's success in slot 4, then REVERTs, or runs
/// INVALID where its input's first word is 1. At 2 it TSTOREs 0x101 at 0xff.
fn undoing_a_successful_call() -> Vec<u8> {
    compile(&format!(
        "PUSH1 0x00 SLOAD
         DUP1 PUSH1 0x01 EQ @second JUMPI
         DUP1 PUSH1 0x02 EQ @third JUMPI
         PUSH1 0x01 ADD PUSH1 0x00 SSTORE
         PUSH2 0x0100 PUSH1 0xff TSTORE PUSH1 0xff TLOAD PUSH1 0x01 SSTORE
         PUSH1 0x00 CALLDATALOAD PUSH1 0x80 MSTORE
         {all} PUSH1 0x03 SSTORE PUSH1 0xff TLOAD PUSH1 0x02 SSTORE STOP
         second: POP PUSH1 0x02 PUSH1 0x00 SSTORE
         PUSH1 0x00 CALLDATALOAD PUSH1 0x80 MSTORE
         PUSH1 0x00 CALLDATALOAD @invalid JUMPI
         {all} PUSH1 0x04 SSTORE PUSH1 0x20 PUSH1 0x00 REVERT
         invalid: {limited} PUSH1 0x04 SSTORE INVALID
         third: PUSH2 0x0101 PUSH1 0xff TSTORE STOP",
        all = calling("CALL", "ADDRESS", "GAS"),
        limited = calling("CALL", "ADDRESS", "PUSH2 0xffff"),
    ))
}

/// A frame that REVERTs takes back the TSTORE of a call it made that
/// succeeded: the first frame reads its own 0x100 at 0xff after its call,
/// which failed, and the second call's success is taken back with the rest.
/// The next transaction finds the counter at 1 and fails with INVALID,
/// leaving the slots as the first left them.
#[test]
fn a_revert_takes_back_the_tstore_of_a_call_that_succeeded() {
    let test = "testDynamicReentrancyContextsRevertOrInvalidUndoesTstorageAfterSuccessfullCall";
    let mut chain = chain_with(&[(CONTRACT, &undoing_a_successful_call())]);
    // TLOAD before the call and after it, and each call's success.
    let kept = |chain: &Chain| {
        let slots = (1..=4).map(|slot| chain.storage(CONTRACT, slot));
        slots.collect::<Vec<_>>()
    };
    let ran = send(&mut chain, 0, 300_000, &[U256::ZERO]);
    assert!(ran.success, "{test}");
    assert_eq!(kept(&chain), values(&[0x100, 0x100, 0, 0]), "{test}");
    let ran = send(&mut chain, 1, 300_000, &[U256::from(1)]);
    assert!(!ran.success, "{test}");
    assert_eq!(kept(&chain), values(&[0x100, 0x100, 0, 0]), "{test}");
}

/// rskj's contracts for reentrancy: the first call sets a flag in slot 0,
/// TSTOREs 0x100 at the key `key` pushes and keeps TLOAD of it in slot 3,
/// calls itself, and keeps the call's success in slot 1 and TLOAD of the key
/// in slot 2. The call it makes, finding the flag, runs `inner`.
fn reentering(key: &str, inner: &str) -> Vec<u8> {
    compile(&format!(
        "PUSH1 0x00 SLOAD @inner JUMPI
         PUSH1 0x01 PUSH1 0x00 SSTORE
         PUSH2 0x0100 {key} TSTORE {key} TLOAD PUSH1 0x03 SSTORE
         {all} PUSH1 0x01 SSTORE {key} TLOAD PUSH1 0x02 SSTORE STOP
         inner: {inner} STOP",
        all = calling("CALL", "ADDRESS", "GAS"),
    ))
}

/// Slots 1 to 4 of [`reentering`] after a transaction runs it.
fn reentered(key: &str, inner: &str) -> Vec<U256> {
    let mut chain = chain_with(&[(CONTRACT, &reentering(key, inner))]);
    let ran = send(&mut chain, 0, 300_000, &[]);
    assert!(ran.success);
    (1..=4).map(|slot| chain.storage(CONTRACT, slot)).collect()
}

/// A contract's call to itself reads what the contract stored before it (in
/// slot 4): the call's success, then TLOAD after it, before it, and in it.
#[test]
fn a_reentrant_call_reads_what_the_contract_stored_before_it() {
    let slots = reentered("PUSH1 0x00", "PUSH1 0x00 TLOAD PUSH1 0x04 SSTORE");
    let test = "testReentrancyContextsTstoreAfterReentrantCall";
    assert_eq!(slots, values(&[1, 0x100, 0x100, 0x100]), "{test}");
}

/// After its call to itself, a contract reads what the call stored.
#[test]
fn a_contract_reads_what_its_reentrant_call_stored() {
    let slots = reentered("PUSH1 0xff", "PUSH2 0x0101 PUSH1 0xff TSTORE");
    let test = "testReentrancyContextsTloadAfterReentrantTstore";
    assert_eq!(slots, values(&[1, 0x101, 0x100, 0]), "{test}");
}

/// A contract's call to itself overwrites what the contract stored, reads
/// its own value back (in slot 4), and the contract reads it after.
#[test]
fn a_reentrant_call_overwrites_what_the_contract_stored() {
    let inner = "PUSH2 0x0101 PUSH1 0xff TSTORE PUSH1 0xff TLOAD PUSH1 0x04 SSTORE";
    let slots = reentered("PUSH1 0xff", inner);
    let test = "testReentrancyContextsManipulateInReentrantCall";
    assert_eq!(slots, values(&[1, 0x101, 0x100, 0x101]), "{test}");
}

/// rskj's contract for a TSTORE in a call and a TLOAD returned in a static
/// call, by a counter in slot 0. At 0 it counts 1, TSTOREs 0x100 at 0xff,
/// keeps TLOAD of it in slot 1, calls itself, and keeps the call's success
/// in slot 4 and TLOAD of 0xfe in slot 2. At 1 it counts 2, TSTOREs 0x101 at
/// 0xfe, STATICCALLs itself, and keeps that call's success in slot 5 and
/// the word it returned in slot 3. At 2 it returns TLOAD of 0xfe.
fn static_reader() -> Vec<u8> {
    compile(&format!(
        "PUSH1 0x00 SLOAD
         DUP1 PUSH1 0x01 EQ @second JUMPI
         DUP1 PUSH1 0x02 EQ @third JUMPI
         PUSH1 0x01 ADD PUSH1 0x00 SSTORE
         PUSH2 0x0100 PUSH1 0xff TSTORE PUSH1 0xff TLOAD PUSH1 0x01 SSTORE
         {all} PUSH1 0x04 SSTORE PUSH1 0xfe TLOAD PUSH1 0x02 SSTORE STOP
         second: POP PUSH1 0x02 PUSH1 0x00 SSTORE
         PUSH2 0x0101 PUSH1 0xfe TSTORE
         {statically} PUSH1 0x05 SSTORE PUSH1 0xa0 MLOAD PUSH1 0x03 SSTORE STOP
         third: PUSH1 0xfe TLOAD PUSH1 0x00 MSTORE PUSH1 0x20 PUSH1 0x00 RETURN",
        all = calling("CALL", "ADDRESS", "GAS"),
        statically = calling("STATICCALL", "ADDRESS", "GAS"),
    ))
}

/// A static call of the contract reads, and returns, what a call of it
/// stored, and the contract reads that value after the call returns.
#[test]
fn a_static_call_returns_what_a_call_stored_and_the_caller_reads_it_after() {
    let mut chain = chain_with(&[(CONTRACT, &static_reader())]);
    let ran = send(&mut chain, 0, 300_000, &[]);
    let test = "testReentrancyContextsTstoreInCallThenTloadReturnInStaticCall";
    assert!(ran.success, "{test}");
    let slots: Vec<U256> = (1..=5).map(|slot| chain.storage(CONTRACT, slot)).collect();
    // Before, 0xfe after the call, what the static call returned, and both
    // calls' success.
    assert_eq!(slots, values(&[0x100, 0x101, 0x101, 1, 1]), "{test}");
}

/// TLOAD and TSTORE cost 100 gas each, whether or not the key was written
/// before in the transaction. Each figure is the gas between two GASes.
#[test]
fn tload_and_tstore_cost_100_gas_whether_or_not_the_key_was_written() {
    let measures = [
        "PUSH1 0x10 TLOAD POP",
        "PUSH1 0x10 PUSH1 0x10 TSTORE PUSH1 0x10 TLOAD POP",
        "PUSH1 0x10 PUSH1 0xff TSTORE",
        "PUSH1 0x10 PUSH1 0xff TSTORE PUSH1 0x11 PUSH1 0xff TSTORE",
    ];
    let code = measures.map(|code| format!("GAS {code} GAS SWAP1 SUB {LOG_TOP}"));
    let mut chain = chain_with(&[(CONTRACT, &compile(&code.join(" ")))]);
    let ran = send(&mut chain, 0, 350_000, &[]);
    let test = "testTransientStorageGasMeasureTests";
    assert!(ran.success, "{test}");
    // The PUSH1s at 3, the POPs and the second GAS at 2, and each TLOAD and
    // TSTORE at 100: the TSTORE of a key just written as the first.
    let expected = [
        3 + 100 + 2 + 2,
        3 + 3 + 100 + 3 + 100 + 2 + 2,
        3 + 3 + 100 + 2,
        3 + 3 + 100 + 3 + 3 + 100 + 2,
    ];
    assert_eq!(logged(&ran), values(&expected), "{test}");
}

/// TSTOREs in a loop run out of gas, and the transaction fails with all of
/// it: at a new key each time, at a key 2^codesize past the gas left, and
/// with a TLOAD of each key after its TSTORE.
#[test]
fn a_loop_of_tstores_runs_out_of_gas_and_fails_with_all_of_it() {
    for (test, body, gas) in [
        ("testTstoreLoopUntilOutOfGas", "GAS GAS TSTORE", 300_000),
        (
            "testTstoreWideAddressSpaceLoopUntilOutOfGas",
            "GAS GAS PUSH1 0x01 CODESIZE SHL ADD TSTORE",
            500_000,
        ),
        (
            "testTstoreAndTloadLoopUntilOutOfGas",
            "GAS DUP1 GAS SWAP1 TSTORE TLOAD POP",
            700_000,
        ),
    ] {
        let looping = compile(&format!("again: {body} @again JUMP"));
        let mut chain = chain_with(&[(CONTRACT, &looping)]);
        let ran = send(&mut chain, 0, gas, &[]);
        assert!(!ran.success, "{test}");
        assert_eq!(ran.gas_used, gas, "{test}");
    }
}

/// rskj's tester of SELFDESTRUCT: calls the callee with each of `options` as
/// the first word of its input, logging each call's success, then logs the
/// word the last call returned. The callee is [`CALLEE`], or, where
/// `created` is the length of the init code [`HOLDER`] holds, one this
/// contract creates from it first.
fn destruction_tester(options: &[u8], created: Option<usize>) -> Vec<u8> {
    let (create, callee) = match created {
        Some(len) => (
            format!(
                "PUSH2 0x{len:04x} PUSH1 0x00 PUSH2 0x0100 {holder} EXTCODECOPY
                 PUSH2 0x{len:04x} PUSH2 0x0100 PUSH1 0x00 CREATE PUSH1 0x40 MSTORE",
                holder = push_address(HOLDER)
            ),
            "PUSH1 0x40 MLOAD".to_string(),
        ),
        None => (String::new(), push_address(CALLEE)),
    };
    let called = calling("CALL", &callee, "GAS");
    let calls: Vec<String> = options
        .iter()
        .map(|option| format!("PUSH1 0x{option:02x} PUSH1 0x80 MSTORE {called} {LOG_TOP}"))
        .collect();
    let calls = calls.join(" ");
    compile(&format!("{create} {calls} PUSH1 0xa0 MLOAD {LOG_TOP}"))
}

/// Transient storage outlives a SELFDESTRUCT in the same transaction, for a
/// contract that was there before the transaction and for one it created:
/// what the contract stored before it destroyed itself, or after, or what it
/// stored before a call of itself that destroyed it, it still reads.
#[test]
fn transient_storage_outlives_a_selfdestruct_in_the_same_transaction() {
    let tload = "PUSH1 0x00 CALLDATALOAD PUSH1 0x01 EQ @destroy JUMPI
        PUSH1 0xff TLOAD PUSH1 0x00 MSTORE PUSH1 0x20 PUSH1 0x00 RETURN
        destroy: PUSH2 0x0100 PUSH1 0xff TSTORE PUSH1 0x00 SELFDESTRUCT"
        .to_string();
    let tload_inner = format!(
        "PUSH1 0x00 CALLDATALOAD PUSH1 0x01 EQ @first JUMPI
         PUSH1 0x00 SELFDESTRUCT
         first: PUSH2 0x0100 PUSH1 0xff TSTORE
         PUSH1 0x02 PUSH1 0x80 MSTORE {itself} POP
         PUSH1 0xff TLOAD PUSH1 0x00 MSTORE PUSH1 0x20 PUSH1 0x00 RETURN",
        itself = calling("CALL", "ADDRESS", "GAS")
    );
    let tstore = "PUSH1 0x00 CALLDATALOAD DUP1 PUSH1 0x01 EQ @destroy JUMPI
        PUSH1 0x02 EQ @store JUMPI
        PUSH1 0xff TLOAD PUSH1 0x00 MSTORE PUSH1 0x20 PUSH1 0x00 RETURN
        destroy: PUSH1 0x00 SELFDESTRUCT
        store: PUSH2 0x0100 PUSH1 0xff TSTORE STOP"
        .to_string();
    let cases: [(&str, String, &[u8]); 3] = [
        ("tload_after_selfdestruct", tload, &[1, 2]),
        ("tload_after_inner_selfdestruct", tload_inner, &[1]),
        ("tstore_after_selfdestruct", tstore, &[1, 2, 3]),
    ];
    for (case, callee, options) in cases {
        // Each call succeeds, and the last returns the 0x100 stored.
        let mut expected = vec![U256::from(1); options.len()];
        expected.push(U256::from(0x100));
        let callee = compile(&callee);
        let tester = destruction_tester(options, None);
        let mut chain = chain_with(&[(CONTRACT, &tester), (CALLEE, &callee)]);
        let ran = send(&mut chain, 0, 300_000, &[]);
        assert!(ran.success, "{case}_pre_existing_contract");
        assert_eq!(logged(&ran), expected, "{case}_pre_existing_contract");
        let init = init("", &callee);
        let tester = destruction_tester(options, Some(init.len()));
        let mut chain = chain_with(&[(CONTRACT, &tester), (HOLDER, &init)]);
        let ran = send(&mut chain, 0, 300_000, &[]);
        assert!(ran.success, "{case}_new_contract");
        assert_eq!(logged(&ran), expected, "{case}_new_contract");
    }
}

/// Transient storage is empty again for the block's next transaction: the
/// first TSTOREs 0x64 at 0xff and reads it back, the second reads zero.
#[test]
fn transient_storage_is_empty_again_for_the_blocks_next_transaction() {
    let mut chain = chain_with(&[(CONTRACT, &basic())]);
    let (key, value) = (U256::from(0xff), U256::from(0x64));
    let store = sent(0, 200_000, &[U256::from(1), key, value]);
    let load = sent(1, 200_000, &[U256::from(2), key]);
    let block = chain.block(&[store, load]);
    let test = "testTloadTstoreCheckingBetweenTransactions";
    assert!(block.tx_results.iter().all(|ran| ran.success), "{test}");
    assert_eq!(logged(&block.tx_results[0]), values(&[0x64]), "{test}");
    assert_eq!(logged(&block.tx_results[1]), values(&[0]), "{test}");
}
