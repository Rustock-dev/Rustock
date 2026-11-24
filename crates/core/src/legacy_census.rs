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

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Whether the counters record anything.
///
/// Off by default, so a node carries the call sites and none of the work: a
/// disabled `hit` is one relaxed load of a never-written flag, which predicts
/// perfectly and costs nothing measurable. A measurement run turns it on.
///
/// Runtime rather than a cargo feature on purpose. Features unify across a
/// workspace build, so an example that asked for `census` would silently switch
/// it on in the node binary built beside it -- the opposite of what a build-time
/// switch is for.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Start recording. Idempotent, and there is deliberately no way to stop:
/// a run that counted half of something would report it as a whole.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

/// Whether [`enable`] has been called. A report should say so -- all-zero
/// counters mean something very different when nothing was recording.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

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
    WindowedBridgeMethod => (
        "BR-011a",
        "a bridge method a later activation removed was invoked while it still existed \
         (addFederatorPublicKey, getFederatorPublicKey, getRetiringFederatorPublicKey, \
         getPendingFederatorPublicKey, addLockWhitelistAddress, getBtcBlockchainBlockLocator)",
        "mainnet below #729,000 or #1,591,000 depending on the method",
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
    // Not a deletion candidate either: live at every height. Measured because
    // only the *negative* half of the discontinuity needs proving, and a
    // prospective close would remove it.
    BlockHeaderDepthOutOfRange => (
        "PC-042",
        "a block-header precompile call supplied a depth outside 0..32,767: a negative one \
         throws and costs the caller all its gas, while one past the 16-bit range answers \
         empty, so the outcome is not monotonic in the argument",
        "all heights -- the negative half is what a prospective close would remove",
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
    // The second positive control, and a sharper one than BR-086: a single
    // named block with exact figures to match. notes/bounded-windows.md §7 is
    // explicit -- "a counter that reports zero for this one is broken".
    PrecompileTxGasExceedsLimit => (
        "EVM-023a",
        "a transaction sent straight to a precompile is tested against the precompile cost \
         alone but charged that plus the intrinsic cost, so gas used exceeds the gas limit \
         and the remainder goes negative (RSKIP-136; mainnet #3,307 used 31,280 of 30,000)",
        "mainnet below #3,397",
    ),
    EcrecoverDirtyVWord => (
        "PC-051",
        "ECRECOVER read octet 63 of the 32-octet v word and ignored the other 31, so a call \
         Ethereum refuses succeeded here",
        "all heights, all networks -- tightening needs an activation, not a retroactive close",
    ),
    UncleCountWrapsDifficulty => (
        "BV-125",
        "the uncle count entered the difficulty through 32-bit wrapping arithmetic and the \
         product actually wrapped, so the narrowed value differed from the wide one",
        "all heights -- a non-zero count is a live incident, not a historical curiosity",
    ),
    CommitFederationHandsOverImmediately => (
        "BR-125",
        "commitFederation rotated the federations immediately, with no SVP: the legacy \
         handover moved the active federation's UTXOs to the old set there and then",
        "mainnet below #7,338,024 (RSKIP-419)",
    ),
    AddSignatureTopicUsesBtcKey => (
        "BR-136",
        "addSignature's log topic carried the federator's BTC key rather than its RSK key",
        "mainnet below arrowhead600 (RSKIP-415); identical for legacy single-key members, \
         so a count is the legacy path being taken, not necessarily a differing topic",
    ),
    BtcHeaderRefusedForChainWork => (
        "BR-097",
        "the bridge refused a Bitcoin header because bitcoinj's 12-octet chain-work field \
         overflows at BTC mainnet #849,138 -- receiveHeader answers -99, receiveHeaders \
         stops its loop",
        "BTC #849,138 up to arrowhead631 #6,549,300; mainnet only, and on testnet the \
         guard never lifts",
    ),
    ExtcodehashByTrieExistence => (
        "EVM-043",
        "EXTCODEHASH answered from the trie-existence marker rather than the empty-account \
         test, and the two differed: zero where the standard rule gives the empty-code hash",
        "mainnet #2,392,700..#3,614,800",
    ),
    RejectedPeginMarkedProcessed => (
        "BR-055",
        "a rejected, unrefundable peg-in was marked processed -- a true window, opened by \
         RSKIP-459 and closed again by RSKIP-551",
        "mainnet #7,338,024..#8,804,200",
    ),
    GasLimitNarrowedToSignedLong => (
        "BV-110",
        "a header's gas limit held 2^63 or more, so reading it as a low signed long gave a \
         negative bound that no gas used satisfies -- the narrowed value differed from the \
         whole one",
        "all heights -- such a header is invalid, so a canonical replay should report zero",
    ),
    PegoutAmountSignedLong => (
        "BR-061",
        "a releaseBtc request's satoshi amount did not fit a signed 64-bit long, so the \
         low-64-bit signed read differed from the wide value (a negative long is below \
         every minimum and is refunded as it is)",
        "all heights -- unreachable on mainnet/testnet balances, reachable on regtest",
    ),
    FlyoverCodeSignExtended => (
        "BR-112",
        "a flyover response code was negative and so was encoded sign-extended across the \
         full 256 bits rather than at a narrower width",
        "from iris300 -- known occupied: a mainnet transaction depends on the full width",
    ),
    UncleSumWraps => (
        "BV-173",
        "the uncle-count sum over the 32 newest headers wrapped in 32-bit arithmetic, so \
         the narrowed sum differed from the wide one",
        "all heights -- the counts are the miners' to choose, so this is monitoring",
    ),
    StipendSumOverflows => (
        "EVM-062",
        "a call's requested gas was within the 2,300 stipend of 2^63-1, so the signed \
         64-bit sum went negative and the calling frame failed with all its gas",
        "mainnet below #2,018,000 (RSKIP-150)",
    ),
    StaticcallNotAnOpcode => (
        "EVM-041a",
        "STATICCALL was reached before RSKIP-91 made it an opcode, so the frame halted and \
         consumed all its gas",
        "mainnet below #729,000. NOTE: covers the STATICCALL half only -- the shift opcodes \
         and CREATE2 below wasabi100 come from the spec selection, not from a branch here, \
         so they need the reachability census instead",
    ),
    PrecompileThrowBurnsCallerGas => (
        "EVM-023",
        "a precompile's exception left the VM and failed the calling contract with all of \
         its gas, rather than being handled",
        "mainnet below #3,614,800 (RSKIP-197); known occupied on TESTNET at #808,484, so a \
         mainnet zero says nothing about the branch being dead",
    ),
    PrecompileOutputPastDeclaredRegion => (
        "EVM-100",
        "a successful precompile call wrote its whole output past the output region the \
         caller declared",
        "mainnet below #2,018,000 (RSKIP-150)",
    ),
    PrecompileCallKeepsReturnData => (
        "EVM-101",
        "a successful precompile call did not reset the caller's return-data buffer \
         (case 1 of three)",
        "mainnet below #729,000 (RSKIP-90)",
    ),
    EmptyCallKeepsReturnData => (
        "EVM-101",
        "a call that ran nothing did not reset the caller's return-data buffer \
         (case 2 of three)",
        "mainnet below #3,614,800 (RSKIP-171)",
    ),
    RefusedCreateKeepsReturnData => (
        "EVM-101",
        "a CREATE refused for depth, funds or an address collision left the caller's \
         return-data buffer holding an earlier call's output",
        "all heights -- RSKIP-171 cleared the call paths only, so this third case is live",
    ),
}

impl Legacy {
    /// Count one occurrence. One relaxed atomic add, on a branch that is
    /// already behind a height test.
    pub fn hit(self) {
        if !is_enabled() {
            return;
        }
        HITS[self as usize].fetch_add(1, Ordering::Relaxed);
    }

    pub fn count(self) -> u64 {
        HITS[self as usize].load(Ordering::Relaxed)
    }
}

/// Every counter's value, in catalogue order, for persisting mid-run.
///
/// A long replay must be stoppable and resumable, and a counter that lives
/// only in memory makes the resumed half report its own totals as the whole
/// chain's -- which would read as a clean window rather than as a partial run.
pub fn snapshot() -> Vec<u64> {
    HITS.iter().map(|h| h.load(Ordering::Relaxed)).collect()
}

/// Reload counters saved by [`snapshot`].
///
/// Ignores a snapshot of the wrong length: that means the catalogue changed
/// between runs, and carrying old numbers under new names would be worse than
/// starting the counters at zero. The caller is told so it can say so.
#[must_use]
pub fn restore(values: &[u64]) -> bool {
    if values.len() != CATALOGUE.len() {
        return false;
    }
    for (h, v) in HITS.iter().zip(values) {
        h.store(*v, Ordering::Relaxed);
    }
    true
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
    Legacy::MigrationEndAge.count() > 0 && Legacy::PrecompileTxGasExceedsLimit.count() > 0
}

/// The controls, named, so a report can say *which* one failed rather than
/// that "the campaign" did.
///
/// Two of them, deliberately. `BR-086` proves a bridge-side counter can fire;
/// `EVM-023a` is sharper, because it names one block and the exact figures it
/// must produce -- mainnet #3,307, 31,280 gas against a 30,000 limit. A run
/// that covered #3,307 and reports zero here has a broken instrument.
pub fn positive_controls() -> [(&'static str, u64); 2] {
    [
        ("BR-086", Legacy::MigrationEndAge.count()),
        ("EVM-023a", Legacy::PrecompileTxGasExceedsLimit.count()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_has_its_own_counter() {
        // The macro keeps the enum and the catalogue in step; this checks the
        // indexing, which is what would silently mis-attribute a hit.
        enable();
        let before = census();
        Legacy::PublicReceiveHeaders.hit();
        let after = census();
        for (i, ((_, _, _, b), (_, _, _, a))) in before.iter().zip(after.iter()).enumerate() {
            let expected = u64::from(i == Legacy::PublicReceiveHeaders as usize);
            assert_eq!(a - b, expected, "counter {i} moved by {}", a - b);
        }
    }

    /// The switch is the new way for a campaign to report nothing, so it gets
    /// its own test: a disabled counter must not move, and `is_enabled` must
    /// tell a report which case it is looking at.
    #[test]
    fn a_disabled_counter_records_nothing_and_says_so() {
        // Deliberately not calling `enable` first. Other tests in this binary
        // may already have, so assert the relationship rather than the state.
        if !is_enabled() {
            let before = Legacy::FreeBridgeTransaction.count();
            Legacy::FreeBridgeTransaction.hit();
            assert_eq!(
                Legacy::FreeBridgeTransaction.count(),
                before,
                "a disabled counter moved"
            );
        }
        enable();
        assert!(is_enabled());
        let before = Legacy::FreeBridgeTransaction.count();
        Legacy::FreeBridgeTransaction.hit();
        assert_eq!(Legacy::FreeBridgeTransaction.count(), before + 1);
    }

    #[test]
    fn the_catalogue_describes_every_counter() {
        assert_eq!(CATALOGUE.len(), HITS.len());
        for (rule, what, window) in CATALOGUE {
            assert!(!rule.is_empty() && !what.is_empty() && !window.is_empty());
        }
    }
}
