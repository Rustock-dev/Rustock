//! rskj's `BridgeSupportIT`, its three `initialChainHeadWithBtcCheckpoints_*`
//! tests, of RSKIP454. rskj swaps the checkpoints file mainnet's Bridge seeds
//! its Bitcoin store from, for files of chain work written in 12 bytes, in 32,
//! and in both. rustock carries no checkpoints file, only the checkpoint each
//! network's store is seeded with (`BridgeConstants::btc_checkpoint`), so the
//! ports give mainnet's Bridge its own checkpoint with the chain work written
//! in 12 bytes, written in 32, and grown past what 12 bytes hold. Before
//! RSKIP454 the store writes the checkpoint in the legacy format, whose 12
//! bytes of chain work refuse the last (`StoredBlock.serializeCompactLegacy`),
//! and the call throws; from it, in the 32-byte format, which takes all three.

use super::support::*;

/// Mainnet's lovell700, which brings RSKIP454.
const LOVELL700: u64 = 7_338_024;
/// The height of mainnet's checkpoint.
const CHECKPOINT_HEIGHT: i64 = 499_968;
/// The chain work of mainnet's checkpoint, in 12 bytes.
const IN_12_BYTES: &str = "00cd6eae36c0cee5640896d3";
/// The same, in 32 bytes.
const IN_32_BYTES: &str = "000000000000000000000000000000000000000000cd6eae36c0cee5640896d3";
/// A chain work past what 12 bytes hold.
const PAST_12_BYTES: &str = "0100cd6eae36c0cee5640896d3";

/// `getBtcBlockchainBestChainHeight` at block `number` of mainnet, whose
/// Bridge seeds its store from mainnet's checkpoint with `chain_work`: the
/// height it answers, or none where the call throws.
fn best_chain_height(number: u64, chain_work: &'static str) -> Option<i64> {
    let mut constants = BridgeConstants::mainnet();
    let (header, height, kept) = constants.btc_checkpoint.expect("mainnet's checkpoint");
    assert_eq!((i64::from(height), kept), (CHECKPOINT_HEIGHT, IN_12_BYTES));
    constants.btc_checkpoint = Some((header, height, chain_work));
    let mut bridge = Bridge::new(RskHardforkConfig::mainnet(), constants, number);
    let answer = bridge.execute(&selector_only("getBtcBlockchainBestChainHeight()"));
    if threw(&answer) {
        return None;
    }
    Some(int_of(&answer.expect("an answer").bytes))
}

/// rskj's
/// `initialChainHeadWithBtcCheckpoints_whenCheckpointsWith12BytesChainWork_before_RSKIP454_ok`,
/// of a file written in 12 bytes and one written in both: before RSKIP454
/// the store is seeded from the checkpoint, the head of its best chain.
#[test]
fn before_rskip454_a_checkpoint_whose_chain_work_fits_12_bytes_seeds_the_store() {
    assert_eq!(
        best_chain_height(LOVELL700 - 1, IN_12_BYTES),
        Some(CHECKPOINT_HEIGHT)
    );
    assert_eq!(
        best_chain_height(LOVELL700 - 1, IN_32_BYTES),
        Some(CHECKPOINT_HEIGHT)
    );
}

/// rskj's
/// `initialChainHeadWithBtcCheckpoints_whenCheckpointsWith32BytesChainWork_before_RSKIP454_shouldFail`:
/// before RSKIP454 a checkpoint whose chain work 12 bytes can't hold fails
/// the call ("The given number does not fit in 12").
#[test]
fn before_rskip454_a_checkpoint_whose_chain_work_is_past_12_bytes_fails_the_call() {
    assert_eq!(best_chain_height(LOVELL700 - 1, PAST_12_BYTES), None);
}

/// rskj's
/// `initialChainHeadWithBtcCheckpoints_whenCheckpointsWith32BytesChainWork_after_RSKIP454_ok`,
/// of its files: from RSKIP454 the store is seeded from the checkpoint
/// whatever its chain work.
#[test]
fn from_rskip454_a_checkpoint_of_any_chain_work_seeds_the_store() {
    for chain_work in [IN_12_BYTES, IN_32_BYTES, PAST_12_BYTES] {
        assert_eq!(
            best_chain_height(LOVELL700, chain_work),
            Some(CHECKPOINT_HEIGHT),
            "{chain_work}"
        );
    }
}
