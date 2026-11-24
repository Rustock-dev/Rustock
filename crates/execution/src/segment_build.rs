//! Build one segbuild segment by re-executing the block range it covers.
//!
//! This is the deferred half of `docs/segbuild-continuous-design.md`: the node
//! syncs normally and records *where* segments should end (a counter and an
//! append-only log, in `rustock_storage::segbuild_boundaries`), and this builds
//! them afterwards by replaying a settled range out of the node's own store.
//!
//! It is the library form of the `build_segments` example that built the
//! existing 161 GB set, reduced to the one case a running node needs: a single
//! range, no chunk claiming, no worker pool, no baseline. Everything that made
//! that tool correct is kept, and the comments say which measurement each piece
//! comes from, because every one of them was learnt by getting it wrong first.
//!
//! # Either a correct segment or no segment
//!
//! A segment that under-covers its range is the worst outcome available: a
//! missing trie node is indistinguishable at the API from a genuine absence
//! (§6.9d), so a consumer reads a wrong answer rather than an error. Two things
//! prevent it here.
//!
//! **Every state root is verified.** From #1,591,000 every header carries a
//! Unitrie root, so the replay checks itself at every block and stops at the
//! first mismatch.
//!
//! **The segment is built under a name no reader recognises**, and renamed only
//! after the range has been replayed to its end and the window sealed. A build
//! that fails, or is killed, leaves `.building-<start>-<end>`, which
//! `NNN-start-end` parsing rejects. There is no window in which a half-built
//! segment is visible as a segment.
//!
//! # What it reads
//!
//! The node's block store, and the node's trie store as the fallback under a
//! sliding window of in-memory tables. The trie store is garbage-collected, so
//! whether the nodes are still there is not an accident: the builder's range
//! holds a collection bound and a pin while it is pending
//! (`SegmentBoundaries::collection_bound`). A node that was collected anyway
//! surfaces here as a failed build, which is the honest outcome.

use crate::processor::BlockProcessor;
use anyhow::{bail, Context, Result};
use rustock_core::Block;
use rustock_storage::segbuild_boundaries::SegmentBoundaries;
use rustock_storage::window_store::{WindowConfig, WindowStore};
use rustock_storage::BlockStore;
use rustock_trie::{TrieNode, TrieStore};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, info, warn};

/// First block whose header carries a Unitrie root. Below this a header
/// declares an Orchid root, which names no node in a Unitrie store: such a
/// range can neither be seeded from its parent header nor verified against it,
/// and is built by the offline tool with a baseline instead.
pub const UNITRIE_FROM: u64 = 1_591_000;

/// Blocks between dropping the materialized trie tree and re-rooting from the
/// hash.
///
/// `execute_block` returns a `TrieNode` whose children are materialized as
/// execution touches them, and carrying it forward accumulates the whole
/// touched subtree in memory: measured at ~800 MB against a 145 MB window,
/// i.e. the tree was five times the cache it was supposed to be feeding. Every
/// one of those nodes is already in the window store, so the tree is pure
/// duplication -- re-rooting from the hash makes it lazy again and the nodes
/// fault back in from memory.
const REROOT_EVERY: u64 = 500;

/// Blocks between checkpoints. Each one seals the resident window, so it costs
/// a refill afterwards; rare enough not to matter, frequent enough that a kill
/// costs minutes rather than hours.
const CHECKPOINT_EVERY: u64 = 20_000;

/// Blocks between progress lines.
///
/// A segment is ~90,000 blocks and runs at order 100 blk/s, so without this a
/// build is a quarter of an hour of silence between "Building" and "Built" --
/// and an operator cannot tell a slow build from a stuck one. Nine lines per
/// segment is enough to see the rate and cheap enough to ignore.
const PROGRESS_EVERY: u64 = 10_000;

#[derive(Clone, Debug)]
pub struct BuildConfig {
    /// Directory holding the segment set. The segment is created inside it.
    pub out_root: PathBuf,
    /// Bytes an in-memory table accumulates before it is sealed to disk.
    pub seal_bytes: u64,
    /// In-memory tables kept resident. Memory, not core count, is the limit:
    /// three workers at 700 MB x 2 tables was the measured ceiling on a 7.7 GB
    /// box, and a fourth was *worse* than three.
    pub tables: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildOutcome {
    /// Final name of the segment directory, relative to `out_root`.
    pub name: String,
    pub start: u64,
    pub end: u64,
    /// Blocks replayed. Less than the range when a build resumed from a
    /// checkpoint.
    pub blocks: u64,
    /// State roots checked against a header. Equal to `blocks` for any range
    /// this builder accepts.
    pub verified: u64,
    pub segments: usize,
    /// Canonical hash at `end` when the build ran. The caller records it, and
    /// it is the only way to notice later that the chain reorganised below a
    /// segment that already exists.
    pub end_hash: [u8; 32],
}

/// Make a finished segment readable by whatever will consume it.
///
/// The node runs with `UMask=0077` -- sensible hardening for a node -- so
/// everything it creates is owner-only. A segment nothing but the node can open
/// defeats the entire purpose of the set, and the failure is silent: the build
/// verifies, the log says so, and a consumer gets a permission error that looks
/// nothing like "this segment is fine, you just cannot read it".
///
/// So the mode is set explicitly rather than inherited. Offline builds produce
/// `drwxr-xr-x`/`-rw-r--r--` because the tool runs under a normal umask, and
/// this makes the node's output match, so a consumer cannot tell which built
/// which.
///
/// Best-effort: a segment that is correct but awkwardly permissioned is still
/// a correct segment, so a failure here is logged and not fatal.
#[cfg(unix)]
fn relax_permissions(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;

    fn walk(path: &Path, depth: u32) -> std::io::Result<()> {
        // Segment stores are two levels deep (`<segment>/sealed/*.sst`); the
        // bound is only here so a symlink loop cannot spin.
        if depth > 8 {
            return Ok(());
        }
        let meta = std::fs::symlink_metadata(path)?;
        if meta.file_type().is_symlink() {
            return Ok(());
        }
        let mode = if meta.is_dir() { 0o755 } else { 0o644 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
        if meta.is_dir() {
            for entry in std::fs::read_dir(path)? {
                walk(&entry?.path(), depth + 1)?;
            }
        }
        Ok(())
    }

    if let Err(e) = walk(dir, 0) {
        warn!(
            target: "rustock::segbuild",
            "Built {} but could not make it readable ({e}); a consumer running as another \
             user will get a permission error rather than data. Fix with \
             `chmod -R a+rX {}`.",
            dir.display(),
            dir.display()
        );
    }
}

#[cfg(not(unix))]
fn relax_permissions(_dir: &Path) {}

/// The directory a build in progress uses. Deliberately not parseable as
/// `NNN-start-end`, so no reader can mistake it for a segment.
pub fn pending_dir(out_root: &Path, start: u64, end: u64) -> PathBuf {
    out_root.join(format!(".building-{start}-{end}"))
}

/// The next segment index: one above the highest already present.
///
/// Only a name that is exactly `NNN-start-end` counts. A suffixed name is a
/// retired segment and must not influence numbering -- two abandoned
/// directories sharing an index is what made §11 of `trie-segments-design.md`
/// hard to diagnose. An index that repeats stops indicating order, which is the
/// whole reason the index is in the name.
pub fn next_index(out_root: &Path) -> Result<u64> {
    let mut highest: Option<u64> = None;
    let dir = match std::fs::read_dir(out_root) {
        Ok(d) => d,
        // No set yet is not an error: the first build creates it.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e).context(format!("reading {}", out_root.display())),
    };
    for entry in dir {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(idx) = parse_index(&name) {
            highest = Some(highest.map_or(idx, |h: u64| h.max(idx)));
        }
    }
    Ok(highest.map_or(0, |h| h + 1))
}

/// The highest block any segment in the set covers, or `None` for an empty
/// set.
///
/// This is where a continuous builder picks up: the first range it is
/// responsible for starts one block later. Deriving it from the directory names
/// rather than configuring it means a set cannot be extended from the wrong
/// place by a mistyped flag.
///
/// Only an exact `NNN-start-end` name counts. The production set holds
/// `000-1-1591000.superseded-by-900-908-20261120` and
/// `012-2938787-3081559.abandoned-empty-superseded-20261120`; a retired segment
/// covers nothing.
pub fn set_end(out_root: &Path) -> Result<Option<u64>> {
    let dir = match std::fs::read_dir(out_root) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).context(format!("reading {}", out_root.display())),
    };
    let mut highest: Option<u64> = None;
    for entry in dir {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some((_, end)) = parse_range(&name) {
            highest = Some(highest.map_or(end, |h: u64| h.max(end)));
        }
    }
    Ok(highest)
}

/// `NNN-start-end`, and nothing else.
fn parse_range(name: &str) -> Option<(u64, u64)> {
    let parts: Vec<&str> = name.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    parts[0].parse::<u64>().ok()?;
    Some((parts[1].parse().ok()?, parts[2].parse().ok()?))
}

/// The index of a `NNN-start-end` name.
fn parse_index(name: &str) -> Option<u64> {
    parse_range(name)?;
    name.split('-').next()?.parse().ok()
}

/// Re-execute `[start, end]` and seal the segment it reads.
///
/// `source` is the node's own trie store, read-only as far as this is
/// concerned: every node read is copied into the window, and the window is what
/// is written. `cancel` is polled once per block so a shutdown does not have to
/// wait for a range to finish; a cancelled build leaves its pending directory
/// and resumes from the last checkpoint.
///
/// `processor` must be configured like the node's own, or the replay is not a
/// replay: chain config, fork detection and Bridge constants all change what a
/// block computes, and a segment built under different rules would fail its
/// own verification rather than be silently wrong -- but failing every build on
/// a chain with custom Bridge constants is not a useful outcome either. What it
/// must *not* carry is the legacy event store: re-executing a block that has
/// already been recorded would record its peg events twice.
pub fn build_range(
    blocks: Arc<BlockStore>,
    source: Arc<dyn TrieStore>,
    processor: &BlockProcessor,
    config: &BuildConfig,
    start: u64,
    end: u64,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<Option<BuildOutcome>> {
    if start > end {
        bail!("range #{start}..#{end} is not a range");
    }
    // Empty the BTC block cache before replaying this range.
    //
    // A segment must contain every node a *cold* replay of its range reads. A
    // cache carried in from the previous segment serves some of those reads
    // from memory, so the nodes never enter the window and the segment is
    // complete only for a reader whose cache happens to be warm the same way.
    // Clearing here makes the build's read set match a replay that starts cold
    // at the same boundary.
    //
    // Clearing rather than disabling is deliberate: the cache still pays for
    // itself *within* a range, and a disabled cache would make the build read a
    // superset of what any replay needs. See `docs/trie-segments-design.md`
    // 6.9d and the 2026-11-22 measurement.
    if let Some(cache) = processor.btc_block_cache() {
        cache.clear();
    }
    if start < UNITRIE_FROM {
        bail!(
            "refusing to build #{start}..#{end}: below #{UNITRIE_FROM} a header declares an \
             Orchid root, which names no node in a Unitrie store. The range can be neither \
             seeded from its parent header nor verified against it. Build the pre-fork era \
             with the offline tool and a baseline."
        );
    }

    let end_hash = blocks
        .canonical_hash(end)?
        .with_context(|| format!("no canonical block at #{end}; refusing to build to there"))?;

    std::fs::create_dir_all(&config.out_root)
        .with_context(|| format!("creating {}", config.out_root.display()))?;
    let pending = pending_dir(&config.out_root, start, end);

    let window = Arc::new(WindowStore::new(
        WindowConfig {
            seal_bytes: config.seal_bytes,
            tables: config.tables,
            dir: pending.clone(),
        },
        Some(source.clone()),
    )?);
    let win: Arc<dyn TrieStore> = window.clone();

    // Resume a build this range already started. The pending directory's name
    // is derived from the range, so a restart finds its own work.
    let mut first = start;
    let mut root: Option<TrieNode> = None;
    if let Some((last, root_bytes)) = WindowStore::read_checkpoint(&pending) {
        if last >= start && last < end {
            match win.get(&root_bytes).or_else(|| source.get(&root_bytes)) {
                Some(data) => {
                    info!(
                        target: "rustock::segbuild",
                        "Resuming the build of #{start}..#{end} at #{}", last + 1
                    );
                    root = Some(TrieNode::from_message(&data, win.as_ref()));
                    first = last + 1;
                }
                None => warn!(
                    target: "rustock::segbuild",
                    "The checkpoint at #{last} names a root that is no longer readable; \
                     building #{start}..#{end} from the start"
                ),
            }
        }
    }

    // Seed from the parent's state.
    let mut root = match root {
        Some(r) => r,
        None => {
            let hash = blocks
                .canonical_hash(first)?
                .with_context(|| format!("no canonical block at #{first}"))?;
            let header = blocks
                .header(hash)?
                .with_context(|| format!("no header for the canonical block at #{first}"))?;
            let parent = blocks
                .header(header.parent_hash)?
                .with_context(|| format!("no parent header for #{first}"))?;
            let seed = parent.state_root;
            let data = source.get(seed.as_slice()).with_context(|| {
                format!(
                    "the state root {seed:?} of #{} is not in the node's store. The range's \
                     seed state has been collected -- either the build lag is deeper than \
                     the collector's bound, or a bound was overridden as stale. This range \
                     has to be filled from an archive.",
                    parent.number
                )
            })?;
            // Put the seed into the window as well as reading it. This read is
            // the one in the whole build that bypasses copy-on-read, and
            // leaving it out is why 96 of the original 97 chunks could not
            // replay their own first block (§11.3a) and why segment 097 needed
            // repairing after the fact.
            win.put(seed.as_slice(), &data);
            info!(
                target: "rustock::segbuild",
                "Seeded #{start}..#{end} from #{} root {seed:?}", parent.number
            );
            TrieNode::from_message(&data, source.as_ref())
        }
    };

    let t0 = Instant::now();
    let mut done = 0u64;
    let mut verified = 0u64;

    for n in first..=end {
        if cancel() {
            info!(
                target: "rustock::segbuild",
                "Cancelled building #{start}..#{end} at #{n}; the partial build is kept and \
                 will resume from its last checkpoint"
            );
            window.finish()?;
            return Ok(None);
        }
        window.set_block(n);

        let hash = blocks
            .canonical_hash(n)?
            .with_context(|| format!("no canonical hash at #{n} while building #{start}..#{end}"))?;
        let header = blocks.header(hash)?.with_context(|| format!("no header at #{n}"))?;
        let (txs, ommers) = blocks.body(hash)?.with_context(|| format!("no body at #{n}"))?;
        let header_root = header.state_root;

        let processed = processor
            .execute_block(&Block { header, transactions: txs, ommers }, &root, win.clone())
            .with_context(|| format!("executing #{n}"))?;

        if processed.state_root_hash != header_root {
            // Stop here and keep nothing. The window is sealed so the partial
            // work is not lost to a later diagnosis, but it stays under the
            // pending name, which no reader will treat as a segment.
            window.finish()?;
            bail!(
                "state root mismatch at #{n} while building #{start}..#{end}: computed {:?}, \
                 header says {header_root:?}. No segment was produced.",
                processed.state_root_hash
            );
        }
        verified += 1;

        let root_hash = processed.state_root_hash;
        root = processed.new_state_root;
        done += 1;

        if done.is_multiple_of(REROOT_EVERY) {
            match win.get(root_hash.as_slice()) {
                Some(data) => root = TrieNode::from_message(&data, win.as_ref()),
                None => warn!(
                    target: "rustock::segbuild",
                    "Re-root at #{n}: {root_hash:?} is not readable; keeping the \
                     materialized tree, which costs memory but not correctness"
                ),
            }
        }
        if done.is_multiple_of(CHECKPOINT_EVERY) {
            window.checkpoint(n, root_hash.as_slice())?;
        }
        if done.is_multiple_of(PROGRESS_EVERY) {
            let secs = t0.elapsed().as_secs_f64();
            info!(
                target: "rustock::segbuild",
                "Building #{start}..#{end}: at #{n}, {done} of {} blocks, {:.0} blk/s,                  {} MB sealed",
                end - first + 1,
                done as f64 / secs.max(0.001),
                window.sealed_bytes.load(std::sync::atomic::Ordering::Relaxed) / (1 << 20)
            );
        }
    }

    window.finish()?;
    let segments = window.segment_count();
    drop(window);

    // Only now does the segment exist. The rename is the commit.
    let index = next_index(&config.out_root)?;
    let name = format!("{index:03}-{start}-{end}");
    let final_dir = config.out_root.join(&name);
    if final_dir.exists() {
        bail!(
            "{} already exists; refusing to replace a segment. The build is in {}",
            final_dir.display(),
            pending.display()
        );
    }
    std::fs::rename(&pending, &final_dir).with_context(|| {
        format!("renaming {} to {}", pending.display(), final_dir.display())
    })?;
    // After the rename, so a half-built directory is never briefly readable as
    // a segment, and before the log line, so "Built" means "built and usable".
    relax_permissions(&final_dir);

    let secs = t0.elapsed().as_secs_f64();
    info!(
        target: "rustock::segbuild",
        "Built {name}: {done} blocks, {verified} state roots verified, {segments} table(s), \
         {:.0} blk/s",
        done as f64 / secs.max(0.001)
    );

    Ok(Some(BuildOutcome {
        name,
        start,
        end,
        blocks: done,
        verified,
        segments,
        end_hash: end_hash.into(),
    }))
}

/// Attempts at one range before it is written off.
///
/// Three rather than one because the commonest failures are transient -- a disk
/// that was momentarily full, a store that was mid-collection -- and three
/// rather than for ever because a range that cannot be built must not starve
/// every later range (§3.1).
pub const MAX_ATTEMPTS: u32 = 3;

/// How the builder asks the store what it has already collected.
///
/// `Some(n)` means a node missing beneath block `n` is expected rather than
/// damage: the collector took it and this store cannot get it back. `None`
/// means no sweep has completed, so nothing has been collected yet. A store
/// with no collector at all supplies no callback and nothing is ever skipped.
pub type CollectedBelow = Arc<dyn Fn() -> Option<u64> + Send + Sync>;

/// What one tick of the builder did. Every variant is a normal outcome; none
/// is a reason for the caller to stop the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tick {
    /// Building is halted and will stay halted until a human clears it.
    Halted(String),
    /// Nothing is ready: no boundary is `lag` blocks settled yet.
    Idle,
    /// A segment was built and recorded.
    Built(BuildOutcome),
    /// A build was interrupted. The partial work is kept and resumes.
    Cancelled,
    /// A build failed. `uncovered` is set when it was the last attempt and the
    /// range has been written off.
    Failed { start: u64, end: u64, attempt: u32, uncovered: bool },
    /// The range was written off without being attempted, because the
    /// collector has already swept beneath its start. Replaying it would read
    /// nodes this store cannot return.
    Unbuildable { start: u64, end: u64, collected_below: u64 },
}

/// Drives the deferred builder: one range per tick, nothing concurrent.
///
/// The caller polls [`Self::tick`] from a background task. One range at a time
/// is deliberate -- two concurrent builds double the memory and compete for the
/// same store, and the measured ceiling on a 7.7 GB box was three workers at
/// 700 MB x 2 tables, with a fourth *slower* than three.
pub struct SegmentBuilder {
    boundaries: Arc<SegmentBoundaries>,
    blocks: Arc<BlockStore>,
    source: Arc<dyn TrieStore>,
    /// Built once and shared across builds. See [`build_range`] for what it
    /// must and must not be configured with.
    processor: Arc<BlockProcessor>,
    /// The collection watermark, when the source store has one.
    collected_below: Option<CollectedBelow>,
    config: BuildConfig,
    /// Blocks to wait after a boundary before building its range.
    lag: u64,
    /// First block of the ranges this builder is responsible for: where the
    /// set it is extending leaves off.
    first_block: u64,
}

impl SegmentBuilder {
    pub fn new(
        boundaries: Arc<SegmentBoundaries>,
        blocks: Arc<BlockStore>,
        source: Arc<dyn TrieStore>,
        processor: Arc<BlockProcessor>,
        collected_below: Option<CollectedBelow>,
        config: BuildConfig,
        lag: u64,
        first_block: u64,
    ) -> Self {
        Self {
            boundaries,
            blocks,
            source,
            processor,
            collected_below,
            config,
            lag,
            first_block,
        }
    }

    /// Do at most one range's worth of work.
    ///
    /// Returns rather than propagates every failure: extending the segment set
    /// is a by-product of syncing, and nothing here is a reason to stop
    /// following the chain.
    pub fn tick(&self, head: u64, cancel: &(dyn Fn() -> bool + Sync)) -> Tick {
        if let Some(reason) = self.boundaries.halted() {
            // Already halted. Said at `debug` because the caller is expected to
            // report the state at its own rate -- every 30 seconds for as long
            // as a human takes to respond would be thousands of identical
            // lines.
            debug!(target: "rustock::segbuild", "Still halted, building nothing: {reason}");
            return Tick::Halted(reason);
        }

        // Has the chain moved under a segment that already exists? Only a
        // reorg deeper than the lag can do that -- on mainnet, more than a
        // thousand blocks -- which is not an operational event to recover from.
        // `verify_built_still_canonical` halts durably if so.
        let blocks = self.blocks.clone();
        match self.boundaries.verify_built_still_canonical(|h| {
            blocks.canonical_hash(h).ok().flatten().map(|hash| hash.into())
        }) {
            Ok(Some(reason)) => {
                // The moment it happens, and the loudest line this module has.
                // A reorg deeper than the lag means settled history was
                // rewritten; the builder stops and stays stopped, and an
                // operator who never sees this has no way to know why the set
                // stopped growing.
                error!(
                    target: "rustock::segbuild",
                    "HALTING segment building: {reason} Nothing will be built until a human                      investigates and removes the HALTED file. The node is unaffected and                      keeps following the chain."
                );
                return Tick::Halted(reason);
            }
            Ok(None) => {}
            Err(e) => {
                warn!(
                    target: "rustock::segbuild",
                    "Could not check whether built segments are still canonical: {e:#}"
                );
                return Tick::Idle;
            }
        }

        let ready = self.boundaries.ready(head, self.lag, self.first_block);
        let Some(range) = ready.first().copied() else { return Tick::Idle };

        // Has the collector already been beneath this range? If so the replay
        // would read nodes the store cannot return, and the honest thing is to
        // say so now rather than find out after minutes of work -- or, worse,
        // three times over.
        //
        // The watermark is a recorded fact, not an inference from how far
        // behind the builder is: a node missing beneath it was taken by the
        // collector and is not recoverable here, while a node missing above it
        // is damage. Distance from the head would only say the range *lost its
        // protection*, which is not the same as having been collected -- the
        // range is still buildable until a sweep actually reaches it.
        if let Some(below) = self.collected_below.as_ref().and_then(|f| f()) {
            if below >= range.start {
                let reason = format!(
                    "the collector has swept beneath block {below}, at or above this range's                      start, so the nodes its blocks wrote are not recoverable from this store"
                );
                error!(
                    target: "rustock::segbuild",
                    "Writing off #{}..#{} without attempting it: {reason}. The segment set                      has a known hole over that range, recorded in UNCOVERED; it can be                      filled from an archive. Building continues with later ranges.",
                    range.start, range.end
                );
                if let Err(e) = self.boundaries.mark_uncovered(range.start, range.end, &reason) {
                    warn!(
                        target: "rustock::segbuild",
                        "Could not record #{}..#{} as uncovered: {e:#}",
                        range.start, range.end
                    );
                }
                return Tick::Unbuildable {
                    start: range.start,
                    end: range.end,
                    collected_below: below,
                };
            }
        }

        info!(
            target: "rustock::segbuild",
            "Building #{}..#{} ({} MB accumulated), {} range(s) pending",
            range.start, range.end, range.approx_bytes / (1 << 20), ready.len()
        );

        match build_range(
            self.blocks.clone(),
            self.source.clone(),
            self.processor.as_ref(),
            &self.config,
            range.start,
            range.end,
            cancel,
        ) {
            Ok(Some(outcome)) => {
                if let Err(e) =
                    self.boundaries.mark_built(outcome.start, outcome.end, outcome.end_hash)
                {
                    // The segment is on disk and correct; only the record of it
                    // failed. Saying so is all that can be done here -- the
                    // next tick will offer the range again, see the directory
                    // already there, and refuse rather than overwrite it.
                    warn!(
                        target: "rustock::segbuild",
                        "Built {} but could not record it: {e:#}", outcome.name
                    );
                }
                Tick::Built(outcome)
            }
            Ok(None) => Tick::Cancelled,
            Err(e) => {
                let attempt = self.boundaries.record_failure(range.start, range.end);
                let reason = format!("{e:#}");
                if attempt >= MAX_ATTEMPTS {
                    error!(
                        target: "rustock::segbuild",
                        "Giving up on #{}..#{} after {attempt} attempts: {reason}. The segment \
                         set now has a known hole over that range, recorded in UNCOVERED; it \
                         can be filled from an archive. Building continues with later ranges.",
                        range.start, range.end
                    );
                    if let Err(e) = self.boundaries.mark_uncovered(range.start, range.end, &reason)
                    {
                        warn!(
                            target: "rustock::segbuild",
                            "Could not record #{}..#{} as uncovered: {e:#}",
                            range.start, range.end
                        );
                    }
                    Tick::Failed { start: range.start, end: range.end, attempt, uncovered: true }
                } else {
                    warn!(
                        target: "rustock::segbuild",
                        "Build of #{}..#{} failed (attempt {attempt} of {MAX_ATTEMPTS}): \
                         {reason}",
                        range.start, range.end
                    );
                    Tick::Failed { start: range.start, end: range.end, attempt, uncovered: false }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_next_index_continues_past_the_highest_and_ignores_retired_names() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(next_index(d.path()).unwrap(), 0, "an empty set starts at 000");

        for name in ["000-1-100", "097-1-2", "908-5-6"] {
            std::fs::create_dir(d.path().join(name)).unwrap();
        }
        assert_eq!(next_index(d.path()).unwrap(), 909, "one above the highest");

        // A suffixed name is retired, and a build in progress is not a segment.
        // Neither may move the index -- an index that repeats stops indicating
        // order, and that is what the index is for.
        std::fs::create_dir(d.path().join("999-7-8.abandoned")).unwrap();
        std::fs::create_dir(d.path().join(".building-9-10")).unwrap();
        assert_eq!(next_index(d.path()).unwrap(), 909);
    }

    /// A segment nothing but the node can open is useless, and the failure is
    /// silent: the build verifies, the log says so, and a consumer gets a
    /// permission error that looks nothing like "this segment is fine".
    #[cfg(unix)]
    #[test]
    fn a_finished_segment_is_readable_by_a_consumer() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let seg = d.path().join("909-1-2");
        std::fs::create_dir_all(seg.join("sealed")).unwrap();
        std::fs::write(seg.join("sealed").join("000001.sst"), b"nodes").unwrap();

        // What `UMask=0077` leaves behind, which is what the mainnet node runs.
        for p in [seg.join("sealed").join("000001.sst"), seg.join("sealed"), seg.clone()] {
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
        }

        relax_permissions(&seg);

        let mode = |p: &std::path::Path| {
            std::fs::metadata(p).unwrap().permissions().mode() & 0o777
        };
        assert_eq!(mode(&seg), 0o755, "the segment directory must be enterable");
        assert_eq!(mode(&seg.join("sealed")), 0o755, "and so must its store");
        assert_eq!(
            mode(&seg.join("sealed").join("000001.sst")),
            0o644,
            "and its files readable"
        );
    }

    #[test]
    fn the_set_end_is_where_a_continuous_builder_picks_up() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(set_end(d.path()).unwrap(), None, "an empty set has no end");

        // The shapes the production set actually holds.
        for name in [
            "000-1-1591000.superseded-by-900-908-20261120",
            "012-2938787-3081559.abandoned-empty-superseded-20261120",
            "096-9229944-9230008",
            "097-9230009-9420000",
            ".building-9420001-9500000",
        ] {
            std::fs::create_dir(d.path().join(name)).unwrap();
        }
        assert_eq!(
            set_end(d.path()).unwrap(),
            Some(9_420_000),
            "a retired segment covers nothing, and a build in progress is not a segment"
        );
    }

    #[test]
    fn the_index_survives_a_missing_directory() {
        let d = tempfile::tempdir().unwrap();
        let absent = d.path().join("not-created-yet");
        assert_eq!(
            next_index(&absent).unwrap(),
            0,
            "the first build creates the set, so an absent root is not an error"
        );
    }

    #[test]
    fn a_pending_name_is_not_a_segment_name() {
        let p = pending_dir(Path::new("/x"), 10, 20);
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(
            parse_index(&name),
            None,
            "a build in progress must not parse as NNN-start-end"
        );
    }
}
