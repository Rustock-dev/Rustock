//! Read an **rskj** unitrie directly, without importing it first.
//!
//! rskj keeps its flat unitrie in RocksDB's **default** column family, keyed by
//! node hash. Rustock keeps the same bytes in a `trie_nodes` column family.
//! The import between them is a straight key-to-value copy -- content-addressed
//! nodes, nothing transformed -- so the only real difference is which column
//! family the bytes live in.
//!
//! That makes an adapter cheap, and worth having: it lets a tool that wants an
//! archival trie use an rskj snapshot as-is. For testnet that is the difference
//! between parallel range builds and a single sequential pass from genesis,
//! because ranges are independent only when each can seed from an archival
//! pre-state.
//!
//! # The mistake this refuses to make
//!
//! Pointing a default-column-family reader at a rustock store finds nothing and
//! reports every hash absent -- which looks exactly like a pruned archive, and
//! has already been taken for a real result once (see `rskj_root_probe`). So
//! [`RskjTrieStore::open_read_only`] **refuses** a store that has a
//! `trie_nodes` column family, rather than opening it and answering `None` to
//! everything.
//!
//! Read-only: `put` is a no-op, because an rskj snapshot is a source, never a
//! destination.

use anyhow::{bail, Context, Result};
use rocksdb::{Options, DB};
use rustock_trie::TrieStore;
use std::path::{Path, PathBuf};

pub struct RskjTrieStore {
    db: DB,
}

impl RskjTrieStore {
    /// Open an rskj unitrie for random access.
    ///
    /// Accepts either the `unitrie` directory itself or the database directory
    /// containing it, because both are natural things to have a path to.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self> {
        let given = path.as_ref();
        let dir: PathBuf = if given.join("unitrie").join("CURRENT").exists() {
            given.join("unitrie")
        } else {
            given.to_path_buf()
        };
        if !dir.join("CURRENT").exists() {
            bail!("{} is not a RocksDB store (no CURRENT)", dir.display());
        }

        // Refuse a rustock-format store rather than answering None to every
        // hash. See the module note.
        let probe = Options::default();
        if let Ok(cfs) = DB::list_cf(&probe, &dir) {
            if cfs.iter().any(|c| c == "trie_nodes") {
                bail!(
                    "{} has a `trie_nodes` column family, so it is a rustock store, not an \
                     rskj one. Opening it here would find nothing in the default column \
                     family and report every node absent, which is indistinguishable from a \
                     pruned archive. Use RocksDbTrieStore for a rustock store.",
                    dir.display()
                );
            }
        }

        // Tuned for scattered point lookups, as `rskj_import::open_source_random`
        // is: keep every table open and give the index and filter blocks a real
        // cache, because a point lookup consults those first. The opposite
        // setting -- a bounded table cache, right for a sequential scan -- made
        // a comparable random workload run at ~83 reads/s.
        let mut opts = Options::default();
        opts.create_if_missing(false);
        opts.set_max_open_files(-1);
        let cache = rocksdb::Cache::new_lru_cache(512 * 1024 * 1024);
        let mut bb = rocksdb::BlockBasedOptions::default();
        bb.set_block_cache(&cache);
        bb.set_cache_index_and_filter_blocks(true);
        bb.set_pin_l0_filter_and_index_blocks_in_cache(true);
        opts.set_block_based_table_factory(&bb);

        let db = DB::open_for_read_only(&opts, &dir, false)
            .with_context(|| format!("opening rskj unitrie at {}", dir.display()))?;
        Ok(Self { db })
    }
}

impl TrieStore for RskjTrieStore {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.db.get(key).ok().flatten()
    }
    /// A no-op: an rskj snapshot is a source, never a destination.
    fn put(&self, _key: &[u8], _value: &[u8]) {}
    fn flush(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_node_from_the_default_column_family() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut o = Options::default();
            o.create_if_missing(true);
            let db = DB::open(&o, dir.path()).unwrap();
            db.put(b"\xaa\xbb", b"node-bytes").unwrap();
        }
        let s = RskjTrieStore::open_read_only(dir.path()).unwrap();
        assert_eq!(s.get(b"\xaa\xbb"), Some(b"node-bytes".to_vec()));
        assert_eq!(s.get(b"\x01\x02"), None);
    }

    /// The failure this type exists to prevent: a rustock store opened here
    /// would report every node absent, which reads as a pruned archive.
    #[test]
    fn refuses_a_rustock_store_instead_of_reporting_everything_absent() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut o = Options::default();
            o.create_if_missing(true);
            o.create_missing_column_families(true);
            let _ = DB::open_cf(&o, dir.path(), vec!["default", "trie_nodes"]).unwrap();
        }
        let e = match RskjTrieStore::open_read_only(dir.path()) {
            Ok(_) => panic!("a rustock store was accepted"),
            Err(e) => e.to_string(),
        };
        assert!(e.contains("trie_nodes"), "{e}");
        assert!(e.contains("rustock store"), "{e}");
    }

    #[test]
    fn a_directory_that_is_not_a_store_is_refused_clearly() {
        let dir = tempfile::tempdir().unwrap();
        let e = match RskjTrieStore::open_read_only(dir.path()) {
            Ok(_) => panic!("a non-store directory was accepted"),
            Err(e) => e.to_string(),
        };
        assert!(e.contains("not a RocksDB store"), "{e}");
    }
}

#[cfg(test)]
mod real_store_tests {
    use super::*;

    /// Against a real rskj snapshot, if one is pointed at.
    ///
    /// The unit tests above prove the plumbing on a synthetic store; this
    /// proves the format assumption on a real one, which is the part that
    /// would be embarrassing to get wrong.
    ///
    /// ```text
    /// RSKJ_UNITRIE=<dir> RSKJ_ROOT=<0xhash> \
    ///   cargo test -p rustock-storage --lib real_store -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "requires RSKJ_UNITRIE and RSKJ_ROOT"]
    fn a_real_rskj_snapshot_answers_a_real_state_root() {
        let (Ok(dir), Ok(root)) = (std::env::var("RSKJ_UNITRIE"), std::env::var("RSKJ_ROOT")) else {
            eprintln!("RSKJ_UNITRIE / RSKJ_ROOT not set; skipping");
            return;
        };
        let key: Vec<u8> = (0..root.trim_start_matches("0x").len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&root.trim_start_matches("0x")[i..i + 2], 16).unwrap())
            .collect();
        let s = RskjTrieStore::open_read_only(&dir).expect("open");
        let got = s.get(&key);
        println!("root {root} -> {:?} octets", got.as_ref().map(|v| v.len()));
        assert!(
            got.is_some(),
            "the snapshot does not hold that state root: either it is pruned, or this is \
             not the chain it belongs to"
        );
    }
}
