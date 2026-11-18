//! rskj's `BridgeSupportProcessFundsMigrationTest`: here its tests of
//! RSKIP294 (hop400). Before it a migration spends every UTXO of the
//! retiring federation, however many; from it, at most
//! `maxInputsPerPegoutTransaction`. rskj's listed tests run under iris300;
//! the ports run mainnet's Bridge at iris300 and, for the contrast rskj's
//! other tests draw, at hop400, with sixty retiring UTXOs, more than
//! mainnet's fifty inputs.

use super::support::*;

const MANY: u8 = 60;

fn max_inputs() -> usize {
    BridgeConstants::mainnet().max_inputs_per_pegout_transaction as usize
}

/// What is left of the retiring federation's UTXOs.
fn retiring_utxos(collections: &mut Collections) -> usize {
    crate::bridge::storage::load_old_federation_utxos(&mut collections.bridge.ctx).len()
}

/// Whether the retiring federation is still stored.
fn retiring_federation(collections: &mut Collections) -> bool {
    named(&mut collections.bridge.ctx, "oldFederation").is_some_and(|stored| !stored.is_empty())
}

/// rskj's `StandardMultisigFederationTest.updateCollections_duringMigration_preRSKIP294_withMoreUtxosThanMaxInputs_shouldUseAllRetiringUtxos`:
/// in the migration's window, before RSKIP294 the migration spends all
/// sixty UTXOs and the retiring federation stays; from it, fifty.
#[test]
fn before_rskip294_a_migration_in_its_window_spends_every_retiring_utxo() {
    for (number, spent) in [
        (MAINNET_IRIS300, usize::from(MANY)),
        (MAINNET_HOP400, max_inputs()),
    ] {
        let mut collections = Collections::migrating(number, STANDARD_MULTISIG, MANY, false);
        collections.update();
        let waiting = collections.waiting();
        assert_eq!(waiting.len(), 1, "{number}");
        assert_eq!(waiting[0].input.len(), spent, "{number}");
        assert_eq!(
            retiring_utxos(&mut collections),
            usize::from(MANY) - spent,
            "{number}"
        );
        assert!(retiring_federation(&mut collections), "{number}");
    }
}

/// rskj's `StandardMultisigFederationTest.updateCollections_pastMigrationAge_preRSKIP294_withMoreUtxosThanMaxInputs_shouldUseAllRetiringUtxos`,
/// and from the rest of the class
/// `updateCollections_pastMigrationAge_withMoreUtxosThanMaxInputs_shouldCreateLastMigrationTxWithMaxInputsAndClearRetiringFedEvenIfUtxosRemain`:
/// past the migration's age the last migration spends all sixty UTXOs
/// before RSKIP294, fifty from it, and the retiring federation is cleared
/// either way.
#[test]
fn before_rskip294_the_last_migration_spends_every_retiring_utxo() {
    for (number, spent) in [
        (MAINNET_IRIS300, usize::from(MANY)),
        (MAINNET_HOP400, max_inputs()),
    ] {
        let mut collections = Collections::migrating(number, STANDARD_MULTISIG, MANY, true);
        collections.update();
        let waiting = collections.waiting();
        assert_eq!(waiting.len(), 1, "{number}");
        assert_eq!(waiting[0].input.len(), spent, "{number}");
        assert_eq!(
            retiring_utxos(&mut collections),
            usize::from(MANY) - spent,
            "{number}"
        );
        assert!(!retiring_federation(&mut collections), "{number}");
    }
}
