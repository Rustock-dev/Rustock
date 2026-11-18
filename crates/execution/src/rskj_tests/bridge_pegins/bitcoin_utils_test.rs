//! rskj's `BitcoinUtilsTest` (`co/rsk/peg/bitcoin/BitcoinUtilsTest.java`), its
//! listed tests of where a coinbase transaction's witness commitment is
//! found, `BitcoinUtils.findWitnessCommitment`, before and from RSKIP460
//! (lovell700).
//!
//! rskj's tests read three coinbase transactions, two of them mined, with an
//! output whose script bitcoinj can't parse: before RSKIP460 the search
//! throws a `ScriptException`, and from it the search skips that output and
//! finds the commitment. rustock looks for it inside
//! `registerBtcCoinbaseTransaction`, which stores the block's witness root
//! only when the commitment found is the one the root and the reserved value
//! make. Each port therefore registers rskj's transaction with the 32 bytes
//! rskj finds replaced by the commitment of a witness root chosen here, every
//! other byte as rskj has it: the call stores the root only if the
//! commitment is found where rskj finds it.
//!
//! Read in full. Its listed test of `getMultiSigTransactionHashWithoutSignatures`
//! from RSKIP305 is held by `bridge/peg.rs`'s
//! `svp_hashes_strip_as_rskj_before_and_after_rskip305`. Its other tests are
//! of the sighashes, redeem scripts and signatures of peg-outs and
//! migrations, the peg-out ports' part, and of commitments in well-formed
//! coinbases, which `bridge/tx.rs`'s tests hold.

use sha2::Digest as _;

use crate::abi::encode::{self, Arg};
use crate::bridge::constants::BridgeConstants;
use crate::bridge::{btc_chain, tx};
use crate::hardfork::{RskHardforkConfig, RskNetworkUpgrade, RSK_MAINNET_CHAIN_ID};

use super::support::*;

/// rskj's `malFormedTestnetCoinbaseTx`, testnet's
/// ae0a6c774908d3ddd334d40cc385ef1c2ad7e6381a69058114899f5ce567f26c. Its last
/// output pushes 75 bytes where 39 are left.
const MALFORMED_TESTNET: &str = "010000000001010000000000000000000000000000000000000000000000000000000000000000ffffffff32030a0e250004fee5346404196a763b0cc3dd196400000000000000000a636b706f6f6c0e2f6d696e65642062792072736b2fffffffff039cce2a00000000001976a914ec2f9ffaba0d68ea6bd7c25cedfe2ae710938e6088ac0000000000000000266a24aa21a9ede46b6d3bc71412e8905cedfad91532bdccb693d93a1335fee0b6223a7ed1ee5b00000000000000002a6a52534b424c4f434b3a8bc552daa25a7a473ac4640ba2b9d621c95652c61488143ca02bbf1b00392fb10120000000000000000000000000000000000000000000000000000000000000000000000000";

/// rskj's `malFormedMainnetCoinbaseTx`, mainnet's
/// 079e68032d9a4cdb222f464b9966756ccb749633aee678c6a51536b4fc38e29c. Its last
/// output pushes 37 bytes where 16 are left.
const MALFORMED_MAINNET: &str = "010000000001010000000000000000000000000000000000000000000000000000000000000000ffffffff3c03ebba0c040c93ef652f4d41524120506f6f6c202876303232373234292f76649b3c094f135bf4b83108c14ea85f12307cd4bf00d6010000ffffffffffffffff0371c71a27000000001976a9142fc701e2049ee4957b07134b6c1d771dd5a96b2188ac0000000000000000266a24aa21a9ed5e4aae37309d88e9f49d3a4c6fb424e491cbdbac754b8ef06bb90d2a149bd96900000000000000002cfabe6d6d9797129041127735e99e277241fbc539b327b16ad3e8537125cdc12ccf2d0586010000000000000001200000000000000000000000000000000000000000000000000000000000000000e5a28d27";

/// rskj's `coinbaseTxWithWitnessCommitmentOutputInNonStandardFormat`: its last
/// output is a commitment followed by four bytes, the last two of which push
/// 57 bytes where one is left.
const NON_STANDARD: &str = "010000000001010000000000000000000000000000000000000000000000000000000000000000ffffffff32030a0e250004fee5346404196a763b0cc3dd196400000000000000000a636b706f6f6c0e2f6d696e65642062792072736b2fffffffff039cce2a00000000001976a914ec2f9ffaba0d68ea6bd7c25cedfe2ae710938e6088ac0000000000000000266a24aa21a9ede46b6d3bc71412e8905cedfad91532bdccb693d93a1335fee0b6223a7ed1ee5b00000000000000002a6a24aa21a9ed4f434b3a8bc552daa25a7a473ac4640ba2b9d621c95652c61488143ca02bbf1b00392fb10120000000000000000000000000000000000000000000000000000000000000000000000000";

/// Each transaction, and the commitment rskj finds in it from RSKIP460.
const TRANSACTIONS: [(&str, &str); 3] = [
    (
        MALFORMED_TESTNET,
        "e46b6d3bc71412e8905cedfad91532bdccb693d93a1335fee0b6223a7ed1ee5b",
    ),
    (
        MALFORMED_MAINNET,
        "5e4aae37309d88e9f49d3a4c6fb424e491cbdbac754b8ef06bb90d2a149bd969",
    ),
    (
        NON_STANDARD,
        "4f434b3a8bc552daa25a7a473ac4640ba2b9d621c95652c61488143ca02bbf1b",
    ),
];

/// The witness root registered, in the order the call is given it.
const WITNESS_ROOT: [u8; 32] = [0x3c; 32];

/// The witness reserved value the transactions' coinbase inputs carry.
const RESERVED: [u8; 32] = [0; 32];

/// rskj's `validateWitnessInformation`: the commitment a witness root and a
/// reserved value make, `Sha256Hash.twiceOf(root.getReversedBytes(), reserved)`.
fn commitment(root: [u8; 32], reserved: [u8; 32]) -> [u8; 32] {
    let mut data = root.to_vec();
    data.reverse();
    data.extend_from_slice(&reserved);
    sha2::Sha256::digest(sha2::Sha256::digest(&data)).into()
}

/// `raw` with the commitment header and `found` after it, the bytes rskj
/// finds, followed by `replacement` instead: found once and only once.
fn replacing(raw: &[u8], found: &[u8; 32], replacement: &[u8; 32]) -> Vec<u8> {
    let mut needle = vec![0xaa, 0x21, 0xa9, 0xed];
    needle.extend_from_slice(found);
    let at: Vec<usize> = raw
        .windows(needle.len())
        .enumerate()
        .filter(|(_, window)| *window == needle)
        .map(|(at, _)| at + 4)
        .collect();
    assert_eq!(
        at.len(),
        1,
        "the commitment rskj finds is in the transaction once"
    );
    let mut replaced = raw.to_vec();
    replaced[at[0]..at[0] + 32].copy_from_slice(replacement);
    replaced
}

/// `registerBtcCoinbaseTransaction` of the coinbase `raw`, held in a mainnet
/// block, at RSK block 10 of the ladder up to `last`, with [`WITNESS_ROOT`]:
/// whether it returned, and the witness root then stored for the block.
fn register_coinbase(raw: &[u8], last: RskNetworkUpgrade) -> (bool, Option<[u8; 32]>) {
    let (config, cfg) = (
        BridgeConstants::mainnet(),
        up_to(last, RSK_MAINNET_CHAIN_ID),
    );
    let mut ctx = ctx_at(10);
    let header = hold_with_root(
        &mut ctx,
        tx::calculate_btc_tx_hash(raw),
        850_000,
        850_000,
        &cfg,
    );
    let block_hash = btc_chain::bitcoin_hash_to_b256(&header.block_hash()).0;
    let tree = one_leaf_tree(raw);
    let args = encode::args(&[
        Arg::Bytes(raw),
        Arg::Word(block_hash),
        Arg::Bytes(&tree),
        Arg::Word(WITNESS_ROOT),
        Arg::Word(RESERVED),
    ]);
    let returned = tx::register_btc_coinbase_transaction(&mut ctx, &args, 0, &config, &cfg).is_ok();
    (
        returned,
        tx::get_coinbase_information(&mut ctx, &block_hash),
    )
}

/// Whether the ladder up to `last` has RSKIP460.
fn has_rskip460(last: RskNetworkUpgrade) -> bool {
    let cfg: RskHardforkConfig = up_to(last, RSK_MAINNET_CHAIN_ID);
    cfg.has_rskip460(10)
}

/// The transaction `raw`, with what rskj finds in it from RSKIP460 replaced
/// by [`WITNESS_ROOT`]'s commitment.
fn committing_to_the_root(raw: &str, found: &str) -> Vec<u8> {
    let raw = hex::decode(raw).expect("a transaction in hex");
    let found: [u8; 32] = hex::decode(found)
        .expect("hex")
        .try_into()
        .expect("32 bytes");
    let replaced = replacing(&raw, &found, &commitment(WITNESS_ROOT, RESERVED));
    // Still a coinbase rust-bitcoin reads, with the scripts as they were.
    let read: bitcoin::Transaction =
        bitcoin::consensus::deserialize(&replaced).expect("a transaction");
    assert!(read.is_coinbase());
    replaced
}

/// rskj `findWitnessCommitment_whenMalFormedTxBeforeRSKIP460_shouldThrowException`:
/// before RSKIP460 (arrowhead600), the search reads the last output's script
/// first, which bitcoinj can't parse, and throws: the call fails and no
/// witness root is stored.
#[test]
fn before_rskip460_a_coinbase_with_a_script_bitcoinj_cannot_parse_is_refused() {
    assert!(!has_rskip460(RskNetworkUpgrade::Arrowhead600));
    for (raw, found) in TRANSACTIONS {
        let raw = committing_to_the_root(raw, found);
        assert_eq!(
            register_coinbase(&raw, RskNetworkUpgrade::Arrowhead600),
            (false, None),
            "{found}"
        );
    }
}

/// rskj `findWitnessCommitment_whenMalFormedTxAfterRSKIP460_shouldIgnoreMalformedOutputAndReturnFoundWitnessCommitment`:
/// from RSKIP460 (every upgrade on), the output bitcoinj can't parse is
/// skipped and the commitment found is the one rskj's test names, the last
/// one, standard or not: the call stores the witness root it was given.
#[test]
fn from_rskip460_the_unparseable_output_is_skipped_and_the_commitment_found() {
    assert!(has_rskip460(RskNetworkUpgrade::Vetiver900));
    for (raw, found) in TRANSACTIONS {
        let raw = committing_to_the_root(raw, found);
        assert_eq!(
            register_coinbase(&raw, RskNetworkUpgrade::Vetiver900),
            (true, Some(WITNESS_ROOT)),
            "{found}"
        );
    }
    // The transaction as mined commits to no root of ours: the commitment
    // found isn't this root's, and the call fails.
    let (raw, _) = TRANSACTIONS[0];
    let mined = hex::decode(raw).expect("hex");
    assert_eq!(
        register_coinbase(&mined, RskNetworkUpgrade::Vetiver900),
        (false, None)
    );
}
