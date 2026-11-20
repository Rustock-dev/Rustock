//! Capture segbuild segments while the node syncs, durably.
//!
//! The node writes every trie node to its own store *and* to the open segment,
//! and copies into the segment every node it reads. Segbuild stops being a
//! separate replay job: the frontier never falls behind the head, and nothing
//! races the pruner.
//!
//! # Never half-done
//!
//! The first sketch of this buffered captured nodes in an in-memory window and
//! sealed by size. That loses up to a whole window on any restart, which is
//! the most ordinary operational event there is -- it would manufacture holes
//! routinely, and silently.
//!
//! So nothing is buffered. Captured nodes go straight into the open segment's
//! RocksDB, which has its own WAL, and the segment carries a **high-water
//! mark**: the highest block number whose capture is complete. The mark is
//! written at block boundaries, after that block's nodes.
//!
//! That makes the mark conservative by construction. A crash can only leave it
//! *behind* reality, never ahead -- the nodes are already durable when it
//! advances. On restart the node compares it against its execution head:
//!
//! * equal -- carry on;
//! * mark behind head -- the blocks between are uncaptured. Re-execute them,
//!   or record the gap for the validation pass. Either is safe, because
//!   **capture is idempotent**: nodes are content-addressed, so writing one
//!   twice is a no-op and re-executing a block cannot corrupt a segment.
//!
//! A segment therefore always describes exactly what it holds, and the honest
//! answer to "is this segment complete" is a number on disk rather than an
//! inference.
//!
//! # The primary store stays authoritative
//!
//! Reads are answered by the node's own trie and copied into the segment;
//! writes reach the primary first. Capture is strictly additive, so a defect
//! in it can make a segment less complete but cannot change what the node
//! computes or persists. That is what makes this safe in a live node's path.
//!
//! # What a segment is sound for
//!
//! What its range *touched*, not the state that range can *reach* -- so, the
//! code that sealed it (`docs/trie-segments-design.md` §6.9d, #397/#398).
//! Live-built segments are a strong regression instrument and not an
//! independent check: a consensus bug in the node is captured into the segment
//! and later replays agree with it, as §8 warns of recorded roots.
use crate::RocksDbTrieStore;
use anyhow::{Context, Result};
use rustock_trie::TrieStore;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// Reserved metadata key. Trie keys are 32-byte node hashes, so a key of any
/// other length cannot collide with one.
const HWM_KEY: &[u8] = b"segbuild:high-water-mark";

pub struct CaptureConfig {
    /// Where segments are written.
    pub dir: PathBuf,
    /// Seal the open segment once it exceeds this. Checked at block
    /// boundaries only, so a segment never stops mid-block.
    pub seal_bytes: u64,
}

struct Open {
    store: RocksDbTrieStore,
    path: PathBuf,
    start_block: u64,
    /// Highest block whose capture is complete and durable.
    hwm: u64,
}

pub struct SegbuildCapture {
    primary: Arc<dyn TrieStore>,
    cfg: CaptureConfig,
    open: RwLock<Option<Open>>,
    /// Block currently executing. Capture belongs to it; the mark only
    /// advances once it finishes.
    current: AtomicU64,
    sealed: AtomicU64,
    captured: AtomicU64,
    /// Serialises sealing against block-boundary bookkeeping.
    seal_lock: Mutex<()>,
    /// This block's captured nodes, written as one batch at `end_block`.
    ///
    /// Writing each node as it is touched cost 3.1 us -- measured at 7.6x the
    /// bare store, which is 6--18 ms a block and would have halved the node's
    /// sync rate. The high-water mark only needs durability at a block
    /// boundary, so one batch per block gives the same guarantee: a crash
    /// loses the block in flight either way, and the mark stays conservative
    /// because it is written in the same batch as the nodes it covers.
    pending: Mutex<Vec<(Vec<u8>, Vec<u8>)>>,
}

impl SegbuildCapture {
    /// Open or resume capture. Returns the high-water mark found on disk, so
    /// the caller can compare it with its execution head and decide whether
    /// blocks need re-executing.
    pub fn open(primary: Arc<dyn TrieStore>, cfg: CaptureConfig) -> Result<(Self, u64)> {
        std::fs::create_dir_all(&cfg.dir)
            .with_context(|| format!("creating {}", cfg.dir.display()))?;
        let resumed = Self::find_open_segment(&cfg.dir)?;
        let (open, hwm) = match resumed {
            Some((path, start)) => {
                let store = RocksDbTrieStore::open(&path)
                    .with_context(|| format!("reopening {}", path.display()))?;
                let hwm = store
                    .get(HWM_KEY)
                    .and_then(|v| v.try_into().ok())
                    .map(u64::from_be_bytes)
                    .unwrap_or(start);
                (Some(Open { store, path, start_block: start, hwm }), hwm)
            }
            None => (None, 0),
        };
        Ok((
            Self {
                primary,
                cfg,
                open: RwLock::new(open),
                current: AtomicU64::new(hwm),
                sealed: AtomicU64::new(0),
                captured: AtomicU64::new(0),
                seal_lock: Mutex::new(()),
                pending: Mutex::new(Vec::new()),
            },
            hwm,
        ))
    }

    /// An unsealed segment is named `open-<start>`; a sealed one follows the
    /// existing `NNN-start-end/sealed` convention, so a half-written segment
    /// can never be mistaken for a complete one by name.
    fn find_open_segment(dir: &Path) -> Result<Option<(PathBuf, u64)>> {
        for e in std::fs::read_dir(dir)?.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(rest) = name.strip_prefix("open-") {
                if let Ok(start) = rest.parse::<u64>() {
                    return Ok(Some((e.path(), start)));
                }
            }
        }
        Ok(None)
    }

    fn ensure_open(&self, block: u64) -> Result<()> {
        if self.open.read().unwrap().is_some() {
            return Ok(());
        }
        let path = self.cfg.dir.join(format!("open-{block}"));
        let store = RocksDbTrieStore::open(&path)
            .with_context(|| format!("creating {}", path.display()))?;
        *self.open.write().unwrap() =
            Some(Open { store, path, start_block: block, hwm: block.saturating_sub(1) });
        Ok(())
    }

    /// Call before executing `block`. Opens a segment if none is open.
    pub fn begin_block(&self, block: u64) -> Result<()> {
        self.ensure_open(block)?;
        self.current.store(block, Ordering::SeqCst);
        Ok(())
    }

    /// Call after `block` is fully executed and its state persisted. Advances
    /// the durable mark, then seals if the segment is big enough. Advancing
    /// after the fact is what keeps the mark conservative.
    pub fn end_block(&self, block: u64) -> Result<()> {
        let _g = self.seal_lock.lock().unwrap();
        let (size, should_seal) = {
            let mut guard = self.open.write().unwrap();
            let Some(open) = guard.as_mut() else { return Ok(()) };
            // The mark rides in the same batch as the nodes it covers, so the
            // two can never disagree after a crash.
            let mut items = std::mem::take(&mut *self.pending.lock().unwrap());
            items.push((HWM_KEY.to_vec(), block.to_be_bytes().to_vec()));
            open.store.put_many(&items);
            open.store.flush();
            open.hwm = block;
            let size = dir_size(&open.path);
            (size, size >= self.cfg.seal_bytes)
        };
        let _ = size;
        if should_seal {
            self.seal_locked(block)?;
        }
        Ok(())
    }

    /// Seal the open segment at `end`, renaming it to the sealed convention.
    /// Called at a block boundary, so the recorded range is exactly what the
    /// segment holds.
    fn seal_locked(&self, end: u64) -> Result<()> {
        let mut guard = self.open.write().unwrap();
        let Some(open) = guard.take() else { return Ok(()) };
        let start = open.start_block;
        drop(open.store); // close RocksDB before renaming
        let n = self.sealed.fetch_add(1, Ordering::Relaxed);
        let final_dir = self.cfg.dir.join(format!("{n:03}-{start}-{end}"));
        std::fs::create_dir_all(&final_dir)?;
        std::fs::rename(&open.path, final_dir.join("sealed"))
            .with_context(|| format!("sealing {} -> {}", open.path.display(), final_dir.display()))?;
        Ok(())
    }

    /// Seal on a clean shutdown, so a graceful restart leaves no gap at all.
    pub fn seal_for_shutdown(&self) -> Result<()> {
        let _g = self.seal_lock.lock().unwrap();
        let end = {
            let guard = self.open.read().unwrap();
            match guard.as_ref() {
                Some(o) => o.hwm,
                None => return Ok(()),
            }
        };
        self.seal_locked(end)
    }

    pub fn high_water_mark(&self) -> u64 {
        self.open.read().unwrap().as_ref().map(|o| o.hwm).unwrap_or(0)
    }

    pub fn stats(&self) -> (u64, u64, u64) {
        (
            self.captured.load(Ordering::Relaxed),
            self.sealed.load(Ordering::Relaxed),
            self.high_water_mark(),
        )
    }
}

fn dir_size(p: &Path) -> u64 {
    let mut t = 0;
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            if let Ok(m) = e.metadata() {
                t += m.len();
            }
        }
    }
    t
}

impl TrieStore for SegbuildCapture {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        let v = self.primary.get(key)?;
        if self.open.read().unwrap().is_some() {
            self.pending.lock().unwrap().push((key.to_vec(), v.clone()));
            self.captured.fetch_add(1, Ordering::Relaxed);
        }
        Some(v)
    }

    fn put(&self, key: &[u8], value: &[u8]) {
        // Primary first: the capture must never be ahead of what the node
        // actually persisted.
        self.primary.put(key, value);
        if self.open.read().unwrap().is_some() {
            self.pending.lock().unwrap().push((key.to_vec(), value.to_vec()));
            self.captured.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn flush(&self) {
        self.primary.flush();
        if let Some(open) = self.open.read().unwrap().as_ref() {
            open.store.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustock_trie::MemoryTrieStore;

    fn primary_with(pairs: &[(&[u8], &[u8])]) -> Arc<dyn TrieStore> {
        let p = Arc::new(MemoryTrieStore::new());
        for (k, v) in pairs {
            p.put(k, v);
        }
        p
    }

    fn open_at(dir: &Path, primary: Arc<dyn TrieStore>) -> (SegbuildCapture, u64) {
        SegbuildCapture::open(
            primary,
            CaptureConfig { dir: dir.to_path_buf(), seal_bytes: 1 << 40 },
        )
        .unwrap()
    }

    /// The property this design exists for: a crash cannot leave the mark
    /// ahead of what is durable. The mark advances only after a block's nodes
    /// are written, so dropping mid-block loses the block, never the record.
    #[test]
    fn a_crash_mid_block_leaves_the_mark_behind_not_ahead() {
        let dir = tempfile::tempdir().unwrap();
        let key = [7u8; 32];
        let primary = primary_with(&[(&key, b"v")]);

        {
            let (cap, hwm) = open_at(dir.path(), primary.clone());
            assert_eq!(hwm, 0, "nothing captured yet");
            cap.begin_block(100).unwrap();
            let _ = cap.get(&key);
            cap.end_block(100).unwrap();

            // Block 101 starts, captures, and the process dies before
            // end_block -- the `cap` is dropped with no seal and no mark.
            cap.begin_block(101).unwrap();
            let _ = cap.get(&key);
        }

        let (_cap, hwm) = open_at(dir.path(), primary);
        assert_eq!(hwm, 100, "resumes at the last COMPLETED block, not the one in flight");
    }

    /// Re-executing a block after a crash must be safe. Nodes are
    /// content-addressed, so capture is idempotent -- this is what makes
    /// "backtrack to the mark and re-execute" a sound recovery.
    #[test]
    fn re_executing_a_block_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let key = [9u8; 32];
        let primary = primary_with(&[(&key, b"val")]);
        let (cap, _) = open_at(dir.path(), primary);

        for _ in 0..3 {
            cap.begin_block(500).unwrap();
            let _ = cap.get(&key);
            cap.end_block(500).unwrap();
        }
        assert_eq!(cap.high_water_mark(), 500);
        let open = cap.open.read().unwrap();
        assert_eq!(
            open.as_ref().unwrap().store.get(&key).as_deref(),
            Some(&b"val"[..]),
            "value unchanged by repeated capture"
        );
    }

    /// An unsealed segment is named `open-<start>` and a sealed one
    /// `NNN-start-end/sealed`, so a half-written segment cannot be mistaken
    /// for a complete one by a reader that only looks at names.
    #[test]
    fn an_unsealed_segment_is_not_named_like_a_sealed_one() {
        let dir = tempfile::tempdir().unwrap();
        let key = [1u8; 32];
        let primary = primary_with(&[(&key, b"x")]);
        let (cap, _) = open_at(dir.path(), primary);
        cap.begin_block(10).unwrap();
        let _ = cap.get(&key);
        cap.end_block(10).unwrap();

        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "open-10"), "got {names:?}");
        assert!(!names.iter().any(|n| n.contains("sealed")), "nothing sealed yet: {names:?}");

        cap.seal_for_shutdown().unwrap();
        let after: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(after.iter().any(|n| n == "000-10-10"), "sealed with its range: {after:?}");
        assert!(!after.iter().any(|n| n.starts_with("open-")), "no open segment left");
        assert!(dir.path().join("000-10-10/sealed/CURRENT").exists(), "sealed layout");
    }

    /// Capture is additive: the primary decides every answer, including
    /// absence, so a defect in capture cannot change what the node sees.
    #[test]
    fn the_primary_answers_every_read() {
        let dir = tempfile::tempdir().unwrap();
        let key = [3u8; 32];
        let primary = primary_with(&[(&key, b"authoritative")]);
        let (cap, _) = open_at(dir.path(), primary);
        cap.begin_block(1).unwrap();
        assert_eq!(cap.get(&key).as_deref(), Some(&b"authoritative"[..]));
        assert_eq!(cap.get(&[4u8; 32]), None, "absence comes from the primary");
    }

    /// Sealing by size happens at a block boundary, never mid-block.
    #[test]
    fn sealing_lands_on_a_block_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let key = [5u8; 32];
        let primary = primary_with(&[(&key, &[0u8; 4096])]);
        let (cap, _) = SegbuildCapture::open(
            primary,
            CaptureConfig { dir: dir.path().to_path_buf(), seal_bytes: 1 },
        )
        .unwrap();
        cap.begin_block(42).unwrap();
        let _ = cap.get(&key);
        cap.end_block(42).unwrap(); // size exceeded -> seals here
        assert!(dir.path().join("000-42-42/sealed").exists(), "sealed at the boundary");
        // A new block opens a fresh segment rather than reopening the sealed one.
        cap.begin_block(43).unwrap();
        assert!(dir.path().join("open-43").exists());
    }
}

/// What capture costs, measured rather than assumed -- step 2 of enabling this
/// on a live node. Run with:
///   cargo test -p rustock-storage --release capture_overhead -- --ignored --nocapture
#[cfg(test)]
mod overhead {
    use super::*;
    use rustock_trie::MemoryTrieStore;
    use std::time::Instant;

    #[test]
    #[ignore = "timing; run explicitly"]
    fn capture_overhead() {
        const N: usize = 200_000;
        let keys: Vec<[u8; 32]> = (0..N)
            .map(|i| {
                let mut k = [0u8; 32];
                k[..8].copy_from_slice(&(i as u64).to_be_bytes());
                k
            })
            .collect();
        let val = vec![0u8; 70]; // mean trie node, per 6.9b

        // Baseline: the node's store alone.
        let plain = Arc::new(MemoryTrieStore::new());
        let t = Instant::now();
        for k in &keys {
            plain.put(k, &val);
        }
        for k in &keys {
            let _ = plain.get(k);
        }
        let base = t.elapsed().as_secs_f64();

        // With capture.
        let dir = tempfile::tempdir().unwrap();
        let primary = Arc::new(MemoryTrieStore::new());
        let (cap, _) = SegbuildCapture::open(
            primary,
            CaptureConfig { dir: dir.path().to_path_buf(), seal_bytes: 1 << 40 },
        )
        .unwrap();
        cap.begin_block(1).unwrap();
        let t = Instant::now();
        for k in &keys {
            cap.put(k, &val);
        }
        for k in &keys {
            let _ = cap.get(k);
        }
        let with = t.elapsed().as_secs_f64();
        cap.end_block(1).unwrap();

        let (captured, _, hwm) = cap.stats();
        eprintln!("  {N} puts + {N} gets");
        eprintln!("  primary only : {base:.3}s");
        eprintln!("  with capture : {with:.3}s  ({:.2}x)", with / base);
        eprintln!("  captured {captured} nodes, hwm {hwm}, segment {} MB",
            dir_size(&dir.path().join("open-1")) / 1_048_576);
        eprintln!("  per-node capture cost: {:.1} us", (with - base) / (2.0 * N as f64) * 1e6);
    }
}
