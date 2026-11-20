//! Rebuild the tail of a pre-RSKIP126 baseline, from the first block whose
//! recorded root the current build no longer reproduces.
//!
//! A stale baseline does not need recomputing from genesis. Roots below the
//! first affected block are still right -- `baseline_probe` is how you
//! establish that cheaply, block by block -- and the baseline itself supplies
//! the pre-state to start from. So the work is the tail, chained, because once
//! a block's post-state differs every root after it differs too.
//!
//! The run ends on the anchor, and that is the point of it. #1,591,000 is the
//! first block whose header carries a Unitrie root, so whether the rebuilt
//! tail arrives at that header decides which build is right:
//!
//! - **anchor matches**: the current build reproduces the chain's cumulative
//!   pre-fork state. The roots it just computed are the good ones and the old
//!   baseline was recording a defect.
//! - **anchor fails**: the current build does not agree with the chain at the
//!   fork. The old baseline did. That is a consensus regression, and this tool
//!   says so rather than writing the file.
//!
//! Either way nothing is overwritten: the output is a new file, and the old
//! one is left for comparison.
//!
//! Usage:
//!   baseline_rebuild <block-dir> <trie-dir> <old-baseline> <from> <out-file>
//!                    [--scratch DIR] [--progress FILE]
//!
//! `from` is the first block to recompute; roots below it are copied across
//! unchanged. `--scratch` is where the replay's new nodes go, since the base
//! store is read-only and the state does not fit in memory for 860k blocks.

use anyhow::{Context, Result};
use rustock_core::Block;
use rustock_execution::{BlockProcessor, RskHardforkConfig};
use rustock_storage::{BlockStore, RocksDbTrieStore};
use rustock_trie::{OverlayTrieStore, TrieNode, TrieStore};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

#[path = "support/baseline.rs"]
mod baseline;

/// Re-materialize the working root from the store periodically. The in-memory
/// tree a chained replay accumulates is otherwise unbounded; build_segments
/// does the same for the same reason.
const REROOT_EVERY: u64 = 2_000;

fn hexs(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn flag(name: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter().position(|x| x == name).and_then(|i| a.get(i + 1).cloned())
}

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let pos: Vec<&String> = a[1..].iter().filter(|x| !x.starts_with("--")).collect();
    if pos.len() < 5 {
        eprintln!(
            "usage: baseline_rebuild <block-dir> <trie-dir> <old-baseline> <from> <out-file> \
             [--scratch DIR] [--progress FILE]"
        );
        std::process::exit(64);
    }
    let (block_dir, trie_dir, old_path) = (pos[0].clone(), pos[1].clone(), pos[2].clone());
    let from: u64 = pos[3].parse().context("bad <from>")?;
    let out_path = pos[4].clone();
    // Relative by default; an absolute path from one machine is no use on
    // another. This wants a volume with room for the replay's new nodes.
    let scratch = flag("--scratch")
        .unwrap_or_else(|| "baseline-rebuild-scratch".to_string());
    let progress = flag("--progress");

    let old = baseline::Baseline::open(Path::new(&old_path))?;
    anyhow::ensure!(
        from > old.first() && from <= old.last(),
        "<from> #{from} must be inside #{}..#{} and have a recorded parent",
        old.first() + 1,
        old.last()
    );
    let last = old.last();
    println!("old baseline #{}..#{last}", old.first());
    println!("recomputing  #{from}..#{last}  ({} blocks)", last - from + 1);
    println!("seed         baseline[#{}] = 0x{}", from - 1, hexs(&old.root(from - 1).unwrap()));

    let store = Arc::new(BlockStore::open_read_only(&block_dir)?);
    let base: Arc<dyn TrieStore> = Arc::new(RocksDbTrieStore::open_read_only(&trie_dir)?);
    let spill: Arc<dyn TrieStore> = Arc::new(RocksDbTrieStore::open(&scratch)?);
    let trie: Arc<dyn TrieStore> = Arc::new(OverlayTrieStore::with_spill(base.clone(), spill));
    println!("scratch      {scratch}");

    let seed = old.root(from - 1).expect("checked");
    let seed_data = trie
        .get(&seed)
        .with_context(|| format!("seed root for #{} is not in {trie_dir}", from - 1))?;
    let mut root = TrieNode::from_message(&seed_data, trie.as_ref());

    let processor = BlockProcessor::new(RskHardforkConfig::mainnet(), store.clone());

    // Keep every recomputed root; the file is written once at the end, after
    // the anchor has been judged.
    let mut roots: Vec<[u8; 32]> = Vec::with_capacity((last - from + 1) as usize);
    let mut changed = 0u64;
    let mut first_change: Option<u64> = None;
    let mut rejoined: Option<u64> = None;

    let t0 = Instant::now();
    for n in from..=last {
        let hash = store.canonical_hash(n)?.with_context(|| format!("no hash for #{n}"))?;
        let header = store.header(hash)?.with_context(|| format!("no header for #{n}"))?;
        let (transactions, ommers) =
            store.body(hash)?.with_context(|| format!("no body for #{n}"))?;
        let block = Block { header, transactions, ommers };
        let p = processor.execute_block(&block, &root, trie.clone())?;

        let computed: [u8; 32] = p.state_root_hash.as_slice().try_into().expect("32 bytes");
        let was = old.root(n).expect("in range");
        if computed != was {
            if first_change.is_none() {
                first_change = Some(n);
            }
            changed += 1;
            rejoined = None;
        } else if changed > 0 && rejoined.is_none() {
            // Agreeing again after a divergence is worth noting: it means the
            // difference was overwritten rather than carried forward.
            rejoined = Some(n);
        }
        roots.push(computed);

        let p_root_hash = p.state_root_hash;
        root = p.new_state_root;

        if (n - from + 1).is_multiple_of(REROOT_EVERY) {
            if let Some(d) = trie.get(p_root_hash.as_slice()) {
                root = TrieNode::from_message(&d, trie.as_ref());
            }
            let done = n - from + 1;
            let rate = done as f64 / t0.elapsed().as_secs_f64();
            let line = format!(
                "#{n} | {done}/{} | {rate:.0} blk/s | {changed} roots changed | eta {:.1}h",
                last - from + 1,
                (last - n) as f64 / rate / 3600.0
            );
            println!("{line}");
            if let Some(f) = &progress {
                let _ = std::fs::write(f, format!("{line}\n"));
            }
        }
    }

    let secs = t0.elapsed().as_secs_f64();
    println!();
    println!("replayed {} blocks in {:.2} h ({:.0} blk/s)", last - from + 1, secs / 3600.0, (last - from + 1) as f64 / secs);
    println!("roots changed: {changed} of {}", last - from + 1);
    if let Some(f) = first_change {
        println!("first change:  #{f}");
    }
    if let Some(r) = rejoined {
        println!("last rejoin:   #{r} (a divergence was overwritten rather than carried)");
    }

    // --- the anchor decides whether any of this may be written -----------
    let a_hash = store.canonical_hash(last)?.context("no anchor hash")?;
    let a_hdr = store.header(a_hash)?.context("no anchor header")?;
    let header_root: [u8; 32] = a_hdr.state_root.as_slice().try_into().expect("32 bytes");
    let computed_anchor = roots[roots.len() - 1];

    println!();
    println!("anchor #{last}:");
    println!("  recomputed 0x{}", hexs(&computed_anchor));
    println!("  header     0x{}", hexs(&header_root));
    println!("  old        0x{}", hexs(&old.anchor()));

    if computed_anchor != header_root {
        println!();
        println!("ANCHOR FAILS. This build does not reproduce the chain's cumulative pre-fork");
        println!("state at the first header that declares a Unitrie root. The old baseline did.");
        println!("That is a consensus regression in the pre-fork era, not a stale file, so");
        println!("nothing has been written. Investigate before regenerating anything.");
        std::process::exit(1);
    }
    println!("  MATCH -- this build reproduces the chain at the fork");

    // Splice: roots below `from` are unaffected and copied as they were.
    let mut all: Vec<[u8; 32]> = Vec::with_capacity((last - old.first() + 1) as usize);
    for n in old.first()..from {
        all.push(old.root(n).expect("in range"));
    }
    all.extend_from_slice(&roots);

    baseline::write(Path::new(&out_path), old.first(), last, &all, header_root)?;
    println!();
    println!("wrote {out_path}: {} roots, anchor verified against the chain", all.len());
    println!("the old baseline is untouched at {old_path}");
    Ok(())
}
