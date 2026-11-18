//! rskj `org.ethereum.core.BlockHeaderBuilderTest`: the fields a header is
//! built with, by the rules active at its height.
//!
//! rustock's miner fills a header's fields as rskj's `BlockHeaderBuilder`
//! does, which the miner's own tests hold; this holds what those fields come
//! to in the encoding blocks carry and in the hash.

use super::block_factory_test::{built, count, full, heights, mined};
use crate::rskj_rlp;
use crate::types::header::HeaderVersionFields;
use alloy_primitives::{keccak256, Bytes};

/// `encoded`, a header, without its last `drop` fields.
fn without_last(encoded: &[u8], drop: usize) -> Vec<u8> {
    let (list, _) = rskj_rlp::first_element(encoded).unwrap();
    let rskj_rlp::Element::List { encoding, offset } = list else {
        panic!("a header is a list");
    };
    let payload = &encoding[offset..];
    let mut ends = Vec::new();
    let mut position = 0;
    while position < payload.len() {
        let (_, next) = rskj_rlp::first_element(&payload[position..]).unwrap();
        position += next;
        ends.push(position);
    }
    let kept = &payload[..ends[ends.len() - 1 - drop]];
    let mut out = Vec::new();
    alloy_rlp::Header {
        list: true,
        payload_length: kept.len(),
    }
    .encode(&mut out);
    out.extend_from_slice(kept);
    out
}

/// rskj `testHeaderCreationWithParameters`: a header built under RSKIPUMM
/// with the three merged-mining fields has 20 fields; RSKIP351 adds its
/// version and edges (22), and RSKIP535 its base event (23), which without
/// RSKIP351 adds none. Built with RSKIP92's encoding, rskj's test counts
/// those fields but the merkle proof and the coinbase (18, 20, 21): rskj
/// writes that form only to its RPC, and hashes the header compressed, a
/// versioned header's own fields in slot 6's commitment. From RSKIP92 the
/// hash leaves the proof and the coinbase out, whatever the version.
#[test]
fn a_built_header_carries_the_fields_its_rules_give_it() {
    // rskj's cases: consensus-compliant, RSKIP92's encoding, RSKIP351,
    // RSKIP535, and the fields it counts. A consensus-compliant header uses
    // RSKIP92's encoding whatever it is asked, and RSKIP144 is on with
    // RSKIP351 and off without it.
    for (compliant, rskip92_encoding, rskip351, rskip535, rskj_count) in [
        (false, false, false, false, 20),
        (false, false, false, true, 20),
        (false, false, true, false, 22),
        (false, false, true, true, 23),
        (false, true, false, false, 18),
        (false, true, false, true, 18),
        (false, true, true, false, 20),
        (false, true, true, true, 21),
        (true, false, false, false, 18),
        (true, false, false, true, 18),
        (true, false, true, false, 20),
        (true, false, true, true, 21),
    ] {
        let rskip92 = compliant || rskip92_encoding;
        let heights = heights(|h| {
            h.rskip_umm = Some(0);
            if rskip92 {
                h.orchid = 0;
            }
            if rskip351 {
                h.rskip144 = Some(0);
                h.rskip351 = Some(0);
            }
            if rskip535 {
                h.rskip535 = Some(0);
            }
        });
        // As rskj's builder fills them: an empty UMM root, empty edges and
        // an empty base event.
        let mut header = mined(built(1), 80);
        header.umm_root = Some(Bytes::new());
        let version = heights.header_version(1);
        if version > 0 {
            header.version_fields = Some(HeaderVersionFields {
                version,
                edges: Some(Vec::new()),
                base_event: (version == 2).then(Bytes::new),
            });
        }
        let header = header.with_activations(&heights);
        let case = format!("{compliant} {rskip92_encoding} {rskip351} {rskip535}");
        let fields = count(&full(&header));
        let compressed = header.encode_compressed();
        if rskip92 {
            assert_eq!(fields - 2, rskj_count, "{case}");
            let hashed = without_last(&compressed, 2);
            assert_eq!(count(&hashed), 18, "{case}");
            assert_eq!(header.hash(), keccak256(&hashed), "{case}");
        } else {
            assert_eq!(fields, rskj_count, "{case}");
            assert_eq!(header.hash(), keccak256(&compressed), "{case}");
        }
    }
}
