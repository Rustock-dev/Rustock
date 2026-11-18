# Parity: rskj's peg-in and federation-state tests

rskj VETIVER-9.0.4's tests of the rules an upgrade turns on or off in the
Bridge's peg-ins, the Bitcoin coinbase registration, the Bridge's gas costs,
the lock whitelist and the federations' stored state: each listed test, what
it asserts, and where rustock holds it. The ports are under
`crates/execution/src/rskj_tests/bridge_pegins/` and
`crates/execution/src/rskj_tests/bridge_federation_state/`.

Verdicts:

- **ported**: a test there holds the case.
- **covered**: a test rustock already had holds it.
- **not applicable**: the case cannot arise, in rskj or rustock, on a real
  chain; the row says why.
- **fixed**: rustock answered otherwise; it is put right, and the port passes.
  The difference is under [Differences found](#differences-found).

rskj's tests mock what they read: the activations one rule at a time, the
storage, the sender a peg-in is read to be from. The ports read the Bridge's
storage as it keeps it, run on an upgrade ladder, and spend real inputs.
Where rskj mocks a rule on that no ladder has on without others, the case
runs on the ladder up to the upgrade that brings it, and the row says so.

## Differences found

Both are fixed, and their ports pass.

1. **The active federation's creation height, once the next federation is
   active.** From the block a new federation becomes active, rskj's
   `getActiveFederationCreationBlockHeight` answers that federation's creation
   height; rustock answered the active height kept, or 0, until
   `updateCollections` kept the next height as the active one.
   - rskj: `rskj-core/src/main/java/co/rsk/peg/federation/FederationSupportImpl.java`,
     lines 906-922.
   - rustock, before the fix: `get_active_federation_creation_block_height`
     read only the active height kept.
   - Port: `bridge_federation_state::federation_support_impl_test::once_the_new_federation_is_active_its_creation_height_is_answered`.
2. **RSKIP375: a confirmed peg-out under a key the set waiting for
   signatures already has.** From RSKIP375 rskj refuses to keep it there:
   `processConfirmedPegouts` throws, and `updateCollections` fails, leaving
   both sets as they were. rustock kept the confirmed peg-out under the key
   in place of the one there. Two peg-outs made in one `updateCollections`
   (a migration and a batch) share their creation hash, which RSKIP375 keys
   the set by.
   - rskj: `rskj-core/src/main/java/co/rsk/peg/BridgeSupport.java`, lines
     1600-1646 (`processConfirmedPegouts`,
     `checkIfEntryExistsInPegoutsWaitingForSignatures`), and
     `rskj-core/src/main/java/co/rsk/peg/Bridge.java`, lines 493-502.
   - rustock, before the fix: `update_collections` put the confirmed peg-out
     into the set waiting for signatures without looking for the key.
   - Ports: `bridge_pegins::bridge_support_test::collections::from_rskip375_a_confirmed_pegout_under_a_key_already_kept_fails_the_update`
     and `...::from_rskip375_a_second_pegout_of_one_creation_waits_until_the_first_is_signed`.

## Counts

| class | listed | ported | covered | not applicable | fixed |
|---|---|---|---|---|---|
| BridgeSupportTest | 97 | 86 | 5 | 4 | 2 |
| FederationStorageProviderImplTests | 10 | 7 | 0 | 3 | 0 |
| PeginInformationTest | 9 | 9 | 0 | 0 | 0 |
| FederationSupportImplTest | 7 | 1 | 3 | 3 | 0 |
| BridgeSerializationUtilsTest | 5 | 5 | 0 | 0 | 0 |
| VoteFederationChangeTest | 5 | 5 | 0 | 0 | 0 |
| BridgeSupportRejectedPeginTest | 4 | 4 | 0 | 0 | 0 |
| BridgeCostsTest | 3 | 3 | 0 | 0 | 0 |
| BitcoinUtilsTest | 3 | 2 | 1 | 0 | 0 |
| WhitelistStorageProviderImplTest | 3 | 3 | 0 | 0 | 0 |
| RepositoryBtcBlockStoreWithCacheTest | 2 | 1 | 1 | 0 | 0 |
| BridgeConstantsTest | 1 | 1 | 0 | 0 | 0 |
| FederationConstantsTest | 1 | 1 | 0 | 0 | 0 |
| **all** | **150** | **128** | **10** | **10** | **2** |

One more difference came from the rest of `FederationSupportImplTest`
(above, 1); it is fixed, and its port passes. One row of that class moved
from not applicable to covered, by the test written with the fix.

## co.rsk.peg.BridgeSupportTest

Module: `rskj_tests::bridge_pegins::bridge_support_test` (`locking_cap`,
`events`, `collections`, `sender_types`, `merkle_roots`, `coinbase`,
`chain_work`, `release_info`). RSKIP134, RSKIP143 and RSKIP146 come at
Papyrus200, RSKIP170, RSKIP176, RSKIP181 and RSKIP186 at Iris300, RSKIP271
at Hop400, RSKIP326 and RSKIP375 at Fingerroot500, RSKIP434 at Arrowhead631,
RSKIP305 at Reed800.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| `registerBtcTransaction_before_RSKIP134_activation_sends_above_lockingcap` | before RSKIP134 a legacy peg-in above the locking cap is credited | ported | `locking_cap::before_rskip134_a_peg_in_above_the_locking_cap_is_credited` (Wasabi100) |
| `registerBtcTransaction_before_RSKIP134_activation_sends_exactly_lockingcap` | credited | ported | `locking_cap::before_rskip134_a_peg_in_up_to_the_locking_cap_is_credited` |
| `registerBtcTransaction_before_RSKIP134_activation_sends_below_lockingcap` | credited | ported | `locking_cap::before_rskip134_a_peg_in_below_the_locking_cap_is_credited` |
| `registerBtcTransaction_after_RSKIP134_activation_sends_above_lockingcap` | refunded to the sender from the peg-in's outputs, in order | ported | `locking_cap::from_rskip134_a_peg_in_above_the_locking_cap_is_refunded` (Papyrus200) |
| `registerBtcTransaction_after_RSKIP134_activation_sends_exactly_lockingcap` | credited | ported | `locking_cap::from_rskip134_a_peg_in_up_to_the_locking_cap_is_credited` |
| `registerBtcTransaction_after_RSKIP134_activation_sends_below_lockingcap` | credited | ported | `locking_cap::from_rskip134_a_peg_in_below_the_locking_cap_is_credited` |
| `eventLoggerLogLockBtc_before_rskip_146_activation` | no `lock_btc` | ported | `events::before_rskip146_a_credited_peg_in_logs_no_lock_btc` |
| `eventLoggerLogLockBtc_after_rskip_146_activation` | `lock_btc` | ported | `events::from_rskip146_to_rskip170_a_credited_peg_in_logs_lock_btc` |
| `eventLoggerLogPeginRejectionEvents_before_rskip_181_activation` | no `rejected_pegin` or `unrefundable_pegin` for an undetermined sender | ported | `pegin_information_test::before_rskip379_a_peg_in_with_neither_a_sender_nor_instructions_is_refused`, its Papyrus200 case: rskj mocks RSKIP170 on and RSKIP181 off, and both come at Iris300 |
| `eventLoggerLogPeginRejectionEvents_after_rskip_181_activation` | both events | ported | the same test, its Iris300 case |
| `eventLoggerLogPeginBtc_before_rskip_170_activation` | `lock_btc`, not `pegin_btc` | ported | `events::from_rskip146_to_rskip170_a_credited_peg_in_logs_lock_btc` |
| `eventLoggerLogPeginBtc_after_rskip_170_activation` | `pegin_btc`, not `lock_btc` | ported | `events::from_rskip170_a_credited_peg_in_logs_pegin_btc` |
| `registerBtcTransactionLockTxNotWhitelisted_before_rskip_146_activation` | three peg-ins of a sender not whitelisted, to the active, the retiring and both federations, refunded without a creation hash; no `release_requested` | ported | `events::before_rskip146_peg_ins_of_a_sender_not_whitelisted_are_refunded` |
| `registerBtcTransactionLockTxNotWhitelisted_after_rskip_146_activation` | refunded with the creation hash; `release_requested` | ported | `events::from_rskip146_peg_ins_of_a_sender_not_whitelisted_are_refunded` |
| `registerBtcTransaction_sending_segwit_tx_twice_locks_just_once` | marked processed by the id without the witness; registered again, credited once | ported | `events::a_segwit_peg_in_registered_twice_is_credited_once` |
| `callProcessFundsMigration_is_migrating_before_rskip_146_activation` | the migration waits without its creation hash; no `release_requested` | ported | `collections::before_rskip146_a_migration_waits_without_its_transactions_hash` |
| `callProcessFundsMigration_is_migrating_after_rskip_146_activation` | with its creation hash; `release_requested` of one bitcoin | ported | `collections::from_rskip146_a_migration_waits_with_its_transactions_hash_and_is_logged` |
| `callProcessFundsMigration_is_migrated_before_rskip_146_activation` | as `is_migrating`, which rskj sets up alike | ported | `collections::before_rskip146_a_migration_waits_without_its_transactions_hash` |
| `callProcessFundsMigration_is_migrated_after_rskip_146_activation` | as `is_migrating` | ported | `collections::from_rskip146_a_migration_waits_with_its_transactions_hash_and_is_logged` |
| `updateFederationCreationBlockHeights_before_rskip_186_activation` | `updateCollections` leaves the creation heights alone | ported | `collections::before_rskip186_update_collections_leaves_the_creation_heights_alone` |
| `updateFederationCreationBlockHeights_after_rskip_186_activation` | the next height, its federation active, becomes the active one and is cleared | ported | `collections::from_rskip186_update_collections_makes_the_next_creation_height_the_active_one` |
| `rskTxWaitingForSignature_uses_updateCollection_rskTxHash_before_rskip_146_activation` | kept under the `updateCollections` transaction | ported | `collections::before_rskip146_a_confirmed_pegout_is_kept_under_the_update_transaction` |
| `rskTxWaitingForSignature_postRSKIP326_emitNewPegoutConfirmedEvent` | `pegout_confirmed` | ported | `collections::from_rskip326_a_confirmed_pegout_is_logged` |
| `rskTxWaitingForSignature_preRSKIP326_noPegoutConfirmedEventEmitted` | no `pegout_confirmed` | ported | `collections::before_rskip326_a_confirmed_pegout_is_not_logged` (Hop400) |
| `rskTxWaitingForSignature_postRSKIP326NoTxWithEnoughConfirmation_pegoutConfirmedEventNotEmitted` | none confirmed, none logged | ported | `collections::from_rskip326_a_pegout_not_yet_confirmed_is_not_logged` |
| `rskTxWaitingForSignature_uses_updateCollection_rskTxHash_after_rskip_146_activation_if_release_transaction_doesnt_have_rstTxHash` | with no creation hash, kept under the `updateCollections` transaction | ported | `collections::from_rskip146_a_confirmed_pegout_of_no_creation_hash_is_kept_under_the_update_transaction` |
| `rskTxWaitingForSignature_uses_release_transaction_rstTxHash_after_rskip_146_activation` | kept under its creation hash | ported | `collections::from_rskip146_a_confirmed_pegout_is_kept_under_its_creation_transaction`; rskj's creation hash is zero, no transaction's is, so the port's is not |
| `rskTxWaitingForSignature_uses_updateCollection_rskTxHash_after_rskip_176_activation` | kept under the `updateCollections` transaction again | ported | `collections::from_rskip176_a_confirmed_pegout_is_kept_under_the_update_transaction_again` |
| `rskTxWaitingForSignature_fail_adding_an_already_existing_key_after_rskip_375` | a second peg-out of one creation hash fails `updateCollections`, both sets unchanged, until the first is signed | fixed | `collections::from_rskip375_a_second_pegout_of_one_creation_waits_until_the_first_is_signed`: Differences found, 2 |
| `rskTxWaitingForSignature_uses_pegoutCreation_rskTxHash_after_rskip_375_activation` | kept under its creation hash, on regtest, testnet and mainnet | ported | `collections::from_rskip375_a_confirmed_pegout_is_kept_under_its_creation_transaction` |
| `rskTxWaitingForSignature_override_entry_is_allowed_before_rskip_375_activation` | takes the place of the peg-out under its key, on each network | ported | `collections::before_rskip375_a_confirmed_pegout_takes_the_place_of_one_under_its_key` (Hop400) |
| `rskTxWaitingForSignature_override_entry_attempt_after_rskip_375_activation` | throws | fixed | `collections::from_rskip375_a_confirmed_pegout_under_a_key_already_kept_fails_the_update`: Differences found, 2 |
| `when_registerBtcTransaction_sender_not_recognized_before_rskip170_no_lock_and_no_refund` | not credited, not refunded, not processed | ported | `sender_types::before_rskip170_a_peg_in_without_a_sender_is_left_alone` (Papyrus200) |
| `when_registerBtcTransaction_sender_not_recognized_after_rskip170_lock` | credited where its instructions say | ported | `sender_types::from_rskip170_a_peg_in_without_a_sender_is_credited_where_its_instructions_say` |
| `when_registerBtcTransaction_usesLegacyType_beforeFork_lock_and_no_refund` | a whitelisted P2PKH sender credited, its entry used up | ported | `sender_types::before_rskip143_a_whitelisted_p2pkh_sender_is_credited` (Wasabi100) |
| `when_registerBtcTransaction_usesLegacyType_afterFork_notWhitelisted_no_lock_and_refund` | refunded | ported | `sender_types::from_rskip143_a_p2pkh_sender_not_whitelisted_is_refunded` (Papyrus200) |
| `when_registerBtcTransaction_usesLegacyType_afterFork_lock_and_no_refund` | credited | ported | `sender_types::from_rskip143_a_whitelisted_p2pkh_sender_is_credited` |
| `when_registerBtcTransaction_usesSegCompatibilityType_beforeFork_no_lock_and_no_refund` | a P2SH-P2WPKH sender left alone | ported | `sender_types::before_rskip143_a_p2sh_p2wpkh_sender_is_left_alone` |
| `when_registerBtcTransaction_usesSegCompatibilityType_afterFork_lock_and_no_refund` | credited | ported | `sender_types::from_rskip143_a_whitelisted_p2sh_p2wpkh_sender_is_credited` |
| `when_registerBtcTransaction_usesSegCompatibilityType_afterFork_notWhitelisted_no_lock_and_refund` | refunded to the sender's address: rskj's real `P2shP2wpkhBtcLockSender` reads it as a P2SH address | ported | `sender_types::from_rskip143_a_p2sh_p2wpkh_sender_not_whitelisted_is_refunded` |
| `when_registerBtcTransaction_usesMultisigType_beforeFork_no_lock_and_no_refund` | left alone | ported | `sender_types::before_rskip143_a_multisig_sender_is_left_alone` |
| `when_registerBtcTransaction_usesMultisigType_afterFork_no_lock_and_refund` | refunded to the multisig's P2SH address | ported | `sender_types::from_rskip143_a_multisig_sender_is_refunded` |
| `when_registerBtcTransaction_usesMultisigWithWitnessType_beforeFork_no_lock_and_no_refund` | left alone | ported | `sender_types::before_rskip143_a_p2sh_p2wsh_multisig_sender_is_left_alone` |
| `when_registerBtcTransaction_usesMultisigWithWitnessType_afterFork_no_lock_and_refund` | refunded to the P2SH-P2WSH address | ported | `sender_types::from_rskip143_a_p2sh_p2wsh_multisig_sender_is_refunded` |
| `registerBtcTransaction_rejects_tx_with_witness_before_rskip_143_activation` | a segwit transaction throws | ported | `merkle_roots::before_rskip143_a_segwit_transaction_fails_its_registration` |
| `registerBtcTransaction_accepts_lock_tx_with_witness_after_rskip_143_activation` | proven by the coinbase's witness root, credited | ported | `merkle_roots::from_rskip143_a_segwit_peg_in_proven_by_its_coinbase_is_credited` |
| `registerBtcTransaction_rejects_tx_with_witness_and_unregistered_coinbase_after_rskip_143_activation` | not processed | ported | `merkle_roots::from_rskip143_a_segwit_transaction_no_coinbase_proves_is_not_registered` |
| `registerBtcTransaction_rejects_tx_with_witness_and_unqual_witness_root_after_rskip_143_activation` | not processed | ported | `merkle_roots::from_rskip143_a_segwit_transaction_of_another_witness_root_is_not_registered` |
| `registerBtcTransaction_rejects_tx_without_witness_unequal_roots_after_rskip_143` | not processed | ported | `merkle_roots::from_rskip143_a_transaction_of_another_root_is_not_registered` |
| `registerBtcTransaction_accepts_lock_tx_without_witness_after_rskip_143_activation` | credited | ported | `merkle_roots::from_rskip143_a_peg_in_proven_by_its_blocks_root_is_credited` |
| `registerBtcTransaction_accepts_lock_tx_version1_after_rskip_170_activation` | the destination credited, not the sender | ported | `sender_types::from_rskip170_a_peg_in_is_credited_where_its_instructions_say` |
| `registerBtcTransaction_ignores_pegin_instructions_before_rskip_170_activation` | the sender credited, not the destination | ported | `sender_types::before_rskip170_a_peg_in_is_credited_to_its_sender_whatever_its_instructions_say` |
| `when_registerBtcTransaction_invalidPeginProtocolVersion_afterFork_no_lock_and_refund` | refunded to the sender | ported | `sender_types::from_rskip170_a_peg_in_of_an_unknown_version_is_refunded` |
| `isBlockMerkleRootValid_equal_merkle_roots` | valid | ported | `merkle_roots::the_blocks_merkle_root_proves_the_transaction`, through `getBtcTransactionConfirmations` |
| `isBlockMerkleRootValid_unequal_merkle_roots_before_rskip_143` | not valid | ported | `merkle_roots::a_root_other_than_the_blocks_proves_nothing` |
| `isBlockMerkleRootValid_coinbase_information_null_after_rskip_143` | not valid | ported | `merkle_roots::from_rskip143_a_witness_root_no_coinbase_registered_proves_nothing` |
| `isBlockMerkleRootValid_coinbase_information_not_null_and_unequal_mroots_after_rskip_143` | not valid | ported | `merkle_roots::from_rskip143_a_witness_root_other_than_the_registered_one_proves_nothing` |
| `isBlockMerkleRootValid_coinbase_information_not_null_and_equal_mroots_after_rskip_143` | valid | ported | `merkle_roots::from_rskip143_a_branch_to_the_registered_witness_root_proves_the_transaction` |
| `getBtcTransactionConfirmations_rejects_tx_with_witness_before_rskip_143` | -5 | ported | `merkle_roots::before_rskip143_a_branch_from_the_witness_hash_proves_nothing` |
| `getBtcTransactionConfirmations_accepts_tx_with_witness_after_rskip_143` | the confirmations | ported | `merkle_roots::from_rskip143_a_branch_to_the_registered_witness_root_proves_the_transaction` |
| `getBtcTransactionConfirmations_unregistered_coinbase_after_rskip_143` | -5 | ported | `merkle_roots::from_rskip143_a_witness_root_no_coinbase_registered_proves_nothing` |
| `getBtcTransactionConfirmations_registered_coinbase_unequal_witnessroot_after_rskip_143` | -5 | ported | `merkle_roots::from_rskip143_a_witness_root_other_than_the_registered_one_proves_nothing` |
| `getBtcTransactionConfirmations_tx_without_witness_unequal_roots_after_rskip_143` | -5 | ported | `merkle_roots::a_root_other_than_the_blocks_proves_nothing` |
| `getBtcTransactionConfirmations_accepts_tx_without_witness_after_rskip_143` | the confirmations | ported | `merkle_roots::the_blocks_merkle_root_proves_the_transaction` |
| `when_RegisterBtcCoinbaseTransaction_wrong_witnessReservedValue_noSent` | a reserved value not of 32 bytes throws | not applicable | through the Bridge the reserved value is an ABI `bytes32` (`BridgeMethods.java` 710-719), always 32 bytes |
| `when_RegisterBtcCoinbaseTransaction_MerkleTreeWrongFormat_noSent` | a tree rskj cannot read throws, nothing kept | ported | `coinbase::a_coinbase_by_a_tree_rskj_cannot_read_fails_its_registration` |
| `when_RegisterBtcCoinbaseTransaction_HashNotInPmt_noSent` | nothing kept, no throw | ported | `coinbase::a_coinbase_by_a_tree_of_another_transaction_is_not_kept` |
| `when_RegisterBtcCoinbaseTransaction_notVerify_noSent` | a coinbase bitcoinj does not verify throws | covered | `bridge/tx.rs` `a_coinbase_bitcoinj_does_not_verify_is_refused` |
| `when_RegisterBtcCoinbaseTransaction_not_equal_merkle_root_noSent` | nothing kept, no throw | ported | `coinbase::a_coinbase_in_a_block_of_another_merkle_root_is_not_kept` |
| `when_RegisterBtcCoinbaseTransaction_null_stored_block_noSent` | a block not held throws | ported | `coinbase::a_coinbase_in_a_block_not_held_fails_its_registration` |
| `registerBtcCoinbaseTransaction` | the witness root kept | covered | `bridge/tx.rs` `a_coinbase_whose_commitment_matches_is_registered` |
| `hasBtcCoinbaseTransaction_before_rskip_143_activation` | false | not applicable | the method opens at RSKIP143 in rskj (`BridgeMethods.java` 721-732) and in rustock (`bridge/mod.rs`, `Since(Papyrus200)`); before it a call names no method |
| `hasBtcCoinbaseTransaction_after_rskip_143_activation` | true | ported | `coinbase::a_block_has_coinbase_information_once_it_is_kept` |
| `hasBtcCoinbaseTransaction_fails_with_null_coinbase_information_after_rskip_143_activation` | false | ported | `coinbase::a_block_has_coinbase_information_once_it_is_kept` |
| `validationsForRegisterBtcTransaction_tx_without_inputs_before_rskip_143` | throws | covered | `bridge/peg.rs` `a_transaction_without_inputs_is_what_rskj_counts` and `a_crafted_pegin_without_inputs_fails_the_transaction` |
| `validationsForRegisterBtcTransaction_tx_without_inputs_after_rskip_143` | throws | covered | the same tests |
| `registerPegIn_version0_tx_no_lockable_by_invalid_sender` | a multisig sender refunded | ported | `sender_types::from_rskip143_a_multisig_sender_is_refunded` |
| `registerPegIn_version0_tx_no_lockable_by_surpassing_locking_cap` | refunded | ported | `sender_types::from_rskip134_a_legacy_peg_in_past_the_locking_cap_is_refunded` |
| `registerPegIn_version1_tx_no_lockable_by_surpassing_locking_cap` | refunded to the sender | ported | `sender_types::from_rskip170_a_peg_in_past_the_locking_cap_is_refunded_to_its_sender` |
| `registerPegIn_version1_tx_no_lockable_by_surpassing_locking_cap_unknown_sender_with_refund_address` | refunded to the refund address | ported | `sender_types::from_rskip170_a_peg_in_without_a_sender_past_the_locking_cap_is_refunded_where_it_says` |
| `registerPegIn_version1_tx_no_lockable_by_surpassing_locking_cap_unknown_sender_without_refund_address` | not refunded | ported | `sender_types::from_rskip170_a_peg_in_without_a_sender_or_refund_address_past_the_locking_cap_is_not_refunded` |
| `registerPegIn_errorParsingPeginInstructions_beforeRskip170_dontRefundSender` | not refunded | ported | `sender_types::before_rskip170_unreadable_instructions_are_not_refunded` |
| `registerPegIn_errorParsingPeginInstructions_afterRskip170_refundSender` | refunded to the sender | ported | `sender_types::from_rskip170_unreadable_instructions_are_refunded_to_the_sender` |
| `ChainWorkTests.receiveHeader_beforeRSKIP434_returnsUnexpectedExceptionResponseCodeAndDoesNotSaveTheBlock` | mainnet's 849,138: -99, not kept | ported | `chain_work::before_rskip434_receive_header_does_not_take_mainnets_block_849138` |
| `ChainWorkTests.receiveHeaders_beforeRSKIP434_savesBlocksUntilReachesBlockWithTooMuchChainWork` | 849,135 to 849,137 kept, not 849,138 or 849,139 | ported | `chain_work::before_rskip434_receive_headers_stops_at_mainnets_block_849138` |
| `ChainWorkTests.receiveHeader_afterRSKIP434_returnsSuccessfulAndSavesTheBlock` | 0, kept | ported | `chain_work::from_rskip434_receive_header_takes_mainnets_block_849138` |
| `ChainWorkTests.receiveHeaders_afterRSKIP434_savesAllBlocks` | all kept | ported | `chain_work::from_rskip434_receive_headers_takes_mainnets_blocks_from_849138` |
| `ChainWorkTests.receiveHeader_networkNotMainnet_returnsSuccessfulAndSavesTheBlock` | testnet and regtest, before RSKIP434 and from it: 0, kept | ported | `chain_work::off_mainnet_receive_header_takes_block_849138` |
| `ChainWorkTests.receiveHeaders_networkNotMainnet_savesAllBlocks` | all kept | ported | `chain_work::off_mainnet_receive_headers_takes_the_blocks_from_849138` |
| `ReleaseTransactionInfo.updateCollections_whenReleasesInQueue_beforeRSKIP305_justEmitsPegoutTransactionCreatedEvent` | a batch logs `pegout_transaction_created`, keeps no outpoint values | ported | `release_info::before_rskip305_a_batch_is_logged_with_what_it_spends_and_nothing_is_kept` |
| `ReleaseTransactionInfo.updateCollections_whenReleasesInQueueAndLegacyFed_afterRSKIP305_shouldSetRedeemDataInScriptSigAndProcessReleaseTransactionInfo` | a batch's scriptSig holds the redeem script, outpoint values kept | ported | `release_info::from_rskip305_a_legacy_federations_batch_keeps_what_it_spends_under_its_hash` |
| `ReleaseTransactionInfo.registerBtcTransaction_forRefundableLegacyPeginFromMultisig_beforeRSKIP305_justEmitsPegoutTransactionCreatedEvent` | the refund logs `release_requested` and `pegout_transaction_created`, keeps no outpoint values | ported | `release_info::before_rskip305_a_refund_is_logged_with_what_it_spends_and_nothing_is_kept` (Lovell700) |
| `ReleaseTransactionInfo.registerBtcTransaction_forRefundableLegacyPeginFromMultisig_afterRSKIP305_processReleaseTransactionsInfo` | the outpoint values kept under the refund's hash | ported | `release_info::from_rskip305_what_a_refund_spends_is_kept_under_its_hash` (Reed800, Vetiver900) |
| `getNextPegoutCreationBlockNumber_before_RSKIP271_activation` | 0 | not applicable | the method opens at RSKIP271 in rskj (`BridgeMethods.java` 792-803) and in rustock (`bridge/mod.rs`, Hop400); before it a call names no method |
| `getNextPegoutCreationBlockNumber_after_RSKIP271_activation` | the height kept | ported | `collections::the_next_pegout_creation_block_answered_is_the_one_kept` |
| `getQueuedPegoutsCount_before_RSKIP271_activation` | 0 | not applicable | as above (`BridgeMethods.java` 804-815); `bridge_costs_test::a_call_that_names_no_method_costs_23000_and_nothing_for_its_data` holds that the call names no method at mainnet's block 4,598,499 |
| `getQueuedPegoutsCount_after_RSKIP271_activation` | the count | covered | `bridge/peg.rs` `queued_pegouts_count_sums_both_queues` |

Counts: 97 listed; 84 ported, 5 covered, 4 not applicable, 2 differ, 2 not
gotten to.

Rest of the class, read from its first listed test (line 1434) to its end.
Ported beyond its listed tests: the
one-off whitelist entry is used up when the locking cap turns a legacy
peg-in down, as rskj's whitelist uses it up before the cap is looked at
(`WhitelistSupportImpl.java` 154-162), held in every refused row of
`locking_cap`. Not ported: its tests of rules no upgrade changes, among
them `validationsForRegisterBtcTransaction`'s refusals (a negative height,
too few confirmations, a tree rskj cannot read, a hash not in the tree, a
block of another merkle root), `receiveHeader`'s answers (too soon after the
last, too old, a parent not held, a block already held, an unexpected
exception) and `registerPegIn_noPeginInstructions`.

## co.rsk.peg.federation.FederationStorageProviderImplTests

Module: `rskj_tests::bridge_federation_state::federation_storage_provider_impl_tests`.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| `saveNewFederation_before_RSKIP123_should_not_save_null` | a null new federation is not written | not applicable | no flow of rskj's or rustock's sets the new federation to null |
| `saveNewFederation_after_RSKIP123_should_not_save_null` | the same | not applicable | the same |
| `testSaveNewFederation` | the new federation written as rskj writes it in each era | ported | `a_committed_federation_is_kept_as_the_new_one_as_rskj_keeps_it_in_each_era` |
| `saveOldFederation_before_RSKIP123_should_save_null` | a cleared old federation, without a format | ported | `before_rskip123_the_retired_federation_is_cleared_without_a_format` |
| `saveOldFederation_after_RSKIP123_should_save_null` | a cleared old federation, with the standard format | ported | `from_rskip123_the_retired_federation_is_cleared_with_a_standard_format` |
| `getNewFederationBtcUTXOs` | the cell of the network and era read | ported | `the_active_federations_utxos_are_read_from_the_cell_of_the_network_and_era` |
| `saveNewFederationBtcUTXOs_utxosShouldBeSavedToStorage` | the cell of the network and era written | ported | `the_active_federations_utxos_are_written_in_the_cell_of_the_network_and_era` |
| `testSaveOldFederation` | the old federation written as rskj writes it in each era | ported | `the_federation_committed_over_is_kept_as_the_old_one_as_rskj_keeps_it_in_each_era` |
| `ProposedFederationTests.saveProposedFederation_preRSKIP419_whenProposedFederationIsSet_shouldNotSave` | no proposed federation written | ported | `before_rskip419_no_proposed_federation_is_kept` |
| `ProposedFederationTests.getProposedFederation_preRSKIP419_whenProposedFederationIsSet_shouldReturnEmpty` | none read | not applicable | the proposed federation's getters open at RSKIP419, and before it none is written, which the row above holds |

Counts: 10 listed; 7 ported, 3 not applicable. Rest of the class read; the
clearing of a retired federation is held through `updateCollections`, as
it clears it, before and from RSKIP123.

## co.rsk.peg.PeginInformationTest

Module: `rskj_tests::bridge_pegins::pegin_information_test`.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| `parse_fromPeginInstructions` | destination and refund address from the instructions | ported | `from_rskip170_the_instructions_give_the_destination_and_the_refund_address` |
| `parse_fromPeginInstructions_withoutBtcLockSender` | the same with no sender | ported | `with_no_sender_the_instructions_give_the_destination_and_the_refund_address` |
| `parse_fromPeginInstructions_withoutBtcRefundAddress` | the sender's address for the refund | ported | `instructions_without_a_refund_address_leave_the_senders` |
| `parse_fromPeginInstructions_withoutBtcLockSender_withoutBtcRefundAddress` | no refund address | ported | `with_no_sender_and_no_refund_address_in_the_instructions_there_is_none` |
| `parse_fromPeginInstructions_invalidProtocolVersion` | an invalid payload | ported | `instructions_of_a_version_other_than_1_are_an_invalid_payload` |
| `parse_withoutBtcLockSender_withoutPeginInstructions` | refused | ported | `before_rskip379_a_peg_in_with_neither_a_sender_nor_instructions_is_refused` |
| `parse_withBtcLockSender_withPeginInstructions_preIris` | the instructions not read | ported | `before_rskip170_the_instructions_are_not_read` |
| `parse_withoutBtcLockSender_withPeginInstructions_preIris` | no sender, nothing | ported | `before_rskip170_instructions_do_not_stand_in_for_a_sender` |
| `parse_bech32_withPeginInstructions` | a bech32 sender on each side of RSKIP379 | ported | `a_peg_in_from_a_bech32_sender_is_read_as_rskj_reads_it_on_each_side_of_rskip379` |

Counts: 9 listed; 9 ported. Rest of the class read: `parse_fromBtcLockSender`
is a legacy peg-in credited to its sender, which the other ports hold.

## co.rsk.peg.federation.FederationSupportImplTest

Module: `rskj_tests::bridge_federation_state::federation_support_impl_test`.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| `ActiveFederationTestsWithNullFederations.getActiveFederationRedeemScript_beforeRSKIP293_returnsEmpty` | empty | not applicable | only `getActivePowpegRedeemScript` reads it, and it opens at RSKIP293 in rskj and rustock (`bridge/mod.rs`, Hop400) |
| `ActiveFederationTestsWithNullFederations.getActiveFederationRedeemScript_afterRSKIP293_returnsGenesisFederationRedeemScript` | the genesis federation's script | covered | `bridge/governance.rs` `the_active_powpeg_redeem_script_is_rskjs` |
| `ActiveFederationTestsWithNullOldFederation.getActiveFederationRedeemScript_beforeRSKIP293_returnsEmpty` | empty | not applicable | as above |
| `ActiveFederationTestsWithNullOldFederation.getActiveFederationRedeemScript_afterRSKIP293_returnsNewFederationRedeemScript` | the new federation's script | covered | `bridge/governance.rs` `the_active_powpeg_redeem_script_is_rskjs` |
| `ActiveFederationTestsWithNonNullFederations.getActiveFederationRedeemScript_beforeRSKIP293_returnsEmpty` | empty | not applicable | as above |
| `ActiveFederationCreationBlockHeightTests.getActiveFederationCreationBlockHeight_beforeRSKIP186_returnsZero` | 0 | covered | `bridge_methods::bridge_test::before_rskip186_the_active_federation_creation_block_height_is_zero`, written with the RSKIP186 fix (the Bridge's method opens at RSKIP186, and the getter below it answers 0) |
| `UpdateFederationCreationBlockHeightTests.updateFederationCreationBlockHeights_beforeRSKIP186_doesNothing` | the heights left alone | ported | `before_rskip186_the_creation_heights_are_left_as_they_are` |

Counts: 7 listed; 1 ported, 3 covered, 3 not applicable. Rest of the class
read; ported from it, of `getActiveFederationCreationBlockHeight`:
`before_the_new_federation_is_active_the_active_creation_height_is_answered`,
which agrees, and
`once_the_new_federation_is_active_its_creation_height_is_answered`, which
differed (Differences found, 1) and passes since the fix.

## co.rsk.peg.BridgeSerializationUtilsTest

Module: `rskj_tests::bridge_federation_state::bridge_serialization_utils_test`.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| `SerializeAndDeserializeFederation.serializeAndDeserializeNonStandardErpFederation_preRSKIP284_testnet` | read back with testnet's hardcoded script | ported | `on_testnet_before_rskip284_the_federation_reads_back_with_the_hardcoded_script` |
| `SerializeAndDeserializeFederation.serializeAndDeserializeNonStandardErpFederation_postRSKIP284_preRSKIP293_testnet` | its own script | ported | `on_testnet_from_rskip284_until_rskip293_the_federation_reads_back_with_its_own_script` |
| `SerializeAndDeserializeFederation.serializeAndDeserializeNonStandardErpFederation_preRSKIP293_mainnet` | its own script | ported | `on_mainnet_before_rskip293_the_federation_reads_back_with_its_own_script` |
| `SerializeAndDeserializeFederation.serializeAndDeserializeNonStandardErpFederation_postRSKIP293_testnet` | the delay written as a Java number | ported | `on_testnet_from_rskip293_the_federation_reads_back_with_the_delay_as_a_java_number` |
| `SerializeAndDeserializeFederation.serializeAndDeserializeNonStandardErpFederation_postRSKIP293_mainnet` | the same on mainnet | ported | `on_mainnet_from_rskip293_the_federation_reads_back_with_the_delay_as_a_java_number` |

Counts: 5 listed; 5 ported.

## co.rsk.peg.federation.VoteFederationChangeTest

Module: `rskj_tests::bridge_federation_state::vote_federation_change_test`.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| `voteCommitFederation_preRSKIP305_commitFederationWithOver10Members_throwsException` | a commit of eleven members throws | ported | `before_rskip305_a_commit_of_eleven_members_throws` |
| `voteCommitFederation_postRSKIP305_commitFederationWith20Members_shouldNotThrow` | twenty members committed | ported | `from_rskip305_a_commit_of_twenty_members_is_proposed` |
| `voteCommitFederation_preRSKIP186_whenPendingFederationIsSet_shouldPerformLegacyCommitFederationActionsButNotSetFederationChangeInfo` | the legacy hand-over, no change information | ported | `before_rskip186_a_commit_hands_over_without_the_federation_change_information` |
| `voteCommitFederation_postRSKIP186_preRSKIP419_whenPendingFederationIsSet_shouldPerformLegacyCommitFederationActions` | the hand-over with the change information | ported | `from_rskip186_until_rskip419_a_commit_hands_over_with_the_federation_change_information` |
| `voteCommitFederation_postRSKIP419_whenPendingFederationIsSet_shouldPerformCommitFederationActions` | the federation proposed, nothing handed over | ported | `from_rskip419_a_commit_proposes_the_federation_and_hands_nothing_over` |

Counts: 5 listed; 5 ported.

## co.rsk.peg.BridgeSupportRejectedPeginTest

Module: `rskj_tests::bridge_pegins::bridge_support_rejected_pegin_test`.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| `registerBtcTransaction_whenBelowTheMinimum_shouldRejectPegin` | rejected, not refunded, marked as RSKIP459 to RSKIP551 mark it | ported | `a_peg_in_below_the_minimum_is_turned_down_without_a_refund` |
| `registerBtcTransaction_whenUndeterminedSender_shouldRejectPegin` | the same for an undetermined sender | ported | `a_peg_in_from_an_undetermined_sender_is_turned_down_without_a_refund` |
| `registerBtcTransaction_whenPeginV1WithInvalidPayloadAndUnderminedSender_shouldRejectPegin` | an invalid payload from no sender | ported | `a_peg_in_whose_instructions_do_not_parse_from_no_sender_is_not_refunded` |
| `registerBtcTransaction_whenUtxoToActiveFedBelowMinimumAndUtxoToRetiringFedAboveMinimum_shouldRejectPegin` | what each live federation is paid counts | ported | `only_what_live_federations_are_paid_counts_toward_the_minimum` |

Counts: 4 listed; 4 ported. Rest of the class read; ported from it:
`a_payment_to_a_flyover_address_is_left_alone` and
`a_transaction_that_pays_no_federation_is_left_alone`.

## co.rsk.peg.BridgeCostsTest

Module: `rskj_tests::bridge_pegins::bridge_costs_test`.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| `receiveHeadersGasCost_beforeDynamicCost` | 22,000 and the data's cost | ported | `before_rskip124_receive_headers_costs_22000_whatever_it_carries` |
| `receiveHeadersGasCost_afterDynamicCost` | 66,000 and 1,650 a header | ported | `from_rskip124_until_rskip132_receive_headers_costs_66000_and_1650_a_header` |
| `receiveHeadersGasCost_afterDynamicCost_review` | 25,000 and 3,500 a header | ported | `from_rskip132_receive_headers_costs_25000_and_3500_a_header` |

Counts: 3 listed; 3 ported. Rest of the class read; ported from it:
`a_call_that_names_no_method_costs_23000_and_nothing_for_its_data`.

## co.rsk.peg.bitcoin.BitcoinUtilsTest

Module: `rskj_tests::bridge_pegins::bitcoin_utils_test`.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| `getMultiSigTransactionHashWithoutSignaturesBeforeRSKIP305_fromP2PKH_withParseableScriptPubKey_shouldThrowIAE` | before RSKIP305 a P2PKH input throws | covered | `bridge/peg.rs` `svp_hashes_strip_as_rskj_before_and_after_rskip305` |
| `FindWitnessCommitment.findWitnessCommitment_whenMalFormedTxBeforeRSKIP460_shouldThrowException` | a malformed coinbase throws | ported | `before_rskip460_a_coinbase_with_a_script_bitcoinj_cannot_parse_is_refused` |
| `FindWitnessCommitment.findWitnessCommitment_whenMalFormedTxAfterRSKIP460_shouldIgnoreMalformedOutputAndReturnFoundWitnessCommitment` | the malformed output skipped, the commitment found | ported | `from_rskip460_the_unparseable_output_is_skipped_and_the_commitment_found` |

Counts: 3 listed; 2 ported, 1 covered.

## co.rsk.peg.whitelist.WhitelistStorageProviderImplTest

Module: `rskj_tests::bridge_pegins::whitelist_storage_provider_impl_test`.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| `save_whenLockWhiteListIsNotNullAndRSKIP87IsNotActive_shouldReturnSavedValue` | the one-off cell written, not the unlimited one | ported | `before_rskip87_a_one_off_entry_is_saved_and_the_unlimited_cell_is_not` |
| `save_whenIsActiveRSKIP87_shouldSavedUnlimitedLockWhitelist` | each entry in its cell | ported | `from_rskip87_a_one_off_and_an_unlimited_entry_are_saved_each_in_its_cell` |
| `getWhitelist_whenIsActiveRSKIP87_shouldReturnUnlimitedEntries` | both cells read | ported | `from_rskip87_the_whitelist_read_holds_both_cells_entries` |

Counts: 3 listed; 3 ported.

## co.rsk.peg.RepositoryBtcBlockStoreWithCacheTest

Module: `rskj_tests::bridge_pegins::repository_btc_block_store_with_cache_test`.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| `getStoredBlockAtMainChainHeight_postIris_heightLowerThanMaxDepth_limitInBtcHeightWhenBlockIndexActivates` | looks no lower than the index's height | covered | `bridge/btc_chain.rs` `below_the_index_activation_rskj_looks_only_so_deep` |
| `getStoredBlockAtMainChainHeight_postIris_heightLowerThanMaxDepth_limitInChainHeadMinusMaxDepthToSearch` | below the search depth under the head, throws | ported | `from_rskip199_below_the_search_depth_under_the_head_the_lookup_throws` |

Counts: 2 listed; 1 ported, 1 covered.

## co.rsk.peg.constants.BridgeConstantsTest

Module: `rskj_tests::bridge_pegins::bridge_constants_test`.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| `getMinimumPeginTxValue` | the legacy minimum before RSKIP219, the new one from it | ported | `the_minimum_peg_in_is_the_legacy_one_before_rskip219_and_the_new_one_from_it` |

Counts: 1 listed; 1 ported.

## co.rsk.peg.federation.constants.FederationConstantsTest

Module: `rskj_tests::bridge_federation_state::federation_constants_test`.

| rskj test | what it asserts | verdict | rustock test or reason |
|---|---|---|---|
| `getFundsMigrationAgeSinceActivationEnd` | mainnet's later end between RSKIP357 and RSKIP374 | ported | `the_migration_ends_later_on_mainnet_between_rskip357_and_rskip374` |

Counts: 1 listed; 1 ported.
