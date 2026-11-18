//! rskj `co.rsk.core.BlockHeaderTest`, which builds its headers as RSKIP92
//! has them: what of it no block has reached.

use super::block_factory_test::{built, mined, twenty_bytes};
use alloy_primitives::{keccak256, Bytes};

/// rskj `getHashForMergedMiningWhenUmmRoot` and
/// `getHashForMergedMiningWhenEmptyUmmRoot`: a header with a UMM root of 20
/// bytes merge-mines the hash of the first 20 bytes of its own merged-mining
/// hash and the root; one with an empty root, its own, the hash of the header
/// compressed and without its merged-mining fields. rskj's test then writes
/// fork-detection data over the last 12 bytes, which rustock's miner does
/// apart from the header, and its proof-of-work rule reads past.
#[test]
fn a_umm_root_joins_the_merged_mining_hash() {
    for root in [twenty_bytes("ummRoot"), Bytes::new()] {
        let mut bare = built(500);
        bare.umm_root = Some(root.clone());
        let own = keccak256(bare.encode_compressed());
        let expected = match root.is_empty() {
            true => own,
            false => keccak256([&own[..20], &root[..]].concat()),
        };
        assert_eq!(
            mined(bare, 80).hash_for_merged_mining(),
            expected,
            "UMM root of {} bytes",
            root.len()
        );
    }
}
