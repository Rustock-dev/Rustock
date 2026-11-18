//! rskj's `BridgeSupportTest` (`co/rsk/peg/BridgeSupportTest.java`): its
//! tests of the rules an upgrade turns on or off in `BridgeSupport`, of the
//! peg-ins it credits or refunds, of what `updateCollections` moves, of what
//! proves a Bitcoin transaction or coinbase to it, and of the chain work its
//! Bitcoin store takes.
//!
//! rskj's tests mock most of what `BridgeSupport` reads: the activations, a
//! rule at a time, the storage, the sender a peg-in is read to be from. Here
//! the Bridge reads its storage as it keeps it, the activations are an
//! upgrade ladder, and a sender is the input a peg-in spends. Where rskj
//! mocks on a rule that no ladder has on without others, the case runs on the
//! ladder up to the upgrade that brings it, and the test says so.

use bitcoin::Transaction;

use crate::bridge::{peg, storage};

use super::support::Ctx;

mod chain_work;
mod coinbase;
mod collections;
mod events;
mod locking_cap;
mod merkle_roots;
mod release_info;
mod sender_types;

/// The peg-outs one cell of the set waiting for confirmations holds: the
/// cell without the hash of the transaction that made them, or, from
/// RSKIP146, the one with it.
fn waiting_in(ctx: &mut Ctx, cell: &str, with_hash: bool) -> Vec<Transaction> {
    peg::deserialize_pegouts_waiting_for_confirmations(
        &storage::bridge_load_bytes_named(ctx, cell),
        with_hash,
    )
    .into_iter()
    .map(|entry| bitcoin::consensus::deserialize(&entry.btc_tx_raw).expect("a transaction"))
    .collect()
}
