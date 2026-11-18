//! rskj's `BridgeSupportProcessFundsMigrationTest`: `updateCollections`
//! moving the retiring federation's funds to the active one.
//!
//! rskj's tests run every upgrade from block 0, with the active federation
//! created at block 100. Here it is created at a mainnet height in the era a
//! test is about, and `updateCollections` is called where rskj's tests call
//! it: one block into the migration age (`duringMigrationBlockNumber`) or at
//! its end (`pastMigrationBlockNumber`). The federations are rskj's
//! builders': 'member01' on retires and 'newActiveMember-0' on is active.

use super::support::*;
use crate::bridge::federation::{
    rsk_address_from_public_key, serialize_federation_multikey, StoredMember,
};
use crate::bridge::peg::{
    build_federation_redeem_script, federation_output_hash160, federation_redeem_for_format_pub,
    load_pegout_confirmation_set, p2sh_output_script_program,
};
use crate::bridge::serialization::rlp_encode_u64;
use crate::bridge::storage::{
    bridge_load_bytes_named, bridge_store_bytes_named, bridge_store_u256,
    load_old_federation_utxos, store_old_federation_utxos, BridgeUtxo, FEE_PER_KB_KEY,
    NEW_FEDERATION_FORMAT_VERSION_KEY, NEW_FEDERATION_KEY, OLD_FEDERATION_FORMAT_VERSION_KEY,
    OLD_FEDERATION_KEY,
};
use alloy_primitives::U256;

/// rskj's test's fee per kB.
const FEE_PER_KB: u64 = 8_000;
/// The least a P2SH output may carry (`MIN_NON_DUST_VALUE_FOR_P2SH_OUTPUT_SCRIPT`).
const MIN_NON_DUST: u64 = 2_700;
const COIN: u64 = 100_000_000;
/// Mainnet's federation activation age before RSKIP383 (fingerroot500).
const LEGACY_AGE: u64 = 18_500;
/// Mainnet's federation activation age from RSKIP383.
const AGE: u64 = 40_320;
/// How long after the activation age mainnet's funds migrate, outside
/// RSKIP357's special case.
const MIGRATION_END: u64 = 10_585;

/// A federation: its members, sorted as rskj sorts them, and its format
/// version.
struct Federation {
    members: Vec<StoredMember>,
    format: u64,
}

impl Federation {
    /// Members whose keys are those of `seeds` (`getBtcEcKeysFromSeeds`).
    fn of(seeds: impl Iterator<Item = String>, format: u64) -> Self {
        let mut members: Vec<StoredMember> = seeds
            .map(|seed| StoredMember::from_btc(key_from_seed(&seed)))
            .collect();
        members.sort();
        Self { members, format }
    }

    fn keys(&self) -> Vec<[u8; 33]> {
        self.members.iter().map(|m| m.btc).collect()
    }
}

/// The builders' members, 'member01' on.
fn retiring(count: usize, format: u64) -> Federation {
    Federation::of((1..=count).map(|i| format!("member{i:02}")), format)
}

/// The test's active members, 'newActiveMember-0' on.
fn active(count: usize, format: u64) -> Federation {
    Federation::of((0..count).map(|i| format!("newActiveMember-{i}")), format)
}

/// UTXOs of `values` paid to `federation` as block `number` reads it, in
/// transactions whose hashes sort as the values are listed.
fn utxos(federation: &Federation, number: u64, values: &[u64]) -> Vec<BridgeUtxo> {
    let redeem = federation_redeem_for_format_pub(
        &federation.keys(),
        federation.format,
        &BridgeConstants::mainnet(),
        &RskHardforkConfig::mainnet(),
        number,
    );
    let script = p2sh_output_script_program(&federation_output_hash160(&redeem, federation.format));
    values
        .iter()
        .zip(1u32..)
        .map(|(&value_satoshis, n)| {
            let mut tx_hash = [0u8; 32];
            tx_hash[..4].copy_from_slice(&n.to_be_bytes());
            BridgeUtxo {
                tx_hash,
                vout: 0,
                value_satoshis,
                height: 1,
                script: script.clone(),
                coinbase: false,
            }
        })
        .collect()
}

/// The Bridge at block `number` of mainnet: `active` created at block
/// `created`, `retiring` before it holding `held`, the fee per kB at
/// `fee_per_kb`, and a member of `active` to call.
fn mainnet_at(
    number: u64,
    created: u64,
    active: &Federation,
    retiring: &Federation,
    held: &[BridgeUtxo],
    fee_per_kb: u64,
) -> Bridge {
    let mut bridge = Bridge::new(
        RskHardforkConfig::mainnet(),
        BridgeConstants::mainnet(),
        number,
    );
    let ctx = &mut bridge.ctx;
    bridge_store_u256(ctx, FEE_PER_KB_KEY, U256::from(fee_per_kb));
    for (key, version_key, federation, block) in [
        (
            NEW_FEDERATION_KEY,
            NEW_FEDERATION_FORMAT_VERSION_KEY,
            active,
            created,
        ),
        (
            OLD_FEDERATION_KEY,
            OLD_FEDERATION_FORMAT_VERSION_KEY,
            retiring,
            1,
        ),
    ] {
        let stored = serialize_federation_multikey(&federation.members, 100_000_000_000, block);
        bridge_store_bytes_named(ctx, key, &stored);
        bridge_store_bytes_named(ctx, version_key, &rlp_encode_u64(federation.format));
    }
    store_old_federation_utxos(ctx, held);
    bridge.caller = rsk_address_from_public_key(&active.members[0].rsk).unwrap();
    bridge
}

/// What a call of `updateCollections` leaves.
struct After {
    /// Whether the call threw.
    failed: bool,
    /// The migration transactions waiting for confirmations.
    migrations: Vec<bitcoin::Transaction>,
    /// Whether the retiring federation is still there.
    retiring_kept: bool,
    /// How many UTXOs it holds.
    retiring_utxos: usize,
}

fn update_collections(mut bridge: Bridge) -> After {
    let result = bridge.execute(&selector_only("updateCollections()"));
    let failed = threw(&result);
    assert!(failed || result.is_ok(), "{result:?}");
    let migrations = load_pegout_confirmation_set(&mut bridge.ctx, true)
        .iter()
        .map(|entry| bitcoin::consensus::deserialize(&entry.btc_tx_raw).unwrap())
        .collect();
    After {
        failed,
        migrations,
        retiring_kept: !bridge_load_bytes_named(&mut bridge.ctx, OLD_FEDERATION_KEY).is_empty(),
        retiring_utxos: load_old_federation_utxos(&mut bridge.ctx).len(),
    }
}

/// rskj's `…withNewFederationAgeBeforeMigrationBegins…`,
/// `…duringMigration_withOneSpendableRetiringUtxo…`,
/// `…duringMigration_withBalanceBelowThreshold…` and
/// `…pastMigrationAge_withOneSpendableRetiringUtxo…`, between P2SH-P2WSH
/// federations: funds migrate from the block after the activation age when
/// they are more than half the fee per kB, and the federation retires at the
/// migration age's end.
#[test]
fn a_retiring_federations_funds_migrate_after_the_activation_age() {
    let created = 9_000_000;
    let (active, retiring) = (active(20, 4000), retiring(20, 4000));
    let begins = created + AGE;
    for (at, value, migrations, kept) in [
        (begins, COIN, 0, true),
        (begins + 1, COIN, 1, true),
        (begins + 1, FEE_PER_KB / 2, 0, true),
        (begins + MIGRATION_END, COIN, 1, false),
    ] {
        let held = utxos(&retiring, at, &[value]);
        let after = update_collections(mainnet_at(
            at, created, &active, &retiring, &held, FEE_PER_KB,
        ));
        assert!(!after.failed, "#{at}");
        assert_eq!(after.migrations.len(), migrations, "#{at}");
        assert_eq!(after.retiring_kept, kept, "#{at}");
        assert_eq!(after.retiring_utxos, 1 - migrations, "#{at}");
    }
}

/// rskj's `…duringMigration_withManyMinNonDustRetiringUtxos_whenMigrationBuildFails_shouldThrowIllegalStateException`:
/// funds whose inputs cost more in fees than they hold leave in no amount
/// down to the dust limit, and `createMigrationTransaction` throws. In the
/// migration age that fails the call, and nothing it wrote stays.
#[test]
fn in_the_migration_age_funds_that_cannot_leave_fail_update_collections() {
    let created = 9_000_000;
    let (active, retiring) = (active(20, 4000), retiring(20, 4000));
    let during = created + AGE + 1;
    let held = utxos(&retiring, during, &[MIN_NON_DUST; 5]);
    let after = update_collections(mainnet_at(
        during, created, &active, &retiring, &held, FEE_PER_KB,
    ));
    assert!(after.failed);
    assert!(after.migrations.is_empty());
    assert!(after.retiring_kept);
    assert_eq!(after.retiring_utxos, 5);
}

/// rskj's `…pastMigrationAge_withMinNonDustRetiringUtxo…`,
/// `…withManyMinNonDustRetiringUtxo…` and
/// `…withHighFees_shouldClearRetiringFedWithoutMigrationTx`: at the migration
/// age's end `processFundsMigration` catches what `migrateFunds` throws, and
/// the federation retires with the funds that could not leave.
#[test]
fn past_the_migration_age_funds_that_cannot_leave_retire_with_the_federation() {
    let created = 9_000_000;
    let (active, retiring) = (active(20, 4000), retiring(20, 4000));
    let past = created + AGE + MIGRATION_END;
    for (values, fee_per_kb) in [
        (vec![MIN_NON_DUST], FEE_PER_KB),
        (vec![MIN_NON_DUST; 5], FEE_PER_KB),
        (vec![COIN], 2 * COIN),
    ] {
        let held = utxos(&retiring, past, &values);
        let after = update_collections(mainnet_at(
            past, created, &active, &retiring, &held, fee_per_kb,
        ));
        assert!(!after.failed, "{values:?}");
        assert!(after.migrations.is_empty(), "{values:?}");
        assert!(!after.retiring_kept, "{values:?}");
        assert_eq!(after.retiring_utxos, values.len());
    }
}

/// rskj's `createMigrationTransaction` asks bitcoinj for the whole balance,
/// then half of it, and so on, until the transaction can be built or the
/// amount is dust: when the 18,000 satoshis held in thirty-one UTXOs would
/// cost more in fees than they are, 9,000 leave from the one UTXO that holds
/// them.
#[test]
fn a_migration_asks_for_half_again_until_it_can_be_built() {
    let created = 9_000_000;
    let (active, retiring) = (active(9, 1000), retiring(9, 1000));
    let during = created + AGE + 1;
    let mut values = vec![9_000];
    values.extend([300; 30]);
    let held = utxos(&retiring, during, &values);
    let after = update_collections(mainnet_at(
        during, created, &active, &retiring, &held, FEE_PER_KB,
    ));
    assert!(!after.failed);
    assert_eq!(after.migrations.len(), 1);
    assert_eq!(after.migrations[0].input.len(), 1);
    assert!(after.retiring_kept);
    assert_eq!(after.retiring_utxos, 30);
}

/// rskj sends a migration to `getActiveFederationAddress()`, the active
/// federation read back in the format it was stored in: a standard multisig
/// federation is still one after RSKIP201 (iris300), when a federation
/// committed would be an ERP one.
#[test]
fn a_migration_pays_the_active_federation_in_its_stored_format() {
    let created = 3_700_000;
    let (active, retiring) = (active(9, 1000), retiring(9, 1000));
    let during = created + LEGACY_AGE + 1;
    assert!(RskHardforkConfig::mainnet().has_rskip201(during));
    let held = utxos(&retiring, during, &[COIN]);
    let after = update_collections(mainnet_at(
        during, created, &active, &retiring, &held, FEE_PER_KB,
    ));
    let keys = active.keys();
    let multisig = build_federation_redeem_script(&keys, keys.len() / 2 + 1);
    let to = p2sh_output_script_program(&hash160(&multisig));
    assert_eq!(after.migrations.len(), 1);
    assert_eq!(
        after.migrations[0].output[0].script_pubkey.as_bytes(),
        to.as_slice()
    );
}

/// rskj's `…withMoreUtxosThanMaxInputs…`: from RSKIP294 (hop400) the retiring
/// federation's wallet holds only its first 50 UTXOs, the most inputs a
/// peg-out has, and a migration spends those; before it, all of them.
#[test]
fn from_rskip294_a_migration_spends_at_most_a_pegouts_inputs() {
    let cfg = RskHardforkConfig::mainnet();
    let (active, retiring) = (active(9, 1000), retiring(9, 1000));
    for (created, inputs, left) in [(3_700_000, 51, 0), (4_700_000, 50, 1)] {
        let during = created + LEGACY_AGE + 1;
        assert_eq!(cfg.has_rskip294(during), inputs == 50);
        let held = utxos(&retiring, during, &[COIN; 51]);
        let after = update_collections(mainnet_at(
            during, created, &active, &retiring, &held, FEE_PER_KB,
        ));
        assert_eq!(after.migrations.len(), 1, "#{during}");
        assert_eq!(after.migrations[0].input.len(), inputs, "#{during}");
        assert_eq!(after.retiring_utxos, left, "#{during}");
    }
}

/// rskj's `FederationConstants.getFundsMigrationAgeSinceActivationEnd`:
/// between RSKIP357 (hop401) and RSKIP374 (fingerroot500) funds migrate until
/// 172,800 blocks after the activation age, not 10,585. At the usual end a
/// federation retires before hop401 and from fingerroot500, and is still
/// migrating between them.
#[test]
fn between_rskip357_and_rskip374_the_migration_age_ends_later() {
    let cfg = RskHardforkConfig::mainnet();
    let (active, retiring) = (active(9, 1000), retiring(9, 1000));
    for (created, age, kept) in [
        (4_900_000, LEGACY_AGE, false),
        (5_000_000, LEGACY_AGE, true),
        (5_500_000, AGE, false),
    ] {
        let at = created + age + MIGRATION_END;
        assert_eq!(cfg.has_rskip357(at) && !cfg.has_rskip374(at), kept);
        let held = utxos(&retiring, at, &[COIN]);
        let after = update_collections(mainnet_at(
            at, created, &active, &retiring, &held, FEE_PER_KB,
        ));
        assert_eq!(after.migrations.len(), 1, "#{at}");
        assert_eq!(after.retiring_kept, kept, "#{at}");
    }
}

/// rskj's `buildMigrationTransaction` keeps a migration at version 1, where
/// RSKIP201 left it, until RSKIP376 (arrowhead600) leaves it at version 2
/// as every peg-out.
#[test]
fn from_rskip376_a_migration_transaction_is_version_2() {
    let cfg = RskHardforkConfig::mainnet();
    let (active, retiring) = (active(9, 1000), retiring(9, 1000));
    for (created, version) in [(5_500_000, 1), (6_300_000, 2)] {
        let during = created + AGE + 1;
        assert_eq!(cfg.has_rskip376(during), version == 2);
        let held = utxos(&retiring, during, &[COIN]);
        let after = update_collections(mainnet_at(
            during, created, &active, &retiring, &held, FEE_PER_KB,
        ));
        assert_eq!(after.migrations.len(), 1, "#{during}");
        assert_eq!(after.migrations[0].version.0, version, "#{during}");
    }
}
