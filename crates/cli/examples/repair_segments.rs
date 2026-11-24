//! Fill the holes in segbuild segments by re-executing the blocks that
//! exposed them, writing every node the segment could not serve.
//!
//! A segment holds the trie nodes its *build* touched, not the whole state at
//! its heights, so newer code that reads state the builder never read gets
//! `None` where the chain has a value. That is unsound and silently so: it
//! produced a false consensus-divergence report against #374
//! (`docs/trie-segments-design.md` 6.9d, issues #397/#398).
//!
//! Why re-execute rather than copy the recorded keys. Nodes are
//! content-addressed, so the gap list could in principle be copied straight
//! across -- but a recorded list is capped and therefore partial, and
//! inserting a node whose children are still missing only moves the failure
//! one level down. Re-execution discovers exactly the nodes the walk needs,
//! transitively and in dependency order.
//!
//! Operates on a WRITABLE COPY. The sealed segments are a 135 GB asset built
//! over days; repairing a copy means a mistake costs a re-copy, and leaves the
//! originals sealed until someone has read the diff.
//!
//! Usage:
//!   repair_segments <block-dir> <segments-rw-root> <fallback-trie> <gap-file> [--verify-only]
//!
//! `<gap-file>` is the `block<TAB>segment<TAB>key` file the full re-execution
//! writes; only its block and segment columns are used, so a capped file is
//! still a complete work list as long as every affected block appears once.
use anyhow::Context;
use rustock_core::Block;
use rustock_execution::{BlockProcessor, RskHardforkConfig};
use rustock_storage::{BlockStore, RocksDbTrieStore};
use rustock_trie::{TrieNode, TrieStore};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Reads the segment, falls back to the complete trie, and writes what it had
/// to fall back for straight into the segment.
///
/// `put` deliberately discards: execution's own new nodes are not part of the
/// segment's job here, and persisting them would grow it with state no build
/// ever captured. Only the explicit repair write happens.
struct RepairStore {
    segment: RocksDbTrieStore,
    /// Shared across every segment, and opened once.
    ///
    /// The archival trie is 131 GB and read-only, and opening it costs 15-21
    /// seconds. Opening it inside the per-segment loop spent about twenty
    /// minutes of a thirty-minute run reopening the same database 77 times.
    full: Arc<RocksDbTrieStore>,
    inserted: AtomicU64,
    served: AtomicU64,
}

impl TrieStore for RepairStore {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        if let Some(v) = self.segment.get(key) {
            self.served.fetch_add(1, Ordering::Relaxed);
            return Some(v);
        }
        let v = self.full.get(key)?;
        self.segment.put(key, &v);
        self.inserted.fetch_add(1, Ordering::Relaxed);
        Some(v)
    }
    fn put(&self, _key: &[u8], _value: &[u8]) {}
    fn flush(&self) {
        self.segment.flush();
    }
}

/// Segment-only, for the verification pass: a miss must be visible, not
/// papered over, so this returns `None` and the replay fails loudly.
struct SegmentOnly {
    segment: RocksDbTrieStore,
    misses: AtomicU64,
}

impl TrieStore for SegmentOnly {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        let v = self.segment.get(key);
        if v.is_none() {
            self.misses.fetch_add(1, Ordering::Relaxed);
        }
        v
    }
    fn put(&self, _key: &[u8], _value: &[u8]) {}
    fn flush(&self) {}
}

fn exec_block(
    store: &Arc<BlockStore>,
    trie: Arc<dyn TrieStore>,
    number: u64,
) -> anyhow::Result<bool> {
    let parent_hash = store
        .canonical_hash(number - 1)?
        .context("no canonical hash for parent")?;
    let parent = store.header(parent_hash)?.context("no parent header")?;
    let root_data = trie
        .get(parent.state_root.as_slice())
        .with_context(|| format!("no pre-state for #{}", number - 1))?;
    let root = TrieNode::from_message(&root_data, trie.as_ref());

    let hash = store.canonical_hash(number)?.context("no canonical hash")?;
    let header = store.header(hash)?.context("no header")?;
    let (transactions, ommers) = store.body(hash)?.context("no body")?;
    let expected = header.state_root;
    let block = Block { header, transactions, ommers };
    let processor = BlockProcessor::new(RskHardforkConfig::mainnet(), store.clone());
    let p = processor.execute_block(&block, &root, trie.clone())?;
    Ok(p.state_root_hash.as_slice() == expected.as_slice())
}

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 {
        eprintln!(
            "usage: repair_segments <block-dir> <segments-rw-root> <fallback-trie> <gap-file> [--verify-only]"
        );
        std::process::exit(2);
    }
    let (block_dir, seg_root, full_dir, gap_file) =
        (a[1].clone(), a[2].clone(), a[3].clone(), a[4].clone());
    let verify_only = a.iter().any(|x| x == "--verify-only");

    rustock_execution::supply::set_per_transaction(false);
    rustock_execution::supply::set_per_block(false);

    // segment -> the blocks that needed a fallback in it
    let mut work: BTreeMap<String, BTreeSet<u64>> = BTreeMap::new();
    for line in std::fs::read_to_string(&gap_file)?.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let mut f = line.split('\t');
        let (Some(b), Some(seg)) = (f.next(), f.next()) else { continue };
        if let Ok(n) = b.trim().parse::<u64>() {
            work.entry(seg.trim().to_string()).or_default().insert(n);
        }
    }
    let blocks: usize = work.values().map(|s| s.len()).sum();
    println!("work: {} segments, {blocks} blocks", work.len());

    let store = Arc::new(BlockStore::open_read_only(&block_dir)?);
    // Opened once, before the loop: see `RepairStore::full`. Skipped entirely
    // under --verify-only, which never reads it.
    let full: Arc<RocksDbTrieStore> = if verify_only {
        Arc::new(RocksDbTrieStore::open_read_only(&full_dir)?)
    } else {
        let t = std::time::Instant::now();
        let f = Arc::new(RocksDbTrieStore::open_read_only(&full_dir)?);
        println!("fallback trie opened in {:.1}s (once, shared by every segment)", t.elapsed().as_secs_f64());
        f
    };
    let mut total_inserted = 0u64;
    let mut failures = 0usize;

    for (seg, nums) in &work {
        let dir = format!("{seg_root}/{seg}/sealed");
        if !std::path::Path::new(&dir).join("CURRENT").exists() {
            println!("  {seg}: MISSING at {dir} -- skipped");
            failures += 1;
            continue;
        }

        if !verify_only {
            let rs = Arc::new(RepairStore {
                segment: RocksDbTrieStore::open(&dir)?,
                full: Arc::clone(&full),
                inserted: AtomicU64::new(0),
                served: AtomicU64::new(0),
            });
            let trie: Arc<dyn TrieStore> = rs.clone();
            let mut ok = true;
            for n in nums {
                match exec_block(&store, trie.clone(), *n) {
                    Ok(true) => {}
                    Ok(false) => {
                        println!("  {seg} #{n}: STATE ROOT MISMATCH even with the fallback");
                        ok = false;
                    }
                    Err(e) => {
                        println!("  {seg} #{n}: {e:#}");
                        ok = false;
                    }
                }
            }
            trie.flush();
            let ins = rs.inserted.load(Ordering::Relaxed);
            total_inserted += ins;
            println!(
                "  {seg}: {} block(s), {ins} nodes inserted, {} served by the segment{}",
                nums.len(),
                rs.served.load(Ordering::Relaxed),
                if ok { "" } else { "  [EXECUTION PROBLEM]" }
            );
            if !ok {
                failures += 1;
            }
        }

        // Verification: segment only, no fallback. Every root must match and
        // nothing may miss -- that is the whole point of the repair.
        let so = Arc::new(SegmentOnly {
            segment: RocksDbTrieStore::open_read_only(&dir)?,
            misses: AtomicU64::new(0),
        });
        let trie: Arc<dyn TrieStore> = so.clone();
        let mut verified = true;
        for n in nums {
            match exec_block(&store, trie.clone(), *n) {
                Ok(true) => {}
                Ok(false) => {
                    println!("  {seg} #{n}: VERIFY FAILED (root differs, segment only)");
                    verified = false;
                }
                Err(e) => {
                    println!("  {seg} #{n}: VERIFY FAILED ({e:#})");
                    verified = false;
                }
            }
        }
        let misses = so.misses.load(Ordering::Relaxed);
        println!(
            "  {seg}: verify {} ({misses} reads still missing)",
            if verified && misses == 0 { "OK" } else { "FAILED" }
        );
        if !verified {
            failures += 1;
        }
    }

    println!("\n{total_inserted} nodes inserted across {} segments", work.len());
    if failures > 0 {
        println!("{failures} segment(s) did not come clean -- do NOT swap these in");
        std::process::exit(1);
    }
    println!("all repaired segments verify with no fallback: safe to swap in");
    Ok(())
}
