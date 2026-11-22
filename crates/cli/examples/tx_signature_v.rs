//! What values of the signature octet `v` does this chain actually carry?
//!
//! `v` is one octet, decomposed as `recoveryId = 1 if v is even else 0` and
//! `chainId = (v - 35) / 2` with the subtraction **floored at zero**. The
//! accepted set is `27, 28, 34 … 255`; `29` through `33` are refused and the
//! signature is never taken (`EVM-073`).
//!
//! Two values are worth finding, and neither is EIP-155 behaviour:
//!
//! - **`v = 34`** is accepted and means exactly what `28` means: the floored
//!   subtraction sends it to chain 0, and 34 being even gives recovery id 1 --
//!   the same pair. So two octets encode one signature meaning, and 34 is not a
//!   value EIP-155 can produce. Narrowing the accepted set to `27, 28, 35 …`
//!   would make it exactly EIP-155 plus the two legacy values, which is a
//!   de-divergence rather than a special case moved elsewhere.
//! - **`v = 35`** names chain **0**, not chain 1.
//!
//! **A single `v = 34` kills that proposal outright**, because such a
//! transaction sits in a valid block whose sender was recovered from it:
//! refusing the octet later would make that block invalid. So this is a scan
//! whose value is entirely in the possibility of one hit.
//!
//! It needs no replay -- `v` is a stored field -- and no recipient to match, so
//! it is cheaper than a selector scan: a walk of transaction bodies testing one
//! integer.
//!
//! The whole histogram is reported rather than just the two values, because the
//! distribution is the context that makes a zero readable: a run that found no
//! `34` among a hundred transactions has shown nothing, and one that found none
//! among sixty million has.
//!
//! Read-only, and safe to run against a live node.
//!
//! ```text
//! tx_signature_v <data-dir> [--from N] [--to N]   # canonical blocks, exact
//! tx_signature_v <data-dir> --screen              # every stored body, faster
//! ```
//!
//! `--screen` iterates the bodies column family sequentially, which skips the
//! canonical-hash lookup and the random read per block -- where the exact walk
//! spends its time. It covers non-canonical bodies too, so it is a superset:
//! nothing found settles the question, a hit needs the exact walk over the
//! range concerned.

use alloy_rlp::{Decodable, Header as RlpHeader};
use anyhow::{Context, Result};
use rustock_core::Transaction;
use rustock_storage::BlockStore;
use std::collections::BTreeMap;
use std::time::Instant;

fn flag(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.windows(2).find(|w| w[0] == format!("--{name}")).map(|w| w[1].clone())
}

/// What `v` means under `EVM-073`, for the values worth naming in the report.
fn note(v: u64) -> &'static str {
    match v {
        0 => "no signature",
        27 | 28 => "legacy, pre-EIP-155",
        29..=33 => "REFUSED by EVM-073 -- should be impossible in a block",
        34 => "ACCEPTED, means exactly what 28 means -- not an EIP-155 value",
        35 | 36 => "chain 0 (the floored subtraction)",
        95 | 96 => "chain 30, mainnet",
        97 | 98 => "chain 31, testnet",
        _ => "",
    }
}

/// The `v` octets of one stored body, decoding only the transaction list.
///
/// The ommers that follow need the store's own header reader, and nothing here
/// looks at them, so the decode stops once the transactions are out.
fn body_v_values(bytes: &[u8]) -> Result<Vec<u64>> {
    let mut buf = bytes;
    let outer = RlpHeader::decode(&mut buf)?;
    let mut body = &buf[..outer.payload_length];
    let txs = RlpHeader::decode(&mut body)?;
    let mut txs_buf = &body[..txs.payload_length];
    let mut out = Vec::new();
    while !txs_buf.is_empty() {
        out.push(Transaction::decode(&mut txs_buf)?.v);
    }
    Ok(out)
}

/// Sequential pass over the bodies column family: no canonical-hash lookup and
/// no random read per block, which is where the per-block walk spends its time.
///
/// It is a **screen, not an answer**, and in one direction only. The column
/// family holds every body the node ever stored, canonical or not, so this is a
/// superset: finding nothing here settles the canonical question outright,
/// while a hit has to be followed up with the per-block walk, because a
/// transaction in an orphaned block invalidates nothing.
fn screen(data_dir: &str) -> Result<()> {
    use rocksdb::{IteratorMode, Options, DB};

    let mut opts = Options::default();
    opts.create_if_missing(false);
    opts.set_max_open_files(256);
    let cfs = [
        "headers",
        "block_numbers",
        "total_difficulty",
        "block_bodies",
        "receipts",
        "tx_index",
        "trie_nodes",
    ];
    let db = DB::open_cf_for_read_only(&opts, data_dir, cfs, false)
        .with_context(|| format!("opening {data_dir} read-only"))?;
    let cf = db.cf_handle("block_bodies").context("no block_bodies column family")?;

    println!("screening every stored body sequentially (canonical and not)");
    let mut seen: BTreeMap<u64, u64> = BTreeMap::new();
    let (mut bodies, mut txs, mut undecodable) = (0u64, 0u64, 0u64);
    let started = Instant::now();
    let mut last_report = Instant::now();

    for item in db.iterator_cf(cf, IteratorMode::Start) {
        let (_, bytes) = item?;
        bodies += 1;
        match body_v_values(&bytes) {
            Ok(values) => {
                txs += values.len() as u64;
                for v in values {
                    *seen.entry(v).or_default() += 1;
                }
            }
            // Counted rather than ignored: a body this cannot read is a body
            // whose `v` values were not inspected, and a screen that quietly
            // skipped some would report a zero it has not earned.
            Err(_) => undecodable += 1,
        }
        if last_report.elapsed().as_secs() >= 15 {
            eprintln!(
                "  {bodies} bodies ({:.0}/s), {txs} transactions, {} distinct v, \
                 {undecadable} unreadable",
                bodies as f64 / started.elapsed().as_secs_f64().max(0.001),
                seen.len(),
                undecadable = undecodable,
            );
            last_report = Instant::now();
        }
    }

    println!();
    println!("bodies read        {bodies}");
    println!("transactions       {txs}");
    println!("unreadable bodies  {undecodable}");
    println!("elapsed            {:.0}s", started.elapsed().as_secs_f64());
    println!();
    println!("            v      count  meaning");
    for (v, count) in &seen {
        println!("{v:>13} {count:>10}  {}", note(*v));
    }
    println!();
    let v34 = seen.get(&34).copied().unwrap_or(0);
    if v34 == 0 && undecodable == 0 {
        println!(
            "SCREEN CLEAN: no v = 34 in any stored body, canonical or not, across {txs} \
             transactions. Since every canonical body is among these, the canonical answer \
             is zero and no per-block walk is needed."
        );
    } else if v34 == 0 {
        println!(
            "No v = 34 found, but {undecodable} body/bodies could not be decoded, so the \
             screen is incomplete. Resolve those before treating this as a zero."
        );
        std::process::exit(2);
    } else {
        println!(
            "{v34} transaction(s) with v = 34 among stored bodies. This is a superset: run \
             the per-block walk over the ranges concerned to find whether any is in a \
             canonical block, which is what decides the question."
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    if std::env::args().any(|a| a == "--screen") {
        let data_dir = std::env::args()
            .nth(1)
            .filter(|a| !a.starts_with("--"))
            .context("usage: tx_signature_v <data-dir> --screen")?;
        return screen(&data_dir);
    }
    let data_dir = std::env::args()
        .nth(1)
        .filter(|a| !a.starts_with("--"))
        .context("usage: tx_signature_v <data-dir> [--from N] [--to N]")?;
    let store = BlockStore::open_read_only(&data_dir)
        .with_context(|| format!("opening {data_dir} read-only"))?;
    let head = store
        .head()?
        .and_then(|h| store.header(h).ok().flatten())
        .context("no chain head")?
        .number;
    let from: u64 = flag("from").and_then(|v| v.parse().ok()).unwrap_or(1);
    let to: u64 = flag("to").and_then(|v| v.parse().ok()).unwrap_or(head).min(head);

    println!("walking transaction bodies of #{from}..#{to}");

    let mut seen: BTreeMap<u64, u64> = BTreeMap::new();
    let mut txs = 0u64;
    let mut blocks = 0u64;
    // Where the interesting values turn up, so a hit can be investigated
    // rather than merely counted.
    let mut flagged: Vec<(u64, u64)> = Vec::new();
    let started = Instant::now();
    let mut last_report = Instant::now();

    for number in from..=to {
        let Some(hash) = store.canonical_hash(number).ok().flatten() else { continue };
        let Some((transactions, _)) = store.body(hash).ok().flatten() else { continue };
        blocks += 1;
        for tx in &transactions {
            txs += 1;
            *seen.entry(tx.v).or_default() += 1;
            if matches!(tx.v, 29..=34) && flagged.len() < 100 {
                flagged.push((number, tx.v));
            }
        }
        if last_report.elapsed().as_secs() >= 15 {
            let done = number - from + 1;
            eprintln!(
                "  #{number}: {done} blocks ({:.0}/s), {txs} transactions, {} distinct v",
                done as f64 / started.elapsed().as_secs_f64().max(0.001),
                seen.len()
            );
            last_report = Instant::now();
        }
    }

    println!();
    println!("blocks read        {blocks}");
    println!("transactions       {txs}");
    println!("distinct v values  {}", seen.len());
    println!();
    println!("            v      count  meaning");
    for (v, count) in &seen {
        println!("{v:>13} {count:>10}  {}", note(*v));
    }

    if !flagged.is_empty() {
        println!();
        println!("occurrences of v in 29..=34 (block, v):");
        for (number, v) in &flagged {
            println!("  #{number}  v={v}");
        }
    }

    println!();
    let v34 = seen.get(&34).copied().unwrap_or(0);
    let refused: u64 = (29..=33).filter_map(|v| seen.get(&v)).sum();
    if refused > 0 {
        println!(
            "{refused} transaction(s) carry a v of 29..=33, which EVM-073 refuses. Such a \
             transaction should not be in a block at all, so this is a finding about the \
             store or the decoder before it is one about the chain."
        );
    }
    if v34 == 0 {
        println!(
            "NO v = 34 in #{from}..#{to}, across {txs} transactions. Narrowing the accepted \
             set to 27, 28, 35.. would change nothing for any transaction in this range."
        );
    } else {
        println!(
            "{v34} transaction(s) carry v = 34. Each sits in a valid block whose sender was \
             recovered from it, so refusing the octet would make those blocks invalid: the \
             narrowing proposal is dead, and this is the fact that kills it."
        );
    }
    if txs == 0 {
        println!("but no transactions were read at all -- check the data directory and range.");
        std::process::exit(2);
    }
    Ok(())
}
