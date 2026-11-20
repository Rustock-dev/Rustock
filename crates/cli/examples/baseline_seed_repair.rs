//! Give each pre-fork segment the one node it needs to replay its own first
//! block.
//!
//! A chunk covering `#start..#end` is replayed by a verifier that seeds from
//! `baseline[start]` and starts at `start+1`, so block `start` itself is never
//! re-executed from the segment -- it is the seed. To replay it you need the
//! state at `start-1`, and the node naming that state is the one read in the
//! whole build that bypasses copy-on-read: `build_segments` fetches it
//! straight from the fallback store. So it is absent, and the chunk cannot
//! check its own first block.
//!
//! This is the same defect §11.3a records for 96 of the original 97 chunks and
//! the one segment 097 needed `repair_chunk --limit 1` for. `repair_chunk`
//! cannot serve the pre-fork era, because it takes the seed from the parent
//! *header*, which below #1,591,000 is an Orchid root naming no Unitrie node.
//! The baseline is what names it here.
//!
//! `build_segments` now puts that node into the window as it reads it, so
//! segments built after 2026-11-20 do not need this. It exists for the ones
//! built before.
//!
//! Purely additive and content-addressed: inserting a node a store already has
//! is a no-op, so running it twice is the same as running it once.
//!
//! Usage: baseline_seed_repair <segment-root> <source-trie> <baseline> [--apply]
//!
//! Without `--apply` it only reports which chunks are missing their node.

use anyhow::{Context, Result};
use rustock_storage::RocksDbTrieStore;
use rustock_trie::TrieStore;
use std::path::Path;

#[path = "support/baseline.rs"]
mod baseline;

fn hexs(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// `NNN-start-end`, and nothing else -- a suffixed name is a retired chunk.
fn parse(name: &str) -> Option<(u64, u64)> {
    let p: Vec<&str> = name.split('-').collect();
    if p.len() != 3 {
        return None;
    }
    let _idx: u64 = p[0].parse().ok()?;
    Some((p[1].parse().ok()?, p[2].parse().ok()?))
}

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: baseline_seed_repair <segment-root> <source-trie> <baseline> [--apply]");
        std::process::exit(64);
    }
    let root = a[1].clone();
    let source_dir = a[2].clone();
    let base = baseline::Baseline::open(Path::new(&a[3]))?;
    let apply = a.iter().any(|x| x == "--apply");

    let source = RocksDbTrieStore::open_read_only(&source_dir)
        .with_context(|| format!("opening source trie {source_dir}"))?;

    let mut names: Vec<(String, u64, u64)> = Vec::new();
    for e in std::fs::read_dir(&root)? {
        let e = e?;
        if !e.file_type()?.is_dir() {
            continue;
        }
        let n = e.file_name().to_string_lossy().to_string();
        if let Some((s, en)) = parse(&n) {
            names.push((n, s, en));
        }
    }
    names.sort_by_key(|(_, s, _)| *s);
    anyhow::ensure!(!names.is_empty(), "no NNN-start-end segments under {root}");

    let mut needed = 0u32;
    let mut inserted = 0u32;
    let mut unavailable = 0u32;

    for (name, start, end) in &names {
        if *start <= 1 {
            println!("{name}: starts at block 1, seeded from genesis, nothing to repair");
            continue;
        }
        let parent = start - 1;
        let Some(seed) = base.root(parent) else {
            println!("{name}: #{parent} is outside the baseline, skipped");
            continue;
        };
        let sealed = format!("{root}/{name}/sealed");

        // Read-only first: deciding whether to write by opening read-write
        // would take the lock on every chunk including the ones already fine.
        let have = {
            let s = RocksDbTrieStore::open_read_only(&sealed)
                .with_context(|| format!("opening {sealed}"))?;
            s.get(&seed).is_some()
        };
        if have {
            println!("{name}: #{parent} seed already present");
            continue;
        }
        needed += 1;

        let Some(data) = source.get(&seed) else {
            println!(
                "{name}: #{parent} seed 0x{} MISSING and the source store does not have it either",
                hexs(&seed)
            );
            unavailable += 1;
            continue;
        };

        if !apply {
            println!(
                "{name}: #{parent} seed 0x{} missing, {} bytes available to insert",
                hexs(&seed),
                data.len()
            );
            continue;
        }

        let w = RocksDbTrieStore::open(&sealed)
            .with_context(|| format!("opening {sealed} read-write"))?;
        w.put(&seed, &data);
        drop(w);

        // Confirm through a fresh read-only handle rather than trusting the
        // write: the point of the exercise is that the node is readable.
        let check = RocksDbTrieStore::open_read_only(&sealed)?;
        anyhow::ensure!(
            check.get(&seed).is_some(),
            "{name}: inserted the #{parent} seed but a fresh read cannot find it"
        );
        println!("{name}: inserted #{parent} seed 0x{} ({} bytes)", hexs(&seed), data.len());
        inserted += 1;
        let _ = end;
    }

    println!();
    println!("segments {}  needing repair {needed}  inserted {inserted}", names.len());
    if unavailable > 0 {
        println!(
            "{unavailable} could not be repaired: the source store lacks the node. Point this \
             at a store that covers the era."
        );
        std::process::exit(1);
    }
    if !apply && needed > 0 {
        println!("dry run -- re-run with --apply to insert");
    }
    Ok(())
}
