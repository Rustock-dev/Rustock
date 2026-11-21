//! Build a real segment out of real blocks.
//!
//! `segment_build` replays a settled canonical range and seals the trie nodes
//! it reads. Every property that matters about it -- that the seed is found,
//! that each header's state root verifies, that the sealed segment is the
//! range's read set and nothing else, that a partial build is not visible as a
//! segment -- is a property of real chain data. A unit test over a synthetic
//! trie would check the plumbing and none of that.
//!
//! Ignored by default. It needs this node's own block store and a trie store
//! that covers the range:
//!
//! ```sh
//! RUSTOCK_BLOCKS_DB=/var/lib/rustock \
//! RUSTOCK_TRIE_DB=/mnt/import/rustock-trie \
//! TMPDIR=/mnt/import/paseo/tmp \
//!   cargo test --release -p rustock-execution --test segment_build_from_store \
//!     -- --ignored --nocapture
//! ```
//!
//! `RUSTOCK_SEGBUILD_START` and `RUSTOCK_SEGBUILD_BLOCKS` move and resize the
//! range; the defaults are a short recent one, because cost per block rises
//! tenfold across the chain's history and a recent range is the expensive case.

use rustock_execution::hardfork::RskHardforkConfig;
use rustock_execution::processor::BlockProcessor;
use rustock_execution::segment_build::{
    build_range, BuildConfig, CollectedBelow, SegmentBuilder, Tick,
};
use rustock_storage::segbuild_boundaries::SegmentBoundaries;
use rustock_storage::{BlockStore, RocksDbTrieStore};
use rustock_trie::TrieStore;
use std::sync::Arc;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn env_u64(name: &str, default: u64) -> u64 {
    env(name).and_then(|v| v.parse().ok()).unwrap_or(default)
}

struct Fixture {
    blocks: Arc<BlockStore>,
    trie: Arc<dyn TrieStore>,
    start: u64,
    end: u64,
}

/// `None` when the stores are not configured, so the test skips rather than
/// fails on a machine that has no chain.
fn fixture() -> Option<Fixture> {
    let (Some(blocks_db), Some(trie_db)) = (env("RUSTOCK_BLOCKS_DB"), env("RUSTOCK_TRIE_DB"))
    else {
        eprintln!("RUSTOCK_BLOCKS_DB and RUSTOCK_TRIE_DB not both set; skipping");
        return None;
    };
    let blocks = Arc::new(BlockStore::open_read_only(&blocks_db).expect("open block store"));
    let trie: Arc<dyn TrieStore> =
        Arc::new(RocksDbTrieStore::open_read_only(&trie_db).expect("open trie store"));
    let start = env_u64("RUSTOCK_SEGBUILD_START", 9_000_000);
    let blocks_n = env_u64("RUSTOCK_SEGBUILD_BLOCKS", 200);
    Some(Fixture { blocks, trie, start, end: start + blocks_n - 1 })
}

fn config(out_root: &std::path::Path) -> BuildConfig {
    BuildConfig {
        out_root: out_root.to_path_buf(),
        // Small enough that a short range still exercises sealing a table and
        // reading back through it, which is where the window's lookup order
        // matters.
        seal_bytes: 8 * 1024 * 1024,
        tables: 2,
    }
}

/// Mainnet rules, as the node runs them.
fn processor(blocks: Arc<BlockStore>) -> BlockProcessor {
    BlockProcessor::new(RskHardforkConfig::mainnet(), blocks)
}

#[test]
#[ignore = "requires a block store and a trie store (RUSTOCK_BLOCKS_DB, RUSTOCK_TRIE_DB)"]
fn a_range_builds_into_a_segment_that_verifies_every_block() {
    let Some(f) = fixture() else { return };
    let out = tempfile::tempdir().expect("tempdir");

    let outcome = build_range(
        f.blocks.clone(),
        f.trie.clone(),
        &processor(f.blocks.clone()),
        &config(out.path()),
        f.start,
        f.end,
        &|| false,
    )
    .expect("the build must succeed")
    .expect("an uncancelled build produces a segment");

    let expected = f.end - f.start + 1;
    assert_eq!(outcome.blocks, expected, "every block in the range was replayed");
    assert_eq!(
        outcome.verified, expected,
        "and every one of them had its state root checked against its header -- an \
         unverified block in a segment is state nothing has checked"
    );
    assert_eq!(outcome.name, format!("000-{}-{}", f.start, f.end));
    assert_eq!(
        outcome.end_hash,
        <[u8; 32]>::from(f.blocks.canonical_hash(f.end).unwrap().unwrap()),
        "the recorded hash must be the chain's, or a later reorg check is meaningless"
    );

    let dir = out.path().join(&outcome.name);
    assert!(dir.join("sealed").exists(), "the segment holds a sealed store");
    assert!(
        !out.path().join(format!(".building-{}-{}", f.start, f.end)).exists(),
        "the pending directory is renamed, not copied"
    );

    // The seed is the node the original build missed for 96 of 97 chunks: the
    // state at start-1, read straight from the fallback and so never copied
    // into the segment. Without it the segment cannot replay its own first
    // block, and nothing about the build itself notices.
    let parent_root = {
        let hash = f.blocks.canonical_hash(f.start).unwrap().unwrap();
        let header = f.blocks.header(hash).unwrap().unwrap();
        f.blocks.header(header.parent_hash).unwrap().unwrap().state_root
    };
    let sealed = RocksDbTrieStore::open_read_only(dir.join("sealed").to_str().unwrap())
        .expect("open the sealed segment");
    assert!(
        sealed.get(parent_root.as_slice()).is_some(),
        "the segment must contain the state root of #{} it seeded from",
        f.start - 1
    );

    // And the range's own roots are in it, which is what makes it the read set
    // of its range rather than a cache of whatever was touched.
    for n in [f.start, (f.start + f.end) / 2, f.end] {
        let hash = f.blocks.canonical_hash(n).unwrap().unwrap();
        let root = f.blocks.header(hash).unwrap().unwrap().state_root;
        assert!(
            sealed.get(root.as_slice()).is_some(),
            "the segment must contain the state root of #{n}"
        );
    }
}

#[test]
#[ignore = "requires a block store and a trie store (RUSTOCK_BLOCKS_DB, RUSTOCK_TRIE_DB)"]
fn a_cancelled_build_leaves_no_segment_and_resumes() {
    let Some(f) = fixture() else { return };
    let out = tempfile::tempdir().expect("tempdir");
    let cfg = config(out.path());

    // Stop after a few blocks.
    let seen = std::sync::atomic::AtomicU64::new(0);
    let stop_after = 5u64;
    let p = processor(f.blocks.clone());
    let outcome =
        build_range(f.blocks.clone(), f.trie.clone(), &p, &cfg, f.start, f.end, &|| {
            seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= stop_after
        })
    .expect("a cancelled build is not a failure");
    assert_eq!(outcome, None, "a cancelled build produces no segment");

    // Nothing a reader would take for a segment.
    let names: Vec<String> = std::fs::read_dir(out.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(
        names,
        vec![format!(".building-{}-{}", f.start, f.end)],
        "the partial build is kept, under a name that does not parse as a segment: {names:?}"
    );

    // Run it again to completion: it must produce the same segment as a build
    // that was never interrupted.
    let outcome =
        build_range(f.blocks.clone(), f.trie.clone(), &p, &cfg, f.start, f.end, &|| false)
            .expect("the resumed build must succeed")
            .expect("and must produce a segment");
    assert_eq!(outcome.name, format!("000-{}-{}", f.start, f.end));
    assert_eq!(
        outcome.verified,
        outcome.blocks,
        "every block the resumed build replayed was verified"
    );
}

#[test]
#[ignore = "requires a block store and a trie store (RUSTOCK_BLOCKS_DB, RUSTOCK_TRIE_DB)"]
fn the_pre_unitrie_era_is_refused_rather_than_built_unverified() {
    let Some(f) = fixture() else { return };
    let out = tempfile::tempdir().expect("tempdir");
    // Below #1,591,000 a header declares an Orchid root. Replaying such a range
    // against header roots would verify nothing, and a segment built out of
    // unchecked state is exactly what must not exist.
    let p = processor(f.blocks.clone());
    let err = build_range(
        f.blocks,
        f.trie,
        &p,
        &config(out.path()),
        1_000_000,
        1_000_100,
        &|| false,
    )
    .expect_err("the pre-fork era must be refused");
    let msg = format!("{err:#}");
    assert!(msg.contains("Orchid root"), "{msg}");
    assert_eq!(
        std::fs::read_dir(out.path()).unwrap().count(),
        0,
        "and nothing is created on the way to refusing"
    );
}

/// The whole loop, which the individual pieces do not cover: a boundary is cut
/// at the executed head, stays unbuilt until it is `lag` blocks settled, builds
/// into a recorded segment, and then stops holding the collector's floor.
///
/// This is the part a node actually runs. `build_range` can be right while the
/// driver never offers it a range, offers one twice, or leaves the floor held
/// after a successful build -- and the last of those would quietly stop the
/// collector for ever.
#[test]
#[ignore = "requires a block store and a trie store (RUSTOCK_BLOCKS_DB, RUSTOCK_TRIE_DB)"]
fn the_loop_cuts_a_boundary_builds_it_and_then_releases_the_floor() {
    let Some(f) = fixture() else { return };
    let out = tempfile::tempdir().expect("tempdir");
    let lag = 1_000u64;

    // Target one byte, so the first accounted write crosses it. Where to cut
    // is not what this test is about.
    let boundaries = Arc::new(SegmentBoundaries::open(out.path(), 1).expect("open the logs"));
    let builder = SegmentBuilder::new(
        boundaries.clone(),
        f.blocks.clone(),
        f.trie.clone(),
        Arc::new(processor(f.blocks.clone())),
        // No collector on the source store here, so nothing is ever skipped.
        None,
        config(out.path()),
        lag,
        f.start,
    );

    // Before any boundary: nothing to build, and the floor sits at the block
    // the first range will seed from.
    assert_eq!(builder.tick(f.start, &|| false), Tick::Idle);
    assert_eq!(
        boundaries.collection_bound(f.start),
        Some(f.start - 1),
        "the open range needs its seed kept from the start"
    );

    // A block's writes cross the target, and the cut lands on the executed
    // head -- which is how the node does it, so the boundary is always a block
    // that has been executed.
    boundaries.add_bytes(64);
    let boundary = boundaries
        .end_block(f.end)
        .expect("recording a boundary")
        .expect("the target was crossed");
    assert_eq!(boundary.block, f.end);

    // Inside the lag, the range is not offered: a reorg could still reach it.
    assert_eq!(
        builder.tick(f.end, &|| false),
        Tick::Idle,
        "a boundary that is not yet settled must not be built"
    );
    assert_eq!(boundaries.built_count(), 0);

    // Settled, it builds.
    let outcome = match builder.tick(f.end + lag, &|| false) {
        Tick::Built(o) => o,
        other => panic!("expected a build, got {other:?}"),
    };
    assert_eq!((outcome.start, outcome.end), (f.start, f.end));
    assert_eq!(outcome.name, format!("000-{}-{}", f.start, f.end));
    assert_eq!(outcome.verified, outcome.blocks, "every block was checked");
    assert!(out.path().join(&outcome.name).join("sealed").exists());

    // Recorded against the chain's hash, which is what makes a later deep
    // reorg detectable at all.
    let built = boundaries.built();
    assert_eq!(built.len(), 1);
    assert_eq!((built[0].start, built[0].end), (f.start, f.end));
    assert_eq!(
        built[0].end_hash,
        Some(<[u8; 32]>::from(f.blocks.canonical_hash(f.end).unwrap().unwrap()))
    );

    // The floor moves up to the next range's seed. If it did not, one built
    // segment would hold the collector down for ever.
    assert_eq!(
        boundaries.collection_bound(f.start),
        Some(f.end),
        "a built range must stop holding the collector's floor"
    );

    // And there is nothing left to do: a range is built once.
    assert_eq!(builder.tick(f.end + lag, &|| false), Tick::Idle);
    assert_eq!(boundaries.built_count(), 1);
    assert!(boundaries.halted().is_none(), "nothing here is a reason to halt");
    assert!(boundaries.uncovered().is_empty(), "and nothing was written off");
}

/// A range the collector has already swept beneath is written off without
/// being attempted.
///
/// The watermark is the store's record of what it took: a node missing beneath
/// it was collected and is not recoverable here. Replaying such a range would
/// read into that hole -- possibly after minutes of work, and three times over
/// before the attempt counter gave up. The honest move is to record the hole
/// now and go on to the ranges that can still be built.
#[test]
#[ignore = "requires a block store and a trie store (RUSTOCK_BLOCKS_DB, RUSTOCK_TRIE_DB)"]
fn a_range_the_collector_swept_beneath_is_written_off_unattempted() {
    let Some(f) = fixture() else { return };
    let out = tempfile::tempdir().expect("tempdir");
    let lag = 1_000u64;

    let boundaries = Arc::new(SegmentBoundaries::open(out.path(), 1).expect("open the logs"));
    // A watermark at the range's own start: the collector has been here.
    let watermark = f.start;
    let builder = SegmentBuilder::new(
        boundaries.clone(),
        f.blocks.clone(),
        f.trie.clone(),
        Arc::new(processor(f.blocks.clone())),
        Some(Arc::new(move || Some(watermark)) as CollectedBelow),
        config(out.path()),
        lag,
        f.start,
    );

    boundaries.add_bytes(64);
    boundaries.end_block(f.end).unwrap().expect("a boundary");

    let tick = builder.tick(f.end + lag, &|| false);
    assert_eq!(
        tick,
        Tick::Unbuildable { start: f.start, end: f.end, collected_below: watermark },
        "the range must be written off rather than attempted"
    );

    // Nothing was built, and nothing was even started: no pending directory,
    // because the replay never began.
    assert_eq!(boundaries.built_count(), 0);
    assert!(
        !out.path().join(format!(".building-{}-{}", f.start, f.end)).exists(),
        "a range that cannot be built must not be opened for building"
    );

    // The hole is recorded, with a reason an operator can act on.
    let uncovered = boundaries.uncovered();
    assert_eq!(uncovered.len(), 1);
    assert_eq!((uncovered[0].start, uncovered[0].end), (f.start, f.end));
    assert!(
        uncovered[0].reason.contains("not recoverable from this store"),
        "{}",
        uncovered[0].reason
    );

    // It stops holding the collector's floor -- holding it for a range that
    // will never be built is how the disk fills -- and it is not offered again.
    assert_eq!(boundaries.collection_bound(f.start), Some(f.end));
    assert_eq!(builder.tick(f.end + lag, &|| false), Tick::Idle);
    assert!(boundaries.halted().is_none(), "writing off a range is not a halt");
}

/// The same builder, with the watermark just below the range, builds it.
///
/// Without this the test above would pass for a builder that skipped
/// everything.
#[test]
#[ignore = "requires a block store and a trie store (RUSTOCK_BLOCKS_DB, RUSTOCK_TRIE_DB)"]
fn a_watermark_below_the_range_does_not_stop_it() {
    let Some(f) = fixture() else { return };
    let out = tempfile::tempdir().expect("tempdir");
    let lag = 1_000u64;

    let boundaries = Arc::new(SegmentBoundaries::open(out.path(), 1).expect("open the logs"));
    // One block below the start: everything this range's blocks wrote is still
    // above what the collector took.
    let watermark = f.start - 1;
    let builder = SegmentBuilder::new(
        boundaries.clone(),
        f.blocks.clone(),
        f.trie.clone(),
        Arc::new(processor(f.blocks.clone())),
        Some(Arc::new(move || Some(watermark)) as CollectedBelow),
        config(out.path()),
        lag,
        f.start,
    );

    boundaries.add_bytes(64);
    boundaries.end_block(f.end).unwrap().expect("a boundary");

    match builder.tick(f.end + lag, &|| false) {
        Tick::Built(o) => assert_eq!((o.start, o.end), (f.start, f.end)),
        other => panic!("a watermark below the range must not stop it: {other:?}"),
    }
    assert!(boundaries.uncovered().is_empty());
}
