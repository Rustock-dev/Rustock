//! A segment-backed trie reader that refuses to confuse a gap with an absence.
//!
//! `TrieStore::get` answers `Option`, and a segbuild reader cannot tell these
//! apart from the answer alone:
//!
//! - the key is **genuinely absent from the state** -- a legitimate answer the
//!   consensus rules branch on;
//! - the node is **absent from this store** -- the segment never captured it.
//!
//! Reading the second as the first is unsound, and silently so. It is also
//! *asymmetric*: only code that reads state the builder never read can hit it,
//! so it always looks like a regression in the newer code. That asymmetry is
//! what made a replay of #374 look like a mainnet consensus divergence
//! (`docs/trie-segments-design.md` 6.9d), and what made segment
//! `026-3792187-3815755` present as a `STATE ROOT MISMATCH` at #3,812,411 on
//! 2026-11-22 -- investigated for a day as a consensus bug before being
//! isolated as a missing node.
//!
//! # Why no marker in the segment is needed
//!
//! The distinction is already in the data. A trie walk reaches a store only
//! through `NodeRef::Hash(h)`, and a parent commits to its child's hash. So a
//! node named by a hash **exists in the real trie by construction**: if the
//! store does not have it, the store is incomplete. A genuine absence never
//! reaches the store at all -- it is `NodeRef::Empty`, decided in the parent.
//!
//! Issue #398 originally proposed recording negative reads in the segment
//! format to tell the two apart. That is unnecessary, and the cost would have
//! been a format change invalidating every segment already built.
//!
//! # Where this may and may not be used
//!
//! **Only on a replay path**, where every `get` is a committed reference -- a
//! node message, or a long value by its `value_hash`. It must never sit behind
//! a deliberate presence probe such as `snapshot.rs`'s
//! `if store.get(h).is_none()`, which expects misses and handles them; this
//! would turn that expectation into an abort.

use rustock_trie::TrieStore;

/// Wraps a segment store so a missing node stops the run instead of being
/// reported as an absence.
pub struct AbortOnMissingNode<S> {
    inner: S,
    /// Named in the abort, because "a node is missing" is not actionable and
    /// "a node is missing from segment 026-3792187-3815755" is.
    segment: String,
}

impl<S: TrieStore> AbortOnMissingNode<S> {
    pub fn new(inner: S, segment: impl Into<String>) -> Self {
        Self { inner, segment: segment.into() }
    }

    /// The abort text, separated so it can be asserted without aborting.
    ///
    /// It names **both** explanations deliberately. The intuitive reading of a
    /// missing node is "the code under test is wrong", and that is the one
    /// reading this message exists to prevent.
    pub fn message(segment: &str, key: &[u8]) -> String {
        format!(
            "ABORT: node {hash} is not present in segment {segment}\n\
             \n\
             This is NOT evidence of a consensus bug in the code under test.\n\
             Two explanations, and they are not distinguishable from here:\n\
             \n\
             \x20 (a) the code under test reads state this segment never captured,\n\
             \x20     because the code that BUILT the segment never read it --\n\
             \x20     an incomplete segment, not a wrong execution;\n\
             \x20 (b) the code that built this segment was itself wrong, and\n\
             \x20     the state it captured does not match the chain.\n\
             \n\
             A passing run of an older version is not a control here: older code\n\
             that never performs the read is immune by construction.\n\
             \n\
             Re-run with a complete store behind the segments (an archival trie\n\
             as a fallback) before concluding anything about consensus.",
            hash = key.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        )
    }
}

impl<S: TrieStore> TrieStore for AbortOnMissingNode<S> {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        match self.inner.get(key) {
            Some(v) => Some(v),
            // Panic rather than return None. The whole point is that `None`
            // here is a lie: the caller asked for a node by its committed hash.
            None => panic!("{}", Self::message(&self.segment, key)),
        }
    }
    fn put(&self, key: &[u8], value: &[u8]) {
        self.inner.put(key, value);
    }
    fn flush(&self) {
        self.inner.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store that holds exactly one key, so a miss is arranged rather than
    /// hoped for.
    struct OneKey(Vec<u8>, Vec<u8>);
    impl TrieStore for OneKey {
        fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
            (key == self.0.as_slice()).then(|| self.1.clone())
        }
        fn put(&self, _k: &[u8], _v: &[u8]) {}
        fn flush(&self) {}
    }

    #[test]
    fn a_present_node_is_returned_unchanged() {
        let s = AbortOnMissingNode::new(OneKey(vec![1, 2, 3], vec![9, 9]), "seg");
        assert_eq!(s.get(&[1, 2, 3]), Some(vec![9, 9]));
    }

    #[test]
    #[should_panic(expected = "is not present in segment seg")]
    fn a_missing_node_aborts_instead_of_answering_absent() {
        let s = AbortOnMissingNode::new(OneKey(vec![1, 2, 3], vec![9, 9]), "seg");
        let _ = s.get(&[4, 5, 6]);
    }

    /// The wrong reading is the intuitive one, so the message must say so.
    #[test]
    fn the_message_names_both_explanations_and_denies_the_obvious_one() {
        let m = AbortOnMissingNode::<OneKey>::message("026-3792187-3815755", &[0xab, 0xcd]);
        assert!(m.contains("NOT evidence of a consensus bug"));
        assert!(m.contains("incomplete segment, not a wrong execution"));
        assert!(m.contains("the code that built this segment was itself wrong"));
        assert!(m.contains("026-3792187-3815755"));
        assert!(m.contains("abcd"), "the node's hash must be in the message");
    }
}
