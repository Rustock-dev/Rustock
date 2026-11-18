//! rskj's `FederationConstantsTest`
//! (`co/rsk/peg/federation/constants/FederationConstantsTest.java`), its
//! listed test of how long after a new federation's activation the old one
//! is migrated from (`getFundsMigrationAgeSinceActivationEnd`): 10,585 blocks
//! on mainnet, but 172,800 while RSKIP357 is active and RSKIP374 isn't
//! (hop401 to fingerroot500), 900 on testnet and 150 on regtest throughout.
//!
//! On mainnet the port runs `updateCollections` around the migration's end,
//! where the old federation is cleared.
//!
//! Read in full. Its other tests check each network's values, which
//! `bridge/constants.rs` holds, and the activation age's, which
//! `bridge/governance.rs`'s `testnets_activation_age_has_a_step_before_lovell`
//! and the port of `FederationSupportImplTest` reach.

use crate::bridge::constants::BridgeConstants;
use crate::bridge::peg;
use crate::hardfork::RskHardforkConfig;
use crate::precompiles::BridgeTxContext;

use super::support::*;

/// Whether mainnet's old federation is still kept at `block`, with a new
/// federation created at `created` (in `format`) and nothing to migrate,
/// after `updateCollections`.
fn old_federation_kept(block: u64, created: u64, format: u64) -> bool {
    let (config, cfg) = (BridgeConstants::mainnet(), RskHardforkConfig::mainnet());
    let mut ctx = ctx_at(block);
    let old: Vec<[u8; 33]> = (0..3u8).map(|i| public_key(&[0x61 + i])).collect();
    store_federation(&mut ctx, NEW, &members(9), 1_000, created, Some(format));
    store_federation(&mut ctx, OLD, &old, 500, created - 1_000, Some(1000));
    peg::update_collections(&mut ctx, 0, &config, &cfg, &BridgeTxContext::default())
        .expect("updateCollections returns");
    !cell(&mut ctx, OLD.0).is_empty()
}

/// rskj `getFundsMigrationAgeSinceActivationEnd`: the values, and on
/// mainnet the block the old federation is cleared at, the activation age
/// (18,500 before fingerroot500, 40,320 from it) and the migration's end
/// after the new federation's creation.
#[test]
fn the_migration_ends_later_on_mainnet_between_rskip357_and_rskip374() {
    let mainnet = BridgeConstants::mainnet();
    assert_eq!(
        (
            mainnet.funds_migration_age_end,
            mainnet.special_case_funds_migration_age_end
        ),
        (10_585, 172_800)
    );
    let testnet = BridgeConstants::testnet();
    assert_eq!(
        (
            testnet.funds_migration_age_end,
            testnet.special_case_funds_migration_age_end
        ),
        (900, 900)
    );
    let regtest = BridgeConstants::rskj_regtest();
    assert_eq!(
        (
            regtest.funds_migration_age_end,
            regtest.special_case_funds_migration_age_end
        ),
        (150, 150)
    );

    let cfg = RskHardforkConfig::mainnet();
    // Before RSKIP357 (hop401): 10,585.
    let created = 4_900_000;
    let end = created + 18_500 + 10_585;
    assert!(!cfg.has_rskip357(end));
    assert!(old_federation_kept(end - 1, created, 2000));
    assert!(!old_federation_kept(end, created, 2000));

    // With RSKIP357 and not RSKIP374: 172,800.
    let created = 4_980_000;
    let end = created + 18_500 + 172_800;
    assert!(cfg.has_rskip357(created) && !cfg.has_rskip374(end));
    assert!(old_federation_kept(
        created + 18_500 + 10_585,
        created,
        3000
    ));
    assert!(old_federation_kept(end - 1, created, 3000));
    assert!(!old_federation_kept(end, created, 3000));

    // From RSKIP374 (fingerroot500): 10,585 again, after 40,320.
    let created = 5_500_000;
    let end = created + 40_320 + 10_585;
    assert!(cfg.has_rskip374(created));
    assert!(old_federation_kept(end - 1, created, 3000));
    assert!(!old_federation_kept(end, created, 3000));
}
