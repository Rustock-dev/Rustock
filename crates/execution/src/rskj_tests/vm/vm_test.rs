//! rskj's `org.ethereum.vm.VMTest`: the return data a call that runs no code
//! leaves its caller (RSKIP171), and the refusals, limits and error paths of
//! the rest of the class. rskj steps each program on a mocked program; the
//! port runs it as a contract's code in a block of mainnet.

use alloy_primitives::{hex, Address, U256};

use super::harness::{
    call, compile, push_address, return_top, run_code, words, Chain, CONTRACT, GAS, ONE_RBTC,
    SENDER,
};
use crate::executor::TxExecutionResult;
use crate::precompiles::BLOCK_HEADER_ADDR;

/// orchid060 on mainnet: RSKIP103.
const ORCHID060: u64 = 1_052_700;
/// iris300 on mainnet: RSKIP171.
const IRIS: u64 = 3_614_800;
/// rskj's mock contract, which the cases of the return data call first.
const RETURNER: Address = Address::repeat_byte(0x2e);
/// The contract a case's contract calls.
const CALLEE: Address = Address::repeat_byte(0xce);
/// The identity precompiled contract.
const IDENTITY: Address = Address::with_last_byte(4);
/// The gas rskj's cases ask for a call: 6,000,000, more than is left.
const CALL_GAS: &str = "PUSH4 0x005b8d80";

/// What a transaction of [`GAS`] gets from calling [`CONTRACT`], whose code
/// is `code` and which holds 1,000 wei, in block `number` of mainnet, with
/// the contracts `others` there too.
fn run_with(number: u64, code: &[u8], others: &[(Address, &[u8])]) -> TxExecutionResult {
    let mut chain = Chain::mainnet(number);
    chain.put(SENDER, 0, ONE_RBTC, &[]);
    chain.put(CONTRACT, 1, U256::from(1_000), code);
    for (address, code) in others {
        chain.put(*address, 1, U256::ZERO, code);
    }
    chain.run(SENDER, call(0, CONTRACT, U256::ZERO, GAS, &[]))
}

/// [`run_with`], [`RETURNER`] there, a program that STATICCALLs it, which
/// leaves the word 0x15 (0x10 + 0x05) as the return data and at 0x40, then
/// runs `then` and ends with `end`.
fn after_a_word_returned(number: u64, then: &str, end: &[u8]) -> TxExecutionResult {
    let returner =
        compile("PUSH1 0x10 PUSH1 0x05 ADD PUSH1 0x40 MSTORE PUSH1 0x20 PUSH1 0x40 RETURN");
    let first = format!(
        "PUSH1 0x20 PUSH1 0x40 PUSH1 0x00 PUSH1 0x00 {} {CALL_GAS} STATICCALL",
        push_address(RETURNER)
    );
    let code = [compile(&format!("{first} {then}")), end.to_vec()].concat();
    run_with(number, &code, &[(RETURNER, &returner)])
}

/// From RSKIP171 a STATICCALL of an account with no code (address 0) leaves
/// its caller no return data: RETURNDATASIZE reads 0, and RETURNDATACOPY of
/// a word fails, with all the gas. Before it the caller keeps the call
/// before's 32 bytes, the word 0x15.
#[test]
fn a_static_call_of_no_code_leaves_no_return_data_from_rskip171() {
    let nothing =
        format!("PUSH1 0x20 PUSH1 0x40 PUSH1 0x00 PUSH1 0x00 PUSH1 0x00 {CALL_GAS} STATICCALL");
    let size = format!("{nothing} RETURNDATASIZE");
    let copy = format!("{nothing} PUSH1 0x20 PUSH1 0x00 PUSH1 0x40 RETURNDATACOPY");
    let copied = compile("PUSH1 0x20 PUSH1 0x40 RETURN");

    let ran = after_a_word_returned(IRIS - 1, &size, &return_top(1));
    let test = "beforeIrisReturnDataSizeAfterCallToNonExistentContract";
    assert_eq!(words(&ran.output), [U256::from(32)], "{test}");
    let ran = after_a_word_returned(IRIS - 1, &copy, &copied);
    let test = "beforeIrisReturnDataBufferAfterCallToNonExistentContract";
    assert_eq!(words(&ran.output), [U256::from(0x15)], "{test}");

    let ran = after_a_word_returned(IRIS, &size, &return_top(1));
    let test = "returnDataSizeAfterCallToNonExistentContract";
    assert_eq!(words(&ran.output), [U256::ZERO], "{test}");
    let ran = after_a_word_returned(IRIS, &copy, &copied);
    let test = "returnDataBufferAfterCallToNonExistentContract";
    assert!(!ran.success, "{test}");
    assert_eq!(ran.gas_used, GAS, "{test}");
}

/// A CALL whose value its caller cannot pay runs nothing. From RSKIP171 it
/// leaves the caller no return data, and before it the call before's 32
/// bytes, whether the callee is a contract or a precompiled contract (the
/// block header contract, RSKIP119's).
#[test]
fn a_call_its_caller_cannot_pay_leaves_no_return_data_from_rskip171() {
    for (callee, case) in [
        (RETURNER, "InsufficientFunds"),
        (BLOCK_HEADER_ADDR, "InsufficientFundsAtPrecompiled"),
    ] {
        // 0xffffffff wei, of the 1,000 the caller holds.
        let to = push_address(callee);
        let unpaid = format!(
            "PUSH1 0x20 PUSH1 0x40 PUSH1 0x00 PUSH1 0x00 PUSH4 0xffffffff {to}
             {CALL_GAS} CALL RETURNDATASIZE"
        );
        let ran = after_a_word_returned(IRIS - 1, &unpaid, &return_top(1));
        let test = format!("beforeIrisReturnDataBufferWith{case}");
        assert_eq!(words(&ran.output), [U256::from(32)], "{test}");
        let ran = after_a_word_returned(IRIS, &unpaid, &return_top(1));
        let test = format!("returnDataBufferAfter{case}");
        assert_eq!(words(&ran.output), [U256::ZERO], "{test}");
    }
}

/// A STATICCALL's callee that would change state fails, and the STATICCALL
/// pushes 0: one that SSTOREs, LOGs with each count of topics, CREATEs or
/// SELFDESTRUCTs, each given the words it takes. The same callee run by a
/// CALL succeeds, and a STATICCALL of a callee that only computes pushes 1.
/// Each call is given 200,000 gas, which a callee that fails spends, so that
/// its caller has gas left to return what the call pushed.
#[test]
fn a_static_calls_callee_that_would_change_state_fails() {
    let mut changing = vec!["PUSH1 0x01 PUSH1 0x02 SSTORE".to_string()];
    let topics = |n: usize| "PUSH1 0x01 ".repeat(n);
    changing.extend((0..5).map(|n| format!("{}PUSH1 0x01 PUSH1 0x02 LOG{n}", topics(n))));
    changing.push("PUSH1 0x00 PUSH1 0x00 PUSH1 0x00 CREATE".to_string());
    changing.push("PUSH1 0x2b SELFDESTRUCT".to_string());
    let callee = push_address(CALLEE);
    let zeros = "PUSH1 0x00 PUSH1 0x00 PUSH1 0x00 PUSH1 0x00";
    let statically = compile(&format!("{zeros} {callee} PUSH3 0x030d40 STATICCALL"));
    let statically = [statically, return_top(1)].concat();
    let plainly = compile(&format!("{zeros} PUSH1 0x00 {callee} PUSH3 0x030d40 CALL"));
    let plainly = [plainly, return_top(1)].concat();
    for code in &changing {
        let others = [(CALLEE, &compile(code)[..])];
        let ran = run_with(IRIS, &statically, &others);
        let test = "testSTATICCALLWithStatusZeroUsingOpCode";
        assert_eq!(words(&ran.output), [U256::ZERO], "{test}: {code}");
        let ran = run_with(IRIS, &plainly, &others);
        assert_eq!(words(&ran.output), [U256::from(1)], "{code}, by a CALL");
    }
    let computing = compile("PUSH1 0x01 PUSH1 0x02 SUB");
    let ran = run_with(IRIS, &statically, &[(CALLEE, &computing)]);
    let test = "testSTATICCALLWithStatusOne";
    assert_eq!(words(&ran.output), [U256::from(1)], "{test}");
}

/// Before RSKIP103 (orchid060) STATICCALL takes seven words, a value among
/// them as CALL's: of six it fails for want of the seventh, with all the
/// gas, and of seven it leaves only its success. From RSKIP103 it takes six,
/// and of seven leaves the first under its success.
#[test]
fn staticcall_takes_seven_words_before_rskip103_and_six_from_it() {
    let callee = compile("PUSH1 0x01 PUSH1 0x02 SUB");
    let others = [(CALLEE, &callee[..])];
    let six = format!(
        "PUSH1 0x00 PUSH1 0x00 PUSH1 0x00 PUSH1 0x00 {} {CALL_GAS} STATICCALL",
        push_address(CALLEE)
    );
    let seven = compile(&format!("PUSH1 0x2a {six}"));

    let ran = run_with(ORCHID060 - 1, &compile(&six), &others);
    let test = "testSTATICCALLWithStatusOneFailsWithOldCode";
    assert!(!ran.success, "{test}");
    assert_eq!(ran.gas_used, GAS, "{test}");

    let test = "testSTATICCALLWithStatusOneAndAdditionalValueInStackUsingPreFixStaticCall";
    let one = [seven.clone(), return_top(1)].concat();
    let ran = run_with(ORCHID060 - 1, &one, &others);
    assert_eq!(words(&ran.output), [U256::from(1)], "{test}");
    // Nothing is left under the success to return.
    let two = [seven, return_top(2)].concat();
    let ran = run_with(ORCHID060 - 1, &two, &others);
    assert!(!ran.success, "{test}");

    let ran = run_with(ORCHID060, &two, &others);
    let test = "testSTATICCALLWithStatusOneAndAdditionalValueInStackUsingFixStaticCall";
    let left = [U256::from(1), U256::from(0x2a)];
    assert_eq!(words(&ran.output), left, "{test}");
}

/// A CALL that asks more gas than is left gives the callee all of it, as
/// rskj holds none back (there is no EIP-150 64th): a callee that fails, here
/// for want of the words ADD takes, leaves its caller none, and the
/// transaction uses all of its gas.
#[test]
fn a_call_asking_more_gas_than_is_left_gives_the_callee_all_of_it() {
    let broken = compile("ADD");
    let code = compile(&format!(
        "PUSH1 0x00 PUSH1 0x00 PUSH1 0x00 PUSH1 0x00 PUSH1 0x01 {} PUSH8 0x7fffffffffffffff CALL",
        push_address(CALLEE)
    ));
    let ran = run_with(IRIS, &code, &[(CALLEE, &broken)]);
    assert!(ran.success, "testCALLWithBigUserSpecifiedGas");
    assert_eq!(ran.gas_used, GAS, "testCALLWithBigUserSpecifiedGas");
}

/// A LOG of 2^59 bytes, whose data cost is past the 2^62 - 1 rskj's gas
/// once stopped at, runs out of gas, with all of it.
#[test]
fn a_log_of_more_data_than_its_gas_pays_for_runs_out_of_gas() {
    let ran = run_code(IRIS, &compile("PUSH8 0x0800000000000000 PUSH1 0x00 LOG0"));
    let test = "testLOGWithDataCostBiggerThanPreviousGasSize";
    assert!(!ran.success, "{test}");
    assert_eq!(ran.gas_used, GAS, "{test}");
}

/// A copy of no bytes expands no memory and is charged none. RETURNDATACOPY
/// of none to 0x20 costs 3 gas less than of the word (its copy), the RETURN
/// of 0x20 to 0x40 after it paying the memory either way; CALLDATACOPY of
/// none to the memory's last byte leaves the memory at a word, where of a
/// word it grows to two.
#[test]
fn a_copy_of_no_bytes_expands_no_memory() {
    let one = compile("PUSH1 0x01 PUSH1 0x02 SUB PUSH1 0x00 MSTORE PUSH1 0x20 PUSH1 0x00 RETURN");
    let others = [(CALLEE, &one[..])];
    let callee = push_address(CALLEE);
    let return_data_copy = |len: u8| {
        let code = format!(
            "PUSH1 0x20 PUSH1 0x00 PUSH1 0x00 PUSH1 0x00 {callee} PUSH1 0xff STATICCALL
             PUSH1 0x{len:02x} PUSH1 0x00 PUSH1 0x20 RETURNDATACOPY PUSH1 0x20 PUSH1 0x20 RETURN"
        );
        run_with(IRIS, &compile(&code), &others)
    };
    let (word, none) = (return_data_copy(0x20), return_data_copy(0));
    let test = "testReturnDataCopyChargesCorrectGas";
    assert_eq!(words(&word.output), [U256::from(1)], "{test}");
    assert_eq!(words(&none.output), [U256::ZERO], "{test}");
    assert_eq!(word.gas_used, none.gas_used + 3, "{test}");

    let call_data_copy = |len: u8| {
        let code = format!(
            "PUSH1 0x20 PUSH1 0x00 PUSH1 0x00 PUSH1 0x00 {callee} PUSH2 0xffff STATICCALL
             PUSH1 0x{len:02x} PUSH1 0x00 PUSH1 0x01 MSIZE SUB CALLDATACOPY MSIZE"
        );
        run_with(IRIS, &[compile(&code), return_top(1)].concat(), &others)
    };
    let test = "testCallDataCopyDoesNotExpandMemoryForFree";
    let (none, word) = (call_data_copy(0), call_data_copy(0x20));
    assert_eq!(words(&none.output), [U256::from(32)], "{test}");
    assert_eq!(words(&word.output), [U256::from(64)], "{test}");
}

/// A call of a precompiled contract pays memory only for the room its
/// caller gave the output: identity's output, given no room at 0x20, takes a
/// word less memory than given a word's room there, for 3 gas less; given
/// no room at 0x01000000, it takes none, as given a byte's room at 0.
#[test]
fn a_precompiles_caller_pays_memory_only_for_the_room_it_gave_the_output() {
    let identity = push_address(IDENTITY);
    // A CALL of identity on `input` bytes at 0, with room for `out` bytes of
    // its output where `at` pushes; then MSIZE.
    let calling = |at: &str, out: u8, input: u8| {
        let code = format!(
            "PUSH1 0x00 PUSH1 0x00 PUSH1 0x{out:02x} {at} PUSH1 0x{input:02x} PUSH1 0x00
             PUSH1 0x00 {identity} {CALL_GAS} CALL MSIZE"
        );
        run_with(IRIS, &[compile(&code), return_top(1)].concat(), &[])
    };
    let lying = calling("PUSH1 0x20", 0, 0x01);
    let honest = calling("PUSH1 0x20", 0x20, 0x20);
    let test = "getFreeMemoryUsingPrecompiledContractLyingAboutReturnSize";
    assert_eq!(words(&lying.output), [U256::from(32)], "{test}");
    assert_eq!(words(&honest.output), [U256::from(64)], "{test}");
    assert_eq!(honest.gas_used, lying.gas_used + 3, "{test}");
    let far = calling("PUSH4 0x01000000", 0, 0x01);
    let near = calling("PUSH1 0x00", 0x01, 0x01);
    let test = "getFreeMemoryUsingPrecompiledContractAndSettingFarOffOffset";
    assert_eq!(words(&far.output), [U256::from(32)], "{test}");
    assert_eq!(words(&near.output), [U256::from(32)], "{test}");
    assert_eq!(far.gas_used, near.gas_used, "{test}");
}

/// A program short of the words its opcode takes, jumping where no JUMPDEST
/// is, or running a byte that is no opcode fails, with all of its gas:
/// rskj's StackTooSmallException, BadJumpDestinationException and
/// IllegalOperationException.
#[test]
fn a_program_short_of_words_or_jumping_nowhere_fails_with_all_the_gas() {
    for (test, code) in [
        ("testDUPN_2", "80"),
        ("testSWAPN_2", "90"),
        ("testMSTORE_5", "61123452"),
        ("testMLOAD_6", "51"),
        ("testJUMPI_3", "600157"),
        ("testEXP_4", "621234560a"),
        (
            "testCODECOPY_6",
            "605e6007396000605f556014600054601e60205463abcddcba6040545b51602001600a5254516040\
             016014525451606001601e5254516080016028525460a052546016604860003960166000f26000\
             603f556103e756600054600053602002351234",
        ),
        (
            "testEXTCODECOPY_5",
            "605e600773471fd3ad3e9eeadeec4608b92d16ce6b500704cc3c",
        ),
        ("testJUMP_1", "60aa60bb600e5660cc60dd60ee5b60ff"),
        ("testJUMPI_4", "60016022909057"),
        ("testJUMPDEST_1", "602360085660015b600255"),
        ("testInvalidOpcodes_1", "a5"),
    ] {
        let ran = run_code(IRIS, &hex::decode(code).expect("hex"));
        assert!(!ran.success, "{test}");
        assert_eq!(ran.gas_used, GAS, "{test}");
    }
}
