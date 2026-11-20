//! How high does an archive trie store actually reach?
//!
//! The segment builder replays blocks forward, so it only needs the seed state
//! at its first block -- but every window eviction falls back to the archive,
//! and a fallback that misses is indistinguishable from a consensus failure.
//! Knowing the archive's real upper bound is therefore a precondition for
//! backfilling, not a curiosity.
//!
//! Presence of a height's state root is the test. It is a necessary condition,
//! not a sufficient one: a present root proves the top node was retained, not
//! that the whole trie below it was. Treat the answer as an upper bound on what
//! the archive can serve.
//!
//! Usage: archive_bound <block-dir> <archive-dir> [low] [high]

use rustock_storage::{BlockStore, RocksDbTrieStore};
use rustock_trie::TrieStore;

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let blocks = BlockStore::open_read_only(&a[1])?;
    let archive = RocksDbTrieStore::open_read_only(&a[2])?;

    let has = |n: u64| -> anyhow::Result<bool> {
        let Some(hash) = blocks.canonical_hash(n)? else { return Ok(false) };
        let Some(h) = blocks.header(hash)? else { return Ok(false) };
        Ok(archive.get(h.state_root.as_slice()).is_some())
    };

    let mut low: u64 = a.get(3).map_or(1, |s| s.parse().unwrap());
    let mut high: u64 = a.get(4).map_or(9_430_000, |s| s.parse().unwrap());

    println!("probing #{low} and #{high} first");
    if !has(low)? {
        println!("#{low} ABSENT -- low bound is not in the archive, nothing to bisect");
        return Ok(());
    }
    if has(high)? {
        println!("#{high} PRESENT -- archive reaches at least the high bound");
        return Ok(());
    }
    println!("#{low} present, #{high} absent; bisecting");

    // Invariant: has(low), !has(high).
    while high - low > 1 {
        let mid = low + (high - low) / 2;
        let p = has(mid)?;
        println!("  #{mid} {}", if p { "present" } else { "ABSENT" });
        if p { low = mid } else { high = mid }
    }
    println!("highest state root present in archive: #{low} (#{high} absent)");
    Ok(())
}
