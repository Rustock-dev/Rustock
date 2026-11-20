//! Create an empty, valid RocksDB with only the default column family.
//!
//! Needed because a read-only RocksDB open requires the store to exist: there
//! must be a `CURRENT` and a manifest, and an empty directory has neither. So a
//! consumer that opens a database it never reads from still needs one to be
//! there.
//!
//! The case this was written for: replaying blocks in rskj against a segbuild
//! segment store. `TransactionExecutorFactory` takes the receipt store, so
//! rskj opens `<database>/receipts` whether or not anything reads it -- and
//! re-execution writes receipts, it does not consult them. Extracting the real
//! receipts database out of a 136 GiB snapshot to satisfy a handle nobody reads
//! would be absurd; an empty one does.
//!
//! **Only the default column family.** rskj's `RocksDbDataSource` opens with
//! the single-argument `RocksDB.openReadOnly`, which fails outright on a
//! database that has column families it was not told about. A store created by
//! rustock's own trie opener carries a `trie_nodes` family and is therefore
//! useless here, which is the one way to get this subtly wrong.
//!
//! Refuses a directory that already holds a database, so it cannot quietly
//! replace real data with nothing.
//!
//! Usage: make_empty_rocksdb <dir>

use anyhow::{bail, Context, Result};
use rocksdb::{Options, DB};
use std::path::Path;

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 2 {
        eprintln!("usage: make_empty_rocksdb <dir>");
        std::process::exit(64);
    }
    let dir = Path::new(&a[1]);

    if dir.join("CURRENT").exists() {
        bail!(
            "{} already holds a RocksDB (CURRENT exists). Refusing to touch it.",
            dir.display()
        );
    }
    if dir.exists() {
        let mut entries = std::fs::read_dir(dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .peekable();
        if entries.peek().is_some() {
            bail!(
                "{} exists and is not empty, but has no CURRENT. Clear it deliberately first.",
                dir.display()
            );
        }
    } else {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating {}", dir.display()))?;
    }

    let mut options = Options::default();
    options.create_if_missing(true);
    {
        let db = DB::open(&options, dir).with_context(|| format!("creating {}", dir.display()))?;
        // Nothing to write. Opening read-write is what lays down CURRENT and
        // the manifest; closing flushes them.
        drop(db);
    }

    // Confirm through the same path the consumer will take, rather than
    // trusting that the write worked.
    let probe = DB::open_for_read_only(&Options::default(), dir, false)
        .with_context(|| format!("re-opening {} read-only", dir.display()))?;
    let count = probe.iterator(rocksdb::IteratorMode::Start).count();
    drop(probe);
    if count != 0 {
        bail!("{} is not empty after creation ({count} entries)", dir.display());
    }

    println!("created an empty RocksDB at {}", dir.display());
    println!("  default column family only, 0 entries, opens read-only");
    Ok(())
}
