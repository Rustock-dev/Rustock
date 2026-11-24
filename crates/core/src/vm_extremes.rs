//! The largest thing the chain ever did, rather than how often it did it.
//!
//! The census next door ([`crate::legacy_census`]) answers "was this branch
//! ever taken". These answer "how far did it ever go" -- the deepest call
//! stack, the most EVM memory a transaction expanded, the largest `RETURN`
//! payload. Different question, so a different instrument: a maximum, not a
//! counter.
//!
//! # Why a packed `u64` rather than a value and a block side by side
//!
//! A maximum is worth little without the block that set it: "4,183 octets" is
//! a fact, "4,183 octets at #7,912,004" is something you can go and look at.
//! Two separate atomics would race -- one thread could win the value while
//! another overwrote the block -- and a mutex on a path that runs per frame is
//! too expensive.
//!
//! So each extreme is one `AtomicU64` holding `value << 32 | block`. A single
//! `fetch_max` then orders by value and carries the block with it, with no
//! lock and no torn pair. Where two blocks reach the same maximum the higher
//! one wins the tie, so the block reported is the **last** to reach it, not
//! the first -- which is what the report must say. Every quantity here fits comfortably in 32 bits: a
//! call depth is bounded by 1,024, a `RETURN` by the memory a block's gas can
//! buy, and the chain's height is under 2^24.
//!
//! Values at or above 2^32 are clamped rather than wrapped, so a hit on the
//! clamp reads as `4294967295` and is obviously a clamp rather than quietly
//! aliasing a small number.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Whether the high-water marks record anything. See
/// [`crate::legacy_census::enable`] for why this is runtime and not a feature.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Start recording.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

/// Whether [`enable`] has been called.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

thread_local! {
    /// The block each worker thread is replaying.
    ///
    /// Thread-local because a replay runs several segments at once, on
    /// different heights; a global would attribute one worker's extreme to
    /// another's block. The extremes themselves stay global -- the chain-wide
    /// maximum is the point.
    static CURRENT_BLOCK: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Called once per block by whatever is driving execution.
pub fn set_block(number: u64) {
    if !is_enabled() {
        return;
    }
    CURRENT_BLOCK.with(|b| b.set(number));
}

static MAX_CALL_DEPTH: AtomicU64 = AtomicU64::new(0);
static MAX_MEMORY_BYTES: AtomicU64 = AtomicU64::new(0);
static MAX_RETURN_BYTES: AtomicU64 = AtomicU64::new(0);

/// Returns whether this raised the maximum, so the caller can journal the
/// contract responsible -- which the packed value has no room for, and which
/// is the thing you actually want when a mark turns out to be interesting.
fn note(slot: &AtomicU64, value: u64) -> bool {
    if !is_enabled() {
        return false;
    }
    let clamped = value.min(u64::from(u32::MAX));
    let block = CURRENT_BLOCK.with(|b| b.get()).min(u64::from(u32::MAX));
    let packed = (clamped << 32) | block;
    let previous = slot.fetch_max(packed, Ordering::Relaxed);
    // Compare values, not packed words: a new block with an equal value raises
    // the packed word but is not a new maximum worth journalling.
    (previous >> 32) < clamped
}

fn read(slot: &AtomicU64) -> (u64, u64) {
    let packed = slot.load(Ordering::Relaxed);
    (packed >> 32, packed & 0xffff_ffff)
}

/// The depth of a frame a CALL-family opcode opened.
#[must_use]
pub fn note_call_depth(depth: u64) -> bool {
    note(&MAX_CALL_DEPTH, depth)
}

/// EVM memory expanded, in octets, sampled as a frame ends.
#[must_use]
pub fn note_memory_bytes(bytes: u64) -> bool {
    note(&MAX_MEMORY_BYTES, bytes)
}

/// The length of a `RETURN`'s payload, in octets.
#[must_use]
pub fn note_return_bytes(bytes: u64) -> bool {
    note(&MAX_RETURN_BYTES, bytes)
}

/// The three packed extremes, for persisting mid-run. See
/// [`crate::legacy_census::snapshot`] for why.
pub fn snapshot() -> [u64; 3] {
    [
        MAX_CALL_DEPTH.load(Ordering::Relaxed),
        MAX_MEMORY_BYTES.load(Ordering::Relaxed),
        MAX_RETURN_BYTES.load(Ordering::Relaxed),
    ]
}

/// Reload extremes saved by [`snapshot`].
///
/// `fetch_max` rather than `store`, so reloading can only raise a maximum --
/// a stale or duplicated snapshot cannot lower one that this run already set.
pub fn restore(values: &[u64; 3]) {
    MAX_CALL_DEPTH.fetch_max(values[0], Ordering::Relaxed);
    MAX_MEMORY_BYTES.fetch_max(values[1], Ordering::Relaxed);
    MAX_RETURN_BYTES.fetch_max(values[2], Ordering::Relaxed);
}

/// `(what, value, block)` for each extreme, for a report.
pub fn extremes() -> [(&'static str, u64, u64); 3] {
    let (d, db) = read(&MAX_CALL_DEPTH);
    let (m, mb) = read(&MAX_MEMORY_BYTES);
    let (r, rb) = read(&MAX_RETURN_BYTES);
    [
        ("deepest CALL-family frame", d, db),
        ("most EVM memory expanded (octets)", m, mb),
        ("largest RETURN payload (octets)", r, rb),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_maximum_keeps_the_block_that_set_it() {
        enable();
        let slot = AtomicU64::new(0);
        set_block(100);
        note(&slot, 7);
        set_block(200);
        note(&slot, 3); // smaller: must not displace the larger
        assert_eq!(read(&slot), (7, 100));
        set_block(300);
        note(&slot, 9);
        assert_eq!(read(&slot), (9, 300));
    }

    #[test]
    fn an_oversized_value_clamps_rather_than_wrapping() {
        enable();
        let slot = AtomicU64::new(0);
        set_block(1);
        note(&slot, u64::from(u32::MAX) + 5_000);
        assert_eq!(read(&slot).0, u64::from(u32::MAX));
    }
}
