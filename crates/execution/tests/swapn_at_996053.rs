//! Does removing `SWAPN` change mainnet block #996,053?
//!
//! The whole-chain run of 2026-11-22 found exactly one `SWAPN` execution in
//! 9,429,718 blocks, at #996,053, transaction 0, in contract
//! `0x56bF637ee636D8ebDE45a046c45DEac6eF67a019`. The transaction is a contract
//! **creation** whose init code is 68 octets of `transfer(address,uint256)`
//! calldata -- someone sent an ERC-20 transfer with an empty `to`. Its first
//! octet is `0xa9`, which below iris300 is `SWAPN`, and the octet after it is
//! its operand. `SWAPN 5` on an empty stack underflows, the creation fails,
//! and the address holds no account to this day.
//!
//! So `EVM-045`'s window is occupied -- but perhaps only nominally. With the
//! opcode present, `0xa9` underflows: an exceptional halt consuming all gas.
//! With it gone, `0xa9` is `OpcodeNotFound`: also an exceptional halt consuming
//! all gas. If both leave the same state, RSKIP-191 removed nothing this chain
//! depended on, and the count of one is not an obstacle to retiring the branch.
//!
//! **That is a claim about consensus, so it is measured here rather than argued.**
//! This test replays the real block against the real pre-state and asserts the
//! computed root matches the header. Run it once as it stands, then once with
//! `SWAPN` replaced by the invalid-opcode handler in
//! `rsk_instructions::install`, and compare. One instruction changes between
//! the two runs and nothing else -- moving `iris300` instead would also move
//! RSKIP-218's REMASC recipient and RSKIP-174's created-at-a-funded-address
//! rule, either of which would shift the root for reasons unrelated to `SWAPN`.
//!
//! ```text
//! RUSTOCK_BLOCKS_DB=/var/lib/rustock \
//! RUSTOCK_ARCHIVE_TRIE=<archival trie> \
//!   cargo test -p rustock-execution --test swapn_at_996053 -- --ignored --nocapture
//! ```

use std::sync::Arc;

const BLOCK: u64 = 996_053;

/// The pre-RSKIP126 root baseline: one Unitrie root per block for 1..=1,591,000.
///
/// Read directly rather than through the CLI's `support::baseline`, which is an
/// example-local module a crate test cannot reach. Layout, little-endian:
/// magic `RSKBASE1` (8), schema (4), reserved (4), first (8), last (8),
/// anchor (32), then roots at a fixed 32-octet stride.
fn baseline_root(path: &str, block: u64) -> Option<[u8; 32]> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 64 || &bytes[0..8] != b"RSKBASE1" {
        return None;
    }
    let first = u64::from_le_bytes(bytes[16..24].try_into().ok()?);
    let last = u64::from_le_bytes(bytes[24..32].try_into().ok()?);
    if block < first || block > last {
        return None;
    }
    let at = 64 + (block - first) as usize * 32;
    bytes.get(at..at + 32)?.try_into().ok()
}

#[test]
#[ignore = "requires RUSTOCK_BLOCKS_DB and RUSTOCK_ARCHIVE_TRIE"]
fn block_996053_replays_and_reports_its_root() {
    let (Ok(db), Ok(archive), Ok(baseline)) = (
        std::env::var("RUSTOCK_BLOCKS_DB"),
        std::env::var("RUSTOCK_ARCHIVE_TRIE"),
        std::env::var("RUSTOCK_BASELINE"),
    ) else {
        eprintln!("RUSTOCK_BLOCKS_DB / RUSTOCK_ARCHIVE_TRIE / RUSTOCK_BASELINE not set; skipping");
        return;
    };

    let blocks = Arc::new(rustock_storage::BlockStore::open_read_only(&db).expect("block store"));
    let trie: Arc<dyn rustock_trie::TrieStore> =
        Arc::new(rustock_storage::RocksDbTrieStore::open_read_only(&archive).expect("archive"));

    let hash = blocks.canonical_hash(BLOCK).unwrap().expect("canonical hash");
    let header = blocks.header(hash).unwrap().expect("header");
    let (transactions, ommers) = blocks.body(hash).unwrap().expect("body");
    // #996,053 is below #1,591,000, so BOTH the pre-state and the expected root
    // come from the baseline: the headers of that era declare Orchid-format
    // roots, which name no node in a Unitrie store and cannot be compared
    // against a computed Unitrie root.
    let seed = baseline_root(&baseline, BLOCK - 1).expect("no baseline root for the parent");
    let expected = baseline_root(&baseline, BLOCK).expect("no baseline root for the block");
    let pre_bytes = trie.get(&seed).expect("pre-state root absent from the archive");
    let pre = rustock_trie::TrieNode::from_message(&pre_bytes, trie.as_ref());
    let processor = rustock_execution::BlockProcessor::new(
        rustock_execution::RskHardforkConfig::mainnet(),
        blocks.clone(),
    );
    let block = rustock_core::Block { header, transactions, ommers };
    let out = processor
        .execute_block(&block, &pre, trie.clone())
        .expect("execution failed outright");

    println!("block   #{BLOCK}");
    println!("  baseline root 0x{}", expected.iter().map(|b| format!("{b:02x}")).collect::<String>());
    println!("  computed root {:?}", out.state_root_hash);
    println!("  block gas_used {}  paid_fees {}", out.gas_used, out.paid_fees);
    for (i, r) in out.receipts.iter().enumerate() {
        println!("  receipt {i}: {r:?}");
    }
    assert_eq!(
        out.state_root_hash.as_slice(),
        expected.as_slice(),
        "computed root differs from the baseline -- with SWAPN removed this is the \
         finding; with SWAPN present it means the fixture is wrong"
    );
}
