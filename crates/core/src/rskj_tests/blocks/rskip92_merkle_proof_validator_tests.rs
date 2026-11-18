//! rskj `org.ethereum.validator.Rskip92MerkleProofValidatorTests`.
//!
//! From RSKIP92 (orchid) a merged-mining merkle proof is a run of 32-byte
//! hashes, each combined on the right of the node below it, starting from
//! the coinbase's hash (`Rskip92MerkleProofValidator`). A proof whose length
//! is no whole number of hashes is refused, and from RSKIP180 (iris300) so is
//! one over 960 bytes, checked first. rskj refuses the block when building
//! the validator throws (`ProofOfWorkRule.isValid` catches it and answers
//! false); here the same checks are [`MergedMiningRule`]'s.
//!
//! [`MergedMiningRule`]: crate::validation::MergedMiningRule

use super::support::{
    coinbase_committing_to, folded_root, hashes, merged_mining_rule, minable_header, mined_through,
    mined_with_siblings, regtest_with_iris300_at, wire,
};
use crate::validation::merged_mining::{self, MAX_MERGED_MINING_MERKLE_PROOF_LENGTH};
use crate::validation::{HeaderValidator, ValidationError};
use alloy_primitives::Bytes;

/// RSKIP180 arrives at this height in these tests; below it the proof's
/// length is held only to whole hashes.
const IRIS300: u64 = 100;

/// bitcoinj's `MerkleTreeUtils.combineLeftRight`, worked by hand: each node
/// reversed from display order, the two double-SHA-256'd, and the result
/// reversed back.
fn combine(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut input = [0u8; 64];
    input[..32].copy_from_slice(left);
    input[..32].reverse();
    input[32..].copy_from_slice(right);
    input[32..].reverse();
    let mut out: [u8; 32] = Sha256::digest(Sha256::digest(input)).into();
    out.reverse();
    out
}

/// rskj `isValid_PassValidHashes_ShouldReturnTrue`,
/// `isValid_PassInvalidCoinbaseHash_ShouldReturnFalse` and
/// `isValid_PassInvalidRootHash_ShouldReturnFalse`: five hashes, the first
/// of them the coinbase's too, fold from the coinbase to one root, and a
/// different coinbase or a different root does not match it. Then the same
/// at the rule, on a header merge-mined through five siblings.
#[test]
fn a_proof_folds_the_coinbase_hash_with_each_hash_in_turn() {
    let list = hashes(5);
    let proof = list.concat();
    let coinbase = list[0];
    let root = list
        .iter()
        .fold(coinbase, |node, hash| combine(&node, hash));

    assert_eq!(
        merged_mining::rskip92_merkle_root(&coinbase, &proof),
        Some(root)
    );
    assert_ne!(
        merged_mining::rskip92_merkle_root(&[0; 32], &proof),
        Some(root),
        "another coinbase"
    );
    assert_ne!(
        merged_mining::rskip92_merkle_root(&coinbase, &proof),
        Some([0; 32]),
        "another root"
    );

    let rule = merged_mining_rule(regtest_with_iris300_at(0));
    let header = minable_header(10);
    let coinbase = coinbase_committing_to(&header, 30);
    let mined = mined_with_siblings(header.clone(), coinbase.clone(), &list);
    assert_eq!(rule.validate(&mined), Ok(()));

    let mut wrong_root = mined.clone();
    wrong_root.bitcoin_merged_mining_header =
        mined_through(header.clone(), coinbase.clone(), list.concat(), [0; 32])
            .bitcoin_merged_mining_header;
    assert_eq!(
        rule.validate(&wrong_root),
        Err(ValidationError::BitcoinMerkleProofInvalid)
    );

    // A bit of the coinbase's midstate: the tag is still there, but the
    // coinbase's hash, which the fold starts from, is another.
    let mut other_coinbase = coinbase;
    other_coinbase[20] ^= 1;
    let mut wrong_coinbase = mined;
    wrong_coinbase.bitcoin_merged_mining_coinbase_transaction = Some(Bytes::from(other_coinbase));
    assert_eq!(
        rule.validate(&wrong_coinbase),
        Err(ValidationError::BitcoinMerkleProofInvalid)
    );
}

/// rskj `createInstance_PassNullProofAndRskip180Enabled_ShouldThrowIllegalArgumentError`
/// and `createInstance_PassNullProofAndRskip180Disabled_ShouldThrowNullPointerError`:
/// with no proof, the validator can't be built whether or not RSKIP180 is
/// active, and the block is refused.
#[test]
fn a_header_without_a_merkle_proof_is_refused_with_or_without_rskip180() {
    let rule = merged_mining_rule(regtest_with_iris300_at(IRIS300));
    for number in [IRIS300 - 1, IRIS300] {
        let header = minable_header(number);
        let coinbase = coinbase_committing_to(&header, 30);
        let mut mined = mined_with_siblings(header, coinbase, &[]);
        assert_eq!(
            rule.validate(&mined),
            Ok(()),
            "block {number}, with its proof"
        );
        mined.bitcoin_merged_mining_merkle_proof = None;
        assert_eq!(
            rule.validate(&mined),
            Err(ValidationError::BitcoinMerkleProofDecodeError),
            "block {number}"
        );
    }
}

/// rskj `createInstance_PassLargeProofAndRskip180Enabled_ShouldThrowError`,
/// `createInstance_PassLargeMalformedProofAndRskip180Disabled_ShouldThrowError`
/// and `createInstance_PassLargeProofAndRskip180Disabled_ShouldNotThrowError`:
/// from RSKIP180 a proof over 960 bytes is refused before its form is
/// looked at; below it, 31 hashes (992 bytes) are a proof like any other,
/// and 961 bytes are no whole number of hashes.
#[test]
fn from_rskip180_a_proof_over_960_bytes_is_refused() {
    let rule = merged_mining_rule(regtest_with_iris300_at(IRIS300));
    let max = MAX_MERGED_MINING_MERKLE_PROOF_LENGTH;
    assert_eq!(max, 960);
    let mined = |number: u64, siblings: usize| {
        let header = minable_header(number);
        let coinbase = coinbase_committing_to(&header, 30);
        mined_with_siblings(header, coinbase, &hashes(siblings))
    };

    // 30 hashes, 960 bytes: at the limit, on either side of RSKIP180.
    assert_eq!(rule.validate(&mined(IRIS300, 30)), Ok(()));
    assert_eq!(rule.validate(&mined(IRIS300 - 1, 30)), Ok(()));

    // 31 hashes, 992 bytes: a proof below RSKIP180, too long from it.
    assert_eq!(rule.validate(&mined(IRIS300 - 1, 31)), Ok(()));
    assert_eq!(
        rule.validate(&mined(IRIS300, 31)),
        Err(ValidationError::MerkleProofTooLarge { max, got: 992 })
    );

    // 961 bytes: refused for its size from RSKIP180, which is checked
    // before its form, and for its form below it.
    let malformed = |number: u64| {
        let mut header = mined(number, 30);
        let mut proof = header
            .bitcoin_merged_mining_merkle_proof
            .take()
            .unwrap()
            .to_vec();
        proof.push(0);
        header.bitcoin_merged_mining_merkle_proof = Some(Bytes::from(proof));
        rule.validate(&header)
    };
    assert_eq!(
        malformed(IRIS300),
        Err(ValidationError::MerkleProofTooLarge { max, got: 961 })
    );
    assert_eq!(
        malformed(IRIS300 - 1),
        Err(ValidationError::BitcoinMerkleProofDecodeError)
    );
}

/// rskj `createInstance_PassMalformedProofAndRskip180Enabled_ShouldThrowError`
/// and `createInstance_PassMalformedProofAndRskip180Disabled_ShouldThrowError`:
/// 33 bytes are no whole number of hashes, with or without RSKIP180, though
/// the first 32 are the hash that would make the proof right.
#[test]
fn a_proof_of_no_whole_number_of_hashes_is_refused_with_or_without_rskip180() {
    let rule = merged_mining_rule(regtest_with_iris300_at(IRIS300));
    for number in [IRIS300 - 1, IRIS300] {
        let header = minable_header(number);
        let coinbase = coinbase_committing_to(&header, 30);
        let sibling = hashes(1)[0];
        let root = folded_root(merged_mining::compute_coinbase_hash(&coinbase), &[sibling]);
        let proof = [&sibling[..], &[0]].concat();
        let mined = mined_through(header, coinbase, proof, wire(root));
        assert_eq!(
            rule.validate(&mined),
            Err(ValidationError::BitcoinMerkleProofDecodeError),
            "block {number}"
        );
    }
}
