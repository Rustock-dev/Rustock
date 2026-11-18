//! rskj's `RepositoryBtcBlockStoreWithCacheTest`
//! (`co/rsk/peg/RepositoryBtcBlockStoreWithCacheTest.java`): the Bridge's
//! Bitcoin block store, its listed tests of how deep below the head a block
//! is looked for from RSKIP199 (`getStoredBlockAtMainChainHeight`), on rskj's
//! regtest, where the block index activates at Bitcoin height 10 and blocks
//! below it are looked for at most 5 below the head.
//!
//! Read in full. Its first listed test, the limit at the index's height, is
//! held by `bridge/btc_chain.rs`'s
//! `below_the_index_activation_rskj_looks_only_so_deep` on mainnet and
//! testnet. Its other tests are of rskj's cache of the store, which
//! `bridge/btc_block_cache_readpath_tests.rs` holds to the store's answers,
//! and of lookups above the head and through the index, which
//! `bridge/btc_chain.rs`'s tests hold.

use alloy_primitives::U256;
use bitcoin::hashes::Hash as _;

use crate::bridge::btc_chain::{main_chain_block_at_height, MainChainLookup};
use crate::bridge::btc_store::{self, StoredBlock};
use crate::bridge::constants::BridgeConstants;

use super::support::ctx_at;

/// rskj `getStoredBlockAtMainChainHeight_postIris_heightLowerThanMaxDepth_limitInChainHeadMinusMaxDepthToSearch`:
/// with the head less than the search depth above the index's height (here
/// at 14, one below 10 + 5), the deepest block looked for is the search depth
/// below the head, 9: below it the lookup throws, and from it the lookup goes
/// on, here to a block the store doesn't hold.
#[test]
fn from_rskip199_below_the_search_depth_under_the_head_the_lookup_throws() {
    let config = BridgeConstants::rskj_regtest();
    let (index, depth) = (
        config.btc_height_when_block_index_activates,
        config.max_depth_to_search_blocks_below_index_activation,
    );
    assert_eq!((index, depth), (10, 5));
    let head_height = index + depth - 1;
    let genesis = bitcoin::constants::genesis_block(bitcoin::Network::Regtest).header;
    let header = bitcoin::block::Header {
        prev_blockhash: genesis.block_hash(),
        merkle_root: bitcoin::TxMerkleNode::all_zeros(),
        nonce: 1,
        ..genesis
    };
    let mut ctx = ctx_at(1);
    let head = StoredBlock::new(header, head_height, U256::ZERO);
    btc_store::put_stored_block(&mut ctx, &head, true);
    btc_store::store_chain_head(&mut ctx, &head, true);

    let limit = head_height - depth;
    assert!(matches!(
        main_chain_block_at_height(&mut ctx, limit - 1, true, Some(&config)),
        MainChainLookup::StoreError
    ));
    assert!(matches!(
        main_chain_block_at_height(&mut ctx, limit, true, Some(&config)),
        MainChainLookup::Absent
    ));
    // Before RSKIP199 nothing limits the search.
    assert!(matches!(
        main_chain_block_at_height(&mut ctx, limit - 1, false, Some(&config)),
        MainChainLookup::Absent
    ));
}
