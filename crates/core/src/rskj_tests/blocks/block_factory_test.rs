//! rskj `co.rsk.core.BlockFactoryTest`: how `BlockFactory.decodeHeader` reads
//! a header at its height.
//!
//! rskj counts a header's fields exactly (`canBeDecoded`): the sixteen every
//! header has; the UMM root from RSKIPUMM; from RSKIP351, and in the full
//! encoding only, the version, the edges where RSKIP144 is active too and the
//! base event where RSKIP535 is; then the three merged-mining fields or none.
//! On testnet below #7,800,000 it also reads a full header one field short
//! of that, with an empty base event (`isInBaseEventRecoveryWindow`). A
//! header of any other count isn't read; neither is one whose Bitcoin header
//! isn't 80 bytes from RSKIP98.
//!
//! rskj's test mocks its activations rule by rule. Here a case's heights put
//! each rule it has on at genesis and every other off; rustock's `orchid`
//! is RSKIP92 and RSKIP98 together, as `reference.conf` has them. A header
//! is built with the fields rskj's builder gives it, encoded by rustock,
//! its fields counted against the count rskj's test asserts, and read back
//! under those heights.

use super::support::{compressed_coinbase, header};
use crate::config::{ActivationHeights, ChainConfig};
use crate::rskj_rlp;
use crate::types::header::{Header, HeaderForm, HeaderVersionFields};
use crate::types::header_extension::{BlockHeaderExtension, ExtensionCommitment};
use crate::validation::merged_mining::{extract_fork_detection_data, RSK_TAG};
use alloy_primitives::{hex, keccak256, Address, Bloom, Bytes, B256, U256};
use alloy_rlp::Encodable;

/// Heights with every rule off, for a case to turn on the ones its rskj test
/// enables.
pub(super) fn heights(turn_on: impl FnOnce(&mut ActivationHeights)) -> ActivationHeights {
    let mut heights = ActivationHeights {
        orchid: u64::MAX,
        wasabi100: u64::MAX,
        papyrus200: u64::MAX,
        iris300: u64::MAX,
        fingerroot500: u64::MAX,
        hop400: u64::MAX,
        reed810: u64::MAX,
        rskip97: None,
        rskip144: None,
        rskip351: None,
        rskip290: None,
        rskip_umm: None,
        rskip535: None,
        base_event_recovery_below: None,
    };
    turn_on(&mut heights);
    heights
}

/// The header rskj's `createBlockHeader` builds at `number`: no merged-mining
/// fields, no UMM root, no version fields.
pub(super) fn built(number: u64) -> Header {
    let mut header = header(number, 7_731_067);
    header.parent_hash = keccak256(format!("hash{number}"));
    header.ommers_hash = keccak256([alloy_rlp::EMPTY_LIST_CODE]);
    header.beneficiary = Address::from_slice(&keccak256(format!("coinbase{number}"))[12..]);
    header.state_root = keccak256([alloy_rlp::EMPTY_STRING_CODE]);
    header.transactions_root = keccak256("tx_trie_root");
    header.receipts_root = keccak256([alloy_rlp::EMPTY_STRING_CODE]);
    header.difficulty = U256::from(1);
    header.gas_limit = U256::from(6_800_000);
    header.gas_used = 3_000_000;
    header.minimum_gas_price = U256::from(10);
    header
}

/// With rskj's merged-mining fields: a Bitcoin header of `btc_header`
/// bytes, a 32-byte proof and a 128-byte coinbase.
pub(super) fn mined(mut header: Header, btc_header: usize) -> Header {
    header.bitcoin_merged_mining_header = Some(Bytes::from(vec![0; btc_header]));
    header.bitcoin_merged_mining_merkle_proof = Some(Bytes::from(vec![0; 32]));
    header.bitcoin_merged_mining_coinbase_transaction = Some(Bytes::from(vec![0; 128]));
    header
}

/// From RSKIP351: `version`, the `edges` and the `base_event`.
fn versioned(
    mut header: Header,
    version: u8,
    edges: Option<&[i16]>,
    base_event: Option<&[u8]>,
) -> Header {
    header.version_fields = Some(HeaderVersionFields {
        version,
        edges: edges.map(<[i16]>::to_vec),
        base_event: base_event.map(Bytes::copy_from_slice),
    });
    header
}

/// rskj's test bloom: its first four bytes set.
fn bloom() -> Bloom {
    let mut bytes = [0u8; 256];
    bytes[..4].fill(1);
    Bloom::from(bytes)
}

const EDGES: [i16; 4] = [-5, 300, 7, 32_767];

/// Twenty bytes named by `label`, as rskj's `TestUtils.generateBytes`.
pub(super) fn twenty_bytes(label: &str) -> Bytes {
    Bytes::copy_from_slice(&keccak256(label)[..20])
}

fn base_event() -> Vec<u8> {
    keccak256("baseEvent").to_vec()
}

/// The full encoding, as blocks carry headers (`getFullEncoded`).
pub(super) fn full(header: &Header) -> Vec<u8> {
    let mut out = Vec::new();
    header.encode(&mut out);
    out
}

/// How many fields an encoded header has.
pub(super) fn count(encoded: &[u8]) -> usize {
    let (list, _) = rskj_rlp::first_element(encoded).unwrap();
    list.elements().unwrap().len()
}

fn read(
    encoded: &[u8],
    heights: &ActivationHeights,
    form: HeaderForm,
) -> alloy_rlp::Result<Header> {
    Header::decode_in(&mut &encoded[..], heights, form)
}

/// The hash rskj gives `header` built under `heights`.
fn hash_under(header: &Header, heights: &ActivationHeights) -> B256 {
    header.clone().with_activations(heights).hash()
}

/// Reads `header` back from `encoded` under `heights`, and checks it keeps
/// its hash.
fn round_trip(
    header: &Header,
    encoded: &[u8],
    heights: &ActivationHeights,
    form: HeaderForm,
) -> Header {
    let read =
        read(encoded, heights, form).unwrap_or_else(|e| panic!("block {}: {e}", header.number));
    assert_eq!(
        read.hash(),
        hash_under(header, heights),
        "block {}",
        header.number
    );
    read
}

/// The commitment a compressed header carries in its bloom's slot.
fn commitment(header: &Header) -> ExtensionCommitment {
    header.extension_commitment().unwrap().unwrap()
}

// ── the genesis block, and the hash a header has without mining fields

/// rskj's test genesis, which is testnet's.
const GENESIS: &str = concat!(
    "f901dbf901d6a00000000000000000000000000000000000000000000000000000000000000000a01dcc",
    "4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d493479433333333333333333333333333333333333",
    "33333a045bce5168430c42b3d568331753f900a32457b4f3748697cbd8375ff4da72641a056e81f171bcc55a6ff8345e6",
    "92c0f86e5b48e01b996cadc001622fb5e363b421a056e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb",
    "5e363b421b901000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "00000000000000000000000000000000000000000085000010000080834c4b40808085434d272841800080000000c0c0",
);

/// rskj's test genesis, read under `heights` as the header of a block.
fn decoded_genesis(heights: &ActivationHeights) -> Header {
    let raw = hex::decode(GENESIS).unwrap();
    let (block, _) = rskj_rlp::first_element(&raw).unwrap();
    let parts = block.elements().unwrap();
    assert_eq!(parts.len(), 3, "header, transactions, uncles");
    let rskj_rlp::Element::List { encoding, .. } = parts[0] else {
        panic!("a header is a list");
    };
    read(encoding, heights, HeaderForm::Full).unwrap()
}

/// rskj `decodeGenesisBlock`: the genesis block, read with RSKIP92 on from
/// genesis, has testnet's genesis hash. rskj reads block 0 as a
/// `GenesisHeader`, which writes its difficulty back as the bytes it was
/// read from, `0x0000100000` here, and hashes those.
#[test]
fn the_genesis_block_reads_to_its_hash() {
    let genesis = decoded_genesis(&heights(|h| h.orchid = 0));
    let expected: B256 = "0xcabb7fbe88cd6d922042a32ffc08ce8b1fbb37d650b9d4e7dbfe2a7469adfa42"
        .parse()
        .unwrap();
    assert_eq!(genesis.hash(), expected);
}

/// rskj `genesisHasVersion0`: with no rule on, the genesis block reads as a
/// version-0 header, with the fields of testnet's genesis, its difficulty
/// held as the bytes it came as.
#[test]
fn the_genesis_block_reads_as_testnets_version_0_genesis() {
    let genesis = decoded_genesis(&heights(|_| {}));
    let testnet = ChainConfig::testnet().genesis_header();
    assert_eq!(genesis.version_fields, None);
    let difficulty = |header: &Header| header.kept().and_then(|kept| kept.difficulty.clone());
    assert_eq!(difficulty(&genesis), Some(hex!("0000100000").into()));
    assert_eq!(difficulty(&genesis), difficulty(&testnet));
    assert_eq!(
        Header {
            kept: None,
            ..genesis
        },
        Header {
            kept: None,
            ..testnet
        }
    );
}

/// rskj `newHeaderWithNoForkDetectionDataAndRskip110On`: a header without
/// merged-mining fields hashes as its merged-mining hash, which leaves
/// those fields out.
#[test]
fn a_header_without_mining_fields_hashes_as_its_merged_mining_hash() {
    let heights = heights(|h| {
        h.orchid = 0;
        h.wasabi100 = 0;
    });
    let header = built(20).with_activations(&heights);
    assert_eq!(header.hash(), header.hash_for_merged_mining());
}

// ── before RSKIPUMM and RSKIP351: sixteen fields, and three for mining

/// rskj `decodeBlockPriorToHeight449AndRskip110On`,
/// `decodeBlockPriorToHeight449AndRskip110Off`,
/// `decodeBlockAfterHeight449AndRskip110OFF` and
/// `decodeBlockAfterHeight449AndRskip110On`: a merge-mined header has 19
/// fields and reads back with its hash and the fork-detection data its
/// coinbase carries, below block 449 and above it, with RSKIP110 or without.
/// rskj's test writes the tag at the coinbase's start; here it follows the
/// coinbase's 40-byte SHA-256 midstate, where the proof-of-work rule looks
/// for it, and where rustock reads fork-detection data from.
#[test]
fn a_merge_mined_header_has_nineteen_fields_and_reads_back() {
    for (number, rskip110) in [(20, true), (20, false), (457, false), (457, true)] {
        let heights = heights(|h| {
            h.orchid = 0;
            if rskip110 {
                h.wasabi100 = 0;
            }
        });
        let mut header = mined(built(number), 80);
        if number == 457 && rskip110 {
            // The coinbase commits to the merged-mining hash's first twenty
            // bytes, then twelve of fork-detection data.
            let hash = header.hash_for_merged_mining();
            let fork_detection = keccak256("forkDetectionData");
            let tail = [RSK_TAG, &hash[..20], &fork_detection[..12]].concat();
            header.bitcoin_merged_mining_coinbase_transaction =
                Some(Bytes::from(compressed_coinbase(0, &tail)));
            assert_eq!(
                extract_fork_detection_data(&header).map(|data| data.to_vec()),
                Some(fork_detection[..12].to_vec())
            );
        }
        let encoded = full(&header);
        assert_eq!(count(&encoded), 19, "block {number}");
        let read = round_trip(&header, &encoded, &heights, HeaderForm::Full);
        assert_eq!(
            extract_fork_detection_data(&read),
            extract_fork_detection_data(&header)
        );
    }
}

/// rskj `decodeWithNoMergedMiningDataAndRskip110OffAndNoForkDetectionData`
/// and `decodeWithNoMergedMiningDataAndRskip110OffAndForkDetectionData`: a
/// header without mining fields has 16, and reads back with its hash.
/// Fork-detection data lives in the coinbase, so there is none to give it.
#[test]
fn a_header_without_mining_fields_has_sixteen_fields_and_reads_back() {
    let heights = heights(|h| h.orchid = 0);
    let header = built(20);
    let encoded = full(&header);
    assert_eq!(count(&encoded), 16);
    round_trip(&header, &encoded, &heights, HeaderForm::Full);
}

// ── RSKIPUMM: the UMM root

/// rskj `decodeBlockRskip110OffRskipUMMOnAndNoMergedMiningFieldsValidUMMRoot`,
/// `...AndNoMergedMiningFieldsEmptyUMMRoot`, `...AndNoMergedMiningFieldsNullUMMRoot`,
/// `...AndMergedMiningFieldsValidUMMRoot`, `...AndMergedMiningFieldsEmptyUmmRoot`
/// and `...AndMergedMiningFieldsNullUmmRoot`: from RSKIPUMM a header carries a
/// UMM root, twenty bytes or none, after its uncle count, and reads back with
/// it; one that carries no field for it isn't read.
#[test]
fn from_rskipumm_a_header_has_a_umm_root_or_isnt_read() {
    let heights = heights(|h| {
        h.orchid = 0;
        h.rskip_umm = Some(0);
    });
    for mining in [false, true] {
        for (umm_root, fields) in [
            (Some(twenty_bytes("ummRoot")), 17),
            (Some(Bytes::new()), 17),
            (None, 16),
        ] {
            let mut header = built(500);
            header.umm_root = umm_root.clone();
            if mining {
                header = mined(header, 80);
            }
            let encoded = full(&header);
            let fields = fields + if mining { 3 } else { 0 };
            assert_eq!(count(&encoded), fields);
            match umm_root {
                Some(root) => {
                    let read = round_trip(&header, &encoded, &heights, HeaderForm::Full);
                    assert_eq!(read.umm_root, Some(root));
                }
                None => assert!(
                    read(&encoded, &heights, HeaderForm::Full).is_err(),
                    "{fields} fields, no UMM root"
                ),
            }
        }
    }
}

// ── RSKIP351: the version

/// rskj `headerIsVersion0Before351Activation` and
/// `headerIsVersion1After351AndBefore535Activation`: a header built below
/// RSKIP351 is version 0; from it, without RSKIP535, version 1.
#[test]
fn a_header_is_version_1_from_rskip351_and_before_rskip535() {
    let heights = heights(|h| h.rskip351 = Some(20));
    assert_eq!(heights.header_version(19), 0);
    assert_eq!(heights.header_version(20), 1);
}

/// RSKIP144 and RSKIP351 from block 20, as rskj's tests below have them.
fn rskip351_at_20(also: impl FnOnce(&mut ActivationHeights)) -> ActivationHeights {
    heights(|h| {
        h.rskip144 = Some(20);
        h.rskip351 = Some(20);
        also(h);
    })
}

/// rskj `decodeCompressedBefore351`, `decodeCompressedOfExtendedBefore351`
/// and `decodeFullBefore351And144`: below RSKIP351 a header is version 0 and
/// has one encoding, its bloom in slot 6, whichever form it is read as.
#[test]
fn below_rskip351_a_header_has_one_encoding_with_its_bloom() {
    let heights = rskip351_at_20(|_| {});
    let mut header = built(19);
    header.logs_bloom = bloom();
    assert_eq!(header.encode_compressed(), full(&header));
    for form in [HeaderForm::Compressed, HeaderForm::Full] {
        let read = round_trip(&header, &full(&header), &heights, form);
        assert_eq!(read.version_fields, None);
        assert_eq!(read.extension_data, None);
        assert_eq!(read.logs_bloom, bloom());
    }
}

/// rskj `decodeCompressedWithNoEdgesAndMergedMiningFields`: a version-1
/// header with a UMM root and mining fields, sent compressed, reads back with
/// its hash, its UMM root, and slot 6's commitment to its extension, which
/// names version 1; its bloom and edges aren't in that form.
#[test]
fn a_compressed_version_1_header_with_mining_fields_reads_back() {
    let heights = rskip351_at_20(|h| {
        h.orchid = 0;
        h.rskip_umm = Some(0);
    });
    let mut header = mined(built(20), 80);
    header.umm_root = Some(Bytes::new());
    header.logs_bloom = bloom();
    let header = versioned(header, 1, None, None);
    let encoded = header.encode_compressed();
    assert_eq!(count(&encoded), 20);
    let read = round_trip(&header, &encoded, &heights, HeaderForm::Compressed);
    assert_eq!(commitment(&read), commitment(&header));
    assert_eq!(commitment(&read).version, 1);
    assert_eq!(read.version_fields, None);
    assert_eq!(read.logs_bloom, Bloom::ZERO);
    assert_eq!(read.umm_root, Some(Bytes::new()));
}

/// rskj `decodeCompressedAfter351WithEdges`: a version-1 header with edges,
/// sent compressed, reads back with slot 6's commitment to its extension,
/// version 1, and neither bloom nor edges.
#[test]
fn a_compressed_version_1_header_commits_to_its_edges() {
    let heights = rskip351_at_20(|_| {});
    let mut header = built(20);
    header.logs_bloom = bloom();
    let header = versioned(header, 1, Some(&[1, 2, 3, 4]), None);
    let encoded = header.encode_compressed();
    assert_eq!(count(&encoded), 16);
    let read = round_trip(&header, &encoded, &heights, HeaderForm::Compressed);
    let extension = header.extension().unwrap();
    assert_eq!(
        read.extension_commitment().unwrap().unwrap(),
        ExtensionCommitment::from_extension_data(&extension.create_extension_data()).unwrap()
    );
    assert_eq!(read.version_fields, None);
    assert_eq!(read.logs_bloom, Bloom::ZERO);
}

/// rskj `decodeFullAfter351WithEdges`: in full, a version-1 header carries
/// its bloom, its version and its edges, and reads back with them.
#[test]
fn a_full_version_1_header_carries_its_bloom_and_edges() {
    let heights = rskip351_at_20(|_| {});
    let mut header = built(20);
    header.logs_bloom = bloom();
    let header = versioned(header, 1, Some(&[1, 2, 3, 4]), None);
    let encoded = full(&header);
    assert_eq!(count(&encoded), 18);
    let read = round_trip(&header, &encoded, &heights, HeaderForm::Full);
    assert_eq!(read.logs_bloom, bloom());
    assert_eq!(read.version_fields, header.version_fields);
    assert_eq!(read.extension(), header.extension());
    let extension = header.extension().unwrap();
    assert_eq!(
        commitment(&read),
        ExtensionCommitment::from_extension_data(&extension.create_extension_data()).unwrap()
    );
}

// ── RSKIP144: the edges

/// rskj `decodeBlockRskip144OnRskipUMMOnAndMergedMiningFields` (22 fields),
/// `decodeBlockHeaderRSKIP1444ON_UMMOn_MergedMiningFieldsOFF_RSKIP5350FF`
/// (19), `decodeBlockRskip144OnRskipUMMOffAndMergedMiningFields` (21) and
/// `decodeBlockRskip144OnRskipUMMOffAndNoMergedMiningFields` (18): with
/// RSKIP144 and RSKIP351 a full header carries its version and its edges,
/// after the UMM root where there is one, and reads back with them.
#[test]
fn with_rskip144_a_full_header_carries_its_edges() {
    for (umm, mining, fields) in [
        (true, true, 22),
        (true, false, 19),
        (false, true, 21),
        (false, false, 18),
    ] {
        let heights = heights(|h| {
            h.rskip144 = Some(0);
            h.rskip351 = Some(0);
            if umm {
                h.rskip_umm = Some(0);
            }
        });
        let mut header = built(500);
        header.logs_bloom = bloom();
        if umm {
            header.umm_root = Some(Bytes::new());
        }
        if mining {
            header = mined(header, 80);
        }
        let header = versioned(header, 1, Some(&EDGES), None);
        let encoded = full(&header);
        assert_eq!(count(&encoded), fields, "UMM {umm}, mining {mining}");
        let read = round_trip(&header, &encoded, &heights, HeaderForm::Full);
        assert_eq!(read.version_fields.unwrap().edges, Some(EDGES.to_vec()));
    }
}

// ── RSKIP535: the base event

/// rskj `decodeBlockRskip535WithoutRskip351`: RSKIP535 needs RSKIP351, so
/// without it a header is version 0, with no base event, in 16 fields.
#[test]
fn without_rskip351_rskip535_adds_no_base_event() {
    let heights = heights(|h| {
        h.orchid = 0;
        h.rskip535 = Some(0);
    });
    assert_eq!(heights.header_version(500), 0);
    let header = built(500);
    let encoded = full(&header);
    assert_eq!(count(&encoded), 16);
    let read = round_trip(&header, &encoded, &heights, HeaderForm::Full);
    assert_eq!(read.version_fields, None);
}

/// rskj `decodeBlockRskip535WithoutRskip144` (18 fields),
/// `decodeBlockRskip535WithRskip144AndNoMergedMiningFields` (19),
/// `decodeBlockRskip535WithUMMAndNoMergedMiningFields` (19),
/// `decodeBlockRskip535WithRskip144AndUMMAndNoMergedMiningFields` (20) and
/// `decodeBlockRskip535WithEmptyBaseEvent` (18): with RSKIP351 and RSKIP535 a
/// full header is version 2 and carries its base event, empty or not, after
/// its edges where RSKIP144 gives it some, and reads back with them.
#[test]
fn with_rskip535_a_full_header_carries_its_base_event() {
    for (umm, edges, event, fields) in [
        (false, false, base_event(), 18),
        (false, true, base_event(), 19),
        (true, false, base_event(), 19),
        (true, true, base_event(), 20),
        (false, false, Vec::new(), 18),
    ] {
        let heights = heights(|h| {
            h.orchid = 0;
            h.rskip351 = Some(0);
            h.rskip535 = Some(0);
            if edges {
                h.rskip144 = Some(0);
            }
            if umm {
                h.rskip_umm = Some(0);
            }
        });
        assert_eq!(heights.header_version(500), 2);
        let mut header = built(500);
        if umm {
            header.umm_root = Some(twenty_bytes("ummRoot"));
        }
        let header = versioned(header, 2, edges.then_some(&EDGES[..]), Some(&event));
        let encoded = full(&header);
        assert_eq!(count(&encoded), fields, "UMM {umm}, edges {edges}");
        let read = round_trip(&header, &encoded, &heights, HeaderForm::Full);
        let version = read.version_fields.unwrap();
        assert_eq!(version.version, 2);
        assert_eq!(version.base_event, Some(Bytes::from(event)));
        assert_eq!(version.edges, edges.then(|| EDGES.to_vec()));
        assert_eq!(read.umm_root, header.umm_root);
    }
}

/// rskj `decodeCompressedBlockRskip535WithRskip144`,
/// `decodeCompressedBlockRskip535WithoutRskip144` and
/// `decodeCompressedBlockRskip535WithUMMWithoutRskip144`: a version-2 header
/// sent compressed reads back with its hash and slot 6's commitment, which
/// names version 2, and neither edges nor base event, which that form
/// leaves to the extension.
#[test]
fn a_compressed_version_2_header_commits_to_its_base_event() {
    for (umm, edges) in [(false, true), (false, false), (true, false)] {
        let heights = heights(|h| {
            h.orchid = 0;
            h.rskip351 = Some(0);
            h.rskip535 = Some(0);
            if edges {
                h.rskip144 = Some(0);
            }
            if umm {
                h.rskip_umm = Some(0);
            }
        });
        let mut header = built(500);
        header.logs_bloom = bloom();
        if umm {
            header.umm_root = Some(twenty_bytes("ummRoot"));
        }
        let event = base_event();
        let header = versioned(header, 2, edges.then_some(&EDGES[..]), Some(&event));
        let encoded = header.encode_compressed();
        let read = round_trip(&header, &encoded, &heights, HeaderForm::Compressed);
        assert_eq!(commitment(&read), commitment(&header));
        assert_eq!(commitment(&read).version, 2);
        assert_eq!(read.version_fields, None, "UMM {umm}, edges {edges}");
        assert_eq!(read.umm_root, header.umm_root);
    }
}

/// rskj `decodeFullBlockRskip535WithRskip144`: in full, a version-2 header
/// with edges reads back with its bloom, edges and base event.
#[test]
fn a_full_version_2_header_carries_its_bloom_edges_and_base_event() {
    let heights = heights(|h| {
        h.orchid = 0;
        h.rskip144 = Some(0);
        h.rskip351 = Some(0);
        h.rskip535 = Some(0);
    });
    let mut header = built(500);
    header.logs_bloom = bloom();
    let event = base_event();
    let header = versioned(header, 2, Some(&EDGES), Some(&event));
    let read = round_trip(&header, &full(&header), &heights, HeaderForm::Full);
    assert_eq!(read.logs_bloom, bloom());
    assert_eq!(read.version_fields, header.version_fields);
}

/// rskj `emptyBaseEventPreservedAcrossExtensionRoundTripWithMining` and
/// `emptyBaseEventPreservedAcrossExtensionRoundTripNoMining`: an empty base
/// event survives the extension's own encoding, keeps its field in the
/// header's (23 fields with mining), and reads back empty, the hash the same.
#[test]
fn an_empty_base_event_keeps_its_field_and_reads_back_empty() {
    for mining in [true, false] {
        let heights = heights(|h| {
            h.orchid = 0;
            h.rskip144 = Some(0);
            h.rskip351 = Some(0);
            h.rskip535 = Some(0);
            if mining {
                h.rskip_umm = Some(0);
            }
        });
        let mut header = built(500);
        if mining {
            header = mined(header, 80);
            header.umm_root = Some(twenty_bytes("ummRoot"));
        }
        let header = versioned(header, 2, Some(&EDGES), Some(&[]));
        let extension = header.extension().unwrap();
        let from_wire = BlockHeaderExtension::from_encoded(&extension.to_encoded()).unwrap();
        assert_eq!(from_wire.base_event(), Some(&[][..]));
        assert_eq!(from_wire, extension);
        let encoded = full(&header);
        if mining {
            assert_eq!(count(&encoded), 23);
        }
        let read = round_trip(&header, &encoded, &heights, HeaderForm::Full);
        assert_eq!(read.version_fields.unwrap().base_event, Some(Bytes::new()));
    }
}

// ── testnet's one-field-short header

/// RSKIP351, RSKIP535, RSKIP144 and RSKIPUMM from genesis, merged mining
/// with them, on the chain whose other heights `chain` gives.
fn base_event_rules(chain: ActivationHeights, umm: bool) -> ActivationHeights {
    ActivationHeights {
        orchid: 0,
        rskip144: Some(0),
        rskip351: Some(0),
        rskip535: Some(0),
        rskip_umm: umm.then_some(0),
        ..chain
    }
}

/// A version-2 header at `number` written without its base event's field, as
/// a version-2 extension with none writes it: 22 fields with mining.
fn one_field_short(number: u64, mining: bool) -> Header {
    let mut header = built(number);
    if mining {
        header = mined(header, 80);
        header.umm_root = Some(twenty_bytes("ummRoot"));
    }
    versioned(header, 2, Some(&EDGES), None)
}

/// rskj `headerOneElementShortDecodableOnTestnetWithinBoundWithMining` and
/// `headerOneElementShortDecodableOnTestnetWithinBoundNoMining`: on testnet,
/// below #7,800,000, a full header one field short of its layout is read,
/// with an empty base event, whether the three mining fields follow or not.
#[test]
fn testnet_reads_a_header_one_field_short_with_an_empty_base_event() {
    for mining in [true, false] {
        let heights = base_event_rules(ActivationHeights::testnet(), mining);
        let header = one_field_short(500, mining);
        let encoded = full(&header);
        if mining {
            assert_eq!(count(&encoded), 22);
        }
        let read = read(&encoded, &heights, HeaderForm::Full).unwrap();
        let version = read.version_fields.unwrap();
        assert_eq!(version.base_event, Some(Bytes::new()));
        assert_eq!(version.edges, Some(EDGES.to_vec()));
    }
}

/// rskj `headerOneElementShortRejectedOnMainnetFactory`,
/// `headerOneElementShortRejectedAboveUpperBound` and
/// `headerOneElementShortRejectedOnNonTestnetChainId`: that tolerance is
/// testnet's alone, and ends at #7,800,000.
#[test]
fn no_other_chain_and_no_later_block_reads_a_header_one_field_short() {
    for (name, chain, number, is_read) in [
        ("mainnet", ActivationHeights::mainnet(), 500, false),
        ("regtest", ActivationHeights::regtest(), 500, false),
        ("testnet", ActivationHeights::testnet(), 7_800_000, false),
        ("testnet", ActivationHeights::testnet(), 7_799_999, true),
    ] {
        let heights = base_event_rules(chain, true);
        let encoded = full(&one_field_short(number, true));
        assert_eq!(count(&encoded), 22);
        assert_eq!(
            read(&encoded, &heights, HeaderForm::Full).is_ok(),
            is_read,
            "{name}, block {number}"
        );
    }
}

/// rskj `headerOneElementShortWithBaseEventPresentRejectedOnTestnet`: a
/// header that keeps its base event and drops a mining field is the same
/// length, so testnet reads its base event in the Bitcoin header's place,
/// which RSKIP98 holds to 80 bytes, and refuses it.
#[test]
fn a_header_short_of_a_mining_field_is_not_one_short_of_its_base_event() {
    let heights = base_event_rules(ActivationHeights::testnet(), true);
    let mut header = mined(built(500), 80);
    header.umm_root = Some(twenty_bytes("ummRoot"));
    let event = base_event();
    let header = versioned(header, 2, Some(&EDGES), Some(&event));
    let encoded = full(&header);
    assert_eq!(count(&encoded), 23);
    round_trip(&header, &encoded, &heights, HeaderForm::Full);

    let (list, _) = rskj_rlp::first_element(&encoded).unwrap();
    let fields = list.elements().unwrap();
    let mut payload = Vec::new();
    for field in &fields[..fields.len() - 1] {
        field.raw_data().encode(&mut payload);
    }
    let mut truncated = Vec::new();
    alloy_rlp::Header {
        list: true,
        payload_length: payload.len(),
    }
    .encode(&mut truncated);
    truncated.extend_from_slice(&payload);
    assert_eq!(count(&truncated), 22);
    assert!(read(&truncated, &heights, HeaderForm::Full).is_err());
}

/// rskj `compressedHeaderRejectedWhenSizeDoesNotMatch`: the tolerance is the
/// full encoding's; read as compressed, the same 22 fields are none of the
/// counts that form has, and the header isn't read.
#[test]
fn a_compressed_header_is_not_read_one_field_short() {
    let heights = base_event_rules(ActivationHeights::testnet(), true);
    let encoded = full(&one_field_short(500, true));
    assert!(read(&encoded, &heights, HeaderForm::Full).is_ok());
    assert!(read(&encoded, &heights, HeaderForm::Compressed).is_err());
}

// ── RSKIP98: the Bitcoin header's size

/// rskj `decodeHeaderValidatesBtcHeaderSize`: from RSKIP98 a header is read
/// with a Bitcoin header of 80 bytes, and not of 79, 81 or 160.
#[test]
fn from_rskip98_the_bitcoin_header_is_read_at_80_bytes_only() {
    let heights = heights(|h| h.orchid = 0);
    for size in [80, 79, 81, 160] {
        let header = mined(built(20), size);
        let encoded = full(&header);
        if size == 80 {
            let read = round_trip(&header, &encoded, &heights, HeaderForm::Full);
            assert_eq!(read.bitcoin_merged_mining_header.unwrap().len(), 80);
        } else {
            assert!(
                read(&encoded, &heights, HeaderForm::Full).is_err(),
                "{size} bytes"
            );
        }
    }
}

/// rskj `decodeHeaderRejectsOversizedBtcHeaderWithVariousActiveRules`: an
/// 81-byte Bitcoin header isn't read after a UMM root, nor after a version
/// and edges.
#[test]
fn an_oversized_bitcoin_header_is_refused_after_the_umm_root_or_the_edges() {
    let umm = heights(|h| {
        h.orchid = 0;
        h.rskip_umm = Some(0);
    });
    let mut header = mined(built(500), 81);
    header.umm_root = Some(Bytes::new());
    assert!(read(&full(&header), &umm, HeaderForm::Full).is_err());
    header.bitcoin_merged_mining_header = Some(Bytes::from(vec![0; 80]));
    assert!(read(&full(&header), &umm, HeaderForm::Full).is_ok());

    let edges = heights(|h| {
        h.orchid = 0;
        h.rskip144 = Some(0);
        h.rskip351 = Some(0);
    });
    let header = versioned(mined(built(500), 81), 1, Some(&EDGES), None);
    assert!(read(&full(&header), &edges, HeaderForm::Full).is_err());
    let header = versioned(mined(built(500), 80), 1, Some(&EDGES), None);
    assert!(read(&full(&header), &edges, HeaderForm::Full).is_ok());
}
