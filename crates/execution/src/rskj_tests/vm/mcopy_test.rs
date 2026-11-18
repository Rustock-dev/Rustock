//! rskj's `co.rsk.vm.opcode.MCopyGasTest` and `MCopyInputTest`, their tests
//! of RSKIP445 (lovell700): MCOPY costs 3 gas, 3 a word it copies and the
//! memory it expands to the greater of its two ranges, and a copy no gas can
//! pay for fails the program with all its gas.

use alloy_primitives::U256;

use super::harness::{compile, hex_word, run_code, GAS};

/// lovell700 on mainnet: RSKIP445.
const LOVELL: u64 = 7_338_024;

/// The tests' words of memory: nothing, all ones, nine bytes, and eight
/// words that count from 0x00 to 0xff.
const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const ONES: &str = "0101010101010101010101010101010101010101010101010101010101010101";
const NINE: &str = "0001020304050607080000000000000000000000000000000000000000000000";
const EIGHT_WORDS: [&str; 8] = [
    "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
    "202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f",
    "404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f",
    "606162636465666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f",
    "808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f",
    "a0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebf",
    "c0c1c2c3c4c5c6c7c8c9cacbcccdcecfd0d1d2d3d4d5d6d7d8d9dadbdcdddedf",
    "e0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f1f2f3f4f5f6f7f8f9fafbfcfdfeff",
];

/// The program a case runs: `memory` stored from 0, a word at a time, the
/// three words MCOPY takes, and MCOPY where it `copies`.
fn program(memory: &[&str], dst: U256, src: U256, length: U256, copies: bool) -> Vec<u8> {
    let mut code = String::new();
    for (i, word) in memory.iter().enumerate() {
        code += &format!("PUSH32 0x{word} PUSH2 0x{:04x} MSTORE ", i * 32);
    }
    let [length, src, dst] = [length, src, dst].map(hex_word);
    code += &format!("PUSH32 {length} PUSH32 {src} PUSH32 {dst}");
    if copies {
        code += " MCOPY";
    }
    compile(&code)
}

/// The gas MCOPY alone takes: the program's with it, less the same
/// program's without it.
#[test]
fn mcopy_costs_3_gas_3_a_word_and_the_memory_it_expands() {
    let test = "testMCopy_ShouldConsumeTheCorrectAmountOfGas";
    let cases: [(&[&str], u64, u64, u64, u64); 6] = [
        (&[ZERO, EIGHT_WORDS[0]], 0, 32, 32, 6),
        (&[ONES], 0, 0, 32, 6),
        (&[NINE], 0, 1, 8, 6),
        (&[NINE], 1, 0, 8, 6),
        (&EIGHT_WORDS, 256, 256, 1, 9),
        (&EIGHT_WORDS, 0, 256, 256, 51),
    ];
    for (memory, dst, src, length, gas) in cases {
        let case = format!("{test}: {dst} {src} {length}");
        let [dst, src, length] = [dst, src, length].map(U256::from);
        let with = run_code(LOVELL, &program(memory, dst, src, length, true));
        let without = run_code(LOVELL, &program(memory, dst, src, length, false));
        assert!(with.success && without.success, "{case}");
        assert_eq!(with.gas_used - without.gas_used, gas, "{case}");
    }
}

/// `MCopyGasTest.testMCopy_ShouldThrowOutOfGasException` and
/// `MCopyInputTest.testMCopy_ShouldThrowOOGException`: an offset or a length
/// past what any gas pays for. The latter's words are `DataWord.valueOf` of a
/// long, its 8 bytes, a negative one's as two's complement.
#[test]
fn an_mcopy_no_gas_can_pay_for_fails_with_all_the_gas() {
    let (zero, one) = (U256::ZERO, U256::from(1));
    let mut cases: Vec<(&[&str], U256, U256, U256)> = Vec::new();
    for memory in [&[][..], &EIGHT_WORDS[..]] {
        for big in [U256::MAX, U256::MAX - one, U256::MAX >> 1] {
            cases.push((memory, big, zero, one));
            cases.push((memory, zero, big, one));
            cases.push((memory, zero, zero, big));
        }
    }
    let long = |n: i64| U256::from(n as u64);
    let max_memory = 1i64 << 30;
    cases.extend([
        (&[ZERO][..], zero, zero, long(-1)),
        (&[][..], zero, zero, long(-(2 * (i64::MAX / 3)))),
        (&[][..], zero, zero, long(i64::from(i32::MAX) + 1)),
        (&[][..], long(max_memory), zero, one),
        (&[][..], zero, long(max_memory), one),
        (&[][..], zero, zero, long(max_memory + 1)),
        (&[][..], zero, zero, long(-1)),
    ]);
    for (memory, dst, src, length) in cases {
        let ran = run_code(LOVELL, &program(memory, dst, src, length, true));
        assert!(!ran.success, "{dst} {src} {length}");
        assert_eq!(ran.gas_used, GAS, "{dst} {src} {length}");
    }
}
