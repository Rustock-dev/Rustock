//! rskj's `co.rsk.vm.VMExecutionTest`: opcodes on either side of the upgrade
//! that brings them, each run as a contract's code in a block of mainnet.
//!
//! rskj steps each program on a mocked program with the one rule under test
//! switched on or off; here the block's height switches it, at the first
//! block of its upgrade on mainnet and the block before it.

use alloy_primitives::{hex, U256};

use super::harness::{
    call, compile, return_top, run_code, run_code_on, words, Chain, CONTRACT, GAS, ONE_RBTC,
    RETURN_ONE_WORD_GAS, SENDER,
};
use crate::hardfork::RskHardforkConfig;

/// wasabi100 on mainnet: RSKIP120, the shifts.
const WASABI: u64 = 1_591_000;
/// papyrus200 on mainnet: RSKIP151, SELFBALANCE, and RSKIP152, CHAINID.
const PAPYRUS: u64 = 2_392_700;
/// papyrus200 on testnet.
const TESTNET_PAPYRUS: u64 = 863_000;
/// iris300 on mainnet: RSKIP174.
const IRIS: u64 = 3_614_800;
/// arrowhead600 on mainnet: RSKIP398, PUSH0.
const ARROWHEAD: u64 = 6_223_700;
/// lovell700 on mainnet: RSKIP445, MCOPY.
const LOVELL: u64 = 7_338_024;

/// rskj's `executeShift`: `number` shifted by `shift` with `op`, as
/// `PUSH32 number PUSH1 shift op` (PUSH2 for a shift of two bytes), and
/// the word it leaves.
fn shifted(op: &str, number: U256, shift: &str) -> U256 {
    let push = (shift.len() - 2) / 2;
    let number = hex::encode(number.to_be_bytes::<32>());
    let code = compile(&format!("PUSH32 0x{number} PUSH{push} {shift} {op}"));
    let ran = run_code(WASABI, &[code, return_top(1)].concat());
    assert!(ran.success, "{op} {shift} of 0x{number}");
    words(&ran.output)[0]
}

/// 0x00..01, 0x80..00, 0x7f..ff and 0xff..ff, rskj's numbers to shift.
fn numbers() -> (U256, U256, U256, U256) {
    let one = U256::from(1);
    (one, one << 255, U256::MAX >> 1, U256::MAX)
}

/// RSKIP120 (wasabi100): SHL shifts left as EIP-145 has it, by up to 255
/// bits and to zero past them. Each row is one of rskj's tests.
#[test]
fn shl_shifts_left_as_eip145_has_it_from_rskip120() {
    let (one, top, low_255, all) = numbers();
    for (test, number, shift, expected) in [
        ("testSHL1", one, "0x00", one),
        ("testSHL2", one, "0x01", U256::from(2)),
        ("testSHL3", one, "0xff", top),
        ("testSHL4", one, "0x0100", U256::ZERO),
        ("testSHL5", all, "0x00", all),
        ("testSHL6", all, "0xff", top),
        ("testSHL7", low_255, "0x01", all - U256::from(1)),
    ] {
        assert_eq!(shifted("SHL", number, shift), expected, "{test}");
    }
}

/// RSKIP120 (wasabi100): SHR shifts right, filling with zeros.
#[test]
fn shr_shifts_right_as_eip145_has_it_from_rskip120() {
    let (one, top, _, all) = numbers();
    for (test, number, shift, expected) in [
        ("testSHR1", one, "0x00", one),
        ("testSHR2", one, "0x01", U256::ZERO),
        ("testSHR3", top, "0x01", top >> 1),
        ("testSHR4", top, "0xff", one),
        ("testSHR5", all, "0x00", all),
        ("testSHR6", all, "0xff", one),
        ("testSHR7", top, "0x0100", U256::ZERO),
    ] {
        assert_eq!(shifted("SHR", number, shift), expected, "{test}");
    }
}

/// RSKIP120 (wasabi100): SAR shifts right, filling with the sign bit.
#[test]
fn sar_shifts_right_keeping_the_sign_as_eip145_has_it_from_rskip120() {
    let (one, top, low_255, all) = numbers();
    for (test, number, shift, expected) in [
        ("testSAR1", one, "0x00", one),
        ("testSAR2", one, "0x01", U256::ZERO),
        ("testSAR3", top, "0x01", U256::from(3) << 254),
        ("testSAR4", top, "0xff", all),
        ("testSAR5", all, "0x00", all),
        ("testSAR6", all, "0x01", all),
        ("testSAR7", low_255, "0xf8", U256::from(0x7f)),
        ("testSAR8", low_255, "0x0100", U256::ZERO),
    ] {
        assert_eq!(shifted("SAR", number, shift), expected, "{test}");
    }
}

/// Before RSKIP120 the shifts are no opcodes: rskj throws
/// `IllegalOperationException`, and the call fails with all of its gas.
#[test]
fn the_shifts_are_invalid_opcodes_before_rskip120() {
    let all = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    let one = "0x0000000000000000000000000000000000000000000000000000000000000001";
    for (test, code) in [
        (
            "testSAR3ShouldFailOnOldVersion",
            format!("PUSH32 {all} PUSH1 0xff SAR"),
        ),
        (
            "testSHL1ShouldFailOnOldVersion",
            format!("PUSH32 {one} PUSH1 0x01 SHL"),
        ),
        (
            "testSHR1ShouldFailOnOldVersion",
            format!("PUSH32 {one} PUSH1 0x01 SHR"),
        ),
    ] {
        let before = run_code(WASABI - 1, &compile(&code));
        assert!(!before.success, "{test}");
        assert_eq!(before.gas_used, GAS, "{test}: all the gas");
        let from = run_code(WASABI, &compile(&code));
        assert!(from.success, "{test} from RSKIP120");
    }
}

/// RSKIP152 (papyrus200): CHAINID pushes the chain's id, 30 on mainnet, 31
/// on testnet and 33 on a regtest (rskj's `Constants`).
#[test]
fn chainid_pushes_each_networks_chain_id_from_rskip152() {
    let code = [compile("CHAINID"), return_top(1)].concat();
    for (test, config, number, id) in [
        (
            "chainIDMainnet",
            RskHardforkConfig::mainnet(),
            PAPYRUS,
            30u64,
        ),
        (
            "chainIDTestnet",
            RskHardforkConfig::testnet(),
            TESTNET_PAPYRUS,
            31,
        ),
        ("chainIDRegtest", RskHardforkConfig::regtest(), 1, 33),
    ] {
        let ran = run_code_on(config, number, &code);
        assert!(ran.success, "{test}");
        assert_eq!(words(&ran.output), [U256::from(id)], "{test}");
        // BASE tier.
        assert_eq!(ran.gas_used, 21_000 + 2 + RETURN_ONE_WORD_GAS, "{test}");
    }
}

/// Before RSKIP151 SELFBALANCE is no opcode, and the call fails with all of
/// its gas.
#[test]
fn selfbalance_is_an_invalid_opcode_before_rskip151() {
    let code = [compile("SELFBALANCE"), return_top(1)].concat();
    let before = run_code(PAPYRUS - 1, &code);
    assert!(!before.success, "selfBalanceFailsWithRSKIPNotActivated");
    assert_eq!(before.gas_used, GAS);
    assert!(run_code(PAPYRUS, &code).success);
}

/// The output and the gas of a call to a contract holding 100 wei whose code
/// is `code`, from RSKIP151.
fn run_with_100_wei(code: &[u8]) -> (Vec<U256>, u64) {
    let mut chain = Chain::mainnet(PAPYRUS);
    chain.put(SENDER, 0, ONE_RBTC, &[]);
    chain.put(CONTRACT, 1, U256::from(100), code);
    let ran = chain.run(SENDER, call(0, CONTRACT, U256::ZERO, GAS, &[]));
    assert!(ran.success);
    (words(&ran.output), ran.gas_used)
}

/// RSKIP151 (papyrus200): SELFBALANCE pushes the balance of the contract
/// that runs it, for the LOW tier's 5 gas.
#[test]
fn selfbalance_pushes_the_contracts_own_balance_for_5_gas() {
    let (pushed, gas) = run_with_100_wei(&[compile("SELFBALANCE"), return_top(1)].concat());
    assert_eq!(pushed, [U256::from(100)], "selfBalanceRunsCorrectly");
    assert_eq!(
        gas,
        21_000 + 5 + RETURN_ONE_WORD_GAS,
        "selfBalanceRunsCorrectly"
    );
}

/// SELFBALANCE pushes what BALANCE of the contract's own address pushes.
#[test]
fn selfbalance_pushes_what_balance_of_the_contract_pushes() {
    let (selfbalance, gas) = run_with_100_wei(&[compile("SELFBALANCE"), return_top(1)].concat());
    let push_own_address = format!("PUSH20 0x{}", hex::encode(CONTRACT));
    let (balance, _) = run_with_100_wei(
        &[
            compile(&format!("{push_own_address} BALANCE")),
            return_top(1),
        ]
        .concat(),
    );
    assert_eq!(selfbalance, [U256::from(100)], "selfBalanceIsSameAsBalance");
    assert_eq!(balance, selfbalance, "selfBalanceIsSameAsBalance");
    assert_eq!(
        gas,
        21_000 + 5 + RETURN_ONE_WORD_GAS,
        "selfBalanceIsSameAsBalance"
    );
}

/// RSKIP398 (arrowhead600): PUSH0 pushes one word, zero, for the BASE
/// tier's 2 gas: the word pushed before it is the next one down.
#[test]
fn push0_pushes_one_zero_word_from_rskip398() {
    let ran = run_code(
        ARROWHEAD,
        &[compile("PUSH1 0x2a PUSH0"), return_top(2)].concat(),
    );
    assert!(ran.success, "testPUSH0Activation");
    assert_eq!(
        words(&ran.output),
        [U256::ZERO, U256::from(0x2a)],
        "testPUSH0Activation"
    );
    // PUSH1, PUSH0, and three PUSH2s, two MSTOREs and two words of memory.
    assert_eq!(ran.gas_used, 21_000 + 3 + 2 + 24);
}

/// RSKIP445 (lovell700): MCOPY of nothing runs and takes its three words off
/// the stack: the word pushed before them is the top one after it.
#[test]
fn mcopy_takes_its_three_words_off_the_stack_from_rskip445() {
    let code = compile("PUSH1 0x2a PUSH1 0x00 PUSH1 0x00 PUSH1 0x00 MCOPY");
    let ran = run_code(LOVELL, &[code, return_top(1)].concat());
    assert!(ran.success, "testMCOPY_WhenActive_ExecutesAsExpected");
    assert_eq!(
        words(&ran.output),
        [U256::from(0x2a)],
        "testMCOPY_WhenActive_ExecutesAsExpected"
    );
    // Four PUSH1s, and MCOPY's VERY_LOW 3 for no words copied.
    assert_eq!(ran.gas_used, 21_000 + 12 + 3 + RETURN_ONE_WORD_GAS);
}

/// Before RSKIP445 MCOPY is no opcode, and the call fails with all of its
/// gas.
#[test]
fn mcopy_is_an_invalid_opcode_before_rskip445() {
    let ran = run_code(
        LOVELL - 1,
        &compile("PUSH1 0x00 PUSH1 0x00 PUSH1 0x00 MCOPY"),
    );
    assert!(!ran.success, "testMCOPY_WhenInactive_ExecutesAsExpected");
    assert_eq!(
        ran.gas_used, GAS,
        "testMCOPY_WhenInactive_ExecutesAsExpected"
    );
}

/// A JUMP to where no JUMPDEST is fails, with all of the call's gas: rskj's
/// `BadJumpDestinationException`, for a destination just past the code, past
/// it, 2^256 - 1 (rskj's PC[-1]) and 255.
#[test]
fn a_jump_to_where_no_jumpdest_is_fails_with_all_the_gas() {
    let all_ones = format!("0x{}", "ff".repeat(32));
    for (test, code) in [
        ("invalidJustAfterEndOfCode", "PUSH1 0x03 JUMP".to_string()),
        ("invalidJumpOutOfRange", "PUSH1 0x05 JUMP".to_string()),
        ("invalidNegativeJump", format!("PUSH32 {all_ones} JUMP")),
        ("invalidTooFarJump", "PUSH1 0xff JUMP".to_string()),
    ] {
        let ran = run_code(LOVELL, &compile(&code));
        assert!(!ran.success, "{test}");
        assert_eq!(ran.gas_used, GAS, "{test}");
    }
    // The jump that lands on a JUMPDEST runs on (testJumpSkippingInvalidJump).
    let ran = run_code(
        LOVELL,
        &[
            compile("PUSH1 0x05 JUMP PUSH1 0xa0 JUMPDEST PUSH1 0x01"),
            return_top(1),
        ]
        .concat(),
    );
    assert!(ran.success);
    assert_eq!(words(&ran.output), [U256::from(1)]);
}

/// A CREATE from a contract onto an address that already holds a balance
/// keeps that balance, and adds the endowment to it, at every height: rskj's
/// `Program.createContract` creates the account carrying over what it held
/// (`track.createAccount(contractAddress, existingAccount)`), whatever is
/// active. Only a transaction's own creation burned it before RSKIP174.
#[test]
fn a_create_from_a_contract_keeps_the_balance_already_at_the_address_at_every_height() {
    // Store 0x60 (PUSH1) as the init code's one byte, and CREATE with 10 wei.
    let creator = compile(
        "PUSH32 0x6000000000000000000000000000000000000000000000000000000000000000 PUSH1 0x00 MSTORE \
         PUSH1 0x01 PUSH1 0x00 PUSH1 0x0a CREATE",
    );
    let created = CONTRACT.create(1);
    for number in [1, IRIS - 1, IRIS] {
        let mut chain = Chain::mainnet(number);
        chain.put(SENDER, 0, ONE_RBTC, &[]);
        chain.put(
            CONTRACT,
            1,
            U256::from(1_010),
            &[creator.clone(), return_top(1)].concat(),
        );
        chain.put(created, 0, U256::from(100), &[]);
        let ran = chain.run(SENDER, call(0, CONTRACT, U256::ZERO, GAS, &[]));
        assert!(ran.success, "#{number}");
        assert_eq!(
            words(&ran.output),
            [U256::from_be_slice(created.as_slice())],
            "#{number}"
        );
        assert_eq!(chain.balance(created), U256::from(110), "#{number}");
        assert_eq!(chain.balance(CONTRACT), U256::from(1_000), "#{number}");
    }
}
