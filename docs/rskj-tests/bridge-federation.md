# rskj's tests of the federation, its scripts and the Bridge's utilities

One row per rskj test that names a rule by its number, and one per test ported from the rest of
its class, from rskj's release VETIVER-9.0.4 (`rskj-core/src/test/java/`). Verdicts: **covered**
(the rustock test that asserts the same), **ported** (a test written for it), **not applicable**
(why rustock needs no such test) and **fixed** (a ported test that found rustock doing otherwise
than rskj; rustock was put right, the test passes, and the row says what differed).

rskj's places are paths under `rskj-core/src/main/java/` at VETIVER-9.0.4, and bitcoinj-thin's
under `src/main/java/` at v0.14.4-rsk-18. The ported tests are in
`crates/execution/src/rskj_tests/bridge_federation/`, one module for each rskj class; a test
named with a file is elsewhere in `crates/execution/src/`.

## co.rsk.peg.BridgeUtilsTest

Module `bridge_utils_test.rs`. rskj reaches these utilities from three Bridge methods, and the
ports go through them: `registerFastBridgeBtcTransaction` reads and writes the addresses and
counts the value sent, `getEstimatedFeesForNextPegOutEvent` sizes a peg-out of two inputs, and
`releaseBtc` sizes the regular one for its minimum.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| BridgeUtilsTest.serializeBtcAddressWithVersion_p2pkh_testnet_before_rskip284 | before RSKIP284 a testnet P2PKH address is written `6f` and its hash160 | ported | `a_flyover_address_is_written_as_its_version_byte_and_hash` |
| BridgeUtilsTest.serializeBtcAddressWithVersion_p2sh_testnet_before_rskip284 | before RSKIP284 a testnet P2SH address is written `00c4` and its hash160 | not applicable | no path writes it: `Bridge.registerFlyoverBtcTransaction` reads both addresses first (Bridge.java:1355-1368), and before RSKIP284 bitcoinj refuses version 196, -60 as a signed byte; that refusal is covered by `a_flyover_address_bitcoinj_refuses_is_a_generic_error` (bridge/peg.rs) |
| BridgeUtilsTest.serializeBtcAddressWithVersion_p2pkh_mainnet_before_rskip284 | before RSKIP284 a mainnet P2PKH address is written `00` and its hash160 | ported | `a_flyover_address_is_written_as_its_version_byte_and_hash` |
| BridgeUtilsTest.serializeBtcAddressWithVersion_p2sh_mainnet_before_rskip284 | before RSKIP284 a mainnet P2SH address is written `05` and its hash160 | ported | `a_flyover_address_is_written_as_its_version_byte_and_hash` |
| BridgeUtilsTest.serializeBtcAddressWithVersion_p2pkh_testnet_after_rskip284 | from RSKIP284 `6f` and the hash160 | ported | `a_flyover_address_is_written_as_its_version_byte_and_hash` |
| BridgeUtilsTest.serializeBtcAddressWithVersion_p2sh_testnet_after_rskip284 | from RSKIP284 `c4`, one byte, and the hash160 | ported | `a_flyover_address_is_written_as_its_version_byte_and_hash` |
| BridgeUtilsTest.serializeBtcAddressWithVersion_p2pkh_mainnet_after_rskip284 | from RSKIP284 `00` and the hash160 | ported | `a_flyover_address_is_written_as_its_version_byte_and_hash` |
| BridgeUtilsTest.serializeBtcAddressWithVersion_p2sh_mainnet_after_rskip284 | from RSKIP284 `05` and the hash160 | ported | `a_flyover_address_is_written_as_its_version_byte_and_hash` |
| BridgeUtilsTest.deserializeBtcAddressWithVersion_before_rskip284_p2sh_testnet | before RSKIP284 a testnet P2SH address (`c4`) throws | covered | `a_flyover_address_bitcoinj_refuses_is_a_generic_error` (bridge/peg.rs: testnet #2,122,593, a P2SH address answers GENERIC_ERROR) |
| BridgeUtilsTest.deserializeBtcAddressWithVersion_before_rskip284_p2pkh_testnet | before RSKIP284 `6f` and 20 bytes read as version 111 and that hash | covered | `a_flyover_derivation_hash_reads_the_address_bitcoinj_read` (bridge/peg.rs), and `a_flyover_address_is_written_as_its_version_byte_and_hash` |
| BridgeUtilsTest.deserializeBtcAddressWithVersion_null_bytes | null throws | not applicable | an ABI `bytes` argument decodes to an array, never to Java's null |
| BridgeUtilsTest.deserializeBtcAddressWithVersion_empty_bytes | from RSKIP284 no bytes throws | ported | `from_rskip284_a_flyover_address_is_21_bytes_of_a_version_its_network_takes` |
| BridgeUtilsTest.deserializeBtcAddressWithVersion_p2pkh_testnet | from RSKIP284 a testnet P2PKH address read as version 111 and its hash | ported | `from_rskip284_a_flyover_address_is_21_bytes_of_a_version_its_network_takes` |
| BridgeUtilsTest.deserializeBtcAddressWithVersion_p2pkh_testnet_wrong_network | from RSKIP284 a testnet P2PKH version throws on mainnet | ported | `from_rskip284_a_flyover_address_is_21_bytes_of_a_version_its_network_takes` |
| BridgeUtilsTest.deserializeBtcAddressWithVersion_p2sh_testnet | from RSKIP284 a testnet P2SH address read as version 196 and its hash | ported | `from_rskip284_a_flyover_address_is_21_bytes_of_a_version_its_network_takes` |
| BridgeUtilsTest.deserializeBtcAddressWithVersion_p2sh_testnet_wrong_network | from RSKIP284 a testnet P2SH version throws on mainnet | ported | `from_rskip284_a_flyover_address_is_21_bytes_of_a_version_its_network_takes` |
| BridgeUtilsTest.deserializeBtcAddressWithVersion_p2pkh_mainnet | from RSKIP284 a mainnet P2PKH address read as version 0 and its hash | ported | `from_rskip284_a_flyover_address_is_21_bytes_of_a_version_its_network_takes` |
| BridgeUtilsTest.deserializeBtcAddressWithVersion_p2pkh_mainnet_wrong_network | from RSKIP284 a mainnet P2PKH version throws on testnet | covered | `a_flyover_address_bitcoinj_refuses_is_a_generic_error` (bridge/peg.rs: testnet #2,581,800, version 0 answers GENERIC_ERROR) |
| BridgeUtilsTest.deserializeBtcAddressWithVersion_p2sh_mainnet | from RSKIP284 a mainnet P2SH address read as version 5 and its hash | ported | `from_rskip284_a_flyover_address_is_21_bytes_of_a_version_its_network_takes` |
| BridgeUtilsTest.deserializeBtcAddressWithVersion_p2sh_mainnet_wrong_network | from RSKIP284 a mainnet P2SH version throws on testnet | ported | `from_rskip284_a_flyover_address_is_21_bytes_of_a_version_its_network_takes` |
| BridgeUtilsTest.deserializeBtcAddressWithVersion_with_extra_bytes | from RSKIP284 31 bytes throw | ported | `from_rskip284_a_flyover_address_is_21_bytes_of_a_version_its_network_takes` (22 bytes: `a_flyover_address_bitcoinj_refuses_is_a_generic_error`, bridge/peg.rs) |
| BridgeUtilsTest.deserializeBtcAddressWithVersion_invalid_address_hash | from RSKIP284 2 bytes throw | ported | `from_rskip284_a_flyover_address_is_21_bytes_of_a_version_its_network_takes` |
| BridgeUtilsTest.testCalculatePegoutTxSize_2Inputs_2Outputs | 2 inputs, 2 outputs: 2058 bytes for 13 members, 694 for their P2SH-P2WSH ERP federation | ported | `pegout_sizes_are_rskjs_for_each_federation_format` (the 2058 also in `regular_pegout_tx_size_matches_rskj`, bridge/peg.rs) |
| BridgeUtilsTest.testCalculatePegoutTxSize_9Inputs_2Outputs | 9002 and 2866 | ported | `pegout_sizes_are_rskjs_for_each_federation_format`: the sizer at rskj's shape, and rustock's estimate at 2 inputs and 2 outputs, the input count every rskj caller passes (BridgeSupport.java:959, :1228, :2647) |
| BridgeUtilsTest.testCalculatePegoutTxSize_10Inputs_20Outputs | 10570 and 3752 | ported | `pegout_sizes_are_rskjs_for_each_federation_format`, rustock's estimate at 2 inputs and 20 outputs |
| BridgeUtilsTest.testCalculatePegoutTxSize_50Inputs_200Outputs | 56010 and 21922 | ported | `pegout_sizes_are_rskjs_for_each_federation_format`, rustock's estimate at 2 inputs and 200 outputs |
| BridgeUtilsTest.testCalculatePegoutTxSize_50Inputs_200Outputs_nonStandardErpFederation | 26510 for a non-standard ERP federation of 3, 13172 for its P2SH-P2WSH one | ported | `pegout_sizes_are_rskjs_for_each_federation_format`, rustock's estimate at 2 inputs and 200 outputs |
| BridgeUtilsTest.testCalculatePegoutTxSize_100Inputs_50Outputs_nonStandardErpFederation | 41810 and 15135 | ported | `pegout_sizes_are_rskjs_for_each_federation_format`, rustock's estimate at 2 inputs and 50 outputs |
| BridgeUtilsTest.getRegularPegoutTxSize_has_proper_calculations | the regular peg-out size within 10% of a signed 2-of-3 peg-out | ported | `the_regular_pegout_size_sets_releasebtcs_minimum`: the size, and `releaseBtc` queues a request worth the minimum it sets and refuses one a satoshi short |
| BridgeUtilsTest.getAmountSentToAddresses_ok | from RSKIP293 what is sent to the active and retiring addresses is summed | ported | `from_rskip293_a_flyover_payment_counts_what_both_federations_are_sent` |
| BridgeUtilsTest.getAmountSentToAddresses_no_output_for_address | nothing sent: 0 | ported | `a_flyover_payment_of_nothing_to_the_federation_is_refused` |
| BridgeUtilsTest.getAmountSentToAddresses_output_value_is_0 | an output of 0: 0 | ported | `a_flyover_payment_of_nothing_to_the_federation_is_refused` |
| BridgeUtilsTest.testValidateFlyoverPeginValue_sent_zero_amount_before_RSKIP293 | nothing to the federation: UNPROCESSABLE_TX_VALUE_ZERO_ERROR | ported | `a_flyover_payment_of_nothing_to_the_federation_is_refused` (before and from RSKIP293) |
| BridgeUtilsTest.testValidateFlyoverPeginValue_sent_one_utxo_with_amount_below_minimum_before_RSKIP293 | before RSKIP293 a UTXO below the minimum is valid | ported | `before_rskip293_every_flyover_output_is_taken_whatever_its_value` |
| BridgeUtilsTest.testValidateFlyoverPeginValue_sent_one_utxo_with_amount_below_minimum_after_RSKIP293 | from RSKIP293 one UTXO below the minimum: UNPROCESSABLE_TX_UTXO_AMOUNT_SENT_BELOW_MINIMUM_ERROR | ported | `from_rskip293_each_flyover_utxo_must_be_worth_the_minimum` |
| BridgeUtilsTest.testValidateFlyoverPeginValue_funds_sent_equal_to_minimum_after_RSKIP293 | from RSKIP293 UTXOs of the minimum are valid | ported | `from_rskip293_each_flyover_utxo_must_be_worth_the_minimum` |
| BridgeUtilsTest.testValidateFlyoverPeginValue_funds_sent_above_minimum_after_RSKIP293 | from RSKIP293 UTXOs above it are valid | ported | `from_rskip293_each_flyover_utxo_must_be_worth_the_minimum` |
| BridgeUtilsTest.testGetUTXOsSentToAddresses_multiple_utxos_sent_to_random_address_and_one_utxo_sent_to_bech32_address_before_RSKIP293 | before RSKIP293 a native segwit output throws (ScriptException) | fixed | `before_rskip293_a_flyover_payment_with_an_output_that_is_no_address_is_a_generic_error`. RSKIP293. rskj reads every output as an address (`BridgeUtilsLegacy.getAmountSentToAddress`, BridgeUtilsLegacy.java:104-108; `getUTXOsSentToAddress`, :132-133), bitcoinj-thin's `Script.getToAddress` throws for one neither P2PKH nor P2SH (script/Script.java:320-329), and `Bridge.registerFlyoverBtcTransaction` answers GENERIC_ERROR (Bridge.java:1382-1385). rustock matched the flyover output's script and passed over the rest: it answered the amount, transferred it and registered the UTXOs. Before RSKIP293 it now reads every output as bitcoinj does |
| BridgeUtilsTest.testGetUTXOsSentToAddresses_multiple_utxo_sent_to_multiple_addresses_before_RSKIP293 | before RSKIP293 the UTXOs of the first address only, a 0-value one among them | ported | `before_rskip293_every_flyover_output_is_taken_whatever_its_value` |
| BridgeUtilsTest.testGetUTXOsSentToAddresses_no_utxo_sent_to_given_address_before_RSKIP293 | before RSKIP293 none for an address not paid | ported | `a_flyover_payment_of_nothing_to_the_federation_is_refused` (nothing registered) |
| BridgeUtilsTest.testGetUTXOsSentToAddresses_multiple_utxos_sent_to_random_address_and_one_utxo_sent_to_bech32_address_after_RSKIP293 | from RSKIP293 a native segwit output is passed over, the others kept by index | ported | `from_rskip293_each_federation_keeps_the_flyover_utxos_it_is_sent` |
| BridgeUtilsTest.testGetUTXOsSentToAddresses_multiple_utxo_sent_to_multiple_addresses_after_RSKIP293 | from RSKIP293 the UTXOs of each address given | ported | `from_rskip293_each_federation_keeps_the_flyover_utxos_it_is_sent` (each federation's UTXOs to its own set, BridgeSupport.java:2851-2892) |
| BridgeUtilsTest.testGetUTXOsSentToAddresses_no_utxo_sent_to_given_address_after_RSKIP293 | none for an address not paid (the test runs with RSKIP293 off) | ported | `a_flyover_payment_of_nothing_to_the_federation_is_refused` |
| BridgeUtilsTest.hasEnoughSignatures_one_signature | not on the list: a signature missing, not enough | ported | `a_pegout_missing_a_signature_on_any_input_has_not_enough_signatures` |
| BridgeUtilsTest.hasEnoughSignatures_no_signatures | not on the list: none, not enough | ported | `a_pegout_missing_a_signature_on_any_input_has_not_enough_signatures` |
| BridgeUtilsTest.hasEnoughSignatures_several_inputs_one_missing_signature | not on the list: one missing on each of three inputs, not enough | ported | `a_pegout_missing_a_signature_on_any_input_has_not_enough_signatures` (with the signed cases, `_two_signatures` and `_several_inputs_all_signed`) |
| BridgeUtilsTest.getCoinFromBigInteger_bigger_than_long_value | not on the list: a number past a Java long throws | ported | `a_locking_cap_past_a_java_long_fails_the_increase`, through `increaseLockingCap` (Bridge.java:1306) |

The rest of the class was read. `validateHeightAndConfirmations_*`, `calculateMerkleRoot_*` and
`validateInputsCount_*` are covered by `pegin_validations_answer_as_rskj_in_its_order` and
`a_transaction_without_inputs_is_what_rskj_counts` (bridge/peg.rs). `isFreeBridgeTx*` is the window
before `areBridgeTxsPaid`, which mainnet's early federator transactions reached; the free side is
`test_free_bridge_tx_executes_with_zero_gas` (executor.rs). `countMissingSignatures_*` feeds a log
line (BridgeSupport.java:1815). `isInputSignedByThisFederator_*` is what every peg-out's signing
does. `getAddressFromEth*`, `getFederation*Wallet*`, `testIsContractTx*`, the other
`hasEnoughSignatures_*` (all signed, for ERP and Flyover scripts), `getCoinFromBigInteger_null_value`
(Java's null) and `testCalculatePegoutTxSize_ZeroInput_ZeroOutput` (no caller passes nothing) name
no refusal a block could reach that the rows above leave untested.

## co.rsk.peg.BridgeUtilsLegacyTest

Module `bridge_utils_legacy_test.rs`. The legacy utilities are what rskj runs before RSKIP284,
RSKIP293 and RSKIP271; each throws `DeprecatedMethodCallException` when called after its rule,
which guards rskj's own callers and is no rule of the network's: rustock has one reader that
takes the era from the block.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| BridgeUtilsLegacyTest.deserializeBtcAddressWithVersionLegacy_after_rskip284 | the legacy reader throws after RSKIP284 | not applicable | a guard on rskj's callers: from RSKIP284 `BridgeUtils.deserializeBtcAddressWithVersion` reads the address itself (BridgeUtils.java:608-622), and so does rustock, by the block |
| BridgeUtilsLegacyTest.deserializeBtcAddressWithVersionLegacy_null_bytes | null throws | not applicable | an ABI `bytes` argument is never Java's null |
| BridgeUtilsLegacyTest.deserializeBtcAddressWithVersionLegacy_empty_bytes | before RSKIP284 no bytes throw | ported | `before_rskip284_a_flyover_address_is_refused_short_or_of_another_network` |
| BridgeUtilsLegacyTest.deserializeBtcAddressWithVersionLegacy_p2pkh_testnet | before RSKIP284 `6f` and 20 bytes read as version 111 and that hash | covered | `a_flyover_derivation_hash_reads_the_address_bitcoinj_read` (bridge/peg.rs), and `a_flyover_address_is_written_as_its_version_byte_and_hash` |
| BridgeUtilsLegacyTest.deserializeBtcAddressWithVersionLegacy_p2pkh_testnet_wrong_network | before RSKIP284 a testnet P2PKH version throws on mainnet | covered | `a_flyover_address_bitcoinj_refuses_is_a_generic_error` (bridge/peg.rs: mainnet #4,000,000, version `6f` answers GENERIC_ERROR) |
| BridgeUtilsLegacyTest.deserializeBtcAddressWithVersionLegacy_p2sh_testnet | before RSKIP284 `c4`, -60 as a signed byte, throws | covered | `a_flyover_address_bitcoinj_refuses_is_a_generic_error` (bridge/peg.rs) |
| BridgeUtilsLegacyTest.deserializeBtcAddressWithVersionLegacy_p2sh_testnet_wrong_network | before RSKIP284 `c4` throws on mainnet | ported | `before_rskip284_a_flyover_address_is_refused_short_or_of_another_network` |
| BridgeUtilsLegacyTest.deserializeBtcAddressWithVersionLegacy_p2pkh_mainnet | before RSKIP284 `00` and 20 bytes read as version 0 and that hash | ported | `a_flyover_address_is_written_as_its_version_byte_and_hash` (bridge_utils_test.rs) |
| BridgeUtilsLegacyTest.deserializeBtcAddressWithVersionLegacy_p2pkh_mainnet_wrong_network | before RSKIP284 a mainnet P2PKH version throws on testnet | covered | `a_flyover_address_bitcoinj_refuses_is_a_generic_error` (bridge/peg.rs: testnet #2,122,593, version 0 answers GENERIC_ERROR) |
| BridgeUtilsLegacyTest.deserializeBtcAddressWithVersionLegacy_p2sh_mainnet | before RSKIP284 `05` and 20 bytes read as version 5 and that hash | ported | `a_flyover_address_is_written_as_its_version_byte_and_hash` (bridge_utils_test.rs) |
| BridgeUtilsLegacyTest.deserializeBtcAddressWithVersionLegacy_p2sh_mainnet_wrong_network | before RSKIP284 a mainnet P2SH version throws on testnet | ported | `before_rskip284_a_flyover_address_is_refused_short_or_of_another_network` |
| BridgeUtilsLegacyTest.deserializeBtcAddressWithVersionLegacy_with_extra_bytes | before RSKIP284 the bytes after the 21st are ignored | covered | `a_flyover_address_bitcoinj_refuses_is_a_generic_error` (a 22nd byte ignored) and `a_flyover_derivation_hash_reads_the_address_bitcoinj_read` (the 21 bytes read), bridge/peg.rs |
| BridgeUtilsLegacyTest.deserializeBtcAddressWithVersionLegacy_invalid_address_hash | before RSKIP284 a hash of one byte throws | ported | `before_rskip284_a_flyover_address_is_refused_short_or_of_another_network` |
| BridgeUtilsLegacyTest.getAmountSentToAddress_after_RSKIP293 | the legacy amount throws after RSKIP293 | not applicable | a guard on rskj's callers: from RSKIP293 `BridgeUtils.getAmountSentToAddresses` counts by the wallet (BridgeUtils.java:158-163) |
| BridgeUtilsLegacyTest.getUTXOsSentToAddress_after_RSKIP293 | the legacy UTXOs throw after RSKIP293 | not applicable | a guard on rskj's callers (BridgeUtils.java:257-258) |
| BridgeUtilsLegacyTest.calculatePegoutTxSize_before_rskip_271 | before RSKIP271 the estimate of 2 inputs and 2 outputs for 13 members is within 2% of a real 2076-byte peg-out | ported | `before_rskip271_the_legacy_pegout_size_sets_releasebtcs_minimum`: the size exactly (2054), through the minimum `releaseBtc` asks at the last block before RSKIP271 and at its first; the 2% is also in `regular_pegout_tx_size_matches_rskj` (bridge/peg.rs) |
| BridgeUtilsLegacyTest.calculatePegoutTxSize_after_rskip_271 | the legacy sizer throws after RSKIP271 | not applicable | a guard on rskj's callers (BridgeUtils.java:647-649) |
| BridgeUtilsLegacyTest.calculatePegoutTxSize_ZeroInput_ZeroOutput | 0 inputs and 0 outputs throw | not applicable | no caller passes fewer than 2 inputs and 1 output (BridgeSupport.java:959, :1228, :2647) |

The rest of the class was read: `getAmountSentToAddress_ok`, `_no_outputs` and `_zero_amount`, and
`getUTXOsSentToAddress_*`, are the counting before RSKIP293 that `a_flyover_payment_of_nothing_to_the_federation_is_refused`
and `before_rskip293_every_flyover_output_is_taken_whatever_its_value` (bridge_utils_test.rs) hold.

## co.rsk.peg.federation.NonStandardErpFederationsTest

Module `non_standard_erp_federations_test.rs`. rskj builds a non-standard ERP federation (format
2000) anew with the activations of the block being run, and so does rustock
(`federation_redeem_for_format`): the ports ask for the redeem script at blocks of each era. Its set-up
turns RSKIP201 on, which every era below has.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| NonStandardErpFederationsTest.createFederation_postRSKIP293_withValidCsvValues_valid | from RSKIP293 delays of 130, 500, 33,000 and 65,535 are pushed as Java numbers, little-endian | ported | `a_delay_is_pushed_in_two_big_endian_bytes_before_rskip293_and_as_a_java_number_from_it` |
| NonStandardErpFederationsTest.createFederation_postRSKIP284_preRSKIP293_withValidCsvValueOneByteLong_valid | on testnet between RSKIP284 and RSKIP293 a delay of 20 is pushed as two big-endian bytes | ported | `a_delay_is_pushed_in_two_big_endian_bytes_before_rskip293_and_as_a_java_number_from_it` |
| NonStandardErpFederationsTest.createFederation_postRSKIP293_withInvalidCsvValues_throwsErpFederationCreationException | a delay of -100, 0, 65,536, 100,000 or 8,400,000 is refused | not applicable | the delay is each network's constant (52,560 on mainnet and testnet, 500 on regtest; `FederationMainNetConstants.java:62`, `FederationTestNetConstants.java:55`, `FederationRegTestConstants.java:52`), within 1 to 65,535; no transaction or block chooses it |
| NonStandardErpFederationsTest.of_postRSKIP293_withInvalidCsvValues_throwsErpFederationCreationException | the builder refuses those delays | not applicable | as the row above |
| NonStandardErpFederationsTest.createdRedeemScriptProgramFromNonStandardErpBuilderCsvUnsignedBE_withRealValues_equalsRealRedeemScriptProgram_mainnet | the builder before RSKIP293 gives a real federation's script | covered | `nonstandard_erp_redeem_scripts_match_rskjs_real_values` (bridge/peg.rs) |
| NonStandardErpFederationsTest.createdRedeemScriptProgramFromNonStandardErpBuilder_withRealValues_equalsRealRedeemScriptProgram_mainnet | the builder from RSKIP293 gives a real federation's script | covered | `nonstandard_erp_redeem_scripts_match_rskjs_real_values` (bridge/peg.rs) |
| NonStandardErpFederationsTest.createdFederationInfo_withRealValues_equalsExistingFederationInfo_testnet | the last non-standard ERP federation of testnet: its P2SH script and address 2MtxpJPt2xCa3AyFYUjTT7Aop9Z6gGf4rqA | ported | `the_last_nonstandard_erp_federation_of_testnet_has_its_real_address` |
| NonStandardErpFederationsTest.getNonStandardErpRedeemScript_compareOtherImplementation | the scripts of `redeemScripts.json`, written by another implementation, within 520 bytes | ported | `nonstandard_erp_redeem_scripts_are_those_another_implementation_writes`, with four of the file's 391 such vectors (the smallest, the largest, a two-byte and a three-byte delay); all 391 were run once against rustock and matched |
| NonStandardErpFederationsTest.getRedeemScript_before_RSKIP293 | on mainnet before RSKIP293 the script is laid out with a two-byte delay | ported | `a_nonstandard_erp_redeem_script_is_laid_out_as_rskjs_in_each_era` |
| NonStandardErpFederationsTest.getRedeemScript_after_RSKIP293 | on mainnet from RSKIP293 the delay is a Java number | ported | `a_nonstandard_erp_redeem_script_is_laid_out_as_rskjs_in_each_era` |
| NonStandardErpFederationsTest.getRedeemScript_changes_related_to_RSKIP293_testnet | on testnet the script changes at RSKIP293 | covered | `the_nonstandard_erp_builder_is_picked_as_rskjs_factory_picks_it` (bridge/peg.rs), and `a_nonstandard_erp_redeem_script_is_laid_out_as_rskjs_in_each_era` |
| NonStandardErpFederationsTest.createErpFederation_testnet_constants_before_RSKIP293 | testnet between RSKIP284 and RSKIP293: the layout | ported | `a_nonstandard_erp_redeem_script_is_laid_out_as_rskjs_in_each_era`, with testnet's own emergency keys (rskj's test keeps mainnet's) |
| NonStandardErpFederationsTest.createErpFederation_testnet_constants_after_RSKIP293 | testnet from RSKIP293: the layout | ported | `a_nonstandard_erp_redeem_script_is_laid_out_as_rskjs_in_each_era`, as the row above |
| NonStandardErpFederationsTest.createErpFederation_mainnet_constants_before_RSKIP293 | mainnet before RSKIP293: the layout | ported | `a_nonstandard_erp_redeem_script_is_laid_out_as_rskjs_in_each_era` |
| NonStandardErpFederationsTest.createErpFederation_mainnet_constants_after_RSKIP293 | mainnet from RSKIP293: the layout | ported | `a_nonstandard_erp_redeem_script_is_laid_out_as_rskjs_in_each_era` |
| NonStandardErpFederationsTest.getRedeemScript_before_RSKIP_284_testnet | on testnet before RSKIP284 the hard-coded script | covered | `the_nonstandard_erp_builder_is_picked_as_rskjs_factory_picks_it` and `nonstandard_erp_redeem_scripts_match_rskjs_real_values` (bridge/peg.rs) |
| NonStandardErpFederationsTest.getRedeemScript_before_RSKIP_284_mainnet | on mainnet before RSKIP284 not the hard-coded script; the layout | ported | `a_nonstandard_erp_redeem_script_is_laid_out_as_rskjs_in_each_era` (mainnet from RSKIP201) |
| NonStandardErpFederationsTest.getRedeemScript_after_RSKIP_284_testnet | on testnet between RSKIP284 and RSKIP293 the builder with the unsigned big-endian delay; the layout | ported | `a_nonstandard_erp_redeem_script_is_laid_out_as_rskjs_in_each_era` |
| NonStandardErpFederationsTest.getRedeemScript_after_RSKIP_201_before_RSKIP_293_mainnet | on mainnet from RSKIP201, before RSKIP284 and after it, no hard-coded script; the layout | ported | `a_nonstandard_erp_redeem_script_is_laid_out_as_rskjs_in_each_era` |
| NonStandardErpFederationsTest.testEquals_differentRedeemScript | on testnet the same members' federation before RSKIP284 and after it differ | ported | `a_nonstandard_erp_redeem_script_is_laid_out_as_rskjs_in_each_era` (the three testnet scripts differ; before RSKIP284 one script for any members) |
| NonStandardErpFederationsTest.createNonStandardErpFedWithSameRedeemScriptAsHardcodedOne_after_RSKIP293_fails | the hard-coded script cannot be built from RSKIP293 | not applicable | `@Disabled` in rskj: its delay, 5,295,360, is above the largest rskj takes, and no network's delay is |
| NonStandardErpFederationsTest.spendFromNonStandardErpFed_before_RSKIP293_testnet_using_erp_multisig_can_spend | before RSKIP293 the emergency keys spend with the input's sequence at testnet's delay | ported | `before_rskip293_bitcoin_reads_the_emergency_delay_little_endian_from_big_endian_bytes`: rustock runs no Bitcoin script, so the test reads the script's delay as Bitcoin's `OP_CHECKSEQUENCEVERIFY` does (52,560 is read as 20,685) against that sequence |
| NonStandardErpFederationsTest.spendFromNonStandardErpFed_before_RSKIP293_testnet_using_erp_multisig_cant_spend | before RSKIP293 with a delay of 300 they cannot (read as 11,265) | ported | `before_rskip293_bitcoin_reads_the_emergency_delay_little_endian_from_big_endian_bytes` |
| NonStandardErpFederationsTest.spendFromNonStandardErpFed_before_RSKIP293_testnet_using_standard_multisig_can_spend | the members spend | not applicable | Bitcoin's interpreter running the members' branch, which has no delay; rustock runs no Bitcoin script, and the branch's bytes are held by `a_nonstandard_erp_redeem_script_is_laid_out_as_rskjs_in_each_era` |
| NonStandardErpFederationsTest.spendFromNonStandardErpFed_before_RSKIP293_mainnet_using_erp_multisig_can_spend | as on testnet, mainnet's delay | ported | `before_rskip293_bitcoin_reads_the_emergency_delay_little_endian_from_big_endian_bytes` |
| NonStandardErpFederationsTest.spendFromNonStandardErpFed_before_RSKIP293_mainnet_using_erp_multisig_cant_spend | as on testnet, 300 | ported | `before_rskip293_bitcoin_reads_the_emergency_delay_little_endian_from_big_endian_bytes` |
| NonStandardErpFederationsTest.spendFromNonStandardErpFed_before_RSKIP293_mainnet_using_standard_multisig_can_spend | the members spend | not applicable | as for testnet's |
| NonStandardErpFederationsTest.spendFromNonStandardErpFed_after_RSKIP293_testnet_using_erp_multisig_can_spend | from RSKIP293 the emergency keys spend at the delay itself | ported | `before_rskip293_bitcoin_reads_the_emergency_delay_little_endian_from_big_endian_bytes` (read as 52,560) |
| NonStandardErpFederationsTest.spendFromNonStandardErpFed_after_RSKIP293_testnet_using_standard_multisig_can_spend | the members spend | not applicable | as for the members before RSKIP293 |
| NonStandardErpFederationsTest.spendFromNonStandardErpFed_after_RSKIP293_mainnet_using_erp_multisig_can_spend | as on testnet | ported | `before_rskip293_bitcoin_reads_the_emergency_delay_little_endian_from_big_endian_bytes` |
| NonStandardErpFederationsTest.spendFromNonStandardErpFed_after_RSKIP293_mainnet_using_standard_multisig_can_spend | the members spend | not applicable | as for the members before RSKIP293 |

The rest of the class was read. The refusals of no emergency keys, of thresholds of 0, below it or above
the members, and of uncompressed emergency keys are of values that are each network's constants or
`size / 2 + 1`. The refusal of a redeem script above 520 bytes is reached by committing a federation,
and its rows are `PendingFederationTest`'s. The getters, `testEquals_*` and `federationArgs_*` hold Java
objects.

## co.rsk.peg.NonStandardErpRedeemScriptBuilderFactoryTest

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| NonStandardErpRedeemScriptBuilderFactoryTest.preRSKIP284_returns_builderHardcoded_testnet | testnet before RSKIP284: the hard-coded builder | covered | `the_nonstandard_erp_builder_is_picked_as_rskjs_factory_picks_it` (bridge/peg.rs) |
| NonStandardErpRedeemScriptBuilderFactoryTest.preRSKIP284_doesnt_return_builderHardcoded_mainnet | mainnet before RSKIP284: not it | covered | `the_nonstandard_erp_builder_is_picked_as_rskjs_factory_picks_it` (bridge/peg.rs) |
| NonStandardErpRedeemScriptBuilderFactoryTest.postRSKIP284_preRSKIP293_returns_builderWithCsvUnsignedBE | testnet and mainnet between RSKIP284 and RSKIP293: the unsigned big-endian delay | covered | `the_nonstandard_erp_builder_is_picked_as_rskjs_factory_picks_it` (bridge/peg.rs; mainnet has both at hop400, and its builder before RSKIP293 is this one either side of RSKIP284) |
| NonStandardErpRedeemScriptBuilderFactoryTest.postRSKIP293_returns_builder | testnet and mainnet from RSKIP293: today's builder | covered | `the_nonstandard_erp_builder_is_picked_as_rskjs_factory_picks_it` (bridge/peg.rs) |

## co.rsk.peg.federation.FederationFactoryTest

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| FederationFactoryTest.differentNonStandardErpFederations_areNotEqualFeds_butHaveSameNonStandardFedFormat_testnet | testnet: the federations built at iris300, at hop400 without RSKIP293 and at hop400 are all format 2000 and differ | ported | `a_nonstandard_erp_redeem_script_is_laid_out_as_rskjs_in_each_era` (one format, three scripts) |
| FederationFactoryTest.differentNonStandardErpFederations_areNotEqualFeds_butHaveSameNonStandardFedFormat_mainnet | mainnet: the first two are the same federation, the third differs | ported | `a_nonstandard_erp_redeem_script_is_laid_out_as_rskjs_in_each_era` (the script at iris300 is the one before hop400, and differs from hop400's) |

The rest of `FederationFactoryTest` holds each builder's format version, which a federation is stored
with at its commit (`PendingFederationTest`'s rows).

## co.rsk.peg.federation.PendingFederationTest

Module `pending_federation_test.rs`. rskj builds the federation as a commit's deciding vote goes
through, so the ports vote as a majority of the network's federation-change authorizers do
(`createFederation`, `addFederatorPublicKeyMultikey` for each member, `commitFederation` with the hash
`getPendingFederationHash` answers), and hold the address the `commit_federation` event gives the
federation built to the one rskj's builder for the era gives it, computed in the test. rskj's tests run
each network's constants; the ports run mainnet's and testnet's.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| PendingFederationTest.buildFederation_with9Members_beforeRSKIP201Activation_shouldBuildMultiSigFed | before RSKIP201 nine members make a standard multisig | ported | `a_commit_builds_the_federation_of_its_era` |
| PendingFederationTest.buildFederation_with15Members_beforeRSKIP201Activation_shouldBuildMultiSigFed | before RSKIP201 fifteen members make one | ported | `a_commit_builds_the_federation_of_its_era` (20 members, and 21 refused, are `a_federation_bitcoin_could_not_spend_from_is_not_committed`, bridge/governance.rs) |
| PendingFederationTest.buildFederation_with9Members_afterRSKIP201Activation_beforeRSKIP353_shouldBuildNonStandardERPFed | from RSKIP201, before RSKIP353, a non-standard ERP federation | ported | `a_commit_builds_the_federation_of_its_era`, before RSKIP293 and from it on mainnet, from it on testnet |
| PendingFederationTest.buildFederation_with10Members_inMainnet_afterRSKIP201Activation_beforeRSKIP293_shouldThrowScriptCreationException | mainnet, iris300: ten members make a redeem script above 486 bytes, and the build throws | ported | `a_commit_throws_where_bitcoin_could_not_spend_from_the_federation_it_builds` (nine are committed) |
| PendingFederationTest.buildFederation_with11Members_inTestnet_afterRSKIP201Activation_beforeRSKIP353_shouldThrowScriptCreationException | testnet, hop400: eleven members throw | ported | `a_commit_throws_where_bitcoin_could_not_spend_from_the_federation_it_builds` (ten are committed) |
| PendingFederationTest.buildFederation_with10Members_inMainnet_afterRSKIP201Activation_beforeRSKIP353_shouldThrowScriptCreationException | mainnet, hop400: ten members throw | ported | `a_commit_throws_where_bitcoin_could_not_spend_from_the_federation_it_builds` |
| PendingFederationTest.buildFederation_with9Members_afterRSKIP353Activation_beforeRSKIP305_shouldBuildP2SHERPFed | from RSKIP353, before RSKIP305, a P2SH ERP federation | ported | `a_commit_builds_the_federation_of_its_era` |
| PendingFederationTest.buildFederation_with11Members_inTestnet_afterRSKIP353Activation_beforeRSKIP305_shouldThrowScriptCreationException | testnet: eleven members of a P2SH ERP federation throw | ported | `a_commit_throws_where_bitcoin_could_not_spend_from_the_federation_it_builds` |
| PendingFederationTest.buildFederation_with10Members_inMainnet_afterRSKIP353Activation_beforeRSKIP305_shouldThrowScriptCreationException | mainnet: ten members of a P2SH ERP federation throw | ported | `a_commit_throws_where_bitcoin_could_not_spend_from_the_federation_it_builds`; also `a_federation_bitcoin_could_not_spend_from_is_not_committed` (bridge/governance.rs) |
| PendingFederationTest.buildFederation_with102Members_afterRSKIP305_shouldThrowErpFederationCreationException | from RSKIP305, 102 members throw | ported | `a_commit_throws_where_bitcoin_could_not_spend_from_the_federation_it_builds` |
| PendingFederationTest.buildFederation_with9Members_afterRSKIP305Activation_shouldBuildP2SHP2WSHERPFed | from RSKIP305, a P2SH-P2WSH ERP federation | ported | `a_commit_builds_the_federation_of_its_era` |
| PendingFederationTest.buildFederation_with20Members_afterRSKIP305Activation_shouldBuildP2SHP2WSHErpFed | from RSKIP305, twenty members make one, their count written as a push | ported | `a_commit_builds_the_federation_of_its_era` |
| PendingFederationTest.buildFederation_with21Members_afterRSKIP305Activation_shouldThrowErpFederationCreationException | from RSKIP305, twenty-one members throw (bitcoinj's multisig takes 20 keys) | ported | `a_commit_throws_where_bitcoin_could_not_spend_from_the_federation_it_builds` |
| PendingFederationTest.buildFederation_withLessMembersThanRequired_shouldFailWithIncompleteFederationLog | not on the list: a pending federation of one member does not build | ported | `a_pending_federation_of_one_member_is_not_committed`: the commit answers INSUFFICIENT_MEMBERS before any build (FederationSupportImpl.java:666-669) |

The rest of the class was read: `isComplete_*` is the row above; `equals_*`, `toString_*`, `getHash` and
the serialization tests hold the Java object and its storage form, which `FederationStorageProviderImplTests`
and `BridgeSerializationUtilsTest` cover for the cells a block writes.

## co.rsk.peg.PegUtilsLegacyTest

Module `peg_utils_legacy_test.rs`. rskj applies these rules where `registerBtcTransaction` reads no
pegout index, so the ports register each transaction at Bitcoin height 349 of rskj's regtest, below
its index, at RSK block 1,000, all upgrades active or the named one and those after it moved later,
and read what the Bridge did: the call's success, the transaction marked processed, the events, the
active federation's UTXOs. A peg-out or migration is marked processed with no event; a transaction
no rule takes is left alone.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| PegUtilsLegacyTest.isValidPegInTx_less_than_minimum_not_pegin_after_iris | from RSKIP219 half the minimum is no peg-in | ported | `a_peg_in_must_send_the_minimum_of_its_era` |
| PegUtilsLegacyTest.isValidPegInTx_spending_from_federation_is_pegout_after_iris | a spend of the federation paying it is no peg-in | ported | `a_spend_of_the_federation_is_a_pegout` (taken as a peg-out) |
| PegUtilsLegacyTest.isValidPegInTx_sending_50_btc_after_iris | 50 BTC is a peg-in | ported | `a_peg_in_must_send_the_minimum_of_its_era` |
| PegUtilsLegacyTest.isValidPegInTx_value_between_old_and_new_before_iris | before RSKIP219 a value between the new and the legacy minimum is no peg-in | ported | `a_peg_in_must_send_the_minimum_of_its_era` |
| PegUtilsLegacyTest.isValidPegInTx_value_between_old_and_new_after_iris | from RSKIP219 it is | ported | `a_peg_in_must_send_the_minimum_of_its_era` |
| PegUtilsLegacyTest.isValidPegInTx_hasChangeUtxoFromFlyoverFederation_beforeRskip201_isPegin | before RSKIP201 the active federation's Flyover change, its redeem script as the scriptSig, is a peg-in | ported | `before_rskip201_the_change_of_a_flyover_or_erp_federation_is_a_peg_in`: through the Bridge the peg-in's sender cannot be read from that scriptSig, and the call fails (P2shMultisigBtcLockSender.java:52-54), where a peg-out would be marked processed |
| PegUtilsLegacyTest.isValidPegInTx_hasChangeUtxoFromFlyoverFederation_afterRskip201_notPegin | from RSKIP201 it is no peg-in | ported | `from_rskip201_the_change_of_a_flyover_or_erp_federation_is_no_peg_in` (a peg-out) |
| PegUtilsLegacyTest.isValidPegInTx_hasChangeUtxoFromFlyoverErpFederation_beforeRskip201_isPegin | as the Flyover one, behind a non-standard ERP script too | ported | `before_rskip201_the_change_of_a_flyover_or_erp_federation_is_a_peg_in` |
| PegUtilsLegacyTest.isValidPegInTx_hasChangeUtxoFromFlyoverErpFederation_afterRskip201_notPegin | from RSKIP201 no peg-in | ported | `from_rskip201_the_change_of_a_flyover_or_erp_federation_is_no_peg_in` |
| PegUtilsLegacyTest.isValidPegInTx_hasChangeUtxoFromErpFederation_beforeRskip201_isPegin | a non-standard ERP change before RSKIP201 is a peg-in | ported | `before_rskip201_the_change_of_a_flyover_or_erp_federation_is_a_peg_in` |
| PegUtilsLegacyTest.isValidPegInTx_hasChangeUtxoFromErpFederation_afterRskip201_notPegin | from RSKIP201 no peg-in | ported | `from_rskip201_the_change_of_a_flyover_or_erp_federation_is_no_peg_in` |
| PegUtilsLegacyTest.isValidPegInTx_hasChangeUtxoFromFlyoverRetiredFederation_beforeRskip201_isPegin | before RSKIP201 a signed spend of the retired federation behind a Flyover prefix is a peg-in | not applicable | through the Bridge there is no last retired federation before RSKIP201: rskj writes its script at a handover from RSKIP186 (`handoverToNewFederation`), which activates with RSKIP201 at iris300 |
| PegUtilsLegacyTest.isValidPegInTx_hasChangeUtxoFromFlyoverRetiredFederation_afterRskip201_notPegin | from RSKIP201 no peg-in | ported | `from_rskip201_the_change_of_a_flyover_or_erp_federation_is_no_peg_in` (paying the active federation, a migration) |
| PegUtilsLegacyTest.isValidPegInTx_hasChangeUtxoFromFlyoverErpRetiredFederation_beforeRskip201_isPegin | as above, behind a non-standard ERP script too | not applicable | as the retired Flyover row before RSKIP201 |
| PegUtilsLegacyTest.isValidPegInTx_hasChangeUtxoFromFlyoverErpRetiredFederation_afterRskip201_notPegin | from RSKIP201 no peg-in | ported | `from_rskip201_the_change_of_a_flyover_or_erp_federation_is_no_peg_in` |
| PegUtilsLegacyTest.isValidPegInTx_hasChangeUtxoFromErpRetiredFederation_beforeRskip201_isPegin | a retired non-standard ERP spend before RSKIP201 is a peg-in | not applicable | as the retired Flyover row before RSKIP201 |
| PegUtilsLegacyTest.isValidPegInTx_hasChangeUtxoFromErpRetiredFederation_afterRskip201_notPegin | from RSKIP201 no peg-in | ported | `from_rskip201_the_change_of_a_flyover_or_erp_federation_is_no_peg_in` |
| PegUtilsLegacyTest.isValidPegInTx_has_multiple_utxos_below_minimum_but_total_amount_is_ok_before_RSKIP293 | before RSKIP293 four quarters of the minimum make a peg-in | ported | `from_rskip293_each_utxo_of_a_peg_in_must_be_worth_the_minimum` |
| PegUtilsLegacyTest.isValidPegInTx_has_utxos_below_minimum_and_total_amount_as_well_before_RSKIP293 | before RSKIP293 a total below it makes none | ported | `from_rskip293_each_utxo_of_a_peg_in_must_be_worth_the_minimum` |
| PegUtilsLegacyTest.isValidPegInTx_has_utxos_below_minimum_after_RSKIP293 | from RSKIP293 one UTXO below it makes none, whatever the others | ported | `from_rskip293_each_utxo_of_a_peg_in_must_be_worth_the_minimum` |
| PegUtilsLegacyTest.isValidPegInTx_utxo_equal_to_minimum_after_RSKIP293 | from RSKIP293 one of the minimum makes one | ported | `from_rskip293_each_utxo_of_a_peg_in_must_be_worth_the_minimum` |
| PegUtilsLegacyTest.isValidPegInTx_p2shErpScript_sends_funds_to_federation_address_after_RSKIP353 | a spend of a P2SH ERP script of the federation's members paying the federation is no peg-in | ported | `a_spend_of_a_p2sh_erp_script_of_the_federations_members_is_a_pegout` |
| PegUtilsLegacyTest.isValidPegInTx_p2shErpScript_sends_funds_to_random_address_after_RSKIP353 | paying elsewhere, no peg-in | ported | `a_spend_of_a_p2sh_erp_script_of_the_federations_members_is_a_pegout` |
| PegUtilsLegacyTest.isValidPegInTx_flyoverpP2shErpScript_sends_funds_to_federation_address_after_RSKIP353 | its Flyover, paying the federation, no peg-in | ported | `a_spend_of_a_p2sh_erp_script_of_the_federations_members_is_a_pegout` |
| PegUtilsLegacyTest.isValidPegInTx_flyoverP2shErpScript_sends_funds_to_random_address_after_RSKIP353 | its Flyover, paying elsewhere, no peg-in | ported | `a_spend_of_a_p2sh_erp_script_of_the_federations_members_is_a_pegout` |
| PegUtilsLegacyTest.testTxIsProcessableInLegacyVersion | before RSKIP143 a legacy peg-in only from P2PKH; from it P2SH-P2WPKH, P2SH multisig and P2SH-P2WSH too | ported | `before_rskip143_a_peg_in_is_taken_only_from_a_p2pkh_sender` (P2SH multisig; P2PKH taken before it in `a_peg_in_must_send_the_minimum_of_its_era`). The segwit senders are not ported: the test chain's merkle root is the hash of the whole serialization, not what rskj hashes for a witness transaction before RSKIP143 |
| PegUtilsLegacyTest.isMigrationTx_sending_funds_from_retired_p2sh_fed_to_active_p2sh_fed | a retired P2SH ERP federation's spend to the active one is a migration when known by its standard P2SH script, not by its whole one | ported | `a_spend_of_a_retiring_or_retired_federation_to_the_active_one_is_a_migration` |
| PegUtilsLegacyTest.isMigrationTx_sending_funds_from_retiring_p2sh_fed_to_active_p2sh_fed | the retiring P2SH ERP federation's, a migration | ported | `a_spend_of_a_retiring_or_retired_federation_to_the_active_one_is_a_migration` |
| PegUtilsLegacyTest.isMigrationTx_sending_funds_from_retired_standard_fed_to_active_p2sh_fed | a retired standard federation's to a P2SH ERP one | ported | `a_spend_of_a_retiring_or_retired_federation_to_the_active_one_is_a_migration` |
| PegUtilsLegacyTest.isMigrationTx_sending_funds_from_retiring_standard_fed_to_active_p2sh_fed | the retiring standard one's | ported | `a_spend_of_a_retiring_or_retired_federation_to_the_active_one_is_a_migration` |
| PegUtilsLegacyTest.isMigrationTx_sending_funds_from_retired_standard_fed_to_active_standard_fed | retired standard to active standard | ported | `a_spend_of_a_retiring_or_retired_federation_to_the_active_one_is_a_migration` |
| PegUtilsLegacyTest.isMigrationTx_sending_funds_from_retiring_standard_fed_to_active_standard_fed | retiring standard to active standard | ported | `a_spend_of_a_retiring_or_retired_federation_to_the_active_one_is_a_migration` |
| PegUtilsLegacyTest.isPegOutTx_fromFlyoverFederation | a spend of the federation's redeem script behind a Flyover prefix, paying elsewhere, is no peg-out before RSKIP201 and is one from it | fixed | `before_rskip201_a_spend_of_a_wrapped_federation_redeem_script_is_no_pegout`; from RSKIP201 `from_rskip201_a_spend_of_a_wrapped_federation_redeem_script_is_a_pegout`. RSKIP201. Before it rskj's `isPegOutTx` hashes the input's redeem script as it is (PegUtilsLegacy.java:58-80; the standard script is extracted only from RSKIP201, :67-71), and `isValidPegInTx` counts only a correct spend of a federation's P2SH script (:186-194): the transaction is neither, and is left alone. rustock reduced each input to the federation's standard redeem script at every height and marked the transaction processed as a peg-out, registering its outputs to the federation; it now does so only from RSKIP201. Blocks before iris300: mainnet below #3,614,800, testnet below #2,060,500 |
| PegUtilsLegacyTest.isPegOutTx_fromErpFederation | the same within a non-standard ERP script | fixed | as the row above |
| PegUtilsLegacyTest.isPegOutTx_fromFlyoverErpFederation | the same behind both | fixed | as the row above |

The rest of the class was read: `isValidPegInTx` and `isValidPegInTxForTwoFederations` (the minimum and a
federation's own spend, ported above), `txIsFromOldFederation*` (covered by `mainnet_old_federation_address_hash160`,
bridge/peg.rs, and the migration branch of `register_btc_transaction`), the other `isPegOutTx_*` (a standard
or P2SH-P2WSH federation's own spend, which every peg-out exercises), and the `fromP2shErpScriptSender` cases
before RSKIP353 (a P2SH ERP script before any federation had one).
