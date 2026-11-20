//! Check individual pre-RSKIP126 blocks against the baseline, independently.
//!
//! A baseline records a root for *every* block of the era, which means it
//! records every block's pre-state as well as its post-state. So a block can
//! be checked on its own: seed from `baseline[N-1]`, execute block N, compare
//! against `baseline[N]`. No replay chain, no warm-up, no ordering -- each
//! probe is a few seconds and they are independent of each other.
//!
//! That is what makes this the cheap way to answer "which blocks does the
//! current build compute differently from the build that recorded this
//! baseline?". Regenerating the baseline costs a genesis-to-fork replay of
//! 1.59M blocks; bisecting with this costs a handful of single blocks, and
//! unlike a regeneration it says *which* blocks changed rather than only that
//! the file is stale.
//!
//! A mismatch here does not say which side is right. The era has one
//! independent check -- the header root at #1,591,000 (`baseline_info`) -- and
//! everything else is rustock against its own earlier self. What this tool
//! gives is the *shape* of a disagreement: whether it is one block or a
//! stretch, where it starts, and whether it heals.
//!
//! Usage:
//!   baseline_probe <block-dir> <trie-dir> <baseline> <from> [to] [--stride N]
//!                  [--stop-after N]
//!
//! `trie-dir` must hold the era's nodes (the pre-fork archive, or a segment's
//! sealed store). With no `to`, probes the single block `from`.

use anyhow::{Context, Result};
use rustock_core::Block;
use rustock_execution::{BlockProcessor, RskHardforkConfig};
use rustock_storage::{BlockStore, RocksDbTrieStore};
use rustock_trie::{TrieNode, TrieStore};
use std::path::Path;
use std::sync::Arc;

#[path = "support/baseline.rs"]
mod baseline;

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
    if pos.len() < 4 {
        eprintln!(
            "usage: baseline_probe <block-dir> <trie-dir> <baseline> <from> [to] \
             [--stride N] [--stop-after N]"
        );
        std::process::exit(64);
    }
    let block_dir = pos[0].clone();
    let trie_dir = pos[1].clone();
    let base_path = pos[2].clone();
    let from: u64 = pos[3].parse().context("bad <from>")?;
    let to: u64 = if pos.len() > 4 { pos[4].parse().context("bad <to>")? } else { from };
    let stride: u64 = flag("--stride").and_then(|s| s.parse().ok()).unwrap_or(1).max(1);
    let stop_after: usize =
        flag("--stop-after").and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);

    let base = baseline::Baseline::open(Path::new(&base_path))?;
    anyhow::ensure!(
        from >= base.first() + 1 && to <= base.last(),
        "range #{from}..#{to} needs seeds and roots inside the baseline's #{}..#{}; \
         block #{} has no recorded parent",
        base.first(),
        base.last(),
        base.first()
    );

    let store = Arc::new(BlockStore::open_read_only(&block_dir)?);
    let trie: Arc<dyn TrieStore> = Arc::new(RocksDbTrieStore::open_read_only(&trie_dir)?);
    let processor = BlockProcessor::new(RskHardforkConfig::mainnet(), store.clone());

    let mut checked = 0u64;
    let mut agreed = 0u64;
    let mut mismatched: Vec<u64> = Vec::new();
    let mut seed_absent: Vec<u64> = Vec::new();

    let mut n = from;
    while n <= to {
        let seed = base.root(n - 1).expect("checked in range");
        let expected = base.root(n).expect("checked in range");

        // A missing seed is not a disagreement and must not be counted as one:
        // it means this store cannot serve the probe, which is a different
        // problem from the block computing a different root.
        let Some(seed_data) = trie.get(&seed) else {
            seed_absent.push(n);
            n += stride;
            continue;
        };
        let root = TrieNode::from_message(&seed_data, trie.as_ref());

        let hash = store.canonical_hash(n)?.with_context(|| format!("no hash for #{n}"))?;
        let header = store.header(hash)?.with_context(|| format!("no header for #{n}"))?;
        let (transactions, ommers) =
            store.body(hash)?.with_context(|| format!("no body for #{n}"))?;
        let ntx = transactions.len();
        let block = Block { header, transactions, ommers };
        let p = processor.execute_block(&block, &root, trie.clone())?;

        checked += 1;
        if p.state_root_hash.as_slice() == expected {
            agreed += 1;
        } else {
            mismatched.push(n);
            println!(
                "#{n:<9} MISMATCH  txs={ntx}  computed 0x{}  baseline 0x{}",
                hexs(p.state_root_hash.as_slice()),
                hexs(&expected)
            );
            if mismatched.len() >= stop_after {
                println!("stopping after {} mismatches as asked", mismatched.len());
                break;
            }
        }
        n += stride;
    }

    println!();
    println!("probed  #{from}..#{to} stride {stride}");
    println!("checked {checked}");
    println!("agreed  {agreed}");
    println!("differ  {}", mismatched.len());
    if !seed_absent.is_empty() {
        println!(
            "seed absent for {} block(s) -- {} could not be probed from this store \
             (first few: {:?})",
            seed_absent.len(),
            if seed_absent.len() == 1 { "it" } else { "they" },
            &seed_absent[..seed_absent.len().min(8)]
        );
    }
    if !mismatched.is_empty() {
        let show = &mismatched[..mismatched.len().min(40)];
        println!("differing blocks: {show:?}");
        println!();
        println!(
            "This says the current build and the build that recorded the baseline disagree, \
             not which is right. The era's only check against the chain is the header root at \
             #{} -- see baseline_info.",
            base.last()
        );
        std::process::exit(1);
    }
    Ok(())
}
