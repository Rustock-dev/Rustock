# rskj tests and their rustock counterparts

One row per rskj test in the consensus packages, with one of three verdicts: **covered**
(the rustock test that asserts the same behaviour), **not applicable** (why rustock needs no
such test), or **gap** (no rustock test; the last column says whether the code has the
behaviour anyway). A gap whose code check found the behaviour missing or different is an
issue; the ones found on 2026-10-30 are filed in rsk-clients-lab's `docs/issues/`.

Why this table exists: two bugs found by rsk-clients-lab on 2026-10-29 (the miner and the
RSKIP252 cap; `eth_gasPrice` and the fee-market gate) each had an rskj test naming the missing
behaviour, in a class other than the one the port had sliced on. Neither shows on mainnet or
regtest, so syncing cannot catch this kind of gap. The test names can.

rskj at e507f45; rustock at the commit this file first appears in. Test methods counted by
`@Test`, `@ParameterizedTest` and `@RepeatedTest` annotations. The mapping was done by reading
each rskj test body and searching rustock by rule name, RSKIP number and the rskj class name
(many rustock tests cite it); the code checks on gaps were then re-verified by hand for the
consensus rules. Keep the table current: a port that adds a test flips its row to covered.

| package | tests | covered | not applicable | gap |
|---|---:|---:|---:|---:|
| `co.rsk.validators` | 77 | 67 | 10 | 0 |
| `co.rsk.mine` | 110 | 104 | 6 | 0 |
| `co.rsk.mine.gas`, at the release | 24 | 24 | 0 | 0 |
| `co.rsk.config.mining`, at the release | 13 | 13 | 0 | 0 |
| `co.rsk.net.handler.txvalidator and org.ethereum.facade` | 24 | 19 | 5 | 0 |
| total | 248 | 227 | 21 | 0 |

Gaps where the code differs from rskj, in order of weight:

1. RSKIP179, the Bitcoin-to-RSK timestamp link, was not enforced (`BlockTimeStampValidationRuleTest`); fixed the same day, `BitcoinTimestampRule`.
2. The future-timestamp bound was 15 s on every chain; rskj uses 60 s on mainnet, 540 s on testnet and none on regtest. The minimum gas limit was 5,000 against rskj's 3,000,000. Both fixed the same day.
3. A pending transaction the EVM refused aborted the whole template instead of being dropped and evicted (`BlockToMineBuilderInvalidTxEvictionTest`), and the miner never consulted sync state (`MinerManagerTest.mineBlockWhileSyncingBlocks`). Both fixed the same day.
4. Pool admission lacked the 80x per-transaction price cap, the sublist gas limit under RSKIP144, the canonical-nonce check and the free-bridge exemption (`co.rsk.net.handler.txvalidator`). Fixed the same day; the sublist bound is tested at `gas_limit_is_bounded_by_the_larger_sublist_under_rskip144`.
5. Miner behaviours rskj had and rustock did not: submission rate limit, duplicate submission answered OK, template rebuilt on new block and new transactions, the extraData format, the minimum gas price target, eviction of underpriced transactions. All six fixed the same day.
6. A timed miner started again right after a stop ran a second thread beside the stopped one (`TimedMinerClientSimpleTest.testMultipleStartStopCycles`), and `evm_increaseTime` moved the clock back on a negative value, which rskj's `MinerClock` ignores (`MinerClockTest`). Both fixed, with the tests of their rows.
7. rustock had no stable minimum gas price (rskj's `miner.stableGasPrice`, under `co.rsk.mine.gas` below): its miner's minimum gas price stepped toward the configured one only. Fixed: the stable price, its web and contract sources and its settings, with every test of `co.rsk.mine.gas` and `co.rsk.config.mining` ported.

## rskj's release, area by area

Since this table, rustock was checked against the tests of rskj's release VETIVER-9.0.4 that name a consensus rule or a network upgrade, area by area, in one table each under `docs/rskj-tests/`. A row there is one rskj test, with the verdicts above and two more: **ported** (a test written for it, in an `rskj_tests` folder of the crate it tests) and **fixed** (a ported test that found rustock doing otherwise than rskj; rustock was put right, the test passes, and the row says what differed). The rows below were held against the same release where those tables say so (the validators, in the blocks table), and flipped where a test has been written since.

| table | rskj tests | ported | covered | not applicable | fixed | other |
|---|---:|---:|---:|---:|---:|---|
| [The VM, the precompiled contracts and REMASC](rskj-tests/vm.md) | 218 | 170 | 44 | 4 | 0 |  |
| [Headers, block rules, a block's run, the miner and the pool](rskj-tests/blocks.md) | 245 | 107 | 100 | 36 | 1 | 1 not in the release |
| [The Bridge's methods, and Flyover's](rskj-tests/bridge-methods.md) | 294 | 254 | 24 | 12 | 4 |  |
| [The federation, its scripts and the Bridge's utilities](rskj-tests/bridge-federation.md) | 151 | 113 | 16 | 18 | 4 |  |
| [Peg-ins and the federations' stored state](rskj-tests/bridge-pegins.md) | 150 | 128 | 10 | 10 | 2 |  |
| [The Bridge's storage, peg-outs, signatures and events](rskj-tests/bridge-pegouts.md) | 181 | 153 | 12 | 14 | 2 |  |
| total | 1,239 | 925 | 206 | 94 | 13 | 1 |

## co.rsk.validators

Paths: rskj tests `rskj-core/src/test/java/co/rsk/validators/`; rustock files relative to the rustock checkout (`crates/...`). `tests.rs` = `crates/core/src/validation/tests.rs`, `block_rules.rs` = `crates/core/src/validation/block_rules.rs`, `header_rules.rs` = `crates/core/src/validation/header_rules.rs`.

| rskj test | verdict | rustock test or reason | code check (gaps only) |
|---|---|---|---|
| BlockDifficultyRuleTest.validWhenCalculatedDifficultyMatches | covered | `test_difficulty_rule` (tests.rs) | |
| BlockDifficultyRuleTest.invalidWhenCalculatedDifficultyDoesntMatch | covered | `a_difficulty_other_than_the_calculated_one_is_refused` (`rustock-core`, `block_difficulty_rule_test.rs`): one more and one less refused (ported, in an `rskj_tests/blocks/` folder; the [blocks table](rskj-tests/blocks.md)) | |
| BlockDifficultyValidationRuleTest.testDifficulty | covered | `test_difficulty_rule`, `test_difficulty_equal_timestamps_increases` (tests.rs; regtest +1/2048 step) | |
| BlockParentGasLimitRuleTest.cantConstructRuleWithZeroGasLimitBoundDivisor | not applicable | Java constructor arg check; rustock divisor is a config constant (1024, `crates/core/src/config.rs:188`) | |
| BlockParentGasLimitRuleTest.cantConstructRuleWithNegativeGasLimitBoundDivisor | not applicable | same; `u64` cannot be negative | |
| BlockParentGasLimitRuleTest.validityWhenGas | covered | `test_gas_limit_rule` (tests.rs; same, +delta ok, +delta+1 rejected) | |
| BlockParentNumberRuleTest.isValid | covered | `test_block_number_rule` (tests.rs) | |
| BlockRootValidationRuleTest.validBlockPasses | covered | `test_process_block_validates_tx_root` (crates/execution/src/processor.rs) | |
| BlockRootValidationRuleTest.calculatedRootMatchesHelperForBothFlags | covered | `the_derived_body_matches_the_committed_transactions_root` (crates/core/src/types/transaction.rs) + processor tx-root test | |
| BlockRootValidationRuleTest.replacedTransactionsListInvalidatesCachedRoot | not applicable | Java `Block` cached-root invalidation; rustock recomputes the root each time (`processor.rs:833`) | |
| BlockRootValidationRuleTest.mismatchedHeaderRootIsRejected | covered | `test_process_block_validates_tx_root` (processor.rs:2489, asserts `TransactionsRootMismatch`) | |
| BlockTimeStampValidationRuleTest.timestampsAreCloseEnough | covered | `timestamps_are_close_enough` (tests.rs), with `bitcoin_timestamp_is_not_checked_before_rskip179`, `a_header_without_a_decodable_bitcoin_header_fails_rskip179`, `testnet_widens_the_window_from_rskip297` | |
| BlockTimeStampValidationRuleTest.blockInThePast | covered | `block_in_the_past` (tests.rs) | |
| BlockTimeStampValidationRuleTest.blockInTheFutureLimit | covered | `block_in_the_future_limit` (tests.rs) | |
| BlockTimeStampValidationRuleTest.blockInTheFuture | covered | `block_in_the_future` (tests.rs) | |
| BlockTimeStampValidationRuleTest.blockTimeLowerThanParentBlockTime | covered | `block_time_against_the_parents` (tests.rs) | |
| BlockTimeStampValidationRuleTest.blockTimeLowerThanParentBlockHeaderTime | covered | `block_time_against_the_parents` (tests.rs) | |
| BlockTimeStampValidationRuleTest.blockTimeGreaterThanParentBlockTime | covered | `block_time_against_the_parents` (tests.rs) | |
| BlockTimeStampValidationRuleTest.blockTimeGreaterThanParentBlockHeaderTime | covered | `block_time_against_the_parents` (tests.rs) | |
| BlockTimeStampValidationRuleTest.blockTimeEqualsParentBlockTime | covered | `block_time_against_the_parents` (tests.rs) | |
| BlockTimeStampValidationRuleTest.blockTimeEqualsParentBlockHeaderTime | covered | `block_time_against_the_parents` (tests.rs) | |
| BlockTxsMaxGasPriceRuleTest.isValid_whenRskip252DisabledThenBlockValidRegardlessGasPrice | covered | `block_txs_max_gas_price_enforces_the_100x_cap_from_rskip252` (block_rules.rs; pre-fingerroot500 block accepted) | |
| BlockTxsMaxGasPriceRuleTest.isValid_whenRskip252EnabledThenBlockInvalidDueToTxWithMoreGasPriceThanCap | covered | same test (10_001 > 100x100 rejected) | |
| BlockTxsValidationRuleTest.validNonceSame | covered | `a_transaction_at_its_senders_nonce_is_valid` (`rustock-execution`, `block_txs_validation_rule_test.rs`), a block run on mainnet's #8,000,000. rskj refuses such a block before it runs (`co/rsk/validators/BlockTxsValidationRule.java`), rustock as it runs, by revm's check of the nonce, which each refusal is held to be. The table had these seven as gaps (ported, in an `rskj_tests/blocks/` folder; the [blocks table](rskj-tests/blocks.md)) | |
| BlockTxsValidationRuleTest.invalidNonceLower | covered | `a_transaction_below_or_above_its_senders_nonce_is_refused` (ported, in an `rskj_tests/blocks/` folder; the [blocks table](rskj-tests/blocks.md)) | |
| BlockTxsValidationRuleTest.invalidNonceHigher | covered | the same (ported, in an `rskj_tests/blocks/` folder; the [blocks table](rskj-tests/blocks.md)) | |
| BlockTxsValidationRuleTest.validTransactionsWithSameNonceAndDifferentSenders | covered | `two_senders_may_send_at_the_same_nonce` (ported, in an `rskj_tests/blocks/` folder; the [blocks table](rskj-tests/blocks.md)) | |
| BlockTxsValidationRuleTest.validConsecutiveTransactionsFromSameSender | covered | `a_senders_nonce_counts_up_through_the_block` (ported, in an `rskj_tests/blocks/` folder; the [blocks table](rskj-tests/blocks.md)) | |
| BlockTxsValidationRuleTest.invalidTransactionsFromSameSenderWithSkippedNonce | covered | the same (ported, in an `rskj_tests/blocks/` folder; the [blocks table](rskj-tests/blocks.md)) | |
| BlockTxsValidationRuleTest.invalidTransactionsWithSameNonceAndSameSender | covered | the same (ported, in an `rskj_tests/blocks/` folder; the [blocks table](rskj-tests/blocks.md)) | |
| BlockUnclesValidationRuleTest.rejectBlockWithSiblingUncle | covered | `a_sibling_of_the_block_is_no_uncle` (`rustock-core`, `block_uncles_validation_rule_test.rs`), with a child of genesis accepted (ported, in an `rskj_tests/blocks/` folder; the [blocks table](rskj-tests/blocks.md)) | |
| BlockUnclesValidationRuleTest.rejectBlockWithUncleHavingHigherNumber | covered | `a_block_above_the_one_naming_it_is_no_uncle` (`block_uncles_validation_rule_test.rs`) (ported, in an `rskj_tests/blocks/` folder; the [blocks table](rskj-tests/blocks.md)) | |
| ExtraDataRuleTests.blockWithValidExtraData | covered | `extra_data_rule_caps_at_32_bytes` (block_rules.rs) | |
| ExtraDataRuleTests.blockWithValidNullExtraData | not applicable | `extra_data` is a non-null `Bytes` in rustock; empty bytes trivially pass `header_rules`/`block_rules.rs:125` | |
| ExtraDataRuleTests.blockWithValidLongerExtraDataThanAccepted | covered | `extra_data_rule_caps_at_32_bytes` (33 bytes -> `ExtraDataTooLarge`) | |
| ForkDetectionDataRuleTest.validForBlocksBeforeRskip110UsingMethodThatReceivesBlockAsParameter | covered | `before_rskip110_no_fork_detection_data_is_asked_for` (`crates/core/src/rskj_tests/blocks/fork_detection_data_rule_test.rs`, as the rest of this class) | |
| ForkDetectionDataRuleTest.validForBlocksBeforeRskip110 | covered | `before_rskip110_no_fork_detection_data_is_asked_for` | |
| ForkDetectionDataRuleTest.invalidForRskip110ActiveButForkDetectionData | not applicable | the test's mock reports data that rskj's own header never carries below block 449 ([blocks table](rskj-tests/blocks.md)) | |
| ForkDetectionDataRuleTest.validForRskip110ActiveButNoForkDetectionDataBecauseNoEnoughBlocksToCalculateIt | covered | `below_block_449_a_block_carries_no_fork_detection_data`; block 449 itself, which rskj leaves unchecked, by `fork_detection_starts_above_block_449` (tests.rs) | |
| ForkDetectionDataRuleTest.invalidForRskip110ActiveAndForkDetectionDataButMissingBlocksForCalculation | covered | `a_chain_too_short_to_calculate_from_refuses_the_block` | |
| ForkDetectionDataRuleTest.validForRskip110ActiveAndForkDetectionData | covered | `fork_detection_data_matching_the_chain_is_valid` | |
| ForkDetectionDataRuleTest.invalidForRskip110ActiveAndForkDetectionDataBecauseDataDoesNotMatch | covered | `fork_detection_data_not_matching_the_chain_is_refused` | |
| GasLimitRuleTests.gasLimitGreaterThanMinimumGasLimit | covered | `gas_limit_greater_than_minimum_gas_limit` (tests.rs) | |
| GasLimitRuleTests.gasLimitEqualMinimumGasLimit | covered | `gas_limit_equal_minimum_gas_limit` (tests.rs) | |
| GasLimitRuleTests.gasLimitLessThanMinimumGasLimit | covered | `gas_limit_less_than_minimum_gas_limit` (tests.rs) | |
| PrevMinGasPriceValidatorTest.noParentBlock | not applicable | rustock's `ParentHeaderValidator` always receives a parent; a missing parent is rejected earlier by the sync pipeline | |
| PrevMinGasPriceValidatorTest.genesisBlock | covered | `genesis_is_held_to_no_parents_minimum_gas_price` (`rustock-core`, `prev_min_gas_price_validator_test.rs`) (ported, in an `rskj_tests/blocks/` folder; the [blocks table](rskj-tests/blocks.md)) | |
| PrevMinGasPriceValidatorTest.noMinGasPrice | not applicable | `minimum_gas_price` is a non-null `U256` in rustock | |
| PrevMinGasPriceValidatorTest.outOfValidMGPRangeBlock | covered | `prev_min_gas_price_allows_one_percent_either_way` (block_rules.rs; 989/1011 rejected around 1000) | |
| PrevMinGasPriceValidatorTest.validMGPInNewBlock | covered | same test (1050 vs parent 1000 is outside +1%, rejected like 1011) | |
| RemascValidationRuleTest.noTxInTheBlock | covered | `remasc_rule_requires_remasc_as_the_last_transaction` (block_rules.rs; empty block err) | |
| RemascValidationRuleTest.noRemascTxInTheBlock | covered | same (absent case) | |
| RemascValidationRuleTest.remascTxIsNotTheLastOne | covered | same (misplaced case) | |
| RemascValidationRuleTest.remascTxInBlock | covered | same (ok case) | |
| TxGasPriceCapTest.isSurpassed_whenMinGasPriceIsZeroThenFalseRegardlessTxGasPrice | covered | `block_txs_max_gas_price_enforces_the_100x_cap_from_rskip252` (zero_min case), FOR_BLOCK only; FOR_TRANSACTION (txpool cap) is outside this package's consensus scope | |
| TxGasPriceCapTest.isSurpassed_whenRemascTransactionThenFalseRegardlessTxGasPrice | not applicable | no such transaction exists: rskj makes a `RemascTransaction` only of a transaction priced at zero (`org/ethereum/core/Transaction.java` 568-590), and rustock's `Transaction::is_remasc` asks the same (`remasc_is_rskjs_is_remasc_transaction`, `crates/core/src/types/transaction.rs`). A zero price is under every cap, so the exemption (`TxGasPriceCap.java` 44-46) changes no result (the [blocks table](rskj-tests/blocks.md)) | |
| TxGasPriceCapTest.isSurpassed_whenLessThanCapThenFalse | covered | same test (at_cap ok implies below-cap ok; `>` compare at `block_rules.rs:87`) | |
| TxGasPriceCapTest.isSurpassed_whenEqualsCapThenFalse | covered | same test (at_cap = 10_000 accepted) | |
| TxGasPriceCapTest.isSurpassed_whenMoreThanCapThenTrue | covered | same test (10_001 rejected) | |
| TxsMinGasPriceValidatorTest.blockWithEmptyTxList | covered | `txs_min_gas_price_rejects_an_underpriced_tx` (only_remasc case; loop over zero user txs) | |
| TxsMinGasPriceValidatorTest.blockWithNullMGP | not applicable | `minimum_gas_price` is non-null `U256` | |
| TxsMinGasPriceValidatorTest.blockWithAllValidTx | covered | same test (ok case) | |
| TxsMinGasPriceValidatorTest.blockWithAllInvalidTx | covered | same test (99 < 100 -> `TxGasPriceBelowMinimum`) | |
| TxsMinGasPriceValidatorTest.blockWithSomeInvalidTx | covered | same test (first-failing-tx rejects the block, `block_rules.rs:45`) | |
| ValidGasUsedValidatorTest.blockWithValidGasUsed | covered | `test_gas_used_rule` (tests.rs) | |
| ValidGasUsedValidatorTest.blockWithInvalidGasUsedBiggerThanGasLimit | covered | `test_gas_used_rule` (110 > 100 -> `GasUsedExceedsLimit`); also `test_rskj_invalid_block_bad_gas_used` (processor.rs) | |
| ValidGasUsedValidatorTest.blockWithInvalidGasUsedLessThanZero | not applicable | `gas_used` is `u64`; negative is unrepresentable (RLP decode of a negative is impossible) | |
| ValidTxExecutionSublistsEdgesTest.blockWithRSKIP144Deactivated | covered | `before_activation_anything_is_accepted` (block_rules.rs) | |
| ValidTxExecutionSublistsEdgesTest.blockWithValidEdges | covered | `two_ascending_in_bounds_edges_are_valid` | |
| ValidTxExecutionSublistsEdgesTest.blockWithEmptyEdges | covered | `empty_edges_are_valid` | |
| ValidTxExecutionSublistsEdgesTest.blockWithTooManyEdges | covered | `more_edges_than_execution_threads_is_rejected` | |
| ValidTxExecutionSublistsEdgesTest.blockWithOutOfBoundsEdgesBecauseOfRemascTx | covered | `an_edge_at_the_transaction_count_is_out_of_bounds` | |
| ValidTxExecutionSublistsEdgesTest.blockWithOutOfBoundsEdges | covered | `an_edge_past_the_transaction_list_is_rejected` | |
| ValidTxExecutionSublistsEdgesTest.blockWithNegativeEdge | covered | `a_negative_edge_is_rejected` | |
| ValidTxExecutionSublistsEdgesTest.blockWithEdgeZero | covered | `a_first_edge_of_zero_is_rejected` | |
| ValidTxExecutionSublistsEdgesTest.blockWithRepeatedEdge | covered | `a_repeated_edge_is_rejected` | |
| ValidTxExecutionSublistsEdgesTest.blockWithEdgesNotInOrder | covered | `edges_out_of_order_are_rejected` | |

## co.rsk.mine

Paths: rustock = `clients/rustock`, `tests.rs` = `crates/execution/src/mining/tests.rs`, `template.rs` = `crates/execution/src/mining/template.rs`, `server.rs` = `crates/execution/src/mining/server.rs`, `executor.rs` = `crates/execution/src/executor.rs`, `rpc tests` = `crates/rpc/src/tests.rs`.

| rskj test | verdict | rustock test or reason | code check (gaps only) |
|---|---|---|---|
| AutoMinerClientTest.byDefaultIsDisabled | covered | `a_timed_miner_mines_blocks_on_its_own` (tests.rs) asserts `!is_running()` before start; `starting_twice_runs_one_miner` (rpc tests) | |
| AutoMinerClientTest.minesBlock | covered | `a_pending_transaction_is_mined_without_being_asked` (rpc tests); `a_timed_miner_mines_blocks_on_its_own` (tests.rs) | |
| AutoMinerClientTest.disablesMining | covered | `a_timed_miner_mines_blocks_on_its_own` (tests.rs): stop -> `!is_running()`, no more blocks | |
| BlockToMineBuilderEvictionEndToEndTest.build_whenTxExecutionThrowsError_doesNotPropagateAndEvictsTxFromPool | covered | `build_when_tx_execution_throws_error_does_not_propagate_and_evicts_tx_from_pool` (mining/tests.rs) | |
| BlockToMineBuilderEvictionEndToEndTest.build_afterErrorTxEvicted_subsequentBuildProducesBlockAndIsNotRePoisoned | covered | `build_after_error_tx_evicted_subsequent_build_produces_block_and_is_not_re_poisoned` (mining/tests.rs) | |
| BlockToMineBuilderInvalidTxEvictionTest.build_whenBlockResultHasInvalidTxs_shouldEvictThemFromPool | covered | `build_when_tx_execution_throws_error_does_not_propagate_and_evicts_tx_from_pool` (mining/tests.rs) | |
| BlockToMineBuilderInvalidTxEvictionTest.build_whenBlockResultHasNoInvalidTxs_shouldNotEvictFromPool | covered | `build_when_block_result_has_no_invalid_txs_should_not_evict_from_pool` (mining/tests.rs) | |
| BlockToMineBuilderTest.BuildBlockHasEmptyUnclesWhenCreateAnInvalidBlock | covered | `a_template_leaves_out_uncles_the_rule_refuses` (crates/execution/src/mining/tests.rs) | |
| BlockToMineBuilderTest.BuildBlockHasUnclesWhenCreateAnInvalidBlock | covered | `a_losing_sibling_is_selected_as_an_uncle`, `the_uncle_list_is_capped`, `cumulative_difficulty_counts_the_uncles` (tests.rs) | |
| BlockToMineBuilderTest.buildBlockBeforeUMMActivation | covered | `umm_root_appears_once_papyrus_at` -> `umm_root_appears_once_papyrus_is_active` (tests.rs), first assertion (`umm_root.is_none()` before activation) | |
| BlockToMineBuilderTest.buildBlockAfterUMMActivation | covered | `umm_root_appears_once_papyrus_is_active` (tests.rs): `Some(Bytes::new())` after activation; `a_solution_is_imported_with_umm_root_present` | |
| BlockToMineBuilderTest.buildBlockBeforeRskip351 | covered | `a_template_carries_the_fields_its_height_brings` (mining/tests.rs): a template without RSKIP351 has no version fields. The table's gap was written when only version 2 was tested (the [blocks table](rskj-tests/blocks.md)) | |
| BlockToMineBuilderTest.buildBlockAfterRskip351 | covered | `a_template_carries_the_header_version_the_chain_activates` (tests.rs) (version 2 on an all-active chain; v1 branch at template.rs:304-305 untested but same ladder) | |
| ForkDataDetectionCalculatorTest.calculateWithMinPossibleBlockchainHeight | covered | `fork_detection_data_commits_to_height_and_uncles` (tests.rs) (length 12, height bytes 8..12, uncle byte) | |
| ForkDataDetectionCalculatorTest.calculateReturnsEmptyWhenNotEnoughBlocks | covered | `fork_detection_data_is_empty_on_a_short_chain` (tests.rs) | |
| ForkDataDetectionCalculatorTest.calculateWithDivisibleBy64height | covered | `fork_detection_data_commits_to_a_bitcoin_header_every_64_blocks` (tests.rs). rskj's test names the block 446 below the best for the last byte, where its calculator reads 447 (`bestBlockHeight - cpvStartHeight + i * 64`); it passes because its chain gives #64 and #65 one Bitcoin header. Here each block has its own, and rustock reads 447, as rskj's code does | |
| ForkDataDetectionCalculatorTest.calculateWithUnclesOnPreviousBlocks | covered | `fork_detection_data_commits_to_height_and_uncles` (tests.rs): sums only the last 32 blocks' `uncle_count` | |
| ForkDataDetectionCalculatorTest.calculateWithMaxUnclesOnPreviousBlocks | covered | `fork_detection_uncle_count_saturates` (tests.rs): byte 7 saturates at 255 | |
| GasLimitCalculatorTest.NextBlockGasLimitIsDecreasedByAFactor | covered | `next_block_gas_limit_is_decreased_by_a_factor` (mining/tests.rs), ported with the rest of the class at rskj's release: `forcegaslimit` off, the gas-used path, which rustock's miner now has (`--mining-force-gas-limit false`) | |
| GasLimitCalculatorTest.NextBlockGasLimitIsNotDecreasedLowerThanMinGasLimit | covered | `next_block_gas_limit_is_not_decreased_lower_than_min_gas_limit` (mining/tests.rs) | |
| GasLimitCalculatorTest.NextBlockGasLimitIsIncreasedBasedOnGasUsed | covered | `next_block_gas_limit_is_increased_based_on_gas_used` (mining/tests.rs) | |
| GasLimitCalculatorTest.NextBlockGasLimitIsIncreasedBasedOnFullGasUsed | covered | `next_block_gas_limit_is_increased_based_on_full_gas_used` (mining/tests.rs) | |
| GasLimitCalculatorTest.NextBlockGasLimitIsNotIncreasedMoreThanTargetGasLimit | covered | `next_block_gas_limit_is_not_increased_more_than_target_gas_limit` (mining/tests.rs) | |
| GasLimitCalculatorTest.NextBlockGasLimitRemainsTheSame | covered | `next_block_gas_limit_remains_the_same` (mining/tests.rs); through the template on rskj's default target, `a_template_on_rskjs_defaults_keeps_a_parent_already_there` | |
| GasLimitCalculatorTest.NextBlockGasLimitIsIncreasedByMaximumValue | covered | `next_block_gas_limit_is_increased_by_maximum_value` (mining/tests.rs); at any size, `the_gas_limit_steps_a_1024th_at_any_size` | |
| GasLimitCalculatorTest.NextBlockGasLimitIsIncreasedToTarget | covered | `next_block_gas_limit_is_increased_to_target` (mining/tests.rs) | |
| GasLimitCalculatorTest.NextBlockGasLimitIsDecreasedToTarget | covered | `next_block_gas_limit_is_decreased_to_target` (mining/tests.rs) | |
| GasLimitCalculatorTest.NextBlockGasLimitIsDecreasedToMinimum | covered | `next_block_gas_limit_is_decreased_to_minimum` (mining/tests.rs); through the template, `a_gas_limit_target_under_the_minimum_is_the_minimum` | |
| GasLimitCalculatorTest.NextBlockGasLimitIsDecreasedByMaximumValue | covered | `next_block_gas_limit_is_decreased_by_maximum_value` (mining/tests.rs). The test's `validByConsensus` reads ethereumj's `ParentGasLimitRule`, a 1024th of the parent's either way; the rule a block is held to, rskj's `BlockParentGasLimitRule`, is a gas wider below where the 1024th leaves a remainder. The port checks the miner's gas limit against both | |
| MainNetMinerTest.submitBitcoinBlockProofOfWorkNotGoodEnough | covered | `a_solution_that_misses_the_target_is_refused`, `with_pow_validation_an_unsolved_block_is_refused` (tests.rs): `SelfCheck` error, nothing imported | |
| MainNetMinerTest.submitBitcoinBlockInvalidBlockDoesntEliminateCache | covered | `a_refused_solution_leaves_its_work_answerable` (tests.rs): the refusal, and the solution after it, which rskj's test leaves commented out (its difficulty is too high to solve) | |
| MinerClockTest.timestampForChildIsParentTimestampIfRegtest | not applicable | rskj regtest "fixed clock" (child = parent + adjustment, no wall clock); rustock has no fixed-clock mode: `timestamp_for_child` (template.rs:702-709) always uses wall clock (+ `evm_increaseTime` offset) with `max(parent+1)`. Non-consensus, dev-only semantics. | |
| MinerClockTest.timestampForChildIsClockTimeIfNotRegtest | covered | `a_template_is_never_timestamped_at_or_before_its_parent` (tests.rs) + `timestamp_for_child` = `now.max(parent+1)` (template.rs:707-708) | |
| MinerClockTest.timestampForChildIsTimestampPlusOneIfNotRegtest | covered | `a_template_is_never_timestamped_at_or_before_its_parent` (tests.rs) | |
| MinerClockTest.adjustTimeIfRegtest | not applicable | fixed-clock mode absent (see above); the offset itself is covered by `evm_reset_returns_to_genesis_and_clears_the_clock` (rpc tests) | |
| MinerClockTest.adjustTimeIfNotRegtest | covered | `the_miners_clock_runs_ahead_by_what_was_added` (`crates/execution/tests/miner_clock.rs`, a test binary of its own: the offset is one value for the process). Written with a fix: rustock moved the clock back on a negative `evm_increaseTime`, where rskj's `MinerClock.increaseTime` ignores zero or fewer seconds; it now does too | |
| MinerClockTest.clearTimeIncrease | covered | `evm_reset_returns_to_genesis_and_clears_the_clock` (rpc tests): `time_offset() == 0` after reset | |
| MinerManagerTest.refreshWorkRunOnce | covered | `a_solution_completes_a_block_and_is_imported` (tests.rs); `every_scenario_mines_a_block_this_node_accepts` (workload.rs) | |
| MinerManagerTest.refreshWorkRunTwice | covered | `a_timed_miner_mines_blocks_on_its_own` (tests.rs): consecutive blocks, head advances by `mined` | |
| MinerManagerTest.mineBlockTwiceReusingTheSameWork | covered | `a_repeated_solution_reports_the_block_as_already_present` (tests.rs). Note rskj imports a second sibling block at height 1 (different hash because the BTC solution differs); rustock refuses with `ParentNoLongerHead` (server.rs:570-576) -- deliberate divergence, documented in the test | |
| MinerManagerTest.mineBlockWhileSyncingBlocks | covered | `mine_block_while_syncing_blocks` (mining/tests.rs) | |
| MinerManagerTest.doWork | covered | `a_timed_miner_mines_blocks_on_its_own` (tests.rs); `a_pending_transaction_is_mined_without_being_asked` (rpc tests) | |
| MinerManagerTest.doWorkEvenWithoutMinerServer | not applicable | null-MinerServer wiring (Java null-safety of `MinerClientImpl`); rustock's miner handle is `Option<Arc<..>>` resolved at startup (crates/cli/src/main.rs:3335-3420) | |
| MinerManagerTest.doWorkInThread | covered | `a_timed_miner_mines_blocks_on_its_own` (tests.rs): `is_running()` true after `start()` on its own thread | |
| MinerManagerTest.mineBlock | covered | `a_solution_completes_a_block_and_is_imported`, `a_solution_is_imported_on_a_chain_deep_enough_for_remasc_to_pay` (tests.rs); `evm_reset_returns_to_genesis_and_clears_the_clock` (rpc tests) for the snapshot-reset part | |
| MinerServerTest.buildBlockToMineCheckThatLastTransactionIsForREMASC | covered | `a_template_without_remasc_would_not_validate`, `remasc_transaction_is_recognised_as_remasc` (tests.rs), `the_remasc_transaction_closes_the_sequential_sublist` (partition.rs); `build()` pushes REMASC last (template.rs:191-197) | |
| MinerServerTest.submitBitcoinBlockTwoTags | covered | `compression_picks_the_requested_tag_of_two`, `compression_leaves_the_tag_inside_the_first_tail_block` (crates/core/src/validation/merged_mining.rs); `complete_and_import` uses `compress_coinbase(raw, true)` = last tag (server.rs:580). The "resubmit same PoW for the other header -> ERROR" half maps to `a_repeated_solution_reports_the_block_as_already_present` | |
| MinerServerTest.submitBitcoinBlock | covered | `a_solution_completes_a_block_and_is_imported` (tests.rs): `ImportedBest`, height parent+1 | |
| MinerServerTest.submitBitcoinBlockPartialMerkleWhenBlockIsEmpty | covered | `all_three_submit_forms_accept_the_same_solution` (tests.rs) ("partial" form, coinbase-only block); `a_single_transaction_block_needs_no_proof` | |
| MinerServerTest.submitBitcoinBlockPartialMerkleWhenBlockHasTransactions | covered | `a_partial_merkle_proof_drops_the_coinbase`, `merkle_proofs_fold_back_to_the_root`, `wire_hashes_are_reversed_on_the_way_in` (tests.rs); submit path `submit_bitcoin_block_partial_merkle` (server.rs:543-556) | |
| MinerServerTest.submitBitcoinBlockTransactionsWhenBlockIsEmpty | covered | `all_three_submit_forms_accept_the_same_solution` (tests.rs) ("transactions" form) | |
| MinerServerTest.submitBitcoinBlockTransactionsWhenBlockHasTransactions | covered | `merkle_proofs_fold_back_to_the_root`, `wire_hashes_are_reversed_on_the_way_in`, `a_malformed_wire_hash_is_rejected` (tests.rs) via `proof_from_tx_hashes` (merkle.rs:63) | |
| MinerServerTest.workWithNoTransactionsZeroFees | covered | `work_with_no_transactions_pays_the_miner_nothing` (tests.rs) | |
| MinerServerTest.initialWorkTurnsNotifyFlagOn | covered | `a_solution_completes_a_block_and_is_imported` (tests.rs) asserts `work.notify` on first work; `notify_is_reported_once` | |
| MinerServerTest.secondWorkWithNoChangesTurnsNotifyFlagOff | covered | `notify_is_reported_once` (tests.rs); `get_work` clears the flag after one read (server.rs:482-502), `should_notify` (server.rs:469-476) implements the 10% fee threshold | |

## co.rsk.mine (continued)

Paths: rustock = `clients/rustock/crates/...`, rskj = `clients/rskj/rskj-core/src/...`.

| rskj test | verdict | rustock test or reason | code check (gaps only) |
|---|---|---|---|
| MinerServerTest.gasUnitInDollarsIsInitializedOkAtConstructor | covered | `work_on_a_new_parent_is_a_notify_again` (tests.rs); the test asserts a notify on work built on a new parent, not what its name says | |
| MinerServerTest.secondBuildBlockToMineTurnsNotifyFlagOff | covered | `second_build_block_to_mine_turns_notify_flag_off` (mining/tests.rs) | |
| MinerServerTest.submitTwoBitcoinBlocksAtSameTimeWithoutRateLimit | covered | `submit_two_bitcoin_blocks_at_same_time_without_rate_limit` (mining/tests.rs) | |
| MinerServerTest.submitTwoBitcoinBlocksAtSameTimeWithRateLimit | covered | `submit_two_bitcoin_blocks_at_same_time_with_rate_limit` (mining/tests.rs) | |
| MinerServerTest.extraDataNotInitializedWithClientData | covered | `extra_data_not_initialized_with_client_data` (mining/extra_data.rs) | |
| MinerServerTest.extraDataWithClientData | covered | `extra_data_with_client_data` (mining/extra_data.rs) | |
| MinerServerTest.extraDataWithClientDataMoreThan32Bytes | covered | `extra_data_with_client_data_more_than_32_bytes` (mining/extra_data.rs) | |
| MinerServerTest.onBestBlockBuildBlockToMine | covered | `on_best_block_build_block_to_mine` (mining/tests.rs) | |
| MinerServerTest.onNewTxBuildBlockToMine | covered | `on_new_tx_build_block_to_mine` (mining/tests.rs) | |
| MinerServerTest.rebuildsOnlyOnceArrivingFeesAreMaterialAgainstTheCandidate | covered | `rebuilds_only_once_arriving_fees_are_material_against_the_candidate` (mining/tests.rs) | |
| MinerServerTest.rebuildsOnEveryBatchWhenThereIsNoCandidateToCompareAgainst | covered | `rebuilds_on_every_batch_when_there_is_no_candidate_to_compare_against` (mining/tests.rs) | |
| MinerServerTest.onNewTxEventsAreThrottledWhenBuildQueueIsBacklogged | not applicable | Java single-thread executor coalescing; rustock has no event-driven rebuild at all (see rows above) | |
| MinerUtilsTest.getAllTransactionsTest | covered | `transactions_are_ordered_by_price_but_never_out_of_nonce_order` (execution/src/mining/tests.rs:658): two txs of one sender both kept, nonce order | |
| MinerUtilsTest.validTransactionRepositoryNonceTest | covered | `filter_transactions_skips_a_sender_past_a_nonce_gap_but_not_past_a_mined_leftover` (tests.rs:733): expected nonce read from state via `nonce_of` | |
| MinerUtilsTest.validTransactionAccWrapNonceTest | covered | same test: nonce 1 then 2 accepted after the in-block `next_nonce` map advances | |
| MinerUtilsTest.invalidNonceTransactionTest | covered | same test: wrong nonce skipped, sender's later txs blocked only by a gap; nothing is evicted | |
| MinerUtilsTest.invalidGasPriceTransactionTest | covered | `invalid_gas_price_transaction_test`, `an_underpriced_pending_transaction_is_evicted` (mining/tests.rs) | |
| MinerUtilsTest.harmfulTransactionTest | not applicable | null gasPrice cannot exist with rustock's `U256` field | |
| MinerUtilsTest.filterTransactions_whenRskip252DisabledThenTxIncludedRegardlessGasPrice | covered | `filter_transactions_when_rskip252_disabled_then_tx_included_regardless_gas_price` (tests.rs:710) | |
| MinerUtilsTest.filterTransactions_whenRskip252EnabledThenTxWithMoreGasPriceThanCapExcluded | covered | `filter_transactions_when_rskip252_enabled_then_tx_with_more_gas_price_than_cap_excluded` (tests.rs:696) | |
| MinerUtilsTest.getAllTransactionsCheckOrderTest | covered | `transactions_are_ordered_by_price_but_never_out_of_nonce_order` + `transaction_ordering_is_stable_for_equal_prices` (tests.rs:658, 677); same-nonce higher-price-first is in `order_by_price_sender_and_nonce` (template.rs:585-589) though not asserted on its own | |
| MinerUtilsTest.getAllTransactions_withNonCanonicalNonce_discardsAndReturnsValidTxs | covered | `a_non_canonical_nonce_is_refused_on_admission` (sync/txpool.rs): the pool refuses such a transaction, and the miner takes its transactions from the pool only | |
| MinerUtilsTest.getAllTransactions_allValid_noDiscarding | covered | `filter_transactions_skips_a_sender_past_a_nonce_gap_but_not_past_a_mined_leftover` / `transactions_are_ordered_...` (all valid txs kept) | |
| MinimumGasPriceCalculatorTest.increaseMgp | covered | `increase_mgp` (mining/tests.rs) | |
| MinimumGasPriceCalculatorTest.decreaseMGP | covered | `decrease_mgp` (mining/tests.rs) | |
| MinimumGasPriceCalculatorTest.mgpOnRage | covered | `mgp_on_range` (mining/tests.rs) | |
| MinimumGasPriceCalculatorTest.previousMgpEqualsTarget | covered | `previous_mgp_equals_target` (mining/tests.rs) | |
| MinimumGasPriceCalculatorTest.previousValueIsZero | covered | `previous_value_is_zero` (mining/tests.rs) | |
| MinimumGasPriceCalculatorTest.previousValueIsSmallTargetIsZero | covered | `previous_value_is_small_target_is_zero` (mining/tests.rs) | |
| MinimumGasPriceCalculatorTest.cantGetMGPtoBeNegative | covered | `cant_get_mgp_to_be_negative` (mining/tests.rs); the target is unsigned here | |
| SubmissionRateLimitHandlerTest.ofMiningConfig_WhenRateLimitIsZero_ThenHandlerShouldBeDisabled | covered | `of_mining_config_when_rate_limit_is_zero_then_handler_should_be_disabled` (mining/tests.rs) | |
| SubmissionRateLimitHandlerTest.ofMiningConfig_WhenRateLimitIsNegative_ThenHandlerShouldBeDisabled | covered | `of_mining_config_when_rate_limit_is_zero_then_handler_should_be_disabled` (mining/tests.rs); the window is unsigned here | |
| SubmissionRateLimitHandlerTest.ofMiningConfig_WhenRateLimitIsPositive_ThenHandlerShouldBeEnabled | covered | `of_mining_config_when_rate_limit_is_positive_then_handler_should_be_enabled` (mining/tests.rs) | |
| SubmissionRateLimitHandlerTest.isSubmissionAllowed_WhenDisabled_ThenShouldReturnTrue | covered | `is_submission_allowed_when_disabled_then_should_return_true` (mining/tests.rs) | |
| SubmissionRateLimitHandlerTest.isSubmissionAllowed_WhenEnabledAndLimitNotExceeded_ThenShouldReturnTrue | covered | `is_submission_allowed_when_enabled_and_limit_not_exceeded_then_should_return_true` (mining/tests.rs) | |
| SubmissionRateLimitHandlerTest.isSubmissionAllowed_WhenEnabledAndLimitExceeded_ThenShouldReturnFalse | covered | `is_submission_allowed_when_enabled_and_limit_exceeded_then_should_return_false` (mining/tests.rs) | |
| TimedMinerClientMiningTest.minesBlockSuccessfully | covered | `a_timed_miner_mines_blocks_on_its_own` (tests.rs:1901): get work + submit through the server | |
| TimedMinerClientMiningTest.handlesMiningErrorsGracefully | covered | `a_timed_miner_rides_out_work_it_cannot_build` (tests.rs): work refused while the node has a better block to fetch, then a block once it has it | |
| TimedMinerClientMiningTest.stopsMiningWhenStopped | covered | `a_timed_miner_mines_blocks_on_its_own`: `mined()` unchanged 400 ms after `stop()` | |
| TimedMinerClientMiningTest.usesExponentialDistribution | covered | `the_draw_is_exponential`, `the_empirical_median_is_near_the_configured_one` (timed.rs tests) + >=3 blocks in 1.5 s at 100 ms median (tests.rs:1913-1917) | |
| TimedMinerClientSimpleTest.testConstructor | covered | `a_timed_miner_mines_blocks_on_its_own`: `!is_running()` before start (tests.rs:1907) | |
| TimedMinerClientSimpleTest.testStartAndStop | covered | same test (start -> running, stop -> not running) | |
| TimedMinerClientSimpleTest.testMultipleStartStopCycles | covered | `a_timed_miner_started_again_at_once_runs_one_thread` (tests.rs). Written with a fix: a start right after a stop ran a second thread beside the stopped one, which read the flag set again and mined on | |
| TimedMinerClientSimpleTest.testConstructorWithDifferentDurations | not applicable | constructor smoke test; `median()` is a plain getter (timed.rs:200) | |
| TimedMinerClientTest.byDefaultIsDisabled | covered | `a_timed_miner_mines_blocks_on_its_own` (tests.rs:1907) | |
| TimedMinerClientTest.startsMining | covered | same (tests.rs:1908-1910) | |
| TimedMinerClientTest.stopsMining | covered | same (tests.rs:1912-1913) | |
| TimedMinerClientTest.canStartAndStopMultipleTimes | covered | the same | |
| TimedMinerClientTest.constructorWithCustomMedianTime | covered | `the_median_of_the_draw_is_the_configured_median`, `the_first_target_sets_the_baseline_and_later_ones_scale_the_median` (timed.rs tests) | |
| TransactionModuleTest.sendTransactionMustNotBeMined | covered | `an_idle_chain_is_not_mined` (rpc/src/tests.rs:5295) + `test_eth_sign_transaction_produces_a_transaction_from_the_sender`; `eth_send_transaction` only relays (rpc/src/sign.rs:77-99), no block without a miner | |
| TransactionModuleTest.sendTransactionMustBeMined | covered | `a_pending_transaction_is_mined_without_being_asked` (rpc/src/tests.rs:5305). Note: rskj autoMine mines inside the RPC call; rustock's `AutoMiner` polls with a 250 ms tick (rpc/src/evm.rs:303-312, cli main.rs) | |
| TransactionModuleTest.sendSeveralTransactionsWithAutoMining | covered | `a_pending_transaction_is_mined_without_being_asked` (one tx -> 1..3 blocks, then idle) | |
| TransactionModuleTest.sendRawTransactionWithAutoMining | covered | `test_eth_send_raw_transaction_with_submitter` + `a_pending_transaction_is_mined_without_being_asked` | |
| TransactionModuleTest.sendRawTransactionWithoutAutoMining | covered | `test_eth_send_raw_transaction_with_submitter` (tx reaches the pool) + `an_idle_chain_is_not_mined` | |
| TransactionModuleTest.testGasEstimation | covered | `the_estimate_covers_the_allowance_a_call_asks_for` and the estimation suite around rpc/src/tests.rs:5515-5700 (CALL-with-value estimate; exact 33264+stipend figure not asserted) | |

## co.rsk.mine.gas

The table above was made at rskj `e507f45` and has no rows for this package; these are its tests at rskj's release VETIVER-9.0.4. rskj's miner takes its minimum gas price from a provider: a fixed one, the configured `miner.minGasPrice`, or, with `miner.stableGasPrice` on, one that reads a price from a web API or a contract call, keeps it a while, and falls back on the fixed price. rustock has both: the providers in `crates/execution/src/mining/min_gas_price.rs`, and the web and contract sources, the factory and the settings in `crates/cli/src/stable_gas_price.rs`, which reads a web answer as rskj's Jackson reads it (`stable_gas_price/java.rs`).

| rskj test | verdict | rustock test or reason | code check (gaps only) |
|---|---|---|---|
| FixedMinGasPriceProviderTest.testConstructorAndGetMinGasPrice | covered | `test_constructor_and_get_min_gas_price` (`mining/min_gas_price.rs`) | |
| FixedMinGasPriceProviderTest.testGetType | covered | `test_get_type` | |
| FixedMinGasPriceProviderTest.testGetMinGasPriceAsCoin | covered | `test_get_min_gas_price_as_coin` | |
| MinGasPriceProviderFactoryTest.createFixedMinGasPriceProvider | covered | `create_fixed_min_gas_price_provider` (`stable_gas_price.rs`) | |
| MinGasPriceProviderFactoryTest.createWithNullConfig | covered | `create_with_null_config` | |
| MinGasPriceProviderFactoryTest.createWithDisabledConfigReturnFixed | covered | `create_with_disabled_config_return_fixed` | |
| MinGasPriceProviderFactoryTest.createWebProviderMethod | covered | `create_web_provider_method` | |
| StableMinGasPriceProviderTest.testGetMinGasPrice | covered | `test_get_min_gas_price` (`mining/min_gas_price.rs`) | |
| StableMinGasPriceProviderTest.GetMinGasPriceUsesFallbackWhenReturnIsNull | covered | `get_min_gas_price_uses_fallback_when_return_is_null` | |
| StableMinGasPriceProviderTest.whenRequestingTheValueTwiceCachedValueIsUsed | covered | `when_requesting_the_value_twice_cached_value_is_used` | |
| StableMinGasPriceProviderTest.testGetMinGasPriceAsCoin | covered | `test_get_min_gas_price_as_coin_of_a_stable_price` | |
| StableMinGasPriceProviderTest.fallBackPriceIsReturnedWhenProvidedValueIsAboveMaxRange | covered | `fall_back_price_is_returned_when_provided_value_is_above_max_range` | |
| StableMinGasPriceProviderTest.fallBackPriceIsReturnedWhenProvidedValueIsBelowMinRange | covered | `fall_back_price_is_returned_when_provided_value_is_below_min_range` | |
| EthCallMinGasPriceProviderTest.constructorSetsFieldsCorrectly | covered | `constructor_sets_fields_correctly` (`stable_gas_price.rs`), for the data the settings can give; data that is absent is refused, as in the next row | |
| EthCallMinGasPriceProviderTest.constructorSetsFieldsToNullWhenConfigReturnsNull | covered | `the_settings_refuse_an_eth_call_without_to_from_or_data`: the test's mocked settings give none, where rskj's own (`EthCallMinGasPriceSystemConfig`, by `getString`) refuse to start | |
| EthCallMinGasPriceProviderTest.getStableMinGasPrice_callsEthModulesCallMethod | covered | `get_stable_min_gas_price_calls_eth_modules_call_method` | |
| EthCallMinGasPriceProviderTest.getStableMinGasPrice_callsFallback_whenNoData | covered | `get_stable_min_gas_price_calls_fallback_when_no_data` | |
| EthCallMinGasPriceProviderTest.getStableMinGasPrice_callsFallback_whenEthModuleIsNull | covered | `get_stable_min_gas_price_calls_fallback_when_eth_module_is_null` | |
| EthCallMinGasPriceProviderTest.getType_returnsOnChain | covered | `get_type_returns_on_chain` | |
| HttpGetMinGasPriceProviderTest.returnsMappedPriceFromWebClient | covered | `returns_mapped_price_from_web_client` | |
| HttpGetMinGasPriceProviderTest.whenRequestingTheValueTwiceCachedValueIsUsed | covered | `when_requesting_the_value_twice_cached_value_is_used` | |
| HttpGetMinGasPriceProviderTest.whenEmptyResponseReturnsFallbackProvider | covered | `when_empty_response_returns_fallback_provider` | |
| HttpGetMinGasPriceProviderTest.whenErrorReturnsFallbackProvider | covered | `when_error_returns_fallback_provider` | |
| HttpGetMinGasPriceProviderTest.whenPathIsWrongReturnsFallBack | covered | `when_path_is_wrong_returns_fall_back` | |

## co.rsk.config.mining

The settings of the stable minimum gas price, at rskj's release VETIVER-9.0.4. rustock's are the config file's `[mining.stable_gas_price]`, read as rskj reads `miner.stableGasPrice` (`StableGasPriceConfig` in `crates/cli/src/stable_gas_price.rs`): its keys in the file's snake case, its durations as HOCON writes them.

| rskj test | verdict | rustock test or reason | code check (gaps only) |
|---|---|---|---|
| EthCallMinGasPriceSystemConfigTest.testAddress | covered | `test_address` | |
| EthCallMinGasPriceSystemConfigTest.testFrom | covered | `test_from` | |
| EthCallMinGasPriceSystemConfigTest.testData | covered | `test_data` | |
| HttpGetStableMinGasSystemConfigTest.testGetUrl | covered | `test_get_url` | |
| HttpGetStableMinGasSystemConfigTest.testRequestPath | covered | `test_request_path` | |
| HttpGetStableMinGasSystemConfigTest.testGetTimeout | covered | `test_get_timeout` | |
| StableMinGasPriceSystemConfigTest.testGetRefreshRate | covered | `test_get_refresh_rate` | |
| StableMinGasPriceSystemConfigTest.testGetMinStableGasPrice | covered | `test_get_min_stable_gas_price` | |
| StableMinGasPriceSystemConfigTest.testIsEnabled | covered | `test_is_enabled` | |
| StableMinGasPriceSystemConfigTest.testGetMethod | covered | `test_get_method` | |
| StableMinGasPriceSystemConfigTest.wrongMethodShouldThrowException | covered | `wrong_method_should_throw_exception` | |
| StableMinGasPriceSystemConfigTest.missingPropertyShouldThrowException | covered | `missing_property_should_throw_exception` | |
| StableMinGasPriceSystemConfigTest.testRanges | covered | `test_ranges` | |

## co.rsk.net.handler.txvalidator and org.ethereum.facade

Paths: rskj = `rsk-clients-lab/clients/rskj/rskj-core`, rustock = `rsk-clients-lab/clients/rustock`. Rustock's pool admission is `TransactionPool::validate_tx` at `crates/sync/src/txpool.rs:515-566`; `TRANSACTION_GAS_CAP` at :13; `is_remasc_tx` :859; `intrinsic_gas` :871.

| rskj test | verdict | rustock test or reason | code check (gaps only) |
|---|---|---|---|
| TxValidatorAccountBalanceValidatorTest.validAccountBalance | covered | `test_add_pending_transaction` (txpool.rs:1335; balance >= gasPrice*gasLimit admitted), `test_aggregate_balance_exceeded` (:1642, first tx accepted) | |
| TxValidatorAccountBalanceValidatorTest.invalidAccountBalance | covered | `test_insufficient_balance_rejected` (txpool.rs:1619), `test_null_sender_insufficient_funds` (:1895) | |
| TxValidatorAccountBalanceValidatorTest.balanceIsNotValidatedIfFreeTx | covered | `balance_is_not_validated_if_free_tx` (sync/txpool.rs) | |
| TxValidatorAccountStateValidatorTest.validAccountState | not applicable | `AccountState.deleted` is never set to true anywhere in rskj main code (only `setDeleted` setter exists, AccountState.java:118); rustock's `AccountState` has no such flag | |
| TxValidatorAccountStateValidatorTest.invalidAccountState | not applicable | same: dead flag in rskj, no counterpart in rustock | |
| TxValidatorGasLimitValidatorTest.validGasLimit | covered | every admitted tx (e.g. `test_add_pending_transaction`, gas 21000 <= 8M header limit); note rskj compares to `max(sublistGasLimit)` once RSKIP144+351 are active (TxPendingValidator.java:77-79) while rustock compares to the full `header.gas_limit` (txpool.rs:534-538) -- see Summary | |
| TxValidatorGasLimitValidatorTest.invalidGasLimit | covered | `test_high_gas_limit_rejected` (txpool.rs:1493, 9M > 8M -> `GasLimitExceeded`) | |
| TxValidatorIntrinsicGasLimitValidatorTest.validIntrinsicGasPrice | covered | `test_intrinsic_gas_*` for tx1-tx3 and `valid_intrinsic_gas_price_for_a_free_bridge_tx_with_zero_gas_limit` for tx4 (sync/txpool.rs) | |
| TxValidatorIntrinsicGasLimitValidatorTest.invalidIntrinsicGasPrice | covered | `test_intrinsic_gas_exceeded` (txpool.rs:1630), `test_intrinsic_gas_with_calldata` (:1325, 16/4 per byte), `test_intrinsic_gas_contract_creation` (:1318) | |
| TxValidatorMaximumGasPriceValidatorTest.validate_whenRskip252DisabledThenTxValidRegardlessGasPrice | covered | `validate_when_rskip252_disabled_then_tx_valid_regardless_gas_price` (sync/txpool.rs) | |
| TxValidatorMaximumGasPriceValidatorTest.validate_whenRskip252EnabledThenTxWithMoreGasPriceThanCapInvalid | covered | `validate_when_rskip252_enabled_then_tx_with_more_gas_price_than_cap_invalid` (sync/txpool.rs) | |
| TxValidatorMinimumGasPriceValidatorTest.validMinimumGasPrice | covered | `test_add_pending_transaction` (gas price == header minimum 59_240_000 admitted, txpool.rs:1335/:922), bumped replacements above it (`test_same_nonce_bumped_accepted` :1474) | |
| TxValidatorMinimumGasPriceValidatorTest.invalidMinimumGasPrice | covered | `test_low_gas_price_rejected` (txpool.rs:1608 -> `GasPriceTooLow`); the `null` gas price case is not applicable (U256, cannot be null) | |
| TxValidatorNonceEncodingValidatorTest.validateNonceEncoding | covered | `validate_nonce_encoding` and `a_non_canonical_nonce_is_refused_on_admission` (sync/txpool.rs) | |
| TxValidatorNonceRangeValidatorTest.oneSlotRange | covered | `test_nonce_range_one_slot` (txpool.rs:1696; state nonce 1, slots 1: 0 too low, 1 ok, 2 too high) | |
| TxValidatorNonceRangeValidatorTest.fiveSlotsRange | covered | `test_nonce_range_five_slots` (txpool.rs:2003; 1..=5 ok, 0 and 6 refused) | |
| TxValidatorNonceRangeValidatorTest.illegalAccountSlotsValue_Zero | covered | `illegal_account_slots_value_zero` (sync/txpool.rs), `account_slots_of_zero_are_refused` (cli/main.rs) | |
| TxValidatorNonceRangeValidatorTest.illegalAccountSlotsValue_Negative | not applicable | `account_slots` is `u64`; a negative value cannot be expressed (clap rejects it at parse) | |
| TxValidatorNotNullValidatorTest.nullTx | not applicable | `add_transaction(&[u8])` takes a value, not a nullable reference; undecodable bytes are `PoolError::RlpDecode` (`test_undecodable_transactions_still_count_as_offered`, txpool.rs:1297) | |
| TxValidatorNotNullValidatorTest.tx | not applicable | same | |
| TxValidatorNotRemascTxValidatorTest.remascTx | covered | `test_remasc_tx_rejected` (txpool.rs:1585 -> `IsRemascTransaction`). Note rustock identifies REMASC structurally (to == REMASC addr && zero price/limit/value, txpool.rs:859-867) where rskj uses `instanceof RemascTransaction` | |
| TxValidatorNotRemascTxValidatorTest.commonTx | covered | every admitted tx, e.g. `test_add_pending_transaction` (txpool.rs:1335) | |
| EthereumImplTest.getGasPrice_returns_GasPriceTrackerValue_when_feeMarketWorking_is_true | covered | `test_eth_gas_price_uses_the_tracker` (crates/rpc/src/tests.rs:3546); impl `eth_gas_price` crates/rpc/src/eth.rs:95-110 | |
| EthereumImplTest.getGasPrice_returns_correctedBestBlockValue_when_feeMarketWorking_is_false | covered | `test_eth_gas_price_returns_corrected_best_block_value_when_fee_market_is_not_working` (tests.rs:3561) plus `corrected_minimum_gas_price_truncates_like_rskj` (:3575); multiplier 1.1 const at eth.rs:114 (rskj test uses 1.2 via config; rustock hard-codes rskj's default, not configurable) | |
