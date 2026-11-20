//! Build segbuild segments as a by-product of syncing.
//!
//! Sergio's design: while the node syncs normally, every node written goes to
//! both the garbage-collected trie and the open segment, and every node read
//! from the GC'd trie is written to the open segment too. When a segment
//! fills, it is sealed and the next one opened. Segbuild stops being a
//! separate replay job and becomes a by-product of running the node, so the
//! frontier never falls behind the head and nothing races the pruner.
//!
//! # The primary store stays authoritative
//!
//! Reads are answered by the primary and the answer is *copied* into the
//! window; writes go to the primary first. The capture is strictly additive,
//! so a defect in it cannot change what the node computes or persists -- it
//! can only make a segment less complete. That is the one property worth
//! insisting on when adding anything to a live node's trie path.
//!
//! # What this does and does not guarantee
//!
//! A segment ends up holding what the node **touched** over its range, which
//! is not the same as the state that range can **reach**. A node written long
//! before and still live, which nothing in this range re-reads, is not
//! captured -- that is exactly the gap that made #374 look like it broke
//! mainnet consensus (`docs/trie-segments-design.md` §6.9d, #397).
//!
//! What changes is the provenance. The holes in the existing segments exist
//! because they were sealed by code that never read the RSKIP379 pegout index;
//! a node building them live runs current code, which does. So the guarantee
//! is **"complete for the code that sealed it"**, and the residual risk
//! narrows to code newer than that node -- which is what #398's abort message
//! should say when it fires.
//!
//! Live-built segments are therefore a strong *regression* instrument and not
//! an independent check: a consensus bug in the node is encoded into the
//! segment, and later replays against it agree. `trie-segments-design.md` §8
//! makes the same point about recorded roots.
use crate::window_store::WindowStore;
use rustock_trie::TrieStore;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct SegbuildTee {
    /// The node's own trie. Authoritative for every answer.
    primary: Arc<dyn TrieStore>,
    /// Capture. Writes here are idempotent -- the window's newest table is a
    /// map -- so copying on every read costs a hash insert, not an I/O.
    window: Arc<WindowStore>,
    captured_on_read: AtomicU64,
    captured_on_write: AtomicU64,
}

impl SegbuildTee {
    pub fn new(primary: Arc<dyn TrieStore>, window: Arc<WindowStore>) -> Self {
        Self {
            primary,
            window,
            captured_on_read: AtomicU64::new(0),
            captured_on_write: AtomicU64::new(0),
        }
    }

    /// Tell the capture which block is executing, so a sealed segment can
    /// record the range it actually covers.
    pub fn set_block(&self, n: u64) {
        self.window.set_block(n);
    }

    pub fn stats(&self) -> (u64, u64, usize, u64) {
        (
            self.captured_on_read.load(Ordering::Relaxed),
            self.captured_on_write.load(Ordering::Relaxed),
            self.window.segment_count(),
            self.window.resident_bytes(),
        )
    }
}

impl TrieStore for SegbuildTee {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        let v = self.primary.get(key)?;
        self.window.put(key, &v);
        self.captured_on_read.fetch_add(1, Ordering::Relaxed);
        Some(v)
    }

    fn put(&self, key: &[u8], value: &[u8]) {
        // Primary first: if it fails the node must notice, and the capture
        // must never be ahead of what the node actually persisted.
        self.primary.put(key, value);
        self.window.put(key, value);
        self.captured_on_write.fetch_add(1, Ordering::Relaxed);
    }

    fn flush(&self) {
        self.primary.flush();
        self.window.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustock_trie::MemoryTrieStore;

    /// The property that makes this safe to put in a live node's trie path:
    /// every answer comes from the primary, and the capture cannot change one.
    #[test]
    fn the_primary_answers_and_the_capture_cannot_change_it() {
        let primary = Arc::new(MemoryTrieStore::new());
        primary.put(b"a", b"from-primary");
        let dir = tempfile::tempdir().unwrap();
        let window = Arc::new(
            WindowStore::new(
                crate::window_store::WindowConfig {
                    seal_bytes: 1 << 30,
                    tables: 2,
                    dir: dir.path().to_path_buf(),
                },
                None,
            )
            .unwrap(),
        );
        // Poison the capture with a different value for the same key. A
        // content-addressed store makes this impossible in practice; the point
        // is that even then the primary decides.
        window.put(b"a", b"from-window");

        let tee = SegbuildTee::new(primary.clone(), window.clone());
        assert_eq!(tee.get(b"a").as_deref(), Some(&b"from-primary"[..]));
        assert_eq!(tee.get(b"absent"), None, "absence comes from the primary too");
    }

    /// A read copies into the window, which is the whole mechanism: the
    /// segment accumulates what the node touched without a second lookup.
    #[test]
    fn a_read_is_captured_and_a_write_goes_to_both() {
        let primary = Arc::new(MemoryTrieStore::new());
        primary.put(b"read-me", b"v1");
        let dir = tempfile::tempdir().unwrap();
        let window = Arc::new(
            WindowStore::new(
                crate::window_store::WindowConfig {
                    seal_bytes: 1 << 30,
                    tables: 2,
                    dir: dir.path().to_path_buf(),
                },
                None,
            )
            .unwrap(),
        );
        let tee = SegbuildTee::new(primary.clone(), window.clone());

        assert!(window.get(b"read-me").is_none(), "not captured yet");
        let _ = tee.get(b"read-me");
        assert_eq!(window.get(b"read-me").as_deref(), Some(&b"v1"[..]), "read captured");

        tee.put(b"written", b"v2");
        assert_eq!(primary.get(b"written").as_deref(), Some(&b"v2"[..]), "primary got it");
        assert_eq!(window.get(b"written").as_deref(), Some(&b"v2"[..]), "capture got it");

        let (on_read, on_write, _, _) = tee.stats();
        assert_eq!((on_read, on_write), (1, 1));
    }
}
