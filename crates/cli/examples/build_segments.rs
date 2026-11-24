//! Replay a block range, verifying every state root, and build the range's
//! trie segments as a by-product.
//!
//! One process per range. Ranges are independent because the archival snapshot
//! holds every block's pre-state from #1,591,000 (docs/trie-segments-design.md
//! §2), so a worker seeds from the header's parent state root and goes forward;
//! the range starting at genesis builds its state from the chain config instead.
//!
//! Reads are served by a sliding window of in-memory tables, falling back to
//! this worker's own sealed segments and then to the archival trie. Everything
//! read or written lands in the newest table, so each sealed segment is
//! self-contained for its block range.
//!
//! Verification is not a separate pass: from #1,591,000 every header carries a
//! Unitrie root, and a mismatch stops this worker at that block.
//!
//! Work is claimed, not assigned. Predicting the cost of a block range failed
//! twice here -- balanced on trie-nodes-touched one worker drew a 142-hour
//! range against another's 8, and rebalanced on sampled gas it was still 44
//! against 24, because gas buys different amounts of real work in different
//! eras. Workers instead claim the next unclaimed chunk when they finish one,
//! so the split self-corrects and the pooled ETA becomes meaningful.
//!
//! Each chunk produces its own segment database, which is what a range-scoped
//! segment *is*: dense, self-contained, and routable by height.
//!
//! Usage: build_segments <worker-id> <block-dir> <archive-trie> <chunks-dir>
//!                       <out-root> <seal-mb> <tables> <progress-file>

use rustock_core::config::ChainConfig;
use rustock_core::Block;
use rustock_execution::{BlockProcessor, RskHardforkConfig};
use rustock_storage::window_store::{WindowConfig, WindowStore};
use rustock_storage::{BlockStore, RocksDbTrieStore};
#[path = "support/baseline.rs"]
mod baseline;

use rustock_trie::{account_key, AccountState, TrieKeySlice, TrieNode, TrieStore};
use std::io::Write;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

/// Mainnet's RSKIP-126 height, below which headers declare Orchid roots.
///
/// A default, not a constant of the protocol: on testnet `wasabi100` is at
/// height 0, so the Unitrie starts at genesis, there is no Orchid era and no
/// baseline is needed. `--network testnet` sets this to 0.
const UNITRIE_FROM_MAINNET: u64 = 1_591_000;
const PROGRESS_EVERY: u64 = 200;
/// Blocks between dropping the materialized trie tree and re-rooting from the
/// hash.
///
/// `execute_block` returns a `TrieNode` whose children are materialized as
/// execution touches them, and carrying it forward accumulates the whole
/// touched subtree in memory: measured at ~800 MB per worker against a 145 MB
/// window, i.e. the tree was five times the cache it was supposed to be
/// feeding, and four workers spent 3.2 GB of a 7.7 GB machine on it. Every one
/// of those nodes is already in the window store, so the tree is pure
/// duplication -- re-rooting from the hash makes it lazy again and the nodes
/// fault back in from memory.
const REROOT_EVERY: u64 = 500;
/// Blocks between checkpoints. Each one seals the resident window, so it costs
/// a refill afterwards; rare enough not to matter, frequent enough that a kill
/// costs minutes rather than hours.
const CHECKPOINT_EVERY: u64 = 50_000;

fn checkpoint_every() -> u64 {
    std::env::var("RUSTOCK_CHECKPOINT_EVERY")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v| *v > 0)
        .unwrap_or(CHECKPOINT_EVERY)
}


/// Open the archival trie, in whichever format it happens to be.
///
/// Rustock keeps trie nodes in a `trie_nodes` column family; rskj keeps the
/// same bytes in the default one. The import between them is a straight
/// key-to-value copy, so an rskj snapshot can serve as the archive directly --
/// which for testnet is the difference between parallel range builds and a
/// single sequential pass from genesis, since ranges are independent only when
/// each can seed from an archival pre-state.
///
/// Chosen by inspecting the column families rather than by a flag, because the
/// wrong flag here is silent: a default-family reader on a rustock store finds
/// nothing and reports every node absent, which is indistinguishable from a
/// pruned archive.
fn open_archive(path: &str) -> anyhow::Result<Arc<dyn TrieStore>> {
    let probe = rocksdb::Options::default();
    let dir = {
        let p = std::path::Path::new(path);
        if p.join("unitrie").join("CURRENT").exists() { p.join("unitrie") } else { p.to_path_buf() }
    };
    let rustock_format = rocksdb::DB::list_cf(&probe, &dir)
        .map(|cfs| cfs.iter().any(|c| c == "trie_nodes"))
        .unwrap_or(true);
    if rustock_format {
        println!("archive {}: rustock format", dir.display());
        Ok(Arc::new(RocksDbTrieStore::open_read_only(path)?))
    } else {
        println!("archive {}: rskj format, read directly (no import)", dir.display());
        Ok(Arc::new(rustock_storage::rskj_trie_store::RskjTrieStore::open_read_only(&dir)?))
    }
}

fn write_progress(path: &str, fields: &[(&str, String)]) {
    let body: String = fields.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
    let tmp = format!("{path}.tmp");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// Atomically take the next chunk. `rename` is the claim: exactly one worker
/// can win it, with no lock file and no coordinator.
/// `prefer_start`: the block after the chunk this worker just finished.
///
/// Taking the adjacent chunk keeps the sliding window warm across the seam.
/// Measured, a cold window costs about 40% for its first ~10,000 blocks -- one
/// worker went from 4.3 to 7.6 blocks/s as it warmed -- so a worker that hops
/// to an unrelated range pays that again every chunk. Adjacency avoids it in
/// the common case and costs nothing when no adjacent chunk is free.
fn claim_next(chunks: &std::path::Path, id: &str, prefer_start: Option<u64>) -> Option<(u64, u64, String)> {
    let mut todo: Vec<_> = std::fs::read_dir(chunks).ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".todo"))
        .collect();
    todo.sort();
    if let Some(want) = prefer_start {
        if let Some(pos) = todo.iter().position(|n| {
            n.split('-').nth(1).and_then(|v| v.parse::<u64>().ok()) == Some(want)
        }) {
            let adj = todo.remove(pos);
            todo.insert(0, adj);
        }
    }
    for name in todo {
        let from = chunks.join(&name);
        let stem = name.trim_end_matches(".todo").to_string();
        let to = chunks.join(format!("{stem}.run-{id}"));
        if std::fs::rename(&from, &to).is_ok() {
            let parts: Vec<&str> = stem.split('-').collect();
            if parts.len() >= 3 {
                if let (Ok(a), Ok(b)) = (parts[1].parse::<u64>(), parts[2].parse::<u64>()) {
                    return Some((a, b, stem));
                }
            }
            let _ = std::fs::rename(&to, &from);
        }
    }
    None
}

/// Re-take chunks this worker was running when it died, so a restart resumes
/// them (the chunk's own checkpoint says where it had reached).
fn reclaim_own(chunks: &std::path::Path, id: &str) {
    let suffix = format!(".run-{id}");
    if let Ok(rd) = std::fs::read_dir(chunks) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.ends_with(&suffix) {
                let stem = n.trim_end_matches(&suffix);
                let _ = std::fs::rename(chunks.join(&n), chunks.join(format!("{stem}.todo")));
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 8 {
        eprintln!(
            "usage: build_segments <worker-id> <block-dir> <archive-trie> <chunks-dir> \
             <out-root> <seal-mb> <tables> <progress-file>\n\
             \n\
             options:\n\
             \x20 --network mainnet|testnet   consensus rules AND where the Unitrie starts\n\
             \x20                             (testnet implies --unitrie-from 0, because\n\
             \x20                             wasabi100 is at height 0 there)\n\
             \x20 --unitrie-from N            override that height explicitly\n\
             \x20 --baseline FILE             pre-RSKIP126 roots, needed only below\n\
             \x20                             --unitrie-from\n\
             \n\
             The archive may be a rustock store or an rskj one; the format is detected\n\
             from its column families, not from a flag."
        );
        std::process::exit(2);
    }
    let (id, block_dir, archive, chunks_dir, out_root) = (&a[0], &a[1], &a[2], &a[3], &a[4]);
    let seal_mb: u64 = a[5].parse()?;
    let tables: usize = a[6].parse()?;
    let progress = a[7].clone();
    // Which chain's rules to execute under, and where its Unitrie starts.
    let testnet = a.iter().any(|x| x == "testnet")
        || a.windows(2).any(|w| w[0] == "--network" && w[1] == "testnet");
    let unitrie_from: u64 = a
        .windows(2)
        .find(|w| w[0] == "--unitrie-from")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(if testnet { 0 } else { UNITRIE_FROM_MAINNET });
    println!(
        "network: {}  unitrie from #{unitrie_from}",
        if testnet { "TESTNET" } else { "mainnet" }
    );

    let chunks = std::path::PathBuf::from(chunks_dir);
    reclaim_own(&chunks, id);

    // Pre-fork expected roots and seeds. Without it a range below #1,591,000
    // is replayed but not checked, which is how a segment gets built out of
    // state nobody ever verified (docs/trie-segments-design.md 8.2).
    let base: Option<std::sync::Arc<baseline::Baseline>> = match a.iter()
        .position(|x| x == "--baseline")
        .and_then(|i| a.get(i + 1))
    {
        Some(f) => {
            let b = baseline::Baseline::open(std::path::Path::new(f))?;
            eprintln!("[w{id}] baseline #{}..#{}", b.first(), b.last());
            Some(std::sync::Arc::new(b))
        }
        None => None,
    };

    let t_all = Instant::now();
    let mut total_done: u64 = 0;
    let mut prefer: Option<u64> = None;
    loop {
        let Some((start, end, stem)) = claim_next(&chunks, id, prefer) else {
            eprintln!("[w{id}] no chunks left");
            break;
        };
        let out_dir = format!("{out_root}/{stem}");
        eprintln!("[w{id}] claimed {stem} (#{start}..#{end})");
        match run_chunk(id, block_dir, archive, &out_dir, testnet, unitrie_from, start, end, seal_mb, tables,
                        &progress, &t_all, &mut total_done, base.as_ref()) {
            Ok(n) => {
                let _ = std::fs::rename(chunks.join(format!("{stem}.run-{id}")),
                                        chunks.join(format!("{stem}.done")));
                prefer = Some(end + 1);
                eprintln!("[w{id}] finished {stem}: {n} blocks");
            }
            Err(e) => {
                eprintln!("[w{id}] chunk {stem} FAILED: {e:?}");
                let _ = std::fs::rename(chunks.join(format!("{stem}.run-{id}")),
                                        chunks.join(format!("{stem}.failed")));
                return Err(e);
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_chunk(
    id: &str, block_dir: &str, archive: &str, out_dir: &str, testnet: bool, unitrie_from: u64,
    start: u64, end: u64, seal_mb: u64, tables: usize,
    progress: &str, t_all: &Instant, total_done: &mut u64,
    base: Option<&std::sync::Arc<baseline::Baseline>>,
) -> anyhow::Result<u64> {

    let store = Arc::new(BlockStore::open_read_only(block_dir)?);
    let fallback: Arc<dyn TrieStore> = open_archive(archive)?;
    let window = Arc::new(WindowStore::new(
        WindowConfig {
            seal_bytes: seal_mb * 1024 * 1024,
            tables,
            dir: std::path::PathBuf::from(out_dir),
        },
        Some(fallback.clone()),
    )?);
    let win: Arc<dyn TrieStore> = window.clone();

    // Resume from a checkpoint if this range was already partly built.
    let resume = WindowStore::read_checkpoint(std::path::Path::new(out_dir));
    let mut start = start;
    let mut seeded_root: Option<TrieNode> = None;
    if let Some((last, root_bytes)) = resume {
        if last >= start && last < end {
            let data = win.get(&root_bytes).or_else(|| fallback.get(&root_bytes));
            match data {
                Some(d) => {
                    seeded_root = Some(TrieNode::from_message(&d, win.as_ref()));
                    eprintln!("[w{id}] resuming at #{} from checkpoint", last + 1);
                    start = last + 1;
                }
                None => eprintln!("[w{id}] checkpoint at #{last} names a root that is not in the \
                                   sealed store; starting the range over"),
            }
        }
    }

    // Seed the starting state.
    let mut root = if let Some(r) = seeded_root { r } else if start <= 1 {
        let config = if testnet { ChainConfig::testnet() } else { ChainConfig::mainnet() };
        let mut r = TrieNode::empty();
        for e in &config.genesis_alloc() {
            let k = account_key(&e.address);
            r = r.put(&TrieKeySlice::from_key(&k), &AccountState::new(e.nonce, e.balance).encode(), win.as_ref());
        }
        r.save(win.as_ref(), true);
        eprintln!("[w{id}] genesis root {:?}", r.compute_hash(win.as_ref()));
        r
    } else if let Some(seed) = base.and_then(|b| {
        if start - 1 < unitrie_from { b.root(start - 1) } else { None }
    }) {
        // The parent header below the fork declares an Orchid root, which is
        // not a key in a Unitrie store, so the baseline is the only way to
        // start a range mid-era -- and the reason the era can be split at all.
        let data = fallback.get(&seed).ok_or_else(|| {
            anyhow::anyhow!(
                "[w{id}] baseline seed root for #{} is not in the fallback store",
                start - 1
            )
        })?;
        // Put the seed node into the window as well as reading it. This read
        // is the one in the whole build that bypasses copy-on-read, which is
        // why 96 of the original 97 chunks could not seed themselves (11.3a)
        // and why segment 097 needed repairing after the fact. Without this a
        // chunk cannot replay its own first block: that needs the state at
        // start-1, and the only node naming it would be missing.
        win.put(&seed, &data);
        eprintln!("[w{id}] seeded from baseline #{} root 0x{}", start - 1,
                  seed.iter().map(|x| format!("{x:02x}")).collect::<String>());
        TrieNode::from_message(&data, fallback.as_ref())
    } else {
        let h = store.header(store.canonical_hash(start)?.expect("hash"))?.expect("header");
        let p = store.header(h.parent_hash)?.expect("parent");
        let data = fallback.get(p.state_root.as_slice()).ok_or_else(|| {
            anyhow::anyhow!("[w{id}] parent state root {:?} for #{} not in the archive", p.state_root, p.number)
        })?;
        eprintln!("[w{id}] seeded from #{} root {:?}", p.number, p.state_root);
        TrieNode::from_message(&data, fallback.as_ref())
    };

    // The network decides the consensus rules. Executing testnet blocks under
    // mainnet rules would compute different state roots for every block that
    // straddles an activation the two ladders place differently -- which is
    // most of them, since testnet puts orchid and wasabi100 at height 0.
    let processor = BlockProcessor::new(
        if testnet { RskHardforkConfig::testnet() } else { RskHardforkConfig::mainnet() },
        store.clone(),
    );
    let ckpt_every = checkpoint_every();
    let t0 = Instant::now();
    let total = end - start + 1;
    let mut verified = 0u64;
    if start < unitrie_from && base.is_none() {
        eprintln!(
            "[w{id}] WARNING: #{start}..#{} lies below #{unitrie_from}, where headers carry \
             Orchid roots. Without --baseline these blocks are replayed but NOT verified, so \
             this segment will be built out of state nothing has checked.",
            end.min(unitrie_from.saturating_sub(1))
        );
    }

    let mut done = 0u64;
    let chunk_base = *total_done;

    for n in start..=end {
        window.set_block(n);
        let hash = match store.canonical_hash(n)? { Some(h) => h, None => { eprintln!("[w{id}] no canonical hash at #{n}"); break } };
        let header = match store.header(hash)? { Some(h) => h, None => break };
        let (txs, oms) = match store.body(hash)? { Some(b) => b, None => { eprintln!("[w{id}] no body at #{n}"); break } };
        let header_root = header.state_root;
        let p = processor.execute_block(&Block { header, transactions: txs, ommers: oms }, &root, win.clone())?;

        // At and above #1,591,000 the header is authoritative. Below it the
        // header is Orchid-format, so the baseline is the only thing that can
        // check the block -- and with no baseline the range is replayed
        // unchecked, which the warning above the loop calls out.
        let expected: Option<Vec<u8>> = if n >= unitrie_from {
            Some(header_root.as_slice().to_vec())
        } else {
            base.and_then(|b| b.root(n)).map(|r| r.to_vec())
        };

        if let Some(expected) = expected {
            if p.state_root_hash.as_slice() != expected.as_slice() {
                eprintln!("[w{id}] STATE ROOT MISMATCH at #{n}: computed {:?} expected 0x{}",
                          p.state_root_hash,
                          expected.iter().map(|x| format!("{x:02x}")).collect::<String>());
                write_progress(progress, &[
                    ("id", id.to_string()), ("state", "FAILED".into()),
                    ("block", n.to_string()), ("start", start.to_string()), ("end", end.to_string()),
                    ("done", done.to_string()), ("total", total.to_string()),
                    ("elapsed_s", format!("{:.0}", t0.elapsed().as_secs_f64())),
                ]);
                window.finish()?;
                anyhow::bail!("state root mismatch at #{n}");
            }
            verified += 1;
        }
        let p_root_hash = p.state_root_hash;
        root = p.new_state_root;
        done += 1;

        if done.is_multiple_of(REROOT_EVERY) {
            match win.get(p_root_hash.as_slice()) {
                Some(data) => root = TrieNode::from_message(&data, win.as_ref()),
                None => eprintln!("[w{id}] re-root at #{n}: root {p_root_hash:?} not readable; \
                                   keeping the materialized tree"),
            }
        }

        if done.is_multiple_of(ckpt_every) {
            window.checkpoint(n, p_root_hash.as_slice())?;
        }

        if done.is_multiple_of(PROGRESS_EVERY) || n == end {
            let el = t_all.elapsed().as_secs_f64();
            let rate = (chunk_base + done) as f64 / el.max(0.001);
            let reads = window.reads.load(Ordering::Relaxed).max(1);
            // Percent and ETA, so a reader does not have to divide two of the
            // other fields to learn whether this will finish tonight.
            let pct = (chunk_base + done) as f64 * 100.0 / (total.max(1)) as f64;
            let remaining = total.saturating_sub(chunk_base + done) as f64;
            let eta_s = if rate > 0.0 { remaining / rate } else { 0.0 };
            println!(
                "[w{id}] #{n}  {:.2}%  {rate:.1} blk/s  eta {:.1} h  ({}/{} blocks, {:.0}m elapsed)",
                pct, eta_s / 3600.0, chunk_base + done, total, el / 60.0
            );
            let _ = std::io::Write::flush(&mut std::io::stdout());
            write_progress(progress, &[
                ("id", id.to_string()), ("state", "running".into()),
                ("block", n.to_string()), ("start", start.to_string()), ("end", end.to_string()),
                ("done", (chunk_base + done).to_string()), ("total", total.to_string()),
                ("pct", format!("{pct:.2}")), ("eta_s", format!("{eta_s:.0}")),
                ("elapsed_s", format!("{el:.0}")), ("rate", format!("{rate:.2}")),
                ("verified", verified.to_string()),
                ("segments", window.segment_count().to_string()),
                ("sealed_bytes", window.sealed_bytes.load(Ordering::Relaxed).to_string()),
                ("resident_bytes", window.resident_bytes().to_string()),
                ("fallback_pct", format!("{:.3}", window.hits_fallback.load(Ordering::Relaxed) as f64 * 100.0 / reads as f64)),
                ("sealed_pct", format!("{:.3}", window.hits_sealed.load(Ordering::Relaxed) as f64 * 100.0 / reads as f64)),
            ]);
        }
    }

    window.finish()?;
    *total_done += done;
    let el = t0.elapsed().as_secs_f64();
    write_progress(progress, &[
        ("id", id.to_string()), ("state", "running".into()),
        ("block", end.to_string()), ("start", start.to_string()), ("end", end.to_string()),
        ("done", done.to_string()), ("total", total.to_string()),
        ("elapsed_s", format!("{el:.0}")), ("rate", format!("{:.2}", done as f64 / el.max(0.001))),
        ("verified", verified.to_string()),
        ("segments", window.segment_count().to_string()),
        ("sealed_bytes", window.sealed_bytes.load(Ordering::Relaxed).to_string()),
    ]);
    let _ = std::io::stderr().flush();
    eprintln!("[w{id}] chunk done: {done} blocks, {verified} state roots verified, {} segments, {:.0} blk/s",
              window.segment_count(), done as f64 / el.max(0.001));
    Ok(done)
}
