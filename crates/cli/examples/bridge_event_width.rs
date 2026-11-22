//! Did the bridge ever log an **empty** single dynamic field?
//!
//! The bridge encodes an event whose data is one dynamic `bytes`/`string` by
//! sizing the content as `((len - 1) / 32 + 1) * 32` in signed truncating
//! division (rskj `SolidityType.BytesType.encode`). At `len = 0` that is
//! `(-1 / 32 + 1) * 32 = 32`, so an empty field still emits one zero word and
//! the data is **96 octets** where the canonical ABI gives 64.
//!
//! Whether any caller ever produced it is a question about the chain, and the
//! evidence is in the **receipts** rather than in the execution -- so this needs
//! no replay. It is the cheapest measurement of its kind: a sequential read of
//! stored receipts.
//!
//! # Why "data is 96 octets" is the wrong test on its own
//!
//! The encoding is `offset(32) ++ length(32) ++ content`, and the content is
//! padded to a whole number of words. So a field of **1 to 32** octets also
//! encodes to exactly 96 -- legitimately, and commonly, since a 32-octet hash
//! is the ordinary case. Filtering on the total alone would report every one of
//! those as a hit.
//!
//! What distinguishes the quirk is the **length word**: 96 octets *and* a
//! declared length of zero. This reports both numbers, because the gap between
//! them is what shows the sharper test was needed.
//!
//! # What it reads
//!
//! Every log of every canonical block in the range, filtered to the bridge
//! address. Logs are what the chain recorded, which is what binds: an argument
//! from today's callers being non-empty is a fact about code, and can be
//! falsified by a version of that code nobody is reading.
//!
//! Read-only, and safe to run against a live node.
//!
//! ```text
//! bridge_event_width <data-dir> [--from N] [--to N]
//! ```

use anyhow::{Context, Result};
use rustock_storage::BlockStore;
use std::time::Instant;

/// rskj `PrecompiledContracts.BRIDGE_ADDR`.
const BRIDGE: &str = "0000000000000000000000000000000001000006";

fn flag(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.windows(2).find(|w| w[0] == format!("--{name}")).map(|w| w[1].clone())
}

fn main() -> Result<()> {
    let data_dir = std::env::args()
        .nth(1)
        .filter(|a| !a.starts_with("--"))
        .context("usage: bridge_event_width <data-dir> [--from N] [--to N]")?;
    let store = BlockStore::open_read_only(&data_dir)
        .with_context(|| format!("opening {data_dir} read-only"))?;
    let head = store
        .head()?
        .and_then(|h| store.header(h).ok().flatten())
        .context("no chain head")?
        .number;

    let from: u64 = flag("from").and_then(|v| v.parse().ok()).unwrap_or(1);
    let to: u64 = flag("to").and_then(|v| v.parse().ok()).unwrap_or(head).min(head);
    let bridge: Vec<u8> = alloy_primitives::hex::decode(BRIDGE)?;

    println!("scanning receipts of #{from}..#{to} for bridge logs");

    let (mut blocks, mut bridge_logs, mut width_96, mut empty_96) = (0u64, 0u64, 0u64, 0u64);
    let mut hits: Vec<(u64, String)> = Vec::new();
    let started = Instant::now();
    let mut last_report = Instant::now();

    for number in from..=to {
        let Some(hash) = store.canonical_hash(number).ok().flatten() else { continue };
        let Some(receipts) = store.receipts(hash).ok().flatten() else { continue };
        blocks += 1;
        for receipt in &receipts {
            for log in &receipt.logs {
                if log.address.as_slice() != bridge.as_slice() {
                    continue;
                }
                bridge_logs += 1;
                if log.data.len() != 96 {
                    continue;
                }
                width_96 += 1;
                // offset(32) ++ length(32) ++ content. A declared length of
                // zero with a word of content present is the quirk; 1..=32 is
                // an ordinary short field that happens to be the same width.
                let declared = &log.data[32..64];
                if declared.iter().all(|b| *b == 0) {
                    empty_96 += 1;
                    let topic = log
                        .topics
                        .first()
                        .map(|t| format!("{t:?}"))
                        .unwrap_or_else(|| "(no topics)".into());
                    if hits.len() < 50 {
                        hits.push((number, topic));
                    }
                }
            }
        }
        if last_report.elapsed().as_secs() >= 15 {
            let done = number - from + 1;
            let rate = done as f64 / started.elapsed().as_secs_f64().max(0.001);
            eprintln!(
                "  #{number}: {done} blocks ({rate:.0}/s), {bridge_logs} bridge logs, \
                 {width_96} of 96 octets, {empty_96} of them empty"
            );
            last_report = Instant::now();
        }
    }

    println!();
    println!("blocks with receipts      {blocks}");
    println!("bridge logs               {bridge_logs}");
    println!("  of exactly 96 octets    {width_96}   <- what a width-only test would report");
    println!("  of those, length zero   {empty_96}   <- the 96-octet empty encoding");
    println!("elapsed                   {:.0}s", started.elapsed().as_secs_f64());

    if !hits.is_empty() {
        println!();
        println!("occurrences (block, topic0):");
        for (number, topic) in &hits {
            println!("  #{number}  {topic}");
        }
        if empty_96 as usize > hits.len() {
            println!("  ... and {} more", empty_96 as usize - hits.len());
        }
    }

    println!();
    if empty_96 == 0 {
        println!(
            "NONE FOUND in #{from}..#{to}. No bridge log in this range carries the 96-octet \
             empty encoding, so as far as this chain's receipts go, no caller ever produced \
             an empty single dynamic field."
        );
    } else {
        println!(
            "{empty_96} occurrence(s). The 96-octet encoding is in the chain's receipts, so \
             it is frozen into a receipts root and the behaviour is load-bearing."
        );
    }
    if blocks == 0 {
        println!("but no receipts were read at all -- check the data directory and the range.");
        std::process::exit(2);
    }
    Ok(())
}
