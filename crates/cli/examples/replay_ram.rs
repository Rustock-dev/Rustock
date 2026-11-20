//! Replay a block range with the whole trie segment held in RAM.
//!
//! `docs/trie-segments-design.md` §6.9a/b establishes that replay is bound by
//! *per-read latency*, not bytes or CPU: a trie walk is a chain of dependent
//! reads, so none of them can be issued in parallel, and at the 0.78 ms random
//! read of §6.8 the ~268 block-cache misses per block come to ~209 ms of pure
//! waiting. Bloom filters do not help (measured, §6.9a). Staging the segment
//! into `tmpfs` gives 9--13x (§6.9b/c).
//!
//! `tmpfs` is a lower bound, because it still pays RocksDB's per-`get` cost:
//! index probe, block decode, syscall. This loads the segment into a plain
//! in-memory map instead and skips all three.
//!
//! The load is the one part of this that *can* be parallelised -- it is a
//! sequential scan over disjoint key ranges, not a dependent walk -- so it is
//! sharded across threads. Each shard owns its own map and value arena, so
//! lookups afterwards take no lock at all.
//!
//! Usage: replay_ram <block-dir> <trie-dir> <from> <to> [--shards N]
use rocksdb::{Direction, IteratorMode, Options, DB};
use rustock_core::Block;
use rustock_execution::{BlockProcessor, RskHardforkConfig};
use rustock_storage::BlockStore;
use rustock_trie::{TrieNode, TrieStore};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const CF_TRIE: &str = "trie_nodes";

/// One shard of a *static* node map.
///
/// Nothing is inserted after load, which is what lets every per-entry pointer
/// go. Keys in this store are all exactly 32 bytes (verified at load), so they
/// live in one arena at a fixed stride with no length and no pointer; RocksDB
/// iterates them in order, so the arena is sorted for free and lookup is a
/// binary search. Values sit in a second arena in the same order, and a single
/// `u32` start offset per entry is enough -- the length is the next entry's
/// offset minus this one.
///
/// The first version of this used `HashMap<Box<[u8]>, (u32, u32)>` and a
/// `Vec<u8>` grown by `push`: 1,570 MB resident for a 684 MB segment. Most of
/// that was not the map but the arena doubling (up to 846 MB allocated to hold
/// 423 MB) plus one 32-byte heap allocation per key.
const KEY_LEN: usize = 32;

struct Shard {
    /// Sorted, `KEY_LEN` bytes per entry.
    keys: Vec<u8>,
    vals: Vec<u8>,
    /// `offs[i]` is where entry `i`'s value starts; `offs` has `n + 1` entries
    /// so the last length needs no special case.
    offs: Vec<u32>,
}

impl Shard {
    fn len(&self) -> usize {
        self.keys.len() / KEY_LEN
    }
    fn key(&self, i: usize) -> &[u8] {
        &self.keys[i * KEY_LEN..(i + 1) * KEY_LEN]
    }
    fn get(&self, key: &[u8]) -> Option<&[u8]> {
        let n = self.len();
        let (mut lo, mut hi) = (0usize, n);
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.key(mid).cmp(key) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => {
                    let (a, b) = (self.offs[mid] as usize, self.offs[mid + 1] as usize);
                    return Some(&self.vals[a..b]);
                }
            }
        }
        None
    }
    fn bytes(&self) -> usize {
        self.keys.capacity() + self.vals.capacity() + self.offs.capacity() * 4
    }
}

/// Every node of one segment, resident and immutable. `get` takes no lock.
struct RamTrie {
    shards: Vec<Shard>,
    /// Width of each shard's leading-byte range. MUST match the loader's
    /// partitioning: the loader walks `[i*span, (i+1)*span)`, so lookup has to
    /// divide, not mask. Masking the low bits instead sent every lookup to the
    /// wrong shard and surfaced as a MissingTrieNode mid-replay.
    span: usize,
}

impl RamTrie {
    fn shard_of(&self, key: &[u8]) -> usize {
        (*key.first().unwrap_or(&0) as usize) / self.span
    }
}

impl RamTrie {
    fn lookup(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.shards[self.shard_of(key)].get(key).map(|v| v.to_vec())
    }
}

impl TrieStore for RamTrie {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.lookup(key)
    }
    fn put(&self, _key: &[u8], _value: &[u8]) {}
    fn flush(&self) {}
}

/// A resident segment with a complete store behind it.
///
/// A segbuild segment holds only the nodes its build touched, so a miss means
/// one of two things `Option` cannot tell apart: the key is genuinely absent
/// from the state, or the segment never captured it. Reading the second as the
/// first is unsound and silently so -- it made #374 look like it broke mainnet
/// consensus at #8,922,011, when in truth the RSKIP379 pegout index was simply
/// not in segment 093 (docs/trie-segments-design.md 6.9d).
///
/// With a complete store behind the segment the ambiguity disappears: a miss in
/// RAM is served from the full trie and counted, and a miss in *both* is a real
/// absence. Every fallback hit is a node the segment should have held, so the
/// count doubles as the measurement that says whether segbuild needs rebuilding
/// -- and the recorded keys are the repair list.
struct RamWithFallback {
    ram: RamTrie,
    full: Box<dyn TrieStore>,
    from_ram: AtomicU64,
    from_full: AtomicU64,
    absent: AtomicU64,
    missing: Mutex<Vec<Vec<u8>>>,
}

impl TrieStore for RamWithFallback {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        if let Some(v) = self.ram.lookup(key) {
            self.from_ram.fetch_add(1, Ordering::Relaxed);
            return Some(v);
        }
        if let Some(v) = self.full.get(key) {
            self.from_full.fetch_add(1, Ordering::Relaxed);
            let mut m = self.missing.lock().unwrap();
            if m.len() < 10_000 {
                m.push(key.to_vec());
            }
            return Some(v);
        }
        self.absent.fetch_add(1, Ordering::Relaxed);
        None
    }
    fn put(&self, _key: &[u8], _value: &[u8]) {}
    fn flush(&self) {}
}

/// Samples this process's own RSS. External polling from a shell loop was
/// tried first and is a bad idea: it dies with the shell, and the peak that
/// matters (mid-load) is easy to miss between samples.
fn watch_peak(peak: Arc<std::sync::atomic::AtomicU64>, stop: Arc<std::sync::atomic::AtomicBool>) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            let r = rss_mb();
            peak.fetch_max(r, Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
}

fn rss_mb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
        })
        .map(|kb| kb / 1024)
        .unwrap_or(0)
}

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 {
        eprintln!("usage: replay_ram <block-dir> <trie-dir> <from> <to> [--shards N]");
        std::process::exit(2);
    }
    let (block_dir, trie_dir) = (a[1].clone(), a[2].clone());
    let from: u64 = a[3].parse()?;
    let to: u64 = a[4].parse()?;
    // Power of two, so the shard index is a mask of the leading key byte.
    let fallback_dir: Option<String> = a
        .iter()
        .position(|x| x == "--fallback")
        .and_then(|i| a.get(i + 1))
        .cloned();
    let shards: usize = a
        .iter()
        .position(|x| x == "--shards")
        .and_then(|i| a.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);
    anyhow::ensure!(shards.is_power_of_two() && shards <= 256, "--shards must be a power of two, <= 256");

    rustock_execution::supply::set_per_transaction(false);
    rustock_execution::supply::set_per_block(false);

    let peak = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    watch_peak(peak.clone(), stop.clone());

    let store = Arc::new(BlockStore::open_read_only(&block_dir)?);

    // ---- load, sharded ----
    let t_load = Instant::now();
    let span = 256 / shards;
    let mut built: Vec<Shard> = std::thread::scope(|scope| -> anyhow::Result<Vec<Shard>> {
        let mut hs = Vec::new();
        for i in 0..shards {
            let dir = trie_dir.clone();
            hs.push(scope.spawn(move || -> anyhow::Result<Shard> {
                // One read-only handle per shard. They are cheap and this way
                // no thread waits on another's iterator.
                let mut opts = Options::default();
                opts.create_if_missing(false);
                let db = DB::open_cf_for_read_only(&opts, &dir, vec![CF_TRIE], false)?;
                let cf = db.cf_handle(CF_TRIE).ok_or_else(|| anyhow::anyhow!("no {CF_TRIE} CF"))?;
                let lo = (i * span) as u8;
                let hi_excl = i * span + span; // 256 for the last shard
                let start = [lo];
                // Reserve from the store's own estimate so neither arena
                // doubles. A wrong estimate only costs one shrink at the end.
                // Reserve from the store's own estimates, with a small margin,
                // and then do NOT shrink. `shrink_to_fit` reallocates: it
                // allocates the smaller block, copies, frees the old, so peak
                // RSS carries both and glibc keeps the freed pages. Measured,
                // that cost ~400 MB of slack on a 639 MB map. A 5% overshoot
                // held for the run is far cheaper than a realloc of the whole
                // arena.
                let prop = |n: &str| {
                    db.property_int_value_cf(cf, n).ok().flatten().unwrap_or(0) as usize
                };
                let n_est = prop("rocksdb.estimate-num-keys") / shards.max(1);
                let live = prop("rocksdb.estimate-live-data-size") / shards.max(1);
                // live data covers keys + values; take the value share off it,
                // falling back to a per-entry guess if the property is absent.
                let v_est = live.saturating_sub(n_est * KEY_LEN).max(n_est * 72);
                let mut keys: Vec<u8> = Vec::with_capacity(n_est * KEY_LEN * 105 / 100);
                let mut vals: Vec<u8> = Vec::with_capacity(v_est * 105 / 100);
                let mut offs: Vec<u32> = Vec::with_capacity(n_est * 105 / 100 + 1);
                for item in db.iterator_cf(cf, IteratorMode::From(&start, Direction::Forward)) {
                    let (k, v) = item?;
                    if (k.first().copied().unwrap_or(0) as usize) >= hi_excl {
                        break;
                    }
                    anyhow::ensure!(
                        k.len() == KEY_LEN,
                        "key of {} bytes, expected {KEY_LEN}: the fixed-stride arena \
                         assumes every key is a node hash",
                        k.len()
                    );
                    offs.push(vals.len() as u32);
                    keys.extend_from_slice(&k);
                    vals.extend_from_slice(&v);
                }
                offs.push(vals.len() as u32);
                Ok(Shard { keys, vals, offs })
            }));
        }
        let mut out = Vec::new();
        for h in hs {
            out.push(h.join().expect("shard loader panicked")?);
        }
        Ok(out)
    })?;
    let per_shard: Vec<usize> = built.iter().map(|s| s.len()).collect();
    let nodes: usize = per_shard.iter().sum();
    let vbytes: usize = built.iter().map(|s| s.bytes()).sum();
    let load_secs = t_load.elapsed().as_secs_f64();
    let rss = rss_mb();

    // ---- replay ----
    // One owner for the shards: either the fallback wrapper takes them by
    // value, or the plain RAM store does.
    let mut built_for_plain: Vec<Shard> = Vec::new();
    if fallback_dir.is_none() {
        std::mem::swap(&mut built_for_plain, &mut built);
    }
    let wrapped: Option<Arc<RamWithFallback>> = match &fallback_dir {
        Some(dir) => {
            let full = rustock_storage::RocksDbTrieStore::open_read_only(dir)?;
            println!("fallback   {dir}");
            Some(Arc::new(RamWithFallback {
                ram: RamTrie { shards: built, span },
                full: Box::new(full),
                from_ram: AtomicU64::new(0),
                from_full: AtomicU64::new(0),
                absent: AtomicU64::new(0),
                missing: Mutex::new(Vec::new()),
            }))
        }
        None => None,
    };
    let plain: Option<Arc<RamTrie>> = if wrapped.is_none() {
        Some(Arc::new(RamTrie { shards: built_for_plain, span }))
    } else {
        None
    };
    let trie_store: Arc<dyn TrieStore> = match (&wrapped, &plain) {
        (Some(w), _) => w.clone(),
        (None, Some(r)) => r.clone(),
        _ => unreachable!(),
    };

    let seed_hash = store.canonical_hash(from - 1)?.expect("canonical hash");
    let seed = store.header(seed_hash)?.expect("header");
    let root_data = trie_store.get(seed.state_root.as_slice()).unwrap_or_else(|| {
        panic!("seed state root {} for #{} is not in {trie_dir}", seed.state_root, from - 1)
    });
    let mut root = TrieNode::from_message(&root_data, trie_store.as_ref());

    let processor = BlockProcessor::new(RskHardforkConfig::mainnet(), store.clone());
    let started = Instant::now();
    let mut done = 0u64;
    for n in from..=to {
        let hash = store.canonical_hash(n)?.expect("canonical hash");
        let header = store.header(hash)?.expect("header");
        let (transactions, ommers) = store.body(hash)?.expect("body");
        let expected = header.state_root;
        let block = Block { header, transactions, ommers };
        let p = processor.execute_block(&block, &root, trie_store.clone())?;
        if p.state_root_hash.as_slice() != expected.as_slice() {
            eprintln!("#{n}: STATE ROOT MISMATCH computed {} header {}", p.state_root_hash, expected);
            std::process::exit(1);
        }
        root = p.new_state_root;
        done += 1;
    }
    let secs = started.elapsed().as_secs_f64();

    println!("store      {trie_dir}");
    println!("range      #{from}..#{to}  ({done} blocks, all roots matched)");
    println!("shards     {shards}");
    println!("loaded     {nodes} nodes, {:.0} MB in arenas, RSS {rss} MB", vbytes as f64 / 1048576.0);
    // A skewed split means the parallel load degenerated to one thread.
    let (lo, hi) = (per_shard.iter().min().copied().unwrap_or(0), per_shard.iter().max().copied().unwrap_or(0));
    println!("shard fill min {lo}, max {hi}  ({:.2}x skew)", if lo > 0 { hi as f64 / lo as f64 } else { f64::INFINITY });
    println!("load       {load_secs:.1} s  ({:.0} MB/s)", (vbytes as f64 / 1048576.0) / load_secs);
    println!("replay     {secs:.1} s");
    println!("wall       {:.1} s", secs + load_secs);
    stop.store(true, Ordering::Relaxed);
    let used = vbytes;
    println!("blocks/s   {:.2}   (replay only: {:.2})", done as f64 / (secs + load_secs), done as f64 / secs);
    if let Some(w) = &wrapped {
        let (r, f, ab) = (
            w.from_ram.load(Ordering::Relaxed),
            w.from_full.load(Ordering::Relaxed),
            w.absent.load(Ordering::Relaxed),
        );
        let total = r + f + ab;
        println!(
            "reads      {r} from segment, {f} from fallback, {ab} absent in both \
             ({:.4}% of {total} needed the fallback)",
            if total > 0 { f as f64 / total as f64 * 100.0 } else { 0.0 }
        );
        let m = w.missing.lock().unwrap();
        println!("segment gaps recorded: {} distinct keys (capped at 10000)", m.len());
        if let Some(k) = m.first() {
            println!("  first gap: {}", alloy_primitives::hex::encode(k));
        }
    }
    println!(
        "arenas     {:.0} MB used of {:.0} MB reserved ({:.1}% slack), peak RSS {} MB",
        used as f64 / 1048576.0,
        vbytes as f64 / 1048576.0,
        (vbytes as f64 / used as f64 - 1.0) * 100.0,
        peak.load(Ordering::Relaxed)
    );
    Ok(())
}
