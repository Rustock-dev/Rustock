//! Can this node extend the segbuild set, or is there a gap?
//!
//! Three numbers answer it, and all three are already on disk:
//!
//!   - where the **set** ends, from the `NNN-start-end` directory names;
//!   - where the node has **executed** to, from the block store's `exec_head`;
//!   - the collection **watermark**, from the epoch store's `collected-below`.
//!
//! This exists because answering that took eight commands otherwise: no tool
//! printed the executed head's *number* (`height_peek` prints its hash), so it
//! had to be bisected out of `block_info`. The question is two comparisons, and
//! it should cost one command.
//!
//! The distinction the output insists on: a gap between the set and the
//! executed head is a gap in **coverage**, which says nothing about whether the
//! blocks can still be built. Whether they can is what the watermark answers --
//! beneath it a missing node was taken by the collector and is not recoverable
//! here; at or above it, the nodes are still in an unswept epoch. A node that
//! was stopped for a week has a gap of thousands of blocks whose nodes are all
//! present, because nothing swept while it was down.
//!
//! Read-only, and safe to run against a live node.
//!
//! ```text
//! segbuild_status <data-dir> <segbuild-dir> [--trie-dir DIR] [--engine flatdb|rocksdb]
//! ```
//!
//! `--trie-dir` defaults to `<data-dir>/trie-epochs-flat` and `--engine` to
//! `flatdb`, which is what the mainnet node runs. The trie store is opened only
//! to read the watermark; pass an engine that does not match the store and it
//! says so rather than guessing.

use anyhow::{Context, Result};
use rustock_storage::epoch_store::{EpochConfig, EpochTrieStore};
use rustock_storage::segbuild_boundaries::SegmentBoundaries;
use rustock_storage::BlockStore;

fn flag(name: &str, default: &str) -> String {
    let args: Vec<String> = std::env::args().collect();
    args.windows(2)
        .find(|w| w[0] == format!("--{name}"))
        .map(|w| w[1].clone())
        .unwrap_or_else(|| default.to_string())
}

fn main() -> Result<()> {
    let positional: Vec<String> = std::env::args()
        .skip(1)
        .take_while(|a| !a.starts_with("--"))
        .collect();
    if positional.len() != 2 {
        eprintln!(
            "usage: segbuild_status <data-dir> <segbuild-dir> \
             [--trie-dir DIR] [--engine flatdb|rocksdb]"
        );
        std::process::exit(64);
    }
    let data_dir = positional[0].clone();
    let segbuild_dir = positional[1].clone();
    let trie_dir = flag("trie-dir", &format!("{data_dir}/trie-epochs-flat"));
    let engine = flag("engine", "flatdb");

    let store = BlockStore::open_read_only(&data_dir)
        .with_context(|| format!("opening {data_dir} read-only"))?;

    let exec_head = match store.exec_head()? {
        Some((hash, _)) => store.header(hash)?.map(|h| h.number),
        None => None,
    };
    let canonical_tip = store.head()?.and_then(|h| store.header(h).ok().flatten()).map(|h| h.number);
    let set_end = rustock_execution::segment_build::set_end(std::path::Path::new(&segbuild_dir))?;

    // The watermark. Opened read-only and only for this one value, so a store
    // that cannot be opened is reported rather than fatal: the two block
    // numbers above are still worth printing.
    let engine_parsed = rustock_storage::TrieEngine::parse(&engine)?;
    let config = EpochConfig::default();
    let expected = rustock_storage::epoch_backend::keys_for_rotation(config.rotate_bytes);
    let watermark = match EpochTrieStore::open_read_only_with(
        std::path::Path::new(&trie_dir),
        config,
        engine_parsed.factory(expected),
    ) {
        Ok(es) => Ok(es.collected_below()),
        Err(e) => Err(e),
    };

    println!("block store   {data_dir}");
    println!("segment set   {segbuild_dir}");
    println!("trie store    {trie_dir} ({engine})");
    println!();
    match set_end {
        Some(n) => println!("  set ends at         #{n}"),
        None => println!("  set ends at         (no NNN-start-end segment: nothing to extend)"),
    }
    match exec_head {
        Some(n) => println!("  executed head       #{n}"),
        None => println!("  executed head       (none: this node has executed nothing)"),
    }
    if let Some(n) = canonical_tip {
        println!("  canonical tip       #{n}");
    }
    match &watermark {
        Ok(Some(n)) => println!("  collected below     #{n}"),
        Ok(None) => println!("  collected below     (no sweep has completed)"),
        Err(e) => println!("  collected below     unreadable: {e:#}"),
    }

    // What is pending, if the logs exist yet.
    let logs = SegmentBoundaries::open(&segbuild_dir, u64::MAX);
    if let Ok(logs) = &logs {
        let boundaries = logs.boundaries();
        println!();
        println!("  boundaries recorded {}", boundaries.len());
        println!("  ranges built        {}", logs.built_count());
        if let Some(reason) = logs.halted() {
            println!("  HALTED              {reason}");
        }
        for u in logs.uncovered() {
            println!("  uncovered           #{}..#{}  {}", u.start, u.end, u.reason);
        }
        if let Some(first) = set_end {
            if let Some(bound) = logs.collection_bound(first + 1) {
                println!("  collection bound    #{bound}");
            }
        }
    }

    println!();
    let (Some(set_end), Some(exec_head)) = (set_end, exec_head) else {
        println!("Cannot extend: the set or the executed head is missing (see above).");
        std::process::exit(1);
    };

    let gap = exec_head.saturating_sub(set_end);
    if gap == 0 {
        println!("No gap: the set is current with the executed head. --segbuild-out will start.");
        return Ok(());
    }

    println!("Coverage gap: #{}..#{exec_head} is {gap} blocks.", set_end + 1);
    // The gap's blocks wrote at or after set_end + 1, so those writes are
    // intact unless the collector has been at or above there.
    match watermark {
        Ok(Some(w)) if w > set_end + 1 => {
            println!(
                "NOT buildable from this store: the collector swept beneath #{w}, which is \
                 above the gap's first block. Those nodes are gone. Fill the gap with the \
                 offline tool against an archive trie."
            );
            std::process::exit(2);
        }
        Ok(Some(w)) => println!(
            "Buildable: the watermark (#{w}) is {} blocks below the gap, so every node the \
             gap's blocks wrote is still in an unswept epoch. The seed state at #{set_end} \
             is the one part nothing can promise -- it was written below the watermark and \
             was not pinned -- so a build may still fail on a missing node, loudly.",
            (set_end + 1).saturating_sub(w)
        ),
        Ok(None) => println!("Buildable: no sweep has completed, so nothing has been collected."),
        Err(_) => println!(
            "Buildability unknown: the watermark could not be read (see above). Check \
             --trie-dir and --engine against what the node runs."
        ),
    }
    Ok(())
}
