//! Audit a pre-RSKIP126 baseline against rskj's own Orchid-to-Unitrie table.
//!
//! This is the check `docs/trie-segments-design.md` §8 says the era does not
//! have. §8 is right that the anchor at #1,591,000 is the only check available
//! from *rustock's* own data, and right that comparing against
//! `pre-roots.csv` is circular -- those roots are ours. But rskj's migration
//! left a table mapping each pre-fork Orchid state root to the Unitrie root
//! holding the same state, and that table is neither ours nor derived from
//! anything we computed. Every block below the fork declares an Orchid root in
//! its header, so for each one:
//!
//!   header(N).stateRoot  ->  rskj's table  ->  the Unitrie root rskj migrated
//!
//! compared against `baseline[N]`. That is a per-block independent oracle for
//! the whole era, by lookup rather than replay: minutes instead of the hours a
//! genesis-to-fork replay costs, and it localises a disagreement to the block
//! instead of only reporting that the cumulative state is wrong.
//!
//! It found the 37,840-block window where a stale baseline disagreed with the
//! current build, and said which side was right, in seconds.
//!
//! The table is frozen -- Orchid roots exist only below #1,591,000, so no
//! entry is ever added -- and is not regenerable on this machine. See
//! `/mnt/import/paseo/rskj-stateRoots-DO-NOT-DELETE.md`.
//!
//! Usage: baseline_audit <block-dir> <state-roots-dir> <baseline> [from] [to]
//!                       [--stride N] [--max-report N]

use anyhow::{Context, Result};
use rocksdb::{Options, DB};
use rustock_storage::BlockStore;
use std::path::Path;

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
    if pos.len() < 3 {
        eprintln!(
            "usage: baseline_audit <block-dir> <state-roots-dir> <baseline> [from] [to] \
             [--stride N] [--max-report N]"
        );
        std::process::exit(64);
    }
    let base = baseline::Baseline::open(Path::new(pos[2]))?;
    let from: u64 = pos.get(3).map_or(base.first(), |s| s.parse().expect("bad from"));
    let to: u64 = pos.get(4).map_or(base.last(), |s| s.parse().expect("bad to"));
    let stride: u64 = flag("--stride").and_then(|s| s.parse().ok()).unwrap_or(1).max(1);
    let max_report: usize = flag("--max-report").and_then(|s| s.parse().ok()).unwrap_or(20);

    anyhow::ensure!(
        from >= base.first() && to <= base.last(),
        "#{from}..#{to} is outside the baseline's #{}..#{}",
        base.first(),
        base.last()
    );

    let blocks = BlockStore::open_read_only(pos[0])?;
    let table = DB::open_for_read_only(&Options::default(), pos[1], false)
        .with_context(|| format!("opening rskj stateRoots at {}", pos[1]))?;

    println!("auditing #{from}..#{to} stride {stride} against {}", pos[1]);

    let mut checked = 0u64;
    let mut agreed = 0u64;
    let mut differ: Vec<u64> = Vec::new();
    let mut untranslated: Vec<u64> = Vec::new();
    let mut reported = 0usize;

    let mut n = from;
    while n <= to {
        let Some(hash) = blocks.canonical_hash(n)? else {
            n += stride;
            continue;
        };
        let Some(hdr) = blocks.header(hash)? else {
            n += stride;
            continue;
        };
        // Below the fork this is an Orchid root; it names no Unitrie node, and
        // the table is the only way to turn it into one.
        let orchid = hdr.state_root;

        match table.get(orchid.as_slice())? {
            Some(unitrie) if unitrie.len() == 32 => {
                let want = base.root(n).expect("in range");
                checked += 1;
                if unitrie[..] == want[..] {
                    agreed += 1;
                } else {
                    differ.push(n);
                    if reported < max_report {
                        println!(
                            "#{n:<9} DIFFER  rskj 0x{}  baseline 0x{}",
                            hexs(&unitrie),
                            hexs(&want)
                        );
                        reported += 1;
                    }
                }
            }
            // No entry is not a disagreement. It means this Orchid root was
            // never migrated -- an uncle, or a height the snapshot's node did
            // not execute -- and counting it as a mismatch would invent one.
            _ => untranslated.push(n),
        }
        n += stride;
    }

    println!();
    println!("checked      {checked}");
    println!("agreed       {agreed}");
    println!("differ       {}", differ.len());
    println!("untranslated {}", untranslated.len());

    if !differ.is_empty() {
        // Contiguous runs are the useful shape: a behaviour change shows up as
        // a window, a one-off shows up as a single block.
        let mut runs: Vec<(u64, u64)> = Vec::new();
        for &b in &differ {
            match runs.last_mut() {
                Some((_, end)) if b == *end + stride => *end = b,
                _ => runs.push((b, b)),
            }
        }
        println!();
        println!("differing ranges ({} run(s)):", runs.len());
        for (s, e) in runs.iter().take(20) {
            if s == e {
                println!("  #{s}");
            } else {
                println!("  #{s}..#{e}  ({} blocks)", (e - s) / stride + 1);
            }
        }
        println!();
        println!(
            "rskj's table is independent of anything rustock computed, so where it disagrees \
             with the baseline the baseline is wrong."
        );
        std::process::exit(1);
    }
    if !untranslated.is_empty() {
        println!();
        println!(
            "note: {} block(s) had no entry in the table (first few: {:?}). Those were not \
             checked either way.",
            untranslated.len(),
            &untranslated[..untranslated.len().min(8)]
        );
    }
    println!();
    println!("every checked root agrees with rskj's migration");
    Ok(())
}
