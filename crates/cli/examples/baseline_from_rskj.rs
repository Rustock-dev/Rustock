//! Build a pre-RSKIP126 root baseline from rskj's migration table, by lookup.
//!
//! `baseline_build` packs roots our own replay computed, and `baseline_rebuild`
//! recomputes a stale tail. Both derive the baseline from rustock, which means
//! the result can only ever be audited for correctness afterwards -- never
//! established by construction. This builds the same file the other way round.
//!
//! When rskj migrated at RSKIP126 it recorded, for each pre-fork Orchid state
//! root, the Unitrie root holding the same state. Every block below the fork
//! declares an Orchid root in its header, so
//!
//! ```text
//!   table[header(N).stateRoot]  ==  baseline[N]
//! ```
//!
//! for the whole era. That makes a baseline a 1.59M-entry lookup rather than a
//! genesis-to-fork replay: minutes instead of hours, and independent of
//! anything rustock computes rather than checked against it later
//! (`docs/trie-segments-design.md` §8.3).
//!
//! The one block the table cannot supply is **#1,591,000**, whose header
//! already carries a Unitrie root -- there is no Orchid root to translate. It
//! is taken from the header directly, which is also the era's anchor, so the
//! file's anchor is the chain's by construction.
//!
//! The table is frozen and is **not regenerable on this machine**; see
//! `/mnt/import/paseo/rskj-stateRoots-DO-NOT-DELETE.md`.
//!
//! Usage: baseline_from_rskj <block-dir> <state-roots-dir> <out-file> [last-block]

use anyhow::{bail, Context, Result};
use rocksdb::{Options, DB};
use rustock_storage::BlockStore;
use std::path::Path;

#[path = "support/baseline.rs"]
mod baseline;

use baseline::UNITRIE_FROM;

fn hexs(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: baseline_from_rskj <block-dir> <state-roots-dir> <out-file> [last-block]");
        std::process::exit(64);
    }
    let (block_dir, table_dir, out) = (a[1].clone(), a[2].clone(), a[3].clone());
    let last: u64 = a.get(4).map_or(UNITRIE_FROM, |s| s.parse().expect("bad last-block"));
    let first: u64 = 1;

    let blocks = BlockStore::open_read_only(&block_dir)?;
    let table = DB::open_for_read_only(&Options::default(), &table_dir, false)
        .with_context(|| format!("opening rskj stateRoots at {table_dir}"))?;

    println!("building #{first}..#{last} from {table_dir}");

    let mut roots: Vec<[u8; 32]> = Vec::with_capacity((last - first + 1) as usize);
    let mut missing: Vec<u64> = Vec::new();

    for n in first..=last {
        let hash = blocks
            .canonical_hash(n)?
            .with_context(|| format!("no canonical hash for #{n}"))?;
        let hdr = blocks.header(hash)?.with_context(|| format!("no header for #{n}"))?;

        if n >= UNITRIE_FROM {
            // At and above the fork the header's root already is the Unitrie
            // root; there is nothing to translate and nothing to trust.
            roots.push(hdr.state_root.as_slice().try_into().expect("32 bytes"));
            continue;
        }

        match table.get(hdr.state_root.as_slice())? {
            Some(v) if v.len() == 32 => roots.push(v[..].try_into().expect("32 bytes")),
            _ => {
                missing.push(n);
                // Push a zero so the indices stay aligned; the run fails below
                // rather than writing a file with a hole in it.
                roots.push([0u8; 32]);
            }
        }

        if n.is_multiple_of(200_000) {
            println!("  #{n} ...");
        }
    }

    if !missing.is_empty() {
        bail!(
            "the table has no entry for {} block(s) below #{UNITRIE_FROM} (first few: {:?}). \
             A baseline with holes cannot verify those blocks, and writing one that silently \
             carried zeros would be worse than failing. Is this table from a node that \
             executed the early chain?",
            missing.len(),
            &missing[..missing.len().min(8)]
        );
    }

    // The anchor is the header's own root, so it is the chain's by
    // construction rather than by a comparison that could be skipped.
    let anchor = roots[roots.len() - 1];
    println!("anchor #{last} 0x{}", hexs(&anchor));

    baseline::write(Path::new(&out), first, last, &roots, anchor)?;
    let sz = std::fs::metadata(&out)?.len();
    println!("wrote {out}: {} roots, {sz} bytes, schema {}", roots.len(), baseline::SCHEMA);

    let b = baseline::Baseline::open(Path::new(&out))?;
    anyhow::ensure!(b.first() == first && b.last() == last, "readback range differs");
    anyhow::ensure!(b.anchor() == anchor, "readback anchor differs");
    println!("readback ok");
    Ok(())
}
