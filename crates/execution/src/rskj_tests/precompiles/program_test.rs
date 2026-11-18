//! rskj's `org.ethereum.vm.program.ProgramTest`, its tests of RSKIP516
//! (reed800): from it the input a contract's call passes a precompiled
//! contract is cut to what that contract reads, and only where the contract
//! names a limit (`Program.getInputLength`).

use alloy_primitives::Address;

use super::harness::call_precompile;

/// reed800 on mainnet: RSKIP516.
const REED: u64 = 8_052_200;
/// The identity precompiled contract, which names no limit.
const IDENTITY: Address = Address::with_last_byte(4);

/// A contract's call of a precompiled contract that names no limit passes
/// it all of its input, before RSKIP516 and from it: identity, given 1,000
/// bytes, gives all 1,000 back.
#[test]
fn a_precompile_that_names_no_limit_gets_all_of_a_contracts_input() {
    let input: Vec<u8> = (0..=255u8).cycle().take(1_000).collect();
    for number in [REED - 1, REED] {
        let (success, output) = call_precompile(number, IDENTITY, 100_000, &input);
        assert!(success, "#{number}");
        assert_eq!(output, input, "#{number}");
    }
}
