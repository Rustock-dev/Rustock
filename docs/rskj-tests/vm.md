# rskj's tests of the VM, the precompiled contracts and REMASC, and their rustock counterparts

One row per rskj test that names a rule by its RSKIP number, and per test ported from the rest of
its class (a refusal, a limit, an overflow, an error path or a height window), with one of three
verdicts: **covered** (the rustock test that asserts the same behaviour), **ported** (a rustock test
written for it) or **not applicable** (why rustock needs no such test).

rskj at the release VETIVER-9.0.4: tests under `rskj-core/src/test/java/`, code under
`rskj-core/src/main/java/`. The ported tests are in `crates/execution/src/rskj_tests/`: `vm/`,
`precompiles/` and `remasc/`, one module per rskj class. Every ported test runs its case the way a
block does, through `RskExecutor::execute_block`, at a height on each side of the upgrade it names
where it names one.

## co.rsk.vm.VMExecutionTest

41 of its 69 tests name a rule. rskj steps each program on a mocked program with the rule switched
on or off; the port runs it as a contract's code in a block of mainnet, at the upgrade's first block
and the block before. Module `vm/vm_execution_test.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| VMExecutionTest.testSHL1 | RSKIP120: SHL of 1 by 0 is 1 | ported | `shl_shifts_left_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSHL2 | SHL of 1 by 1 is 2 | ported | `shl_shifts_left_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSHL3 | SHL of 1 by 255 is 2^255 | ported | `shl_shifts_left_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSHL4 | SHL of 1 by 256 is 0 | ported | `shl_shifts_left_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSHL5 | SHL of 2^256-1 by 0 is itself | ported | `shl_shifts_left_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSHL6 | SHL of 2^256-1 by 255 is 2^255 | ported | `shl_shifts_left_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSHL7 | SHL of 2^255-1 by 1 is 2^256-2 | ported | `shl_shifts_left_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSHR1 | RSKIP120: SHR of 1 by 0 is 1 | ported | `shr_shifts_right_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSHR2 | SHR of 1 by 1 is 0 | ported | `shr_shifts_right_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSHR3 | SHR of 2^255 by 1 is 2^254 | ported | `shr_shifts_right_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSHR4 | SHR of 2^255 by 255 is 1 | ported | `shr_shifts_right_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSHR5 | SHR of 2^256-1 by 0 is itself | ported | `shr_shifts_right_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSHR6 | SHR of 2^256-1 by 255 is 1 | ported | `shr_shifts_right_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSHR7 | SHR of 2^255 by 256 is 0 | ported | `shr_shifts_right_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSAR1 | RSKIP120: SAR of 1 by 0 is 1 | ported | `sar_shifts_right_keeping_the_sign_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSAR2 | SAR of 1 by 1 is 0 | ported | `sar_shifts_right_keeping_the_sign_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSAR3 | SAR of 2^255 by 1 is 0xc0..00 | ported | `sar_shifts_right_keeping_the_sign_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSAR4 | SAR of 2^255 by 255 is 2^256-1 | ported | `sar_shifts_right_keeping_the_sign_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSAR5 | SAR of 2^256-1 by 0 is itself | ported | `sar_shifts_right_keeping_the_sign_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSAR6 | SAR of 2^256-1 by 1 is itself | ported | `sar_shifts_right_keeping_the_sign_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSAR7 | SAR of 2^255-1 by 248 is 0x7f | ported | `sar_shifts_right_keeping_the_sign_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSAR8 | SAR of 2^255-1 by 256 is 0 | ported | `sar_shifts_right_keeping_the_sign_as_eip145_has_it_from_rskip120` |
| VMExecutionTest.testSAR3ShouldFailOnOldVersion | without RSKIP120 SAR throws IllegalOperationException | ported | `the_shifts_are_invalid_opcodes_before_rskip120`: fails with all the gas at #1,590,999, runs at #1,591,000 |
| VMExecutionTest.testSHL1ShouldFailOnOldVersion | without RSKIP120 SHL throws | ported | `the_shifts_are_invalid_opcodes_before_rskip120` |
| VMExecutionTest.testSHR1ShouldFailOnOldVersion | without RSKIP120 SHR throws | ported | `the_shifts_are_invalid_opcodes_before_rskip120` |
| VMExecutionTest.dupnAsInvalidOpcodeWhenDeactivated | with RSKIP191 DUPN (0xa8) is an invalid opcode | covered | `dupn_and_swapn_run_as_rskj_runs_them_before_rskip191` (`crates/execution/src/executor.rs`): from #3,614,800 DUPN of every depth fails with all the gas |
| VMExecutionTest.swapnAsInvalidOpcodeWhenDeactivated | with RSKIP191 SWAPN (0xa9) is an invalid opcode | covered | `dupn_and_swapn_run_as_rskj_runs_them_before_rskip191` (`executor.rs`): from #3,614,800 SWAPN fails with all the gas |
| VMExecutionTest.txIndexAsInvalidOpcodeWhenDeactivated | with RSKIP191 TXINDEX (0xaa) is an invalid opcode | covered | `txindex_pushes_the_transactions_place_in_its_block_before_rskip191` (`executor.rs`): fails at #3,614,800 |
| VMExecutionTest.chainIDMainnet | RSKIP152: CHAINID pushes 30 | ported | `chainid_pushes_each_networks_chain_id_from_rskip152` (and `test_chainid_selfbalance_activate_at_papyrus`, `executor.rs`) |
| VMExecutionTest.chainIDTestnet | CHAINID pushes 31 | ported | `chainid_pushes_each_networks_chain_id_from_rskip152`, at testnet's #863,000 |
| VMExecutionTest.chainIDRegtest | CHAINID pushes 33 | ported | `chainid_pushes_each_networks_chain_id_from_rskip152`, on a regtest |
| VMExecutionTest.testChainIDNotActivated | without RSKIP152 CHAINID throws | covered | `test_chainid_selfbalance_activate_at_papyrus` (`executor.rs`): CHAINID fails with the whole gas limit at #2,392,699 |
| VMExecutionTest.selfBalanceFailsWithRSKIPNotActivated | without RSKIP151 SELFBALANCE throws | ported | `selfbalance_is_an_invalid_opcode_before_rskip151` (the executor's papyrus test reaches CHAINID first, never SELFBALANCE) |
| VMExecutionTest.selfBalanceRunsCorrectly | SELFBALANCE pushes the owner's balance, 100, for the LOW tier's gas | ported | `selfbalance_pushes_the_contracts_own_balance_for_5_gas` |
| VMExecutionTest.selfBalanceIsSameAsBalance | SELFBALANCE pushes what BALANCE of the owner pushes | ported | `selfbalance_pushes_what_balance_of_the_contract_pushes` |
| VMExecutionTest.testPUSH0Activation | RSKIP398: PUSH0 pushes one zero word | ported | `push0_pushes_one_zero_word_from_rskip398` (the executor's arrowhead test checks only that the call succeeds) |
| VMExecutionTest.testPUSH0NoActivation | without RSKIP398 PUSH0 throws | covered | `test_push0_activates_at_arrowhead600` (`executor.rs`): the call fails at #6,223,699 |
| VMExecutionTest.testBASEFFEActivation | RSKIP412: BASEFEE pushes the minimum gas price | covered | `test_basefee_activates_at_arrowhead600` (`executor.rs`): pushes the header's minimum gas price at #6,223,700, #6,549,300 and #7,338,024 |
| VMExecutionTest.testBASEFEENoActivation | without RSKIP412 BASEFEE throws | covered | `test_basefee_activates_at_arrowhead600` (`executor.rs`): fails at #6,223,699 |
| VMExecutionTest.testMCOPY_WhenActive_ExecutesAsExpected | RSKIP445: MCOPY of three zero words leaves the stack empty | ported | `mcopy_takes_its_three_words_off_the_stack_from_rskip445` |
| VMExecutionTest.testMCOPY_WhenInactive_ExecutesAsExpected | without RSKIP445 MCOPY throws | ported | `mcopy_is_an_invalid_opcode_before_rskip445` |
| VMExecutionTest.invalidJustAfterEndOfCode | a JUMP to just past the code throws BadJumpDestinationException | ported | `a_jump_to_where_no_jumpdest_is_fails_with_all_the_gas` |
| VMExecutionTest.invalidJumpOutOfRange | a JUMP past the code throws | ported | `a_jump_to_where_no_jumpdest_is_fails_with_all_the_gas` |
| VMExecutionTest.invalidNegativeJump | a JUMP to 2^256-1 throws | ported | `a_jump_to_where_no_jumpdest_is_fails_with_all_the_gas` |
| VMExecutionTest.invalidTooFarJump | a JUMP to 255 throws | ported | `a_jump_to_where_no_jumpdest_is_fails_with_all_the_gas` |
| VMExecutionTest.createContractAndPreserveContractAddressBalance | a CREATE inside a contract onto an address holding 100 leaves 100 plus the endowment there, with no rule active | ported | `a_create_from_a_contract_keeps_the_balance_already_at_the_address_at_every_height`: at #1, #3,614,799 and #3,614,800 |
| VMExecutionTest.dupnTwentiethItemWithoutEnoughItems | DUPN deeper than the stack throws StackTooSmallException | covered | `dupn_and_swapn_run_as_rskj_runs_them_before_rskip191` (`executor.rs`): a depth the stack doesn't reach fails with all the gas |
| VMExecutionTest.dupnTooManyItemsWithOverflow | DUPN of 0x7fffffff throws StackTooSmallException | covered | `dupn_and_swapn_run_as_rskj_runs_them_before_rskip191` (`executor.rs`): depths past the stack and past a Java int fail |
| VMExecutionTest.swapnTooManyItemsWithOverflow | SWAPN of 0x7fffffff throws | covered | `dupn_and_swapn_run_as_rskj_runs_them_before_rskip191` (`executor.rs`) |
| VMExecutionTest.swapnTwentiethItemWithoutEnoughItems | SWAPN deeper than the stack throws | covered | `dupn_and_swapn_run_as_rskj_runs_them_before_rskip191` (`executor.rs`) |

The rest of the class was read: the arithmetic and PUSH1 tests, DUPN's and SWAPN's results by depth and
TXINDEX's value (all held by the executor's DUPN, SWAPN and TXINDEX tests before RSKIP191), the
jump analysis over DUPN's and SWAPN's next byte (`dupn_and_swapn_step_over_the_next_byte_and_stop_past_the_end`),
the 15,000 jumps (a timing), the basic gas of RETURNDATASIZE, RETURNDATACOPY and CALLDATACOPY (every
block's), and CHAINID's opcode number (a constant).

## co.rsk.vm.opcode.TransientStorageTest

Its 5 tests name RSKIP446. rskj steps each program on a mocked program with the rule on; the port
runs it as a contract's code in a block of mainnet at lovell700 (#7,338,024). rustock had no test
that runs TLOAD or TSTORE (`test_rskip445_rskip446_mainnet`, `hardfork.rs`, holds the activation
height only). Module `vm/transient_storage_test.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| TransientStorageTest.testTLoadDynamicExecutionContextUnderflow | RSKIP446: TLOAD on an empty stack throws StackTooSmallException | ported | `tload_with_an_empty_stack_fails_with_all_the_gas` |
| TransientStorageTest.testTLoadDynamicExecutionContextWorksFine | TLOAD of key 1, never stored, leaves one zero word | ported | `tload_of_a_key_never_stored_pushes_zero`: for 100 gas |
| TransientStorageTest.testTStoreDynamicExecutionContextUnderflow | TSTORE on an empty stack throws StackTooSmallException | ported | `tstore_short_of_its_two_words_fails_with_all_the_gas`: with no word and with one |
| TransientStorageTest.testTStoreDynamicExecutionContextWorksFine | TSTORE of 0x420 at key 1 leaves the stack empty, with no exception | ported | `tstore_takes_its_key_and_its_value_off_the_stack`: for 100 gas |
| TransientStorageTest.testTStoreTloadDynamicExecutionContextWorksFine | TLOAD of key 1 after a TSTORE of 0x420 there pushes 0x420 | ported | `tload_pushes_what_tstore_stored_at_the_key` |

## co.rsk.vm.opcode.TransientStorageDslTest

Its 29 tests name RSKIP446. rskj runs Solidity contracts of its DSL files in blocks and counts the
`OK` events they emit, one for each check that holds. The port writes each contract in opcodes, logs
each word the contract checks, and holds the logged words to the values rskj's contracts check them
against; a frame that fails drops its logs, as rskj's drops its events. Each runs at lovell700 on
mainnet. Module `vm/transient_storage_dsl_test.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| TransientStorageDslTest.testTransientStorageOpcodesExecutionsWithRSKIPActivated | RSKIP446: TLOAD of a key after a TSTORE there reads the value stored, for two keys | ported | `tload_reads_what_tstore_stored_at_the_key` |
| TransientStorageDslTest.testTransientStorageOpcodesShareMemorySameTransaction | a contract's TSTORE and TLOAD agree within its transaction | ported | `a_contracts_call_to_itself_reads_what_it_stored_before_the_call`: in one frame, and in the contract's call to itself |
| TransientStorageDslTest.testTransientStorageOpcodesDoesntShareMemoryFromOtherContract | another contract's TLOAD of the key reads zero | ported | `another_contract_reads_none_of_what_a_contract_stored` |
| TransientStorageDslTest.testTransientStorageOpcodesExecutionFailsWithRSKIPDeactivated | with RSKIP446 off, the transaction that runs TLOAD and TSTORE fails | ported | `tload_and_tstore_are_invalid_opcodes_before_rskip446`: it fails with all its gas at #7,338,023. rskj turns the rule off with `lovell700 = -1` under later upgrades at block 0, a schedule rustock refuses: see below |
| TransientStorageDslTest.testTransientStorageTestsEip1153BasicScenarios | EIP-1153's cases, a transaction each, for keys 1, 2, 2^128 and 2^256-1: TLOAD after an SSTORE reads 0, after a TSTORE reads its value, in the next transaction reads 0, and a TSTORE leaves other keys at 0 | ported | `tload_reads_only_what_tstore_stored_in_the_transaction` |
| TransientStorageDslTest.testOnlyConstructorCodeCreateContext | a constructor reads none of its creator's transient storage, and its TSTORE leaves the creator's | ported | `a_constructor_has_the_new_contracts_transient_storage` |
| TransientStorageDslTest.testInConstructorAndCodeCreateContext | the new contract's code, called after the creation, has the new contract's transient storage | ported | `the_code_a_constructor_deploys_has_the_new_contracts_transient_storage` |
| TransientStorageDslTest.testAccrossConstructorAndCodeV0CreateContext | the deployed code reads what its constructor stored in the same transaction | ported | `the_deployed_code_reads_what_its_constructor_stored_in_the_transaction` |
| TransientStorageDslTest.testAccrossConstructorAndCodeV1CreateContext | the same, the constructor first reading key 0 | ported | `the_deployed_code_reads_what_its_constructor_stored_in_the_transaction` |
| TransientStorageDslTest.testNoConstructorCodeCreateContext | code deployed without a constructor reads 0 and its own TSTORE | ported | `code_deployed_by_an_empty_constructor_has_the_new_contracts_transient_storage` |
| TransientStorageDslTest.testDynamicExecutionContextSimpleScenario | CALLCODE and DELEGATECALL run the callee in the caller's transient storage | ported | `callcode_and_delegatecall_run_the_callee_in_the_callers_transient_storage` |
| TransientStorageDslTest.testDynamicExecutionContextWithRevert | a callee that REVERTs takes back its TSTORE, by CALLCODE, DELEGATECALL and CALL | ported | `a_callee_that_reverts_takes_back_its_tstore` |
| TransientStorageDslTest.testDynamicExecutionContextWithInvalid | the same for a callee that runs INVALID | ported | `a_callee_that_runs_invalid_takes_back_its_tstore` |
| TransientStorageDslTest.testDynamicExecutionContextWithStackOverflow | the same for a callee whose own calls of itself fail at depth | ported | `a_callee_failing_deep_in_its_own_calls_takes_back_its_tstore` |
| TransientStorageDslTest.testDynamicExecutionCallContextSubcall | a CALL runs the callee in its own transient storage | ported | `a_call_runs_the_callee_in_its_own_transient_storage` |
| TransientStorageDslTest.testDynamicExecutionStaticCallSubcallCantUseTstore | a STATICCALL's callee cannot TSTORE, and the caller's value stays | ported | `a_static_calls_callee_cannot_tstore` |
| TransientStorageDslTest.testDynamicReentrancyContextsTstoreBeforeRevertOrInvalidHasNoEffect | a reentrant call's TSTORE before a REVERT has no effect; the next transaction, taking the INVALID branch, fails | ported | `a_tstore_before_a_revert_has_no_effect`: the INVALID one with all its gas |
| TransientStorageDslTest.testDynamicReentrancyContextsRevertOrInvalidUndoesAll | a REVERT takes back every TSTORE of its frame; the next transaction fails | ported | `a_revert_takes_back_every_tstore_of_its_frame` |
| TransientStorageDslTest.testDynamicReentrancyContextsRevertOrInvalidUndoesTstorageAfterSuccessfullCall | a REVERT takes back the TSTORE of a call it made that succeeded; the next transaction fails and leaves the storage as it was | ported | `a_revert_takes_back_the_tstore_of_a_call_that_succeeded` |
| TransientStorageDslTest.testReentrancyContextsTstoreAfterReentrantCall | a reentrant call reads what the contract stored before it | ported | `a_reentrant_call_reads_what_the_contract_stored_before_it` |
| TransientStorageDslTest.testReentrancyContextsTloadAfterReentrantTstore | after a reentrant call, the contract reads what the call stored | ported | `a_contract_reads_what_its_reentrant_call_stored` |
| TransientStorageDslTest.testReentrancyContextsManipulateInReentrantCall | a reentrant call overwrites the contract's value and both read the new one | ported | `a_reentrant_call_overwrites_what_the_contract_stored` |
| TransientStorageDslTest.testReentrancyContextsTstoreInCallThenTloadReturnInStaticCall | a static call returns what a call stored, and the caller reads it after | ported | `a_static_call_returns_what_a_call_stored_and_the_caller_reads_it_after` |
| TransientStorageDslTest.testTransientStorageGasMeasureTests | four gas measures of TLOAD and TSTORE, each of a key written or not | ported | `tload_and_tstore_cost_100_gas_whether_or_not_the_key_was_written`: rskj's figures (110, 216, 113, 219) also count its compiled code's own stack moves; the port holds each measure to its opcodes, TLOAD and TSTORE at 100 |
| TransientStorageDslTest.testTstoreLoopUntilOutOfGas | a loop of TSTOREs fails with all its 300,000 gas | ported | `a_loop_of_tstores_runs_out_of_gas_and_fails_with_all_of_it` |
| TransientStorageDslTest.testTstoreWideAddressSpaceLoopUntilOutOfGas | the same at keys far apart, with 500,000 | ported | `a_loop_of_tstores_runs_out_of_gas_and_fails_with_all_of_it` |
| TransientStorageDslTest.testTstoreAndTloadLoopUntilOutOfGas | the same with a TLOAD of each key, with 700,000 | ported | `a_loop_of_tstores_runs_out_of_gas_and_fails_with_all_of_it` |
| TransientStorageDslTest.testTstorageSelfDestructCases_OnEachTest_TheEventsEmittedAreTheExpected | six cases: TLOAD after a SELFDESTRUCT, after an inner one, and TSTORE after one, for a contract there before and one created in the transaction, each reads what was stored | ported | `transient_storage_outlives_a_selfdestruct_in_the_same_transaction` |
| TransientStorageDslTest.testTloadTstoreCheckingBetweenTransactions | the block's next transaction reads 0 where the first stored | ported | `transient_storage_is_empty_again_for_the_blocks_next_transaction` |

The schedule rskj's test runs on. rskj (`co/rsk/vm/opcode/TransientStorageDslTest.java:107-127`) turns
RSKIP446 off by setting `hardforkActivationHeights.lovell700 = -1` on its test regtest, where every
later upgrade is at block 0 (`rskj-core/src/main/resources/config/regtest.conf:16-20`). RSKIP446 is
lovell700's (`rskj-core/src/main/resources/reference.conf:92`), and `ActivationConfig.isActive`
(`org/ethereum/config/blockchain/upgrades/ActivationConfig.java:105-108`) reads that height, so
TLOAD throws as an invalid opcode (`org/ethereum/vm/VM.java:2041-2051`) and the transaction fails.
rustock reads a rule from the upgrade in force (`active_upgrade`, `crates/execution/src/hardfork.rs`):
with lovell700 left out and vetiver900 at block 0 it would have RSKIP446 on. No network has such a
schedule (mainnet leaves out reed810 and testnet arrowhead631, and rustock reads those two at their
own heights), so rustock refuses it (`RskHardforkConfig::with_rskj_overrides`, held by
`an_upgrade_left_out_below_a_scheduled_one_is_refused` and
`an_upgrade_scheduled_out_of_its_order_is_refused`) and the port runs rskj's case in mainnet's last
block before lovell700. The same holds for the tests below that rskj runs with an upgrade at `-1`:
VmDslTest's, TransactionExecutorInitCodeSizeDslTest's, DslFilesTest's, MCopyDslTest's and
NestedCallsStackDepthTest's.

## org.ethereum.vm.VMTest

11 of its 221 tests name a rule in their own code, and its set-up switches on RSKIP89, 90, 91, 103
(but for the tests of the static call before its fix), 150 and 398 for all. rskj steps each program on a mocked
program; the port runs it as a contract's code in a block of mainnet, at the upgrade's first block
and the block before. Module `vm/vm_test.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| VMTest.returnDataBufferAfterCallToNonExistentContract | RSKIP171: after a STATICCALL of address 0, where no code is, RETURNDATACOPY of the word the call before returned throws | ported | `a_static_call_of_no_code_leaves_no_return_data_from_rskip171`: fails with all the gas at #3,614,800 |
| VMTest.beforeIrisReturnDataBufferAfterCallToNonExistentContract | without RSKIP171 it copies the call before's word, 0x15 | ported | `a_static_call_of_no_code_leaves_no_return_data_from_rskip171`: 0x15 at #3,614,799 |
| VMTest.returnDataSizeAfterCallToNonExistentContract | RSKIP171: RETURNDATASIZE after it is 0 | ported | `a_static_call_of_no_code_leaves_no_return_data_from_rskip171` (and, for a CALL, `test_empty_code_call_clears_return_data_buffer_post_rskip171`, `executor.rs`) |
| VMTest.beforeIrisReturnDataSizeAfterCallToNonExistentContract | without RSKIP171 it is 32 | ported | `a_static_call_of_no_code_leaves_no_return_data_from_rskip171` (and, for a CALL, `test_empty_code_call_preserves_return_data_buffer`, `executor.rs`) |
| VMTest.returnDataBufferAfterInsufficientFunds | RSKIP171: after a CALL to a contract with a value its caller can't pay, RETURNDATASIZE is 0 | ported | `a_call_its_caller_cannot_pay_leaves_no_return_data_from_rskip171` |
| VMTest.returnDataBufferAfterInsufficientFundsAtPrecompiled | the same to the block header contract (RSKIP119) | ported | `a_call_its_caller_cannot_pay_leaves_no_return_data_from_rskip171` |
| VMTest.beforeIrisReturnDataBufferWithInsufficientFunds | without RSKIP171 it is the call before's 32 | ported | `a_call_its_caller_cannot_pay_leaves_no_return_data_from_rskip171`, at #3,614,799 |
| VMTest.beforeIrisReturnDataBufferWithInsufficientFundsAtPrecompiled | the same to the block header contract | ported | `a_call_its_caller_cannot_pay_leaves_no_return_data_from_rskip171`, at #3,614,799 |
| VMTest.returnDataBufferAfterEnoughGasAtPrecompiled | RSKIP171: after a CALL of identity with no gas, which can't pay it, RETURNDATASIZE is 0 | covered | `test_precompile_calls_and_the_return_data_by_height` (`executor.rs`): identity called with no gas leaves no return data at #3,700,000 |
| VMTest.beforeIrisReturnDataBufferAfterEnoughGasAtPrecompiled | without RSKIP171 it is the call before's | covered | `test_precompile_calls_and_the_return_data_by_height` (`executor.rs`): the call before's 4 bytes at #1,700,000 |
| VMTest.testBASEFEE | RSKIP412: BASEFEE pushes the minimum gas price | covered | `test_basefee_activates_at_arrowhead600` (`executor.rs`) |
| VMTest.testPUSH0 | RSKIP398, from the set-up: PUSH0 pushes a zero word | covered | `push0_pushes_one_zero_word_from_rskip398` (`vm/vm_execution_test.rs`) |
| VMTest.testSTATICCALLWithStatusOneFailsWithOldCode | without RSKIP103 a STATICCALL of six words throws EmptyStackException | ported | `staticcall_takes_seven_words_before_rskip103_and_six_from_it`: fails with all the gas at #1,052,699 |
| VMTest.testSTATICCALLWithStatusOneAndAdditionalValueInStackUsingPreFixStaticCall | without RSKIP103 a STATICCALL of seven words leaves only its success | ported | `staticcall_takes_seven_words_before_rskip103_and_six_from_it` |
| VMTest.testSTATICCALLWithStatusOneAndAdditionalValueInStackUsingFixStaticCallLeavesValueInStack | with RSKIP103 a STATICCALL of seven words leaves the first under its success | ported | `staticcall_takes_seven_words_before_rskip103_and_six_from_it`, at #1,052,700 (and `test_pre_rskip103_staticcall_takes_a_value`, `executor.rs`, for the value it moves before) |
| VMTest.testSTATICCALLWithStatusZeroUsingSStore | a STATICCALL's callee that SSTOREs fails, and the call pushes 0 | ported | `a_static_calls_callee_that_would_change_state_fails`, where the same callee by a CALL succeeds |
| VMTest.testSTATICCALLWithStatusZeroUsingLogs | the same for LOG0 to LOG4 | ported | `a_static_calls_callee_that_would_change_state_fails`: each LOG given its topics, so that only the static call refuses it |
| VMTest.testSTATICCALLWithStatusZeroUsingCreate | the same for CREATE | ported | `a_static_calls_callee_that_would_change_state_fails` |
| VMTest.testSTATICCALLWithStatusZeroUsingSuicide | the same for SUICIDE | ported | `a_static_calls_callee_that_would_change_state_fails` |
| VMTest.testSTATICCALLWithStatusOne | a STATICCALL of a callee that only computes pushes 1 | ported | `a_static_calls_callee_that_would_change_state_fails` |
| VMTest.testCALLWithBigUserSpecifiedGas | a CALL asking 2^63-1 gas of a callee that fails uses all the gas | ported | `a_call_asking_more_gas_than_is_left_gives_the_callee_all_of_it` |
| VMTest.testLOGWithDataCostBiggerThanPreviousGasSize | LOG0 of 2^59 bytes throws OutOfGasException | ported | `a_log_of_more_data_than_its_gas_pays_for_runs_out_of_gas` |
| VMTest.testReturnDataCopyChargesCorrectGas | RETURNDATACOPY of no bytes is charged no copy: 3 gas less than of a word | ported | `a_copy_of_no_bytes_expands_no_memory` |
| VMTest.testCallDataCopyDoesNotExpandMemoryForFree | CALLDATACOPY of no bytes at the memory's last byte expands none | ported | `a_copy_of_no_bytes_expands_no_memory` |
| VMTest.getFreeMemoryUsingPrecompiledContractLyingAboutReturnSize | identity's output takes no memory past the room its caller gave | ported | `a_precompiles_caller_pays_memory_only_for_the_room_it_gave_the_output` |
| VMTest.getFreeMemoryUsingPrecompiledContractAndSettingFarOffOffset | an output with no room at 0x01000000 takes no memory | ported | `a_precompiles_caller_pays_memory_only_for_the_room_it_gave_the_output` |
| VMTest.testDUPN_2 | DUP1 on an empty stack throws StackTooSmallException | ported | `a_program_short_of_words_or_jumping_nowhere_fails_with_all_the_gas` |
| VMTest.testSWAPN_2 | SWAP1 on an empty stack throws StackTooSmallException | ported | `a_program_short_of_words_or_jumping_nowhere_fails_with_all_the_gas` |
| VMTest.testMSTORE_5 | MSTORE of one word throws StackTooSmallException | ported | `a_program_short_of_words_or_jumping_nowhere_fails_with_all_the_gas` |
| VMTest.testMLOAD_6 | MLOAD on an empty stack throws StackTooSmallException | ported | `a_program_short_of_words_or_jumping_nowhere_fails_with_all_the_gas` |
| VMTest.testJUMPI_3 | JUMPI of one word throws StackTooSmallException | ported | `a_program_short_of_words_or_jumping_nowhere_fails_with_all_the_gas` |
| VMTest.testEXP_4 | EXP of one word throws StackTooSmallException | ported | `a_program_short_of_words_or_jumping_nowhere_fails_with_all_the_gas` |
| VMTest.testCODECOPY_6 | CODECOPY of two words throws StackTooSmallException | ported | `a_program_short_of_words_or_jumping_nowhere_fails_with_all_the_gas` |
| VMTest.testEXTCODECOPY_5 | EXTCODECOPY of three words throws StackTooSmallException | ported | `a_program_short_of_words_or_jumping_nowhere_fails_with_all_the_gas` |
| VMTest.testJUMP_1 | a JUMP to a PUSH1 past the JUMPDEST throws BadJumpDestinationException | ported | `a_program_short_of_words_or_jumping_nowhere_fails_with_all_the_gas` |
| VMTest.testJUMPI_4 | a JUMPI past the code throws BadJumpDestinationException | ported | `a_program_short_of_words_or_jumping_nowhere_fails_with_all_the_gas` |
| VMTest.testJUMPDEST_1 | a JUMP to the PUSH1 after a JUMPDEST throws BadJumpDestinationException | ported | `a_program_short_of_words_or_jumping_nowhere_fails_with_all_the_gas` |
| VMTest.testInvalidOpcodes_1 | 0xA5, no opcode, throws IllegalOperationException | ported | `a_program_short_of_words_or_jumping_nowhere_fails_with_all_the_gas` |
| VMTest.testVersioning_1 | code under a HEADER (0xFC) runs from after it | covered | `a_code_header_is_skipped_as_rskj_skips_it` (`executor.rs`) |
| VMTest.testVersioning_2 | a header's length past 127 is read unsigned | covered | `a_code_header_is_skipped_as_rskj_skips_it` (`executor.rs`) |
| VMTest.testScriptVersion0 | HEADER alone throws IllegalOperationException | covered | `a_code_header_is_skipped_as_rskj_skips_it` (`executor.rs`) |
| VMTest.testScriptVersion1 | HEADER after a header of version 0 throws | covered | `a_code_header_is_skipped_as_rskj_skips_it` (`executor.rs`) |
| VMTest.testScriptVersion2 | HEADER after a header of version 1 throws | covered | `a_code_header_is_skipped_as_rskj_skips_it` (`executor.rs`) |
| VMTest.testScriptVersion3 | a header ten bytes longer skips them | covered | `a_code_header_is_skipped_as_rskj_skips_it` (`executor.rs`) |

The rest of the class was read: the tests of the arithmetic, logic, comparison, PUSH, DUP, SWAP,
memory, storage, PC, JUMP, EXP, RETURN, CODECOPY, EXTCODECOPY, CODESIZE and LOG opcodes (each an
opcode's value or price, under no rule; their failures for want of words are ported above),
testPUSHN_1 and _2 (a PUSH past the code's end, after which nothing runs that a block could see),
regression1Test, regression2Test, decompileDupnSwapn and decompileTxindex (rskj's disassembler), and
the three tests of whether a transaction called a precompiled contract (a record rskj keeps for
itself).

## co.rsk.pcc.AltBN128Test

10 of its 32 tests name a rule: RSKIP137 (papyrus200), which brings the three contracts, and RSKIP197
(iris300), from which an input they refuse throws. rskj runs a contract's `execute` with the rule on
and off; the port has a contract call it in a block of mainnet, at iris300's first block and the block
before, given the contract's price (and fails it given a gas less). Module
`precompiles/alt_bn128_test.rs`; the call is `precompiles/harness.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| AltBN128Test.testALTBN128ShouldFailOnNotActivatedHardFork | without RSKIP137 no contract is at the three addresses | covered | `test_bn128_active_after_papyrus200_with_istanbul_gas` (`precompiles.rs`): none of them at #2,392,699, all three at #2,392,700 |
| AltBN128Test.testALTBN128AddPointNotOnCurveShouldFail | RSKIP197: an addition of points not on the curve answers empty without it and throws with it, for 150 | ported | `input_bn128_refuses_answers_empty_before_rskip197_and_fails_the_call_from_it` (and, for a transaction's own call, `bn128_refusing_a_point_answers_empty_before_rskip197_and_throws_from_it`, `executor.rs`) |
| AltBN128Test.shouldFailForPointNotOnCurve | the same for a multiplication of a point not on the curve, for 6,000 | ported | `input_bn128_refuses_answers_empty_before_rskip197_and_fails_the_call_from_it` |
| AltBN128Test.mulShouldFailForNotEnoughParams | the same for a multiplication of two words | ported | `input_bn128_refuses_answers_empty_before_rskip197_and_fails_the_call_from_it` |
| AltBN128Test.mulShouldFailEmptyParams | a multiplication of nothing answers 64 zero bytes with RSKIP197 or without | ported | `input_bn128_takes_gets_the_same_answer_before_rskip197_and_from_it` |
| AltBN128Test.shouldReturnTrueForNullInput | a pairing of null answers 1 either way, for 45,000 | ported | `input_bn128_takes_gets_the_same_answer_before_rskip197_and_from_it` (a contract's call passes no bytes, never null) |
| AltBN128Test.invalidInputForPairing | a pairing of one byte answers empty without RSKIP197 and throws with it, for 45,000 | ported | `input_bn128_refuses_answers_empty_before_rskip197_and_fails_the_call_from_it` |
| AltBN128Test.emptyInputForPairing | a pairing of nothing answers 1 either way | ported | `input_bn128_takes_gets_the_same_answer_before_rskip197_and_from_it` |
| AltBN128Test.shouldReturnEmptyForPairingPointsNotOnCurve | a pairing of two pairs not on the curve answers empty without RSKIP197 and throws with it, for 113,000 | ported | `input_bn128_refuses_answers_empty_before_rskip197_and_fails_the_call_from_it` |
| AltBN128Test.shouldReturnFalseForValidPointsPairing | a pairing of two valid pairs answers 0 either way, for 113,000 | ported | `input_bn128_takes_gets_the_same_answer_before_rskip197_and_from_it` |

The rest of the class was read: the additions, multiplications and pairings of valid points, from a
contract and from a program (the curve's values, under no rule), the addresses, and the limits each
contract names (`testMaxInput*`, held by `RskPrecompileProvider::max_input` and, where a block can
see one, `a_contracts_input_to_blake2f_is_cut_to_213_bytes_from_reed800`, `executor.rs`).

## org.ethereum.vm.PrecompiledContractTest

10 of its 17 tests name RSKIP153 (iris300), BLAKE2F's: EIP-152's test vectors. rskj runs BLAKE2F's
`execute`; the port has a contract call BLAKE2F at iris300 on mainnet, given the vector's rounds as
gas. Module `precompiles/precompiled_contract_test.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| PrecompiledContractTest.blake2fTestVector0 | RSKIP153: BLAKE2F refuses no bytes, for the input's length | ported | `blake2f_refuses_an_input_of_the_wrong_length_or_flag` |
| PrecompiledContractTest.blake2fTestVector1 | it refuses 212 bytes | ported | `blake2f_refuses_an_input_of_the_wrong_length_or_flag` |
| PrecompiledContractTest.blake2fTestVector2 | it refuses 214 bytes | ported | `blake2f_refuses_an_input_of_the_wrong_length_or_flag` |
| PrecompiledContractTest.blake2fTestVector3 | it refuses a final-block flag of 2 | ported | `blake2f_refuses_an_input_of_the_wrong_length_or_flag` |
| PrecompiledContractTest.blake2fTestVector4 | 0 rounds: EIP-152's hash, for 0 gas | ported | `blake2f_compresses_eip152s_vectors_for_a_gas_a_round` |
| PrecompiledContractTest.blake2fTestVector5 | 12 rounds: its hash, for 12 | ported | `blake2f_compresses_eip152s_vectors_for_a_gas_a_round`, which fails it given 11 |
| PrecompiledContractTest.blake2fTestVector6 | 12 rounds of a block that is not the last: its hash, for 12 | ported | `blake2f_compresses_eip152s_vectors_for_a_gas_a_round` |
| PrecompiledContractTest.blake2fTestVector7 | 1 round: its hash, for 1 | ported | `blake2f_compresses_eip152s_vectors_for_a_gas_a_round` |
| PrecompiledContractTest.blake2fTestVector8 | 2^32-1 rounds: its hash, for 4,294,967,295 | not applicable | `@Disabled` in rskj for the minutes it takes; its price is past any block's gas, so no block runs it |
| PrecompiledContractTest.blake2fTestOnNotActivatedHardFork | without RSKIP153 no contract is at 0x09 | covered | `test_blake2f_active_after_iris300` (`precompiles.rs`): absent at #3,614,799, present at #3,614,800 |

The rest of the class was read: identity, sha256, ripemd160 and modexp of given inputs (values,
under no rule; identity, sha256 and ripemd160 are held to rskj's vectors by
`each_address_holds_its_precompiled_contract`), and ecRecoverTest1, `@Disabled` in rskj.

## co.rsk.vm.precompiles.PrecompiledContractTest

4 of its 6 tests name a rule: RSKIP106 (the HD wallet contract) and RSKIP119 (the block header
contract), both wasabi100's. rskj asks `getContractForAddress` with the rule on and off; rustock's
tests ask its set of precompiled contracts at mainnet's heights.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| PrecompiledContractTest.getBlockHeaderContractBeforeRskip119 | without RSKIP119 no contract is at the block header's address | covered | `test_blockheader_active_after_wasabi` (`precompiles.rs`): absent at #1,590,999 |
| PrecompiledContractTest.getBlockHeaderContractAfterRskip119 | with it the block header contract is there | covered | `test_blockheader_active_after_wasabi` (`precompiles.rs`): present at #1,591,000 |
| PrecompiledContractTest.getHdWalletUtilsBeforeRskip106 | without RSKIP106 no HD wallet contract | covered | `test_hdwallet_not_active_before_wasabi100` (`precompiles.rs`): absent at #0 |
| PrecompiledContractTest.getHdWalletUtilsAfterRskip106 | with it the HD wallet contract is there | covered | `test_all_active_has_everything` (`precompiles.rs`), and `each_address_holds_its_precompiled_contract` at #8,052,200 |

The rest of the class (the Bridge's address, twice) names no rule.

## co.rsk.vm.precompiles.PrecompiledContractAddressTests

Its one test names RSKIP106, RSKIP119 and RSKIP516, on for every address it asks. Module
`precompiles/precompiled_contract_address_tests.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| PrecompiledContractAddressTests.testGetPrecompile | each of eleven addresses holds its contract | ported | `each_address_holds_its_precompiled_contract`: at #8,052,200 all eleven are in rustock's set, and sha256, ripemd160 and identity answer rskj's vectors |

## co.rsk.vm.precompiles.PrecompiledContractsCallErrorHandlingTests

4 of its 11 tests name RSKIP552 (vetiver900): BLAKE2F given null, which rskj reads a transaction's
empty data as.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| PrecompiledContractsCallErrorHandlingTests.blake2fGetGasForData_whenNullData_shouldNotThrowNPE | RSKIP552: BLAKE2F prices null at 0 | covered | `a_transactions_empty_call_to_blake2f_fails_its_block_before_rskip552` (`executor.rs`): from #8,804,200 a transaction's call of it with no data uses 21,000 gas |
| PrecompiledContractsCallErrorHandlingTests.blake2fExecute_whenNullData_shouldThrowVMException | RSKIP552: BLAKE2F refuses null with a VMException | covered | `a_transactions_empty_call_to_blake2f_fails_its_block_before_rskip552` (`executor.rs`): the call is refused, with no output |
| PrecompiledContractsCallErrorHandlingTests.blake2fGetGasForData_whenNullData_andRskipB2FInactive_shouldThrowNPE | without RSKIP552 pricing null throws NullPointerException | covered | `a_transactions_empty_call_to_blake2f_fails_its_block_before_rskip552` (`executor.rs`): at #8,804,199 the block is refused, as rskj's `BlockExecutor` refuses a block whose transaction throws |
| PrecompiledContractsCallErrorHandlingTests.blake2fExecute_whenNullData_andRskipB2FInactive_shouldThrowNPE | without RSKIP552 running null throws NullPointerException | covered | `a_transactions_empty_call_to_blake2f_fails_its_block_before_rskip552` (`executor.rs`) |

The rest of the class was read: the two DSL cases of each precompiled contract given trash by a
contract before iris300 and from it (a throw failing the contract before RSKIP197, held by
`a_pre_rskip197_throw_fails_only_the_contract_that_made_the_call`,
`a_native_contract_that_throws_inside_a_call_fails_the_call` and
`a_contracts_call_to_the_bridge_ends_as_rskj_ends_it_in_each_era`, `executor.rs`, and for BN128 and
BLAKE2F by the ports above), the limits each contract names (`RskPrecompileProvider::max_input`),
and BLAKE2F built with no activations (no block builds it so).

## co.rsk.vm.precompiles.blake2b.Blake2fNullDataTest

Both its tests name RSKIP552: a block of a transaction's call of BLAKE2F with no data and a transfer
after it. Module `precompiles/blake2f_null_data_test.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| Blake2fNullDataTest.blake2fWithNullData_beforeRskip552_blockExecutorHandlesUnexpectedException | without RSKIP552 the block the miner builds leaves the call out, as `BlockExecutor` drops a transaction that throws, and imports | ported | `before_rskip552_the_miner_leaves_out_a_transactions_call_of_blake2f_with_no_data`: the miner's execution drops it as invalid and runs the transfer (a block that holds it is refused: `a_transactions_empty_call_to_blake2f_fails_its_block_before_rskip552`, `executor.rs`) |
| Blake2fNullDataTest.blake2fWithNullData_shouldNotInvalidateBlock | with RSKIP552 the block holds both, and the transfer succeeds | ported | `from_rskip552_a_transactions_call_of_blake2f_with_no_data_leaves_its_block_valid` |

## org.ethereum.vm.program.ProgramTest

7 of its 16 tests name RSKIP516 (reed800): from it a contract's call of a precompiled contract passes
only as much input as the contract names as its limit (`Program.getInputLength`). rskj runs mocked
contracts with limits of 128, 96 and 0 and none; rustock's `RskPrecompileProvider::max_input` names
the real contracts' limits: 128 for ecrecover, BN128's addition and secp256k1's, 96 for the
multiplications, 213 for BLAKE2F. Module `precompiles/program_test.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| ProgramTest.testMaxInputNoTruncationForUnlimitedContracts | without RSKIP516 a contract that names no limit gets all 1,000 bytes | ported | `a_precompile_that_names_no_limit_gets_all_of_a_contracts_input`: identity at #8,052,199 |
| ProgramTest.testProgramMaxInputNoTruncationLogicForUnlimitedContracts | the same | ported | `a_precompile_that_names_no_limit_gets_all_of_a_contracts_input` |
| ProgramTest.testRSKIP516ActiveNoTruncationForUnlimitedContracts | with RSKIP516 a contract that names no limit gets all 1,000 bytes | ported | `a_precompile_that_names_no_limit_gets_all_of_a_contracts_input`: identity at #8,052,200 |
| ProgramTest.testRSKIP516InactiveNoTruncation | without RSKIP516 a contract with a limit gets all 200 bytes | covered | `a_contracts_input_to_blake2f_is_cut_to_213_bytes_from_reed800` (`executor.rs`): before #8,052,200 BLAKE2F gets all of a longer input and refuses it |
| ProgramTest.testRSKIP516ActiveMaxInputTruncation | with RSKIP516 a contract with a limit of 128 gets 128 of 200 bytes | covered | `a_contracts_input_to_blake2f_is_cut_to_213_bytes_from_reed800` (`executor.rs`), for BLAKE2F's 213; the 128-byte contracts read nothing past their 128 bytes, so no block sees their cut |
| ProgramTest.testRSKIP516ActiveTruncationForSecp256k1Multiplication | with RSKIP516 a limit of 96 gets 96 of 150 | covered | as above: secp256k1's multiplication reads three words |
| ProgramTest.testRSKIP516ActiveMaxInputZeroReturnsZeroInput | with RSKIP516 a limit of 0 gets no bytes | not applicable | no precompiled contract of rskj's names a limit of 0 |

The rest of the class was read: the throws of a precompiled contract inside a contract's call (held
by `a_pre_rskip197_throw_fails_only_the_contract_that_made_the_call` and
`a_native_contract_that_throws_inside_a_call_fails_the_call`, `executor.rs`), and the mocked limits
at their edges, which no real contract names.

Read with these, two classes name a rule in their set-up only: `co.rsk.pcc.environment.EnvironmentTest`
(RSKIP203; its call-depth method is held by `environment_counts_the_depth_of_the_contract_that_asks`,
`executor.rs`) and `org.ethereum.vm.program.ProgramBeforeRSKIP197Test` (ProgramTest's throws before
RSKIP197, held by `a_pre_rskip197_throw_fails_only_the_contract_that_made_the_call`).

## co.rsk.vm.VmDslTest

8 of its 11 tests name RSKIP438 (lovell700): from it CREATE and CREATE2 charge 2 gas for each word of
init code, and init code past 49,152 bytes fails the contract that runs them. rskj runs a factory
contract of its DSL files in blocks, on its test regtest (every upgrade at block 0) or with
`lovell700 = -1`. The port has a contract run CREATE and CREATE2 of 49,152 and 49,153 zero bytes of
init code (a STOP) at lovell700 on mainnet and the block before, and holds each to its gas. Module
`vm/vm_dsl_test.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| VmDslTest.testInitCodeSizeCREATEOpcodeValidationSuccessWithoutInitcodeCost | with RSKIP438 off (`lovell700 = -1`, see under TransientStorageDslTest) a factory's CREATE succeeds and pays nothing for its init code | ported | `before_rskip438_create_and_create2_run_init_code_of_any_size_for_no_gas`: 62,237 gas at #7,338,023, where lovell700's first block uses 65,309 |
| VmDslTest.testInitCodeSizeCREATEOpcodeValidationSuccessWithInitcodeCost | with RSKIP438 the CREATE succeeds and pays 2 gas a word of its init code | ported | `from_rskip438_create_and_create2_charge_2_gas_a_word_of_init_code`: 49,152 bytes, 65,309 gas at #7,338,024 |
| VmDslTest.testInitCodeSizeCREATEOpcodeValidationDoesntFailWithRSKIP438Deactivated | with RSKIP438 off a CREATE of init code past 49,152 bytes succeeds | ported | `before_rskip438_create_and_create2_run_init_code_of_any_size_for_no_gas`: 49,153 bytes at #7,338,023 |
| VmDslTest.testInitCodeSizeCREATEOpcodeValidationFailWithRSKIP438Activated | with RSKIP438 that CREATE fails the transaction, with all its gas | ported | `from_rskip438_init_code_past_49152_bytes_fails_the_contract_that_creates` |
| VmDslTest.testInitCodeSizeCREATE2OpcodeValidationSuccessWithoutInitcodeCost | the same for CREATE2 | ported | `before_rskip438_create_and_create2_run_init_code_of_any_size_for_no_gas`: 71,456 gas at #7,338,023, where lovell700's first block uses 74,528 |
| VmDslTest.testInitCodeSizeCREATE2OpcodeValidationSuccessWithInitCodeCost | the same for CREATE2, with RSKIP438 | ported | `from_rskip438_create_and_create2_charge_2_gas_a_word_of_init_code`: 74,528 gas |
| VmDslTest.testInitCodeSizeCREATE2OpcodeValidationDoesntFailIfRSKIP438Deactivated | the same for CREATE2 | ported | `before_rskip438_create_and_create2_run_init_code_of_any_size_for_no_gas`: 49,153 bytes at #7,338,023 |
| VmDslTest.testInitCodeSizeCREATE2OpcodeValidationFailIfRSKIP438Activated | the same for CREATE2, with RSKIP438 | ported | `from_rskip438_init_code_past_49152_bytes_fails_the_contract_that_creates` |

The rest of the class was read: recursive calls 400 and 401 deep (the depth RSKIP150 allows, held by
`test_rskip150_call_depth_capped_at_400`, `executor.rs`) and PUSH0's gas against PUSH1's (held by
`push0_pushes_one_zero_word_from_rskip398`).

## org.ethereum.core.TransactionExecutorInitCodeSizeDslTest

Its 6 tests name RSKIP438: a transaction that creates a contract carries 2 gas for each word of its
init code, and one of more than 49,152 bytes of it is invalid. rskj builds a block of its DSL file's
creation and asks whether the block holds it. The port has the miner build the block
(`execute_block_for_mining`) or runs one, at lovell700 on mainnet and the block before. Module
`vm/transaction_executor_init_code_size_dsl_test.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| TransactionExecutorInitCodeSizeDslTest.testInitCodeSizeTransactionCreationValidationSuccessWithRSKIPActivated | with RSKIP438 a creation of 27,364 bytes succeeds, using 4,469,101 gas, 1,712 of them for its init code | ported | `from_rskip438_a_creation_pays_2_gas_a_word_of_its_init_code`: 49,152 bytes, 3,072 gas more at #7,338,024 than at #7,338,023 |
| TransactionExecutorInitCodeSizeDslTest.testInitCodeSizeTransactionCreationValidationFailsNotEnoughGas | the block leaves out a creation whose sender holds 4,322,017 for 4,323,189 gas at a price of 1 | ported | `a_creation_whose_sender_cannot_pay_for_its_gas_is_left_out_of_the_block`: rskj refuses it for the sender's balance (`TransactionExecutor.init`), before its gas is counted, and so does the miner |
| TransactionExecutorInitCodeSizeDslTest.testInitCodeSizeTransactionCreationValidationDoesntFailDueNotEnoughGasIfRSKIPNotActivated | with RSKIP438 off (`lovell700 = -1`, see under TransientStorageDslTest) a creation carrying all the gas it uses but its init code's price succeeds | ported | `before_rskip438_a_creation_carries_init_code_of_any_size_and_no_gas_for_it`: 49,152 bytes with their bytes' gas and no more, at #7,338,023 |
| TransactionExecutorInitCodeSizeDslTest.testInitCodeSizeTransactionCreationValidationSuccessWithRSKIP438Deactivated | with RSKIP438 off the creation uses 4,467,389 gas, nothing for its init code | ported | `before_rskip438_a_creation_carries_init_code_of_any_size_and_no_gas_for_it`: the creation uses its bytes' gas and nothing for its init code's words |
| TransactionExecutorInitCodeSizeDslTest.testInitCodeSizeTransactionCreationValidationFailsWithRSKIPActiveWithInitcodeSizeMaxReached | with RSKIP438 the block leaves out a creation of 145,961 bytes | ported | `from_rskip438_a_creation_of_more_than_49152_bytes_of_init_code_is_invalid`: 49,153 bytes; the miner leaves it out, a block that holds it is refused, and 49,152 bytes run (a local call takes past the limit: `a_local_creation_passes_the_limit_on_initcode`, `executor.rs`) |
| TransactionExecutorInitCodeSizeDslTest.testInitCodeSizeTransactionCreationValidationSuccessWithRSKIPDeactivatedWithInitcodeSizeMaxReached | with RSKIP438 off a creation of more than 49,152 bytes of init code succeeds | ported | `before_rskip438_a_creation_carries_init_code_of_any_size_and_no_gas_for_it`: 49,153 bytes at #7,338,023 |

## org.ethereum.cost.InitcodeCostCalculatorTest

Its one test names RSKIP438, on a mocked activation. Module `vm/initcode_cost_calculator_test.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| InitcodeCostCalculatorTest.testTransactionInitCodeCostCalculation | with RSKIP438 init code of 4, 40 and 71 bytes costs 2, 4 and 6 gas | ported | `from_rskip438_a_creation_carries_2_gas_for_each_word_of_init_code`: at #7,338,024 a creation of each needs that much more gas than at #7,338,023, and the miner leaves it out a gas short (`test_rskj_initcode_cost_shanghai`, `executor.rs`, holds only a lower bound) |

## co.rsk.test.DslFilesTest

2 of its 30 tests name a rule, by unscheduling its upgrade on rskj's test regtest: RSKIP438
(lovell700) and RSKIP174 (iris300). Module `vm/dsl_files_test.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| DslFilesTest.runCreate01ResourceBeforeActivationOfRSKIP438 | with RSKIP438 off (`lovell700 = -1`, see under TransientStorageDslTest) the creation of solc's empty contract (`create01.txt`) uses 64,857 gas | ported | `a_creation_of_rskjs_empty_contract_uses_6_gas_more_from_rskip438`: 64,857 at #7,338,023 and 64,863 at #7,338,024 |
| DslFilesTest.runCreate01Resource (rest of the class) | the same creation uses 64,863 gas with every upgrade on | ported | `a_creation_of_rskjs_empty_contract_uses_6_gas_more_from_rskip438` (64,863 at #7,338,024) |
| DslFilesTest.runCreateContractAndPreserveNoBalance | with RSKIP174 off (`iris300 = -1`, see under TransientStorageDslTest) a deployment onto an address that holds 100 wei leaves it none | covered | `a_pre_rskip174_deployment_burns_the_balance_already_at_the_address` (`executor.rs`), at testnet's #2,060,499; `from_rskip174_a_deployment_keeps_the_balance_already_at_the_address` holds the block after |

The rest of the class was read: runCreateContractAndPreserveBalance (RSKIP174 on, held by
`from_rskip174_a_deployment_keeps_the_balance_already_at_the_address`), and the accounts, transfers,
contracts, logs, blocks, uncles, reorganisations and creations after a SELFDESTRUCT of its DSL files,
which run on the test regtest with no rule switched.

The tests of these four classes that rskj runs with an upgrade at `-1` are ported at mainnet's or
testnet's own heights, for the reason given under TransientStorageDslTest.
rskj turns RSKIP438 off by setting `hardforkActivationHeights.lovell700 = -1` on its test regtest
(`co/rsk/vm/VmDslTest.java:180-182`, `org/ethereum/core/TransactionExecutorInitCodeSizeDslTest.java:55-57`,
`co/rsk/test/DslFilesTest.java:93-95`), and RSKIP174 by `iris300 = -1` (`DslFilesTest.java:227-229`),
where every later upgrade is at block 0. RSKIP438 is lovell700's and RSKIP174 iris300's
(`rskj-core/src/main/resources/reference.conf:90` and `:46`), and `ActivationConfig.isActive` reads
that height, so init code costs nothing (`org/ethereum/cost/InitcodeCostCalculator.java:37-42`), is
not held to 49,152 bytes (`org/ethereum/core/Transaction.java:221-227`,
`org/ethereum/vm/program/Program.java:614-624`), and a deployment writes a new account over its
address's balance (`org/ethereum/core/TransactionExecutor.java:401-403`). rustock runs revm's
SHANGHAI spec, and with it EIP-3860's price and limit on init code, for lovell700 and every later
upgrade (`upgrade_to_spec_id`, `crates/execution/src/hardfork.rs:1087-1121`), in force where the
latest upgrade scheduled is (`spec_id`, `hardfork.rs:460-462`); its `has_rskip174`
(`hardfork.rs:819-821`) asks the same of iris300. With vetiver900 at block 0 both are on. Mainnet
and testnet schedule every upgrade in order, so the two agree on both.

## co.rsk.remasc.RemascProcessMinerFeesTest

5 of its 15 tests name a rule: they turn RSKIP85 (orchid) off (`ActivationConfigsForTest.allBut`),
under which REMASC pays every block whatever the minimum gas price, and pays the federation every
block. rskj runs `RemascTestRunner`: blocks 1 to 16 of a regtest chain, each with a transfer whose
21,000 gas is its fees and REMASC's transaction, and siblings included as uncles; block 16 pays for
block 6, 21,000 of a reward balance of 105,000: 4,200 to RSK Labs, 168 to the federation, a tenth of
the 16,632 left to the miners that included the siblings and the rest to block 6's miner and the
siblings' miners, a sibling losing a twentieth of its share for each block it came in late. The port
runs the same chain through `RskExecutor::execute_block`, each block kept in the block store as an
imported block is, on regtest's REMASC constants with RSKIP85 at no height. rskj's runner gives its
blocks a minimum gas price of 10, over their transfers' price of 1, with its block validation off;
rskj's validation (`co/rsk/validators/TxsMinGasPriceRule.java`) and rustock's execution
(`GasPriceLessThanBasefee`) refuse such a block, so the port's blocks have their transfers' price as
their minimum, on which no figure depends with RSKIP85 off. Modules `remasc/runner.rs` and
`remasc/remasc_process_miner_fees_test.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| RemascProcessMinerFeesTest.siblingIncludedOneBlockLater | without RSKIP85, a sibling of block 6 that block 8 includes: 1,663 to block 8's miner, 7,484 to block 6's, 7,110 to the sibling's; reward balance 84,000, burned 375 | ported | `a_sibling_included_a_block_late_loses_a_twentieth_of_its_share` |
| RemascProcessMinerFeesTest.oneSiblingIncludedOneBlockLaterAndAnotherIncludedRightAfter | siblings of block 6 that blocks 7 and 8 include: 831 to each includer, 4,989 to block 6's miner and the first sibling's, 4,740 to the second's; burned 252 | ported | `siblings_included_by_two_blocks_share_the_reward_with_their_block` |
| RemascProcessMinerFeesTest.siblingIncludedSevenBlocksLater | a sibling of block 6 that block 13 includes: 5,239 to its miner; REMASC holds 296,246; burned 2,246 | ported | `a_sibling_included_six_blocks_late_loses_six_twentieths_of_its_share` |
| RemascProcessMinerFeesTest.siblingsFeeForMiningBlockMustBeRoundedAndTheRoundedSurplusBurned | nine siblings of block 6 that block 7 includes: 1,656 to block 7's miner, 1,496 to each other miner, no account for block 8's; burned 16 | ported | `nine_siblings_shares_are_rounded_down_and_the_surplus_burned` |
| RemascProcessMinerFeesTest.unclesPublishingFeeMustBeRoundedAndTheRoundedSurplusBurned | ten siblings: 1,660 to block 7's miner, 1,360 to each other; burned 12 | ported | `ten_siblings_publishing_shares_are_rounded_down_and_the_surplus_burned` |

The rest of the class was read: its other ten tests run with every rule on, and `executor.rs`'s
`remasc_*` tests (from `remasc_no_distribution_before_maturity`) port those they name. Of the two
refusals of REMASC invoked other than as its block's last transaction, the transaction's own call is
held by `test_remasc_precompile_direct_call` (`executor.rs`); the contract's call
(processMinersFeesFromTxInvokedByAnotherContract) by `a_contract_calling_remasc_is_refused_as_rskj_refuses_it`.

## co.rsk.remasc.RemascStorageProviderTest

3 of its 18 tests turn RSKIP85 off, on the same runner. Module `remasc/remasc_storage_provider_test.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| RemascStorageProviderTest.setSaveRetrieveAndGetSiblingsBeforeRFS | without RSKIP85, transfers at a gas price of 0 and siblings of blocks 6 and 11: reward and burned balances 0 | ported | `without_fees_remasc_keeps_nothing_for_blocks_with_siblings` |
| RemascStorageProviderTest.alwaysPaysBeforeRFS | without RSKIP85: reward balance 84,000 after block 16, burned 0 | ported | `without_rskip85_remasc_pays_every_block` |
| RemascStorageProviderTest.alwaysPaysFedBeforeRFS | without RSKIP85 the federation balance stays 0, and the first federator holds twice its share of 168 | ported | `without_rskip85_the_federation_is_paid_every_block`, on the genesis federation of mainnet's Bridge constants (rustock's regtest constants hold no federation keys) |
| RemascStorageProviderTest.doesntPayBelowMinimumRewardAfterRFS (rest of the class) | with RSKIP85 a reward under the minimum gas price times the minimum payable gas waits: reward balance 126,000 | ported | `from_rskip85_a_reward_under_its_floor_waits_in_the_reward_balance`: the same floor of 100,000 (rskj's 10,000 gas at 10, here 100,000 at 1) |
| RemascStorageProviderTest.paysWhenHigherThanMinimumRewardAfterRFS (rest of the class) | with RSKIP85 a reward at its floor is paid: reward balance 840,000 | ported | `from_rskip85_a_reward_at_its_floor_is_paid` |
| RemascStorageProviderTest.doesntPayFedBelowMinimumRewardAfterRFS (rest of the class) | with RSKIP85 a federator's share under its floor accrues: federation balance 336, no account for the first federator | ported | `from_rskip85_a_federators_share_under_its_floor_accrues` |
| RemascStorageProviderTest.paysFedWhenHigherThanMinimumRewardAfterRFS (rest of the class) | with RSKIP85 a federator's share over its floor is paid: federation balance 0, the first federator twice its share of 1,680 | ported | `from_rskip85_a_federators_share_over_its_floor_is_paid` |

The rest of the class was read: each field's default, write and read back (encodings held by
`remasc_state_encodes_as_rskj_encodes_it` and the storage-key tests of `remasc.rs`),
setSaveRetrieveAndGetSiblingsAfterRFS (the same balances with every rule on), and
paysOnlyBlocksWithEnoughBalanceAccumulatedAfterRFS, not ported: its blocks carry transactions priced
under their own minimum gas price, which no valid block does; the floor it reaches is held by the two
reward ports above.

## co.rsk.remasc.RemascFederationProviderTest

2 of its 3 tests name RSKIP415 (arrowhead600): REMASC pays a federator at the address of its BTC
public key before it and of its RSK public key from it. rskj asks `getFederatorAddress` of a mocked
federation; the port has the Bridge hold a federation of one member with rskj's two keys, and runs
the runner's chain with RSKIP85 off, so the federation's 168 of blocks 15 and 16 is paid. Module
`remasc/remasc_federation_provider_test.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| RemascFederationProviderTest.getFederatorAddress_preRSKIP415_returnsRskAddressDerivedFromBtcPublicKey | without RSKIP415 the federator's address is `131c7c82...`, its BTC key's | ported | `before_rskip415_remasc_pays_a_federator_at_its_btc_keys_address`: every upgrade from arrowhead600 on unscheduled, the 336 goes there and none to the RSK key's address |
| RemascFederationProviderTest.getFederatorAddress_postRSKIP415_returnsRskAddressDerivedFromRskPublicKey | with RSKIP415 it is `54a948b3...`, its RSK key's | ported | `from_rskip415_remasc_pays_a_federator_at_its_rsk_keys_address` (the key selection alone: `rsk_keys_selects_rsk_field_and_legacy_equals_btc`, `bridge/federation.rs`) |

The rest of the class (the regtest federation's size) names no rule.

## co.rsk.remasc.RemascRskAddressActivationTest

Its one test names RSKIP218 (iris300). Module `remasc/remasc_rsk_address_activation_test.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| RemascRskAddressActivationTest.testActivation | RSK Labs' address is `14d3065c...` without RSKIP218 and `dcb12179...` with it | ported | `rsk_labs_cut_goes_to_its_rskip218_address_from_rskip218`: with RSKIP218 at block 16, block 15 pays RSK Labs' 4,200 to the first and block 16 to the second. rustock reads RSKIP218's height from its REMASC constants (`RemascConfig::labs_address_at`, iris300 on mainnet), held to rskj's by `remasc_config_mainnet_values` |

## co.rsk.vm.ContractCodePrefixDslTest

Its 5 tests name RSKIP544 (vetiver900, EIP-3541), switched by `consensusRules.rskip544`, which
moves that rule alone; mainnet's window is its equivalent.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| ContractCodePrefixDslTest.testCreateFailsWithEFByteAfterRSKIP544Activation | with RSKIP544 a creation of code starting with 0xEF fails, with all its 300,000 gas | covered | `test_rskip544_create_ef_code_fails_after_vetiver900` (`executor.rs`, #8,804,200) |
| ContractCodePrefixDslTest.testCreateSucceedsWithEFByteBeforeRSKIP544Activation | without it that creation succeeds | covered | `test_rskip544_create_ef_code_succeeds_before_vetiver900` (#8,804,199) |
| ContractCodePrefixDslTest.testCreateSucceedsWithFEByte | with it a creation of code starting with 0xFE succeeds | covered | `test_rskip544_create_fe_code_succeeds_after_vetiver900` |
| ContractCodePrefixDslTest.testCreateSucceedsWithEmptyCode | with it a creation of no code succeeds | covered | `test_rskip544_create_empty_code_succeeds_after_vetiver900` |
| ContractCodePrefixDslTest.testGasConsumptionComparison | the failed creation uses more than 21,000, the 0xFE one between 21,000 and 300,000 | covered | `test_rskip544_create_ef_code_fails_after_vetiver900` (300,000) and `test_rskip544_create_fe_code_succeeds_after_vetiver900` |

## co.rsk.vm.RSKIP544Test

1 of its tests is listed: RSKIP544's activation, on CREATE run by a program with the rule mocked on
or off. Module `vm/rskip544_test.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| RSKIP544Test.testActivationBoundary_Parameterized | CREATE of code 0xEF or 0xEF00 deploys without RSKIP544 and pushes 0 with it; of 0xFE deploys either way | ported | `from_rskip544_create_deploys_no_code_that_starts_with_0xef`: rskj's init codes, at #8,804,199 and #8,804,200, read back as the code at the new address (RSK gives a CREATE all the gas left, so the creating contract stops after it) |

The rest of the class was read: the other prefixes and salts, and CREATE2, all with the rule on, the
same refusal (a transaction's creation is held by the `test_rskip544_*` tests).

## co.rsk.vm.Create2Test

2 of its tests are listed, with RSKIP125 (wasabi100) mocked off. Module `vm/create2_test.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| Create2Test.testCREATE_CheckFunctionBeforeRSKIP | without RSKIP125 CREATE's new contract has nonce 0 | covered | `test_created_contract_starts_with_nonce_zero` (`executor.rs`, mainnet #892,228), and `test_internal_create_nonce_one_post_rskip125` from it |
| Create2Test.testCREATE2ShouldFailInvalidOpcode | without RSKIP125 CREATE2 is an invalid opcode | ported | `create2_is_an_invalid_opcode_before_rskip125`: fails with all its gas at #1,590,999, creates at #1,591,000 |

## co.rsk.vm.VMSpecificOpcodesPerformanceTest

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| VMSpecificOpcodesPerformanceTest.testCallsToExistentAccounts | 755 gas a loop of CALLs to an existing account | not applicable | `@Disabled` in rskj, a manual performance measure |
| VMSpecificOpcodesPerformanceTest.testCallsToNonExistingContracts | 25,769 gas a loop of CALLs to accounts that don't exist | not applicable | `@Disabled` in rskj, a manual performance measure |

## org.ethereum.core.ContractCreatingDslRollbackTest

2 of its tests are listed, with RSKIP453 (lovell700) off by `consensusRules.rskip453`, which moves
that rule alone. Module `vm/contract_creating_dsl_rollback_test.rs`.

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| ContractCreatingDslRollbackTest.testCreateExecutionBefore453isNotRollingBackHavingNotEnoughGaa | without RSKIP453 a contract's creation whose code its gas can't pay for leaves the transaction successful | covered | `test_pre_rskip453_internal_create_with_unpayable_deposit_deploys_empty` (`executor.rs`, mainnet #1,071,481) |
| ContractCreatingDslRollbackTest.createExecutionExceedingCodeSizeDoesNotRollbackBefore453 | without RSKIP453 a contract's creation of code past 24,576 bytes leaves the transaction successful | ported | `before_rskip453_code_past_the_limit_leaves_its_contract_empty`: at #7,338,023 the new contract is there with no code (`Program.exceedsMaxContractSize`, then `finalizeContractCreation`) |
| ContractCreatingDslRollbackTest.createExecutionExceedingCodeSizeDoesRollbackAfter453 (rest of the class) | with RSKIP453 that transaction fails | ported | `from_rskip453_code_past_the_limit_fails_the_creation`: at #7,338,024, and no account at the address |

## co.rsk.vm.ExtCodeHashTest

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| ExtCodeHashTest.testDoEXTCODEHASHWithHardForkDeactivated | without RSKIP140 (papyrus200) EXTCODEHASH is an invalid opcode | ported | `extcodehash_is_an_invalid_opcode_before_rskip140` (`vm/ext_code_hash_test.rs`): fails with all its gas at #2,392,699, pushes the code's hash at #2,392,700 |

The rest of the class (the hash of a contract, of an account without code, of no account) is held
by `test_extcodehash_existing_empty_account_is_keccak_empty` (`executor.rs`) and the ports of
`vm/ext_code_hash_test.rs` (`extcodehash_of_an_account_without_code_is_the_hash_of_nothing`,
`extcodehash_of_no_account_is_0`); before RSKIP169, by
`before_rskip169_extcodehash_of_a_contract_whose_constructor_runs_is_0` and the two beside it
(`vm/evm_rules.rs`).

## co.rsk.vm.opcode.BasefeeDslTest

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| BasefeeDslTest.testBASEFEE_whenNotActivated_BehavesAsExpected | with RSKIP412 (arrowhead600) mocked off, BASEFEE is an invalid opcode | covered | `test_basefee_activates_at_arrowhead600` (`executor.rs`) |

## co.rsk.vm.opcode.MCopyDslTest

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| MCopyDslTest.testMCOPY_whenNotActivated_behavesAsExpected | with RSKIP445 off (`lovell700 = -1`, see under TransientStorageDslTest) the transaction that runs MCOPY fails | covered | `mcopy_is_an_invalid_opcode_before_rskip445` (`vm/vm_execution_test.rs`), in mainnet's last block before lovell700 |

MCopyGasTest and MCopyInputTest name RSKIP445 in their set-up only; MCOPY's gas and inputs are held
by `vm_execution_test.rs`'s MCOPY ports.

## org.ethereum.core.NestedCallsStackDepthTest

| rskj test | asserts | verdict | rustock test or reason |
|---|---|---|---|
| NestedCallsStackDepthTest.testContractConsensusInactive | with RSKIP203 off (`arrowhead600 = -1`, see under TransientStorageDslTest) a contract's call of the Environment contract's getCallStackDepth reverts: no contract is there | ported | `the_environment_contract_answers_a_contracts_call_from_rskip203` (`vm/nested_calls_stack_depth_test.rs`): at #6,223,699 the call gets nothing back, where rskj's Solidity caller reverts for want of an answer |
| NestedCallsStackDepthTest.testContractCallsGetCallStackDepth | a contract's call of getCallStackDepth is answered 1 | ported | `the_environment_contract_answers_a_contracts_call_from_rskip203`: one word, 1, at #6,223,700 |
| NestedCallsStackDepthTest.testNestedContractCallsGetCallStackDepth | a call three contracts deep is answered 3 | ported | `a_call_three_contracts_deep_is_answered_3` |

rskj turns each of these two rules off by leaving its upgrade out (`co/rsk/vm/opcode/MCopyDslTest.java:30`,
`org/ethereum/core/NestedCallsStackDepthTest.java:93`); the ports run at mainnet's own heights, for the
reason given under TransientStorageDslTest. NestedCallsStackDepthTest's other two tests are ported with it.

