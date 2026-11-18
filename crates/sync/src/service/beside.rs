//! Blocks beside the chain: meeting a fork the way rskj does.
//!
//! rskj executes every block it is given against its own parent's state as
//! it connects it, best or not (`BlockChainImpl.internalTryToConnect`), and
//! keeps the result. The chain switching to another branch is then a matter
//! of moving a pointer: nothing runs again and nothing waits. And a block it
//! has not connected cannot become its best, so a heavier chain known by its
//! headers alone leaves it where it is.
//!
//! rustock executed only the canonical block at each height. A competing
//! block was kept for a miner to name as an uncle, and the chain switching to
//! it pulled execution back to the fork point to run the new branch there:
//! on the next tick, after a grace period, once its bodies had been fetched.
//! Meanwhile the miner built on the fork point and every query was answered
//! from it, and a branch heavy in headers whose bodies were withheld held the
//! node there.
//!
//! Now every block whose body is held and whose parent this node executed is
//! executed as it arrives, beside the chain, and the store adopts a heavier
//! branch only once this node has executed it past its executed head
//! ([`rustock_storage::BlockStore::deferred_head`]), taking execution
//! straight to the branch's executed tip. This module executes those blocks,
//! fetches what a waiting branch lacks, and takes up a switch the moment the
//! store makes it.

use super::{
    create_body_request, pick_body_peer, SyncService, BODY_PEER_STRIKE_LIMIT, FOLLOW_BODY_TIMEOUT,
    MAX_BODY_REQUESTS_PER_PEER, SYNC_MAX_DISTANCE,
};
use crate::state::SyncState;
use alloy_primitives::{B256, B512};
use rustock_core::types::header::Header;
use rustock_core::types::transaction::Transaction;
use rustock_core::validation::uncles::UNCLE_GENERATION_LIMIT;
use rustock_core::Block;
use rustock_execution::executor::ExecutionError;
use rustock_execution::silent::SilentRecord;
use rustock_execution::{BlockProcessor, ProcessError};
use rustock_storage::BlockStore;
use rustock_trie::{TrieNode, TrieStore};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

#[cfg(test)]
mod tests;

/// The peg decisions the Bridge logs nothing for, made by blocks executed
/// beside the chain, with each block's height. They are indexed by height, so
/// they wait here until a path takes the block onto the chain -- a switch,
/// the follow path or a batch -- and that path takes them out as it indexes
/// the block. Shared, because a batch runs on a thread of its own.
pub(crate) type Unlogged =
    Arc<std::sync::Mutex<std::collections::HashMap<B256, (u64, Vec<SilentRecord>)>>>;

/// Take out the peg decisions `hash` made beside the chain, if it made any.
pub(crate) fn take_unlogged(unlogged: &Unlogged, hash: B256) -> Vec<SilentRecord> {
    unlogged
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&hash)
        .map(|(_, records)| records)
        .unwrap_or_default()
}

/// Executes one block against the state it is given -- its own parent's --
/// and commits what it keys by hash, without moving the executed head.
/// Returns the peg decisions the Bridge logs nothing for, which are indexed
/// by height and so wait until the block is on the chain. Production runs the
/// block processor ([`SyncService::wire_beside`]); tests substitute a
/// stand-in.
pub(crate) type BesideFn = Arc<
    dyn Fn(&Block, &TrieNode) -> Result<Vec<SilentRecord>, BesideFailure> + Send + Sync + 'static,
>;

/// Why a block did not execute beside the chain.
#[derive(Debug, Clone)]
pub(crate) enum BesideFailure {
    /// It breaks a rule of consensus, so no branch through it is adopted, as
    /// rskj connects no block that fails.
    Invalid(String),
    /// Something failed on this node -- its database, not the block -- and
    /// trying again later can succeed.
    Local(String),
}

/// What executing one block beside the chain came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Beside {
    /// Executed now, or by this node before.
    Executed,
    /// Not yet: its body, its parent's execution or its parent's state is not
    /// here, or this node failed to run it.
    NotYet,
    /// It breaks a rule of consensus, and is not tried again.
    Invalid,
}

/// The part of a branch above where it leaves the chain.
enum BranchTo {
    /// Its blocks, oldest first.
    Blocks(Vec<(B256, Header)>),
    /// A header on the way down is not held: the one with this hash, at this
    /// height.
    Missing(B256, u64),
    /// It leaves the chain further down than [`MAX_BRANCH_DEPTH`].
    TooDeep,
}

/// Blocks executed beside the chain in one call, so that a long branch is
/// worked through a tick at a time rather than holding the event loop.
const BESIDE_PER_CALL: usize = 64;

/// Body requests out at once for blocks beside the chain.
const MAX_BRANCH_BODY_REQUESTS: usize = 32;

/// Blocks remembered as invalid, so that none is executed twice.
const MAX_INVALID_REMEMBERED: usize = 4_096;

/// How far below the executed head a block beside the chain keeps the peg
/// decisions it made waiting to be indexed: past any reorg the node follows.
const UNLOGGED_KEPT_DEPTH: u64 = 1_000;

/// How far below its tip a branch is followed down to the chain: four sync
/// rounds, more than the chain moves while a node is away for days.
const MAX_BRANCH_DEPTH: u64 = 4 * SYNC_MAX_DISTANCE;

/// Headers asked for at once below a block beside the chain whose ancestry
/// is not held: as many as a peer serves.
const ANCESTORS_ASKED: u64 = 192;

/// Blocks beside the chain remembered with the peer their last body request
/// lapsed on, so that each is asked of another peer.
const MAX_LAPSED_REMEMBERED: usize = 4 * MAX_BRANCH_BODY_REQUESTS;

/// How long the headers asked for below a branch are remembered, with the
/// peer asked, so that a request that lapses is asked of another: a few
/// lapses.
const BRANCH_ASKED_KEPT: Duration = FOLLOW_BODY_TIMEOUT.saturating_mul(4);

/// How long the branch the store holds back may go with no body of it
/// arriving and none of it executing before it is given up. A heavier chain
/// whose bodies are withheld holds the node in follow mode this long, not for
/// good, and is held back again only if its headers come again.
const DEFERRED_STALL_TIMEOUT: Duration = Duration::from_secs(60);

impl SyncService {
    /// Execute blocks beside the chain with `processor`, and install on the
    /// store the check that says which blocks this node has executed, so that
    /// the store holds a heavier branch back until it is executed.
    pub(super) fn wire_beside(&mut self, processor: Arc<BlockProcessor>, trie: Arc<dyn TrieStore>) {
        let (check_processor, check_trie) = (processor.clone(), trie.clone());
        self.manager
            .store
            .set_executed_check(Arc::new(move |hash, header: &Header| {
                // The two questions `fast_forward_state` asks: the receipts,
                // hashed against the header, say this node executed the block --
                // verified, not believed -- and the trie says its state is still
                // there.
                check_processor.was_executed_locally(hash)
                    && check_trie.get(header.state_root.as_slice()).is_some()
            }));
        self.beside_fn = Some(Arc::new(move |block: &Block, parent_state: &TrieNode| {
            processor.validate_block(block).map_err(|e| {
                BesideFailure::Invalid(format!("it fails consensus validation: {e}"))
            })?;
            let result = processor
                .process_beside(block, parent_state, trie.clone())
                .map_err(|e| match e {
                    ProcessError::Storage(_)
                    | ProcessError::Execution(ExecutionError::Database(_)) => {
                        BesideFailure::Local(e.to_string())
                    }
                    _ => BesideFailure::Invalid(e.to_string()),
                })?;
            // Durable before anything relies on it, as a block executed on the
            // chain is before the executed head moves (`execution_commit`): the
            // store may adopt this block's branch on the strength of it.
            trie.flush();
            Ok(result.unlogged_peg_events)
        }));
    }

    /// Where a block this node mined and kept beside the chain leaves the peg
    /// decisions it made that the Bridge logs no event for
    /// (`StoreChainAccess::leave_unlogged`): the place a peer's block
    /// executed beside the chain leaves its own ([`Self::execute_beside`]).
    /// They wait there, by the block's hash, until a path takes the block
    /// onto the chain and stores them under its height.
    pub fn unlogged_holder(&self) -> Arc<dyn Fn(B256, u64, Vec<SilentRecord>) + Send + Sync> {
        let unlogged = self.beside_unlogged.clone();
        Arc::new(move |hash, number, records| {
            if !records.is_empty() {
                unlogged
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(hash, (number, records));
            }
        })
    }

    /// Execute beside the chain with `beside` in place of the block
    /// processor, and count a block executed here once `beside` has run it,
    /// or once the test has put it in the set returned.
    #[cfg(test)]
    pub(crate) fn set_test_beside(
        &mut self,
        beside: impl Fn(&Block) -> Result<(), BesideFailure> + Send + Sync + 'static,
    ) -> Arc<std::sync::Mutex<std::collections::HashSet<B256>>> {
        let executed = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        let (seen, ran) = (executed.clone(), executed.clone());
        self.manager
            .store
            .set_executed_check(Arc::new(move |hash, _: &Header| {
                seen.lock().unwrap().contains(&hash)
            }));
        self.beside_fn = Some(Arc::new(move |block: &Block, _: &TrieNode| {
            beside(block)?;
            ran.lock().unwrap().insert(block.hash());
            Ok(Vec::new())
        }));
        executed
    }

    /// Whether a child of `hash` can be executed against its state: this node
    /// executed it and holds its state, or it is on the chain at or below the
    /// executed head, where execution put the state -- a block a snapshot sync
    /// started from has no receipts to show for it.
    fn has_executed_state(&self, hash: B256, header: &Header) -> bool {
        let store = &self.manager.store;
        if store.executed_here(hash, header) {
            return true;
        }
        let executed_number = store
            .exec_head()
            .ok()
            .flatten()
            .and_then(|(h, _)| store.header(h).ok().flatten())
            .map(|h| h.number);
        executed_number.is_some_and(|n| header.number <= n)
            && store.canonical_hash(header.number).ok().flatten() == Some(hash)
            && self
                .trie_store
                .as_ref()
                .is_some_and(|t| t.get(header.state_root.as_slice()).is_some())
    }

    /// Execute `hash` beside the chain, as rskj connects every block it is
    /// given: against its own parent's state, recording its receipts and
    /// transactions by hash, and leaving the executed head where it is.
    ///
    /// The block's total difficulty is settled first, its uncles being in
    /// hand, as rskj's is when it connects the block. A block that breaks a
    /// rule of consensus is remembered and never adopted, as rskj never
    /// connects one; one this node failed to run is tried again later.
    pub(crate) fn execute_beside(&mut self, hash: B256) -> Beside {
        if self.invalid_beside.contains(&hash) {
            return Beside::Invalid;
        }
        let (Some(beside), Some(trie)) = (self.beside_fn.clone(), self.trie_store.clone()) else {
            return Beside::NotYet;
        };
        let store = self.manager.store.clone();
        let Ok(Some(header)) = store.header(hash) else {
            return Beside::NotYet;
        };
        if store.executed_here(hash, &header) {
            return Beside::Executed;
        }
        // A header that holds only the commitment to its RSKIP-351 extension
        // can't be checked or run: its bloom and execution edges come beside
        // its body. A body held beside such a header came without them, so it
        // is dropped and fetched again with them, as a block of the chain's is.
        // Checked without its edges, a block is refused that rskj takes:
        // testnet's #8,295,561 and #8,295,940, both of rskj's chain, were
        // remembered as invalid on 14 November 2026.
        if header.extension_data.is_some() {
            self.drop_body_without_extension(hash, &header);
            return Beside::NotYet;
        }
        let Ok(Some((transactions, ommers))) = store.body(hash) else {
            return Beside::NotYet;
        };
        let Ok(Some(parent)) = store.header(header.parent_hash) else {
            return Beside::NotYet;
        };
        if !self.has_executed_state(header.parent_hash, &parent) {
            return Beside::NotYet;
        }
        let Some(data) = trie.get(parent.state_root.as_slice()) else {
            return Beside::NotYet;
        };
        let parent_state = TrieNode::from_message(&data, trie.as_ref());
        // The store can hold bytes under a hash they do not hash to, and
        // executing against those is the consensus error that executing
        // against the parent's own state exists to rule out.
        if parent_state.compute_hash(trie.as_ref()) != parent.state_root {
            warn!(
                target: "rustock::sync",
                "Not executing #{} beside the chain: the trie store's entry for its parent's \
                 state {:?} does not hash to it",
                header.number, parent.state_root
            );
            return Beside::NotYet;
        }
        self.manager.settle_total_difficulty(hash, &header, &ommers);
        let block = Block {
            header,
            transactions,
            ommers,
        };
        match beside(&block, &parent_state) {
            Ok(unlogged) => {
                if !unlogged.is_empty() {
                    self.beside_unlogged
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(hash, (block.header.number, unlogged));
                }
                info!(
                    target: "rustock::sync",
                    "Executed block #{} ({hash:?}) beside the chain, against its parent's state",
                    block.header.number
                );
                Beside::Executed
            }
            Err(BesideFailure::Local(why)) => {
                warn!(
                    target: "rustock::sync",
                    "Could not execute block #{} ({hash:?}) beside the chain; it will be tried \
                     again: {why}",
                    block.header.number
                );
                Beside::NotYet
            }
            Err(BesideFailure::Invalid(why)) => {
                warn!(
                    target: "rustock::sync",
                    "Block #{} ({hash:?}) is invalid and will not be adopted: {why}",
                    block.header.number
                );
                if self.invalid_beside.len() >= MAX_INVALID_REMEMBERED {
                    self.invalid_beside.clear();
                }
                self.invalid_beside.insert(hash);
                Beside::Invalid
            }
        }
    }

    /// Execute `hash` beside the chain and then the held blocks beside the
    /// chain that were waiting on it, oldest first, as rskj connects a block's
    /// descendants once it has connected the block
    /// (`BlockSyncService.connectBlocksAndDescendants`). Returns those executed.
    fn execute_beside_with_descendants(&mut self, hash: B256) -> Vec<B256> {
        let mut executed = Vec::new();
        let mut queue = VecDeque::from([hash]);
        while let Some(next) = queue.pop_front() {
            if executed.len() >= BESIDE_PER_CALL {
                break;
            }
            if self.execute_beside(next) != Beside::Executed {
                continue;
            }
            executed.push(next);
            let store = &self.manager.store;
            let Ok(Some(header)) = store.header(next) else {
                continue;
            };
            let height = header.number + 1;
            for child in store.hashes_at_height(height).unwrap_or_default() {
                let waiting = store
                    .header(child)
                    .ok()
                    .flatten()
                    .is_some_and(|c| c.parent_hash == next)
                    && matches!(store.body(child), Ok(Some(_)))
                    && store.canonical_hash(height).ok().flatten() != Some(child);
                if waiting && !queue.contains(&child) {
                    queue.push_back(child);
                }
            }
        }
        executed
    }

    /// Weigh blocks just executed beside the chain against the head, highest
    /// first: a total their uncles raised can outweigh it now, and a branch
    /// that waited for them to be executed can be adopted. The store makes
    /// any switch; [`Self::take_up_moved_execution`] takes it up.
    fn weigh_after_executing(&mut self, executed: &[B256]) {
        let store = self.manager.store.clone();
        for hash in executed.iter().rev() {
            if let Err(e) = store.reconsider_head(*hash) {
                warn!(target: "rustock::sync", "Weighing {hash:?} against the head failed: {e:?}");
            }
        }
        if let Err(e) = store.retry_deferred_head() {
            warn!(target: "rustock::sync", "Weighing the waiting branch again failed: {e:?}");
        }
    }

    /// A block beside the chain whose body is here: execute it and what waits
    /// on it, weigh them against the head, and take up a switch if the store
    /// makes one.
    pub(crate) async fn execute_beside_chain(&mut self, hash: B256) {
        // A batch running writes the trie and the executed head; the tick
        // comes back to this.
        if self.beside_fn.is_none() || self.executing.is_some() {
            return;
        }
        let before = self.manager.store.switch_mark();
        let executed = self.execute_beside_with_descendants(hash);
        self.weigh_after_executing(&executed);
        self.follow_on(before).await;
        self.advance_deferred_branch().await;
    }

    /// Take up a switch the store made since `before`, a mark of
    /// [`BlockStore::switch_mark`], and, following the tip, run at once the
    /// blocks of the chain just above the executed head whose bodies are
    /// here, rather than on the next tick: a block executed beside the chain
    /// that the chain has since taken up, on top of the executed head, is
    /// adopted there without running again (`fast_forward_state`).
    async fn follow_on(&mut self, before: u64) {
        self.take_up_moved_execution(before);
        if matches!(self.state, SyncState::Following) {
            self.refill_follow_buffer_from_store();
            self.drain_follow_buffer().await;
        }
    }

    /// The branch ending at `tip`, from where it leaves the chain.
    fn branch_to(&self, tip: B256) -> BranchTo {
        let store = &self.manager.store;
        let mut branch: Vec<(B256, Header)> = Vec::new();
        let mut hash = tip;
        for _ in 0..MAX_BRANCH_DEPTH {
            let Ok(Some(header)) = store.header(hash) else {
                return match branch.last() {
                    Some((_, child)) => BranchTo::Missing(hash, child.number.saturating_sub(1)),
                    // The tip itself is not held: nothing to work on.
                    None => BranchTo::Blocks(Vec::new()),
                };
            };
            if store.canonical_hash(header.number).ok().flatten() == Some(hash) {
                branch.reverse();
                return BranchTo::Blocks(branch);
            }
            let parent = header.parent_hash;
            branch.push((hash, header));
            hash = parent;
        }
        BranchTo::TooDeep
    }

    /// Work on the branch ending at `tip`: execute what of it is held, oldest
    /// first, fetch the bodies it lacks, or the headers below it this node
    /// lacks. Returns the blocks executed, and `None` when one of its blocks
    /// is invalid, so that it can never be adopted.
    async fn work_on_branch(&mut self, tip: B256) -> Option<Vec<B256>> {
        let branch = match self.branch_to(tip) {
            BranchTo::Blocks(branch) => branch,
            BranchTo::Missing(hash, number) => {
                self.request_branch_ancestors(tip, hash, number).await;
                return Some(Vec::new());
            }
            BranchTo::TooDeep => {
                debug!(
                    target: "rustock::sync",
                    "The branch to {tip:?} leaves the chain more than {MAX_BRANCH_DEPTH} blocks \
                     below it; not following it"
                );
                return Some(Vec::new());
            }
        };
        if let Some((hash, header)) = branch.iter().find(|(h, _)| self.invalid_beside.contains(h)) {
            debug!(
                target: "rustock::sync",
                "The branch to {tip:?} holds block #{} ({hash:?}), which is invalid; it will not \
                 be adopted",
                header.number
            );
            return None;
        }
        // A block is executed beside the chain only once its parent has been,
        // so the blocks of a branch this node executed come first.
        let store = self.manager.store.clone();
        let start = branch.partition_point(|(hash, header)| store.executed_here(*hash, header));
        let mut executed = Vec::new();
        let mut missing = Vec::new();
        for (i, (hash, _)) in branch.iter().enumerate().skip(start) {
            if executed.len() >= BESIDE_PER_CALL {
                break;
            }
            if matches!(store.body(*hash), Ok(Some(_))) {
                match self.execute_beside(*hash) {
                    Beside::Executed => {
                        executed.push(*hash);
                        continue;
                    }
                    Beside::Invalid => return None,
                    // Its body was dropped, held beside a header without its
                    // extension: fetched again below, with it.
                    Beside::NotYet if !matches!(store.body(*hash), Ok(Some(_))) => {}
                    Beside::NotYet => break,
                }
            }
            // Its body is not here, nor perhaps those after it: fetch them.
            missing.extend(
                branch[i..]
                    .iter()
                    .filter(|(h, _)| !matches!(store.body(*h), Ok(Some(_))))
                    .take(MAX_BRANCH_BODY_REQUESTS)
                    .cloned(),
            );
            break;
        }
        self.request_branch_bodies(missing).await;
        Some(executed)
    }

    /// Work on the branch the store holds back until this node has executed
    /// it: execute what of it is here, fetch what is not, and let the store
    /// weigh it again. Run on every tick, and after any block beside the chain
    /// is executed. Given up when it makes no progress for
    /// [`DEFERRED_STALL_TIMEOUT`].
    pub(crate) async fn advance_deferred_branch(&mut self) {
        if self.beside_fn.is_none() || self.executing.is_some() {
            return;
        }
        let Some(tip) = self.manager.store.deferred_head() else {
            self.deferred_watch = None;
            return;
        };
        if self
            .deferred_watch
            .is_none_or(|(watched, _)| watched != tip)
        {
            self.deferred_watch = Some((tip, Instant::now()));
        }
        let before = self.manager.store.switch_mark();
        match self.work_on_branch(tip).await {
            Some(executed) => {
                if !executed.is_empty() {
                    self.deferred_watch = Some((tip, Instant::now()));
                }
                self.weigh_after_executing(&executed);
            }
            None => self.manager.store.clear_deferred_head(),
        }
        if let Some((watched, since)) = self.deferred_watch {
            if self.manager.store.deferred_head() == Some(watched)
                && since.elapsed() >= DEFERRED_STALL_TIMEOUT
            {
                warn!(
                    target: "rustock::sync",
                    "Giving up on the heavier branch to {watched:?}: none of its bodies came and \
                     none of it executed for {}s. The chain stays where it is; the branch is \
                     weighed again if its headers come again",
                    DEFERRED_STALL_TIMEOUT.as_secs()
                );
                self.manager.store.clear_deferred_head();
                self.deferred_watch = None;
            }
        }
        self.follow_on(before).await;
    }

    /// Blocks stored beside the chain near enough the executed head to compete
    /// with it, known perhaps by their headers alone: rskj receives blocks
    /// whole and connects them as they arrive, and a header says nothing of
    /// the uncles that may make its branch the heavier. Execute those that
    /// can be, fetch the bodies of the rest, and weigh them. `from` is the
    /// peer that served them, if one did, and is asked first.
    pub(crate) async fn take_up_competing(&mut self, hashes: Vec<B256>, from: Option<B512>) {
        if self.beside_fn.is_none() || self.executing.is_some() {
            return;
        }
        let store = self.manager.store.clone();
        let Some(executed_number) = store
            .exec_head()
            .ok()
            .flatten()
            .and_then(|(h, _)| store.header(h).ok().flatten())
            .map(|h| h.number)
        else {
            return;
        };
        let tips: Vec<B256> = hashes
            .into_iter()
            .filter(|hash| {
                !self.invalid_beside.contains(hash)
                    && store.header(*hash).ok().flatten().is_some_and(|header| {
                        header.number + UNCLE_GENERATION_LIMIT >= executed_number
                            && store.canonical_hash(header.number).ok().flatten() != Some(*hash)
                            && !store.executed_here(*hash, &header)
                    })
            })
            .collect();
        if tips.is_empty() {
            return;
        }
        if let Some(peer) = from {
            for tip in &tips {
                self.manager.note_block_holder(*tip, peer);
            }
        }
        let before = store.switch_mark();
        let mut executed = Vec::new();
        for tip in tips {
            if let Some(done) = self.work_on_branch(tip).await {
                executed.extend(done);
            }
        }
        self.weigh_after_executing(&executed);
        self.follow_on(before).await;
    }

    /// Blocks stored just above the executed head that do not build on it:
    /// the sign that the chain may have moved to another branch. Upstream
    /// adopted them for being there; here they are weighed by executing them
    /// ([`Self::take_up_competing`]).
    pub(crate) async fn take_up_stored_successors(&mut self) {
        if self.beside_fn.is_none() {
            return;
        }
        let store = self.manager.store.clone();
        let Some((exec_hash, _)) = store.exec_head().ok().flatten() else {
            return;
        };
        let Some(executed) = store.header(exec_hash).ok().flatten() else {
            return;
        };
        let successors: Vec<B256> = store
            .hashes_at_height(executed.number + 1)
            .unwrap_or_default()
            .into_iter()
            .filter(|h| {
                store
                    .header(*h)
                    .ok()
                    .flatten()
                    .is_some_and(|c| c.parent_hash != exec_hash)
            })
            .collect();
        if !successors.is_empty() {
            self.take_up_competing(successors, None).await;
        }
    }

    /// Ask for the bodies of blocks beside the chain: each of a peer known to
    /// hold it, as rskj asks the peer that sent a block for the blocks it
    /// lacks, or else of another, and never again of the peer a request for it
    /// lapsed on while another can be asked ([`Self::branch_peer`]).
    async fn request_branch_bodies(&mut self, blocks: Vec<(B256, Header)>) {
        if self.spv || blocks.is_empty() {
            return;
        }
        let peers = self.peer_store.peers().await;
        let mut load = self.branch_load(&peers);
        for (hash, header) in blocks {
            if self.pending_branch_bodies.len() >= MAX_BRANCH_BODY_REQUESTS {
                break;
            }
            if self
                .pending_branch_bodies
                .values()
                .any(|(h, ..)| *h == hash)
                || matches!(self.manager.store.body(hash), Ok(Some(_)))
            {
                continue;
            }
            let lapsed = self.branch_body_lapsed.get(&hash).copied();
            let Some(peer) = self.branch_peer(&load, &self.manager.block_holders(hash), lapsed)
            else {
                break;
            };
            let (request_id, message) = create_body_request(hash);
            self.peer_store.send_to_peer(&peer, message).await;
            self.pending_branch_bodies
                .insert(request_id, (hash, header, Instant::now(), peer));
            *load.entry(peer).or_default() += 1;
        }
    }

    /// Ask for the headers below a block beside the chain whose ancestry this
    /// node does not hold, ending at the first one missing, as rskj asks the
    /// peer that sent a block for its missing parent: of a peer known to hold
    /// the branch or else of another, and of another peer than the last if
    /// that one never answered ([`Self::branch_peer`]). Once they link, the
    /// branch is executed beside the chain like any other.
    async fn request_branch_ancestors(&mut self, tip: B256, missing: B256, number: u64) {
        if self.spv {
            return;
        }
        self.branch_headers_asked
            .retain(|_, (asked, _)| asked.elapsed() < BRANCH_ASKED_KEPT);
        let lapsed = match self.branch_headers_asked.get(&missing) {
            Some((asked, _)) if asked.elapsed() < FOLLOW_BODY_TIMEOUT => return,
            Some((_, peer)) => Some(*peer),
            None => None,
        };
        let peers = self.peer_store.peers().await;
        let load = self.branch_load(&peers);
        let Some(peer) = self.branch_peer(&load, &self.manager.block_holders(tip), lapsed) else {
            return;
        };
        let count = ANCESTORS_ASKED.min(number.max(1)) as u32;
        debug!(
            target: "rustock::sync",
            "The branch to {tip:?} lacks #{number} ({missing:?}); asking for {count} header(s) \
             ending there"
        );
        self.request_headers_from(&peer, missing, count).await;
        self.branch_headers_asked
            .insert(missing, (Instant::now(), peer));
    }

    /// Drop body requests for blocks beside the chain that went unanswered.
    /// The peer takes a strike, as one whose request the body pipeline
    /// reclaims does, and the next tick asks another peer for the block.
    pub(crate) fn expire_stale_branch_bodies(&mut self) {
        let now = Instant::now();
        let lapsed: Vec<(B256, B512)> = self
            .pending_branch_bodies
            .values()
            .filter(|(_, _, sent, _)| now.duration_since(*sent) >= FOLLOW_BODY_TIMEOUT)
            .map(|(hash, _, _, peer)| (*hash, *peer))
            .collect();
        self.pending_branch_bodies
            .retain(|_, (_, _, sent, _)| now.duration_since(*sent) < FOLLOW_BODY_TIMEOUT);
        if !lapsed.is_empty() {
            warn!(
                target: "rustock::sync",
                "Expired {} body request(s) for blocks beside the chain with no response; \
                 they will be asked of other peers",
                lapsed.len()
            );
        }
        for (hash, peer) in lapsed {
            *self.body_peer_strikes.entry(peer).or_default() += 1;
            self.note_branch_body_lapsed(hash, peer);
        }
        if let Some(number) = self
            .manager
            .store
            .exec_head()
            .ok()
            .flatten()
            .and_then(|(h, _)| self.manager.store.header(h).ok().flatten())
            .map(|h| h.number)
        {
            self.beside_unlogged
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|_, (block, _)| block.saturating_add(UNLOGGED_KEPT_DEPTH) >= number);
        }
    }

    /// Remember that `peer` did not send the body of `hash`, beside the chain,
    /// so that the block is asked of another peer next.
    fn note_branch_body_lapsed(&mut self, hash: B256, peer: B512) {
        if self.branch_body_lapsed.len() >= MAX_LAPSED_REMEMBERED
            && !self.branch_body_lapsed.contains_key(&hash)
        {
            self.branch_body_lapsed.clear();
        }
        self.branch_body_lapsed.insert(hash, peer);
    }

    /// The requests out for blocks beside the chain, by connected peer.
    fn branch_load(&self, peers: &[B512]) -> HashMap<B512, usize> {
        let mut load: HashMap<B512, usize> = peers.iter().map(|peer| (*peer, 0)).collect();
        for (.., peer) in self.pending_branch_bodies.values() {
            if let Some(count) = load.get_mut(peer) {
                *count += 1;
            }
        }
        load
    }

    /// The peer to ask for something of a branch beside the chain: the
    /// least-loaded of `holders` that is connected, still answers and is not
    /// `lapsed`, the peer whose last request for it went unanswered; else the
    /// least-loaded peer the body pipeline would pick (`pick_body_peer`), not
    /// `lapsed` while another can be asked. It used to be a holder or else the
    /// first peer connected, every time, and a peer is known to hold a block
    /// only once it has announced it or sent it whole: on testnet on
    /// 15 November 2026 the network's branch from #8,296,515 came by header
    /// sync, and a first peer that never answered held the node off the chain
    /// for over an hour.
    fn branch_peer(
        &self,
        load: &HashMap<B512, usize>,
        holders: &HashSet<B512>,
        lapsed: Option<B512>,
    ) -> Option<B512> {
        load.iter()
            .filter(|(peer, count)| {
                holders.contains(*peer)
                    && Some(**peer) != lapsed
                    && **count < MAX_BODY_REQUESTS_PER_PEER
                    && self.body_peer_strikes.get(*peer).copied().unwrap_or(0)
                        < BODY_PEER_STRIKE_LIMIT
            })
            .min_by_key(|(_, count)| **count)
            .map(|(peer, _)| *peer)
            .or_else(|| pick_body_peer(load, &self.body_peer_strikes, lapsed))
    }

    /// A body asked for a block beside the chain. Returns `false` when
    /// `request_id` is not one of those requests.
    pub(crate) async fn on_branch_body(
        &mut self,
        peer: B512,
        request_id: u64,
        transactions: Vec<Transaction>,
        uncles: Vec<Header>,
        extension: Option<rustock_core::types::header_extension::BlockHeaderExtension>,
    ) -> bool {
        let Some((hash, header, sent, asked)) = self.pending_branch_bodies.remove(&request_id)
        else {
            return false;
        };
        // Taken only from the peer asked, as every body is.
        if asked != peer {
            self.pending_branch_bodies
                .insert(request_id, (hash, header, sent, asked));
            return true;
        }
        let checked = match crate::body_check::body_mismatch(&header, &transactions, &uncles) {
            Some(what) => Err(what),
            None => crate::body_check::completed_header(&header, extension.as_ref()),
        };
        let header = match checked {
            Ok(None) => header,
            Ok(Some(completed)) => {
                if let Err(e) = self.manager.store.put_header_with_hash(hash, &completed) {
                    error!(
                        target: "rustock::sync",
                        "Failed to store the header of #{} with its extension: {e:?}",
                        completed.number
                    );
                    return true;
                }
                completed
            }
            Err(what) => {
                // Asked again, of another peer, on the next tick.
                self.charge_bad_body(peer, header.number, what);
                self.note_branch_body_lapsed(hash, peer);
                return true;
            }
        };
        if let Err(e) = self.manager.store.put_body(hash, &transactions, &uncles) {
            error!(target: "rustock::sync", "Failed to store the body of {hash:?}: {e:?}");
            return true;
        }
        self.manager.note_block_holder(hash, peer);
        // It answers, so its strikes go, as in the body pipeline.
        self.body_peer_strikes.remove(&peer);
        self.branch_body_lapsed.remove(&hash);
        // A body of a branch held back counts as progress on it.
        if let Some((_, since)) = self.deferred_watch.as_mut() {
            *since = Instant::now();
        }
        if self
            .manager
            .store
            .canonical_hash(header.number)
            .ok()
            .flatten()
            == Some(hash)
        {
            // On the chain by now, which makes it the follow path's to execute.
            if matches!(self.state, SyncState::Following) {
                self.follow_buffer
                    .insert(header.number, (hash, header, transactions, uncles));
                self.drain_follow_buffer().await;
            }
        } else {
            self.execute_beside_chain(hash).await;
        }
        true
    }

    /// Take up the switches the store made since `mark`
    /// ([`BlockStore::switch_mark`]): the executed head moved to a block of
    /// another branch, executed already, with nothing run again.
    ///
    /// Only a switch the store made counts, taken from the executed head it
    /// found. The executed head moves too when execution records its progress,
    /// and a batch does that on a thread of its own while this call runs.
    /// Taking that for a switch dropped what was queued, fetched again bodies
    /// already held, and announced blocks the batch had run: a node syncing
    /// mainnet from block 0 halted every few thousand blocks on it.
    ///
    /// What executing those blocks one by one would have done is done here:
    /// the in-memory root is the new executed head's committed state; what
    /// was queued for the old branch is dropped; the old branch's blocks are
    /// retracted to subscribers before the new branch's are announced, as
    /// rskj's logs emitter does; the index kept by height follows; the pool
    /// loses the transactions the new branch mined; and the miner is told of
    /// its new best block, as rskj's `onBestBlock` tells it.
    pub(crate) fn take_up_moved_execution(&mut self, mark: u64) {
        let store = self.manager.store.clone();
        let Some(switch) = store.take_switches_since(mark) else {
            return;
        };
        let (old, new, new_root) = (switch.from, switch.to, switch.state_root);
        if old == new {
            return;
        }
        let Some(new_header) = store.header(new).ok().flatten() else {
            return;
        };
        let old_header = store.header(old).ok().flatten();

        // A batch running replaces the in-memory root when it lands, and
        // `resync_state_root_with_exec_head` sets it right on the next tick.
        if self.executing.is_none() {
            if let Some(trie) = self.trie_store.clone() {
                match trie.get(new_root.as_slice()) {
                    Some(data) => {
                        self.current_state_root =
                            Some(TrieNode::from_message(&data, trie.as_ref()));
                    }
                    None => warn!(
                        target: "rustock::sync",
                        "The new executed head's state {new_root:?} is not in the trie store"
                    ),
                }
            }
        }
        self.follow_buffer.clear();
        self.follow_gap_since = None;
        self.pending_exec = None;
        self.last_body_height = new_header.number;

        let sides = old_header.as_ref().and_then(|old_header| {
            fork_between(&store, (old, old_header.clone()), (new, new_header.clone()))
        });
        let (left, taken_up) = sides.unwrap_or_else(|| {
            warn!(
                target: "rustock::sync",
                "Execution moved from {old:?} to #{} ({new:?}) with no common block found \
                 within reach; the blocks between are neither retracted nor announced",
                new_header.number
            );
            (Vec::new(), Vec::new())
        });
        let processor = self.block_processor.clone();
        for (hash, header) in &left {
            let transactions = store
                .body(*hash)
                .ok()
                .flatten()
                .map(|(t, _)| t)
                .unwrap_or_default();
            self.publish_block(*hash, header, &transactions, true);
            if let (Some(processor), Ok(Some(receipts))) = (&processor, store.receipts(*hash)) {
                if let Err(e) = processor.uncommit_by_height(header.number, &receipts) {
                    warn!(target: "rustock::sync", "Unindexing #{} failed: {e}", header.number);
                }
            }
        }
        for (hash, header) in &taken_up {
            let Ok(Some((transactions, ommers))) = store.body(*hash) else {
                continue;
            };
            let receipts = store.receipts(*hash).ok().flatten().unwrap_or_default();
            let unlogged = take_unlogged(&self.beside_unlogged, *hash);
            let block = Block {
                header: header.clone(),
                transactions,
                ommers,
            };
            if let Some(processor) = &processor {
                if let Err(e) = processor.commit_by_height(&block, &receipts, &unlogged) {
                    warn!(target: "rustock::sync", "Indexing #{} failed: {e}", header.number);
                }
            }
            if let Some(pool) = &self.tx_pool {
                pool.remove_mined(&block.transactions);
            }
            self.gas_price.on_block(header, &block.transactions);
            self.publish_block(*hash, header, &block.transactions, false);
        }
        if let Some(pool) = &self.tx_pool {
            pool.evict_outdated(new_header.number);
        }
        self.gas_price.on_best_block(&new_header);
        self.best_block_listeners.notify(&new_header);
        self.credit_executed_bodies(new_header.number);
        info!(
            target: "rustock::sync",
            "Switched to a heavier branch: executed head {old:?} -> #{} ({new:?}); {} block(s) \
             left, {} taken up without executing any again",
            new_header.number,
            left.len(),
            taken_up.len()
        );
    }
}

/// The blocks between `from` and `to` down to the block they share: those on
/// `from`'s side newest first, those on `to`'s side oldest first. `None` when
/// a header on the way is not held, or they share nothing within a rollback's
/// reach.
type Sides = (Vec<(B256, Header)>, Vec<(B256, Header)>);

fn fork_between(store: &BlockStore, from: (B256, Header), to: (B256, Header)) -> Option<Sides> {
    let (mut a, mut b) = (from, to);
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for _ in 0..2 * crate::rollback::MAX_ROLLBACK {
        if a.0 == b.0 {
            right.reverse();
            return Some((left, right));
        }
        if a.1.number >= b.1.number {
            let parent = a.1.parent_hash;
            left.push(a);
            a = (parent, store.header(parent).ok().flatten()?);
        } else {
            let parent = b.1.parent_hash;
            right.push(b);
            b = (parent, store.header(parent).ok().flatten()?);
        }
    }
    None
}
