//! Build the pre-RSKIP126 root baseline from a recorded-roots CSV.
//!
//! `pre_unitrie_roots` replays from genesis and writes a CSV row per block.
//! That CSV is a working artifact: 135 MB of text, on whatever volume the run
//! happened to use, in a format nothing else reads. This turns it into the
//! packed, version-tagged file that segments carry and that the replayers read
//! (`support/baseline.rs`, `docs/trie-segments-design.md` §8.2).
//!
//! The one check that matters happens here. #1,591,000 is the first block whose
//! header carries a Unitrie root, so the recorded root for it must equal that
//! header -- and if it does, the entire cumulative pre-fork state is validated,
//! every balance, nonce, code blob and storage cell that survived to the fork.
//! If it does not, the CSV is wrong and writing it into a segment would bless
//! 1.59M roots on the strength of nothing. This tool refuses in that case, so
//! no baseline can exist without having passed its anchor.
//!
//! Usage: baseline_build <roots-csv> <block-dir> <out-file>
//!
//! The CSV needs a header line and at least `block` and `unitrie_root`
//! columns, located by name rather than position.

use anyhow::{bail, ensure, Context, Result};
use rustock_storage::BlockStore;
use std::io::{BufRead, BufWriter, Write};
use std::path::Path;

#[path = "support/baseline.rs"]
mod baseline;

use baseline::UNITRIE_FROM;

fn parse_root(s: &str) -> Result<[u8; 32]> {
    let t = s.trim().trim_start_matches("0x");
    ensure!(t.len() == 64, "root {s:?} is not 32 bytes of hex");
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&t[i * 2..i * 2 + 2], 16)
            .with_context(|| format!("root {s:?} is not hex"))?;
    }
    Ok(out)
}

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 4 {
        eprintln!("usage: baseline_build <roots-csv> <block-dir> <out-file>");
        std::process::exit(64);
    }
    let (csv, block_dir, out) = (a[1].clone(), a[2].clone(), a[3].clone());

    let f = std::fs::File::open(&csv).with_context(|| format!("opening {csv}"))?;
    let mut lines = std::io::BufReader::new(f).lines();

    let header = lines.next().context("csv is empty")??;
    let cols: Vec<&str> = header.split(',').map(|c| c.trim()).collect();
    let i_block = cols
        .iter()
        .position(|c| *c == "block")
        .context("csv has no 'block' column")?;
    let i_root = cols
        .iter()
        .position(|c| *c == "unitrie_root")
        .context("csv has no 'unitrie_root' column")?;
    println!("csv columns: block at {i_block}, unitrie_root at {i_root}");

    // Dense by construction: index k holds the root for block first+k. Building
    // it as a dense vector rather than a map is what catches a gap, since a
    // missing block would otherwise just be a key nobody looked up.
    let first: u64 = 1;
    let last: u64 = UNITRIE_FROM;
    let count = (last - first + 1) as usize;
    let mut roots: Vec<Option<[u8; 32]>> = vec![None; count];

    let mut seen = 0u64;
    let mut outside = 0u64;
    for line in lines {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(',').collect();
        if f.len() <= i_root.max(i_block) {
            bail!("short csv row: {line:?}");
        }
        let b: u64 = f[i_block]
            .trim()
            .parse()
            .with_context(|| format!("bad block number in {line:?}"))?;
        if b < first || b > last {
            outside += 1;
            continue;
        }
        let r = parse_root(f[i_root])?;
        let slot = &mut roots[(b - first) as usize];
        if let Some(prev) = slot {
            ensure!(*prev == r, "csv gives two different roots for #{b}");
        }
        *slot = Some(r);
        seen += 1;
    }
    println!("read {seen} rows in range, {outside} outside #{first}..#{last}");

    // A gap is fatal: a hole in the baseline is a block nothing can verify,
    // and silently skipping it is how a range reports success for work it did
    // not do.
    let missing: Vec<u64> = roots
        .iter()
        .enumerate()
        .filter(|(_, r)| r.is_none())
        .map(|(i, _)| first + i as u64)
        .take(10)
        .collect();
    if !missing.is_empty() {
        let total = roots.iter().filter(|r| r.is_none()).count();
        bail!(
            "baseline would have {total} holes; first few: {missing:?}. \
             The CSV does not cover #{first}..#{last} densely."
        );
    }
    let roots: Vec<[u8; 32]> = roots.into_iter().map(|r| r.unwrap()).collect();

    // --- the anchor, which is the whole basis for trusting any of this ---
    let blocks = BlockStore::open_read_only(&block_dir)
        .with_context(|| format!("opening block store {block_dir}"))?;
    let hash = blocks
        .canonical_hash(last)?
        .with_context(|| format!("no canonical hash for #{last}"))?;
    let hdr = blocks
        .header(hash)?
        .with_context(|| format!("no header for #{last}"))?;
    let header_root: [u8; 32] = hdr.state_root.as_slice().try_into().expect("32 bytes");
    let recorded = roots[count - 1];

    println!("anchor #{last}:");
    println!("  recorded 0x{}", hexs(&recorded));
    println!("  header   0x{}", hexs(&header_root));
    if recorded != header_root {
        bail!(
            "ANCHOR MISMATCH at #{last}. The recorded roots disagree with the first header that \
             declares a Unitrie root, so the cumulative pre-fork state they describe is not the \
             chain's. Refusing to write a baseline: it would bless {count} roots on no evidence."
        );
    }
    println!("  MATCH -- cumulative pre-fork state validated against the chain");

    baseline::write(Path::new(&out), first, last, &roots, header_root)?;
    let sz = std::fs::metadata(&out)?.len();
    println!("wrote {out}: {count} roots, {sz} bytes, schema {}", baseline::SCHEMA);

    // Read it back through the same reader the replayers use, so a format
    // mistake surfaces now and not in a six-hour verification run.
    let b = baseline::Baseline::open(Path::new(&out))?;
    ensure!(b.first() == first && b.last() == last, "readback range differs");
    ensure!(b.root(first) == Some(roots[0]), "readback of #{first} differs");
    ensure!(b.root(last) == Some(header_root), "readback of #{last} differs");
    let mid = first + (last - first) / 2;
    ensure!(
        b.root(mid) == Some(roots[(mid - first) as usize]),
        "readback of #{mid} differs"
    );
    ensure!(b.root(last + 1).is_none(), "readback past the end returned a root");
    println!("readback ok: #{first}, #{mid}, #{last}, and none past the end");

    let mut w = BufWriter::new(std::io::stdout());
    writeln!(w, "\nanchor 0x{}", hexs(&b.anchor()))?;
    Ok(())
}

fn hexs(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
