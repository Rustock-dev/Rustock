# rskj's tests of headers, block rules, a block's run, the miner and the pool

One row per rskj test, from the release VETIVER-9.0.4 (`rskj-core/src/test/java/`, the number after a test is the line of its signature), and per test ported from the rest of a class. Verdicts as `docs/rskj-test-parity.md` gives them: **covered** (the rustock test that asserts the same, read for what it asserts), **ported** (written here, in the `rskj_tests/blocks/` folder of `rustock-core`, `rustock-execution` or `rustock-sync`), **not applicable** (why rustock needs no such test), or **fixed** (a ported test that found rustock doing otherwise than rskj; rustock was put right, the test passes, and the row says what differed).

Paths of rustock tests: `tests.rs` is `crates/core/src/validation/tests.rs`, `block_rules.rs` is `crates/core/src/validation/block_rules.rs`, and a module named after an rskj class is in the `rskj_tests/blocks/` folder of the crate named in its section.

## Validators: `co.rsk.validators` and `org.ethereum.validator`

### `co/rsk/validators/BlockTimeStampValidationRuleTest.java`

rskj's rule: `co/rsk/validators/BlockTimeStampValidationRule.java` 67-138 (a header at most `validPeriodLength` seconds past the clock, strictly after its parent, and from RSKIP179 less than `getMaxTimestampsDiffInSecs` from its Bitcoin header).

| rskj test | what it asserts | verdict | rustock test |
|---|---|---|---|
| BlockTimeStampValidationRuleTest.timestampsAreCloseEnough (66) | from RSKIP179, a header and its Bitcoin header timestamped equal pass, 300 s apart fail, 299 s apart pass, either way round | covered | `timestamps_are_close_enough` (tests.rs), with `bitcoin_timestamp_is_not_checked_before_rskip179`, `a_header_without_a_decodable_bitcoin_header_fails_rskip179` and `a_zero_period_turns_rskip179_off_too` |
| BlockTimeStampValidationRuleTest.blockInThePast (97) | a header 1,000 s before the clock passes | covered | `block_in_the_past` (tests.rs) |
| BlockTimeStampValidationRuleTest.blockInTheFutureLimit (109) | a header the period (540 s) past the clock passes; rskj's test builds this rule on the system clock, so its header is in the past, and the case it names is the limit | covered | `block_in_the_future_limit` (tests.rs) holds the limit: 540 s past a clock at 10,000 s passes |
| BlockTimeStampValidationRuleTest.blockInTheFuture (121) | twice the period past the clock fails | covered | `block_in_the_future` (tests.rs) |
| BlockTimeStampValidationRuleTest.blockTimeLowerThanParentBlockTime (133) | a header before its parent fails | covered | `block_time_against_the_parents` (tests.rs) |
| BlockTimeStampValidationRuleTest.blockTimeLowerThanParentBlockHeaderTime (150) | the same, given the parent's header | covered | `block_time_against_the_parents` (tests.rs) |
| BlockTimeStampValidationRuleTest.blockTimeGreaterThanParentBlockTime (165) | a header after its parent passes | covered | `block_time_against_the_parents` (tests.rs) |
| BlockTimeStampValidationRuleTest.blockTimeGreaterThanParentBlockHeaderTime (182) | the same, given the parent's header | covered | `block_time_against_the_parents` (tests.rs) |
| BlockTimeStampValidationRuleTest.blockTimeEqualsParentBlockTime (197) | a header timed as its parent fails | covered | `block_time_against_the_parents` (tests.rs) |
| BlockTimeStampValidationRuleTest.blockTimeEqualsParentBlockHeaderTime (214) | the same, given the parent's header | covered | `block_time_against_the_parents` (tests.rs) |

### `co/rsk/validators/ValidTxExecutionSublistsEdgesTest.java`

rskj's rule: `co/rsk/validators/ValidTxExecutionSublistsEdgesRule.java` 27-63, with RSKIP144 and RSKIP351 both active. rustock's tests are `block_rules.rs`'s `sublist_edges_tests`, on ten transactions as rskj's.

| rskj test | what it asserts | verdict | rustock test |
|---|---|---|---|
| ValidTxExecutionSublistsEdgesTest.blockWithRSKIP144Deactivated (50) | before the rules, a header with no edges is valid | covered | `before_activation_anything_is_accepted` |
| ValidTxExecutionSublistsEdgesTest.blockWithValidEdges (57) | edges [2, 5] are valid | covered | `two_ascending_in_bounds_edges_are_valid` |
| ValidTxExecutionSublistsEdgesTest.blockWithEmptyEdges (65) | no edges is valid | covered | `empty_edges_are_valid` |
| ValidTxExecutionSublistsEdgesTest.blockWithTooManyEdges (73) | five edges, over the two execution threads, are invalid | covered | `more_edges_than_execution_threads_is_rejected` (three invalid, two valid too) |
| ValidTxExecutionSublistsEdgesTest.blockWithOutOfBoundsEdgesBecauseOfRemascTx (80) | an edge at 10, the REMASC transaction's index, is invalid | covered | `an_edge_at_the_transaction_count_is_out_of_bounds` (9 valid) |
| ValidTxExecutionSublistsEdgesTest.blockWithOutOfBoundsEdges (90) | an edge at 12 is invalid | covered | `an_edge_past_the_transaction_list_is_rejected` |
| ValidTxExecutionSublistsEdgesTest.blockWithNegativeEdge (97) | an edge at -2 is invalid | covered | `a_negative_edge_is_rejected` |
| ValidTxExecutionSublistsEdgesTest.blockWithEdgeZero (104) | [0, 2] is invalid | covered | `a_first_edge_of_zero_is_rejected` |
| ValidTxExecutionSublistsEdgesTest.blockWithRepeatedEdge (111) | [2, 2] is invalid | covered | `a_repeated_edge_is_rejected` |
| ValidTxExecutionSublistsEdgesTest.blockWithEdgesNotInOrder (118) | [2, 4, 3] is invalid | covered | `edges_out_of_order_are_rejected`. Three edges fail the thread count before their order is looked at, in rskj's rule as in rustock's; the order check is what `a_repeated_edge_is_rejected` reaches |

### `org/ethereum/validator/Rskip92MerkleProofValidatorTests.java`

rskj's validator: `co/rsk/validators/Rskip92MerkleProofValidator.java` 35-58, built by `co/rsk/validators/ProofOfWorkRule.java` 128-139, which answers false where building it throws. rustock: `MergedMiningRule` (`crates/core/src/validation/merged_mining.rs`). Ported in `rustock-core`, `rskip92_merkle_proof_validator_tests.rs`.

| rskj test | what it asserts | verdict | rustock test |
|---|---|---|---|
| Rskip92MerkleProofValidatorTests.isValid_PassValidHashes_ShouldReturnTrue (20) | five hashes fold from the coinbase's hash to the root | ported | `a_proof_folds_the_coinbase_hash_with_each_hash_in_turn`: the fold worked by hand, and a header merge-mined through five siblings passes the rule |
| Rskip92MerkleProofValidatorTests.isValid_PassInvalidCoinbaseHash_ShouldReturnFalse (33) | another coinbase hash doesn't fold to that root | ported | `a_proof_folds_the_coinbase_hash_with_each_hash_in_turn`: and at the rule, a coinbase with a bit of its midstate changed is `BitcoinMerkleProofInvalid` |
| Rskip92MerkleProofValidatorTests.isValid_PassInvalidRootHash_ShouldReturnFalse (46) | the fold doesn't match another root | ported | `a_proof_folds_the_coinbase_hash_with_each_hash_in_turn`: and at the rule, a Bitcoin header holding another root is `BitcoinMerkleProofInvalid` |
| Rskip92MerkleProofValidatorTests.createInstance_PassNullProofAndRskip180Enabled_ShouldThrowIllegalArgumentError (59) | with RSKIP180, no proof can't make a validator, so the block is refused | ported | `a_header_without_a_merkle_proof_is_refused_with_or_without_rskip180` |
| Rskip92MerkleProofValidatorTests.createInstance_PassNullProofAndRskip180Disabled_ShouldThrowNullPointerError (64) | without RSKIP180, the same | ported | `a_header_without_a_merkle_proof_is_refused_with_or_without_rskip180` |
| Rskip92MerkleProofValidatorTests.createInstance_PassLargeProofAndRskip180Enabled_ShouldThrowError (69) | with RSKIP180, 961 bytes are refused | ported | `from_rskip180_a_proof_over_960_bytes_is_refused`: 961 and 992 bytes `MerkleProofTooLarge` at iris300, 960 pass |
| Rskip92MerkleProofValidatorTests.createInstance_PassLargeMalformedProofAndRskip180Disabled_ShouldThrowError (75) | without RSKIP180, 961 bytes are refused, being no whole number of hashes | ported | `from_rskip180_a_proof_over_960_bytes_is_refused`: `BitcoinMerkleProofDecodeError` below iris300 |
| Rskip92MerkleProofValidatorTests.createInstance_PassLargeProofAndRskip180Disabled_ShouldNotThrowError (81) | without RSKIP180, 992 bytes (31 hashes) are a proof | ported | `from_rskip180_a_proof_over_960_bytes_is_refused`: a 31-hash proof passes the rule below iris300 |
| Rskip92MerkleProofValidatorTests.createInstance_PassMalformedProofAndRskip180Enabled_ShouldThrowError (88) | with RSKIP180, 33 bytes are refused | covered | `test_merged_mining_rule_invalid_merkle` (tests.rs; regtest, iris300 at genesis), and `a_proof_of_no_whole_number_of_hashes_is_refused_with_or_without_rskip180` |
| Rskip92MerkleProofValidatorTests.createInstance_PassMalformedProofAndRskip180Disabled_ShouldThrowError (94) | without RSKIP180, 33 bytes are refused | ported | `a_proof_of_no_whole_number_of_hashes_is_refused_with_or_without_rskip180` |

### `co/rsk/validators/ForkDetectionDataRuleTest.java`

rskj's rule: `co/rsk/validators/ForkDetectionDataRule.java` 66-132; the data a header carries, `org/ethereum/core/BlockHeader.java` 578-616, read from block 449 only (`org/ethereum/core/BlockFactory.java` 256-258). rustock: `fork_detection::validate` (`crates/core/src/validation/fork_detection.rs`). Ported in `rustock-core`, `fork_detection_data_rule_test.rs`, with rustock's calculator where rskj's test mocks it.

| rskj test | what it asserts | verdict | rustock test |
|---|---|---|---|
| ForkDetectionDataRuleTest.validForBlocksBeforeRskip110UsingMethodThatReceivesBlockAsParameter (54) | before RSKIP110, block 4,242 is valid | ported | `before_rskip110_no_fork_detection_data_is_asked_for` (and at the activation the rule asks) |
| ForkDetectionDataRuleTest.validForBlocksBeforeRskip110 (73) | the same, on the header | ported | `before_rskip110_no_fork_detection_data_is_asked_for` |
| ForkDetectionDataRuleTest.invalidForRskip110ActiveButForkDetectionData (89) | block 42 reporting twelve bytes of fork-detection data is refused | not applicable | the test's mock reports data rskj's own header never has below 449: `BlockFactory` builds those headers with `includeForkDetectionData` false (`BlockFactory.java` 257-258), so `getMiningForkDetectionData` gives none (`BlockHeader.java` 578, 615) and the refusal (`ForkDetectionDataRule.java` 90-101) fires for no block read or mined. rustock reads none there either: `fork_detection_starts_above_block_449` (tests.rs) holds blocks 100 and 449 valid with the twelve bytes their coinbase carries |
| ForkDetectionDataRuleTest.validForRskip110ActiveButNoForkDetectionDataBecauseNoEnoughBlocksToCalculateIt (108) | block 42, no data, no chain below: valid | ported | `below_block_449_a_block_carries_no_fork_detection_data` |
| ForkDetectionDataRuleTest.invalidForRskip110ActiveAndForkDetectionDataButMissingBlocksForCalculation (127) | block 4,242 with data and no headers to calculate from is refused | ported | `a_chain_too_short_to_calculate_from_refuses_the_block` (none, and 448); also `fork_detection_starts_above_block_449` (tests.rs) at 450 |
| ForkDetectionDataRuleTest.validForRskip110ActiveAndForkDetectionData (155) | data matching what the 449 headers below make is valid | ported | `fork_detection_data_matching_the_chain_is_valid` |
| ForkDetectionDataRuleTest.invalidForRskip110ActiveAndForkDetectionDataBecauseDataDoesNotMatch (190) | a byte off is refused | ported | `fork_detection_data_not_matching_the_chain_is_refused` |

### `co/rsk/validators/BlockTxsMaxGasPriceRuleTest.java`

rskj's rule: `co/rsk/validators/BlockTxsMaxGasPriceRule.java`, with `TxGasPriceCap.FOR_BLOCK` (`co/rsk/validators/TxGasPriceCap.java` 43-54).

| rskj test | what it asserts | verdict | rustock test |
|---|---|---|---|
| BlockTxsMaxGasPriceRuleTest.isValid_whenRskip252DisabledThenBlockValidRegardlessGasPrice (66) | before RSKIP252, transactions over 100 times the block's minimum gas price leave the block valid | covered | `block_txs_max_gas_price_enforces_the_100x_cap_from_rskip252` (block_rules.rs): 10,001 over a minimum of 100 passes at #5,467,999 |
| BlockTxsMaxGasPriceRuleTest.isValid_whenRskip252EnabledThenBlockInvalidDueToTxWithMoreGasPriceThanCap (84) | from RSKIP252, a transaction under the cap leaves it valid, one over it makes it invalid | covered | `block_txs_max_gas_price_enforces_the_100x_cap_from_rskip252` (block_rules.rs): at the cap passes, 10,001 fails |

### `org/ethereum/validator/ProofOfWorkRuleTest.java`

Read whole. rskj's rule: `co/rsk/validators/ProofOfWorkRule.java` 116-227; the 80-byte header from RSKIP98 also in `org/ethereum/core/BlockFactory.java` 269-276. rustock: `MergedMiningRule` (`crates/core/src/validation/merged_mining.rs`). Ported in `rustock-core`, `proof_of_work_rule_test.rs`. Not given rows: `test_1` (a mined block is valid: every block), `test_RLPEncoding` and `test_validAfterEncodingAndDecoding` (a mined header read back is the same header, and valid), and `test_2` and `test_3`, which rskj disables.

| rskj test | what it asserts | verdict | rustock test |
|---|---|---|---|
| ProofOfWorkRuleTest.test_fullBtcBlockAsHeader_rejectedByRskj (237) | from RSKIP98, a whole Bitcoin block where its header goes, with the same work and root and the same merged-mining hash, is refused by the rule and by reading the block | ported | `from_rskip98_a_whole_bitcoin_block_is_no_merged_mining_header` (at mainnet's orchid); also `merged_mining_wants_exactly_a_bitcoin_header` (tests.rs) and `from_rskip98_the_bitcoin_header_is_80_bytes` (`crates/core/src/types/header.rs`) |
| ProofOfWorkRuleTest.test_tamperedTxsIntoBtcHeaderAreRejected (308) | a byte appended to the Bitcoin header is refused by the rule and by reading the block | covered | `merged_mining_wants_exactly_a_bitcoin_header` (tests.rs: 81 bytes `BitcoinHeaderDecodeError`, 80 valid) and `from_rskip98_the_bitcoin_header_is_80_bytes` (header.rs: 79 and 81 refused at orchid) |
| ProofOfWorkRuleTest.test_noRSKTagInCoinbaseTransaction (131) | a coinbase with the hash but no tag is refused | ported | `a_coinbase_without_the_rsk_tag_is_refused` |
| ProofOfWorkRuleTest.test_RSKTagInCoinbaseTransactionTooFar (141) | a tag past the coinbase's first 64 bytes after its midstate is refused | ported | `the_rsk_tag_must_start_within_the_first_64_bytes_of_the_tail`: 63 valid, 64 and 960 refused |
| ProofOfWorkRuleTest.bytesAfterMergedMiningHashAreLessThan128 (170) | up to 128 bytes after the merged-mining hash are valid | ported | `at_most_128_bytes_may_follow_the_merged_mining_hash`: 128 valid |
| ProofOfWorkRuleTest.bytesAfterMergedMiningHashAreMoreThan128 (201) | more than 128 are refused | ported | `at_most_128_bytes_may_follow_the_merged_mining_hash`: 129 refused |
| (no rskj test) `ProofOfWorkRule.java` 191-195 | a tag after the one committing to the block is refused | ported | `the_tag_committing_to_the_block_must_be_the_last_tag` |

### The rest of `co.rsk.validators`: the rows of `docs/rskj-test-parity.md`, held against the release

That table was made at rskj `e507f45`. The release has 73 tests in the package; the table's 77 include four of `BlockRootValidationRuleTest`, a class the release doesn't have. Its rows for the four classes above are replaced by theirs. The other rows were each held against the release's test and rustock's code, and its gaps ported where they test a refusal or a limit.

| rskj test | what it asserts | verdict | rustock test, or why |
|---|---|---|---|
| BlockDifficultyRuleTest.validWhenCalculatedDifficultyMatches (55) | the calculated difficulty is valid | covered | `test_difficulty_rule` (tests.rs) |
| BlockDifficultyRuleTest.invalidWhenCalculatedDifficultyDoesntMatch (63) | any other difficulty is refused | ported | `a_difficulty_other_than_the_calculated_one_is_refused` (`rustock-core`, `block_difficulty_rule_test.rs`): one more and one less refused |
| BlockDifficultyValidationRuleTest.testDifficulty (63) | on regtest, a block 10 s after a parent of 2,048 naming one uncle carries 2,049 | ported | `an_uncle_doubles_the_time_a_block_has_before_it_is_late` (`rustock-core`, `block_difficulty_validation_rule_test.rs`). The table had it covered by tests that name no uncle, and the case turns on one (`(1 + uncleCount) * duration`): without it, 2,048 is expected |
| BlockParentGasLimitRuleTest.cantConstructRuleWithZeroGasLimitBoundDivisor (57) | a divisor of zero can't make the rule | not applicable | a Java constructor's argument check; rustock's divisor is the chain's constant, 1,024 |
| BlockParentGasLimitRuleTest.cantConstructRuleWithNegativeGasLimitBoundDivisor (62) | nor a negative one | not applicable | the same, and the divisor is a `u64` |
| BlockParentGasLimitRuleTest.validityWhenGas (68) | the same limit, both bounds and one past each are valid, valid, invalid | covered | `test_gas_limit_rule` (upper bound) and `the_parent_gas_limit_bounds_are_rskjs_when_the_divisor_leaves_a_remainder` (both, and one past each) (tests.rs) |
| BlockParentNumberRuleTest.isValid (56) | against a parent at 451, 452 valid; 451, 450, 999 invalid | ported | `a_block_is_numbered_one_past_its_parent` (`rustock-core`, `block_parent_number_rule_test.rs`); `test_block_number_rule` (tests.rs) holds only +1 and +2 |
| BlockTxsValidationRuleTest.validNonceSame (61) | a sender at nonce 42 sends at 42: valid | ported | `a_transaction_at_its_senders_nonce_is_valid` (`rustock-execution`, `block_txs_validation_rule_test.rs`), a block run on mainnet's #8,000,000. rskj refuses such a block before it runs (`co/rsk/validators/BlockTxsValidationRule.java`), rustock as it runs, by revm's check of the nonce, which each refusal is held to be. The table had these seven as gaps |
| BlockTxsValidationRuleTest.invalidNonceLower (71) | at 41: refused | ported | `a_transaction_below_or_above_its_senders_nonce_is_refused` |
| BlockTxsValidationRuleTest.invalidNonceHigher (81) | at 43: refused | ported | the same |
| BlockTxsValidationRuleTest.validTransactionsWithSameNonceAndDifferentSenders (91) | two senders at 64 each send at 64: valid | ported | `two_senders_may_send_at_the_same_nonce` |
| BlockTxsValidationRuleTest.validConsecutiveTransactionsFromSameSender (103) | a sender at 42 sends at 42 and 43: valid | ported | `a_senders_nonce_counts_up_through_the_block` |
| BlockTxsValidationRuleTest.invalidTransactionsFromSameSenderWithSkippedNonce (113) | at 42 and 44: refused | ported | the same |
| BlockTxsValidationRuleTest.invalidTransactionsWithSameNonceAndSameSender (123) | twice at 42: refused | ported | the same |
| BlockUnclesValidationRuleTest.rejectBlockWithSiblingUncle (24) | block 2 naming another child of block 1 is refused | ported | `a_sibling_of_the_block_is_no_uncle` (`rustock-core`, `block_uncles_validation_rule_test.rs`), with a child of genesis accepted |
| BlockUnclesValidationRuleTest.rejectBlockWithUncleHavingHigherNumber (44) | block 2 naming block 3 is refused | ported | `a_block_above_the_one_naming_it_is_no_uncle` (`block_uncles_validation_rule_test.rs`) |
| ExtraDataRuleTests.blockWithValidExtraData (45) | extra data within the limit is valid | covered | `extra_data_rule_caps_at_32_bytes` (block_rules.rs), at rskj's limit of 32 where the test sets 42 |
| ExtraDataRuleTests.blockWithValidNullExtraData (54) | none is valid | not applicable | a header's extra data is bytes, empty at the least, never null |
| ExtraDataRuleTests.blockWithValidLongerExtraDataThanAccepted (63) | one byte over the limit is invalid | covered | `extra_data_rule_caps_at_32_bytes` (block_rules.rs): 33 bytes `ExtraDataTooLarge` |
| GasLimitRuleTests.gasLimitGreaterThanMinimumGasLimit (49) | above 3,000,000 passes | covered | `gas_limit_greater_than_minimum_gas_limit` (tests.rs) |
| GasLimitRuleTests.gasLimitEqualMinimumGasLimit (55) | 3,000,000 passes | covered | `gas_limit_equal_minimum_gas_limit` (tests.rs) |
| GasLimitRuleTests.gasLimitLessThanMinimumGasLimit (61) | below fails | covered | `gas_limit_less_than_minimum_gas_limit` (tests.rs) |
| PrevMinGasPriceValidatorTest.noParentBlock (40) | no parent: invalid | not applicable | rustock's parent rules are given a parent; a block whose parent isn't held never reaches them |
| PrevMinGasPriceValidatorTest.genesisBlock (54) | genesis is valid whatever its minimum gas price | ported | `genesis_is_held_to_no_parents_minimum_gas_price` (`rustock-core`, `prev_min_gas_price_validator_test.rs`) |
| PrevMinGasPriceValidatorTest.noMinGasPrice (68) | no minimum gas price: invalid | not applicable | rskj reads it never null (`RLP.parseSignedCoinNonNullZero`), and rustock's is a number |
| PrevMinGasPriceValidatorTest.outOfValidMGPRangeBlock (83) | 1,050 against a parent at 10 is invalid | covered | `prev_min_gas_price_allows_one_percent_either_way` (block_rules.rs) |
| PrevMinGasPriceValidatorTest.validMGPInNewBlock (100) | 1,050 against 1,000 is invalid, despite the test's name | covered | `prev_min_gas_price_allows_one_percent_either_way` (block_rules.rs): 1,011 against 1,000 refused, 1,010 valid |
| RemascValidationRuleTest.noTxInTheBlock (41) | no transactions: invalid | covered | `remasc_rule_requires_remasc_as_the_last_transaction` (block_rules.rs) |
| RemascValidationRuleTest.noRemascTxInTheBlock (49) | no REMASC transaction: invalid | covered | the same |
| RemascValidationRuleTest.remascTxIsNotTheLastOne (73) | REMASC not last: invalid | covered | the same |
| RemascValidationRuleTest.remascTxInBlock (98) | REMASC last: valid | covered | the same |
| TxGasPriceCapTest.isSurpassed_whenMinGasPriceIsZeroThenFalseRegardlessTxGasPrice (33) | with a zero minimum no price passes the cap | covered | `block_txs_max_gas_price_enforces_the_100x_cap_from_rskip252` (block_rules.rs), the block's cap; the pool's is its own row, `TxValidatorMaximumGasPriceValidatorTest` |
| TxGasPriceCapTest.isSurpassed_whenRemascTransactionThenFalseRegardlessTxGasPrice (46) | a `RemascTransaction` priced over the cap doesn't pass it | not applicable | no such transaction exists: rskj makes a `RemascTransaction` only of a transaction priced at zero (`org/ethereum/core/Transaction.java` 568-590), and rustock's `Transaction::is_remasc` asks the same (`remasc_is_rskjs_is_remasc_transaction`, `crates/core/src/types/transaction.rs`). A zero price is under every cap, so the exemption (`TxGasPriceCap.java` 44-46) changes no result |
| TxGasPriceCapTest.isSurpassed_whenLessThanCapThenFalse (59) | under the cap doesn't pass it | covered | `block_txs_max_gas_price_enforces_the_100x_cap_from_rskip252` (block_rules.rs) |
| TxGasPriceCapTest.isSurpassed_whenEqualsCapThenFalse (72) | at the cap doesn't | covered | the same: 10,000 against 100 valid |
| TxGasPriceCapTest.isSurpassed_whenMoreThanCapThenTrue (85) | one over does | covered | the same: 10,001 refused |
| TxsMinGasPriceValidatorTest.blockWithEmptyTxList (40) | no transactions: valid | covered | `txs_min_gas_price_rejects_an_underpriced_tx` (block_rules.rs) |
| TxsMinGasPriceValidatorTest.blockWithNullMGP (50) | no minimum gas price: invalid | not applicable | it is a number, never null |
| TxsMinGasPriceValidatorTest.blockWithAllValidTx (58) | all at or above the minimum: valid | covered | `txs_min_gas_price_rejects_an_underpriced_tx` |
| TxsMinGasPriceValidatorTest.blockWithAllInvalidTx (70) | all below: invalid | covered | the same |
| TxsMinGasPriceValidatorTest.blockWithSomeInvalidTx (82) | some below: invalid | covered | the same |
| ValidGasUsedValidatorTest.blockWithValidGasUsed (46) | gas used under the limit: valid | covered | `test_gas_used_rule` (tests.rs) |
| ValidGasUsedValidatorTest.blockWithInvalidGasUsedBiggerThanGasLimit (56) | over it: invalid | covered | `test_gas_used_rule` (tests.rs) |
| ValidGasUsedValidatorTest.blockWithInvalidGasUsedLessThanZero (66) | negative: invalid | not applicable | rskj reads gas used with `longValueExact` (`BlockFactory.java` 187), so no header it reads has a negative one, and rustock refuses what doesn't fit (`rskj_reads_no_header_past_a_java_long_or_int`, header.rs) |
| BlockRootValidationRuleTest (four rows) | | not in the release | the class is rskj `master`'s, not VETIVER-9.0.4's; the rule itself is there (`co/rsk/validators/BlockRootValidationRule.java`) |

## Headers: `co.rsk.core` and `org.ethereum.core`, in `rustock-core`

### `co/rsk/core/BlockFactoryTest.java`: 47 listed of its 48 tests

rskj's code: `org/ethereum/core/BlockFactory.java` 148-267 (`decodeHeader`), 269-276 (the Bitcoin header's size from RSKIP98), 278-338 (the header made of what it read; block 0 a `GenesisHeader`) and 356-421 (`canBeDecoded`, `isInBaseEventRecoveryWindow`, `calculateExpectedHeaderSize`). rskj's test mocks its activations rule by rule. Each case here turns on from genesis the rules its test enables and leaves every other off (rustock's `orchid` is RSKIP92 with RSKIP98, `wasabi100` RSKIP110), builds its header with the fields rskj's builder gives it, counts the fields of its encoding against the count rskj's test asserts, and reads it back under those heights. All in `block_factory_test.rs`.

| rskj test | what it asserts | verdict | rustock test |
|---|---|---|---|
| BlockFactoryTest.decodeGenesisBlock (79) | testnet's genesis block, read with RSKIP92, has testnet's genesis hash | fixed | `the_genesis_block_reads_to_its_hash`. The rule: the hash of block 0, under RSKIP92. rskj reads block 0 as a `GenesisHeader` (`BlockFactory.java` 288-306), which hashes the difficulty as the bytes it read, `0x0000100000` (`org/ethereum/core/GenesisHeader.java` 58, 106-109). rustock's decoder hashed it as the value's own bytes, `0x100000`, so the block 0 it read hashed to `0x210bb2c9…eac521`, not `0xcabb7fbe…69adfa42`; block 0 now hashes over its difficulty as it was written. Every other field reads as rskj's (`the_genesis_block_reads_as_testnets_version_0_genesis`). |
| BlockFactoryTest.newHeaderWithNoForkDetectionDataAndRskip110On (85) | with RSKIP92 and RSKIP110, a header without merged-mining fields hashes as its merged-mining hash | ported | `a_header_without_mining_fields_hashes_as_its_merged_mining_hash` |
| BlockFactoryTest.decodeBlockPriorToHeight449AndRskip110On (98) | a merge-mined header at 20 with RSKIP110 has 19 fields, and reads back with its hash and fork-detection data | ported | `a_merge_mined_header_has_nineteen_fields_and_reads_back` |
| BlockFactoryTest.decodeBlockPriorToHeight449AndRskip110Off (114) | the same without RSKIP110 | ported | the same |
| BlockFactoryTest.decodeBlockAfterHeight449AndRskip110OFF (130) | the same at 457 without RSKIP110 | ported | the same |
| BlockFactoryTest.decodeBlockAfterHeight449AndRskip110On (146) | at 457 with RSKIP110, a coinbase committing to the merged-mining hash's first 20 bytes and 12 of fork-detection data: the header reads back with both | ported | the same. rskj's test writes the tag at the coinbase's start; the port writes it after the coinbase's 40-byte SHA-256 midstate, where rskj's proof-of-work rule looks for it (`co/rsk/validators/ProofOfWorkRule.java` 166-172) and rustock reads fork-detection data (`extract_fork_detection_data`, `crates/core/src/validation/merged_mining.rs` 430-451). rskj reads that data from the whole coinbase (`BlockHeader.java` 578-616); a coinbase whose tail doesn't carry the tag and hash fails rskj's proof-of-work rule all the same, so no header's verdict turns on it |
| BlockFactoryTest.decodeWithNoMergedMiningDataAndRskip110OffAndNoForkDetectionData (175) | a header without merged-mining fields has 16, and reads back with its hash | ported | `a_header_without_mining_fields_has_sixteen_fields_and_reads_back` |
| BlockFactoryTest.decodeWithNoMergedMiningDataAndRskip110OffAndForkDetectionData (197) | the same, built with fork-detection data, which without RSKIP110 it doesn't carry | ported | the same: rustock's header carries fork-detection data only in its coinbase |
| BlockFactoryTest.decodeBlockRskip110OffRskipUMMOnAndNoMergedMiningFieldsValidUMMRoot (214) | from RSKIPUMM, a UMM root of 20 bytes: 17 fields, read back with it | ported | `from_rskipumm_a_header_has_a_umm_root_or_isnt_read` |
| BlockFactoryTest.decodeBlockRskip110OffRskipUMMOnAndNoMergedMiningFieldsEmptyUMMRoot (232) | an empty one: 17, read back empty | ported | the same |
| BlockFactoryTest.decodeBlockRskip110OffRskipUMMOnAndNoMergedMiningFieldsNullUMMRoot (249) | none: 16 fields, not read | ported | the same |
| BlockFactoryTest.decodeBlockRskip110OffRskipUMMOnAndMergedMiningFieldsValidUMMRoot (265) | the same with merged-mining fields: 20 | ported | the same |
| BlockFactoryTest.decodeBlockRskip110OffRskipUMMOnAndMergedMiningFieldsEmptyUmmRoot (283) | 20, read back empty | ported | the same |
| BlockFactoryTest.decodeBlockRskip110OffRskipUMMOnAndMergedMiningFieldsNullUmmRoot (300) | 19, not read | ported | the same |
| BlockFactoryTest.headerIsVersion0Before351Activation (321) | with RSKIP351 at 20, a header built at 19 is version 0 | ported | `a_header_is_version_1_from_rskip351_and_before_rskip535`, of `ActivationHeights::header_version`, which rustock's miner builds by (`crates/execution/src/mining/template.rs` 353) |
| BlockFactoryTest.headerIsVersion1After351AndBefore535Activation (329) | at 20, without RSKIP535, version 1 | ported | the same |
| BlockFactoryTest.decodeCompressedBefore351 (358) | below RSKIP351, a header sent compressed reads back version 0, its bloom in slot 6 | ported | `below_rskip351_a_header_has_one_encoding_with_its_bloom` |
| BlockFactoryTest.decodeCompressedWithNoEdgesAndMergedMiningFields (381) | a version-1 header with a UMM root and merged-mining fields, sent compressed, reads back with its hash, its UMM root and slot 6's commitment, naming version 1, and neither bloom nor edges | ported | `a_compressed_version_1_header_with_mining_fields_reads_back` |
| BlockFactoryTest.decodeCompressedOfExtendedBefore351 (415) | below RSKIP351, the full encoding read as compressed reads back the same | ported | `below_rskip351_a_header_has_one_encoding_with_its_bloom` |
| BlockFactoryTest.decodeCompressedAfter351WithEdges (438) | a version-1 header with edges, sent compressed, reads back with the commitment to its extension, and neither bloom nor edges | ported | `a_compressed_version_1_header_commits_to_its_edges` |
| BlockFactoryTest.decodeFullBefore351And144 (464) | below RSKIP351, the full encoding read in full: version 0 and its bloom | ported | `below_rskip351_a_header_has_one_encoding_with_its_bloom` |
| BlockFactoryTest.decodeFullAfter351WithEdges (488) | a version-1 header in full reads back with its bloom, version and edges, which make its commitment | ported | `a_full_version_1_header_carries_its_bloom_and_edges` |
| BlockFactoryTest.decodeBlockRskip144OnRskipUMMOnAndMergedMiningFields (515) | with RSKIP144 and RSKIP351, a full header carries its version and edges after its UMM root: 22 fields with merged mining, read back | ported | `with_rskip144_a_full_header_carries_its_edges` |
| BlockFactoryTest.decodeBlockHeaderRSKIP1444ON_UMMOn_MergedMiningFieldsOFF_RSKIP5350FF (541) | without merged mining, 19 | ported | the same |
| BlockFactoryTest.decodeBlockRskip144OnRskipUMMOffAndMergedMiningFields (562) | without RSKIPUMM, 21 | ported | the same |
| BlockFactoryTest.decodeBlockRskip144OnRskipUMMOffAndNoMergedMiningFields (584) | without either, 18 | ported | the same |
| BlockFactoryTest.decodeBlockRskip535WithoutRskip351 (611) | RSKIP535 without RSKIP351: version 0, 16 fields, no base event | ported | `without_rskip351_rskip535_adds_no_base_event` |
| BlockFactoryTest.decodeBlockRskip535WithoutRskip144 (636) | with RSKIP351 and RSKIP535, version 2 with its base event: 18, read back | ported | `with_rskip535_a_full_header_carries_its_base_event` |
| BlockFactoryTest.decodeBlockRskip535WithRskip144AndNoMergedMiningFields (659) | with edges, 19 | ported | the same |
| BlockFactoryTest.decodeBlockRskip535WithUMMAndNoMergedMiningFields (686) | with a UMM root, 19 | ported | the same |
| BlockFactoryTest.decodeBlockRskip535WithRskip144AndUMMAndNoMergedMiningFields (711) | with both, 20 | ported | the same |
| BlockFactoryTest.decodeBlockRskip535WithEmptyBaseEvent (740) | an empty base event: 18, read back empty | ported | the same |
| BlockFactoryTest.decodeCompressedBlockRskip535WithRskip144 (762) | a version-2 header sent compressed reads back with its hash and slot 6's commitment, naming version 2, and neither edges nor base event | ported | `a_compressed_version_2_header_commits_to_its_base_event` |
| BlockFactoryTest.decodeCompressedBlockRskip535WithoutRskip144 (793) | the same without edges | ported | the same |
| BlockFactoryTest.decodeCompressedBlockRskip535WithUMMWithoutRskip144 (822) | the same with a UMM root | ported | the same |
| BlockFactoryTest.decodeFullBlockRskip535WithRskip144 (854) | a version-2 header in full reads back with its bloom, edges and base event | ported | `a_full_version_2_header_carries_its_bloom_edges_and_base_event` |
| BlockFactoryTest.emptyBaseEventPreservedAcrossExtensionRoundTripWithMining (895) | an empty base event survives the extension's own encoding, keeps its field (23 with merged mining) and reads back empty, the hash the same | ported | `an_empty_base_event_keeps_its_field_and_reads_back_empty` |
| BlockFactoryTest.emptyBaseEventPreservedAcrossExtensionRoundTripNoMining (929) | the same without merged mining | ported | the same |
| BlockFactoryTest.headerOneElementShortDecodableOnTestnetWithinBoundWithMining (957) | on testnet below #7,800,000, a full header one field short (22) is read, with an empty base event | ported | `testnet_reads_a_header_one_field_short_with_an_empty_base_event` |
| BlockFactoryTest.headerOneElementShortDecodableOnTestnetWithinBoundNoMining (990) | the same without merged mining | ported | the same |
| BlockFactoryTest.headerOneElementShortRejectedOnMainnetFactory (1017) | not on mainnet | ported | `no_other_chain_and_no_later_block_reads_a_header_one_field_short` |
| BlockFactoryTest.headerOneElementShortRejectedAboveUpperBound (1045) | not on testnet at #7,800,000 | ported | the same, with #7,799,999 read |
| BlockFactoryTest.headerOneElementShortRejectedOnNonTestnetChainId (1072) | not on regtest | ported | the same |
| BlockFactoryTest.headerOneElementShortWithBaseEventPresentRejectedOnTestnet (1101) | a header that keeps its base event and drops a merged-mining field is the same length; testnet reads its base event as the Bitcoin header, which RSKIP98 holds to 80 bytes, and refuses it | ported | `a_header_short_of_a_mining_field_is_not_one_short_of_its_base_event` |
| BlockFactoryTest.compressedHeaderRejectedWhenSizeDoesNotMatch (1143) | read compressed, the same 22 fields aren't read | ported | `a_compressed_header_is_not_read_one_field_short` |
| BlockFactoryTest.decodeHeaderValidatesBtcHeaderSize (1166) | from RSKIP98 a Bitcoin header of 80 bytes is read, of 79, 81 or 160 not | ported | `from_rskip98_the_bitcoin_header_is_read_at_80_bytes_only` |
| BlockFactoryTest.decodeHeaderRejectsOversizedBtcHeaderWithVariousActiveRules (1194) | an 81-byte one isn't read after a UMM root, nor after a version and edges | ported | `an_oversized_bitcoin_header_is_refused_after_the_umm_root_or_the_edges`, with 80 read in both |
| BlockFactoryTest.genesisHasVersion0 (316), from the rest of the class | with no rule on, the genesis block reads as version 0 | ported | `the_genesis_block_reads_as_testnets_version_0_genesis`, which also holds its fields to testnet's genesis header |

### `org/ethereum/core/BlockHeaderBuilderTest.java`: 5 listed of its 44 tests

rustock's miner fills a header's fields as rskj's `BlockHeaderBuilder` does (`crates/execution/src/mining/template.rs` 250-360).

| rskj test | what it asserts | verdict | rustock test |
|---|---|---|---|
| BlockHeaderBuilderTest.testHeaderCreationWithParameters (267) | twelve cases of RSKIP92's encoding, RSKIP351 and RSKIP535: a header built under RSKIPUMM with merged-mining fields has 20 fields, 22 with RSKIP351's version and edges, 23 with RSKIP535's base event, which without RSKIP351 adds none; with RSKIP92's encoding, two fewer | ported | `a_built_header_carries_the_fields_its_rules_give_it` (`rustock-core`, `block_header_builder_test.rs`), all twelve. The form with two fewer is the uncompressed encoding without the merkle proof and the coinbase, which rskj writes only to its RPC (`BlockHeader.getEncoded`, 329; `co/rsk/rpc/modules/rsk/RskModuleImpl.java` 148, 161); for those cases the port holds the full encoding to two more, and the hash, which rskj takes compressed (`getEncodedForHash`, 331), to leave the two out |
| BlockHeaderBuilderTest.createsHeaderWithVersion0WithNoRskip351 (516) | without RSKIP351, version 0 | covered | `a_header_is_laid_out_by_its_height` (`crates/core/src/config.rs`): RSKIP535 without RSKIP351 is version 0; and `a_template_carries_the_fields_its_height_brings` (`crates/execution/src/mining/tests.rs`): a template without RSKIP351 has no version fields |
| BlockHeaderBuilderTest.createsHeaderWithVersion0BeforeRskip351 (522) | before RSKIP351, version 0 | covered | `a_header_is_laid_out_by_its_height`: testnet's #7,139,599, the block before RSKIP351, is version 0 |
| BlockHeaderBuilderTest.createHeaderWithVersion1AfterRskip351 (531) | with RSKIP351 and without RSKIP535, version 1 | covered | `a_header_is_laid_out_by_its_height` (#7,139,600 and #7,604,199) and `a_template_carries_the_fields_its_height_brings` (a template is version 1, with edges where RSKIP144 is on) |
| BlockHeaderBuilderTest.createHeaderWithVersion2AfterRskip535 (538) | with both, version 2 | covered | the same (#7,604,200; a template is version 2 with an empty base event) |

The rest of the class was read: its setters and defaults, with no refusal or limit.

### Classes named in set-up only: `co/rsk/core/BlockHeaderTest.java` (40 tests) and `co/rsk/core/BlockHeaderV2Test.java` (20)

| rskj test | what it asserts | verdict | rustock test |
|---|---|---|---|
| BlockHeaderTest.getHashForMergedMiningWhenUmmRoot (295), from a class in set-up only | a header with a UMM root of 20 bytes merge-mines the hash of its own merged-mining hash's first 20 bytes and the root (`BlockHeader.java` 556-568, 624-634); rskj's test then writes fork-detection data over the last 12, which rustock's miner does apart from the header (`apply_to_hash`, `crates/execution/src/mining/fork_detection.rs`) | ported | `a_umm_root_joins_the_merged_mining_hash` (`rustock-core`, `block_header_test.rs`). No block has carried a UMM root; rustock's own test of one (`a_umm_root_is_twenty_bytes_or_none`, tests.rs) seals it with rustock's hash, so it held the length and not the hash |
| BlockHeaderTest.getHashForMergedMiningWhenEmptyUmmRoot (313), from a class in set-up only | an empty root leaves the header's own | ported | the same |

The rest of `BlockHeaderTest` was read. Its UMM roots of 19 and 21 bytes (325, 333), on which rskj throws, are refused by rustock's proof-of-work rule (`a_umm_root_is_twenty_bytes_or_none`, tests.rs); fork-detection data that a coinbase doesn't carry (344), on which rskj throws, reads as none (`extract_fork_detection_data`), which the fork-detection rule refuses (`ForkDetectionDataUnreadable`, `fork_detection_data_rule_test.rs`). The rest is the encodings of each version and the hash's caching. `BlockHeaderV2Test` was read: its base event's limit of 128 bytes (186, 200, 215) is the setter's (`BlockHeaderV2.java` 81-93), held by `v2_refuses_a_base_event_over_the_limit_and_accepts_one_at_it` (`crates/core/src/types/header_extension.rs`); rskj's decoder doesn't apply it (`BlockHeaderExtensionV2`'s constructor, 37-40, doesn't check), nor does rustock's. The rest is the Java object's: sealing and setters.

## The schedule, a transaction's cost and the difficulty

### `org/ethereum/config/blockchain/upgrades/ActivationConfigTest.java`: 7 listed of its 12 tests

| rskj test | what it asserts | verdict | rustock test, or why |
|---|---|---|---|
| ActivationConfigTest.readWithTwoUpgradesInOrchid060 (174) | with orchid060 moved to 200 and RSKIP98 named by it, at 100 RSKIP98 and RSKIP103 are off and every other rule on | not applicable | rskj's reader of a config file. rustock's schedule is in its code, held to the release's config files by `every_rule_activates_where_rskjs_config_puts_it` (`crates/execution/src/hardfork.rs`), where RSKIP103 is at orchid060, #1,052,700 on mainnet. On a copy of a chain it refuses to move a rung it reads at one height, as orchid060 is, or to schedule apart a rule it doesn't, as RSKIP98 (`a_fixed_rung_is_accepted_only_where_this_node_reads_it`, hardfork.rs, with `hop401` and `rskip97`) |
| ActivationConfigTest.readWithOneHardcodedActivationNumber (190) | RSKIP85 at 200 is off at 100, every other rule on | not applicable | the same reader. For the rules rustock schedules apart, a height of their own holds them off below it: `a_block_runs_as_its_sublists_only_under_both_rules` (hardfork.rs), RSKIP144 at #7,500,000 off at #7,499,999 |
| ActivationConfigTest.failsReadingWithMissingNetworkUpgrade (205) | a config without RSKIP85 isn't read | not applicable | a config file's error; rustock's rules are its code's |
| ActivationConfigTest.headerVersion0 (229) | without RSKIP351 and RSKIP535, version 0 | covered | `a_header_is_laid_out_by_its_height` (`crates/core/src/config.rs`) |
| ActivationConfigTest.headerVersion1 (235) | with RSKIP351 and without RSKIP535, version 1 | covered | the same |
| ActivationConfigTest.isActive_allRulesActive_returnsTrue (247) | `areActive` of three rules, all on, is true | covered | rustock has no `areActive`; both conjunctions rskj asks of it are held: RSKIP351 with RSKIP144 for a block's sublists and edges (`a_block_runs_as_its_sublists_only_under_both_rules`, hardfork.rs; `a_header_is_laid_out_by_its_height`, config.rs), and RSKIP535 with RSKIP351 for the base event (`a_header_is_laid_out_by_its_height`) |
| ActivationConfigTest.isActive_oneRuleInactive_returnsFalse (253) | with one off, false | covered | the same: each is off where either rule is |

The rest of the class was read: config files with a fork missing or a name unknown, which rustock refuses for a copy of a chain (`a_fixed_rung_is_accepted_only_where_this_node_reads_it`), and version 2 with every rule on (`a_header_is_laid_out_by_its_height`).

### `co/rsk/core/TransactionTest.java`: 5 listed of its 17 tests

rskj's code: `org/ethereum/core/Transaction.transactionCost`, which `TransactionExecutor.init` holds a transaction's gas to and the block pays.

| rskj test | what it asserts | verdict | rustock test |
|---|---|---|---|
| TransactionTest.testTransactionCostWithRSKIP400Disabled (446) | a call carrying one nonzero byte costs 21,068 | covered | `test_rskip400_calldata_reduction_at_arrowhead_activation` (`crates/execution/src/executor.rs`): 68 a nonzero byte the block before mainnet's RSKIP400, 21,204 for three |
| TransactionTest.testTransactionCostWithRSKIP400Enabled (459) | with RSKIP400, 21,016 | covered | the same: 16 from it, 21,048 for three |
| TransactionTest.testTransactionCostForContractCreationWithRSKIP438Disabled (472) | a creation carrying one byte costs 53,068 | ported | `from_rskip438_a_creation_pays_for_its_initcode_by_the_word` (`rustock-execution`, `transaction_test.rs`), run by rustock's executor on mainnet: the block before RSKIP438 a creation of one zero byte costs 53,004, of 33 bytes 53,132, of none 53,000. rskj's byte is 0xff priced with RSKIP400 off, which no network has where RSKIP438 is on and rustock's schedule can't make; zero bytes cost 4 on either side of RSKIP400. rustock's own test of initcode (`test_rskj_initcode_cost_shanghai`, executor.rs) holds only `gas_used >= 32,066` |
| TransactionTest.testTransactionCostForContractCreationWithRSKIP438Enabled (488) | from RSKIP438, 2 more for its one word of initcode: 53,070 | ported | the same: 53,006 from mainnet's RSKIP438, and 53,136 for two words |
| TransactionTest.testTransactionCostForContractCreationWithNullDataAndRSKIP438Enabled (504) | a creation with no data: 53,000 | ported | the same: 53,000 on both sides |

The rest of the class was read: signing, EIP-155 and encoding, which every transaction exercises, and which `to` makes a creation: none does, forty zeros don't, and a one-byte `to` is refused, as rustock's reader refuses any but 20 bytes (`what_rskj_cannot_take_is_refused_when_read`, `crates/core/src/types/transaction.rs`).

### `co/rsk/jsontestsuite/LocalBasicTest.java`: 3 listed of its 4 tests

rskj's code: `co/rsk/core/DifficultyCalculator.java` 39-105, with `Constants.getDifficultyBoundDivisor` (154-162) and `getMinimumDifficulty` (140-148). The cases are rskj's `json/BasicTests/difficulty*.json`. Their children sit at their parents' timestamps, which several put past 2^63: rskj reads them as Java longs, wrapping (`org/ethereum/json/Utils.parseLong`), rustock as `u64`s, and two equal timestamps are no time apart either way. All in `rustock-core`, `local_basic_test.rs`.

| rskj test | what it asserts | verdict | rustock test |
|---|---|---|---|
| LocalBasicTest.runDifficultyTestBeforeRSKIP156 (51) | `difficultyBeforeRSKIP156.json`'s ten cases under mainnet's constants without RSKIP156: the difficulty moves by a fiftieth | ported | `before_rskip156_the_difficulty_moves_by_a_fiftieth` |
| LocalBasicTest.runDifficultyTestBeforeRSKIP290 (58) | `difficulty.json`'s ten under testnet's constants without RSKIP290: by a four-hundredth | ported | `from_rskip156_the_difficulty_moves_by_a_four_hundredth` |
| LocalBasicTest.runDifficultyTestAfterRSKIP290 (66) | `difficulty1.json`'s one under testnet's with every rule: a child that would rise to 218,359,490 is raised to the floor of 550,000,000 | ported | `from_rskip290_testnets_difficulty_is_floored_at_550_million` |
| LocalBasicTest.runDifficultyTest (74), from the rest of the class | `difficulty.json`'s ten under mainnet's constants with every rule | ported | `on_mainnet_from_rskip156_the_difficulty_moves_by_a_four_hundredth` |

### `org/ethereum/config/ConstantsTest.java`: 1 listed of its 4 tests

| rskj test | what it asserts | verdict | rustock test |
|---|---|---|---|
| ConstantsTest.rskip297ActivationTest (74) | the window between a header's timestamp and its Bitcoin header's: 300 s on regtest and mainnet either side of RSKIP297, and on testnet 300 s before it and 7,200 s from it | covered | `testnet_and_testnet2_widen_the_window_from_rskip297` (tests.rs) |
| ConstantsTest.maxInitcodeSizeTest (86), from the rest of the class | a creation's initcode may be 49,152 bytes | ported | `from_rskip438_a_creations_initcode_is_held_to_49152_bytes` (`rustock-execution`, `constants_test.rs`): from mainnet's RSKIP438, 49,152 bytes run and 49,153 are refused (`TransactionExecutor.init`), and before it 49,153 run. `a_local_creation_passes_the_limit_on_initcode` (executor.rs) held the refusal alone |

The rest of the class was read: the testnet genesis federation's keys, which are the Bridge's, and the sequential sublist's floor of gas, 10,000,000 on mainnet and testnet and 6,800,000 on regtest (`ChainConfig::min_sequential_set_gas_limit`), which every testnet block that runs as its sublists reads.

## The miner and the pool

Neither holds a network rule but for RSKIP-252's cap, which the block holds too (`BlockTxsMaxGasPriceRuleTest`, above). The rows of `docs/rskj-test-parity.md` for these tests were held against the release and rustock's tests; where its test names have since changed, the current one is given.

| rskj test | what it asserts | verdict | rustock test |
|---|---|---|---|
| BlockToMineBuilderTest.buildBlockBeforeUMMActivation (165) | below RSKIPUMM a block built has no UMM root | covered | `umm_root_appears_once_rskip_umm_is_active` (`crates/execution/src/mining/tests.rs`) |
| BlockToMineBuilderTest.buildBlockAfterUMMActivation (173) | from it, an empty one | covered | the same, and `a_solution_is_imported_with_umm_root_present` (mining/tests.rs) |
| BlockToMineBuilderTest.buildBlockBeforeRskip351 (197) | before RSKIP351 a block built is version 0 | covered | `a_template_carries_the_fields_its_height_brings` (mining/tests.rs): a template without RSKIP351 has no version fields. The table's gap was written when only version 2 was tested |
| BlockToMineBuilderTest.buildBlockAfterRskip351 (204) | from it, without RSKIP535, version 1 | covered | the same: version 1, with empty edges where RSKIP144 is on |
| MinerUtilsTest.filterTransactions_whenRskip252DisabledThenTxIncludedRegardlessGasPrice (167) | before RSKIP252 the miner takes a price however far over the cap | covered | `filter_transactions_when_rskip252_disabled_then_tx_included_regardless_gas_price` (mining/tests.rs) |
| MinerUtilsTest.filterTransactions_whenRskip252EnabledThenTxWithMoreGasPriceThanCapExcluded (190) | from it, under the cap stays and over it goes, to be dropped from the pool | covered | `filter_transactions_when_rskip252_enabled_then_tx_with_more_gas_price_than_cap_excluded` (mining/tests.rs): 199 kept and 201 removed against a cap of 200 |
| TxPendingValidatorTest.isValid_ShouldBeValid_WhenRSKIP144IsNotActivated (47) | without RSKIP144 a transaction's gas limit may be the block's | covered | `gas_limit_is_bounded_by_the_larger_sublist_under_rskip144` (`crates/sync/src/txpool.rs`): without the sublists the bound is the block's, 8,000,000 |
| TxPendingValidatorTest.isValid_ShouldBeInvalid_WhenRSKIP144IsNotActivated (80) | and not one more | covered | `test_high_gas_limit_rejected` (txpool.rs): 9,000,000 against a block of 8,000,000 refused |
| TxPendingValidatorTest.isValid_ShouldBeValid_WhenRSKIP144IsAlreadyActivated (113) | with RSKIP351 and RSKIP144 (`TxPendingValidator.java` 77-78), the larger sublist's | covered | `gas_limit_is_bounded_by_the_larger_sublist_under_rskip144`: 6,800,000 taken |
| TxPendingValidatorTest.isValid_ShouldBeInvalid_WhenRSKIP144IsAlreadyActivated (148) | and not one more | covered | the same: 7,000,000 refused, which fits the block |
| TxValidatorMaximumGasPriceValidatorTest.validate_whenRskip252DisabledThenTxValidRegardlessGasPrice (65) | before RSKIP252 the pool takes a price however far over the cap | covered | `validate_when_rskip252_disabled_then_tx_valid_regardless_gas_price` (txpool.rs) |
| TxValidatorMaximumGasPriceValidatorTest.validate_whenRskip252EnabledThenTxWithMoreGasPriceThanCapInvalid (80) | from it, one wei over the cap is refused | covered | `validate_when_rskip252_enabled_then_tx_with_more_gas_price_than_cap_invalid` (txpool.rs) |

The rest of these classes was read, and `BlockToMineBuilderEvictionEndToEndTest` (set-up only): the miner's choice of transactions and uncles and its eviction of those that fail, and the pool's refusal of a nonce of nine bytes or with leading zeros (held by `validate_nonce_encoding`, txpool.rs). None holds a network rule.

## The wire, the RPC and the command line

| rskj test | what it asserts | verdict | why |
|---|---|---|---|
| Web3ImplLogsTest.getLogsFromBlockchainWithEventInContractCreation (364) | `eth_getLogs` | not applicable | the RPC's |
| Web3ImplLogsTest.getLogsTwiceFromBlockchainWithEventInContractCreation (380) | the same | not applicable | the RPC's |
| Web3ImplLogsTest.getLogsFromBlockchainWithEventInContractCreationReturnsAsExpectedWithBlockHashFilter (846) | the same | not applicable | the RPC's |
| Web3ImplLogsTest.getLogsThrowsExceptionWhenBlockHashIsUsedCombinedWithFromBlock (876) | the same | not applicable | the RPC's |
| Web3ImplLogsTest.getLogsThrowsExceptionWhenBlockHashIsUsedCombinedWithToBlock (883) | the same | not applicable | the RPC's |
| CliToolsTest.importBlocksWithoutRskip351 (361) | the `import-blocks` tool, without RSKIP351 and RSKIP144 | not applicable | the command line's |
| CliToolsTest.importBlocksWithRskip351InMiddle (374) | the same, with them from block 2 | not applicable | the command line's |
| MessageTest.encodeDecodeBlockHeadersResponseMessageWithoutRSKIP351AndRSKIP535 (223) | a headers-response message's encoding | not applicable | the wire's |
| RskContextTest.shouldBuildAsyncNodeBlockProcessor (193) | the node builds its asynchronous block processor | not applicable | the node's wiring |
| NodeBlockProcessorTest.processBodyRequestMessageUsingBlockInBlockchainWithoutRskip351 (795) | a body request is answered without an extension where RSKIP351 is off | not applicable | the wire's |

`EthSubscriptionNotificationTest` (set-up only) was read: the RPC's subscriptions.

## A block's run: `co.rsk.core.bc` and `co.rsk.core.parallel`, in `rustock-execution`

rskj runs most of `BlockExecutorTest` as its miner does (`executeForMining`, `executeAndFill`): picking a block's transactions, laying them out in sublists and filling in the header. What a block is held to is `BlockExecutor.validate` (`co/rsk/core/bc/BlockExecutor.java` 261-316): the state root from RSKIP126 (319-332), the receipts root, the logs bloom, the gas used, the fees and the transactions run. rustock's is `BlockProcessor::process_block` (`crates/execution/src/processor.rs`). rskj runs each test with RSKIP144 on and off; the ports run on testnet at #8,000,000, where a block runs as its sublists, without edges and with them, in `rustock-execution`, `block_executor_test.rs`, unless named otherwise.

### `co/rsk/core/bc/BlockExecutorTest.java`: 44 listed of its 44 tests

| rskj test | what it asserts | verdict | rustock test, or why |
|---|---|---|---|
| BlockExecutorTest.executeBlockWithoutTransaction (191) | a block of no transactions leaves the state as it was, with no receipts | ported | `a_block_without_transactions_leaves_the_state_as_it_was`, without edges and with empty ones |
| BlockExecutorTest.executeForMining_whenTxExecutionThrowsError_isCaughtAndTxMarkedInvalid (210) | the miner's run catches a Java `Error` thrown by a transaction's and marks the transaction invalid | not applicable | the miner's, and a Java `Error`'s |
| BlockExecutorTest.execute_whenValidationTxThrowsError_propagatesInsteadOfRejectingBlock (242) | an `Error` while a block is checked isn't made a verdict that the block is invalid | covered | rustock, running a block beside the chain, holds its own storage or database failure as a local fault, not the block's (`crates/sync/src/service/beside.rs` 170-176): `a_block_that_failed_for_a_local_fault_is_tried_again` (`crates/sync/src/service/beside/tests.rs`). A failure to allocate aborts the process |
| BlockExecutorTest.executeBlockWithOneTransaction (280) | a transfer of 10 at a gas price of 1: 21,000 gas, 21,000 in fees, the sender left 30,000 less both | covered | `test_rskj_block_executor_balance_deduction` (`crates/execution/src/processor.rs`) |
| BlockExecutorTest.executeBlockWithOneTransactionAndCollectingProgramResults (332) | the same, with the node keeping each transaction's program result | not applicable | what it adds is the node's record of program results |
| BlockExecutorTest.executeBlockWithTwoTransactions (385) | two transfers: 21,000 gas each, counted up to 42,000, 42,000 in fees, the sender left 60,000 less those and 20 | ported | `two_transfers_count_their_gas_up_and_pay_their_fees` |
| BlockExecutorTest.executeAndFillBlockWithNoSavingToStore (449) | the miner's run saves no state | not applicable | the miner's and the store's |
| BlockExecutorTest.executeBlockWithSavingToStore (463) | a block's run saves its state | not applicable | the store's |
| BlockExecutorTest.executeAndFillBlockWithOneTransaction (477) | the miner fills a header with what its run comes to | covered | `every_scenario_mines_a_block_this_node_accepts` (`crates/execution/src/mining/workload.rs`) |
| BlockExecutorTest.executeAndFillBlockWithTxToExcludeBecauseSenderHasNoBalance (572) | the miner leaves out a transaction its sender can't pay for | not applicable | the miner's |
| BlockExecutorTest.executeBlockWithTxThatMakesBlockInvalidSenderHasNoBalance (588) | a block holding a transfer from an account with nothing is invalid | ported | `a_sender_with_nothing_to_pay_makes_its_block_invalid` |
| BlockExecutorTest.executeSequentiallyATransactionAndGasShouldBeSubtractedCorrectly (648) | the miner's run under RSKIP144: one transaction laid out, its gas counted | covered | `the_first_transaction_goes_in_the_first_parallel_sublist` (`crates/execution/src/mining/partition.rs`); a receipt's gas counted within its sublist, `gas_restarts_at_each_sublist_boundary` (processor.rs) |
| BlockExecutorTest.executeSequentiallyTenIndependentTxsAndThemShouldGoInBothSublists (674) | ten independent transactions go into both parallel sublists | covered | `independent_transfers_fill_both_parallel_sublists` (workload.rs) and `an_independent_transaction_takes_the_emptier_parallel_sublist` (partition.rs) |
| BlockExecutorTest.gasUsedShouldNeverSurprassBlockGasLimit (716) | the miner's block uses no more gas than its limit | covered | the rule a block is held to, `a_transaction_whose_limit_exceeds_the_blocks_remaining_gas_invalidates_the_block` (processor.rs); the miner, `a_transaction_that_fits_nowhere_is_refused_and_records_nothing` (partition.rs) |
| BlockExecutorTest.whenParallelSublistsAreFullTheLastTxShouldGoToSequential (738) | with the parallel sublists full, the next goes to the sequential | covered | `when_every_parallel_sublist_is_full_an_independent_transaction_goes_sequential` (partition.rs) |
| BlockExecutorTest.executeATxInSequentialAndBlockResultShouldTrackTheGasUsedInTheBlock (784) | the block's gas used is its transactions' sum | covered | `no_edges_means_a_block_wide_sum` (processor.rs), and the port of `executeBlockWithTwoTransactions` |
| BlockExecutorTest.withTheSublistsFullTheLastTransactionShouldNotFit (805) | with every sublist full, the last transaction is left out | covered | `a_transaction_that_fits_nowhere_is_refused_and_records_nothing` (partition.rs) |
| BlockExecutorTest.withSequentialSublistFullRemascTxShouldFit (827) | with the sequential sublist full, REMASC still goes in | not applicable | the miner's: rustock's lays out REMASC with no check of the gas left (`add_remasc`, partition.rs), untested with the sublist full |
| BlockExecutorTest.executeParallelBlocksWithDifferentSubsets (851) | ten transfers as sublists [4, 8] and [5] come to one state | ported | `independent_transfers_come_to_one_state_however_their_block_splits_them` |
| BlockExecutorTest.executeParallelBlockAgainstSequentialBlock (871) | as sublists and as one list, the same | ported | the same |
| BlockExecutorTest.executeInvalidParallelBlockDueToCollision (892) | two transfers, each to the other's sender, in parallel sublists of their own: invalid | ported | `parallel_sublists_may_not_touch_what_another_writes`: `SublistCollision`. rustock's other tests hold the collision in its tracker alone (`two_sublists_crediting_the_same_account_collide`, `crates/execution/src/collision.rs`) |
| BlockExecutorTest.whenExecuteATxWithGasLimitExceedingSublistGasLimitShouldNotBeInlcuded (906) | a transaction declaring more than the sequential sublist's gas is left out | covered | the miner, `a_transaction_that_fits_nowhere_is_refused_and_records_nothing` (partition.rs); a block holding one in a sublist too small for it is invalid, `the_chain_configuration_sets_the_sublists_gas` (processor.rs) |
| BlockExecutorTest.ifThereIsACollisionBetweenParallelAndSequentialSublistsTxShouldNotBeConsidered (955) | the same two, the second in the sequential sublist: both run | ported | `parallel_sublists_may_not_touch_what_another_writes`; the tracker's `the_sequential_set_is_not_recorded` (collision.rs) |
| BlockExecutorTest.executeParallelBlockTwice (970) | the same block run twice comes to one state | ported | `independent_transfers_come_to_one_state_however_their_block_splits_them`. Its other assertion, that two blocks built alike hash alike, is of rskj's block generator |
| BlockExecutorTest.blockWithOnlyRemascShouldGoToSequentialSublist (1011) | the miner lays REMASC out last, in the sequential sublist | covered | `the_remasc_transaction_closes_the_sequential_sublist` (partition.rs) |
| BlockExecutorTest.blockWithOneTxRemascShouldGoToSequentialSublist (1019) | the same, after one transaction | covered | the same |
| BlockExecutorTest.blockWithManyTxsRemascShouldGoToSequentialSublist (1027) | after several | covered | the same |
| BlockExecutorTest.blockWithMoreThanThreadsTxsRemascShouldGoToSequentialSublist (1035) | after more than there are threads | covered | the same |
| BlockExecutorTest.blockWithExcludedTransactionHasRemascInSequentialSublist (1043) | after a transaction left out | covered | the same |
| BlockExecutorTest.validateStateRootWithRskip126DisabledAndValidStateRoot (1052) | below RSKIP126, a block's state root passes | ported | `below_rskip126_a_blocks_state_root_is_not_held_to_its_run`, on mainnet either side of wasabi100, where `reference.conf` puts RSKIP126 |
| BlockExecutorTest.validateStateRootWithRskip126DisabledAndInvalidStateRoot (1074) | below RSKIP126, a wrong one passes too | ported | the same: 0x01020304 passes at #1,590,999 and is refused at #1,591,000 |
| BlockExecutorTest.validateBlock (1102) | a block carrying what its run comes to is valid | ported | `a_block_is_held_to_what_its_run_comes_to` |
| BlockExecutorTest.invalidBlockBadStateRoot (1117) | a state root a byte off is refused | covered | `test_process_block_validates_state_root` (processor.rs), on an empty block; held again with a transfer, as one list and as sublists, by `a_block_is_held_to_what_its_run_comes_to` |
| BlockExecutorTest.invalidBlockBadReceiptsRoot (1134) | a receipts root a byte off is refused | ported | `a_block_is_held_to_what_its_run_comes_to`: `ReceiptsRootMismatch` |
| BlockExecutorTest.invalidBlockBadGasUsed (1151) | gas used of 0 is refused | covered | `test_rskj_invalid_block_bad_gas_used` (processor.rs); held again by the port |
| BlockExecutorTest.invalidBlockBadPaidFees (1167) | fees of 0 are refused | ported | `a_block_is_held_to_what_its_run_comes_to`: `PaidFeesMismatch` |
| BlockExecutorTest.invalidBlockBadLogsBloom (1183) | a logs bloom a byte off is refused | ported | the same: `LogsBloomMismatch` |
| BlockExecutorTest.executeBlocksWithOneStrangeTransactions1 (1522) | a `to` of one byte makes no transaction | covered | `what_rskj_cannot_take_is_refused_when_read` (`crates/core/src/types/transaction.rs`), which refuses a `to` of any length but none and 20 |
| BlockExecutorTest.executeBlocksWithOneStrangeTransactions2 (1530) | nor one of 1,024 bytes | covered | the same |
| BlockExecutorTest.executeBlocksWithOneStrangeTransactions3 (1538) | a value written in 1,024 bytes runs as its number | ported | `a_value_padded_with_zeros_reads_as_its_number` (`rustock-core`, `block_executor_test.rs`): read as 10, and kept and hashed as the transaction writing it in one byte |
| BlockExecutorTest.executeBlockWithContractEmittingLogsUsingDsl (1692) | three calls log twice each, and the block's six logs are numbered 0 to 5 | not applicable | a log's number in its block is the RPC's: a receipt holds a log as its address, topics and data (`org/ethereum/vm/LogInfo.getEncoded`). The logs go into the receipts root and the bloom, held above |
| BlockExecutorTest.executeBlockWithLogEmittingContractAndVerifyLogIndexes (1772) | the same, numbered across three transactions | not applicable | the same |
| BlockExecutorTest.executeBlockWithBaseEventFromRepository (1816) | with RSKIP535 the miner fills a header's base event from the Bridge's storage | covered | `a_version_2_template_carries_the_base_event_its_block_sets` (`crates/execution/src/mining/tests.rs`). No rule holds a block's base event to the Bridge's: rskj sets it only in the miner's `fill` (`BlockExecutor.java` 201-238) |
| BlockExecutorTest.executeBlockWithBaseEventFromRepository_WhenRepositoryIsNull (1868) | with none stored, an empty one | not applicable | the miner's: rustock's template reads none as empty (`stored_base_event`, `crates/execution/src/mining/template.rs`), untested |

### `co/rsk/core/bc/BlockExecutorInvalidTxTest.java`: 4 listed of its 4 tests

| rskj test | what it asserts | verdict | rustock test, or why |
|---|---|---|---|
| BlockExecutorInvalidTxTest.executeForMining_postRSKIP144_whenTxThrows_shouldSkipTxAndContinue (100) | under RSKIP144 the miner drops a transaction whose run throws, and goes on | not applicable | the miner's |
| BlockExecutorInvalidTxTest.executeForMining_preRSKIP144_whenTxThrows_shouldSkipTxAndContinue (134) | the same without it | not applicable | the miner's |
| BlockExecutorInvalidTxTest.executeForMining_blockImport_whenTxThrows_shouldReturnInterrupted (163) | where a block is imported, a transaction that can't run makes it invalid | covered | `a_transaction_whose_limit_exceeds_the_blocks_remaining_gas_invalidates_the_block` (processor.rs); and `a_sender_with_nothing_to_pay_makes_its_block_invalid`, above |
| BlockExecutorInvalidTxTest.executeForMining_preRSKIP144_whenTxReturnsFalse_shouldNotBeInInvalidList (185) | the miner lists as invalid only the transaction that threw | not applicable | the miner's |

### `co/rsk/core/parallel/ParallelExecutionStateTest.java`: 11 listed of its 46 tests

Each builds a chain in rskj's test language twice, with RSKIP144 from block 0 and without it, and asserts that the two come to one state root and that the miner laid the last block out with the edges given. That a block comes to one state however it is laid out is held by `independent_transfers_come_to_one_state_however_their_block_splits_them`, above; the miner's layout by rustock's partition tests (`crates/execution/src/mining/partition.rs`).

| rskj test | what it asserts | verdict | rustock test, or why |
|---|---|---|---|
| ParallelExecutionStateTest.empty (339) | no transactions: no edges | covered | `a_new_handler_has_nothing_and_no_edges` |
| ParallelExecutionStateTest.oneTx (344) | one transfer: [1] | covered | `the_first_transaction_goes_in_the_first_parallel_sublist` |
| ParallelExecutionStateTest.sameSender (370) | two from one sender share a sublist: [2] | covered | `two_transactions_from_one_sender_share_a_sublist` |
| ParallelExecutionStateTest.sameRecipient (405) | two to one recipient: [2] | covered | `two_transactions_writing_the_same_key_go_in_the_same_sublist` |
| ParallelExecutionStateTest.sameContractRecipient (439) | two paying one contract: [2] | covered | the same |
| ParallelExecutionStateTest.callCreatedContract (569) | a call to a contract created before it in the block: [2] | covered | `a_read_of_an_earlier_write_joins_its_sublist` |
| ParallelExecutionStateTest.sendToCreatedContract (601) | a payment to it: [2] | covered | `two_transactions_writing_the_same_key_go_in_the_same_sublist` |
| ParallelExecutionStateTest.whenAnInternalTxCallsAContractThatCallsAPrecompiledShouldGoToSequentialIfDisabled (1158) | with IDENTITY barred from parallel sublists, a call reaching it through two contracts goes to the sequential | not applicable | a set no network has: rskj bars REMASC and the Bridge (`co/rsk/config/RskSystemProperties.java` 82-84), its setter being for tests (264-267), and so does rustock (`SequentialOnlyPrecompileInParallelSublist`, processor.rs) |
| ParallelExecutionStateTest.whenAnInternalTxCallsAContractThatCallsAPrecompiledShouldGoToParallelIfEnabled (1183) | with none barred, it stays in a parallel sublist | covered | as the network has it, IDENTITY isn't barred: `a_transaction_reaches_remasc_or_the_bridge_as_rskj_counts_it` (`crates/execution/src/executor.rs`), whose `calls-identity` reaches IDENTITY and no barred precompile, by rskj's own count |
| ParallelExecutionStateTest.whenAnInternalTxCallsAContractThatCallsAPrecompiledAndRevertsShouldGoToSequentialIfDisabled (1208) | with the Bridge barred, a call reaching it through two contracts, which reverts, goes to the sequential | covered | the same test's `calls-bridge-via-callee` and `calls-reverting-caller-of-bridge`; a block holding a contract's call to the Bridge in a parallel sublist is invalid, `a_refused_bridge_call_in_a_parallel_sublist_invalidates_the_block` (processor.rs); the miner, `a_bridge_call_is_placed_in_the_sequential_sublist` (workload.rs) |
| ParallelExecutionStateTest.whenAnInternalTxCallsAContractThatCallsAPrecompiledAndRevertsShouldGoToParallelIfEnabled (1234) | with none barred, the call to the Bridge stays parallel | not applicable | a set no network has: the Bridge is always barred |

The rest of the class was read: reads, writes and updates of one cell by two transactions, with and without a revert, the other precompile cases, and the falls to the sequential sublist for gas and for collisions. They are the miner's layout, held by rustock's partition tests, and, for a reverted write, by the collision tracker (`under_the_deployed_rule_a_reverted_write_collides`, collision.rs).

### Classes in set-up only

`BlockChainImplInvalidTest` was read: a block refused for its state root (`test_process_block_validates_state_root`, processor.rs), for its uncles' hash (`test_process_block_validates_ommers_hash`, processor.rs), and for its minimum gas price, by the rules of the `PrevMinGasPriceValidatorTest` and `TxsMinGasPriceValidatorTest` rows above. `InvalidTxMiningEvictionTest` was read: the miner's run drops a transaction that throws and the pool evicts it, which holds no network rule.
