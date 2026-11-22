//! Which branches of this chain's own behaviour were ever actually taken.
//!
//! Most rows here are deletion candidates. Every activation that *changes*
//! behaviour leaves a branch behind, guarded by a height test, and a branch
//! over a window that can never reopen can go -- but only if nothing in the
//! window ever took it, and that is a claim about history, not about the code.
//!
//! One row is not a deletion candidate at all. [`Legacy::HighBitAddressCall`]
//! is active at every height on every network, so it will never be removed;
//! its occupancy is worth knowing for a different reason, given with it.
//!
//! The expensive part of proving it is the replay. The counting is nearly free,
//! so a replay that answers one such question should answer all of them: one
//! relaxed atomic add where the branch is taken, and a line in the report.
//!
//! # What a zero means, and what it does not
//!
//! A zero here is evidence over **the range the run covered**, which is why the
//! report prints the block counts beside it. It is also only as good as the
//! instrument: a counter that cannot fire reports zero just as convincingly as
//! a branch nothing took.
//!
//! That is why [`Legacy::MigrationEndAge`] is in the first run. Its window is
//! **known occupied** -- using the ordinary end age inside it diverged a real
//! mainnet block -- so it is the positive control. If it reports zero, the
//! campaign has a bug and every other zero in the table is worthless. Measure a
//! row you know fires before trusting the rows you hope do not.
//!
//! # Removing a guard needs more than a zero
//!
//! Only a branch that makes the whole *block* invalid can be removed without
//! measurement, because no canonical block can contain one. Everything counted
//! here is the other kind: the branch yields a valid block with a different
//! result, so a client without the guard would compute a different state root
//! for a block that is on the chain. Those need this measurement, and a
//! removal still needs its own justification.

use std::sync::atomic::{AtomicU64, Ordering};

/// Declares the catalogue once, so the counter, its index and its description
/// cannot drift apart.
macro_rules! legacy_guards {
    ($($variant:ident => ($rule:expr, $what:expr, $window:expr $(,)?)),+ $(,)?) => {
        /// A legacy branch worth knowing the occupancy of.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(usize)]
        pub enum Legacy { $($variant),+ }

        /// What each counter is about, in `Legacy`'s order.
        pub const CATALOGUE: &[(&str, &str, &str)] = &[$(($rule, $what, $window)),+];

        static HITS: [AtomicU64; CATALOGUE.len()] =
            [const { AtomicU64::new(0) }; CATALOGUE.len()];
    };
}

legacy_guards! {
    // The positive control. Known occupied: see the module note.
    MigrationEndAge => (
        "BR-086",
        "funds-migration end age is 172,800 blocks, not 10,585",
        "mainnet #4,976,300..#5,468,000",
    ),
    PublicReceiveHeaders => (
        "BR-015",
        "receiveHeaders is public rather than federation-only",
        "mainnet #1,591,000..#3,614,800",
    ),
    InternalBridgeCallEndsCaller => (
        "BR-030",
        "an internal call to the bridge ends its caller, consuming its gas",
        "mainnet below #729,000",
    ),
    CreatedContractLosesBalance => (
        "EVM-033",
        "a contract created at an address holding a balance loses it",
        "mainnet below #3,614,800",
    ),
    // Not a deletion candidate: active at all heights, all networks.
    HighBitAddressCall => (
        "EVM-046",
        "a call whose address word has bits above its low 20 octets, and whose low 20 \
         octets name an active precompile, reaches the account rather than the precompile",
        "all heights, all networks -- permanent, measured for divergence not deletion",
    ),
    InternalCreateNonceZero => (
        "EVM-034",
        "an internally created contract's nonce is 0, not 1",
        "mainnet below #1,591,000",
    ),
    FreeBridgeTransaction => (
        "EVM-024",
        "a bridge transaction from a privileged sender executes free, gas limit may be 0",
        "mainnet below #370,000",
    ),
}

impl Legacy {
    /// Count one occurrence. One relaxed atomic add, on a branch that is
    /// already behind a height test.
    pub fn hit(self) {
        HITS[self as usize].fetch_add(1, Ordering::Relaxed);
    }

    pub fn count(self) -> u64 {
        HITS[self as usize].load(Ordering::Relaxed)
    }
}

/// Every counter, as `(rule, what, window, hits)`, in catalogue order.
pub fn census() -> Vec<(&'static str, &'static str, &'static str, u64)> {
    CATALOGUE
        .iter()
        .enumerate()
        .map(|(i, (rule, what, window))| (*rule, *what, *window, HITS[i].load(Ordering::Relaxed)))
        .collect()
}

/// Whether the positive control fired.
///
/// Its window is known occupied, so a run that covered it and reports zero has
/// a broken instrument, not a clean window. The caller prints this verdict
/// rather than leaving a reader to notice.
pub fn positive_control_fired() -> bool {
    Legacy::MigrationEndAge.count() > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_has_its_own_counter() {
        // The macro keeps the enum and the catalogue in step; this checks the
        // indexing, which is what would silently mis-attribute a hit.
        let before = census();
        Legacy::PublicReceiveHeaders.hit();
        let after = census();
        for (i, ((_, _, _, b), (_, _, _, a))) in before.iter().zip(after.iter()).enumerate() {
            let expected = u64::from(i == Legacy::PublicReceiveHeaders as usize);
            assert_eq!(a - b, expected, "counter {i} moved by {}", a - b);
        }
    }

    #[test]
    fn the_catalogue_describes_every_counter() {
        assert_eq!(CATALOGUE.len(), HITS.len());
        for (rule, what, window) in CATALOGUE {
            assert!(!rule.is_empty() && !what.is_empty() && !window.is_empty());
        }
    }
}
