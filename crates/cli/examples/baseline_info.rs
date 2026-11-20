//! Open a pre-RSKIP126 root baseline, validate it, and report.
//!
//! Every check in `support/baseline.rs` runs on open, so this is also how a
//! baseline is verified: magic, schema version, dense length for its declared
//! range, and the header anchor agreeing with the body. With a block directory
//! it goes further and compares the anchor against the chain, which is the only
//! check that says the recorded roots are *right* rather than merely
//! self-consistent (`docs/trie-segments-design.md` §8).
//!
//! Usage: baseline_info <baseline-file> [block-dir] [block]...

use anyhow::{Context, Result};
use rustock_storage::BlockStore;
use std::path::Path;

#[path = "support/baseline.rs"]
mod baseline;

fn hexs(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 2 {
        eprintln!("usage: baseline_info <baseline-file> [block-dir] [block]...");
        std::process::exit(64);
    }
    let b = baseline::Baseline::open(Path::new(&a[1]))?;
    println!("file      {}", a[1]);
    println!("schema    {}", baseline::SCHEMA);
    println!("range     #{}..#{}  ({} roots)", b.first(), b.last(), b.len());
    println!("anchor    0x{}", hexs(&b.anchor()));
    println!("valid     magic, schema, dense length, and anchor-matches-body all pass");

    if a.len() < 3 {
        println!();
        println!("note: no block directory given, so the anchor was NOT compared against the");
        println!("      chain. Self-consistency is not correctness -- these roots were recorded");
        println!("      by rustock, so agreeing with them only detects regressions.");
        return Ok(());
    }

    let blocks = BlockStore::open_read_only(&a[2])
        .with_context(|| format!("opening block store {}", a[2]))?;
    let hash = blocks
        .canonical_hash(b.last())?
        .with_context(|| format!("no canonical hash for #{}", b.last()))?;
    let hdr = blocks
        .header(hash)?
        .with_context(|| format!("no header for #{}", b.last()))?;
    let header_root: [u8; 32] = hdr.state_root.as_slice().try_into().expect("32 bytes");

    println!();
    println!("anchor against the chain at #{}:", b.last());
    println!("  baseline 0x{}", hexs(&b.anchor()));
    println!("  header   0x{}", hexs(&header_root));
    if b.anchor() == header_root {
        println!("  MATCH -- the cumulative pre-fork state is the chain's");
    } else {
        println!("  MISMATCH -- this baseline does not describe the chain");
        std::process::exit(1);
    }

    for s in &a[3..] {
        let n: u64 = s.parse().with_context(|| format!("bad block {s}"))?;
        match b.root(n) {
            Some(r) => println!("#{n:<9} 0x{}", hexs(&r)),
            None => println!("#{n:<9} outside the covered range"),
        }
    }
    Ok(())
}
