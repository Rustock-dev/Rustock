//! The miner server: hands out work, takes solutions back, and imports the
//! block a solution completes.
//!
//! A solution arrives minutes after the work it answers, by which time the
//! node has usually built several newer templates. So templates are kept in a
//! small cache keyed by the merged-mining hash the coinbase commits to --
//! keeping only the newest would make a miner that was merely a little slow
//! lose a block it had legitimately found. rskj keeps 20; so does this.

use alloy_primitives::{B256, Bytes, U256};
use rustock_core::config::ChainConfig;
use rustock_core::validation::{HeaderValidator, MergedMiningRule};
use rustock_core::validation::merged_mining::{
    BLOCK_HEADER_HASH_SIZE, CoinbaseCompressionError, RSK_TAG, compress_coinbase,
    compute_coinbase_hash, find_last_subsequence,
};
use rustock_core::selection_rule::{should_we_add_this_block, Contender};
use rustock_core::{Block, Header};
use rustock_storage::BlockStore;
use rustock_trie::TrieStore;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

use crate::breakdown::{BreakdownSink, ConnectTimings, Laps};
use crate::mining::coinbase;
use crate::mining::merkle::{self, MerkleProofError};
use crate::mining::template::{
    BlockTemplate, BlockTemplateBuilder, PendingTransactionSource, TemplateError,
};
use crate::silent::SilentRecord;

/// What `mnr_getWork` answers with. rskj `co.rsk.mine.MinerWork`.
#[derive(Debug, Clone)]
pub struct MinerWork {
    /// The 32 bytes to put in the coinbase after `RSKBLOCK:`.
    pub block_hash_for_merged_mining: B256,
    /// The Bitcoin block hash, read little-endian, must not exceed this.
    pub target: U256,
    /// Fees the block pays its miner, as a decimal string -- rskj sends
    /// `String.valueOf(Coin)` here rather than a quantity, and pools parse it.
    pub fees_paid_to_miner: U256,
    /// Set once per genuinely new piece of work, so a pool knows when to push
    /// to its miners rather than wait for the next poll.
    pub notify: bool,
    pub parent_block_hash: B256,
}

/// What the submit methods answer with. rskj `co.rsk.mine.SubmittedBlockInfo`.
#[derive(Debug, Clone)]
pub struct SubmittedBlockInfo {
    pub block_imported_result: ImportResult,
    pub block_hash: B256,
    pub block_included_height: u64,
}

/// The outcomes rskj's `ImportResult` can report for a mined block. A miner
/// reads this to tell "my block won" from "my block was valid but somebody
/// else's arrived first", and from "a rule refuses my block".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportResult {
    ImportedBest,
    ImportedNotBest,
    Exist,
    /// rskj `BlockChainImpl.internalTryToConnect`: the store holds no parent
    /// for the block. Nothing is stored.
    NoParent,
    /// rskj `BlockChainImpl.tryToConnect`: a block that passes its proof of
    /// work and that a rule then refuses, or whose connecting fails in any
    /// other way, a failing store included. Nothing is stored.
    InvalidBlock,
}

impl ImportResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            ImportResult::ImportedBest => "IMPORTED_BEST",
            ImportResult::ImportedNotBest => "IMPORTED_NOT_BEST",
            ImportResult::Exist => "EXIST",
            ImportResult::NoParent => "NO_PARENT",
            ImportResult::InvalidBlock => "INVALID_BLOCK",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SubmitError {
    #[error("could not decode the submitted Bitcoin block: {0}")]
    BitcoinBlockDecode(String),
    #[error("could not decode the submitted Bitcoin header: {0}")]
    BitcoinHeaderDecode(String),
    #[error("could not decode the submitted coinbase transaction: {0}")]
    CoinbaseDecode(String),
    #[error("the submitted coinbase carries no {} tag", String::from_utf8_lossy(RSK_TAG))]
    NoTagInCoinbase,
    #[error("no work outstanding for {hash}: it was never handed out, or it has aged out of the cache")]
    UnknownWork { hash: B256 },
    /// rskj `MinerClientImpl.mineBlock` returns false while
    /// `hasBetterBlockToSync()`: a node still catching up does not mine.
    #[error("not mining: the node has a better block to sync")]
    Syncing,
    /// rskj `SubmissionRateLimitHandler`: "Cannot publish block, block
    /// submission rate limit exceeded".
    #[error("cannot publish block, block submission rate limit exceeded")]
    RateLimited,
    #[error("compressing the coinbase: {0}")]
    Compression(#[from] CoinbaseCompressionError),
    #[error("building the merkle proof: {0}")]
    MerkleProof(#[from] MerkleProofError),
    #[error("the completed block fails a rule every node holds it to: {0}")]
    SelfCheck(rustock_core::validation::ValidationError),
    #[error("re-executing the completed block: {0}")]
    Execution(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("building work: {0}")]
    Template(#[from] TemplateError),
}

/// How a submission answers a solved block whose proof of work holds and
/// that a rule then refuses, or that the node fails to connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refused {
    /// As rskj answers a miner: OK, with `INVALID_BLOCK`
    /// (`MinerServerImpl.processSolution`, `BlockChainImpl.tryToConnect`).
    Answered,
    /// As the node's own miner reads it: the error.
    Failed,
}

/// How the node's chain state is read and advanced. Split out so the server
/// can be driven by a test without a RocksDB store behind it.
pub trait ChainAccess: Send + Sync {
    /// Best chain in descending order, at most `limit` entries: index 0 is the
    /// block a new block would build on.
    fn mainchain(&self, limit: usize) -> Result<Vec<Header>, String>;
    /// Hash of the block whose state the trie store currently holds.
    fn executed_head(&self) -> Result<Option<B256>, String>;
    /// Whether a block with this hash is already stored.
    fn has_block(&self, hash: B256) -> Result<bool, String>;
    /// The header stored under `hash`, named by that key.
    fn header(&self, hash: B256) -> Result<Option<Header>, String>;
    /// Uncles to include in a block mined on `parent_hash`.
    fn select_uncles(&self, block_number: u64, parent_hash: B256) -> Result<Vec<Header>, String>;
    /// Store a fully executed block and make it the head if it wins.
    ///
    /// `laps` is the caller's phase clock, disabled when nothing measures:
    /// the implementation laps it around the store writes (`switch_chain`)
    /// and around the hook and the flush (`on_best_block`).
    fn commit_mined_block(
        &self,
        block: &Block,
        state_root: B256,
        laps: &mut Laps,
    ) -> Result<CommitOutcome, String>;
    /// Store a block whose parent is no longer the executed head, executed
    /// beside the chain against its own parent's state, and answer whether
    /// it takes the head, without moving the head: the sync service makes
    /// that switch, as it does for a peer's block executed beside the chain,
    /// once the hook has told it of the block. `unlogged` holds the peg
    /// decisions the block made that the Bridge logs no event for, which are
    /// stored by height, so only once the chain takes the block up.
    fn commit_mined_block_beside(
        &self,
        block: &Block,
        unlogged: Vec<SilentRecord>,
        laps: &mut Laps,
    ) -> Result<CommitOutcome, String>;
}

/// What [`ChainAccess::commit_mined_block`] reports: the import result and
/// the two phases of the connect that happen inside it.
#[derive(Debug, Clone, Copy)]
pub struct CommitOutcome {
    pub result: ImportResult,
    /// Header, body, total difficulty, canonical index and executed head
    /// written (rskj `switchChainMs`).
    pub switch_chain: std::time::Duration,
    /// The committed hook (the pool told what was mined) and the trie flush
    /// when one is due (rskj `onBestBlockMs`).
    pub on_best_block: std::time::Duration,
}

/// [`ChainAccess`] over the node's real block store.
pub struct StoreChainAccess {
    pub store: Arc<BlockStore>,
    pub trie_store: Arc<dyn TrieStore>,
    /// Called with every block this commits, and what the commit made of
    /// it, so a transaction pool can drop what has just been mined.
    ///
    /// A locally mined block does not travel the sync service's import
    /// path, which is the only place that called `remove_mined`. So
    /// without this the pool keeps every transaction the node itself
    /// mined, forever: `txpool_status` reports them as pending, a miner
    /// rebuilds them into the next block, and the pool grows without
    /// bound. Invisible on a node that only follows the chain, which is
    /// why it survived; immediate on one that mines.
    ///
    /// A closure rather than a pool handle because this crate does not
    /// know what a pool is, and should not have to in order to say "this
    /// block is now committed".
    ///
    /// Called once the block is stored and, if it won, the head and the
    /// executed head: rskj's `EthereumImpl.addNewMinedBlock` (L60-68) tells
    /// the rest of the node after `blockchain.tryToConnect` has connected
    /// it, so whatever the hook tells sees the chain with the block on it.
    /// For a block beside the chain ([`ChainAccess::commit_mined_block_beside`])
    /// the head hasn't moved when it is called: the sync service it tells
    /// makes the switch.
    pub on_committed: Option<Arc<dyn Fn(&Block, ImportResult) + Send + Sync>>,
    /// Best blocks between trie flushes: rskj's
    /// `blockchain.flushNumberOfBlocks`, whose `BlockChainFlusher` flushes
    /// every tenth block a node mines or connects. 1 flushes every block.
    ///
    /// The executed head still moves on every block, so the next template
    /// builds on the block just mined. Between flushes the state of those
    /// blocks lives in the trie store's memory: a crash there leaves the
    /// head naming a root that is not on disk, which the node repairs by
    /// re-executing from the last flushed block, as it does after a crash
    /// while catching up.
    pub flush_every: u64,
    /// Best blocks committed since the last flush.
    pub since_flush: std::sync::atomic::AtomicU64,
    /// Where [`ChainAccess::commit_mined_block_beside`] leaves the peg
    /// decisions the Bridge logs no event for that a block it keeps beside
    /// the chain made, with the block's hash and height. They are stored by
    /// height, so not before the chain takes the block up, which is the sync
    /// service's to do: it holds them till then, as it holds those of a
    /// peer's block executed beside the chain. They are left before the
    /// block is stored, so that no path can take the block up without them.
    pub leave_unlogged: Option<Arc<dyn Fn(B256, u64, Vec<SilentRecord>) + Send + Sync>>,
}

impl StoreChainAccess {
    /// Flush every block, which is what a node mining for a pool wants:
    /// its blocks are durable before they are announced.
    pub fn new(
        store: Arc<BlockStore>,
        trie_store: Arc<dyn TrieStore>,
        on_committed: Option<Arc<dyn Fn(&Block, ImportResult) + Send + Sync>>,
    ) -> Self {
        Self::with_flush_every(store, trie_store, on_committed, 1)
    }

    pub fn with_flush_every(
        store: Arc<BlockStore>,
        trie_store: Arc<dyn TrieStore>,
        on_committed: Option<Arc<dyn Fn(&Block, ImportResult) + Send + Sync>>,
        flush_every: u64,
    ) -> Self {
        Self {
            store,
            trie_store,
            on_committed,
            flush_every: flush_every.max(1),
            since_flush: std::sync::atomic::AtomicU64::new(0),
            leave_unlogged: None,
        }
    }

    /// Leave the peg decisions of a block kept beside the chain with `leave`
    /// ([`Self::leave_unlogged`]).
    pub fn with_leave_unlogged(
        mut self,
        leave: Arc<dyn Fn(B256, u64, Vec<SilentRecord>) + Send + Sync>,
    ) -> Self {
        self.leave_unlogged = Some(leave);
        self
    }

    /// Count a best block and say whether it is time to flush. The counter
    /// starts at 0, so with `flush_every` 10 the flushes fall on the 10th,
    /// 20th, ... block mined, as rskj's flusher counts.
    fn flush_due(&self) -> bool {
        use std::sync::atomic::Ordering;
        let n = self.since_flush.fetch_add(1, Ordering::AcqRel) + 1;
        if n >= self.flush_every {
            self.since_flush.store(0, Ordering::Release);
            true
        } else {
            false
        }
    }

    /// Store a mined block's header, body and total difficulty, each keyed
    /// by its hash, and return the total.
    ///
    /// Deliberately not `put_block`: that writes the canonical
    /// `number -> hash` pointer unconditionally, which would hand the
    /// height to a block that lost to the head and leave the canonical
    /// chain naming a block that is not on it. Header and body are safe to
    /// store either way, being keyed by hash.
    fn store_mined(&self, block: &Block, hash: B256) -> Result<U256, String> {
        let parent_td = self
            .store
            .total_difficulty(block.header.parent_hash)
            .map_err(|e| e.to_string())?
            .unwrap_or(U256::ZERO);
        let td = parent_td + cumulative_difficulty(block);
        self.store
            .put_header_with_hash(hash, &block.header)
            .map_err(|e| e.to_string())?;
        self.store
            .put_body(hash, &block.transactions, &block.ommers)
            .map_err(|e| e.to_string())?;
        self.store
            .put_total_difficulty(hash, td)
            .map_err(|e| e.to_string())?;
        Ok(td)
    }

    /// Whether a mined block of total difficulty `td` takes the head. rskj
    /// connects a mined block through `blockchain.tryToConnect`, as it does a
    /// peer's: it takes the head by `SelectionRule.shouldWeAddThisBlock`,
    /// which settles a tie in total difficulty by fees and then hashes.
    fn takes_head(&self, block: &Block, hash: B256, td: U256) -> Result<bool, String> {
        let (current_td, current) = match self.store.head().map_err(|e| e.to_string())? {
            Some(head) => {
                let td = self
                    .store
                    .total_difficulty(head)
                    .map_err(|e| e.to_string())?
                    .unwrap_or(U256::ZERO);
                let current = self.store.contender(head, td).map_err(|e| e.to_string())?;
                (td, current)
            }
            None => (U256::ZERO, None),
        };
        let mined = Contender {
            total_difficulty: td,
            paid_fees: block.header.paid_fees,
            hash,
        };
        Ok(match &current {
            Some(current) => should_we_add_this_block(&mined, current),
            None => td > current_td,
        })
    }
}

impl ChainAccess for StoreChainAccess {
    fn mainchain(&self, limit: usize) -> Result<Vec<Header>, String> {
        let head = match self.store.exec_head().map_err(|e| e.to_string())? {
            Some((hash, _)) => hash,
            None => self
                .store
                .head()
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "the node has no head block".to_string())?,
        };

        let mut chain = Vec::with_capacity(limit);
        let mut cursor = head;
        while chain.len() < limit {
            match self.store.header(cursor).map_err(|e| e.to_string())? {
                Some(mut header) => {
                    // The key a header is stored under is its hash, by
                    // definition; recomputing it from the decoded fields is
                    // not always the same number. A genesis built from an
                    // rskj JSON file hashes over the file's difficulty bytes
                    // as written, which its decoded form no longer has, so
                    // the first template on such a chain would name a
                    // parent no other node knows. (`setup_genesis` stores it
                    // under rskj's hash for the same reason.)
                    header.cached_hash = Some(cursor);
                    let parent = header.parent_hash;
                    let is_genesis = header.number == 0;
                    chain.push(header);
                    if is_genesis {
                        break;
                    }
                    cursor = parent;
                }
                None => break,
            }
        }

        if chain.is_empty() {
            return Err("the head block's header is missing from the store".to_string());
        }
        Ok(chain)
    }

    fn has_block(&self, hash: B256) -> Result<bool, String> {
        self.store.header(hash).map(|h| h.is_some()).map_err(|e| e.to_string())
    }

    fn header(&self, hash: B256) -> Result<Option<Header>, String> {
        // Named by the key it is stored under, as `mainchain` names each: a
        // genesis from an rskj file hashes otherwise when recomputed.
        let header = self.store.header(hash).map_err(|e| e.to_string())?;
        Ok(header.map(|mut header| {
            header.cached_hash = Some(hash);
            header
        }))
    }

    fn executed_head(&self) -> Result<Option<B256>, String> {
        match self.store.exec_head().map_err(|e| e.to_string())? {
            Some((hash, _)) => Ok(Some(hash)),
            None => self.store.head().map_err(|e| e.to_string()),
        }
    }

    fn select_uncles(&self, block_number: u64, parent_hash: B256) -> Result<Vec<Header>, String> {
        // An un-upgraded database has no height index, so the family of a
        // block cannot be enumerated and selection would silently return
        // nothing forever. Say so once rather than quietly mine uncleless
        // blocks and leave the operator wondering why.
        if self.store.height_index_is_empty().map_err(|e| e.to_string())? {
            warn!(
                target: "rustock::mining",
                "No block-height index in this database, so no uncle can be found. \
                 Run the node once with --build-height-index to add it."
            );
            return Ok(Vec::new());
        }
        Ok(crate::mining::uncles::select_uncles(&self.store, block_number, parent_hash))
    }

    fn commit_mined_block(
        &self,
        block: &Block,
        state_root: B256,
        laps: &mut Laps,
    ) -> Result<CommitOutcome, String> {
        use std::time::Duration;
        let hash = block.hash();
        let number = block.header.number;
        let mut switch_chain = Duration::ZERO;
        let mut on_best_block = Duration::ZERO;

        if self.store.has_block(hash).map_err(|e| e.to_string())? {
            return Ok(CommitOutcome {
                result: ImportResult::Exist,
                switch_chain: laps.lap(),
                on_best_block,
            });
        }

        let td = self.store_mined(block, hash)?;
        let takes_head = self.takes_head(block, hash, td)?;
        switch_chain += laps.lap();

        let result = if takes_head {
            self.store.update_canonical_chain(hash).map_err(|e| e.to_string())?;
            self.store.set_exec_head(hash, state_root).map_err(|e| e.to_string())?;
            switch_chain += laps.lap();
            // The trie nodes this block wrote are only durable once flushed;
            // announcing a head whose state cannot be reloaded after a restart
            // would leave the node unable to build on its own block. With
            // `flush_every` above 1 that durability is batched, see the field.
            if self.flush_due() {
                self.trie_store.flush();
            }
            ImportResult::ImportedBest
        } else {
            ImportResult::ImportedNotBest
        };

        // Only now, with the block stored and, if it won, the head: rskj's
        // `EthereumImpl.addNewMinedBlock` (L60-68) broadcasts a mined block
        // once `blockchain.tryToConnect` has connected it, and the node's sync
        // service, told from here, reads the executed head this just set
        // (#323).
        if let Some(hook) = &self.on_committed {
            hook(block, result);
        }
        on_best_block += laps.lap();

        info!(
            target: "rustock::mining",
            "Mined block #{} {:?} imported: {}", number, hash, result.as_str()
        );
        Ok(CommitOutcome { result, switch_chain, on_best_block })
    }

    fn commit_mined_block_beside(
        &self,
        block: &Block,
        unlogged: Vec<SilentRecord>,
        laps: &mut Laps,
    ) -> Result<CommitOutcome, String> {
        let hash = block.hash();
        let number = block.header.number;
        if self.store.has_block(hash).map_err(|e| e.to_string())? {
            return Ok(CommitOutcome {
                result: ImportResult::Exist,
                switch_chain: laps.lap(),
                on_best_block: std::time::Duration::ZERO,
            });
        }
        // Its state durable before the store may adopt its branch on the
        // strength of it, as the sync service makes durable a block it
        // executes beside the chain.
        self.trie_store.flush();
        // Its peg decisions where the sync service takes them from, before the
        // block is stored and any path can take it onto the chain.
        if !unlogged.is_empty() {
            if let Some(leave) = &self.leave_unlogged {
                leave(hash, number, unlogged);
            }
        }
        let td = self.store_mined(block, hash)?;
        // rskj answers what `internalTryToConnect` made of the block:
        // `IMPORTED_BEST` where the selection rule takes it, and
        // `IMPORTED_NOT_BEST` where it extends a chain beside the best one.
        let result = if self.takes_head(block, hash, td)? {
            ImportResult::ImportedBest
        } else {
            ImportResult::ImportedNotBest
        };
        let switch_chain = laps.lap();
        // The peers are sent it, best or not, and the sync service weighs it.
        if let Some(hook) = &self.on_committed {
            hook(block, result);
        }
        let on_best_block = laps.lap();

        info!(
            target: "rustock::mining",
            "Mined block #{} {:?} imported beside the chain: {}", number, hash, result.as_str()
        );
        Ok(CommitOutcome {
            result,
            switch_chain,
            on_best_block,
        })
    }
}

struct ServerState {
    /// Templates still answerable, oldest first.
    cache: VecDeque<BlockTemplate>,
    current: Option<MinerWork>,
    /// Parent of the newest template, to decide whether new work is worth a
    /// notify.
    last_parent: Option<B256>,
    last_fees_with_notify: U256,
}

pub struct MinerServer {
    builder: BlockTemplateBuilder,
    chain: Arc<dyn ChainAccess>,
    pending: Arc<dyn PendingTransactionSource>,
    chain_config: Arc<ChainConfig>,
    state: Mutex<ServerState>,
    /// Whether the node is still catching up. `None` never is.
    sync_gate: Option<Arc<dyn SyncGate>>,
    /// rskj `SubmissionRateLimitHandler`. Disabled unless configured.
    rate_limit: SubmissionRateLimiter,
    /// Decides whether arriving transactions are worth a rebuild.
    rebuild_policy: RebuildPolicy,
    /// Where the `block connect breakdown best:` line of each block this
    /// server connects as best goes. `None`, the default, reads no clock.
    breakdown: Option<BreakdownSink>,
}

/// rskj `SubmissionRateLimitHandler`: at most one submission per
/// `miner.server.workSubmissionRateLimitInMills`. Zero (rskj's default)
/// disables it.
pub struct SubmissionRateLimiter {
    window_ms: u64,
    last_submitted_at: Mutex<u64>,
    clock: Box<dyn Fn() -> u64 + Send + Sync>,
}

impl SubmissionRateLimiter {
    /// rskj `ofMiningConfig`: enabled when the window is positive.
    pub fn new(window_ms: u64) -> Self {
        Self::with_clock(window_ms, Box::new(now_millis))
    }

    pub fn disabled() -> Self {
        Self::new(0)
    }

    /// With a clock of the caller's, for tests (rskj's `TimeProvider`).
    pub fn with_clock(window_ms: u64, clock: Box<dyn Fn() -> u64 + Send + Sync>) -> Self {
        Self { window_ms, last_submitted_at: Mutex::new(0), clock }
    }

    pub fn is_enabled(&self) -> bool {
        self.window_ms > 0
    }

    pub fn is_submission_allowed(&self) -> bool {
        if !self.is_enabled() {
            return true;
        }
        let now = (self.clock)();
        now.saturating_sub(*self.last_submitted_at.lock().expect("rate limit lock")) >= self.window_ms
    }

    pub fn on_submit(&self) {
        *self.last_submitted_at.lock().expect("rate limit lock") = (self.clock)();
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// rskj `MinerServerImpl.NewBlockTxListener.isWorthRebuilding`: rebuilding
/// the candidate re-executes every transaction in it, so arriving
/// transactions trigger one only once their fees, accumulated since the
/// last rebuild, reach `fraction` of what the candidate already pays. With
/// no candidate to compare against, any arrival is worth it. Skipping a
/// rebuild only delays a transaction: the next best block rebuilds
/// unconditionally, and so does the refresh timer.
pub struct RebuildPolicy {
    /// rskj `miner.server.rebuildFeeFraction`, 0.1 by default.
    fraction: f64,
    fees_since_last_build: Mutex<U256>,
}

impl RebuildPolicy {
    pub const DEFAULT_FRACTION: f64 = 0.1;

    pub fn new(fraction: f64) -> Self {
        Self { fraction, fees_since_last_build: Mutex::new(U256::ZERO) }
    }

    /// rskj `resetAccumulatedFees`.
    pub fn reset(&self) {
        *self.fees_since_last_build.lock().expect("rebuild policy lock") = U256::ZERO;
    }

    /// rskj `isWorthRebuilding`: `candidate_fees` is what the current
    /// candidate pays its miner, `None` when there is none.
    pub fn worth_rebuilding(&self, candidate_fees: Option<U256>, arrivals: &[rustock_core::Transaction]) -> bool {
        let mut accumulated = self.fees_since_last_build.lock().expect("rebuild policy lock");
        let Some(candidate) = candidate_fees.filter(|f| !f.is_zero()) else {
            *accumulated = U256::ZERO;
            return true;
        };
        for tx in arrivals {
            *accumulated += tx.gas_price.saturating_mul(tx.gas_limit);
        }
        // rskj: `new BigDecimal(candidate).multiply(fraction).toBigInteger()`,
        // the product truncated.
        let threshold = {
            let scaled = (self.fraction * 1_000_000.0).round() as u64;
            candidate.saturating_mul(U256::from(scaled)) / U256::from(1_000_000u64)
        };
        if *accumulated < threshold {
            return false;
        }
        *accumulated = U256::ZERO;
        true
    }
}

/// What the miner asks before building work: rskj
/// `NodeBlockProcessor.hasBetterBlockToSync`, which is
/// `BlockSyncService`'s "the last block number a peer announced is at least
/// `longSyncLimit` past our best". A node that far behind mines on a stale
/// head and every block it seals is orphaned on arrival.
pub trait SyncGate: Send + Sync {
    fn has_better_block_to_sync(&self) -> bool;
}

impl MinerServer {
    pub fn new(
        builder: BlockTemplateBuilder,
        chain: Arc<dyn ChainAccess>,
        pending: Arc<dyn PendingTransactionSource>,
        chain_config: Arc<ChainConfig>,
    ) -> Self {
        Self {
            builder,
            chain,
            pending,
            chain_config,
            state: Mutex::new(ServerState {
                cache: VecDeque::new(),
                current: None,
                last_parent: None,
                last_fees_with_notify: U256::ZERO,
            }),
            sync_gate: None,
            rate_limit: SubmissionRateLimiter::disabled(),
            rebuild_policy: RebuildPolicy::new(RebuildPolicy::DEFAULT_FRACTION),
            breakdown: None,
        }
    }

    /// rskj `miner.server.workSubmissionRateLimitInMills`: refuse a
    /// submission within `window_ms` of the previous one. Zero disables.
    pub fn with_submission_rate_limit(mut self, window_ms: u64) -> Self {
        self.rate_limit = SubmissionRateLimiter::new(window_ms);
        self
    }

    /// The rate limiter with a clock of the caller's, for tests.
    pub fn with_submission_rate_limiter(mut self, limiter: SubmissionRateLimiter) -> Self {
        self.rate_limit = limiter;
        self
    }

    /// rskj `NewBlockTxListener.onBestBlock`: a new parent invalidates the
    /// candidate outright, so rebuild, and start the fee accumulator over.
    pub fn on_best_block(&self) -> Result<MinerWork, SubmitError> {
        self.rebuild_policy.reset();
        self.build_work()
    }

    /// rskj `NewBlockTxListener.onPendingTransactionsReceived`: rebuild
    /// when the arrivals are worth it (see [`RebuildPolicy`]). `None` when
    /// they were not.
    pub fn on_pending_transactions_received(
        &self,
        transactions: &[rustock_core::Transaction],
    ) -> Option<Result<MinerWork, SubmitError>> {
        let candidate_fees = self
            .state
            .lock()
            .expect("miner state lock")
            .current
            .as_ref()
            .map(|w| w.fees_paid_to_miner);
        self.rebuild_policy
            .worth_rebuilding(candidate_fees, transactions)
            .then(|| self.build_work())
    }

    /// Refuse to build work while `gate` says there is a better block to
    /// sync. rskj gates `MinerClientImpl.mineBlock` on it; here the gate sits
    /// on `build_work`, which every miner, in-process or merged, goes
    /// through.
    /// Time every block this server completes and connects, and hand the
    /// `block connect breakdown best:` line of each one connected as best to
    /// `sink`. Same keys and scope as the import path's `--breakdown-log`.
    pub fn with_breakdown_log(mut self, sink: BreakdownSink) -> Self {
        self.breakdown = Some(sink);
        self
    }

    pub fn with_sync_gate(mut self, gate: Arc<dyn SyncGate>) -> Self {
        self.sync_gate = Some(gate);
        self
    }


    /// The address block rewards are paid to.
    pub fn coinbase_address(&self) -> alloy_primitives::Address {
        self.builder.mining_config().coinbase_address
    }

    /// Build a fresh template on the current best block and make it the work
    /// on offer. Called when the chain advances, when the pool changes, and
    /// on a timer -- and by `get_work` when there is nothing to hand out yet.
    pub fn build_work(&self) -> Result<MinerWork, SubmitError> {
        if self.sync_gate.as_ref().is_some_and(|g| g.has_better_block_to_sync()) {
            return Err(SubmitError::Syncing);
        }
        let mainchain = self
            .chain
            .mainchain(crate::mining::fork_detection::REQUIRED_MAINCHAIN_BLOCKS)
            .map_err(SubmitError::Storage)?;

        let parent = &mainchain[0];
        let uncles = self
            .chain
            .select_uncles(parent.number + 1, parent.hash())
            .map_err(SubmitError::Storage)?;
        if !uncles.is_empty() {
            info!(
                target: "rustock::mining",
                "Including {} uncle(s) in the block being mined at #{}",
                uncles.len(),
                parent.number + 1
            );
        }

        let template = self.builder.build(&mainchain, uncles, self.pending.as_ref())?;

        let mut state = self.state.lock().expect("miner state lock");
        let parent_hash = template.parent_hash();
        let notify = Self::should_notify(&state, parent_hash, template.fees_paid_to_miner);
        if notify {
            state.last_fees_with_notify = template.fees_paid_to_miner;
        }
        state.last_parent = Some(parent_hash);

        let work = MinerWork {
            block_hash_for_merged_mining: template.hash_for_merged_mining,
            target: template.target,
            fees_paid_to_miner: template.fees_paid_to_miner,
            notify,
            parent_block_hash: parent_hash,
        };
        state.current = Some(work.clone());

        state.cache.push_back(template);
        while state.cache.len() > self.builder.mining_config().work_cache_size {
            state.cache.pop_front();
        }

        Ok(work)
    }

    /// rskj `MinerServerImpl.getNotify`: a new parent always warrants a push;
    /// otherwise only a meaningfully richer block does, so that a pool is not
    /// woken for every transaction that trickles in.
    fn should_notify(state: &ServerState, parent_hash: B256, fees: U256) -> bool {
        if state.last_parent != Some(parent_hash) {
            return true;
        }
        // rskj NOTIFY_FEES_PERCENTAGE_INCREASE = 10.
        let threshold = state.last_fees_with_notify * U256::from(110) / U256::from(100);
        fees > threshold
    }

    /// The work currently on offer, building it first if there is none.
    ///
    /// `notify` is cleared after it has been read once: it marks the
    /// transition, not the work.
    pub fn get_work(&self) -> Result<MinerWork, SubmitError> {
        // Build first if there is nothing on offer, then fall through to the
        // same clearing path. Returning `build_work`'s result directly would
        // leave the stored copy still flagged, so the *second* caller would be
        // told to notify about work the first one already took.
        if self.state.lock().expect("miner state lock").current.is_none() {
            self.build_work()?;
        }

        let mut state = self.state.lock().expect("miner state lock");
        let work = state
            .current
            .clone()
            .ok_or_else(|| SubmitError::Storage("no work available".into()))?;
        if work.notify {
            state.current = Some(MinerWork { notify: false, ..work.clone() });
        }
        Ok(work)
    }

    /// A full Bitcoin block, from which everything else is derivable.
    /// rskj `mnr_submitBitcoinBlock`.
    pub fn submit_bitcoin_block(&self, raw_block: &[u8]) -> Result<SubmittedBlockInfo, SubmitError> {
        self.submit_bitcoin_block_as(raw_block, Refused::Answered)
    }

    /// [`MinerServer::submit_bitcoin_block`], answering a block a rule refuses
    /// as `refused` says.
    pub(crate) fn submit_bitcoin_block_as(
        &self,
        raw_block: &[u8],
        refused: Refused,
    ) -> Result<SubmittedBlockInfo, SubmitError> {
        use bitcoin::consensus::Decodable;

        let mut reader = raw_block;
        let btc_block: bitcoin::Block = Decodable::consensus_decode(&mut reader)
            .map_err(|e| SubmitError::BitcoinBlockDecode(e.to_string()))?;

        let coinbase_tx = btc_block
            .txdata
            .first()
            .ok_or_else(|| SubmitError::BitcoinBlockDecode("block has no transactions".into()))?;
        let coinbase_bytes = coinbase::encode_transaction(coinbase_tx);

        let proof = merkle::proof_from_txids(&coinbase::txids_display_order(&btc_block))?;

        self.complete_and_import(
            &coinbase::encode_header(&btc_block.header),
            &coinbase_bytes,
            proof,
            refused,
        )
    }

    /// Header, coinbase and the block's transaction hashes -- enough to
    /// rebuild the merkle branch without shipping every transaction.
    /// rskj `mnr_submitBitcoinBlockTransactions`.
    pub fn submit_bitcoin_block_transactions(
        &self,
        raw_header: &[u8],
        raw_coinbase: &[u8],
        tx_hashes: &str,
    ) -> Result<SubmittedBlockInfo, SubmitError> {
        let hashes = merkle::parse_wire_hashes(tx_hashes)?;
        let proof = merkle::proof_from_tx_hashes(&hashes)?;
        self.complete_and_import(raw_header, raw_coinbase, proof, Refused::Answered)
    }

    /// Header, coinbase and the merkle branch itself -- the cheapest form, and
    /// what a pool that already computed the branch sends.
    /// rskj `mnr_submitBitcoinBlockPartialMerkle`.
    pub fn submit_bitcoin_block_partial_merkle(
        &self,
        raw_header: &[u8],
        raw_coinbase: &[u8],
        merkle_hashes: &str,
        _block_tx_count: u32,
    ) -> Result<SubmittedBlockInfo, SubmitError> {
        let hashes = merkle::parse_wire_hashes(merkle_hashes)?;
        let proof = merkle::proof_from_merkle_hashes(&hashes)?;
        self.complete_and_import(raw_header, raw_coinbase, proof, Refused::Answered)
    }

    /// The common tail of all three submit paths: match the solution to the
    /// work it answers, fill in the three merged-mining fields, check the
    /// result against the same rule a peer would apply, and import it.
    fn complete_and_import(
        &self,
        raw_btc_header: &[u8],
        raw_coinbase: &[u8],
        merkle_proof: Vec<u8>,
        refused: Refused,
    ) -> Result<SubmittedBlockInfo, SubmitError> {
        // The clock of the connect, rskj `internalTryToConnect`'s scope:
        // from matching the solution to its template to the block being best
        // and durable. Reads nothing unless a breakdown sink is set.
        let mut laps = Laps::new(self.breakdown.is_some());
        let mut t = ConnectTimings::default();

        // rskj `processSolution`: the rate limit is checked first, and the
        // submission counts whether or not the work is found.
        if !self.rate_limit.is_submission_allowed() {
            warn!(target: "rustock::mining", "Cannot publish block, block submission rate limit exceeded");
            return Err(SubmitError::RateLimited);
        }
        self.rate_limit.on_submit();

        let work_hash = extract_merged_mining_hash(raw_coinbase)?;
        let template = self.take_template(work_hash)?;


        let compressed = compress_coinbase(raw_coinbase, true)?;

        let mut block = template.block.clone();
        // Filling these must not disturb the merged-mining hash: RSKIP92 keeps
        // them out of the hashed prefix precisely so that a solution can be
        // attached to a block that was already committed to.
        let hash_before = block.header.hash_for_merged_mining();
        block.header.bitcoin_merged_mining_header = Some(Bytes::copy_from_slice(raw_btc_header));
        block.header.bitcoin_merged_mining_coinbase_transaction =
            Some(Bytes::from(compressed));
        block.header.bitcoin_merged_mining_merkle_proof = Some(Bytes::from(merkle_proof));
        debug_assert_eq!(hash_before, block.header.hash_for_merged_mining());

        // rskj `processSolution` holds the solution to the proof of work before
        // it connects anything (`isValid`, `ProofOfWorkRule`), and answers
        // ERROR when it fails: the same rule every peer will run, so failing it
        // means the node built something it would itself reject.
        MergedMiningRule { config: self.chain_config.clone() }
            .validate(&block.header)
            .map_err(SubmitError::SelfCheck)?;

        // Then `ethereum.addNewMinedBlock`, whose `blockchain.tryToConnect`
        // the miner is answered OK with: what it made of the block. A block a
        // rule refuses is `INVALID_BLOCK` there, and is not stored; so is one
        // whose connecting fails in any other way, the node's own store
        // included, as `BlockChainImpl.tryToConnect` catches every `Throwable`
        // and answers `INVALID_BLOCK`. The node's own miner reads the error.
        let completed_hash = block.hash();
        let number = block.header.number;
        match self.connect_mined(&block, &template, &mut laps, &mut t) {
            Err(error) if refused == Refused::Answered => {
                warn!(target: "rustock::mining", "Mined block #{number} {completed_hash} is invalid: {error}");
                Ok(SubmittedBlockInfo {
                    block_imported_result: ImportResult::InvalidBlock,
                    block_hash: completed_hash,
                    block_included_height: number,
                })
            }
            connected => connected,
        }
    }

    /// rskj `BlockChainImpl.internalTryToConnect` for a solved block of this
    /// node's work, in its order: a block already stored, the parent, the
    /// rules, the run, the store. A rule that refuses the block and a store
    /// that fails to read or write are both errors here; the caller answers
    /// what rskj answers for them.
    fn connect_mined(
        &self,
        block: &Block,
        template: &BlockTemplate,
        laps: &mut Laps,
        t: &mut ConnectTimings,
    ) -> Result<SubmittedBlockInfo, SubmitError> {
        let completed_hash = block.hash();
        let number = block.header.number;
        let answer = |result| SubmittedBlockInfo {
            block_imported_result: result,
            block_hash: completed_hash,
            block_included_height: number,
        };

        // The same solution submitted twice: rskj's `addNewMinedBlock`
        // answers `EXIST` and the submission is `OK`. Checked before the
        // parent, which the first submission moved.
        if self.chain.has_block(completed_hash).map_err(SubmitError::Storage)? {
            return Ok(answer(ImportResult::Exist));
        }

        // The work's parent may no longer be the executed head: the chain moved
        // on while the solution was found. rskj connects such a block as it
        // connects any (`BlockChainImpl.internalTryToConnect`): against its own
        // parent's state, beside the chain, stored whether it is best or not,
        // and answers what that made of it.
        let head = self.chain.executed_head().map_err(SubmitError::Storage)?;
        let beside = head != Some(template.parent_hash());
        t.preamble = laps.lap();

        // A parent the store doesn't hold is `NO_PARENT`, before any rule.
        let Some(parent) = self
            .chain
            .header(template.parent_hash())
            .map_err(SubmitError::Storage)?
        else {
            return Ok(answer(ImportResult::NoParent));
        };
        // rskj then holds a mined block to every rule a peer's block is held
        // to before running it (`BlockValidatorImpl`): the header's own and
        // against its parent, the body's, the uncles' and fork detection.
        // RSKIP179 among them: a solution found in a Bitcoin block timestamped
        // five minutes or more from the template is one every peer rejects.
        (rustock_core::validation::BitcoinTimestampRule {
            config: self.chain_config.clone(),
        })
        .validate(&block.header)
        .map_err(SubmitError::SelfCheck)?;
        rustock_core::validation::HeaderVerifier::default_rsk(self.chain_config.clone())
            .verify(&block.header, Some(&parent))
            .map_err(SubmitError::SelfCheck)?;
        self.builder
            .processor()
            .validate_block(block)
            .map_err(SubmitError::SelfCheck)?;
        t.initial_validation = laps.lap();

        let state_root = self
            .builder
            .load_state_root(template.parent_state_root)
            .map_err(SubmitError::Template)?;
        // `process_and_commit`, in its two halves, so the receipts can be
        // timed apart from execution. The timed processor reads the clock
        // only when measuring; the plain one never does. A block beside the
        // chain keeps what it keys by hash, its receipts and transactions, as
        // it runs; what it keys by height waits for the chain to take it up
        // (`BlockProcessor::process_beside`).
        let processor = self.builder.processor();
        let trie_store = self.builder.trie_store();
        let mut process = crate::ProcessTimings::default();
        let executed = if beside {
            processor.process_beside(block, &state_root, trie_store)
        } else if laps.enabled() {
            processor.process_block_timed(block, &state_root, trie_store, &mut process)
        } else {
            processor.process_block(block, &state_root, trie_store)
        }
        .map_err(|e| SubmitError::Execution(e.to_string()))?;
        laps.lap();
        t.initial_validation += process.pre_validation;
        t.execute = process.execute;
        t.trie_save = process.trie_save;
        t.post_execute_validation = process.post_validation;
        if beside {
            let outcome = self
                .chain
                .commit_mined_block_beside(block, executed.unlogged_peg_events, laps)
                .map_err(SubmitError::Storage)?;
            // The head hasn't moved by this. If the block takes it, the sync
            // service's switch moves it and tells the miner of its new best
            // block, which builds new work then.
            return Ok(answer(outcome.result));
        }
        processor
            .commit_receipts(block, &executed)
            .map_err(|e| SubmitError::Execution(e.to_string()))?;
        t.save_receipts = laps.lap();

        let outcome = self
            .chain
            .commit_mined_block(block, executed.state_root_hash, laps)
            .map_err(SubmitError::Storage)?;
        t.switch_chain = outcome.switch_chain;
        t.on_best_block = outcome.on_best_block;
        t.total = laps.total();

        if let (Some(sink), ImportResult::ImportedBest) = (&self.breakdown, outcome.result) {
            sink(&t.breakdown_line(
                block.header.number,
                completed_hash,
                block.transactions.len(),
                block.header.gas_used,
            ));
        }

        // The chain moved, so the work on offer is stale whatever happens next.
        if let Err(e) = self.build_work() {
            warn!(target: "rustock::mining", "Could not rebuild work after a submission: {e}");
        }

        Ok(answer(outcome.result))
    }

    /// The cached template a solution answers. Templates are not removed:
    /// two solutions to the same work is not an error, and the second would
    /// otherwise be reported as unknown work rather than as a duplicate block.
    fn take_template(&self, hash: B256) -> Result<BlockTemplate, SubmitError> {
        let state = self.state.lock().expect("miner state lock");
        state
            .cache
            .iter()
            .rev()
            .find(|t| t.hash_for_merged_mining == hash)
            .cloned()
            .ok_or(SubmitError::UnknownWork { hash })
    }

    /// The template builder behind this server, so a test can build a
    /// template on a parent of its choosing without going through the store.
    #[cfg(test)]
    pub(crate) fn builder_for_test(&self) -> &BlockTemplateBuilder {
        &self.builder
    }

    /// The coinbase hash a completed header commits to, for tests and
    /// diagnostics.
    pub fn coinbase_hash(compressed: &[u8]) -> [u8; 32] {
        compute_coinbase_hash(compressed)
    }
}

/// What a block adds to the chain's total difficulty: its own and its
/// uncles', as rskj's `Block.getCumulativeDifficulty` counts it and the
/// sync path's header-and-body accounting does. Counting the header alone
/// leaves the stored total short by every uncle, which an rskj export of
/// the same chain shows at the first block with one.
pub fn cumulative_difficulty(block: &Block) -> U256 {
    block
        .ommers
        .iter()
        .fold(block.header.difficulty, |total, uncle| total.saturating_add(uncle.difficulty))
}

/// The 32 bytes following the last `RSKBLOCK:` in a coinbase: the key a
/// submission is matched to outstanding work by. rskj
/// `MnrModuleImpl.extractBlockHashForMergedMining`.
pub fn extract_merged_mining_hash(coinbase: &[u8]) -> Result<B256, SubmitError> {
    let position = find_last_subsequence(coinbase, RSK_TAG).ok_or(SubmitError::NoTagInCoinbase)?;
    let start = position + RSK_TAG.len();
    let end = start + BLOCK_HEADER_HASH_SIZE;
    if coinbase.len() < end {
        return Err(SubmitError::NoTagInCoinbase);
    }
    Ok(B256::from_slice(&coinbase[start..end]))
}
