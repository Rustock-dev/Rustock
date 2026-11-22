//! Re-execute the Unitrie era against segbuild on N threads, verifying every
//! state root.
//!
//! Each segment is loaded wholly into RAM before its blocks are replayed: a
//! trie walk is a chain of *dependent* reads, so at the 0.78 ms random-read
//! latency of this device the misses dominate everything else, and residency
//! is worth ~10x (docs/trie-segments-design.md 6.9b/c). The load itself is the
//! one part that parallelises, being a scan over disjoint key ranges, so it is
//! sharded.
//!
//! Scheduling is the interesting part and the reason this is not a shell
//! script. Memory, not CPU, limits how many segments can be resident: a worker
//! must *reserve* its share before loading, and a reservation has to be
//! atomic with the decision to take the work. A first attempt in bash read
//! free memory and then claimed, so three workers each saw the same free
//! figure, each concluded its 2 GB segment fitted, and together they committed
//! 8.3 GB against a 5.1 GB budget. Here the queue and the budget live behind
//! one mutex, so that race cannot be expressed; a worker with nothing that
//! fits waits on a condvar and is woken when another releases.
//!
//! Largest segments go first: they are the long poles, and leaving them to the
//! end strands one thread on a 2 GB segment while the others idle.
//!
//! Usage:
//!   reexec_all <block-dir> <segbuild-dir> [--threads N] [--budget-mb N]
//!              [--shards N] [--state FILE] [--dry]
//!
//! Resumable: completed segments are appended to the state file and skipped.
//! A state-root mismatch is a consensus divergence -- it stops the run.
use anyhow::Context;
use rocksdb::{Direction, IteratorMode, Options, DB};
use rustock_core::Block;
use rustock_execution::{BlockProcessor, RskHardforkConfig};
use rustock_storage::BlockStore;
use rustock_core::config::ChainConfig;
use rustock_trie::{account_key, AccountState, TrieKeySlice, TrieNode, TrieStore};

#[path = "support/baseline.rs"]
mod baseline;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

const CF_TRIE: &str = "trie_nodes";
const KEY_LEN: usize = 32;
/// First block whose header carries a Unitrie root. Below it the headers are
/// Orchid-format, so neither a seed nor an expected root can come from them --
/// both come from `--baseline` instead, and without one a segment ending at or
/// below this height cannot be checked and is skipped
/// (docs/trie-segments-design.md 8, 8.2).
const UNITRIE_FROM: u64 = 1_591_000;
/// Replay's own heap on top of the resident segment: the materialized TrieNode
/// tree and friends, measured at ~630 MB against a 639 MB map (6.5a).
const FIXED_OVERHEAD_MB: u64 = 700;
/// With `--fallback` there is also a 131 GB RocksDB handle open (bounded block
/// cache plus index and filter blocks) and the arenas run a little over the
/// on-disk size. Measured on segment 093: 1,895 MB on disk, 1,917 MB of arenas,
/// 2,766 MB peak. So budget disk + 1,100, which is conservative against that.
const FIXED_OVERHEAD_FALLBACK_MB: u64 = 1_100;

// ---------------------------------------------------------------- RAM trie --

/// One shard of a static node map. Nothing is inserted after load, which is
/// what lets every per-entry pointer go: keys are all exactly 32 bytes, so
/// they sit in one arena at fixed stride, and RocksDB iterates them in order
/// so the arena is sorted and lookup is a binary search. Values follow in a
/// second arena; one `u32` start offset per entry is enough, the length being
/// the next offset minus this one.
struct Shard {
    keys: Vec<u8>,
    vals: Vec<u8>,
    offs: Vec<u32>,
}

impl Shard {
    fn len(&self) -> usize {
        self.keys.len() / KEY_LEN
    }
    fn get(&self, key: &[u8]) -> Option<&[u8]> {
        let (mut lo, mut hi) = (0usize, self.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.keys[mid * KEY_LEN..(mid + 1) * KEY_LEN].cmp(key) {
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
}

struct RamTrie {
    shards: Vec<Shard>,
    /// Width of each shard's leading-byte range. Must match the loader's
    /// partitioning, so lookup divides rather than masks -- masking the low
    /// bits instead sent every lookup to the wrong shard and surfaced as a
    /// MissingTrieNode mid-replay.
    span: usize,
}


impl RamTrie {
    fn lookup(&self, key: &[u8]) -> Option<Vec<u8>> {
        let i = (*key.first().unwrap_or(&0) as usize) / self.span;
        self.shards[i].get(key).map(|v| v.to_vec())
    }
}

impl TrieStore for RamTrie {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.lookup(key)
    }
    fn put(&self, _k: &[u8], _v: &[u8]) {}
    fn flush(&self) {}
}

/// A resident segment with a complete store behind it.
///
/// A segment holds only the nodes its build touched, so a miss cannot be told
/// from a genuine absence -- which made #374 look like it broke mainnet
/// consensus when the RSKIP379 pegout index was merely not in segment 093
/// (docs/trie-segments-design.md 6.9d). Measured on the diverging range: five
/// nodes of 334,798 reads came from the fallback, and those five decided the
/// block. So the gap is tiny and the consequence total.
struct RamWithFallback {
    ram: RamTrie,
    full: Box<dyn TrieStore>,
    from_full: Arc<AtomicU64>,
    absent: Arc<AtomicU64>,
    /// `(block, segment, key)` for every read the segment could not serve.
    /// The block matters more than the key: it names the blocks a segment-only
    /// replay silently got wrong, which is the set to re-check and the input to
    /// repairing the segment.
    gaps: Arc<Mutex<Vec<(u64, String, Vec<u8>)>>>,
    /// Set by the replay loop before each block. `TrieStore::get` has no block
    /// of its own, which is why the Bridge uses the same trick for its silent
    /// records (`crate::silent::set_position`). One per store, so two threads
    /// cannot attribute to each other.
    current_block: AtomicU64,
    segment: String,
}

impl TrieStore for RamWithFallback {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        if let Some(v) = self.ram.lookup(key) {
            return Some(v);
        }
        if let Some(v) = self.full.get(key) {
            let n = self.from_full.fetch_add(1, Ordering::Relaxed) + 1;
            let blk = self.current_block.load(Ordering::Relaxed);
            let mut g = self.gaps.lock().unwrap();
            if g.len() < 50_000 {
                g.push((blk, self.segment.clone(), key.to_vec()));
            }
            // Say the first few out loud: a segment-only run would have got
            // these blocks wrong without any sign of it.
            if n <= 50 {
                println!(
                    "[gap] block #{blk} segment {} needed the fallback for {}",
                    self.segment,
                    alloy_primitives::hex::encode(key)
                );
            }
            return Some(v);
        }
        self.absent.fetch_add(1, Ordering::Relaxed);
        None
    }
    fn put(&self, _k: &[u8], _v: &[u8]) {}
    fn flush(&self) {}
}

fn load_segment(dir: &Path, shards: usize) -> anyhow::Result<RamTrie> {
    let span = 256 / shards;
    let built = std::thread::scope(|scope| -> anyhow::Result<Vec<Shard>> {
        let mut hs = Vec::new();
        for i in 0..shards {
            hs.push(scope.spawn(move || -> anyhow::Result<Shard> {
                let mut o = Options::default();
                o.create_if_missing(false);
                let db = DB::open_cf_for_read_only(&o, dir, vec![CF_TRIE], false)?;
                let cf = db.cf_handle(CF_TRIE).context("no trie_nodes CF")?;
                let prop =
                    |n: &str| db.property_int_value_cf(cf, n).ok().flatten().unwrap_or(0) as usize;
                let n_est = prop("rocksdb.estimate-num-keys") / shards.max(1);
                let live = prop("rocksdb.estimate-live-data-size") / shards.max(1);
                let v_est = live.saturating_sub(n_est * KEY_LEN).max(n_est * 72);
                // Reserve with a small margin and never shrink: shrink_to_fit
                // reallocates, so peak carries both copies and glibc keeps the
                // freed pages -- measured at ~400 MB of slack on a 639 MB map.
                let mut keys = Vec::with_capacity(n_est * KEY_LEN * 105 / 100);
                let mut vals: Vec<u8> = Vec::with_capacity(v_est * 105 / 100);
                let mut offs: Vec<u32> = Vec::with_capacity(n_est * 105 / 100 + 1);
                let start = [(i * span) as u8];
                let hi_excl = i * span + span;
                for item in db.iterator_cf(cf, IteratorMode::From(&start, Direction::Forward)) {
                    let (k, v) = item?;
                    if (k.first().copied().unwrap_or(0) as usize) >= hi_excl {
                        break;
                    }
                    anyhow::ensure!(k.len() == KEY_LEN, "key of {} bytes, expected 32", k.len());
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
            out.push(h.join().map_err(|_| anyhow::anyhow!("shard loader panicked"))??);
        }
        Ok(out)
    })?;
    Ok(RamTrie { shards: built, span })
}

// ---------------------------------------------------------------- segments --

/// Set once from the CLI before any scheduling happens.
static mut FIXED_NOW: u64 = FIXED_OVERHEAD_MB;

#[derive(Clone, Debug)]
struct Segment {
    name: String,
    dir: PathBuf,
    start: u64,
    end: u64,
    disk_mb: u64,
}

impl Segment {
    /// What this segment will cost resident. The arenas come out near the
    /// on-disk size (639 MB of arenas for a 684 MB segment) and replay adds a
    /// roughly fixed heap on top.
    fn need_mb(&self) -> u64 {
        self.disk_mb + unsafe { FIXED_NOW }
    }
    /// Blocks this segment will actually replay.
    ///
    /// Normally the first block of the range only supplies the seed state and
    /// is not itself re-executed, so the count is `end - start`. A range
    /// starting at block 1 is the exception: there is no parent block to seed
    /// from, the seed is the genesis allocation, and block 1 is replayed like
    /// any other -- so it is one more. Getting this wrong makes the planned
    /// total disagree with the executed total, which the completeness check at
    /// the end of a run reports as INCOMPLETE.
    fn blocks(&self) -> u64 {
        if self.start <= 1 {
            self.end.saturating_sub(self.start) + 1
        } else {
            self.end.saturating_sub(self.start)
        }
    }
}

fn dir_mb(p: &Path) -> u64 {
    fn walk(p: &Path) -> u64 {
        let mut t = 0;
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                match e.metadata() {
                    Ok(m) if m.is_dir() => t += walk(&e.path()),
                    Ok(m) => t += m.len(),
                    Err(_) => {}
                }
            }
        }
        t
    }
    walk(p) / 1_048_576
}

fn discover(segroot: &Path, have_baseline: bool) -> anyhow::Result<Vec<Segment>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(segroot)?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !e.path().is_dir() || name == "sealed" || name.contains("truncated-superseded") {
            continue;
        }
        // NNN-start-end
        let parts: Vec<&str> = name.split('-').collect();
        if parts.len() < 3 {
            continue;
        }
        let (start, end) = match (parts[1].parse::<u64>(), parts[2].parse::<u64>()) {
            (Ok(a), Ok(b)) => (a, b),
            _ => continue,
        };
        let dir = e.path().join("sealed");
        if !dir.join("CURRENT").exists() {
            continue; // empty or stale directory
        }
        let disk_mb = dir_mb(&dir);
        if disk_mb <= 1 {
            continue;
        }
        if end <= UNITRIE_FROM && !have_baseline {
            println!(
                "skip {name}: ends at or below #{UNITRIE_FROM}, where headers carry Orchid \
                 roots -- pass --baseline to check it"
            );
            continue;
        }
        out.push(Segment { name, dir, start, end, disk_mb });
    }
    // Longest-processing-time first.
    out.sort_by(|a, b| b.disk_mb.cmp(&a.disk_mb));
    Ok(out)
}

// --------------------------------------------------------------- scheduler --

struct Sched {
    pending: Vec<Segment>,
    budget_mb: u64,
    budget_total_mb: u64,
    in_flight: usize,
    /// Names of the segments currently resident, for the status line.
    running: Vec<String>,
}

enum Take {
    Work(Segment),
    WaitForBudget,
    Drained,
}

impl Sched {
    /// Reserve and take in one step, under the caller's lock. Deciding then
    /// reserving is the race that made the shell version commit 8.3 GB against
    /// 5.1 GB; here there is no window between them.
    fn take(&mut self) -> Take {
        if self.pending.is_empty() {
            return Take::Drained;
        }
        if let Some(i) = self.pending.iter().position(|s| s.need_mb() <= self.budget_mb) {
            let s = self.pending.remove(i);
            self.budget_mb -= s.need_mb();
            self.in_flight += 1;
            self.running.push(s.name.clone());
            return Take::Work(s);
        }
        // Nothing fits. If no one is running, the smallest pending segment can
        // never fit and the run would hang -- say so instead.
        if self.in_flight == 0 {
            let smallest = self.pending.iter().map(|s| s.need_mb()).min().unwrap_or(0);
            eprintln!(
                "error: nothing running and the smallest remaining segment needs {smallest} MB, \
                 over the {} MB budget. Raise --budget-mb or free memory.",
                self.budget_mb
            );
            self.pending.clear();
            return Take::Drained;
        }
        Take::WaitForBudget
    }

    fn release(&mut self, s: &Segment) {
        self.budget_mb += s.need_mb();
        self.in_flight -= 1;
        if let Some(i) = self.running.iter().position(|n| n == &s.name) {
            self.running.remove(i);
        }
    }
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

// -------------------------------------------------------------------- main --

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let flag = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned();
    if a.len() < 3 {
        eprintln!(
            "usage: reexec_all <block-dir> <segbuild-dir> [--threads N] [--budget-mb N] \
             [--shards N] [--state FILE] [--dry]"
        );
        std::process::exit(2);
    }
    let block_dir = a[1].clone();
    let segroot = PathBuf::from(a[2].clone());
    let threads: usize = flag("--threads").and_then(|s| s.parse().ok()).unwrap_or(2);
    let shards: usize = flag("--shards").and_then(|s| s.parse().ok()).unwrap_or(4);
    anyhow::ensure!(shards.is_power_of_two() && shards <= 256, "--shards must be a power of two");
    let fallback_dir: Option<String> =
        flag("--fallback").inspect(|_| unsafe { FIXED_NOW = FIXED_OVERHEAD_FALLBACK_MB });
    // Relative by default: an absolute path from the machine this was written on
    // is a poor default anywhere else, and the gap list is written beside it.
    let state_path =
        flag("--state").unwrap_or_else(|| "reexec-done.txt".to_string());
    let dry = a.iter().any(|x| x == "--dry");

    // The pre-fork era's expected roots and seeds. Loaded once and shared: it
    // is ~49 MB and every worker reads it.
    let base: Option<Arc<baseline::Baseline>> = match flag("--baseline") {
        Some(f) => {
            let b = baseline::Baseline::open(Path::new(&f))?;
            println!(
                "baseline {f}: #{}..#{} ({} roots), anchor 0x{}",
                b.first(),
                b.last(),
                b.len(),
                b.anchor().iter().map(|x| format!("{x:02x}")).collect::<String>()
            );
            Some(Arc::new(b))
        }
        None => None,
    };

    // Check the anchor against the chain once, now, rather than discovering a
    // stale baseline partway through a multi-hour run. This is the era's only
    // comparison against something rustock did not produce: #1,591,000 is the
    // first header to carry a Unitrie root, so matching it validates the whole
    // cumulative pre-fork state. Matching the baseline elsewhere only shows
    // rustock still agrees with itself.
    if let Some(b) = &base {
        let probe = BlockStore::open_read_only(&block_dir)
            .with_context(|| format!("opening block store {block_dir} to check the anchor"))?;
        let hash = probe
            .canonical_hash(b.last())?
            .with_context(|| format!("no canonical hash for anchor block #{}", b.last()))?;
        let hdr = probe
            .header(hash)?
            .with_context(|| format!("no header for anchor block #{}", b.last()))?;
        anyhow::ensure!(
            hdr.state_root.as_slice() == b.anchor(),
            "BASELINE ANCHOR MISMATCH at #{}: baseline says 0x{} and the chain says {}. \
             Every pre-fork root in this baseline describes a state the chain does not have, \
             so verifying against it would prove nothing. Regenerate it.",
            b.last(),
            b.anchor().iter().map(|x| format!("{x:02x}")).collect::<String>(),
            hdr.state_root
        );
        println!("baseline anchor matches the chain at #{}", b.last());
    }

    let budget_mb: u64 = flag("--budget-mb").and_then(|s| s.parse().ok()).unwrap_or_else(|| {
        // MemAvailable less a reserve for the OS and anything else on the box.
        let avail = std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("MemAvailable:"))
                    .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
            })
            .unwrap_or(4_000_000)
            / 1024;
        avail.saturating_sub(1200)
    });

    rustock_execution::supply::set_per_transaction(false);
    rustock_execution::supply::set_per_block(false);

    let done: HashSet<String> = std::fs::read_to_string(&state_path)
        .map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default();

    let all = discover(&segroot, base.is_some())?;
    let pending: Vec<Segment> = all.iter().filter(|s| !done.contains(&s.name)).cloned().collect();
    let total_blocks: u64 = pending.iter().map(|s| s.blocks()).sum();
    println!(
        "segments {} ({} already done), {} blocks, {} threads, budget {} MB",
        pending.len(),
        done.len(),
        total_blocks,
        threads,
        budget_mb
    );
    for s in pending.iter().take(5) {
        // A range starting at block 1 replays block 1 too, having no parent to
        // seed from; every other range's first block is only the seed.
        let first_replayed = if s.start <= 1 { s.start } else { s.start + 1 };
        println!("  {:<30} #{}..#{}  {} MB  needs {} MB", s.name, first_replayed, s.end, s.disk_mb, s.need_mb());
    }
    if pending.len() > 5 {
        println!("  ... {} more", pending.len() - 5);
    }
    if let Some(s) = pending.iter().find(|s| s.need_mb() > budget_mb) {
        println!(
            "note: {} needs {} MB, over the budget -- it will be reported, not silently skipped",
            s.name,
            s.need_mb()
        );
    }
    if dry {
        println!("--dry: nothing executed");
        return Ok(());
    }

    if let Some(p) = Path::new(&state_path).parent() {
        std::fs::create_dir_all(p)?;
    }
    let store = Arc::new(BlockStore::open_read_only(&block_dir)?);
    let sched = Arc::new((
        Mutex::new(Sched {
            pending,
            budget_mb,
            budget_total_mb: budget_mb,
            in_flight: 0,
            running: Vec::new(),
        }),
        Condvar::new(),
    ));
    let state = Arc::new(Mutex::new(
        std::fs::OpenOptions::new().create(true).append(true).open(&state_path)?,
    ));
    let stop = Arc::new(AtomicBool::new(false));
    let blocks_done = Arc::new(AtomicU64::new(0));
    let peak_rss = Arc::new(AtomicU64::new(0));
    let from_full = Arc::new(AtomicU64::new(0));
    let absent = Arc::new(AtomicU64::new(0));
    let gaps: Arc<Mutex<Vec<(u64, String, Vec<u8>)>>> = Arc::new(Mutex::new(Vec::new()));
    let started = Instant::now();

    // A heartbeat, because the header alone says nothing once the run is
    // under way: no rate until the first segment lands, and no sign of how
    // close memory is to the budget. Prints every 60 s.
    {
        let (sched, stop, blocks_done, peak_rss) =
            (sched.clone(), stop.clone(), blocks_done.clone(), peak_rss.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let rss = rss_mb();
                peak_rss.fetch_max(rss, Ordering::Relaxed);
                let el = started.elapsed().as_secs_f64();
                let bd = blocks_done.load(Ordering::Relaxed);
                let (pending_n, in_flight, used, total, names) = {
                    let (m, _) = &*sched;
                    let g = m.lock().unwrap();
                    (
                        g.pending.len(),
                        g.in_flight,
                        g.budget_total_mb - g.budget_mb,
                        g.budget_total_mb,
                        g.running.join(", "),
                    )
                };
                let rate = if el > 0.0 { bd as f64 / el } else { 0.0 };
                let eta = if rate > 0.0 {
                    (total_blocks.saturating_sub(bd)) as f64 / rate / 3600.0
                } else {
                    f64::NAN
                };
                println!(
                    "[status] {:.0}m | {bd}/{total_blocks} blocks ({:.2}%) | {rate:.1} blk/s avg |                      eta {eta:.1}h | {in_flight} running, {pending_n} queued |                      mem {used}/{total} MB reserved, RSS {rss} MB (peak {}) | [{names}]",
                    el / 60.0,
                    bd as f64 / total_blocks as f64 * 100.0,
                    peak_rss.load(Ordering::Relaxed)
                );
                use std::io::Write as _;
                let _ = std::io::stdout().flush();
                std::thread::sleep(std::time::Duration::from_secs(60));
            }
        });
    }

    std::thread::scope(|scope| {
        for id in 0..threads {
            let (sched, store, state, stop, blocks_done) =
                (sched.clone(), store.clone(), state.clone(), stop.clone(), blocks_done.clone());
            let (fb, from_full, absent, gaps) =
                (fallback_dir.clone(), from_full.clone(), absent.clone(), gaps.clone());
            let bl = base.clone();
            scope.spawn(move || {
                loop {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let seg = {
                        let (m, cv) = &*sched;
                        let mut g = m.lock().unwrap();
                        loop {
                            match g.take() {
                                Take::Work(s) => break Some(s),
                                Take::Drained => break None,
                                Take::WaitForBudget => {
                                    if stop.load(Ordering::Relaxed) {
                                        break None;
                                    }
                                    g = cv.wait(g).unwrap();
                                }
                            }
                        }
                    };
                    let Some(seg) = seg else {
                        let (_, cv) = &*sched;
                        cv.notify_all();
                        return;
                    };

                    let r = run_segment(&seg, &store, shards, &blocks_done, fb.as_deref(), &from_full, &absent, &gaps, bl.as_ref());

                    {
                        let (m, cv) = &*sched;
                        m.lock().unwrap().release(&seg);
                        cv.notify_all();
                    }

                    match r {
                        Ok((n, rate)) => {
                            let bd = blocks_done.load(Ordering::Relaxed);
                            let el = started.elapsed().as_secs_f64();
                            let eta = if bd > 0 {
                                (total_blocks.saturating_sub(bd)) as f64 / (bd as f64 / el) / 3600.0
                            } else {
                                0.0
                            };
                            println!(
                                "[t{id}] OK   {:<30} {n} blocks {rate:.1} blk/s | {bd}/{total_blocks} \
                                 ({:.1}%) elapsed {:.0}m eta {eta:.1}h",
                                seg.name,
                                bd as f64 / total_blocks as f64 * 100.0,
                                el / 60.0
                            );
                            let mut f = state.lock().unwrap();
                            let _ = writeln!(f, "{}", seg.name);
                            let _ = f.flush();
                        }
                        Err(e) => {
                            let msg = format!("{e:#}");
                            if msg.contains("no pre-state") {
                                println!("[t{id}] SKIP {:<30} {msg}", seg.name);
                                let mut f = state.lock().unwrap();
                                let _ = writeln!(f, "{}", seg.name);
                            } else {
                                eprintln!("[t{id}] *** FAILED *** {} -- {msg}", seg.name);
                                stop.store(true, Ordering::Relaxed);
                                let (_, cv) = &*sched;
                                cv.notify_all();
                                return;
                            }
                        }
                    }
                }
            });
        }
    });

    let el = started.elapsed().as_secs_f64();
    let bd = blocks_done.load(Ordering::Relaxed);
    println!(
        "\n{bd} blocks re-executed in {:.1} h ({:.1} blk/s aggregate), peak RSS {} MB",
        el / 3600.0,
        bd as f64 / el,
        peak_rss.load(Ordering::Relaxed)
    );
    if fallback_dir.is_some() {
        let g = gaps.lock().unwrap();
        println!(
            "segment gaps: {} reads served by the fallback, {} absent in both, {} distinct keys recorded",
            from_full.load(Ordering::Relaxed),
            absent.load(Ordering::Relaxed),
            g.len()
        );
        // Beside the state file, not at a path baked in from the machine this
        // was written on. It used to be the latter, inside an `if let Ok`, so
        // anywhere else it produced no repair list and said nothing -- the
        // failure mode this tool exists to complain about.
        let gaps_path = std::path::Path::new(&state_path)
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("segment-gaps.txt");
        match std::fs::File::create(&gaps_path) {
            Ok(mut f) => {
                let _ = writeln!(f, "# block\tsegment\tnode-key -- reads a segment could not serve");
                for (b, seg, k) in g.iter() {
                    let _ = writeln!(f, "{b}\t{seg}\t{}", alloy_primitives::hex::encode(k));
                }
                println!("gap keys written to {} (the repair list)", gaps_path.display());
            }
            Err(e) => {
                println!(
                    "WARNING: could not write the repair list to {}: {e}. The {} gap(s) above \
                     are the only record.",
                    gaps_path.display(),
                    g.len()
                );
            }
        }
    }
    if stop.load(Ordering::Relaxed) {
        println!("STOPPED on a failure -- see above");
        std::process::exit(1);
    }
    // A verdict has to be earned. The scheduler drains its queue when the
    // smallest pending segment cannot fit the budget, so a run that executed
    // nothing reaches this line looking exactly like a run that executed
    // everything -- and this tool's whole purpose is to certify a database
    // before it is handed to someone else. Compare against the plan.
    // Reported before the verdict as well, because a run that stops early still
    // observed whatever it executed, and that observation is worth keeping.
    report_rsk_opcodes(bd, total_blocks);
    if bd < total_blocks {
        println!(
            "INCOMPLETE: {bd} of {total_blocks} planned blocks re-executed ({} not run). \
             No divergence was found, but this is not a pass -- see the errors above \
             (a segment over the memory budget is the usual cause).",
            total_blocks - bd
        );
        std::process::exit(2);
    }
    if bd > total_blocks {
        // Not a verification failure: every block that ran, matched. But the
        // plan and the work disagree, so the plan is wrong, and a plan that
        // cannot be trusted cannot certify anything either.
        println!(
            "INCONSISTENT: {bd} blocks re-executed against a plan of {total_blocks} ({} more \
             than planned). Every block that ran verified, but Segment::blocks() disagrees \
             with what run_segment replays, so this run's accounting is unreliable.",
            bd - total_blocks
        );
        std::process::exit(2);
    }
    println!("ALL {total_blocks} BLOCKS RE-EXECUTED, ALL SEGMENTS VERIFIED");
    report_rsk_opcodes(bd, total_blocks);
    Ok(())
}

/// What RSK's own opcodes did during the replay.
///
/// `DUPN` (0xa8), `SWAPN` (0xa9) and `TXINDEX` (0xaa) were live from genesis
/// until RSKIP191 removed them at iris300 (#3,614,800). Whether any
/// transaction ever reached one is not answerable from the code, and a replay
/// is the only thing that answers it exactly: a static scan of the state at
/// the fork rules it out for contracts alive then (`opcode_census` reports
/// none reachable), but not for contracts created and destroyed before it, nor
/// for code a contract computed at runtime.
///
/// Zero is therefore a result and not an absence -- provided the range replayed
/// actually covered the era, which is why the block counts are printed with it.
fn report_rsk_opcodes(done: u64, planned: u64) {
    let [dupn, swapn, txindex] = rustock_execution::rsk_instructions::rsk_opcode_hits();
    let [created, headered] = rustock_execution::rsk_instructions::contracts_created();
    println!();
    println!(
        "RSK opcodes executed in this run ({done} of {planned} planned blocks): \
         DUPN {dupn}, SWAPN {swapn}, TXINDEX {txindex}"
    );
    println!(
        "contracts deployed in this run: {created}, of which {headered} carried the RSK \
         code header"
    );
    // The legacy-branch census. Printed as a table with each row's window, so
    // a zero is always read against the range that produced it.
    let census = rustock_execution::legacy_census::census();
    println!();
    println!("legacy branches taken in this run:");
    for (rule, what, window, hits) in &census {
        println!("  {rule:<8} {hits:>10}  {what}  [{window}]");
    }
    // A campaign that only ever returns zero has not been validated. The
    // migration-end-age window is known occupied -- using the ordinary end age
    // inside it diverged a real mainnet block -- so a zero there means the
    // instrument is broken and every other zero in the table is worthless.
    if rustock_execution::legacy_census::positive_control_fired() {
        println!("  positive control fired: the counters are live, so the zeros above mean something.");
    } else {
        println!(
            "  POSITIVE CONTROL DID NOT FIRE. Either this run did not cover \
             #4,976,300..#5,468,000, or the counters are not wired to the branches they \
             name. Until that is resolved, treat every zero above as unproven."
        );
    }

    let [staticcalls, with_value] =
        rustock_execution::rsk_instructions::staticcall_value_counts();
    println!(
        "seven-operand STATICCALLs (RSKIP-91 to RSKIP-103, #729,000..#1,052,700): \
         {staticcalls} executed, {with_value} carrying a non-zero value"
    );
    if staticcalls > 0 && with_value == 0 {
        println!(
            "  the form ran but was never passed a value, so nothing on this chain ever \
             moved funds through a static call."
        );
    } else if staticcalls == 0 {
        println!(
            "  the seven-operand form never ran in this range -- which for a range below \
             #729,000 means only that the window was not reached."
        );
    }

    let [oversize, unpayable, largest] =
        rustock_execution::rsk_instructions::empty_deploy_counts();
    println!(
        "internal creations below RSKIP-453 that deployed empty instead of reverting: \
         {oversize} over the maximum code size, {unpayable} unable to pay the code deposit; \
         largest code any of them returned: {largest} octets"
    );
    // The bound matters more than the counts. RSKIP-453 is #7,338,024 on
    // mainnet, so a run that stops below that has only looked at part of the
    // era where the quirk applies, and zero means "not here" rather than
    // "never".
    if oversize == 0 && unpayable == 0 {
        println!(
            "  neither branch fired in this range. Both are dead guards below the highest \
             block this run reached -- not below #7,338,024, unless the run covered it."
        );
    }
    if created > 0 && headered == 0 {
        println!(
            "  no contract in this range was ever deployed with a header. A census of the \
             state at #3,614,799 found none alive there either, so for this range the \
             header is unused by deployment as well as by execution."
        );
    }
    if dupn == 0 && swapn == 0 && txindex == 0 {
        println!(
            "  none reached. For a range covering #1..#3,614,799 that is the answer to \
             whether RSKIP191 removed anything live."
        );
    } else {
        println!(
            "  at least one was reached: RSKIP191 removed something this chain had used. \
             The first eight occurrences are logged at WARN under \
             target=rustock::rsk_opcodes."
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn run_segment(
    seg: &Segment,
    store: &Arc<BlockStore>,
    shards: usize,
    blocks_done: &Arc<AtomicU64>,
    fallback: Option<&str>,
    from_full: &Arc<AtomicU64>,
    absent: &Arc<AtomicU64>,
    gaps: &Arc<Mutex<Vec<(u64, String, Vec<u8>)>>>,
    base: Option<&Arc<baseline::Baseline>>,
) -> anyhow::Result<(u64, f64)> {
    let loaded = load_segment(&seg.dir, shards)?;
    let mut with_fb: Option<Arc<RamWithFallback>> = None;
    let trie: Arc<dyn TrieStore> = match fallback {
        Some(dir) => {
            let w = Arc::new(RamWithFallback {
            ram: loaded,
            full: Box::new(rustock_storage::RocksDbTrieStore::open_read_only(dir)?),
            from_full: from_full.clone(),
            absent: absent.clone(),
            gaps: gaps.clone(),
                current_block: AtomicU64::new(0),
                segment: seg.name.clone(),
            });
            with_fb = Some(w.clone());
            w
        }
        None => Arc::new(loaded),
    };

    // Where to start, and what state to start from. Three cases, and only the
    // last is the ordinary one.
    //
    // A range beginning at block 1 has no parent block at all, so its seed is
    // the genesis allocation, built here rather than read. It is also the only
    // case that replays block 1 itself; seeding from `baseline[1]` instead
    // would take that block's root on trust and never check it.
    //
    // A range beginning inside the pre-fork era seeds from the baseline,
    // because the parent header's state root is an Orchid hash and is not a
    // key in a Unitrie store. This is what lets the era be cut into segments
    // at all.
    let mut from = seg.start + 1;
    let to = seg.end;
    let mut root = if seg.start <= 1 {
        from = 1;
        let config = ChainConfig::mainnet();
        let mut r = TrieNode::empty();
        for e in &config.genesis_alloc() {
            let k = account_key(&e.address);
            r = r.put(
                &TrieKeySlice::from_key(&k),
                &AccountState::new(e.nonce, e.balance).encode(),
                trie.as_ref(),
            );
        }
        // Deliberately not saved: this is a verification run against a
        // read-only segment, and the tree is small and fully materialized, so
        // nothing needs to resolve it through the store.
        r
    } else if let Some(seed_root) = base.as_ref().and_then(|b| {
        if seg.start < UNITRIE_FROM { b.root(seg.start) } else { None }
    }) {
        let data = trie.get(&seed_root).with_context(|| {
            format!(
                "no pre-state: baseline seed root for #{} absent from {}",
                seg.start, seg.name
            )
        })?;
        TrieNode::from_message(&data, trie.as_ref())
    } else {
        let seed_hash = store.canonical_hash(from - 1)?.context("no canonical hash for seed")?;
        let seed = store.header(seed_hash)?.context("no seed header")?;
        let root_data = trie.get(seed.state_root.as_slice()).with_context(|| {
            format!("no pre-state: seed root for #{} absent from {}", from - 1, seg.name)
        })?;
        TrieNode::from_message(&root_data, trie.as_ref())
    };

    let processor = BlockProcessor::new(RskHardforkConfig::mainnet(), store.clone());
    let t = Instant::now();
    let mut n = 0u64;
    for num in from..=to {
        if let Some(w) = &with_fb {
            w.current_block.store(num, Ordering::Relaxed);
        }
        let hash = store.canonical_hash(num)?.context("no canonical hash")?;
        let header = store.header(hash)?.context("no header")?;
        let (transactions, ommers) = store.body(hash)?.context("no body")?;

        // Below the fork the header's root is Orchid-format and can never
        // equal a computed Unitrie root, so the baseline supplies the expected
        // value. At and above #1,591,000 the header is authoritative again --
        // which means the first block of the fork is checked against the chain
        // rather than against the baseline, and that comparison is the anchor
        // that makes the whole era's recorded roots worth anything (8).
        let (expected, from_baseline) = if num < UNITRIE_FROM {
            let b = base.as_ref().context(
                "below the fork with no baseline: the header root is Orchid-format and \
                 cannot be compared against a computed Unitrie root",
            )?;
            let r = b.root(num).with_context(|| {
                format!(
                    "baseline covers #{}..#{} and so has no root for #{num}; it cannot check \
                     this segment",
                    b.first(),
                    b.last()
                )
            })?;
            (r.to_vec(), true)
        } else {
            (header.state_root.as_slice().to_vec(), false)
        };

        let block = Block { header, transactions, ommers };
        let p = processor.execute_block(&block, &root, trie.clone())?;
        anyhow::ensure!(
            p.state_root_hash.as_slice() == expected.as_slice(),
            "#{num}: STATE ROOT MISMATCH computed {} {} 0x{}",
            p.state_root_hash,
            if from_baseline { "baseline" } else { "header" },
            expected.iter().map(|x| format!("{x:02x}")).collect::<String>()
        );
        root = p.new_state_root;
        n += 1;
        blocks_done.fetch_add(1, Ordering::Relaxed);
    }
    let secs = t.elapsed().as_secs_f64();
    Ok((n, n as f64 / secs))
}
