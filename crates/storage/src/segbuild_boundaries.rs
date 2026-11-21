//! Where segments should be cut, decided while syncing and built later.
//!
//! The hot path does not build segments. It counts bytes and, when enough have
//! been written, records that a segment ends at this block. A background task
//! builds the range afterwards by re-executing it
//! (`docs/segbuild-continuous-design.md`).
//!
//! That split is the whole point. Building by interception made a segment a
//! side effect of whatever the node happened to do -- including executing
//! blocks beside the chain that never became canonical -- so the segment stopped
//! being *the read set of its range*, which is the only property that makes it
//! verifiable. Re-executing a settled canonical range in isolation has one
//! unambiguous read set.
//!
//! # What has to be durable, and what does not
//!
//! **A boundary must survive a crash.** Losing one merges two segments into an
//! oversized one that may not fit a consumer's memory, which is the property
//! the sizing exists to protect. So boundaries are appended and fsynced -- cheap,
//! at one line per gigabyte of state written.
//!
//! **The counter need not.** It resets on restart, so the segment in progress
//! is cut late. Sizes are advisory: they decide where to cut, and roughly the
//! right place is all that is required.
//!
//! # Append-only, two logs
//!
//! `BOUNDARIES` records decisions, `BUILT` records what has been built, and
//! `UNCOVERED` records a range that has been given up on. None is ever
//! rewritten, so a crash cannot corrupt one into something that parses as a
//! different fact. What remains to build is what is in the first and in neither
//! of the others.
//!
//! `UNCOVERED` is what keeps a hole in the set an explicit fact on disk rather
//! than a gap someone notices later by comparing segment names (§3.1). A range
//! that cannot be built -- its state collected, its disk full -- is retried a
//! few times and then written off, which is also what lets the builder carry on
//! with later ranges instead of looping on the one it cannot do.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A decision that a segment ends at a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Boundary {
    /// Last block of the segment.
    pub block: u64,
    /// Bytes written while it accumulated. Advisory; recorded so a surprising
    /// segment size can be traced to the decision that produced it.
    pub approx_bytes: u64,
}

/// A range waiting to be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingRange {
    pub start: u64,
    pub end: u64,
    pub approx_bytes: u64,
}

/// A range that has been built, and the block hash it was built against.
///
/// The hash is what makes a deep reorg detectable: if the canonical hash at
/// `end` is no longer this one, the segment describes blocks the chain has
/// abandoned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltRange {
    pub start: u64,
    pub end: u64,
    /// Canonical hash at `end` when the range was built. `None` for a record
    /// written before hashes were logged.
    pub end_hash: Option<[u8; 32]>,
}

/// A range the builder has given up on, and why.
///
/// The set then has a known hole, which can be filled from an archive. The
/// alternative -- retrying for ever -- would stop every later range from being
/// built and would log the same failure once a tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UncoveredRange {
    pub start: u64,
    pub end: u64,
    /// What the last attempt failed with. Free text, for a human.
    pub reason: String,
}

/// # This must never be able to stop the node
///
/// Extending the segment set is a by-product of syncing. Every lock here is
/// taken with `unwrap_or_else(|e| e.into_inner())` rather than `unwrap()`, so a
/// panic in a builder thread cannot poison a lock and then panic the node's
/// execution path on the next block. For the same reason nothing here panics,
/// exits, or returns an error that a caller is expected to treat as fatal:
/// errors describe a failure to extend the set, which is not a reason to stop
/// following the chain.
pub struct SegmentBoundaries {
    dir: PathBuf,
    target_bytes: u64,
    /// Bytes written since the last boundary. Deliberately not durable.
    counter: AtomicU64,
    /// Boundaries in the order recorded, lowest block first.
    boundaries: std::sync::RwLock<Vec<Boundary>>,
    /// Every range already built, with the hash it was built against.
    built: std::sync::RwLock<Vec<BuiltRange>>,
    /// Ranges written off as unbuildable, newest last.
    uncovered: std::sync::RwLock<Vec<UncoveredRange>>,
    /// Failed attempts per range, in this process only. Not durable: a
    /// restart is itself a reason to try again, since the commonest causes
    /// (full disk, a store that was mid-collection) are the kind that a
    /// restart may have cleared.
    attempts: std::sync::Mutex<Vec<(u64, u64, u32)>>,
}

const BOUNDARIES: &str = "BOUNDARIES";
const BUILT: &str = "BUILT";
const UNCOVERED: &str = "UNCOVERED";
const HALTED: &str = "HALTED";

fn hex32(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

impl SegmentBoundaries {
    /// Opens the logs in `dir`, creating it if needed.
    ///
    /// `target_bytes` is how much written state a segment should accumulate
    /// before being cut. Bytes, not blocks: cost per block varies tenfold
    /// across eras, so equal block counts produce segments from 0.2 GB to over
    /// 2 GB, while equal bytes produce the ~1 GB segments a consumer can hold
    /// in memory.
    pub fn open(dir: impl Into<PathBuf>, target_bytes: u64) -> Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creating {}", dir.display()))?;
        let boundaries = read_boundaries(&dir.join(BOUNDARIES))?;
        let built = read_built(&dir.join(BUILT))?;
        let uncovered = read_uncovered(&dir.join(UNCOVERED))?;
        Ok(Self {
            dir,
            target_bytes,
            counter: AtomicU64::new(0),
            boundaries: std::sync::RwLock::new(boundaries),
            built: std::sync::RwLock::new(built),
            uncovered: std::sync::RwLock::new(uncovered),
            attempts: std::sync::Mutex::new(Vec::new()),
        })
    }

    /// Account for bytes written by the block being executed.
    ///
    /// The entire hot-path cost of this design: one relaxed atomic add.
    pub fn add_bytes(&self, bytes: u64) {
        self.counter.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Call when a block finishes. Records a boundary if enough has been
    /// written, and returns it.
    ///
    /// Only ever cuts at a block boundary, because a segment that ended
    /// mid-block would cover part of a block's writes and claim the block.
    pub fn end_block(&self, block: u64) -> Result<Option<Boundary>> {
        let acc = self.counter.load(Ordering::Relaxed);
        if acc < self.target_bytes {
            return Ok(None);
        }
        // Reset first: if appending fails, the next block tries again rather
        // than this one cutting repeatedly.
        self.counter.store(0, Ordering::Relaxed);

        let b = Boundary { block, approx_bytes: acc };
        {
            let mut guard = self.boundaries.write().unwrap_or_else(|e| e.into_inner());
            if guard.last().is_some_and(|l| l.block >= block) {
                // Already recorded, or the chain moved backwards. Either way
                // do not append a boundary that is not above the last.
                return Ok(None);
            }
            append_line(
                &self.dir.join(BOUNDARIES),
                &format!("{}\t{}\t{}", b.block, b.approx_bytes, unix_now()),
            )?;
            guard.push(b);
        }
        Ok(Some(b))
    }

    /// Ranges whose end is at least `lag` blocks below `head` and which have
    /// not been built.
    ///
    /// A range starts after the previous boundary, or at `first_block` for the
    /// first one. Returned lowest first, so a builder works forwards and a
    /// later range is never built before an earlier one.
    pub fn ready(&self, head: u64, lag: u64, first_block: u64) -> Vec<PendingRange> {
        let boundaries = self.boundaries.read().unwrap_or_else(|e| e.into_inner());
        let built = self.built.read().unwrap_or_else(|e| e.into_inner());
        let uncovered = self.uncovered.read().unwrap_or_else(|e| e.into_inner());
        let mut out = Vec::new();
        let mut start = first_block;
        for b in boundaries.iter() {
            // A boundary at or below where this builder starts. It can only
            // happen if the set was extended underneath an existing log -- an
            // operator filling a gap offline, say. Such a "range" runs
            // backwards, so skip it, and do not let it drag `start` back below
            // where this builder begins.
            if b.block < start {
                continue;
            }
            let range = PendingRange { start, end: b.block, approx_bytes: b.approx_bytes };
            start = b.block + 1;
            if built.iter().any(|b| b.start == range.start && b.end == range.end) {
                continue;
            }
            // Written off. Offering it again would loop on the range the
            // builder has already failed at, and starve every later one.
            if uncovered.iter().any(|u| u.start == range.start && u.end == range.end) {
                continue;
            }
            if b.block + lag > head {
                // Not settled yet. Everything after this is higher still.
                break;
            }
            out.push(range);
        }
        out
    }

    /// The lowest block whose state a build still needs: one below the start
    /// of the lowest range that has not been built.
    ///
    /// This is half of what protects a pending range from the collector. It is
    /// passed to `EpochTrieStore::collect_bounded` as a **bound**, which keeps
    /// every node written *above* it -- including nodes created and destroyed
    /// inside the range, which no state at either end reaches. The other half
    /// is a **pin** on the state root at this same block, which keeps the seed
    /// trie the build starts from. Neither alone is sufficient:
    ///
    /// - the bound misses the seed, because the seed's nodes were written
    ///   below it and may already be dead at the collection root;
    /// - the pin misses the intra-range nodes, because they are in no state.
    ///
    /// Together they are exhaustive: a node read while re-executing
    /// `[start, end]` was either already in the trie at `start - 1` (pinned)
    /// or written by a block at or after `start` (bounded).
    ///
    /// Includes the range still accumulating, which has no boundary yet: its
    /// state has to be kept for the same reason, and its seed is the last
    /// boundary.
    ///
    /// `None` when there is nothing to protect, and -- deliberately -- when
    /// building is halted. A halted builder is waiting for a human, and a bound
    /// that waits for a human is a bound that fills the disk (§7.1). The
    /// pending state is then collected and those ranges have to be rebuilt from
    /// an archive, which is the lesser failure.
    pub fn collection_bound(&self, first_block: u64) -> Option<u64> {
        if self.halted().is_some() {
            return None;
        }
        let boundaries = self.boundaries.read().unwrap_or_else(|e| e.into_inner());
        let built = self.built.read().unwrap_or_else(|e| e.into_inner());
        let uncovered = self.uncovered.read().unwrap_or_else(|e| e.into_inner());
        let mut start = first_block;
        for b in boundaries.iter() {
            // Behind this builder's start; see `ready`.
            if b.block < start {
                continue;
            }
            let settled = built.iter().any(|x| x.start == start && x.end == b.block)
                // A range that has been written off is not going to be built,
                // so holding the floor for it only costs disk.
                || uncovered.iter().any(|u| u.start == start && u.end == b.block);
            if !settled {
                // Ranges are settled in order, so the first gap is the lowest.
                return Some(start.saturating_sub(1));
            }
            start = b.block + 1;
        }
        // Every closed range is built; the open one still needs its seed.
        Some(start.saturating_sub(1))
    }

    /// Record that `[start, end]` has been built, against the canonical hash
    /// at `end`.
    ///
    /// The hash is not decoration. It is the only way to notice later that the
    /// chain reorganised below a segment that already exists.
    pub fn mark_built(&self, start: u64, end: u64, end_hash: [u8; 32]) -> Result<()> {
        append_line(
            &self.dir.join(BUILT),
            &format!("{start}\t{end}\t{}\t{}", hex32(&end_hash), unix_now()),
        )?;
        self.built.write().unwrap_or_else(|e| e.into_inner()).push(BuiltRange {
            start,
            end,
            end_hash: Some(end_hash),
        });
        Ok(())
    }

    /// Count a failed attempt at `[start, end]` and report how many there have
    /// now been, this process.
    ///
    /// Not durable, deliberately: a restart is itself a reason to try again,
    /// because the usual causes -- a full disk, a store that was mid-collection
    /// -- are the kind a restart may have cleared. What *is* durable is giving
    /// up, which is [`Self::mark_uncovered`].
    pub fn record_failure(&self, start: u64, end: u64) -> u32 {
        let mut guard = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        match guard.iter_mut().find(|(s, e, _)| *s == start && *e == end) {
            Some((_, _, n)) => {
                *n += 1;
                *n
            }
            None => {
                guard.push((start, end, 1));
                1
            }
        }
    }

    /// Write off `[start, end]`: it will not be offered again, and it stops
    /// holding the collector's floor.
    ///
    /// This is §3.1's requirement that a hole be a fact on disk. The segment
    /// set then has a gap that can be filled from an archive, which is strictly
    /// better than a segment that claims the range and under-covers it.
    pub fn mark_uncovered(&self, start: u64, end: u64, reason: &str) -> Result<()> {
        // One line, no tabs or newlines inside the reason, or the record would
        // parse as a different one.
        let flat: String = reason.chars().map(|c| if c.is_control() || c == '\t' { ' ' } else { c }).collect();
        append_line(
            &self.dir.join(UNCOVERED),
            &format!("{start}\t{end}\t{}\t{flat}", unix_now()),
        )?;
        self.uncovered
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(UncoveredRange { start, end, reason: flat });
        Ok(())
    }

    /// Every range written off so far.
    pub fn uncovered(&self) -> Vec<UncoveredRange> {
        self.uncovered.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Whether building is halted, and why.
    ///
    /// Halting is durable and deliberately not self-clearing: the conditions
    /// that cause it mean something has gone wrong with the chain, not with
    /// this process, and a builder that resumed on its own would quietly
    /// produce segments across a discontinuity.
    pub fn halted(&self) -> Option<String> {
        std::fs::read_to_string(self.dir.join(HALTED)).ok()
    }

    /// Stop building, durably, with a reason an operator will read.
    pub fn halt(&self, reason: &str) -> Result<()> {
        let path = self.dir.join(HALTED);
        std::fs::write(
            &path,
            format!(
                "{reason}\n\nBuilding is stopped. Nothing will be built while this file \
                 exists.\nInvestigate, repair the set if needed, then remove this file to \
                 resume.\nrecorded {}\n",
                unix_now()
            ),
        )
        .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    /// Check that every built range still sits on the canonical chain.
    ///
    /// `canonical_hash_at` returns the canonical hash at a height, or `None` if
    /// the height is unknown.
    ///
    /// A reorg shallower than the lag cannot reach a built range, because
    /// nothing is built until it is `lag` blocks settled. So a mismatch here
    /// means the chain reorganised by more than the lag -- on mainnet, more
    /// than a thousand blocks. That is not an operational event to recover
    /// from; it is evidence that the chain's assumptions have failed, and the
    /// honest response is to stop and require a human.
    pub fn verify_built_still_canonical(
        &self,
        mut canonical_hash_at: impl FnMut(u64) -> Option<[u8; 32]>,
    ) -> Result<Option<String>> {
        let built = self.built.read().unwrap_or_else(|e| e.into_inner()).clone();
        for b in built {
            let Some(was) = b.end_hash else { continue };
            let Some(now) = canonical_hash_at(b.end) else {
                // The height is no longer known at all, which is itself a
                // reorg past it.
                let reason = format!(
                    "segbuild: block {} is no longer on the canonical chain. Segment \
                     {}..{} was built against {} and that height now has no canonical \
                     block. A reorg deeper than the build lag has happened.",
                    b.end, b.start, b.end, hex32(&was)
                );
                self.halt(&reason)?;
                return Ok(Some(reason));
            };
            if now != was {
                let reason = format!(
                    "segbuild: a reorg deeper than the build lag has happened. Segment \
                     {}..{} was built against block {} = {}, but that height is now {}. \
                     The segment describes blocks the chain has abandoned.",
                    b.start, b.end, b.end, hex32(&was), hex32(&now)
                );
                self.halt(&reason)?;
                return Ok(Some(reason));
            }
        }
        Ok(None)
    }

    /// Bytes accumulated since the last boundary.
    pub fn accumulated(&self) -> u64 {
        self.counter.load(Ordering::Relaxed)
    }

    /// Every boundary recorded so far.
    pub fn boundaries(&self) -> Vec<Boundary> {
        self.boundaries.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// How many ranges have been built.
    pub fn built_count(&self) -> usize {
        self.built.read().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Every range built so far.
    pub fn built(&self) -> Vec<BuiltRange> {
        self.built.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Append one line and fsync it.
///
/// `sync_all` rather than a buffered write: a boundary that is in the page
/// cache when the machine loses power is a boundary that did not happen, and
/// the cost is one fsync per gigabyte of state written.
fn append_line(path: &Path, line: &str) -> Result<()> {
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    writeln!(f, "{line}").with_context(|| format!("writing {}", path.display()))?;
    f.sync_all().with_context(|| format!("syncing {}", path.display()))?;
    Ok(())
}

/// Reads `BOUNDARIES`, ignoring a trailing partial line.
///
/// A crash during the append can leave a line without its newline. Such a line
/// records a boundary that may not have been acted on, and dropping it costs
/// one late cut -- whereas parsing half a number would invent a boundary
/// somewhere else entirely.
fn read_boundaries(path: &Path) -> Result<Vec<Boundary>> {
    let Ok(text) = std::fs::read_to_string(path) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for line in complete_lines(&text) {
        let mut f = line.split('\t');
        let (Some(block), Some(bytes)) = (f.next(), f.next()) else { continue };
        let (Ok(block), Ok(approx_bytes)) = (block.parse(), bytes.parse()) else { continue };
        out.push(Boundary { block, approx_bytes });
    }
    out.sort_by_key(|b| b.block);
    Ok(out)
}

fn read_built(path: &Path) -> Result<Vec<BuiltRange>> {
    let Ok(text) = std::fs::read_to_string(path) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for line in complete_lines(&text) {
        let mut f = line.split('\t');
        let (Some(s), Some(e)) = (f.next(), f.next()) else { continue };
        let (Ok(start), Ok(end)) = (s.parse(), e.parse()) else { continue };
        // A record written before hashes were logged has a timestamp here
        // instead; it parses as no hash, and such a range is simply not
        // checked rather than treated as reorganised.
        let end_hash = f.next().and_then(unhex32);
        out.push(BuiltRange { start, end, end_hash });
    }
    Ok(out)
}

fn read_uncovered(path: &Path) -> Result<Vec<UncoveredRange>> {
    let Ok(text) = std::fs::read_to_string(path) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for line in complete_lines(&text) {
        let mut f = line.split('\t');
        let (Some(s), Some(e)) = (f.next(), f.next()) else { continue };
        let (Ok(start), Ok(end)) = (s.parse(), e.parse()) else { continue };
        let _when = f.next();
        let reason = f.next().unwrap_or("(no reason recorded)").to_string();
        out.push(UncoveredRange { start, end, reason });
    }
    Ok(out)
}

/// Lines that were fully written, i.e. everything before the last newline.
fn complete_lines(text: &str) -> impl Iterator<Item = &str> {
    let end = text.rfind('\n').map_or(0, |i| i + 1);
    text[..end].lines().filter(|l| !l.trim().is_empty())
}

/// Counts bytes written through it, and changes nothing else.
///
/// This is interception, but only of a number. The capture design that was
/// abandoned had to copy every node, which made a segment depend on what the
/// node happened to execute; counting an uncle's writes merely cuts a segment
/// slightly early, and sizes are advisory.
pub struct CountingWrites {
    inner: std::sync::Arc<dyn rustock_trie::TrieStore>,
    boundaries: std::sync::Arc<SegmentBoundaries>,
}

impl CountingWrites {
    pub fn new(
        inner: std::sync::Arc<dyn rustock_trie::TrieStore>,
        boundaries: std::sync::Arc<SegmentBoundaries>,
    ) -> Self {
        Self { inner, boundaries }
    }
}

impl rustock_trie::TrieStore for CountingWrites {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.inner.get(key)
    }

    fn put(&self, key: &[u8], value: &[u8]) {
        self.boundaries.add_bytes((key.len() + value.len()) as u64);
        self.inner.put(key, value);
    }

    fn put_many(&self, items: &[(Vec<u8>, Vec<u8>)]) {
        let bytes: usize = items.iter().map(|(k, v)| k.len() + v.len()).sum();
        self.boundaries.add_bytes(bytes as u64);
        self.inner.put_many(items);
    }

    fn flush(&self) {
        self.inner.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_boundary_is_cut_only_once_the_target_is_reached() {
        let dir = tempfile::tempdir().unwrap();
        let b = SegmentBoundaries::open(dir.path(), 1000).unwrap();

        b.add_bytes(400);
        assert_eq!(b.end_block(10).unwrap(), None, "under target");
        b.add_bytes(700);
        assert_eq!(
            b.end_block(11).unwrap(),
            Some(Boundary { block: 11, approx_bytes: 1100 }),
            "over target, cut at the block that crossed it"
        );
        assert_eq!(b.accumulated(), 0, "counter resets after a cut");
        assert_eq!(b.end_block(12).unwrap(), None, "does not cut again immediately");
    }

    #[test]
    fn boundaries_survive_a_restart_and_the_counter_does_not() {
        let dir = tempfile::tempdir().unwrap();
        {
            let b = SegmentBoundaries::open(dir.path(), 100).unwrap();
            b.add_bytes(150);
            b.end_block(5).unwrap();
            b.add_bytes(60); // in progress, deliberately not durable
        }
        let b = SegmentBoundaries::open(dir.path(), 100).unwrap();
        assert_eq!(b.boundaries(), vec![Boundary { block: 5, approx_bytes: 150 }]);
        assert_eq!(b.accumulated(), 0, "the counter is not durable by design");
    }

    /// A crash during the append can leave a line with no newline. Dropping it
    /// costs one late cut; parsing it could invent a boundary elsewhere.
    #[test]
    fn a_torn_trailing_line_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(BOUNDARIES), "5\t150\t1700000000\n9\t20").unwrap();
        let b = SegmentBoundaries::open(dir.path(), 100).unwrap();
        assert_eq!(
            b.boundaries(),
            vec![Boundary { block: 5, approx_bytes: 150 }],
            "the complete line survives, the torn one does not"
        );
    }

    #[test]
    fn ready_waits_for_the_lag_and_skips_what_is_built() {
        let dir = tempfile::tempdir().unwrap();
        let b = SegmentBoundaries::open(dir.path(), 10).unwrap();
        for block in [100u64, 200, 300] {
            b.add_bytes(10);
            b.end_block(block).unwrap();
        }

        // head only 500: the 300 boundary is not yet 1000 blocks settled.
        assert_eq!(
            b.ready(500, 1000, 1),
            Vec::new(),
            "nothing is settled enough to build"
        );

        let ready = b.ready(1200, 1000, 1);
        assert_eq!(
            ready,
            vec![
                PendingRange { start: 1, end: 100, approx_bytes: 10 },
                PendingRange { start: 101, end: 200, approx_bytes: 10 },
            ],
            "100 and 200 are settled; 300 needs head >= 1300"
        );

        b.mark_built(1, 100, [7u8; 32]).unwrap();
        assert_eq!(
            b.ready(1200, 1000, 1),
            vec![PendingRange { start: 101, end: 200, approx_bytes: 10 }],
            "a built range is not offered again"
        );
    }

    #[test]
    fn built_ranges_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        {
            let b = SegmentBoundaries::open(dir.path(), 10).unwrap();
            b.add_bytes(10);
            b.end_block(50).unwrap();
            b.mark_built(1, 50, [7u8; 32]).unwrap();
        }
        let b = SegmentBoundaries::open(dir.path(), 10).unwrap();
        assert_eq!(b.built_count(), 1);
        assert!(b.ready(10_000, 1000, 1).is_empty(), "already built");
    }

    /// A boundary must never be recorded at or below the last one: the ranges
    /// are derived by walking boundaries in order, so a backwards one would
    /// produce a range with `start > end`.
    #[test]
    fn a_boundary_at_or_below_the_last_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let b = SegmentBoundaries::open(dir.path(), 10).unwrap();
        b.add_bytes(10);
        assert!(b.end_block(100).unwrap().is_some());
        b.add_bytes(10);
        assert_eq!(b.end_block(100).unwrap(), None, "same block");
        b.add_bytes(10);
        assert_eq!(b.end_block(99).unwrap(), None, "below the last");
        assert_eq!(b.boundaries().len(), 1);
    }

    /// A reorg deeper than the lag reaches a range already built. That is not
    /// an operational event -- on mainnet it means more than a thousand blocks
    /// were abandoned, which implies the chain itself is under attack -- so the
    /// builder stops and stays stopped until a human clears it.
    #[test]
    fn a_reorg_below_a_built_segment_halts_building() {
        let dir = tempfile::tempdir().unwrap();
        let b = SegmentBoundaries::open(dir.path(), 10).unwrap();
        b.add_bytes(10);
        b.end_block(5_000).unwrap();
        b.mark_built(1, 5_000, [0xAA; 32]).unwrap();

        assert!(b.halted().is_none(), "nothing wrong yet");
        assert_eq!(
            b.verify_built_still_canonical(|_| Some([0xAA; 32])).unwrap(),
            None,
            "same hash: the segment still describes the chain"
        );
        assert!(b.halted().is_none());

        // The chain now has a different block at that height.
        let reason = b
            .verify_built_still_canonical(|_| Some([0xBB; 32]))
            .unwrap()
            .expect("a changed hash must be reported");
        assert!(reason.contains("reorg deeper than the build lag"), "{reason}");

        let halted = b.halted().expect("halt must be durable");
        assert!(halted.contains("Nothing will be built while this file exists"));

        // And it survives a restart: halting is not self-clearing.
        let b2 = SegmentBoundaries::open(dir.path(), 10).unwrap();
        assert!(b2.halted().is_some(), "a halt must outlive the process");
    }

    /// A height that has vanished entirely is also a reorg past it.
    #[test]
    fn a_built_segment_whose_height_is_gone_halts_building() {
        let dir = tempfile::tempdir().unwrap();
        let b = SegmentBoundaries::open(dir.path(), 10).unwrap();
        b.add_bytes(10);
        b.end_block(42).unwrap();
        b.mark_built(1, 42, [0xCC; 32]).unwrap();

        let reason = b
            .verify_built_still_canonical(|_| None)
            .unwrap()
            .expect("an unknown height must be reported");
        assert!(reason.contains("no longer on the canonical chain"), "{reason}");
        assert!(b.halted().is_some());
    }

    /// The hash survives a restart, or the check could not run after one.
    #[test]
    fn the_hash_a_range_was_built_against_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        {
            let b = SegmentBoundaries::open(dir.path(), 10).unwrap();
            b.add_bytes(10);
            b.end_block(77).unwrap();
            b.mark_built(1, 77, [0xDE; 32]).unwrap();
        }
        let b = SegmentBoundaries::open(dir.path(), 10).unwrap();
        assert_eq!(
            b.built(),
            vec![BuiltRange { start: 1, end: 77, end_hash: Some([0xDE; 32]) }]
        );
    }

    #[test]
    fn the_collection_bound_tracks_the_lowest_unbuilt_range() {
        let dir = tempfile::tempdir().unwrap();
        let b = SegmentBoundaries::open(dir.path(), 10).unwrap();

        // Nothing closed yet: the open range starts at the first block, so its
        // seed is the block before it.
        assert_eq!(b.collection_bound(1), Some(0));

        b.add_bytes(10);
        b.end_block(1_000).unwrap();
        b.add_bytes(10);
        b.end_block(2_000).unwrap();

        // Two closed ranges pending, lowest first.
        assert_eq!(b.collection_bound(1), Some(0), "1..1000 is unbuilt");

        b.mark_built(1, 1_000, [1; 32]).unwrap();
        assert_eq!(
            b.collection_bound(1),
            Some(1_000),
            "the floor follows the builder: 1001..2000 seeds from #1000"
        );

        b.mark_built(1_001, 2_000, [2; 32]).unwrap();
        assert_eq!(
            b.collection_bound(1),
            Some(2_000),
            "all closed ranges built; the open range still seeds from #2000"
        );
    }

    /// §7.1: a bound that waits for a human is a bound that fills the disk.
    #[test]
    fn a_halted_builder_holds_no_bound() {
        let dir = tempfile::tempdir().unwrap();
        let b = SegmentBoundaries::open(dir.path(), 10).unwrap();
        b.add_bytes(10);
        b.end_block(1_000).unwrap();
        assert_eq!(b.collection_bound(1), Some(0));

        b.halt("deliberate, for the test").unwrap();
        assert_eq!(
            b.collection_bound(1),
            None,
            "a halted builder must not hold the collector's floor down for ever"
        );
    }

    /// §3.1: a range that cannot be built becomes a recorded hole, not an
    /// infinite retry and not a segment that under-covers.
    #[test]
    fn a_written_off_range_stops_being_offered_and_stops_holding_the_floor() {
        let dir = tempfile::tempdir().unwrap();
        let b = SegmentBoundaries::open(dir.path(), 10).unwrap();
        b.add_bytes(10);
        b.end_block(1_000).unwrap();
        b.add_bytes(10);
        b.end_block(2_000).unwrap();

        assert_eq!(b.ready(10_000, 100, 1).len(), 2);
        assert_eq!(b.collection_bound(1), Some(0));

        assert_eq!(b.record_failure(1, 1_000), 1, "attempts are counted per range");
        assert_eq!(b.record_failure(1, 1_000), 2);
        assert_eq!(b.record_failure(1_001, 2_000), 1, "and separately per range");

        b.mark_uncovered(1, 1_000, "the seed state was collected").unwrap();

        let ready = b.ready(10_000, 100, 1);
        assert_eq!(ready.len(), 1, "the written-off range is not offered again");
        assert_eq!(ready[0].start, 1_001, "the builder moves on to the next one");
        assert_eq!(
            b.collection_bound(1),
            Some(1_000),
            "and it stops holding the collector's floor at its own start"
        );

        // The hole is on disk, with a reason, and survives a restart.
        let b2 = SegmentBoundaries::open(dir.path(), 10).unwrap();
        assert_eq!(
            b2.uncovered(),
            vec![UncoveredRange {
                start: 1,
                end: 1_000,
                reason: "the seed state was collected".into()
            }]
        );
        assert_eq!(b2.ready(10_000, 100, 1).len(), 1);
        assert_eq!(
            b2.record_failure(1_001, 2_000),
            1,
            "attempt counts are per process: a restart is a reason to try again"
        );
    }

    /// A reason is free text from an error chain, which can contain anything.
    /// A tab or a newline in it would make the record parse as a different one.
    #[test]
    fn a_reason_cannot_break_its_own_record() {
        let dir = tempfile::tempdir().unwrap();
        let b = SegmentBoundaries::open(dir.path(), 10).unwrap();
        b.mark_uncovered(5, 9, "line one\nline\ttwo\t42\t99").unwrap();

        let b2 = SegmentBoundaries::open(dir.path(), 10).unwrap();
        assert_eq!(
            b2.uncovered(),
            vec![UncoveredRange { start: 5, end: 9, reason: "line one line two 42 99".into() }],
            "one record, with the separators flattened out of the reason"
        );
    }

    /// The set can be extended underneath an existing log -- an operator
    /// filling a hole offline moves where this builder starts. The boundaries
    /// below that point then describe ranges that run backwards, and a builder
    /// must never be handed one.
    #[test]
    fn a_boundary_below_the_builders_start_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let b = SegmentBoundaries::open(dir.path(), 10).unwrap();
        b.add_bytes(10);
        b.end_block(1_000).unwrap();
        b.add_bytes(10);
        b.end_block(5_000).unwrap();

        // The set now covers up to #2,000, so this builder starts at #2,001
        // and the #1,000 boundary is behind it.
        let ready = b.ready(10_000, 100, 2_001);
        assert_eq!(ready.len(), 1, "only the range that runs forwards: {ready:?}");
        assert_eq!(
            (ready[0].start, ready[0].end),
            (2_001, 5_000),
            "and the skipped boundary must not drag the next range's start back with it"
        );
        assert_eq!(b.collection_bound(2_001), Some(2_000), "the floor follows the same rule");
    }

    #[test]
    fn counting_writes_counts_and_delegates() {
        use rustock_trie::TrieStore as _;
        let dir = tempfile::tempdir().unwrap();
        let boundaries = std::sync::Arc::new(SegmentBoundaries::open(dir.path(), 1 << 40).unwrap());
        let inner = std::sync::Arc::new(rustock_trie::MemoryTrieStore::new());
        let counting = CountingWrites::new(inner.clone(), boundaries.clone());

        counting.put(&[1u8; 32], b"hello");
        assert_eq!(boundaries.accumulated(), 32 + 5);
        assert_eq!(
            counting.get(&[1u8; 32]).as_deref(),
            Some(&b"hello"[..]),
            "reads pass through untouched"
        );

        counting.put_many(&[(vec![2u8; 32], vec![0u8; 10]), (vec![3u8; 32], vec![0u8; 20])]);
        assert_eq!(boundaries.accumulated(), 37 + (32 + 10) + (32 + 20));
        assert!(inner.get(&[3u8; 32]).is_some(), "the batch reached the inner store");
    }
}
