# rskj's Bridge method tests and Flyover's, and their rustock counterparts

One row per test of rskj's `BridgeTest`, `BridgeIT` and `BridgeSupportFlyoverTest` that names an
RSKIP, directly or through its set-up or a helper (the listed tests), with one of four verdicts:
**ported** (a rustock test written for it), **covered** (a rustock test asserts the same: one that
was there before, or, in the rest of a class, one ported for another row), **not applicable** (why
rustock needs no such test), or **fixed** (the ported test found rustock doing otherwise than rskj;
rustock was put right and the test passes; described under [Differences found](#differences-found)).
Below each class's listed tests, the rest of the class: what was read for refusals, limits,
overflows, error paths and height windows, with its port or the test that holds it, and the rest's
other tests with why they need no port. The counts below are of the listed tests.

rskj at VETIVER-9.0.4. The ported tests are in `crates/execution/src/rskj_tests/bridge_methods/`:
`bridge_test.rs`, `bridge_it.rs` and `bridge_support_flyover_test.rs`, with `support.rs` for what
they share. Each calls the Bridge as a transaction or a contract does, through `execute_bridge`
(and, for a contract's call, `RskExecutor::execute_block`), on a ladder of rustock's upgrades,
without mocks: what rskj's tests mock (federations, peg-outs waiting, the Bitcoin chain, the
locking cap, the Bridge's balance) is written where the Bridge reads it. Where an rskj test switches
one RSKIP on, the port takes the upgrade that brings it. Other rustock tests are named with their
file under `crates/execution/src/`.

| class | listed | ported | covered | not applicable | fixed |
|---|---:|---:|---:|---:|---:|
| `BridgeTest` | 126 | 123 | 3 | 0 | 0 |
| `BridgeIT` | 45 | 43 | 2 | 0 | 0 |
| `BridgeSupportFlyoverTest` | 43 | 37 | 1 | 1 | 4 |
| total | 214 | 203 | 6 | 1 | 4 |

## Differences found

One port found rustock doing otherwise than rskj: `before_rskip293_an_output_bitcoinj_cannot_read_as_an_address_is_a_generic_error`
(`bridge_support_flyover_test.rs`), the port of four `BridgeSupportFlyoverTest` tests. rustock is put right,
and the port passes.

- **Rule:** RSKIP293, before it. Flyover (RSKIP176) opens at iris300 and RSKIP293 comes with hop400,
  so the window is mainnet blocks 3,614,800 to 4,598,499 and testnet blocks 2,060,500 to 3,102,999.
- **rskj:** before RSKIP293, `BridgeUtils.getAmountSentToAddresses` calls
  `BridgeUtilsLegacy.getAmountSentToAddress`, which reads every output of the payment as an address
  (`output.getScriptPubKey().getToAddress(params)`). bitcoinj throws a `ScriptException` for an
  output that is neither P2PKH nor P2SH, a bech32 one say, and `Bridge.registerFlyoverBtcTransaction`
  catches it and answers GENERIC_ERROR (-900), having written nothing.
- **rustock, before the fix:** `register_fast_bridge_btc_transaction` compared each output's script with
  the Flyover address's and threw for none. A payment to a bech32 address alone was answered VALUE_ZERO
  (-304), and one that paid the federation's Flyover address as well was registered: the LBC was paid and
  the federation got the output. Before RSKIP293 it now reads every output as bitcoinj does, and answers
  GENERIC_ERROR (-900) where one can't be read as an address.

## BridgeTest

rskj's tests named for a method alone (`BridgeTest.addFederatorPublicKey` to
`BridgeTest.getEstimatedFeesForNextPegoutEvent`) call the method at eight upgrades, orchid to
lovell700, from a transaction and by each kind of contract call (CALL, CALLCODE, DELEGATECALL,
STATICCALL), and assert where the method is closed and, from arrowhead600 (RSKIP417), which kinds of
call it takes and what it answers. Each port does the same through `by_every_kind_at_every_upgrade`
in `bridge_test.rs`. What their `assertVoidMethodResult` asserts of a method with nothing to answer
is held by `a_method_with_nothing_to_answer_answers_a_contract_from_arrowhead600`.

| rskj test | what it asserts | verdict | rustock test |
|---|---|---|---|
| BridgeTest.getActivePowpegRedeemScript_before_RSKIP293_activation | closed before RSKIP293 | ported | `the_active_powpeg_redeem_script_is_the_genesis_federations_from_hop400` |
| BridgeTest.getActivePowpegRedeemScript_after_RSKIP293_activation | answers the active federation's redeem script | ported | `the_active_powpeg_redeem_script_is_the_genesis_federations_from_hop400` |
| BridgeTest.getLockingCap_before_RSKIP134_activation | closed before RSKIP134 | ported | `the_locking_cap_is_its_initial_value_from_papyrus200` |
| BridgeTest.getLockingCap_after_RSKIP134_activation | answers the cap, at first the initial one | ported | `the_locking_cap_is_its_initial_value_from_papyrus200` |
| BridgeTest.increaseLockingCap_before_RSKIP134_activation | closed before RSKIP134 | ported | `increase_locking_cap_is_closed_before_papyrus200` |
| BridgeTest.increaseLockingCap_after_RSKIP134_activation | its authorizer raises the cap, up to the cap times its increments multiplier | ported | `increase_locking_cap_raises_the_cap_up_to_its_multiple_for_its_authorizer` |
| BridgeTest.registerBtcCoinbaseTransaction_before_RSKIP143_activation | closed before RSKIP143 | ported | `register_btc_coinbase_transaction_is_closed_before_papyrus200` |
| BridgeTest.registerBtcCoinbaseTransaction_after_RSKIP143_activation | runs from RSKIP143 | ported | `register_btc_coinbase_transaction_runs_from_papyrus200` |
| BridgeTest.registerBtcCoinbaseTransaction_after_RSKIP143_activation_null_data | throws for arguments that do not decode | ported | `register_btc_coinbase_transaction_throws_for_arguments_that_do_not_decode` |
| BridgeTest.registerBtcTransaction_beforeRskip199_rejectsExternalCalls | before RSKIP199 a caller outside the federations is refused | ported | `register_btc_transaction_turns_a_stranger_down_before_iris300` |
| BridgeTest.registerBtcTransaction_beforeRskip199_acceptsCallFromFederationMember | before RSKIP199 a federator's call runs | ported | `register_btc_transaction_takes_a_federator_before_iris300` |
| BridgeTest.registerBtcTransaction_afterRskip199_acceptsExternalCalls | from RSKIP199 anyone's call runs | ported | `register_btc_transaction_takes_anyone_from_iris300` |
| BridgeTest.getActiveFederationCreationBlockHeight_before_RSKIP186_activation | closed before RSKIP186 | ported | `the_active_federation_creation_block_height_is_read_from_iris300` |
| BridgeTest.getActiveFederationCreationBlockHeight_after_RSKIP186_activation | answers the active federation's creation height | ported | `the_active_federation_creation_block_height_is_read_from_iris300` |
| BridgeTest.registerFlyoverBtcTransaction_before_RSKIP176_activation | closed before RSKIP176 | ported | `register_flyover_btc_transaction_is_closed_before_iris300` |
| BridgeTest.registerFlyoverBtcTransaction_after_RSKIP176_activation_testnet_p2sh_refund_address_before_RSKIP284_activation_fails | a testnet P2SH refund address is a generic error before RSKIP284 | ported | `a_p2sh_refund_address_is_a_generic_error_before_hop400` |
| BridgeTest.registerFlyoverBtcTransaction_after_RSKIP176_activation_testnet_p2sh_refund_address_after_RSKIP284_activation_ok | from RSKIP284 the P2SH refund address is read | ported | `a_p2sh_refund_address_gets_through_from_hop400` |
| BridgeTest.registerFlyoverBtcTransaction_after_RSKIP176_activation_generic_error | a refund address that does not parse is a generic error | ported | `a_refund_address_that_does_not_parse_is_a_generic_error` |
| BridgeTest.registerFlyoverBtcTransaction_after_RSKIP176_null_parameter | throws for arguments that do not decode | ported | `register_flyover_btc_transaction_throws_for_arguments_that_do_not_decode` |
| BridgeTest.receiveHeader_before_RSKIP200 | closed before RSKIP200 | ported | `receive_header_is_closed_before_iris300` |
| BridgeTest.receiveHeader_after_RSKIP200_Ok | answers for a header | ported | `receive_header_answers_for_a_header_with_no_parent_from_iris300` |
| BridgeTest.receiveHeaders_after_RSKIP200_notFederation | refuses a caller outside the federations | ported | `receive_headers_turns_a_stranger_down_from_iris300` |
| BridgeTest.receiveHeaders_after_RSKIP200_header_wrong_size | a header of the wrong size is answered -20 | ported | `a_header_of_the_wrong_size_is_answered_minus_20` |
| BridgeTest.getBtcBlockchainBestChainHeightOnlyAllowsLocalCalls_afterRskip220 | local calls only from RSKIP220 | covered | `local_call_gating_matches_rskj` (`bridge/mod.rs`) |
| BridgeTest.getBtcBlockchainBestChainHeightOnlyAllowsLocalCalls_beforeRskip220 | a transaction may call it before RSKIP220 | covered | `local_call_gating_matches_rskj` (`bridge/mod.rs`) |
| BridgeTest.getNextPegoutCreationBlockNumber_before_RSKIP271_activation | closed before RSKIP271 | ported | `the_next_pegout_creation_block_number_is_read_from_hop400` |
| BridgeTest.getNextPegoutCreationBlockNumber_after_RSKIP271_activation | answers the stored height | ported | `the_next_pegout_creation_block_number_is_read_from_hop400` |
| BridgeTest.getQueuedPegoutsCount_before_RSKIP271_activation | closed before RSKIP271 | ported | `the_queued_pegouts_are_counted_from_hop400` |
| BridgeTest.getQueuedPegoutsCount_after_RSKIP271_activation | counts the queued peg-outs | ported | `the_queued_pegouts_are_counted_from_hop400` |
| BridgeTest.getEstimatedFeesForNextPegOutEvent_before_RSKIP271_activation | closed before RSKIP271 | ported | `the_estimated_fees_for_the_next_pegout_are_answered_from_hop400` |
| BridgeTest.getEstimatedFeesForNextPegOutEvent_after_RSKIP271_activation | answers the estimate | ported | `the_estimated_fees_for_the_next_pegout_are_answered_from_hop400` |
| BridgeTest.getEstimatedFeesForPegOutAmount_preRSKIP540__shouldThrowVMException | throws before RSKIP540 | covered | `test_rskip540_estimated_fees_for_pegout_amount_disabled_before_vetiver900` (`executor.rs`) |
| BridgeTest.getEstimatedFeesForPegOutAmount_afterRSKIP540_shouldExecute | answers the estimate for an amount | ported | `the_estimated_fees_for_a_pegout_amount_are_answered_from_vetiver900` |
| BridgeTest.addFederatorPublicKey | open until wasabi100, closed from it | ported | `add_federator_public_key_closes_at_wasabi100` |
| BridgeTest.addFederatorPublicKeyMultikey | closed until wasabi100, open from it | ported | `add_federator_public_key_multikey_opens_at_wasabi100` |
| BridgeTest.addLockWhitelistAddress | open until orchid, closed from it | ported | `add_lock_whitelist_address_closes_at_orchid` |
| BridgeTest.addOneOffLockWhitelistAddress | closed until orchid, open from it | ported | `add_one_off_lock_whitelist_address_opens_at_orchid` |
| BridgeTest.addUnlimitedLockWhitelistAddress | closed until orchid, open from it | ported | `add_unlimited_lock_whitelist_address_opens_at_orchid` |
| BridgeTest.addSignatures | from arrowhead600 a CALL only | ported | `add_signature_takes_only_a_call_from_arrowhead600` |
| BridgeTest.commitFederation | from arrowhead600 a CALL only | ported | `commit_federation_takes_only_a_call_from_arrowhead600` |
| BridgeTest.createFederation | from arrowhead600 a CALL only | ported | `create_federation_takes_only_a_call_from_arrowhead600` |
| BridgeTest.getBtcBlockchainBestChainHeight | from arrowhead600 a CALL or a STATICCALL | ported | `btc_blockchain_best_chain_height_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getBtcBlockchainInitialBlockHeight | from arrowhead600 a CALL or a STATICCALL | ported | `btc_blockchain_initial_block_height_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getBtcBlockchainBlockLocator | open until orchid, closed from it | ported | `btc_blockchain_block_locator_closes_at_orchid` |
| BridgeTest.getBtcBlockchainBlockHashAtDepth | closed until orchid, open from it | ported | `btc_blockchain_block_hash_at_depth_opens_at_orchid` |
| BridgeTest.getBtcTransactionConfirmations | closed until wasabi100, open from it | ported | `btc_transaction_confirmations_open_at_wasabi100` |
| BridgeTest.getBtcTxHashProcessedHeight | from arrowhead600 a CALL or a STATICCALL | ported | `btc_tx_hash_processed_height_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getFederationAddress | from arrowhead600 a CALL or a STATICCALL | ported | `federation_address_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getFederationCreationBlockNumber | from arrowhead600 a CALL or a STATICCALL | ported | `federation_creation_block_number_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getFederationCreationTime | from arrowhead600 a CALL or a STATICCALL | ported | `federation_creation_time_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getFederationSize | from arrowhead600 a CALL or a STATICCALL | ported | `federation_size_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getFederationThreshold | from arrowhead600 a CALL or a STATICCALL | ported | `federation_threshold_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getFederatorPublicKey | open until wasabi100, closed from it | ported | `federator_public_key_closes_at_wasabi100` |
| BridgeTest.getFederatorPublicKeyOfType | closed until wasabi100, open from it | ported | `federator_public_key_of_type_opens_at_wasabi100` |
| BridgeTest.getFeePerKb | from arrowhead600 a CALL or a STATICCALL | ported | `fee_per_kb_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getLockWhitelistAddress | from arrowhead600 a CALL or a STATICCALL | ported | `lock_whitelist_address_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getLockWhitelistEntryByAddress | closed until orchid, open from it | ported | `lock_whitelist_entry_by_address_opens_at_orchid` |
| BridgeTest.getLockWhitelistSize | from arrowhead600 a CALL or a STATICCALL | ported | `lock_whitelist_size_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getMinimumLockTxValue | from arrowhead600 a CALL or a STATICCALL | ported | `minimum_lock_tx_value_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getPendingFederationHash | from arrowhead600 a CALL or a STATICCALL | ported | `pending_federation_hash_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getPendingFederationSize | from arrowhead600 a CALL or a STATICCALL | ported | `pending_federation_size_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getPendingFederatorPublicKey | open until wasabi100, closed from it | ported | `pending_federator_public_key_closes_at_wasabi100` |
| BridgeTest.getPendingFederatorPublicKeyOfType | closed until wasabi100, open from it | ported | `pending_federator_public_key_of_type_opens_at_wasabi100` |
| BridgeTest.getRetiringFederationAddress | from arrowhead600 a CALL or a STATICCALL | ported | `retiring_federation_address_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getRetiringFederationCreationBlockNumber | from arrowhead600 a CALL or a STATICCALL | ported | `retiring_federation_creation_block_number_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getRetiringFederationCreationTime | from arrowhead600 a CALL or a STATICCALL | ported | `retiring_federation_creation_time_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getRetiringFederationSize | from arrowhead600 a CALL or a STATICCALL | ported | `retiring_federation_size_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getRetiringFederationThreshold | from arrowhead600 a CALL or a STATICCALL | ported | `retiring_federation_threshold_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getRetiringFederatorPublicKey | open until wasabi100, closed from it | ported | `retiring_federator_public_key_closes_at_wasabi100` |
| BridgeTest.getRetiringFederatorPublicKeyOfType | closed until wasabi100, open from it | ported | `retiring_federator_public_key_of_type_opens_at_wasabi100` |
| BridgeTest.getProposedFederationAddress | closed until lovell700, open from it | ported | `proposed_federation_address_opens_at_lovell700` |
| BridgeTest.getProposedFederationSize | closed until lovell700, open from it | ported | `proposed_federation_size_opens_at_lovell700` |
| BridgeTest.getProposedFederationCreationTime | closed until lovell700, open from it | ported | `proposed_federation_creation_time_opens_at_lovell700` |
| BridgeTest.getProposedFederationCreationBlockNumber | closed until lovell700, open from it | ported | `proposed_federation_creation_block_number_opens_at_lovell700` |
| BridgeTest.getProposedFederatorPublicKeyOfType | closed until lovell700, open from it | ported | `proposed_federator_public_key_of_type_opens_at_lovell700` |
| BridgeTest.getStateForBtcReleaseClient | from arrowhead600 a CALL or a STATICCALL | ported | `state_for_btc_release_client_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getStateForSvpClient | closed until lovell700, open from it | ported | `state_for_svp_client_opens_at_lovell700` |
| BridgeTest.getStateForDebugging | from arrowhead600 a CALL or a STATICCALL | ported | `state_for_debugging_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.getLockingCap | closed until papyrus200, open from it | ported | `locking_cap_opens_at_papyrus200` |
| BridgeTest.getActivePowpegRedeemScript | closed until hop400, open from it | ported | `active_powpeg_redeem_script_opens_at_hop400` |
| BridgeTest.getActiveFederationCreationBlockHeight | closed until iris300, open from it | ported | `active_federation_creation_block_height_opens_at_iris300` |
| BridgeTest.increaseLockingCap | closed until papyrus200, open from it | ported | `increase_locking_cap_opens_at_papyrus200` |
| BridgeTest.isBtcTxHashAlreadyProcessed | from arrowhead600 a CALL or a STATICCALL | ported | `btc_tx_hash_already_processed_takes_a_call_or_staticcall_from_arrowhead600` |
| BridgeTest.receiveHeaders | from arrowhead600 a CALL only | ported | `receive_headers_takes_only_a_call_from_arrowhead600` |
| BridgeTest.receiveHeader | closed until iris300, open from it | ported | `receive_header_opens_at_iris300` |
| BridgeTest.registerBtcTransaction | from arrowhead600 a CALL only | ported | `register_btc_transaction_takes_only_a_call_from_arrowhead600` |
| BridgeTest.releaseBtc | from arrowhead600 a CALL only | ported | `release_btc_takes_only_a_call_from_arrowhead600` |
| BridgeTest.removeLockWhitelistAddress | from arrowhead600 a CALL only | ported | `remove_lock_whitelist_address_takes_only_a_call_from_arrowhead600` |
| BridgeTest.rollbackFederation | from arrowhead600 a CALL only | ported | `rollback_federation_takes_only_a_call_from_arrowhead600` |
| BridgeTest.setLockWhiteListDisableBlockDelay | from arrowhead600 a CALL only | ported | `set_lock_whitelist_disable_block_delay_takes_only_a_call_from_arrowhead600` |
| BridgeTest.updateCollections | from arrowhead600 a CALL only | ported | `update_collections_takes_only_a_call_from_arrowhead600` |
| BridgeTest.voteFeePerKb | from arrowhead600 a CALL only | ported | `vote_fee_per_kb_takes_only_a_call_from_arrowhead600` |
| BridgeTest.registerBtcCoinbaseTransaction | closed until papyrus200, open from it | ported | `register_btc_coinbase_transaction_opens_at_papyrus200` |
| BridgeTest.hasBtcBlockCoinbaseTransactionInformation | closed until papyrus200, open from it | ported | `btc_block_coinbase_transaction_information_opens_at_papyrus200` |
| BridgeTest.registerFastBridgeBtcTransaction | closed until iris300, open from it | ported | `register_fast_bridge_btc_transaction_opens_at_iris300` |
| BridgeTest.getBtcBlockchainBestBlockHeader | closed until iris300, open from it | ported | `btc_blockchain_best_block_header_opens_at_iris300` |
| BridgeTest.getBtcBlockchainBlockHeaderByHash | closed until iris300, open from it | ported | `btc_blockchain_block_header_by_hash_opens_at_iris300` |
| BridgeTest.getBtcBlockchainBlockHeaderByHeight | closed until iris300, open from it | ported | `btc_blockchain_block_header_by_height_opens_at_iris300` |
| BridgeTest.getBtcBlockchainParentBlockHeaderByHash | closed until iris300, open from it | ported | `btc_blockchain_parent_block_header_by_hash_opens_at_iris300` |
| BridgeTest.getNextPegoutCreationBlockNumber | closed until hop400, open from it | ported | `next_pegout_creation_block_number_opens_at_hop400` |
| BridgeTest.getQueuedPegoutsCount | closed until hop400, open from it | ported | `queued_pegouts_count_opens_at_hop400` |
| BridgeTest.getEstimatedFeesForNextPegoutEvent | closed until hop400, open from it | ported | `estimated_fees_for_next_pegout_event_open_at_hop400` |
| BridgeTest.UnionBridgeTest.setUnionBridgeContractAddressForTestnet_beforeRSKIP502_shouldFail | closed before RSKIP502 | ported | `the_union_bridge_methods_are_closed_before_reed810` |
| BridgeTest.UnionBridgeTest.setUnionBridgeContractAddressForTestnet_afterRSKIP502_shouldSetNewAddress | its authorizer sets the address on testnet | ported | `the_union_bridge_contract_address_is_set_by_its_authorizer_on_testnet` |
| BridgeTest.UnionBridgeTest.setUnionBridgeContractAddressForTestnet_afterRSKIP502_emptyArgument_shouldSuccess | no argument sets the zero address | ported | `no_union_bridge_contract_address_sets_the_zero_address` |
| BridgeTest.UnionBridgeTest.getUnionBridgeContractAddress_beforeRSKIP502_shouldFail | closed before RSKIP502 | ported | `the_union_bridge_methods_are_closed_before_reed810` |
| BridgeTest.UnionBridgeTest.getUnionBridgeContractAddress_afterRSKIP502_shouldReturnAddress | answers the network's address | ported | `the_union_bridge_contract_address_is_its_networks` |
| BridgeTest.UnionBridgeTest.getUnionBridgeLockingCap_beforeRSKIP502_shouldFail | closed before RSKIP502 | ported | `the_union_bridge_methods_are_closed_before_reed810` |
| BridgeTest.UnionBridgeTest.getUnionBridgeLockingCap_afterRSKIP502_shouldReturnLockingCap | answers the initial cap | ported | `the_union_bridge_locking_cap_starts_at_one_rbtc` |
| BridgeTest.UnionBridgeTest.increaseUnionBridgeLockingCap_beforeRSKIP502_shouldFail | closed before RSKIP502 | ported | `the_union_bridge_methods_are_closed_before_reed810` |
| BridgeTest.UnionBridgeTest.increaseUnionBridgeLockingCap_afterRSKIP502_whenMeetRequirements_shouldReturnSuccess | its authorizer may double the cap | ported | `the_union_locking_cap_may_be_doubled_by_its_authorizer` |
| BridgeTest.UnionBridgeTest.increaseUnionBridgeLockingCap_afterRSKIP502_whenNewLockingCapSurpassingMaxIncrement_shouldReturnInvalidLockingCapCode | a cap past double is an invalid value | ported | `the_union_locking_cap_may_not_rise_past_double` |
| BridgeTest.UnionBridgeTest.increaseUnionBridgeLockingCap_afterRSKIP502_whenSendingZero_shouldReturnInvalidLockingCapCode | a cap of zero is an invalid value | ported | `a_union_locking_cap_of_zero_is_an_invalid_value` |
| BridgeTest.UnionBridgeTest.increaseUnionBridgeLockingCap_afterRSKIP502_whenUnauthorized_shouldThrowVmException | throws for anyone but its authorizer | ported | `the_union_locking_cap_throws_for_anyone_but_its_authorizer` |
| BridgeTest.UnionBridgeTest.increaseUnionBridgeLockingCap_afterRSKIP502_emptyArgument_shouldFail | no cap is an invalid value | ported | `no_union_locking_cap_is_an_invalid_value` |
| BridgeTest.UnionBridgeTest.requestUnionBridgeRbtc_beforeRSKIP502_shouldFail | closed before RSKIP502 | ported | `the_union_bridge_methods_are_closed_before_reed810` |
| BridgeTest.UnionBridgeTest.releaseUnionBridgeRbtc_beforeRSKIP502_shouldFail | closed before RSKIP502 | ported | `the_union_bridge_methods_are_closed_before_reed810` |
| BridgeTest.UnionBridgeTest.releaseUnionBridgeRbtc_afterRSKIP502_shouldReleaseRbtc | a release of what was requested succeeds and sends nothing back | ported | `a_release_of_what_was_requested_succeeds_and_sends_nothing_back` |
| BridgeTest.UnionBridgeTest.releaseUnionBridgeRbtc_afterRSKIP502_whenFail_shouldReturnErrorResponseCode | a failed release answers its code and sends the value back | ported | `a_failed_release_sends_the_value_back` |
| BridgeTest.UnionBridgeTest.setUnionBridgeTransferPermissions_beforeRSKIP502_shouldFail | closed before RSKIP502 | ported | `the_union_bridge_methods_are_closed_before_reed810` |
| BridgeTest.UnionBridgeTest.getSuperEvent_preRSKIP529_shouldThrowVMException | closed before RSKIP529 | ported | `the_union_bridge_events_are_closed_before_reed810` |
| BridgeTest.UnionBridgeTest.setSuperEvent_preRSKIP529_shouldThrowVMException | closed before RSKIP529 | ported | `the_union_bridge_events_are_closed_before_reed810` |
| BridgeTest.UnionBridgeTest.clearSuperEvent_preRSKIP529_shouldThrowVMException | closed before RSKIP529 | ported | `the_union_bridge_events_are_closed_before_reed810` |
| BridgeTest.UnionBridgeTest.getBaseEvent_preRSKIP529_shouldThrowVMException | closed before RSKIP529 | ported | `the_union_bridge_events_are_closed_before_reed810` |
| BridgeTest.UnionBridgeTest.setBaseEvent_preRSKIP529_shouldThrowVMException | closed before RSKIP529 | ported | `the_union_bridge_events_are_closed_before_reed810` |
| BridgeTest.UnionBridgeTest.clearBaseEvent_preRSKIP529_shouldThrowVMException | closed before RSKIP529 | ported | `the_union_bridge_events_are_closed_before_reed810` |

### The rest of BridgeTest

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| BridgeTest.increaseLockingCap_whenNewLockingCapIsInvalidParameter_shouldThrowVMException | throws for a cap that is no number | ported | `increase_locking_cap_throws_for_a_cap_that_is_no_positive_long` |
| BridgeTest.increaseLockingCap_whenNewLockingCapIsNegativeValue_shouldThrowVMException | throws for a negative cap | ported | `increase_locking_cap_throws_for_a_cap_that_is_no_positive_long` |
| BridgeTest.increaseLockingCap_whenNewLockingCapIsZeroValue_shouldThrowVMException | throws for a cap of zero | ported | `increase_locking_cap_throws_for_a_cap_that_is_no_positive_long` |
| BridgeTest.increaseLockingCap_whenNoArgumentsInTheMethodSignature_shouldThrowVMException | a method signature without the argument throws | not applicable | calls the Java method through a signature the Bridge does not have |
| BridgeTest.receiveHeader_empty_parameter | throws for no argument | ported | `receive_header_throws_for_no_argument` |
| BridgeTest.receiveHeader_bridgeSupport_Exception | a throw inside `BridgeSupport` becomes a VM exception | not applicable | a mock's throw |
| BridgeTest.activeAndRetiringFederationOnly_activeFederationIsNotFromFederateMember_retiringFederationIsNull_throwsVMException | the federations' methods refuse a stranger | covered | `the_federations_methods_turn_a_stranger_down` (`bridge/mod.rs`); `update_collections_throws_for_a_caller_outside_the_federations` |
| BridgeTest.activeAndRetiringFederationOnly_activeFederationIsNotFromFederateMember_retiringFederationIsNotNull_retiringFederationIsNotFromFederateMember_throwsVMException | and a member of neither federation | covered | `the_federations_methods_turn_a_stranger_down` (`bridge/mod.rs`); `register_btc_transaction_throws_for_a_caller_outside_the_federations` |
| BridgeTest.activeAndRetiringFederationOnly_activeFederationIsFromFederateMember_OK | an active federator's call runs | covered | `register_btc_transaction_takes_a_federator_before_iris300`; `receive_headers_takes_a_federator_before_wasabi100` |
| BridgeTest.activeAndRetiringFederationOnly_activeFederationIsNotFromFederateMember_retiringFederationIsNotNull_retiringFederationIsFromFederateMember_OK | a retiring federator's call runs | covered | `add_signature_takes_the_new_federation_once_active_and_the_old_one_either_side` |
| BridgeTest.getEstimatedFeesForPegOutAmount_withAmountBelowMinimum_shouldThrowBridgeIllegalArgumentException | an amount below the minimum peg-out throws | covered | `test_rskip540_estimated_fees_for_pegout_amount_below_minimum_fails` (`executor.rs`) |
| BridgeTest.getActiveFederationCreationTime_returnsCreationTimeInExpectedTimeUnit | the creation time's unit by upgrade | covered | `federation_creation_time_unit_changes_at_rskip419` (`bridge/getters.rs`) |
| BridgeTest.getRetiringFederationCreationTime_returnsCreationTimeInExpectedTimeUnit | the creation time's unit by upgrade | covered | `federation_creation_time_unit_changes_at_rskip419` (`bridge/getters.rs`) |
| BridgeTest.getProposedFederationCreationTime_shouldReturnValueFromSeconds | the proposed federation's creation time in seconds | covered | `proposed_federation_creation_time_is_always_seconds` (`bridge/getters.rs`) |
| BridgeTest.getProposedFederationCreationTime_whenBridgeSupportReturnsEmpty_shouldReturnMinusOne | -1 with no proposed federation | covered | `proposed_federation_getters_report_minus_one_when_absent` (`bridge/getters.rs`) |
| BridgeTest.getProposedFederatorPublicKeyOfType_whenBridgeSupportCallThrowsIOOBE_throwsVMException | a throw inside `BridgeSupport` becomes a VM exception | not applicable | a mock's throw |
| BridgeTest.UnionBridgeTest.executeIfAuthorized_whenUnauthorized_shouldThrowVMException | an authorizer's method throws for anyone else | covered | `the_authorizers_methods_throw_for_anyone_else` (`bridge/union.rs`) |
| BridgeTest.UnionBridgeTest.executeIfTestnetAndAuthorized_whenTestnetButUnauthorized_shouldThrowVMException | and on testnet | covered | `the_authorizers_methods_throw_for_anyone_else` (`bridge/union.rs`) |
| BridgeTest.UnionBridgeTest.executeIfTestnet_shouldThrowVMException | a testnet-only method throws on mainnet | covered | `the_contract_address_is_never_set_on_mainnet` (`bridge/union.rs`) |
| BridgeTest.UnionBridgeTest.executeIfTestnetAndUnauthorized_shouldThrowVMException | and for anyone but its authorizer | covered | `the_authorizers_methods_throw_for_anyone_else` (`bridge/union.rs`) |
| BridgeTest.UnionBridgeTest.executeIfAuthorized_whenAuthorized_shouldExecute, BridgeTest.UnionBridgeTest.executeIfTestnetAndAuthorized_whenTestnetAndAuthorized_shouldExecute | the authorizer's call runs | covered | `the_union_bridge_contract_address_is_set_by_its_authorizer_on_testnet` |
| BridgeTest.UnionBridgeTest.setUnionBridgeContractAddressForTestnet_whenUnauthorized_shouldThrowVMException | throws for anyone but its authorizer | ported | `the_union_bridge_contract_address_throws_for_anyone_but_its_authorizer` |
| BridgeTest.UnionBridgeTest.setUnionBridgeContractAddressForTestnet_whenMainnet_shouldThrowVMException | throws on mainnet | ported | `the_union_bridge_contract_address_is_never_set_on_mainnet` |
| BridgeTest.UnionBridgeTest.requestUnionBridgeRbtc_whenFail_shouldReturnErrorResponseCode | a failed request answers its code | ported | `a_failed_request_sends_nothing` |
| BridgeTest.UnionBridgeTest.requestUnionBridgeRbtc_emptyArgument_shouldReturnInvalidValue | no amount is an invalid value | covered | `an_empty_argument_list_reads_as_zeros`, `a_negative_cap_or_amount_is_an_invalid_value` (`bridge/union.rs`) |
| BridgeTest.UnionBridgeTest.setUnionBridgeTransferPermissions_whenNotAuthorized_shouldReturnUnauthorizedCode | anyone but its authorizer is refused | ported | `the_union_transfer_permissions_throw_for_anyone_but_their_authorizer` |
| BridgeTest.UnionBridgeTest.setUnionBridgeTransferPermissions_whenEmptyArguments_shouldAssumeArgAsFalseAndProcessed | no arguments read as false | covered | `an_empty_argument_list_reads_as_zeros`, `a_flag_looks_only_at_its_low_32_bits` (`bridge/union.rs`) |
| BridgeTest.UnionBridgeTest.requestUnionBridgeRbtc_shouldTransferredRbtc, BridgeTest.UnionBridgeTest.setUnionBridgeTransferPermissions_whenValidArgs_shouldSetTransferPermissions | a request and the permissions, as given | not applicable | a mock's answer; the flows are `bridge/union.rs`'s |
| BridgeTest.UnionBridgeTest.getSuperEvent_shouldExecute, BridgeTest.UnionBridgeTest.setSuperEvent_shouldExecuteData, BridgeTest.UnionBridgeTest.clearSuperEvent_shouldExecute, BridgeTest.UnionBridgeTest.getBaseEvent_shouldExecute, BridgeTest.UnionBridgeTest.setBaseEvent_shouldExecuteData, BridgeTest.UnionBridgeTest.clearBaseEvent_shouldExecute | the events are set, read and cleared | covered | `an_event_is_stored_as_it_came_and_read_back_as_bytes`, `an_absent_event_reads_as_empty_bytes`, `clearing_an_event_deletes_only_its_own_cell` (`bridge/union.rs`) |
| BridgeTest.UnionBridgeTest.setSuperEvent_unionBridgeSupportThrowsIAE_shouldThrowVMException, BridgeTest.UnionBridgeTest.clearSuperEvent_unionBridgeSupportThrowsIAE_shouldThrowVMException, BridgeTest.UnionBridgeTest.setBaseEvent_unionBridgeSupportThrowsIAE_shouldThrowVMException, BridgeTest.UnionBridgeTest.clearBaseEvent_unionBridgeSupportThrowsIAE_shouldThrowVMException | a throw inside the union bridge's support becomes a VM exception | not applicable | a mock's throw; the refusals are `only_the_union_bridge_contract_may_set_or_clear_an_event` and `an_event_may_be_128_bytes_and_no_more` (`bridge/union.rs`) |

## BridgeIT

Several of rskj's `…afterMultikey` and `…AfterRskip87Fork` tests switch one rule on and leave
RSKIP88 off, a combination no network's upgrades make. Their ports run at the upgrade that brings
the rule, where RSKIP88 is on too, so a closed method throws there where rskj's test has it
answer null.

| rskj test | what it asserts | verdict | rustock test |
|---|---|---|---|
| BridgeIT.executeWithFunctionSignatureLengthTooShortBeforeRskip88 | a call too short for a selector answers null | ported | `a_call_too_short_for_a_selector_is_no_call_before_orchid_and_throws_from_it` |
| BridgeIT.executeWithFunctionSignatureLengthTooShortAfterRskip88 | and throws from RSKIP88 | ported | `a_call_too_short_for_a_selector_is_no_call_before_orchid_and_throws_from_it` |
| BridgeIT.executeWithInexistentFunctionBeforeRskip88 | a selector of no method answers null | ported | `a_selector_of_no_method_is_no_call_before_orchid_and_throws_from_it` |
| BridgeIT.executeWithInexistentFunctionAfterRskip88 | and throws from RSKIP88 | ported | `a_selector_of_no_method_is_no_call_before_orchid_and_throws_from_it` |
| BridgeIT.getBtcBlockchainBlockLocatorBeforeRskip89Fork | answers the chain's locator | ported | `the_block_locator_is_the_chains_before_orchid` |
| BridgeIT.getBtcBlockchainBlockLocatorAfterRskip88And89Fork | closed from RSKIP89 | ported | `the_block_locator_is_closed_from_orchid` |
| BridgeIT.getFederatorPublicKey_beforeMultikey | each federator's key | ported | `each_federators_key_is_answered_before_wasabi100` |
| BridgeIT.getFederatorPublicKey_afterMultikey | closed from RSKIP123 | ported | `the_federators_key_is_closed_from_wasabi100` |
| BridgeIT.getFederatorPublicKeyOfType_beforeMultikey | closed before RSKIP123 | ported | `the_federators_key_of_a_type_is_no_call_before_wasabi100` |
| BridgeIT.getFederatorPublicKeyOfType_afterMultikey | each federator's key of each type | ported | `the_federators_key_of_each_type_is_answered_from_wasabi100` |
| BridgeIT.getRetiringFederatorPublicKey_beforeMultikey_withARetiringFederation_shouldReturnTheCorrectRetiringFederatorPublicKeyBytes | a retiring federator's key | ported | `a_retiring_federators_key_is_answered_before_wasabi100` |
| BridgeIT.getRetiringFederatorPublicKey_beforeMultikey_withoutARetiringFederation_shouldReturnAnEmptyByteArray | no retiring federation, an empty key | ported | `no_retiring_federation_has_an_empty_key_before_wasabi100` |
| BridgeIT.getRetiringFederatorPublicKey_afterMultikey | closed from RSKIP123 | ported | `the_retiring_federators_key_is_closed_from_wasabi100` |
| BridgeIT.getRetiringFederatorPublicKeyOfType_beforeMultikey | closed before RSKIP123 | ported | `the_retiring_federators_key_of_a_type_is_no_call_before_wasabi100` |
| BridgeIT.getRetiringFederatorPublicKeyOfType_afterMultikey_withRetiringFederation_shouldReturnTheCorrectRetiringFederationPublicKeys | a retiring federator's key of each type | ported | `a_retiring_federators_key_of_each_type_is_answered_from_wasabi100` |
| BridgeIT.getRetiringFederatorPublicKeyOfType_afterMultikey_withoutARetiringFederation_shouldReturnTheCorrectRetiringFederationPublicKeys | no retiring federation, an empty key of each type | ported | `no_retiring_federation_has_an_empty_key_of_each_type_from_wasabi100` |
| BridgeIT.getPendingFederatorPublicKey_beforeMultikey | each pending federator's key | ported | `each_pending_federators_key_is_answered_before_wasabi100` |
| BridgeIT.getPendingFederatorPublicKey_afterMultikey | closed from RSKIP123 | ported | `the_pending_federators_key_is_closed_from_wasabi100` |
| BridgeIT.getPendingFederatorPublicKeyOfType_beforeMultikey | closed before RSKIP123 | ported | `the_pending_federators_key_of_a_type_is_no_call_before_wasabi100` |
| BridgeIT.getPendingFederatorPublicKeyOfType_afterMultikey | a pending federator's key of each type | ported | `a_pending_federators_key_of_each_type_is_answered_from_wasabi100` |
| BridgeIT.addFederatorPublicKey_beforeMultikey | a key is voted (with no election open, -10) | ported | `a_federator_public_key_is_voted_before_wasabi100` |
| BridgeIT.addFederatorPublicKey_afterMultikey | closed from RSKIP123 | ported | `a_federator_public_key_is_voted_no_more_from_wasabi100` |
| BridgeIT.addFederatorPublicKeyMultikey_beforeMultikey | closed before RSKIP123 | ported | `a_federators_three_keys_are_no_call_before_wasabi100` |
| BridgeIT.addFederatorPublicKeyMultikey_afterMultikey | three keys are voted (with no election open, -10) | ported | `a_federators_three_keys_are_voted_from_wasabi100` |
| BridgeIT.getLockWhitelistEntryByAddressBeforeRskip87And88Fork | closed before RSKIP87 | ported | `a_whitelist_entry_is_no_call_before_orchid` |
| BridgeIT.getLockWhitelistEntryByAddressAfterRskip87Fork | the entries are answered | ported | `whitelist_entries_are_answered_from_orchid` |
| BridgeIT.addLockWhitelistAddressBeforeRskip87Fork | a caller who is no authorizer is turned down | ported | `a_whitelist_address_from_no_authorizer_is_turned_down_before_orchid` |
| BridgeIT.addLockWhitelistAddressAfterRskip87And88Fork | closed from RSKIP87 | ported | `add_lock_whitelist_address_is_closed_from_orchid` |
| BridgeIT.addOneOffLockWhitelistAddressBeforeRskip87And88Fork | closed before RSKIP87 | ported | `a_one_off_whitelist_address_is_no_call_before_orchid` |
| BridgeIT.addOneOffLockWhitelistAddressAfterRskip87Fork | a caller who is no authorizer is turned down | ported | `a_one_off_whitelist_address_from_no_authorizer_is_turned_down_from_orchid` |
| BridgeIT.addUnlimitedLockWhitelistAddressBeforeRskip87And88Fork | closed before RSKIP87 | ported | `an_unlimited_whitelist_address_is_no_call_before_orchid` |
| BridgeIT.addUnlimitedLockWhitelistAddressAfterRskip87Fork | a caller who is no authorizer is turned down | ported | `an_unlimited_whitelist_address_from_no_authorizer_is_turned_down_from_orchid` |
| BridgeIT.testBlock457BridgeCall | a call of five hundred headers decodes and runs | ported | `a_call_of_five_hundred_headers_decodes_and_runs_before_orchid` (the call's 80,068 bytes are built, not copied from rskj's resource) |
| BridgeIT.executeMethodWithOnlyLocalCallsAllowed_nonLocalCallTx_beforeOrchid | a transaction may call a local-only method | ported | `a_local_only_method_takes_a_transaction_before_orchid` |
| BridgeIT.executeMethodWithOnlyLocalCallsAllowed_nonLocalCallTx | from RSKIP88 it throws | ported | `a_local_only_method_throws_for_a_transaction_from_orchid` |
| BridgeIT.getBtcTransactionConfirmationsBeforeWasabi | closed before RSKIP122 | ported | `btc_transaction_confirmations_are_no_call_before_wasabi100` |
| BridgeIT.getBtcTransactionConfirmations_gasCost | its gas: the base, two a byte, the lookups | ported | `the_gas_for_btc_transaction_confirmations_is_two_a_byte_and_the_lookups` |
| BridgeIT.testCallFromContract_afterOrchid | a contract's call of a local-only method fails the contract | ported | `a_contracts_call_of_a_local_only_method_fails_the_contract_before_iris300` |
| BridgeIT.getBtcBlockchainBestChainHeight_beforeRskip220_isMineable | a transaction may call it before RSKIP220 | covered | `local_call_gating_matches_rskj` (`bridge/mod.rs`) |
| BridgeIT.getBtcBlockchainBestChainHeight_afterRskip220_onlyAllowsLocalCalls | local calls only from RSKIP220 | covered | `local_call_gating_matches_rskj` (`bridge/mod.rs`) |
| BridgeIT.receiveHeadersGasCost_beforeDynamicCost | 22,000 and two a byte | ported | `receive_headers_costs_22000_and_two_a_byte_before_wasabi100` |
| BridgeIT.receiveHeadersGasCost_afterDynamicCost | 66,000 and 1,650 a header past the first | ported | `receive_headers_costs_66000_and_1650_a_header_past_the_first_with_rskip124` |
| BridgeIT.receiveHeadersAccess_beforePublic_noAccessIfNotFromFederationMember | a stranger is refused | ported | `receive_headers_turns_a_stranger_down_before_wasabi100` |
| BridgeIT.receiveHeadersAccess_beforePublic_accessIfFromFederationMember | a federator's call runs | ported | `receive_headers_takes_a_federator_before_wasabi100` |
| BridgeIT.receiveHeadersAccess_afterPublic_accessFromAnyone | anyone's call runs | ported | `receive_headers_takes_anyone_from_wasabi100` |

### The rest of BridgeIT

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| BridgeIT.callUpdateCollectionsWithSignatureNotFromFederation | a caller outside the federations is refused | ported | `update_collections_throws_for_a_caller_outside_the_federations` |
| BridgeIT.callUpdateCollectionsWithTransactionsWaitingForConfirmation | a peg-out short of its confirmations stays | ported | `update_collections_moves_one_confirmed_pegout_on_to_be_signed` |
| BridgeIT.callUpdateCollectionsWithTransactionsWaitingForConfirmationWithEnoughConfirmations | one confirmed peg-out moves on to be signed | ported | `update_collections_moves_one_confirmed_pegout_on_to_be_signed` |
| BridgeIT.sendNoBlockHeader | a call with no header receives none | ported | `receive_headers_with_nothing_but_its_selector_receives_no_header` |
| BridgeIT.sendOrphanBlockHeader | an orphan header leaves the chain as it was | ported | `an_orphan_header_leaves_the_chain_as_it_was` |
| BridgeIT.receiveHeadersNotFromTheFederation | a stranger is refused | ported | `receive_headers_turns_a_stranger_down_before_wasabi100` |
| BridgeIT.receiveHeadersWithNonParseableHeader | a header that does not parse is no call | ported | `before_wasabi100_a_header_is_80_to_85_bytes_with_no_transactions` |
| BridgeIT.receiveHeadersWithCorrectSizeHeaders | 80 to 85 bytes are a header | ported | `before_wasabi100_a_header_is_80_to_85_bytes_with_no_transactions` |
| BridgeIT.receiveHeadersWithIncorrectSizeHeaders | other sizes are not | ported | `before_wasabi100_a_header_is_80_to_85_bytes_with_no_transactions` |
| BridgeIT.receiveHeadersWithHugeDeclaredTransactionsSize | a header that declares transactions is refused | ported | `before_wasabi100_a_header_is_80_to_85_bytes_with_no_transactions` |
| BridgeIT.registerBtcTransactionNotFromFederation | a caller outside the federations is refused | ported | `register_btc_transaction_throws_for_a_caller_outside_the_federations` |
| BridgeIT.registerBtcTransactionWithNonParseableTx | a transaction that does not parse | ported | `register_btc_transaction_answers_nothing_for_bytes_it_cannot_read_before_orchid` |
| BridgeIT.registerBtcTransactionWithHugeDeclaredInputsSize | a huge declared input count | ported | `register_btc_transaction_answers_nothing_for_bytes_it_cannot_read_before_orchid` |
| BridgeIT.registerBtcTransactionWithHugeDeclaredOutputsSize | a huge declared output count | ported | `register_btc_transaction_answers_nothing_for_bytes_it_cannot_read_before_orchid` |
| BridgeIT.registerBtcTransactionWithHugeDeclaredWitnessPushCountSize | a huge declared witness push count | ported | `register_btc_transaction_answers_nothing_for_bytes_it_cannot_read_before_orchid` |
| BridgeIT.registerBtcTransactionWithHugeDeclaredWitnessPushSize | a huge declared witness push | ported | `register_btc_transaction_answers_nothing_for_bytes_it_cannot_read_before_orchid` |
| BridgeIT.registerBtcTransactionWithNonParseableMerkleeProof1 | a proof that does not parse | ported | `register_btc_transaction_answers_nothing_for_bytes_it_cannot_read_before_orchid` |
| BridgeIT.registerBtcTransactionWithNonParseableMerkleeProof2 | another | ported | `register_btc_transaction_answers_nothing_for_bytes_it_cannot_read_before_orchid` |
| BridgeIT.registerBtcTransactionWithHugeDeclaredSizeMerkleeProof | a proof of a huge declared size | ported | `register_btc_transaction_answers_nothing_for_bytes_it_cannot_read_before_orchid` |
| BridgeIT.addSignature_fromNewFederator_whenNewAndOldFederationsAndNewFedIsActive_shouldCall | the new federation signs once active | ported | `add_signature_takes_the_new_federation_once_active_and_the_old_one_either_side` |
| BridgeIT.addSignature_fromOldFederator_whenNewAndOldFederationsAndNewFedIsActive_shouldCall | the old one still signs | ported | `add_signature_takes_the_new_federation_once_active_and_the_old_one_either_side` |
| BridgeIT.addSignature_fromOldFederator_whenNewAndOldFederationsAndNewFedIsInactive_shouldCall | the old one signs before | ported | `add_signature_takes_the_new_federation_once_active_and_the_old_one_either_side` |
| BridgeIT.addSignature_fromNewFederator_whenNewAndOldFederationsAndNewFedIsInactive_shouldThrow | the new one, not yet active, is refused | ported | `add_signature_takes_the_new_federation_once_active_and_the_old_one_either_side` |
| BridgeIT.addSignature_fromNewFederator_whenNoOldFederation_shouldCall | a new federation with no old one signs | ported | `add_signature_takes_a_new_federation_with_no_old_one` |
| BridgeIT.addSignature_fromProposedFederator_whenProposedFederation_shouldCall | the proposed federation signs | ported | `add_signature_takes_the_proposed_federation_while_there_is_one` |
| BridgeIT.addSignature_fromProposedFederator_whenNoProposedFederation_shouldThrow | and is refused once there is none | ported | `add_signature_takes_the_proposed_federation_while_there_is_one` |
| BridgeIT.addSignature_fromNotFederatorSigner_shouldThrow | a signer in no federation is refused | ported | `add_signature_throws_for_a_signer_in_no_federation` |
| BridgeIT.addSignatureWithNonParseablePublicKey | a key that does not parse | ported | `add_signature_is_no_call_for_what_it_cannot_read_before_orchid` |
| BridgeIT.addSignatureWithEmptySignatureArray | no signatures | ported | `add_signature_is_no_call_for_what_it_cannot_read_before_orchid` |
| BridgeIT.addSignatureWithNonParseableSignature | a signature that is not DER | ported | `add_signature_is_no_call_for_what_it_cannot_read_before_orchid` |
| BridgeIT.addSignatureWithNonParseableRskTx | a hash that is not 32 bytes | ported | `add_signature_is_no_call_for_what_it_cannot_read_before_orchid` |
| BridgeIT.getGasForDataInvalidFunction, BridgeIT.getGasForDataUpdateCollections, BridgeIT.getGasForDataReceiveHeaders, BridgeIT.getGasForDataRegisterBtcTransaction, BridgeIT.getGasForDataReleaseBtc, BridgeIT.getGasForDataAddSignature, BridgeIT.getGasForDataGSFBRC, BridgeIT.getGasForDataGSFD, BridgeIT.getGasForDataGBBBCH, BridgeIT.getGasForDataGBBBL, BridgeIT.getGasForDataGetFederationAddress, BridgeIT.getGasForDataGetMinimumLockTxValue | a call's gas is its method's cost and two a byte | ported | `the_gas_for_a_call_is_its_methods_cost_and_two_a_byte` |
| BridgeIT.getGasForDataFreeTx | a federator's Bridge transaction is free while Bridge transactions are | covered | `test_free_bridge_tx_executes_with_zero_gas`, `test_bridge_tx_gas0_rejected_after_are_bridge_txs_paid` (`executor.rs`) |
| BridgeIT.isBtcTxHashAlreadyProcessed_exception | a hash that is not one throws | ported | `a_btc_tx_hash_that_is_not_one_throws` |
| BridgeIT.getBtcTxHashProcessedHeight_exception | a hash that is not one throws | ported | `a_btc_tx_hash_that_is_not_one_throws` |
| BridgeIT.getBtcTransactionConfirmationsAfterWasabi_merkleBranchConstructionError | a path longer than its branch throws | ported | `confirmations_throw_for_a_path_longer_than_its_branch` |
| BridgeIT.testCallFromContract_beforeOrchid | a contract's call of a local-only method fails the contract | ported | `a_contracts_call_of_a_local_only_method_fails_the_contract_before_iris300` |
| BridgeIT.localCallOnlyMethodsDefinition | which methods are local-only | ported | `the_local_only_methods_and_the_mineable_ones_are_rskjs` |
| BridgeIT.mineableMethodsDefinition | which methods are mineable | ported | `the_local_only_methods_and_the_mineable_ones_are_rskjs` |
| BridgeIT.sendNoRskTx | a call with no transaction | not applicable | rustock's Bridge always runs in a transaction |
| BridgeIT.exceptionInUpdateCollection, BridgeIT.exceptionInReleaseBtc, BridgeIT.exceptionInGetStateForBtcReleaseClient, BridgeIT.exceptionInGetStateForSvpClient, BridgeIT.exceptionInGetStateForDebugging, BridgeIT.exceptionInGetBtcBlockchainBestChainHeight, BridgeIT.exceptionInGetBtcBlockchainBlockLocator | a throw inside `BridgeSupport` becomes a VM exception | not applicable | a mock's throw |
| BridgeIT.commitFederation_wrongParameterType, BridgeIT.voteFeePerKb_wrongParameterType | a Java argument of the wrong type | not applicable | the ABI decodes each argument as its method's signature says |
| BridgeIT.bridgeSupportIsCreatedOnInit, BridgeIT.precompiledContractAddress | the contract's set-up and address | not applicable | Java wiring |
| BridgeIT.getBtcTransactionConfirmationsAfterWasabi_ok, BridgeIT.getBtcTransactionConfirmationsAfterWasabi_errorInBridgeSupport | the arguments reach `BridgeSupport`, and its throw | not applicable | a mock's answer and a mock's throw |
| BridgeIT.getFederationAddress, BridgeIT.getMinimumLockTxValue, BridgeIT.isBtcTxHashAlreadyProcessed_normalFlow, BridgeIT.getBtcTxHashProcessedHeight_normalFlow, BridgeIT.getFederationSize, BridgeIT.getFederationThreshold, BridgeIT.getFederationCreationBlockNumber, BridgeIT.getRetiringFederationSize, BridgeIT.getRetiringFederationThreshold, BridgeIT.getRetiringFederationCreationBlockNumber, BridgeIT.getRetiringFederationAddress_withNoRetiringFederation_shouldReturnEmptyString, BridgeIT.getRetiringFederationAddress_withRetiringFederation_shouldReturnTheCorrectRetiringFederationAddress, BridgeIT.getPendingFederationSize, BridgeIT.createFederation, BridgeIT.commitFederation_ok, BridgeIT.rollbackFederation, BridgeIT.getLockWhitelistSize, BridgeIT.getLockWhitelistAddress, BridgeIT.removeLockWhitelistAddress, BridgeIT.getFeePerKb, BridgeIT.voteFeePerKb_ok, BridgeIT.executeMethodWithOnlyLocalCallsAllowed_localCallTx, BridgeIT.executeMethodWithAnyCallsAllowed_localCallTx, BridgeIT.executeMethodWithAnyCallsAllowed_nonLocalCallTx, BridgeIT.getBtcBlockchainBlockHashAtDepth, BridgeIT.getBtcBlockchainInitialBlockHeight | a method passes its arguments to `BridgeSupport` and answers what it gets | not applicable | a mock's answer, no refusal, limit or window |

## BridgeSupportFlyoverTest

rskj's tests call `BridgeSupport.registerFlyoverBtcTransaction`; the ports call the Bridge's
`registerFastBridgeBtcTransaction` from the LBC, a contract, at iris300 where a test has RSKIP293
off and hop400 where it has it on. The Bitcoin addresses the call names are of the test's network.
Where rskj's tests make the active federation the network's genesis keys as a standard multisig
(`getGenesisFederationLegacy`), the ports store those keys so, as the new federation. Where they
leave the locking cap off or mock it away, it is on and the Bridge holds all the bitcoin there is.

| rskj test | what it asserts | verdict | rustock test |
|---|---|---|---|
| BridgeSupportFlyoverTest.FlyoverPeginThatSurpassesLockingCap.registerFlyoverBtcTransaction_whenAmountRequestedSurpassesLockingCap_beforeRSKIP305_justEmitsPegoutTransactionCreatedEvent | over the cap, the refund is logged with the value it spends, which is not stored | ported | `before_rskip305_a_refund_over_the_cap_is_logged_and_its_values_not_stored` |
| BridgeSupportFlyoverTest.FlyoverPeginThatSurpassesLockingCap.registerFlyoverBtcTransaction_whenAmountRequestedSurpassesLockingCap_afterRSKIP305_processReleaseRejectionTransactionInfo | and from RSKIP305 the value is stored | ported | `from_rskip305_a_refund_over_the_cap_is_logged_and_its_values_stored` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_output_to_bech32_before_RSKIP293_regtest | a bech32 output throws before RSKIP293 | fixed | `before_rskip293_an_output_bitcoinj_cannot_read_as_an_address_is_a_generic_error` (see [Differences found](#differences-found)) |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_output_to_bech32_before_RSKIP293_mainnet | the same on mainnet | fixed | `before_rskip293_an_output_bitcoinj_cannot_read_as_an_address_is_a_generic_error` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_output_to_bech32_after_RSKIP293_regtest | from RSKIP293 a bech32 output is passed over | ported | `from_rskip293_outputs_to_other_addresses_are_passed_over` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_output_to_bech32_after_RSKIP293_mainnet | the same on mainnet | ported | `from_rskip293_outputs_to_other_addresses_are_passed_over` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_multiple_output_to_all_address_type_before_RSKIP293_regtest | a payment that also has a bech32 output throws before RSKIP293 | fixed | `before_rskip293_an_output_bitcoinj_cannot_read_as_an_address_is_a_generic_error` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_multiple_output_to_all_address_type_after_RSKIP293_regtest | from RSKIP293 only the federation's output counts | ported | `from_rskip293_outputs_to_other_addresses_are_passed_over` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_multiple_output_to_all_address_type_before_RSKIP293_mainnet | the same on mainnet | fixed | `before_rskip293_an_output_bitcoinj_cannot_read_as_an_address_is_a_generic_error` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_multiple_output_to_all_address_type_after_RSKIP293_mainnet | the same on mainnet | ported | `from_rskip293_outputs_to_other_addresses_are_passed_over` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_amount_sent_is_below_minimum_before_RSKIP293_activation | before RSKIP293 an amount below the minimum peg-in is taken | ported | `before_rskip293_any_amount_to_the_active_federation_is_taken` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_amount_sent_is_below_minimum_after_RSKIP293_activation | from RSKIP293 it is answered -305 | ported | `from_rskip293_a_payment_must_reach_the_minimum_peg_in` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_amount_sent_is_equal_to_minimum | the minimum itself is taken | ported | `from_rskip293_a_payment_must_reach_the_minimum_peg_in` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_amount_sent_is_over_minimum | more is taken | ported | `from_rskip293_a_payment_must_reach_the_minimum_peg_in` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_funds_sent_to_current_retiring_fed_before_RSKIP293_activation | before RSKIP293 a payment to the retiring federation alone is of no value | ported | `before_rskip293_what_pays_the_retiring_federation_does_not_count` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_funds_sent_to_active_and_retiring_fed_before_RSKIP293_activation | and to both, only the active federation's part counts | ported | `before_rskip293_what_pays_the_retiring_federation_does_not_count` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_funds_sent_to_active_and_retiring_fed_after_RSKIP293_activation | from RSKIP293 both parts count | ported | `from_rskip293_what_pays_the_retiring_federation_counts` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_funds_sent_to_active_fed_after_RSKIP293 | a payment to the active federation is taken | ported | `from_rskip293_a_payment_to_the_active_federation_is_taken` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_funds_sent_to_retiring_fed_after_RSKIP293 | from RSKIP293 a payment to the retiring federation is taken | ported | `from_rskip293_what_pays_the_retiring_federation_counts` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_sum_of_funds_sent_to_active_and_retiring_fed_surpasses_locking_cap_before_RSKIP293_no_refund | before RSKIP293 the retiring federation's part does not count against the cap | ported | `before_rskip293_only_the_active_federations_part_counts_against_the_cap` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_funds_sent_to_active_and_retiring_fed_surpasses_locking_cap_before_RSKIP293_only_funds_sent_to_active_fed_should_be_refunded_to_LP | over the cap, the active federation's part is refunded to the LP | ported | `before_rskip293_only_the_active_federations_part_counts_against_the_cap` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_funds_sent_to_active_and_retiring_fed_surpasses_locking_cap_before_RSKIP293_only_funds_sent_to_active_fed_should_be_refunded_to_user | or to the user | ported | `before_rskip293_only_the_active_federations_part_counts_against_the_cap` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_funds_sent_to_retiring_fed_surpasses_locking_cap_should_refund_user_after_RSKIP293 | from RSKIP293 the retiring federation's part over the cap is refunded | ported | `from_rskip293_both_federations_parts_count_against_the_cap` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_sum_of_funds_sent_to_active_and_retiring_fed_surpasses_locking_cap_should_refund_lp_after_RSKIP293 | and both parts together | ported | `from_rskip293_both_federations_parts_count_against_the_cap` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_sum_of_all_utxos_surpass_locking_cap_but_not_funds_sent_to_fed_should_not_refund_after_RSKIP293 | what pays elsewhere does not count against the cap | ported | `from_rskip293_both_federations_parts_count_against_the_cap` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_funds_sent_to_random_addresses_surpass_locking_cap_should_not_refund_after_RSKIP293 | a payment to other addresses alone is of no value | ported | `from_rskip293_both_federations_parts_count_against_the_cap` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_failed_when_tx_with_witness_already_saved_in_storage | a payment with a witness is registered once | ported | `a_payment_with_a_witness_is_registered_once` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_failed_when_tx_with_witness_surpassing_locking_already_saved_in_storage | and once refunded, not again | ported | `a_refunded_payment_with_a_witness_is_not_registered_again` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_failed_when_tx_with_witness_surpassing_locking_already_saved_in_storage_and_then_send_same_without_witness | nor without its witness | ported | `a_payment_refunded_with_a_witness_is_not_registered_without_it` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_failed_when_tx_without_witness_surpassing_locking_already_saved_in_storage | a refunded payment is not registered again | ported | `a_refunded_payment_is_not_registered_again` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_failed_when_tx_without_witness_surpassing_locking_already_saved_in_storage_and_then_send_same_tx_with_witness | with a witness and a proof the chain does not hold, it fails the validations | ported | `a_refunded_payment_again_with_a_witness_fails_the_validations` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_failed_when_tx_without_witness_already_saved_in_storage | a payment is registered once | ported | `a_payment_is_registered_once` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_sender_is_not_lbc | a contract other than the LBC is answered -301 | ported | `a_registration_from_another_contract_than_the_lbc_is_turned_down` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_validationsForRegisterBtcTransaction_returns_false | a payment the chain cannot confirm is answered -303 | ported | `a_payment_the_chain_cannot_confirm_fails_the_validations` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_amount_sent_is_0 | a payment of nothing is answered -304 | ported | `a_payment_of_nothing_is_of_no_value` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_surpasses_locking_cap_and_shouldTransfer_is_true | over the cap, refunded to the LP | ported | `over_the_cap_a_payment_is_refunded_to_the_lp_or_the_user` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_surpasses_locking_cap_and_shouldTransfer_is_false | or to the user | ported | `over_the_cap_a_payment_is_refunded_to_the_lp_or_the_user` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_surpasses_locking_cap_and_tries_to_register_again | a refunded payment is not registered again under a higher cap | ported | `a_payment_refunded_over_the_cap_is_not_registered_again` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_OK | a payment is registered and its value goes to the LBC | ported | `a_payment_is_registered_and_its_value_goes_to_the_lbc` |
| BridgeSupportFlyoverTest.createFlyoverFederationInformation_OK | the Flyover address is the P2SH of the hash pushed and dropped before the federation's redeem script | covered | `flyover_redeem_script_and_p2sh_groundtruth` (`bridge/peg.rs`) |
| BridgeSupportFlyoverTest.getFlyoverWallet_ok | a bitcoinj wallet of the Flyover outputs holds their value | not applicable | rustock has no wallet object; the refund it is for is held by `over_the_cap_a_payment_is_refunded_to_the_lp_or_the_user` and the cap tests |
| BridgeSupportFlyoverTest.getFlyoverDerivationHash_ok | from RSKIP284 the derivation hash is the Keccak-256 of the arguments' hash, the user's address, the LBC and the LP's address | ported | `the_derivation_hash_is_of_the_arguments_the_addresses_and_the_lbc`, with `flyover_derivation_hash_groundtruth` (`bridge/peg.rs`) |
| BridgeSupportFlyoverTest.saveFlyoverDataInStorage_OK | the federation's output, the hash marked used and the Flyover address's information are stored | ported | `a_payment_is_registered_and_its_value_goes_to_the_lbc` |

### The rest of BridgeSupportFlyoverTest

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_is_not_contract | a call that is not a contract's is answered -300 | ported | `a_registration_not_from_a_contract_is_turned_down` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_output_to_P2PKH_regtest, BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_output_to_P2PKH_mainnet, BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_output_to_P2SH_regtest, BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_output_to_P2SH_mainnet, BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_output_to_different_address_type_regtest, BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_output_to_different_address_type_mainnet, BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_multiple_output_to_the_same_address_regtest, BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_multiple_output_to_the_same_address_mainnet, BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_multiple_output_to_different_addresses_regtest, BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_multiple_output_to_different_addresses_mainnet | from RSKIP293 each output to either federation counts and no other does | ported | `from_rskip293_what_pays_either_federation_counts_and_nothing_else` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_multiple_output_to_random_addresses_regtest, BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_multiple_output_to_random_addresses_mainnet | a payment to other addresses alone is of no value | ported | `a_payment_to_other_addresses_alone_is_of_no_value` |
| BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_sent_multiple_output_and_one_with_amount_below_minimum_mainnet, BridgeSupportFlyoverTest.registerFlyoverBtcTransaction_sent_multiple_output_and_one_with_amount_below_minimum_regtest | one output to a federation a satoshi short of the minimum turns the payment down | ported | `from_rskip293_one_output_below_the_minimum_turns_the_payment_down` |
| BridgeSupportFlyoverTest.FlyoverPeginThatSurpassesLockingCap.registerFlyoverBtcTransaction_fundsThatSurpassLockingCapSentToP2shErpActiveAndP2shErpRetiringFeds_shouldSetRedeemDataInInputsScriptSig, BridgeSupportFlyoverTest.FlyoverPeginThatSurpassesLockingCap.registerFlyoverBtcTransaction_fundsThatSurpassLockingCapSentToP2shP2wshErpActiveAndP2shErpRetiringFeds_shouldSetFixedScriptSig_shouldSetRedeemDataInInputsWitness | a refund of what both federations were paid spends each output with its own federation's Flyover redeem script, in the active federation's format: last in each input's script where both are P2SH-ERP, and with a P2SH-P2WSH active federation last in each witness, the retiring P2SH-ERP federation's input too | ported | `a_refund_from_two_p2sh_erp_federations_spends_each_with_its_own_flyover_script`, `a_refund_built_for_a_p2sh_p2wsh_federation_carries_both_redeem_scripts_in_witnesses` |
