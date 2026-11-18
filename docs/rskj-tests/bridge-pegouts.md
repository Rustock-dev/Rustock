# rskj's Bridge storage, peg-out, signature and event tests, and rustock's

One row per rskj test of the classes below that names an upgrade or a rule switch, and per
test ported from the rest of those classes, with one of four verdicts: **covered** (the rustock
test that asserts the same behaviour), **ported** (the rustock test written for it), **not
applicable** (why rustock needs no such test), or **fixed** (rustock did otherwise; the rule,
rskj's place and what rustock did are named under the class, rustock is put right, and the port
passes).

rskj at VETIVER-9.0.4. rskj's tests are under `rskj-core/src/test/java/co/rsk/peg/`, its code
under `rskj-core/src/main/java/co/rsk/peg/`. The ports are in
`crates/execution/src/rskj_tests/bridge_pegouts/`, one module per rskj class; a rustock test
named without a path is there. rskj's tests hand a mocked repository and a mocked activation
config to the class under test; the ports run rustock's Bridge in a context with no trie, at a
block of mainnet's schedule or of rskj's regtest, or under a ladder of upgrades that ends at the
one a test names, and read the cells it leaves.

| class | rows | covered | ported | not applicable | fixed |
|---|---:|---:|---:|---:|---:|
| `BridgeStorageProviderTest` | 52 | 12 | 34 | 5 | 1 |
| `BridgeSupportRegisterBtcTransactionTest` | 32 | 0 | 32 | 0 | 0 |
| `PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest` | 22 | 0 | 20 | 2 | 0 |
| `PegUtilsGetTransactionTypeTest` | 12 | 0 | 12 | 0 | 0 |
| `PegUtilsEvaluatePeginTest` | 1 | 0 | 0 | 1 | 0 |
| `PegUtilsTest` | 1 | 0 | 0 | 1 | 0 |
| `PegUtilsLegacyGetTransactionTypeTest` | 1 | 0 | 1 | 0 | 0 |
| `utils.BridgeEventLoggerLegacyImplTest` | 9 | 0 | 8 | 1 | 0 |
| `utils.BridgeEventLoggerImplTest` | 6 | 0 | 6 | 0 | 0 |
| `BridgeSupportAddSignatureTest` | 6 | 0 | 6 | 0 | 0 |
| `BridgeSupportPegoutTransactionCreatedEventTest` | 3 | 0 | 3 | 0 | 0 |
| `BridgeSupportSigHashTest` | 3 | 0 | 3 | 0 | 0 |
| `BridgeSupportProcessFundsMigrationTest` | 3 | 0 | 3 | 0 | 0 |
| `ReleaseTransactionBuilderTest` | 4 | 0 | 4 | 0 | 0 |
| `BridgeStorageProviderPegoutTxIndexTests` | 4 | 0 | 2 | 2 | 0 |
| `bitcoin.BitcoinUtilsLegacyTest` | 2 | 0 | 2 | 0 | 0 |
| `PegoutsWaitingForConfirmationsTest` | 1 | 0 | 0 | 1 | 0 |

The listed tests ported after the rest are under "Listed tests ported later" at the end.

## BridgeStorageProviderTest

Module `bridge_storage_provider_test.rs`. `bridge_test.rs` =
`crates/execution/src/rskj_tests/bridge_methods/bridge_test.rs`. The last two rows are tests
from the rest of the class.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| BridgeStorageProviderTest.getReleaseRequestQueue_before_rskip_146_activation | before RSKIP146 the queue is `releaseRequestQueue` alone | covered | `release_request_queue_concatenates_legacy_then_with_tx_hash` (crates/execution/src/bridge/peg.rs) |
| BridgeStorageProviderTest.getReleaseRequestQueue_after_rskip_146_activation | from RSKIP146 the queue is the entries of `releaseRequestQueue`, then those of `releaseRequestQueueWithTxHash` | covered | `release_request_queue_concatenates_legacy_then_with_tx_hash` (peg.rs); also `from_rskip146_a_peg_out_request_is_queued_with_its_hash_after_the_legacy_ones` |
| BridgeStorageProviderTest.saveReleaseRequestQueue_before_rskip_146_activation | before RSKIP146 the queue is written to `releaseRequestQueue` alone | ported | `before_rskip146_a_peg_out_request_is_written_to_the_legacy_queue_alone` |
| BridgeStorageProviderTest.saveReleaseRequestQueue_after_rskip_146_activation | from RSKIP146 the entries with a hash go to `releaseRequestQueueWithTxHash` and `releaseRequestQueue` is written back too | fixed | `from_rskip146_a_peg_out_request_writes_the_legacy_queue_back_too`, and `from_rskip146_a_peg_out_request_is_queued_with_its_hash_after_the_legacy_ones` |
| BridgeStorageProviderTest.getPegoutsWaitingForConfirmations_before_rskip_146_activation | before RSKIP146 the set is `releaseTransactionSet` alone | ported | `before_rskip146_the_pegouts_waiting_for_confirmations_are_the_legacy_cell_alone` |
| BridgeStorageProviderTest.getPegoutsWaitingForConfirmations_after_rskip_146_activation | from RSKIP146 the set is what both cells hold | ported | `from_rskip146_the_pegouts_waiting_for_confirmations_span_both_cells` |
| BridgeStorageProviderTest.savePegoutsWaitingForConfirmations_before_rskip_146_activations | before RSKIP146 the set is written to `releaseTransactionSet` alone | ported | `before_rskip146_the_pegouts_waiting_for_confirmations_are_the_legacy_cell_alone` |
| BridgeStorageProviderTest.savePegoutsWaitingForConfirmations_after_rskip_146_activations | from RSKIP146 the set is written split by whether an entry carries its transaction's hash | ported | `from_rskip146_the_pegouts_waiting_for_confirmations_span_both_cells` |
| BridgeStorageProviderTest.getReleaseTransaction_after_rskip_146_activations | three peg-outs saved with their hashes read back as three, none waiting for signatures | ported | `from_rskip146_saved_pegouts_read_back_and_none_waits_for_signatures` |
| BridgeStorageProviderTest.getHeightIfBtcTxhashIsAlreadyProcessed_before_RSKIP134_does_not_use_new_storage | before RSKIP134 a processed hash is read from the map `btcTxHashesAP` | ported | `before_rskip134_a_processed_peg_in_is_kept_in_the_map_of_them_all`. rustock's read also looks in the hash's own cell, without the switch; before RSKIP134 nothing writes such a cell, so on any chain it finds nothing there |
| BridgeStorageProviderTest.getHeightIfBtcTxhashIsAlreadyProcessed_after_RSKIP134_uses_new_storage | from RSKIP134 a hash is read from the map, else from its own cell | ported | `from_rskip134_a_processed_peg_in_is_read_from_the_map_or_its_own_cell` |
| BridgeStorageProviderTest.setHeightBtcTxhashAlreadyProcessed_before_RSKIP134_does_not_use_new_storage | before RSKIP134 a processed hash goes into the map | ported | `before_rskip134_a_processed_peg_in_is_kept_in_the_map_of_them_all` |
| BridgeStorageProviderTest.setHeightBtcTxhashAlreadyProcessed_before_RSKIP134_uses_new_storage | with every upgrade active a processed hash gets a cell of its own, holding its height, and the map is not written | ported | `from_rskip134_a_processed_peg_in_is_kept_in_a_cell_of_its_own` |
| BridgeStorageProviderTest.getCoinBaseInformation_before_RSKIP143 | before RSKIP143 no coinbase information is read | ported | `from_rskip143_a_blocks_coinbase_information_proves_a_peg_in` |
| BridgeStorageProviderTest.getCoinBaseInformation_after_RSKIP143 | from RSKIP143 the stored coinbase information is read | ported | `from_rskip143_a_blocks_coinbase_information_proves_a_peg_in` |
| BridgeStorageProviderTest.setCoinBaseInformation_before_RSKIP143 | before RSKIP143 coinbase information is not kept | covered | `register_btc_coinbase_transaction_is_closed_before_papyrus200` (bridge_test.rs): the one method that writes it is closed before RSKIP143 |
| BridgeStorageProviderTest.setCoinBaseInformation_after_RSKIP143 | from RSKIP143 coinbase information set is read back | ported | `a_blocks_coinbase_information_is_stored_as_rskj_serializes_it` |
| BridgeStorageProviderTest.saveCoinBaseInformation_before_RSKIP143 | before RSKIP143 coinbase information is never written | covered | `register_btc_coinbase_transaction_is_closed_before_papyrus200` (bridge_test.rs) |
| BridgeStorageProviderTest.saveCoinBaseInformation_after_RSKIP143 | from RSKIP143 it is written under `coinbaseInformation-<block hash>` as `serializeCoinbaseInformation` writes it | ported | `a_blocks_coinbase_information_is_stored_as_rskj_serializes_it` |
| BridgeStorageProviderTest.getBtcBestBlockHashByHeight_beforeRskip199 | before RSKIP199 the height index is not read | ported | `from_rskip199_the_main_chain_block_at_a_height_is_the_one_its_index_names` |
| BridgeStorageProviderTest.getBtcBestBlockHashByHeight_afterRskip199_hashNotFound | from RSKIP199 a height the index doesn't hold answers no hash | ported | `from_rskip199_the_main_chain_block_at_a_height_is_the_one_its_index_names` (the chain is walked instead) |
| BridgeStorageProviderTest.getBtcBestBlockHashByHeight_afterRskip199 | from RSKIP199 the hash at a height is read from `btcBlockHeight-<height>` | ported | `from_rskip199_the_main_chain_block_at_a_height_is_the_one_its_index_names` |
| BridgeStorageProviderTest.saveBtcBlocksIndex_beforeRskip199 | before RSKIP199 nothing is indexed | ported | `from_rskip199_each_block_the_chain_takes_is_indexed_by_its_height`; also `seeding_heads_the_store_with_its_genesis_block_then_the_checkpoint` (crates/execution/src/bridge/btc_chain.rs) |
| BridgeStorageProviderTest.saveBtcBlocksIndex_afterRskip199 | from RSKIP199 `btcBlockHeight-<height>` holds `serializeSha256Hash` of the block's hash | ported | `from_rskip199_each_block_the_chain_takes_is_indexed_by_its_height` |
| BridgeStorageProviderTest.isFlyoverFederationDerivationHashUsed_afterRSKIP176_returnTrue | a cell holding the one byte 1 marks the derivation hash used | ported | `a_registered_flyover_payment_is_marked_with_one_true_byte` |
| BridgeStorageProviderTest.isFlyoverFederationDerivationHashUsed_beforeRSKIP176_returnFalse | before RSKIP176 nothing is marked used | covered | `register_flyover_btc_transaction_is_closed_before_iris300` (bridge_test.rs): the Flyover method, the mark's only reader and writer, is closed before RSKIP176 |
| BridgeStorageProviderTest.isFlyoverFederationDerivationHashUsed_storageReturnsNull_returnFalse | an empty cell is no mark | ported | `a_flyover_mark_of_anything_but_one_true_byte_is_no_mark` |
| BridgeStorageProviderTest.isFlyoverFederationDerivationHashUsed_storageReturnsEmpty_returnFalse | no bytes are no mark | ported | `a_flyover_mark_of_anything_but_one_true_byte_is_no_mark` (a cell can't hold no bytes: writing none clears it) |
| BridgeStorageProviderTest.isFlyoverFederationDerivationHashUsed_storageReturnsWrongValue_returnFalse | a byte other than 1 is no mark | ported | `a_flyover_mark_of_anything_but_one_true_byte_is_no_mark` |
| BridgeStorageProviderTest.saveDerivationArgumentsScriptHash_afterRSKIP176_ok | marking writes `[1]` under `fastBridgeHashUsedInBtcTx-<btc tx hash><derivation hash>` | ported | `a_registered_flyover_payment_is_marked_with_one_true_byte` |
| BridgeStorageProviderTest.saveDerivationArgumentsScriptHash_afterRSKIP176_nullBtcTxHash_notSaved | a null transaction hash marks nothing | not applicable | a Java null; rustock's hashes are 32-byte arrays, always there |
| BridgeStorageProviderTest.saveDerivationArgumentsScriptHash_afterRSKIP176_nullDerivationHash_notSaved | a null derivation hash marks nothing | not applicable | a Java null, as above |
| BridgeStorageProviderTest.saveDerivationArgumentsScriptHash_beforeRSKIP176_ok | before RSKIP176 no mark is written | covered | `register_flyover_btc_transaction_is_closed_before_iris300` (bridge_test.rs) |
| BridgeStorageProviderTest.getFlyoverFederationInformation_afterRSKIP176_ok | the information stored under a Flyover redeem script's hash is read | ported | `a_pegout_spends_a_flyover_utxo_with_the_redeem_script_its_information_names` |
| BridgeStorageProviderTest.getFlyoverFederationInformation_beforeRSKIP176_ok | before RSKIP176 no information is read | covered | `register_flyover_btc_transaction_is_closed_before_iris300` (bridge_test.rs): nothing writes the information before RSKIP176, so rustock's read, which has no switch, finds none |
| BridgeStorageProviderTest.getFlyoverFederationInformation_notFound | a hash with nothing stored has no information | ported | `a_pegout_spends_a_flyover_utxo_with_the_redeem_script_its_information_names` (the federation's own UTXO) |
| BridgeStorageProviderTest.getFlyoverFederationInformation_nullParameter_returnEmpty | a null hash has no information | not applicable | a Java null; rustock reads by the 20-byte hash of a P2SH output |
| BridgeStorageProviderTest.getFlyoverFederationInformation_arrayEmpty_returnEmpty | an empty hash has no information | not applicable | rustock reads only by the 20-byte hash taken from a P2SH output |
| BridgeStorageProviderTest.saveFlyoverFederationInformation_afterRSKIP176_ok | the information is written under `fastBridgeFederationInformation-<hash>` as `serializeFlyoverFederationInformation` writes it | covered | `a_payment_is_registered_and_its_value_goes_to_the_lbc` (crates/execution/src/rskj_tests/bridge_methods/bridge_support_flyover_test.rs) asserts the cell's bytes |
| BridgeStorageProviderTest.saveFlyoverFederationInformation_beforeRSKIP176_ok | before RSKIP176 no information is written | covered | `register_flyover_btc_transaction_is_closed_before_iris300` (bridge_test.rs) |
| BridgeStorageProviderTest.saveFlyoverFederationInformation_alreadySet_dont_set_again | the same information set twice is written once | not applicable | counts the mock's writes; rustock writes the cell as it goes, and it holds the same bytes however often they are written |
| BridgeStorageProviderTest.getReceiveHeadersLastTimestamp_before_RSKIP200 | before RSKIP200 no time is read | covered | `receive_header_is_closed_before_iris300` (bridge_test.rs): `receiveHeader`, the time's only reader and writer, is closed before RSKIP200 |
| BridgeStorageProviderTest.getReceiveHeadersLastTimestamp_after_RSKIP200 | from RSKIP200 the time is read | ported | `from_rskip200_receive_header_keeps_and_reads_the_time_of_the_last_header` |
| BridgeStorageProviderTest.saveReceiveHeadersLastTimestamp_before_RSKIP200 | before RSKIP200 no time is written | covered | `receive_header_is_closed_before_iris300` (bridge_test.rs) |
| BridgeStorageProviderTest.saveReceiveHeadersLastTimestamp_after_RSKIP200 | from RSKIP200 the time is written as `serializeLong` | ported | `from_rskip200_receive_header_keeps_and_reads_the_time_of_the_last_header` |
| BridgeStorageProviderTest.getNextPegoutHeight_before_RSKIP271_activation | before RSKIP271 no next height is read | ported | `before_rskip271_the_next_pegout_height_is_neither_read_nor_written` |
| BridgeStorageProviderTest.getNextPegoutHeight_after_RSKIP271_activation | from RSKIP271 the stored height is read | covered | `the_next_pegout_creation_block_number_is_read_from_hop400` (bridge_test.rs) |
| BridgeStorageProviderTest.setNextPegoutHeightAndGetNextPegoutHeight_after_RSKIP271_activation | a height set is read back | ported | `from_rskip271_the_next_pegout_height_is_written_and_read_back` |
| BridgeStorageProviderTest.saveNextPegoutHeight_before_RSKIP271 | before RSKIP271 the height is never written | ported | `before_rskip271_the_next_pegout_height_is_neither_read_nor_written`, `from_rskip271_the_next_pegout_height_is_written_and_read_back` |
| BridgeStorageProviderTest.saveNextPegoutHeight_after_RSKIP271 | from RSKIP271 the height is written as `serializeLong` | ported | `from_rskip271_the_next_pegout_height_is_written_and_read_back` |
| BridgeStorageProviderTest.getReceiveHeadersLastTimestamp_not_in_repository | with no time stored none is read | ported | `from_rskip200_receive_header_keeps_and_reads_the_time_of_the_last_header` (the first header is taken) |
| BridgeStorageProviderTest.saveReceiveHeadersLastTimestamp_not_set | no time is written where none was set | ported | `from_rskip200_receive_header_keeps_and_reads_the_time_of_the_last_header` (`receiveHeaders` writes none; a header refused as too soon leaves it) |

Where rustock differed, now put right:

- **RSKIP146, `saveReleaseRequestQueue_after_rskip_146_activation`.** rskj saves the release
  request queue at the end of every Bridge call that loaded it (`Bridge.execute` and `teardown`,
  `Bridge.java` 415 and 489-491; `BridgeSupport.save`, `BridgeSupport.java` 186-193), and from
  RSKIP146 writes both halves (`BridgeStorageProvider.saveReleaseRequestQueue`,
  `BridgeStorageProvider.java` 197-207): the entries without a hash to `releaseRequestQueue`, the
  empty list `0xc0` where there are none, and those with one to `releaseRequestQueueWithTxHash`.
  `releaseBtc` loads the queue to add its request (`BridgeSupport.java` 1008-1012). rustock's
  `release_btc` wrote `releaseRequestQueueWithTxHash` alone, so the states differed where
  `releaseRequestQueue` had never been written: on a new chain, a peg-out request before the first
  `updateCollections`. The calls that load the queue to read it, `getQueuedPegoutsCount`
  (`BridgeStorageProvider.java` 550-552) and `getEstimatedFeesForNextPegOutEvent`
  (`BridgeSupport.java` 2672-2676), write it back the same way in rskj, and didn't in rustock.
  rustock now writes the queue back, both cells, after every method that read it. The set of
  peg-outs waiting for confirmations, saved the same way, matched already: rustock writes both of
  its cells.

## BridgeSupportRegisterBtcTransactionTest

Module `bridge_support_register_btc_transaction_test.rs`. rskj's tests run under
`fingerroot500` (before RSKIP379), `arrowhead600` with the Bitcoin block below the pegout
index's start and at it, and `all`; the class's set-up names RSKIP379 through
`common_args` and `activationsAndShouldUsePegoutIndexArgs`. The ports run mainnet's Bridge
at a block of each of those upgrades and of lovell700, where RSKIP459 marks a peg-in it
can't refund processed until RSKIP551, at Bitcoin height 841,908 and 841,909 (mainnet's
index start, 837,589 and its grace of 4,320). Where rskj's test mocks the peg-in's sender
out of a multisig spend, the port's input is one no sender rskj knows reads. The last six
rows are tests from the rest of the class.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| BridgeSupportRegisterBtcTransactionTest.pegin_legacy_to_active_fed | a bitcoin to the active federation from a P2PKH sender is registered in every run | ported | `a_legacy_peg_in_to_the_active_federation_is_taken` |
| BridgeSupportRegisterBtcTransactionTest.pegin_multiple_outputs_to_active_fed | ten outputs of the minimum are one peg-in and ten UTXOs | ported | `a_peg_in_of_many_outputs_to_the_active_federation_is_taken_whole` |
| BridgeSupportRegisterBtcTransactionTest.pegin_to_active_fed_with_bech32_output | an output to a bech32 address is not counted | ported | `a_peg_in_with_an_output_to_a_bech32_address_counts_the_federations_alone` |
| BridgeSupportRegisterBtcTransactionTest.pegin_to_active_fed_equal_to_minimum_with_other_random_outputs_below_minimum | the minimum to the federation is a peg-in whatever goes elsewhere | ported | `a_peg_in_of_the_minimum_is_taken_beside_smaller_outputs_elsewhere` |
| BridgeSupportRegisterBtcTransactionTest.pegin_to_active_fed_below_minimum | below the minimum: no peg-in by the legacy rules; INVALID_AMOUNT by the index, processed per RSKIP459 and RSKIP551 | ported | `a_peg_in_with_an_output_below_the_minimum_is_refused` |
| BridgeSupportRegisterBtcTransactionTest.pegin_to_active_fed_below_and_above_minimum | one output below the minimum refuses the peg-in, as above | ported | `a_peg_in_with_an_output_below_the_minimum_is_refused` |
| BridgeSupportRegisterBtcTransactionTest.pegin_multiple_outputs_to_active_fed_sum_amount_equal_to_minimum_pegin | outputs each below the minimum refuse the peg-in though they add up to it | ported | `a_peg_in_with_an_output_below_the_minimum_is_refused` |
| BridgeSupportRegisterBtcTransactionTest.pegin_to_active_fed_below_minimum_and_retiring_above_minimum | the minimum to the retiring federation doesn't make up for too little to the active one (RSKIP459) | ported | `a_peg_in_below_the_minimum_to_the_active_federation_is_refused_whatever_the_retiring_one_gets` |
| BridgeSupportRegisterBtcTransactionTest.pegin_v1_to_active_fed_with_invalid_payload_and_unknown_sender_cannot_be_processed | an empty v1 payload with no sender: PEGIN_V1_INVALID_PAYLOAD, not refundable; before RSKIP379 processed, from it per RSKIP459 and RSKIP551 | ported | `a_peg_in_v1_with_an_empty_payload_and_no_sender_is_not_refundable` |
| BridgeSupportRegisterBtcTransactionTest.pegin_to_retiring_fed_cannot_be_processed | a peg-in to the retiring federation from an unknown sender is not refundable | ported | `a_peg_in_to_the_retiring_federation_from_an_unknown_sender_is_not_refundable` |
| BridgeSupportRegisterBtcTransactionTest.pegin_legacy_from_segwit_to_active_fed_cannot_be_processed | a legacy peg-in from a segwit sender is not refundable | ported | `a_legacy_peg_in_from_a_segwit_sender_is_not_refundable` |
| BridgeSupportRegisterBtcTransactionTest.pegout_no_change_output | a peg-out with no change is processed and registers nothing | ported | `a_pegout_with_no_change_is_processed_and_registers_nothing` |
| BridgeSupportRegisterBtcTransactionTest.PeginFromUserWithDifferentScriptPubKey.registerBtcTx_legacyPeginWithParseableScriptPubKey_beforeRSKIP305_withSVPOnGoing_shouldThrowISE | before RSKIP305, with an SVP spend awaited, a legacy peg-in whose key reads as a script throws and is not processed | ported | `before_rskip305_an_awaited_svp_spend_fails_a_peg_in_whose_key_reads_as_a_script` |
| BridgeSupportRegisterBtcTransactionTest.PeginFromUserWithDifferentScriptPubKey.registerBtcTx_peginV1WithParseableScriptPubKey_beforeRSKIP305_withSVPOnGoing_shouldThrowISE | the same for a peg-in v1 | ported | `before_rskip305_an_awaited_svp_spend_fails_a_peg_in_whose_key_reads_as_a_script` |
| BridgeSupportRegisterBtcTransactionTest.PeginFromUserWithDifferentScriptPubKey.registerBtcTx_testnetRealPeginV1_withSVPOnGoing_beforeRSKIP305_shouldThrowISE | testnet's real peg-in v1 77a135b5...3866a6 throws at lovell700 with an SVP spend awaited | ported | `before_rskip305_testnets_real_peg_in_fails_while_an_svp_spend_is_awaited` |
| BridgeSupportRegisterBtcTransactionTest.pegout_many_outputs_and_inputs_with_change_output | a peg-out of fifty inputs to forty users registers its change | ported | `a_pegout_to_many_users_registers_its_change` |
| BridgeSupportRegisterBtcTransactionTest.pegout_many_outputs_and_one_input | the same with one input | ported | `a_pegout_to_many_users_registers_its_change` |
| BridgeSupportRegisterBtcTransactionTest.pegout_one_output_and_many_input | fifty inputs to one user registers nothing | ported | `a_pegout_of_many_inputs_to_one_user_registers_nothing` |
| BridgeSupportRegisterBtcTransactionTest.migration_ok | a spend of the retiring federation to the active one registers its output | ported | `a_migration_registers_every_output_to_the_active_federation` |
| BridgeSupportRegisterBtcTransactionTest.migration_many_outputs_and_inputs | fifty inputs, forty outputs, forty UTXOs | ported | `a_migration_registers_every_output_to_the_active_federation` |
| BridgeSupportRegisterBtcTransactionTest.migration_many_outputs_and_one_input | one input, forty outputs, forty UTXOs | ported | `a_migration_registers_every_output_to_the_active_federation` |
| BridgeSupportRegisterBtcTransactionTest.migration_one_outputs_and_many_input | fifty inputs, one output of the minimum peg-out, one UTXO | ported | `a_migration_registers_every_output_to_the_active_federation` |
| BridgeSupportRegisterBtcTransactionTest.flyover_segwit_as_migration_utxo | a migration whose witness spends the retiring federation, proven by the witness root, registers its output | ported | `a_segwit_migration_spending_a_flyover_output_registers_its_outputs` |
| BridgeSupportRegisterBtcTransactionTest.flyover_segwit_as_migration_utxo_with_many_outputs_and_inputs | the same with nine more inputs, ten UTXOs | ported | `a_segwit_migration_spending_a_flyover_output_registers_its_outputs` |
| BridgeSupportRegisterBtcTransactionTest.last_retired_fed_to_active_fed | a spend of the last retired federation is a migration | ported | `a_spend_of_the_last_retired_federation_is_a_migration` |
| BridgeSupportRegisterBtcTransactionTest.no_last_retired_fed_in_storage_sending_funds_to_active_fed | with no last retired federation stored the index still takes it for a migration; the legacy rules refund it as a multisig peg-in | ported | `with_no_last_retired_federation_a_spend_of_it_is_refunded_by_the_legacy_rules` |
| BridgeSupportRegisterBtcTransactionTest.PeginFromUserWithDifferentScriptPubKey.registerBtcTx_legacyPeginWithParseableScriptPubKey_withSVPOnGoing_shouldRegisterPegIn | from RSKIP305 the same legacy peg-in, with an SVP spend awaited, is registered | ported | `before_rskip305_an_awaited_svp_spend_fails_a_peg_in_whose_key_reads_as_a_script` (at reed800) |
| BridgeSupportRegisterBtcTransactionTest.PeginFromUserWithDifferentScriptPubKey.registerBtcTx_peginV1WithParseableScriptPubKey_withSVPOnGoing_shouldRegisterPegin | the same for a peg-in v1 | ported | `before_rskip305_an_awaited_svp_spend_fails_a_peg_in_whose_key_reads_as_a_script` (at reed800) |
| BridgeSupportRegisterBtcTransactionTest.pegin_from_multisig_to_retiring_fed_can_be_refunded | a peg-in to the retiring federation from a multisig sender is refunded, LEGACY_PEGIN_MULTISIG_SENDER | ported | `a_peg_in_from_a_multisig_sender_to_the_retiring_federation_is_refunded` |
| BridgeSupportRegisterBtcTransactionTest.invalid_pegin_v1_from_multisig_to_active_fed_can_be_refunded | an empty v1 payload from a multisig sender is refunded, PEGIN_V1_INVALID_PAYLOAD | ported | `a_peg_in_v1_with_an_empty_payload_from_a_multisig_sender_is_refunded` |
| BridgeSupportRegisterBtcTransactionTest.pegout_sighash_no_exists_in_provider | by the index, a spend of the active federation it doesn't hold is refunded as a multisig peg-in | ported | `by_the_index_a_federation_spend_it_doesnt_hold_is_refunded_as_a_peg_in` |
| BridgeSupportRegisterBtcTransactionTest.migration_sighash_no_exists_in_provider | the same for a spend of the retiring federation | ported | `by_the_index_a_federation_spend_it_doesnt_hold_is_refunded_as_a_peg_in` |

## PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest

Module `peg_utils_all_utxos_to_fed_are_above_minimum_pegin_value_test.rs`. rskj's tests ask
`PegUtils.allUTXOsToFedAreAboveMinimumPeginValue` before RSKIP379 (fingerroot500) and from it
(arrowhead600). The ports read its answer in what `registerBtcTransaction` does with a user's
transaction: before RSKIP379 it is the legacy rules' minimum, a peg-in taken or left alone;
from RSKIP379 it is `evaluatePegin`'s, a peg-in taken or turned down INVALID_AMOUNT, by the
pegout index for a transaction that pays a federation and below the index's start for one
that pays none. Each port holds a case's `pre` and `post` test.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_preRSKIP379AndNoOutputs_true | before RSKIP379 a transaction with no outputs passes | not applicable | no transaction without outputs reaches the function: `registerBtcTransaction` and `registerFlyoverBtcTransaction` run bitcoinj's `verify` first, which refuses it (`BridgeSupport.java` 400-401, 2771-2772) |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_postRSKIP379AndNoOutputs_false | from RSKIP379 it fails | not applicable | as above |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_preRSKIP379WithManyOutputsBelowMinimumButNoneForFed_true | before RSKIP379 outputs below the minimum to no federation pass | ported | `outputs_below_the_minimum_to_no_federation_pass_before_rskip379_only` |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_postRSKIP379WithManyOutputsBelowMinimumButNoneForFed_false | from RSKIP379 they fail | ported | `outputs_below_the_minimum_to_no_federation_pass_before_rskip379_only` |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_preRSKIP379WithManyOutputsEqualAndAboveMinimumButNoneForFed_true | before RSKIP379 outputs of the minimum to no federation pass | ported | `outputs_of_the_minimum_to_no_federation_pass_before_rskip379_only` |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_postRSKIP379WithManyOutputsEqualAndAboveMinimumButNoneForFed_false | from RSKIP379 they fail | ported | `outputs_of_the_minimum_to_no_federation_pass_before_rskip379_only` |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_preRSKIP379WithManyOutputsAboveMinimumForFedAndManyOutputsBelowMinimumForRandomAddresses_true | outputs to the federation above the minimum pass beside smaller ones elsewhere | ported | `outputs_to_the_federation_of_the_minimum_pass_beside_smaller_ones_elsewhere` |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_postRSKIP379WithManyOutputsAboveMinimumForFedAndManyOutputsBelowMinimumForRandomAddresses_true | the same from RSKIP379 | ported | `outputs_to_the_federation_of_the_minimum_pass_beside_smaller_ones_elsewhere` |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_preRSKIP379WithManyOutputsForFedButEachBelowMinimumAndSumAboveMinimum_false | outputs to the federation each below the minimum fail, whatever their sum | ported | `outputs_to_the_federation_each_below_the_minimum_fail_whatever_their_sum` |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_postRSKIP379WithManyOutputsForFedButEachBelowMinimumAndSumAboveMinimum_false | the same from RSKIP379 | ported | `outputs_to_the_federation_each_below_the_minimum_fail_whatever_their_sum` |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_preRSKIP379WithManyOutputsForFedAboveMinimumAndOneBelowMinimum_false | one output to the federation below the minimum fails them all | ported | `one_output_to_the_federation_below_the_minimum_fails_them_all` |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_postRSKIP379WithManyOutputsForFedAboveMinimumAndOneBelowMinimum_false | the same from RSKIP379 | ported | `one_output_to_the_federation_below_the_minimum_fails_them_all` |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_preRSKIP379WithManyOutputsForFedAllEqualOrAboveMinimum_true | outputs to the federation all of the minimum or more pass | ported | `outputs_to_the_federation_all_of_the_minimum_or_more_pass` |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_postRSKIP379WithManyOutputsForFedAllEqualOrAboveMinimum_true | the same from RSKIP379 | ported | `outputs_to_the_federation_all_of_the_minimum_or_more_pass` |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_preRSKIP379UtxosToRetiringFedAboveMinimumAndToActiveFedBelowMinimum_false | an output below the minimum to the active federation fails those to the retiring one | ported | `an_output_to_the_active_federation_below_the_minimum_fails_those_to_the_retiring_one` |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_postRSKIP379UtxosToRetiringFedAboveMinimumAndToActiveFedBelowMinimum_false | the same from RSKIP379 | ported | `an_output_to_the_active_federation_below_the_minimum_fails_those_to_the_retiring_one` |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_preRSKIP379UtxosToRetiringAndActiveFedEqualToAndAboveMinimum_true | outputs of the minimum or more to both federations pass | ported | `outputs_of_the_minimum_or_more_to_both_federations_pass` |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_postRSKIP379UtxosToRetiringAndActiveFedEqualToAndAboveMinimum_true | the same from RSKIP379 | ported | `outputs_of_the_minimum_or_more_to_both_federations_pass` |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_preRSKIP379Exactly1UtxoToFedWithMinimumValue_true | one output of exactly the minimum passes | ported | `one_output_of_exactly_the_minimum_passes` |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_postRSKIP379Exactly1UtxoToFedWithMinimumValue_true | the same (rskj's test runs it under fingerroot500 again) | ported | `one_output_of_exactly_the_minimum_passes` (from RSKIP379) |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_preRSKIP379UtxosBelowMinimumToBothRetiringAndActiveFed_false | outputs below the minimum to both federations fail | ported | `outputs_below_the_minimum_to_both_federations_fail` |
| PegUtilsAllUTXOsToFedAreAboveMinimumPeginValueTest.test_allUTXOsToFedAreAboveMinimumPeginValue_postRSKIP379UtxosBelowMinimumToBothRetiringAndActiveFed_false | the same from RSKIP379 | ported | `outputs_below_the_minimum_to_both_federations_fail` |

## PegUtilsGetTransactionTypeTest

Module `peg_utils_get_transaction_type_test.rs`. rskj's tests ask `PegUtils.getTransactionType`;
the class shows in what `registerBtcTransaction` does, in the runs of the ports of
`BridgeSupportRegisterBtcTransactionTest`. Most listed peg-out and migration cases are
transactions those ports register, and their tests (in
`bridge_support_register_btc_transaction_test.rs`) were made to take this class's shapes too;
the rest are in this module. The last row is a test from the rest of the class.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| PegUtilsGetTransactionTypeTest.anyAddressToAnyAddress_pegin_before_RSKIP379 | before RSKIP379 a transaction between any two addresses is a peg-in | ported | `before_rskip379_a_transaction_between_any_addresses_is_a_peg_in` |
| PegUtilsGetTransactionTypeTest.pegout_no_change_output | a spend of the active federation to a user is a peg-out | ported | `a_pegout_with_no_change_is_processed_and_registers_nothing` (bridge_support_register_btc_transaction_test.rs) |
| PegUtilsGetTransactionTypeTest.many_outputs_and_inputs_pegout | fifty inputs to forty users, no change, is a peg-out | ported | `a_spend_of_the_active_federation_to_forty_users_is_a_peg_out` |
| PegUtilsGetTransactionTypeTest.many_outputs_and_one_input_pegout | one input to forty users, no change, is a peg-out | ported | `a_spend_of_the_active_federation_to_forty_users_is_a_peg_out` |
| PegUtilsGetTransactionTypeTest.one_outputs_and_many_input_pegout | fifty inputs to one user is a peg-out | ported | `a_pegout_of_many_inputs_to_one_user_registers_nothing` (bridge_support_register_btc_transaction_test.rs) |
| PegUtilsGetTransactionTypeTest.many_outputs_and_inputs_migration | fifty inputs of the retiring federation to forty outputs of the active one is a migration | ported | `a_migration_registers_every_output_to_the_active_federation` (bridge_support_register_btc_transaction_test.rs) |
| PegUtilsGetTransactionTypeTest.many_outputs_and_one_input_migration | one input to forty outputs is a migration | ported | `a_migration_registers_every_output_to_the_active_federation` (bridge_support_register_btc_transaction_test.rs) |
| PegUtilsGetTransactionTypeTest.one_outputs_and_many_input_migration | fifty inputs to one output is a migration | ported | `a_migration_registers_every_output_to_the_active_federation` (bridge_support_register_btc_transaction_test.rs) |
| PegUtilsGetTransactionTypeTest.flyover_segwit_migration | a migration spending a Flyover output, the retiring federation's redeem script in its witness | ported | `a_segwit_migration_spending_a_flyover_output_registers_its_outputs` (bridge_support_register_btc_transaction_test.rs) |
| PegUtilsGetTransactionTypeTest.flyover_segwit_with_other_inputs_migration | the same with nine more inputs to one output | ported | `a_segwit_migration_spending_a_flyover_output_registers_its_outputs` (bridge_support_register_btc_transaction_test.rs) |
| PegUtilsGetTransactionTypeTest.flyover_segwit_with_many_inputs_and_outputs_migration | the same with nine more inputs and outputs | ported | `a_segwit_migration_spending_a_flyover_output_registers_its_outputs` (bridge_support_register_btc_transaction_test.rs) |
| PegUtilsGetTransactionTypeTest.unknown_anyAddressToAnyAddress_unknown_after_RSIP379_and_using_pegout_tx_index | by the index such a transaction is no peg-in | ported | `before_rskip379_a_transaction_between_any_addresses_is_a_peg_in` |

## PegUtilsEvaluatePeginTest, PegUtilsTest, PegUtilsLegacyGetTransactionTypeTest

Module `peg_utils_legacy_get_transaction_type_test.rs`. `PegUtilsLegacyGetTransactionTypeTest`'s
set-up names RSKIP186; its listed test holds it.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| PegUtilsEvaluatePeginTest.evaluatePegin_preRSKIP379_throwsException | `evaluatePegin` throws before RSKIP379 | not applicable | a guard against a call rskj never makes: `registerPegIn` calls `evaluatePegin` only from RSKIP379 (`BridgeSupport.java` 479-506) |
| PegUtilsTest.test_getTransactionType_before_tbd_600 | `getTransactionTypeUsingPegoutIndex` throws before RSKIP379 | not applicable | as above: `getTransactionType` asks the index only where RSKIP379 enables it (`PegUtils.java` 99-131) |
| PegUtilsLegacyGetTransactionTypeTest.test_sentFromOldFed | a spend of regtest's old federation is a peg-in under papyrus200 and a migration under iris300 (RSKIP186, RSKIP199) | ported | `from_rskip199_a_spend_of_the_old_federation_is_a_migration` |

## utils.BridgeEventLoggerLegacyImplTest

Module `bridge_event_logger_legacy_impl_test.rs`. rskj's tests call the legacy logger with
RSKIP146 mocked off, then on, where it throws `DeprecatedMethodCallException`. The ports call
mainnet's Bridge a block before papyrus200 and at it, through the calls that log each event:
`updateCollections`, `addSignature` from a member of a federation of known keys over a peg-out
waiting for signatures, the signature that completes it, and a federation elected by the
federation-change authorizers. From RSKIP146 the legacy event is never logged; the Solidity one
is. The last row is the test from the rest of the class.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| utils.BridgeEventLoggerLegacyImplTest.testLogUpdateCollectionsBeforeRskip146 | `update_collections_topic`, data the sender as an RLP element | ported | `before_rskip146_update_collections_is_logged_in_the_legacy_format` |
| utils.BridgeEventLoggerLegacyImplTest.testLogUpdateCollectionsAfterRskip146 | the legacy logger throws from RSKIP146 | ported | `before_rskip146_update_collections_is_logged_in_the_legacy_format` (from RSKIP146 the Solidity event alone) |
| utils.BridgeEventLoggerLegacyImplTest.testLogAddSignatureBeforeRskip146 | `add_signature_topic`, data the RLP list of the transaction's hash string, the federator's key hash and the RSK transaction's hash | ported | `before_rskip146_a_signature_is_logged_in_the_legacy_format` |
| utils.BridgeEventLoggerLegacyImplTest.testLogAddSignatureAfterRskip146 | the legacy logger throws from RSKIP146 | ported | `before_rskip146_a_signature_is_logged_in_the_legacy_format` |
| utils.BridgeEventLoggerLegacyImplTest.testLogReleaseBtcBeforeRskip146 | `release_btc_topic`, data the RLP list of the signed transaction's hash string and the transaction | ported | `before_rskip146_a_pegout_signed_is_logged_in_the_legacy_format` |
| utils.BridgeEventLoggerLegacyImplTest.testLogReleaseBtcAfterRskip146 | the legacy logger throws from RSKIP146 | ported | `before_rskip146_a_pegout_signed_is_logged_in_the_legacy_format` |
| utils.BridgeEventLoggerLegacyImplTest.testLogCommitFederationBeforeRskip146 | `commit_federation_topic`, data the old and new federations' address hash and keys and the activation block as a decimal string | ported | `before_rskip146_a_federation_committed_is_logged_in_the_legacy_format` |
| utils.BridgeEventLoggerLegacyImplTest.testLogCommitFederationAfterRskip146 | the legacy logger throws from RSKIP146 | ported | `before_rskip146_a_federation_committed_is_logged_in_the_legacy_format` |
| utils.BridgeEventLoggerLegacyImplTest.testLogCommitFederationFailure_throwsUnsupportedOperationException | the legacy logger has no commit-federation-failure event | not applicable | rskj logs that event only through `BridgeEventLoggerImpl`, and rustock only in the Solidity format (`events.rs` `log_commit_federation_failed`); no legacy-format call exists to port |

## utils.BridgeEventLoggerImplTest

Module `bridge_event_logger_impl_test.rs`. rskj's tests hand the logger an activation config;
the ports call mainnet's Bridge at hop400, arrowhead600, fingerroot500 and lovell700 and read
the event logged: `releaseBtc` taken and turned down, and a federation elected. The class's other
tests run under `all()` activations with no window and are not listed.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| utils.BridgeEventLoggerImplTest.logCommitFederation | `commit_federation`'s activation block: the block and 18,500 before RSKIP383, 40,320 from it | ported | `a_federation_committed_is_logged_with_the_activation_age_of_its_era` |
| utils.BridgeEventLoggerImplTest.logReleaseBtcRequestReceived_preRSKIP326_destinationAddressAsHash160 | before RSKIP326, `release_request_received(address,bytes,uint256)` with the destination's hash160 | ported | `a_pegout_request_taken_logs_its_destination_and_amount_as_its_era_has_them` |
| utils.BridgeEventLoggerImplTest.logReleaseBtcRequestReceived_postRSKIP326_destinationAddressAsBase58 | from RSKIP326, `release_request_received(address,string,uint256)` with the Base58 address | ported | `a_pegout_request_taken_logs_its_destination_and_amount_as_its_era_has_them` |
| utils.BridgeEventLoggerImplTest.logReleaseBtcRequestReceived_postRSKIP427_amountAsWeis | from RSKIP427 the amount in wei | ported | `a_pegout_request_taken_logs_its_destination_and_amount_as_its_era_has_them` |
| utils.BridgeEventLoggerImplTest.logReleaseBtcRequestRejected_preRSKIP427_logAmountAsSatoshis | before RSKIP427, `release_request_rejected`'s amount in satoshis | ported | `a_pegout_request_turned_down_logs_its_amount_in_satoshis_before_rskip427_and_in_wei_from_it` |
| utils.BridgeEventLoggerImplTest.logReleaseBtcRequestRejected_postRSKIP427_logAmountAsWeis | from RSKIP427 in wei | ported | `a_pegout_request_turned_down_logs_its_amount_in_satoshis_before_rskip427_and_in_wei_from_it` |

## BridgeSupportAddSignatureTest

Module `bridge_support_add_signature_test.rs`. rskj's tests mock RSKIP146 on and RSKIP326 either
way; the ports run mainnet's Bridge at hop400 and at fingerroot500. rskj's invalid signature is
70 bytes that aren't DER, handed to `BridgeSupport`; the Bridge turns those down before
`BridgeSupport` sees them, so the port sends a DER signature that doesn't verify.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| BridgeSupportAddSignatureTest.addSignature_calledTwiceWithSameFederatorPreRSKIP326_emitEventTwice | before RSKIP326 a federator signing twice is logged twice | ported | `a_federator_signing_twice_is_logged_twice_before_rskip326_and_once_from_it` |
| BridgeSupportAddSignatureTest.addSignature_calledTwiceWithSameFederatorPostRSKIP326_emitEventOnlyOnce | from RSKIP326, once | ported | `a_federator_signing_twice_is_logged_twice_before_rskip326_and_once_from_it` |
| BridgeSupportAddSignatureTest.addSignatureCreateEventLog_preRSKIP326 | a signature is logged before RSKIP326 | ported | `a_federators_signature_is_logged` |
| BridgeSupportAddSignatureTest.addSignature_invalidSignatureBeforeRSKIP326_eventStillEmitted | before RSKIP326 a signature that fails is still logged | ported | `a_signature_that_does_not_verify_is_logged_before_rskip326_only` |
| BridgeSupportAddSignatureTest.addSignature_afterRSKIP326_eventEmitted | a signature is logged from RSKIP326 | ported | `a_federators_signature_is_logged` |
| BridgeSupportAddSignatureTest.addSignature_invalidSignatureAfterRSKIP326_noEventEmitted | from RSKIP326 a signature that fails is not logged | ported | `a_signature_that_does_not_verify_is_logged_before_rskip326_only` |

The rest of the class (the federator's federation, a missing transaction, too few or too many
signatures, a non-canonical signature, segwit and migration signing) is under
"Listed tests ported later".

## BridgeSupportPegoutTransactionCreatedEventTest

Module `bridge_support_pegout_transaction_created_event_test.rs`. The ports run mainnet's
`updateCollections` at arrowhead600 and lovell700 over a federation of known keys whose UTXOs
are a bitcoin each, three requests of half a bitcoin, and for a migration a retiring federation
and the active one made a block more than its activation age before, as rskj's tests make it.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| BridgeSupportPegoutTransactionCreatedEventTest.updateCollections_whenPegoutBatchIsCreated_shouldLogPegoutTransactionCreatedEvent | a batch logs `batch_pegout_created`, `release_requested`, and from RSKIP428 `pegout_transaction_created` with its hash and spent values | ported | `from_rskip428_a_pegout_batch_built_is_logged_with_the_values_it_spends` |
| BridgeSupportPegoutTransactionCreatedEventTest.updateCollections_whenPegoutMigrationIsCreated_shouldLogPegoutTransactionCreatedEvent | the same for a migration | ported | `from_rskip428_a_migration_built_is_logged_with_the_values_it_spends` |
| BridgeSupportPegoutTransactionCreatedEventTest.updateCollections_whenPegoutMigrationAndBatchAreCreated_shouldLogPegoutTransactionCreatedEvent | both in one call, each logged | ported | `from_rskip428_a_migration_and_a_pegout_batch_built_together_are_each_logged` |

## BridgeSupportSigHashTest

Module `bridge_support_sig_hash_test.rs`, the same set-up under fingerroot500 and arrowhead600;
the ports read the pegout index's cell for the first input's sighash.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| BridgeSupportSigHashTest.test_pegoutTxIndex_when_pegout_batch_is_created | from RSKIP379 the batch's first sighash is indexed; before, nothing | ported | `from_rskip379_a_pegout_batch_built_goes_to_the_pegout_index` |
| BridgeSupportSigHashTest.test_pegoutTxIndex_when_migration_tx_is_created | the same for a migration | ported | `from_rskip379_a_migration_built_goes_to_the_pegout_index` |
| BridgeSupportSigHashTest.test_pegoutTxIndex_when_migration_and_pegout_batch_tx_are_created | both, each indexed | ported | `from_rskip379_a_migration_and_a_pegout_batch_built_together_both_go_to_the_pegout_index` |

## BridgeSupportProcessFundsMigrationTest

Module `bridge_support_process_funds_migration_test.rs`: sixty retiring UTXOs, more than mainnet's
fifty inputs, at iris300 and, for the contrast, hop400. The last row is a test from the rest of
the class.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| BridgeSupportProcessFundsMigrationTest.StandardMultisigFederationTest.updateCollections_duringMigration_preRSKIP294_withMoreUtxosThanMaxInputs_shouldUseAllRetiringUtxos | before RSKIP294 a migration in its window spends every retiring UTXO; the retiring federation stays | ported | `before_rskip294_a_migration_in_its_window_spends_every_retiring_utxo` |
| BridgeSupportProcessFundsMigrationTest.StandardMultisigFederationTest.updateCollections_pastMigrationAge_preRSKIP294_withMoreUtxosThanMaxInputs_shouldUseAllRetiringUtxos | past its age, the same, and the retiring federation is cleared | ported | `before_rskip294_the_last_migration_spends_every_retiring_utxo` |
| BridgeSupportProcessFundsMigrationTest.StandardMultisigFederationTest.updateCollections_pastMigrationAge_withMoreUtxosThanMaxInputs_shouldCreateLastMigrationTxWithMaxInputsAndClearRetiringFedEvenIfUtxosRemain | from RSKIP294 the last migration spends fifty and clears the federation, UTXOs left over | ported | `before_rskip294_the_last_migration_spends_every_retiring_utxo` |

## ReleaseTransactionBuilderTest

Module `release_transaction_builder_test.rs`: the Bridge builds each transaction a block before the
rule and at it. The refund is a peg-in from a multisig sender on rskj's regtest under papyrus200
and iris300. The P2SH ERP migration stores the retiring federation in that format; its UTXOs pay
the standard script, which the version rule does not read.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| ReleaseTransactionBuilderTest.BuildAmountToTest.buildAmountTo_whenRSKIP201IsNotActive_shouldCreatePegoutTxWithBtcVersion1 | a peg-out is version 1 before RSKIP201 | ported | `before_rskip201_a_pegout_is_built_as_version_1` |
| ReleaseTransactionBuilderTest.BuildEmptyWalletToTest.StandardMultiSigFederationTest.buildEmptyWalletTo_whenRSKIP201IsNotActive_shouldCreateRefundTxWithBtcVersion1 | a refund is version 1 before RSKIP201 | ported | `before_rskip201_a_refund_is_built_as_version_1` |
| ReleaseTransactionBuilderTest.BuildMigrationTransactionTest.StandardMultiSigFederationTests.buildMigrationTransaction_whenRSKIP376IsNotActive_shouldCreateMigrationTxWithBtcVersion1 | a migration is version 1 before RSKIP376 | ported | `before_rskip376_a_migration_is_built_as_version_1` |
| ReleaseTransactionBuilderTest.BuildMigrationTransactionTest.P2shErpFederationTests.buildMigrationTransaction_whenRSKIP376IsNotActive_shouldCreateMigrationTxWithBtcVersion1 | the same from a P2SH ERP federation | ported | `before_rskip376_a_migration_is_built_as_version_1` |

## BridgeStorageProviderPegoutTxIndexTests

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| BridgeStorageProviderPegoutTxIndexTests.hasPegoutSigHash_null_sigHash | a null hash is never looked up | not applicable | no flow asks the index for a null hash: the Bridge asks it for the sighash of a registered transaction's first input |
| BridgeStorageProviderPegoutTxIndexTests.hasPegoutTxSigHash_non_null_sigHash | the index is read from RSKIP379 only | ported | `by_the_index_a_federation_spend_it_doesnt_hold_is_refunded_as_a_peg_in`, with the runs before RSKIP379 of the spends in `bridge_support_register_btc_transaction_test.rs` |
| BridgeStorageProviderPegoutTxIndexTests.setPegoutTxSigHash_null | a null hash is never written | not applicable | as above: the Bridge writes the sighash of a peg-out it built |
| BridgeStorageProviderPegoutTxIndexTests.setPegoutTxSigHash_non_null | the index is written from RSKIP379 only | ported | `from_rskip379_a_pegout_batch_built_goes_to_the_pegout_index` (bridge_support_sig_hash_test.rs) |

## bitcoin.BitcoinUtilsLegacyTest

Before RSKIP305 rskj strips a transaction's signatures with `BitcoinUtilsLegacy`, which the Bridge
reaches when it checks a registered transaction against the SVP spend it awaits (from RSKIP419).

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| bitcoin.BitcoinUtilsLegacyTest.getMultiSigTransactionHashWithoutSignaturesBeforeRSKIP305_fromRealPeginFromP2PKHWithParseableScriptPubKey_shouldThrowISE | testnet's real P2PKH peg-in whose key reads as a script throws | ported | `before_rskip305_testnets_real_peg_in_fails_while_an_svp_spend_is_awaited` (bridge_support_register_btc_transaction_test.rs) |
| bitcoin.BitcoinUtilsLegacyTest.getMultiSigTransactionHashWithoutSignaturesBeforeRSKIP305_fromP2PKH_withParseableScriptPubKey_shouldThrowISE | the same for a P2PKH input whose key reads as a script | ported | `before_rskip305_an_awaited_svp_spend_fails_a_peg_in_whose_key_reads_as_a_script` (bridge_support_register_btc_transaction_test.rs) |

## PegoutsWaitingForConfirmationsTest

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| PegoutsWaitingForConfirmationsTest.getNextPegoutWithEnoughConfirmations_rskip559 | with RSKIP559 the pegout picked among several confirmed | not applicable | RSKIP559 is `tbd1000` in VETIVER-9.0.4, scheduled on no network |

## Listed tests ported later

Ported after the rest of the table, in the same tree; the paths are under `crates/execution/src/`.

| rskj test | rule | port | verdict |
|---|---|---|---|
| `PocSighashTest.test_each_input_sighash_is_unique` | RSKIP-305 | `each_input_of_a_pegout_has_a_sighash_of_its_own` (`rskj_tests/bridge_pegouts/poc_sighash_test.rs`) | ported |
| `PocSighashTest.test_sighash_is_different_when_tx_is_altered` | RSKIP-305 | `a_pegout_of_one_more_output_has_none_of_the_sighashes` (`rskj_tests/bridge_pegouts/poc_sighash_test.rs`) | ported |
| `PocSighashTest.test_sighash_is_equal_for_signed_input_and_unsigned_input` | RSKIP-305 | `an_input_has_its_sighash_signed_or_not` (`rskj_tests/bridge_pegouts/poc_sighash_test.rs`) | ported |
| `PocSighashTest.test_each_input_sighash_is_unique_using_real_tx_testnet` | RSKIP-305 | `a_pegout_signed_again_has_the_sighashes_of_the_one_testnet_confirmed` (`rskj_tests/bridge_pegouts/poc_sighash_test.rs`) | ported |
| `PocSighashTest.test_each_input_sighash_is_unique_for_a_signed_erp_tx_testnet` | RSKIP-305 | `the_emergency_keys_sign_other_sighashes_than_the_members` (`rskj_tests/bridge_pegouts/poc_sighash_test.rs`) | ported |
| `PocSighashTest.test_redeemScript_can_be_obtained_from_input` | RSKIP-305 | `an_inputs_redeem_script_is_the_federations` (`rskj_tests/bridge_pegouts/poc_sighash_test.rs`) | ported |
| `BitcoinUtilsLegacyTest.getMultiSigTransactionHashWithoutSignatures_whenTransactionDoesNotHaveInputs_shouldReturnSameTxHash` | RSKIP-379 | `a_transaction_of_no_inputs_keeps_its_hash` (`rskj_tests/bridge_pegouts/bitcoin_utils_legacy_test.rs`) | ported |
| `BitcoinUtilsLegacyTest.getMultiSigTransactionHashWithoutSignatures_whenTransactionIsSegwit_shouldReturnExpectedTxHash` | RSKIP-379 | `a_segwit_transaction_keeps_its_hash_and_is_unsigned_as_it_was` (`rskj_tests/bridge_pegouts/bitcoin_utils_legacy_test.rs`) | ported |
| `BitcoinUtilsLegacyTest.getMultiSigTransactionHashWithoutSignatures_whenTxIsNotSegwitAndTransactionInputsDoNotHaveP2shMultiSigInputScript_shouldThrowIAE` | RSKIP-379 | `an_input_of_no_redeem_script_fails_both` (`rskj_tests/bridge_pegouts/bitcoin_utils_legacy_test.rs`) | ported |
| `BitcoinUtilsLegacyTest.getMultiSigTransactionHashWithoutSignatures_whenTxIsNotSegwitButNotAllTransactionInputsHaveP2shMultiSigInputScript_shouldThrowIAE` | RSKIP-379 | `one_input_of_no_redeem_script_fails_both` (`rskj_tests/bridge_pegouts/bitcoin_utils_legacy_test.rs`) | ported |
| `BitcoinUtilsLegacyTest.getMultiSigTransactionHashWithoutSignatures_whenTransactionIsLegacyAndInputsHaveP2shMultiSigInputScript_shouldReturnExpectedTxHash` | RSKIP-379 | `a_legacy_transaction_is_unsigned_as_it_was` (`rskj_tests/bridge_pegouts/bitcoin_utils_legacy_test.rs`) | ported |
| `BridgeSupportIT.initialChainHeadWithBtcCheckpoints_whenCheckpointsWith12BytesChainWork_before_RSKIP454_ok` | RSKIP-454 | `before_rskip454_a_checkpoint_whose_chain_work_fits_12_bytes_seeds_the_store` (`rskj_tests/bridge_methods/bridge_support_it.rs`) | ported |
| `BridgeSupportIT.initialChainHeadWithBtcCheckpoints_whenCheckpointsWith32BytesChainWork_before_RSKIP454_shouldFail` | RSKIP-454 | `before_rskip454_a_checkpoint_whose_chain_work_is_past_12_bytes_fails_the_call` (same file) | fixed: before RSKIP-454 rustock kept the low 12 bytes of a chain work past them, which bitcoinj refuses to write |
| `BridgeSupportIT.initialChainHeadWithBtcCheckpoints_whenCheckpointsWith32BytesChainWork_after_RSKIP454_ok` | RSKIP-454 | `from_rskip454_a_checkpoint_of_any_chain_work_seeds_the_store` (same file) | ported |
| `BridgeSupportAddSignatureTest`, its plain tests | RSKIP-40; RSKIP-326 | the tests of `rskj_tests/bridge_pegouts/bridge_support_add_signature_test.rs` | ported |
| `BridgeSupportAddSignatureTest.addSignature`, its four cases | RSKIP-415 | `from_rskip415_a_federator_is_logged_at_its_rsk_keys_address` (same file) | ported |
| `BridgeSupportAddSignatureTest.AddSignature` and `MigrationTxSigning`, nested | RSKIP-305 | `a_majority_of_a_segwit_federation_signs_its_pegout_and_it_is_released`, `a_released_pegout_takes_no_more_signatures`, `members_of_the_retiring_federation_sign_its_migration` (same file) | ported |
| `BridgeSupportAddSignatureTest.addSignature_fedPubKey_*`, four | RSKIP-40 | not ported: each hands `BridgeSupport` a null signature list, which returns at once (1683-1685); the Bridge refuses an empty list first (`Bridge.java` 629), as `no_signatures_fail_the_call` holds | not applicable |
| `PegoutsWaitingForConfirmationsTest.getNextPegoutWithEnoughConfirmation_multipleMatch_rskip559Off` | RSKIP-40 | `before_rskip559_the_next_pegout_is_the_first_confirmed_one_the_set_walks_to` (`rskj_tests/bridge_pegouts/pegouts_waiting_for_confirmations_test.rs`) | ported |

Not taken one by one: the 14 tests of three classes that name a rule only in their set-up,
`BridgeRSKIP220NewMethodsTest` (5), `BridgeSupportRSKIP220NewMethodsTest` (6) and
`FlyoverCompatibleBtcWallextWithSingleScriptTest` (3).
