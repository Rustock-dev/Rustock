//! Rewrite a trie segment's SSTs with a bloom filter policy.
//!
//! `segbuild` wrote its segments with no filter policy at all (`lsm_stats`
//! reports `# entries for filter=0`), so every point lookup pays an index
//! probe and a data-block read per candidate file. A reader cannot add a
//! filter block afterwards -- only the writer can -- so the SSTs have to be
//! rewritten. A full compaction with the policy set does exactly that, since
//! compaction output uses the table options in force at the time.
//!
//! Usage: refilter_segment <dir> [bits-per-key]
//!
//! REFUSES to run against the live store or the sealed originals. Point it at
//! a copy.
use rocksdb::{
    BlockBasedOptions, BottommostLevelCompaction, Cache, ColumnFamilyDescriptor, CompactOptions,
    Options, DB,
};
use std::time::Instant;

const CF_TRIE: &str = "trie_nodes";

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).ok_or_else(|| {
        anyhow::anyhow!("usage: refilter_segment <dir> [bits-per-key]")
    })?;
    let bits: f64 = std::env::args().nth(2).unwrap_or_else(|| "10".into()).parse()?;

    // This opens read-write and rewrites every SST. Never let it near the
    // originals or the node's own store.
    let real = std::fs::canonicalize(&path)?;
    let s = real.to_string_lossy();
    for forbidden in ["/srv/segbuild-ro", "/var/lib/rustock", "/srv/rustock"] {
        anyhow::ensure!(
            !s.starts_with(forbidden),
            "refusing to rewrite {s}: it is under {forbidden}. Copy it first."
        );
    }

    let mut block = BlockBasedOptions::default();
    block.set_block_cache(&Cache::new_lru_cache(64 * 1024 * 1024));
    block.set_cache_index_and_filter_blocks(true);
    block.set_pin_l0_filter_and_index_blocks_in_cache(true);
    block.set_bloom_filter(bits, false); // full (not block-based) filter
    // The filter policy has to go on the COLUMN FAMILY's options, not just the
    // database's. `DB::open_cf` with bare names gives each CF its own default
    // options, so a table factory set only on the db-level `Options` never
    // reaches `trie_nodes` -- the first attempt compacted happily and produced
    // no filter at all.
    let mut cf_opts = Options::default();
    cf_opts.set_block_based_table_factory(&block);

    let mut db_opts = Options::default();
    db_opts.create_if_missing(false);
    db_opts.set_block_based_table_factory(&block);

    println!("rewriting {s} with a {bits}-bit/key bloom filter");
    let t = Instant::now();
    let db = DB::open_cf_descriptors(
        &db_opts,
        &real,
        vec![ColumnFamilyDescriptor::new(CF_TRIE, cf_opts)],
    )?;
    let cf = db.cf_handle(CF_TRIE).ok_or_else(|| anyhow::anyhow!("no {CF_TRIE} CF"))?;
    // Force the bottommost level too. Without this a store that is already
    // fully compacted has nothing RocksDB considers worth doing, so the SSTs
    // keep their filterless form.
    let mut copts = CompactOptions::default();
    copts.set_bottommost_level_compaction(BottommostLevelCompaction::Force);
    db.compact_range_cf_opt(cf, None::<&[u8]>, None::<&[u8]>, &copts);
    drop(db);
    println!("done in {:.1}s", t.elapsed().as_secs_f64());
    Ok(())
}
