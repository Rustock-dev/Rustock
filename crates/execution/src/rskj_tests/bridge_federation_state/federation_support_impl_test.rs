//! rskj's `FederationSupportImplTest`
//! (`co/rsk/peg/federation/FederationSupportImplTest.java`): which
//! federation is active, and the creation heights a federation change keeps
//! (RSKIP186).
//!
//! Read in full. Its listed tests of `getActiveFederationRedeemScript` before
//! RSKIP293, and of `getActiveFederationCreationBlockHeight` before RSKIP186,
//! are of answers no call reaches: only the methods
//! `getActivePowpegRedeemScript` and `getActiveFederationCreationBlockHeight`
//! read them, and rskj enables those from RSKIP293 and RSKIP186. Its tests of
//! the active federation's redeem script from RSKIP293 are held by
//! `bridge/governance.rs`'s `the_active_powpeg_redeem_script_is_rskjs`. From
//! the rest of the class the creation height's answer around the new
//! federation's activation is ported here.

use crate::bridge::constants::BridgeConstants;
use crate::bridge::{governance, storage};
use crate::hardfork::RskHardforkConfig;

use super::support::*;

fn mainnet() -> (BridgeConstants, RskHardforkConfig) {
    (BridgeConstants::mainnet(), RskHardforkConfig::mainnet())
}

/// rskj `updateFederationCreationBlockHeights_beforeRSKIP186_doesNothing`:
/// before RSKIP186 (papyrus200) the update `updateCollections` makes leaves
/// the creation heights as they are, even with a next one whose federation
/// is past its activation age; from RSKIP186 (iris300) the same update
/// makes it the active one.
#[test]
fn before_rskip186_the_creation_heights_are_left_as_they_are() {
    let (config, cfg) = mainnet();
    for (next, block, promoted) in [(2_400_000, 2_500_000, false), (3_700_000, 3_800_000, true)] {
        assert_eq!(cfg.has_rskip186(block), promoted);
        let mut ctx = ctx_at(block);
        put(
            &mut ctx,
            storage::NEXT_FEDERATION_CREATION_BLOCK_HEIGHT_KEY,
            &rlp_number(next),
        );
        governance::update_federation_creation_block_heights(&mut ctx, &config, &cfg, block);
        let (active, next_cell) = (
            cell(
                &mut ctx,
                storage::ACTIVE_FEDERATION_CREATION_BLOCK_HEIGHT_KEY,
            ),
            cell(&mut ctx, storage::NEXT_FEDERATION_CREATION_BLOCK_HEIGHT_KEY),
        );
        if promoted {
            assert_eq!((active, next_cell), (rlp_number(next), vec![]));
        } else {
            assert_eq!((active, next_cell), (vec![], rlp_number(next)));
        }
    }
}

/// `getActiveFederationCreationBlockHeight` at `block` of mainnet, with the
/// next creation height `next` and the active one `active` kept.
fn creation_height_answered(block: u64, next: u64, active: Option<u64>) -> u64 {
    let (config, cfg) = mainnet();
    let mut ctx = ctx_at(block);
    put(
        &mut ctx,
        storage::NEXT_FEDERATION_CREATION_BLOCK_HEIGHT_KEY,
        &rlp_number(next),
    );
    if let Some(active) = active {
        put(
            &mut ctx,
            storage::ACTIVE_FEDERATION_CREATION_BLOCK_HEIGHT_KEY,
            &rlp_number(active),
        );
    }
    let out = governance::get_active_federation_creation_block_height(&mut ctx, 0, &config, &cfg)
        .expect("getActiveFederationCreationBlockHeight answers");
    u64::try_from(int_answer(&out.bytes)).expect("a height")
}

/// rskj `getActiveFederationCreationBlockHeight_withNextAndActiveFederationCreationBlockHeightsUnset_returnsZero`,
/// `..._withNextFederationCreationBlockHeightUnsetAndActiveFederationCreationBlockHeightSet_returnsActiveFederationCreationBlockHeight`,
/// and the rows of the two tests after them where the new federation isn't
/// active yet: the answer is the active creation height, or 0 with none.
/// The activation age is mainnet's 18,500 before RSKIP383 (fingerroot500)
/// and 40,320 from it.
#[test]
fn before_the_new_federation_is_active_the_active_creation_height_is_answered() {
    for (next, block) in [
        (4_000_000, 4_000_000 + 18_500 - 1),
        (5_500_000, 5_500_000 + 40_320 - 1),
        (5_500_000, 5_500_000 + 18_500),
    ] {
        assert_eq!(creation_height_answered(block, next, None), 0, "at {block}");
        assert_eq!(
            creation_height_answered(block, next, Some(3_700_000)),
            3_700_000,
            "at {block}"
        );
    }
    let (config, cfg) = mainnet();
    let mut ctx = ctx_at(4_000_000);
    let out = governance::get_active_federation_creation_block_height(&mut ctx, 0, &config, &cfg)
        .expect("an answer");
    assert_eq!(int_answer(&out.bytes), 0, "nothing kept");
}

/// rskj `getActiveFederationCreationBlockHeight_withNewFederationCreationBlockHeightSetAndActiveFederationCreationBlockHeightUnset_returnsCreationBlockHeightAccordingToCurrentBlockAndActivations`
/// and `..._withNewAndActiveFederationCreationBlockHeightsSet_...`, their rows
/// where the new federation is active: from the block where it is, the
/// answer is its creation height, before `updateCollections` keeps it as the
/// active one (`FederationSupportImpl.getActiveFederationCreationBlockHeight`
/// reads the next height first).
#[test]
fn once_the_new_federation_is_active_its_creation_height_is_answered() {
    for (next, block) in [
        (4_000_000, 4_000_000 + 18_500),
        (5_500_000, 5_500_000 + 40_320),
    ] {
        assert_eq!(
            creation_height_answered(block, next, None),
            next,
            "at {block}"
        );
        assert_eq!(
            creation_height_answered(block, next, Some(3_700_000)),
            next,
            "at {block}"
        );
    }
}
