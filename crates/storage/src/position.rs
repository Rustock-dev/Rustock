//! The node's position on the chain, read and written as one thing.
//!
//! # Why this module exists
//!
//! The node keeps three facts about where it is:
//!
//! ```text
//!   KEY_HEAD        the highest block we consider downloaded and linked
//!   KEY_EXEC_HEAD   the highest block we have actually executed
//!   CF_NUMBERS      height -> hash, the canonical index
//! ```
//!
//! Every sync stall this node has suffered -- seven of them, costing minutes,
//! hours and days -- has been the same sentence: **two of those three
//! disagreed, and some code path read the one that was wrong.** They were
//! independently writable, so a caller could update one and not the others, and
//! nothing anywhere said they had to agree.
//!
//! The narrow fixes did not converge. The fifth fix caused the sixth: the
//! orphan-adopt path called [`BlockStore::ensure_canonical_lineage`], which
//! writes `CF_NUMBERS` and nothing else, leaving mainnet #9,262,402 held,
//! linked, valid and unreachable for three hours because every path that looks
//! for work reads `KEY_HEAD`.
//!
//! So this module stops treating that as a bug to be found and starts treating
//! it as a state to be made unrepresentable:
//!
//! * [`Cursor`] is the only way to ask where we are. It reads all three.
//! * [`Validated`] is a block reference that **cannot be constructed** without
//!   proving the header is held and its lineage links. It is the only thing
//!   that can advance a head.
//! * [`Transition`] is the only way to change position, and every variant
//!   writes whatever keys must move **together**, in one `WriteBatch`.
//!
//! There is deliberately no way to write a canonical entry without saying what
//! the head becomes. Stall 6 is not defended against here; it is not
//! expressible.
//!
//! # What this does not do
//!
//! It does not make the node choose the *right* head -- that is fork choice,
//! and it is elsewhere. It guarantees only that whatever the node chooses, its
//! three notions of position agree about it afterwards.

use crate::{BlockStore, CF_NUMBERS, KEY_EXEC_HEAD, KEY_HEAD};
use alloy_primitives::B256;
use anyhow::{Context, Result};
use rocksdb::WriteBatch;
use rustock_core::selection_rule::should_we_add_this_block;
use rustock_core::Header;

/// A block, by height and hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockRef {
    pub number: u64,
    pub hash: B256,
}

impl BlockRef {
    pub fn new(number: u64, hash: B256) -> Self {
        Self { number, hash }
    }
}

impl std::fmt::Display for BlockRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{} ({})", self.number, self.hash)
    }
}

/// Where the lineage of a block stops holding, and why.
///
/// The value of naming these is that the node has never had the diagnostic:
/// the old code returned `Ok(())` for every one of them.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LineageBreak {
    #[error("no header stored for {hash} at #{at}")]
    MissingHeader { at: u64, hash: B256 },

    #[error("#{at} names {holds} but its child says its parent is {parent_of_child}")]
    ParentMismatch { at: u64, holds: B256, parent_of_child: B256 },

    #[error("the lineage of {hash} runs below the pruning floor #{floor}")]
    BelowFloor { hash: B256, floor: u64 },

    #[error("storage error while proving lineage: {0}")]
    Storage(String),
}

/// A block reference that has been **proved** to be present and lineage-linked.
///
/// The only constructor is [`Validated::prove`]. A bare `B256` cannot become
/// one, which is what makes "advance the head onto a block we never downloaded"
/// -- stall 5, fifteen minutes on mainnet -- a type error rather than a wedge.
///
/// It carries the canonical entries that must be written to make it canonical,
/// so that adopting it and indexing it are one operation and cannot come apart.
#[derive(Debug, Clone)]
pub struct Validated {
    tip: BlockRef,
    /// Canonical entries this block needs, ascending by height. Empty when the
    /// block is already canonical all the way down.
    lineage: Vec<(u64, B256)>,
}

impl Validated {
    /// Prove that `hash` is held and that its ancestry links, collecting the
    /// canonical entries needed to adopt it.
    ///
    /// Walks down from `hash` until the chain below is already consistent --
    /// this block is canonical *and* the canonical entry below it is this
    /// block's parent. Stopping merely at "already canonical" is not enough:
    /// a walk truncated by a then-missing header leaves a lower height stale
    /// while a higher one is correct, which is stall 2.
    ///
    /// Unlike the code this replaces, a missing header is an **error**, not a
    /// silent stop. That silent stop is stall 5 exactly: nothing was written,
    /// `Ok` was returned, and the canonical pointer kept naming a block the
    /// node did not have.
    pub fn prove(store: &BlockStore, hash: B256) -> std::result::Result<Self, LineageBreak> {
        let err = |e: anyhow::Error| LineageBreak::Storage(e.to_string());

        let floor = store.prune_floor().map_err(err)?.map_or(0, |f| f.number);

        let header = store
            .header(hash)
            .map_err(err)?
            .ok_or(LineageBreak::MissingHeader { at: u64::MAX, hash })?;
        let tip = BlockRef::new(header.number, hash);

        let mut lineage = Vec::new();
        let mut cursor_hash = hash;
        let mut cursor_header: Header = header;

        loop {
            let n = cursor_header.number;
            let already_canonical = store.canonical_hash(n).map_err(err)? == Some(cursor_hash);
            if !already_canonical {
                lineage.push((n, cursor_hash));
            }

            if n == 0 || n <= floor {
                break;
            }

            let parent_hash = cursor_header.parent_hash;

            // Below the floor there is deliberately nothing to link to.
            if n - 1 < floor {
                break;
            }

            // Stop only when the chain below is already consistent.
            if already_canonical && store.canonical_hash(n - 1).map_err(err)? == Some(parent_hash) {
                break;
            }

            let parent = store
                .header(parent_hash)
                .map_err(err)?
                .ok_or(LineageBreak::MissingHeader { at: n - 1, hash: parent_hash })?;

            if parent.number + 1 != n {
                return Err(LineageBreak::ParentMismatch {
                    at: parent.number,
                    holds: parent_hash,
                    parent_of_child: cursor_hash,
                });
            }

            cursor_hash = parent_hash;
            cursor_header = parent;
        }

        lineage.reverse(); // ascending, so a batch writes bottom-up
        Ok(Self { tip, lineage })
    }

    /// The block this proves.
    pub fn block(&self) -> BlockRef {
        self.tip
    }

    pub fn number(&self) -> u64 {
        self.tip.number
    }

    pub fn hash(&self) -> B256 {
        self.tip.hash
    }

    /// Canonical entries that adopting this block will write.
    pub fn lineage(&self) -> &[(u64, B256)] {
        &self.lineage
    }
}

/// The node's position: all three keys, read together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// `KEY_HEAD` -- the highest block we consider downloaded and linked.
    pub validated_head: BlockRef,
    /// `KEY_EXEC_HEAD` -- the highest block we have executed.
    pub executed: BlockRef,
    /// The state root recorded alongside the executed head.
    pub state_root: B256,
}

impl Cursor {
    /// Blocks downloaded but not yet executed.
    pub fn backlog(&self) -> u64 {
        self.validated_head.number.saturating_sub(self.executed.number)
    }
}

/// A complete change of position.
///
/// Each variant names every key that has to move, so there is no way to move
/// one and forget another. This is the whole point of the type: the seven
/// stalls were all "wrote one, forgot the others", and that sentence cannot be
/// written here.
#[derive(Debug, Clone)]
pub enum Transition {
    /// Adopt a proved block as the canonical tip.
    ///
    /// Writes the lineage **and** the head, and drops any canonical entry left
    /// stranded above the new tip by a previous, longer chain.
    Adopt { head: Validated },

    /// Move the head back, dropping the canonical entries above it so the
    /// range is downloaded again.
    Retreat { to: Validated },

    /// Record execution progress. The block must already be canonical.
    Executed { at: Validated, state_root: B256 },
}

impl Transition {
    /// A short description for logs.
    pub fn describe(&self) -> String {
        match self {
            Transition::Adopt { head } => {
                format!("adopt {} ({} canonical entries)", head.block(), head.lineage.len())
            }
            Transition::Retreat { to } => format!("retreat to {}", to.block()),
            Transition::Executed { at, .. } => format!("executed {}", at.block()),
        }
    }
}

/// A move of the executed head that a change of head made: execution taken
/// off a block the new chain leaves out, or that stands above its new head,
/// to a block of the new chain (see `stage_execution_after_reorg`).
///
/// Execution recording its own progress is not one. A batch does that on a
/// thread of its own while the sync service stores headers, so the executed
/// head read before and after a call can differ with no switch at all. The
/// switch is what the store records here, from the executed head it found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionSwitch {
    /// The executed head the change of head found.
    pub from: B256,
    /// Where it put it.
    pub to: B256,
    /// The state root recorded with `to`.
    pub state_root: B256,
}

/// How many switches are kept for the calls that made them to take up. A
/// call makes a few at most: one for each block it weighs again.
const SWITCHES_KEPT: usize = 1024;

/// The switches changes of head made, numbered, the latest kept.
#[derive(Debug, Default)]
pub(crate) struct Switches {
    /// The number the next switch gets.
    next: u64,
    /// The latest, oldest first.
    kept: std::collections::VecDeque<(u64, ExecutionSwitch)>,
}

impl BlockStore {
    /// Read all three position keys together.
    ///
    /// `None` when the node has no head yet -- an empty database, before
    /// genesis. There is nothing to be incoherent about.
    ///
    /// With no executed head recorded -- a node that has executed nothing, as
    /// one has until its snapshot sync finishes -- `executed` reads as the
    /// head and `state_root` as zero. That keeps the relations between the
    /// three keys checkable, but it is a stand-in, not a block whose state was
    /// computed: code that moves execution reads [`BlockStore::exec_head`].
    pub fn cursor(&self) -> Result<Option<Cursor>> {
        let Some(head_hash) = self.head()? else {
            return Ok(None);
        };
        let Some(head) = self.header(head_hash)? else {
            // KEY_HEAD names a header we do not hold. Report it as absent
            // rather than inventing a position; the caller's invariant check
            // turns this into a located failure.
            anyhow::bail!("KEY_HEAD names {head_hash}, which is not in the header store");
        };

        let (exec_hash, state_root) = match self.exec_head()? {
            Some(pair) => pair,
            None => (head_hash, B256::ZERO),
        };
        let exec_number = match self.header(exec_hash)? {
            Some(h) => h.number,
            None => anyhow::bail!("KEY_EXEC_HEAD names {exec_hash}, which is not in the header store"),
        };

        Ok(Some(Cursor {
            validated_head: BlockRef::new(head.number, head_hash),
            executed: BlockRef::new(exec_number, exec_hash),
            state_root,
        }))
    }

    /// Apply a transition as a single atomic write.
    ///
    /// One `WriteBatch` spanning the canonical index and the head keys, so an
    /// interrupted transition did not happen rather than half-happening. The
    /// old code wrote the lineage in one batch and the head in a separate
    /// `put`, which is precisely the window stall 6 lived in -- except that it
    /// did not even need a crash to get there, because the two calls were made
    /// from different places.
    ///
    /// And one at a time. A batch records execution on a thread of its own
    /// while the sync service moves the head, and each reads the executed head
    /// it changes: a move of the head that read it just before the batch's
    /// record landed would leave execution on a block the new chain leaves
    /// out, and record the switch from the wrong block.
    pub fn apply(&self, transition: &Transition) -> Result<Cursor> {
        let held = self.transitions.lock().unwrap_or_else(|e| e.into_inner());
        self.apply_holding(&held, transition)
    }

    /// [`Self::apply`], for a caller that holds the lock on transitions
    /// already: one that weighs a move against the executed head first.
    pub(crate) fn apply_holding(
        &self,
        _held: &std::sync::MutexGuard<'_, ()>,
        transition: &Transition,
    ) -> Result<Cursor> {
        let cf_numbers = self.cf(CF_NUMBERS)?;
        let mut batch = WriteBatch::default();
        let before = self.cursor()?;

        let switch = match transition {
            Transition::Adopt { head } => {
                for (number, hash) in &head.lineage {
                    batch.put_cf(cf_numbers, number.to_be_bytes(), hash.as_slice());
                }
                self.stage_drop_above(&mut batch, cf_numbers, head.number())?;
                batch.put(KEY_HEAD, head.hash().as_slice());
                self.stage_execution_after_reorg(&mut batch, before.as_ref(), head)?
            }
            Transition::Retreat { to } => {
                for (number, hash) in &to.lineage {
                    batch.put_cf(cf_numbers, number.to_be_bytes(), hash.as_slice());
                }
                self.stage_drop_above(&mut batch, cf_numbers, to.number())?;
                batch.put(KEY_HEAD, to.hash().as_slice());
                self.stage_execution_after_reorg(&mut batch, before.as_ref(), to)?
            }
            Transition::Executed { at, state_root } => {
                // `Validated` proves lineage, not canonicity: a fork block is
                // perfectly provable. Recording execution of one puts the
                // executed head off the canonical chain -- I5 -- and the node
                // then executes forward from a branch it is not on.
                //
                // The simulator found this at step 34 of its first seed. It is
                // stall 4 arriving through a door the type system left open,
                // so the check lives here, where it cannot be forgotten.
                if self.canonical_hash(at.number())? != Some(at.hash()) {
                    anyhow::bail!(
                        "refusing to record execution of {}: it is not the canonical block \
                         at its height",
                        at.block()
                    );
                }
                if let Some(before) = &before {
                    if at.number() > before.validated_head.number {
                        anyhow::bail!(
                            "refusing to record execution of {}: it is above the head {}",
                            at.block(),
                            before.validated_head
                        );
                    }
                }
                let mut buf = Vec::with_capacity(64);
                buf.extend_from_slice(at.hash().as_slice());
                buf.extend_from_slice(state_root.as_slice());
                batch.put(KEY_EXEC_HEAD, &buf);
                None
            }
        };

        self.db
            .write(batch)
            .with_context(|| format!("applying transition: {}", transition.describe()))?;
        if let Some(switch) = switch {
            self.note_switch(switch);
        }

        self.cursor()?
            .context("position is unreadable immediately after applying a transition")
    }

    /// A mark to take up switches of execution from:
    /// [`Self::take_switches_since`] returns those made after it.
    pub fn switch_mark(&self) -> u64 {
        self.switches.lock().unwrap_or_else(|e| e.into_inner()).next
    }

    /// The switches of execution changes of head made since `mark`, as one:
    /// from the executed head the first found to where the last put it.
    /// `None` when there were none, though the executed head may have moved all
    /// the same, by execution recording its progress, which is no switch. Each
    /// is returned once.
    pub fn take_switches_since(&self, mark: u64) -> Option<ExecutionSwitch> {
        let mut switches = self.switches.lock().unwrap_or_else(|e| e.into_inner());
        let mut taken: Option<ExecutionSwitch> = None;
        switches.kept.retain(|&(number, switch)| {
            if number < mark {
                return true;
            }
            taken = Some(match taken {
                Some(first) => ExecutionSwitch {
                    from: first.from,
                    ..switch
                },
                None => switch,
            });
            false
        });
        taken
    }

    fn note_switch(&self, switch: ExecutionSwitch) {
        let mut switches = self.switches.lock().unwrap_or_else(|e| e.into_inner());
        let number = switches.next;
        switches.next += 1;
        if switches.kept.len() == SWITCHES_KEPT {
            switches.kept.pop_front();
        }
        switches.kept.push_back((number, switch));
    }

    /// Bring the executed head back with the chain when adopting or retreating
    /// moves the ground under it.
    ///
    /// Found by the simulator in 34 steps, and it is stall 4's shape: execute
    /// #1 on one branch, adopt a tip on another that rewrites #1, and the
    /// executed head is no longer the canonical block at its height. The node
    /// then executes forward from a block that is not on its own chain.
    ///
    /// Two ways that happens, both handled here:
    ///
    /// * the reorg rewrites the height the executed block sits at (or below
    ///   it), orphaning it;
    /// * the new head is *lower* than the executed block, which is I4.
    ///
    /// The rollback target is the fork point -- the height just below the
    /// lowest one this transition rewrites -- clamped to the new head. That is
    /// conservative: a block common to both branches. Re-executing a few
    /// blocks costs seconds; executing from the wrong branch costs a fork.
    ///
    /// Unless this node has executed the new branch above the fork already,
    /// as it does every block beside the chain on arrival, the way rskj
    /// connects every block: then execution goes to the highest of those
    /// blocks the lineage reaches without a gap, and a switch to a branch
    /// already executed is a change of pointer, with nothing run again.
    ///
    /// It only ever goes to execution that happened. Moving the executed head
    /// anywhere else records as computed a state that was not, and the node
    /// cannot resume from it.
    ///
    /// The state root comes from the target header, which post-RSKIP126 is the
    /// committed unitrie root -- the same source the reorg path has always
    /// used.
    ///
    /// Returns the switch it stages, when it moves execution.
    fn stage_execution_after_reorg(
        &self,
        batch: &mut WriteBatch,
        before: Option<&Cursor>,
        new_head: &Validated,
    ) -> Result<Option<ExecutionSwitch>> {
        let Some(before) = before else {
            return Ok(None);
        };
        // Nothing executed is nothing to bring back. Without a recorded
        // executed head, `cursor` reads execution as the head, and taken at
        // its word that stand-in is above any retreat -- so the rollback below
        // recorded the retreat's target as executed. Mainnet, 6 November 2026:
        // a light node whose snapshot sync had given up retreated from
        // #9,388,800 to #9,388,799 on the watchdog's first escalation and was
        // left "executed" at #9,388,799, with no state under it. A restart
        // could not clear that either, because snapshot sync starts only on a
        // node that has executed nothing.
        if self.exec_head()?.is_none() {
            return Ok(None);
        }
        let executed = before.executed;

        // What will the canonical block at the executed height be once this
        // transition lands?
        let canonical_at_exec = new_head
            .lineage
            .iter()
            .find(|(n, _)| *n == executed.number)
            .map(|(_, h)| *h)
            .or(self.canonical_hash(executed.number)?);

        let orphaned = canonical_at_exec != Some(executed.hash);
        let above_head = executed.number > new_head.number();
        if !orphaned && !above_head {
            return Ok(None);
        }

        // The fork point: just below the lowest height this transition
        // rewrites. With no lineage, nothing was rewritten and the only
        // problem can be the head having moved down.
        let lowest_rewritten = new_head.lineage.first().map(|(n, _)| *n);
        let target_number = match lowest_rewritten {
            Some(n) => n.saturating_sub(1).min(new_head.number()),
            None => new_head.number(),
        };

        // A target at or above the executed height means this transition
        // rewrites nothing at or below it: the executed block was off the
        // chain before it began. Moving execution to the canonical block up
        // there would claim a state never computed. That repair belongs to the
        // sync layer, which can see what state is held (I5).
        if target_number >= executed.number {
            return Ok(None);
        }

        // Resolve the target on the chain as it will be after this batch.
        let target_hash = new_head
            .lineage
            .iter()
            .find(|(n, _)| *n == target_number)
            .map(|(_, h)| *h)
            .or(self.canonical_hash(target_number)?);

        let Some(target_hash) = target_hash else {
            anyhow::bail!(
                "execution at {executed} is orphaned by this transition and there is no \
                 canonical block at #{target_number} to roll it back to"
            );
        };
        let Some(target) = self.header(target_hash)? else {
            anyhow::bail!(
                "execution at {executed} is orphaned and the rollback target {target_hash} \
                 at #{target_number} is not in the header store"
            );
        };
        let (target_hash, target) =
            match self.executed_climb(&new_head.lineage, target_number, new_head.number())? {
                Some(climbed) => climbed,
                None => (target_hash, target),
            };

        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(target_hash.as_slice());
        buf.extend_from_slice(target.state_root.as_slice());
        batch.put(KEY_EXEC_HEAD, &buf);
        Ok(Some(ExecutionSwitch {
            from: executed.hash,
            to: target_hash,
            state_root: target.state_root,
        }))
    }

    /// The highest block of `lineage` above height `above`, and not above
    /// `top`, that this node executed and still holds the state of: climbing
    /// from `above + 1` and stopping at the first block it did not execute,
    /// so that every block below the one returned was executed too.
    fn executed_climb(
        &self,
        lineage: &[(u64, B256)],
        above: u64,
        top: u64,
    ) -> Result<Option<(B256, Header)>> {
        let mut climbed = None;
        let mut next = above.saturating_add(1);
        for &(number, hash) in lineage {
            if number < next {
                continue;
            }
            if number != next || number > top {
                break;
            }
            let Some(header) = self.header(hash)? else {
                break;
            };
            if !self.executed_here(hash, &header) {
                break;
            }
            climbed = Some((hash, header));
            next += 1;
        }
        Ok(climbed)
    }

    /// Whether adopting `new_head` has to wait until this node has executed
    /// more of its branch: rskj's rule that a block becomes the best one only
    /// once it has been connected, that is executed
    /// (`BlockChainImpl.internalTryToConnect`).
    ///
    /// It waits when the move would leave the executed head off the chain --
    /// the new branch rewrites the executed height, or ends below it -- and
    /// the highest block of the new branch this node has executed, climbing
    /// from the fork point without a gap, does not take the place of the
    /// executed head by rskj's selection rule. A node that executes nothing
    /// never waits: it has no executed head to strand, and nothing to execute
    /// the branch with.
    pub(crate) fn adoption_waits_for_execution(&self, new_head: &Validated) -> Result<bool> {
        if !self.executes() {
            return Ok(false);
        }
        let Some((exec_hash, _)) = self.exec_head()? else {
            return Ok(false);
        };
        let Some(executed) = self.header(exec_hash)? else {
            return Ok(false);
        };
        // Nothing rewritten at or below the executed height: the new head
        // extends the chain execution is on.
        let Some(&(lowest, _)) = new_head.lineage.first() else {
            return Ok(false);
        };
        if lowest > executed.number {
            return Ok(false);
        }
        // The lineage runs without a gap from `lowest` to the new head, so
        // when it holds no entry at the executed height the new head is below
        // it, and the executed block is dropped with everything above.
        let stranded = new_head
            .lineage
            .iter()
            .find(|(n, _)| *n == executed.number)
            .is_none_or(|(_, hash)| *hash != exec_hash);
        if !stranded {
            return Ok(false);
        }
        let fork = lowest.saturating_sub(1);
        let Some((tip, _)) = self.executed_climb(&new_head.lineage, fork, new_head.number())?
        else {
            return Ok(true);
        };
        let tip_td = self.total_difficulty(tip)?.unwrap_or_default();
        let exec_td = self.total_difficulty(exec_hash)?.unwrap_or_default();
        Ok(
            match (
                self.contender(tip, tip_td)?,
                self.contender(exec_hash, exec_td)?,
            ) {
                (Some(tip), Some(current)) => !should_we_add_this_block(&tip, &current),
                _ => true,
            },
        )
    }

    /// Stage deletion of every canonical entry above `top`.
    ///
    /// A canonical entry above the head is the shape of stall 6: the block is
    /// held, linked and valid, and unreachable because every path that looks
    /// for work reads the head. Adopting or retreating to a tip therefore has
    /// to clear whatever a previous, longer chain left above it.
    ///
    /// Bounded: it walks up only while entries actually exist, so the common
    /// case (nothing above) costs one read.
    fn stage_drop_above(
        &self,
        batch: &mut WriteBatch,
        cf: &rocksdb::ColumnFamily,
        top: u64,
    ) -> Result<()> {
        /// A reorg deeper than this is not something to unwind entry by entry.
        const MAX_DROP: u64 = 100_000;
        let mut n = top + 1;
        let mut dropped = 0u64;
        while dropped < MAX_DROP {
            match self.canonical_hash(n)? {
                Some(_) => {
                    batch.delete_cf(cf, n.to_be_bytes());
                    n += 1;
                    dropped += 1;
                }
                None => break,
            }
        }
        if dropped >= MAX_DROP {
            anyhow::bail!(
                "more than {MAX_DROP} canonical entries sit above #{top}; refusing to unwind"
            );
        }
        Ok(())
    }
}

/// Relations `apply` checks on itself, immediately after writing.
///
/// Deliberately local and cheap -- three point reads -- so it can run on every
/// commit in release builds. The full coherence sweep lives in the sync crate
/// and covers the height range as well; this is the last line of defence at
/// the layer that does the writing, so that a transition can never *leave*
/// the store in the shape that wedged mainnet.
pub fn verify_local_coherence(store: &BlockStore, cursor: &Cursor) -> Result<(), String> {
    let canonical_at_head = store
        .canonical_hash(cursor.validated_head.number)
        .map_err(|e| e.to_string())?;
    if canonical_at_head != Some(cursor.validated_head.hash) {
        return Err(format!(
            "the head is {} but the canonical block at that height is {:?}",
            cursor.validated_head, canonical_at_head
        ));
    }

    if let Some(stranded) = store
        .canonical_hash(cursor.validated_head.number + 1)
        .map_err(|e| e.to_string())?
    {
        return Err(format!(
            "canonical entry {} at #{} sits above the head {}",
            stranded,
            cursor.validated_head.number + 1,
            cursor.validated_head
        ));
    }

    if cursor.executed.number > cursor.validated_head.number {
        return Err(format!(
            "execution at {} is above the head {}",
            cursor.executed, cursor.validated_head
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeferredHead;
    use alloy_primitives::{Address, Bytes, U256};
    use tempfile::tempdir;

    fn header(number: u64, parent: B256, salt: u64) -> Header {
        Header {
            number,
            parent_hash: parent,
            ommers_hash: B256::ZERO,
            beneficiary: Address::ZERO,
            state_root: B256::ZERO,
            transactions_root: B256::ZERO,
            receipts_root: B256::ZERO,
            logs_bloom: Default::default(),
            extension_data: None,
            difficulty: U256::from(1),
            gas_limit: U256::from(8_000_000),
            gas_used: 0,
            timestamp: number * 15 + salt,
            extra_data: Bytes::default(),
            paid_fees: U256::ZERO,
            minimum_gas_price: U256::ZERO,
            uncle_count: 0,
            umm_root: None,
            version_fields: None,
            bitcoin_merged_mining_header: None,
            bitcoin_merged_mining_merkle_proof: None,
            bitcoin_merged_mining_coinbase_transaction: None,
            cached_hash: None,
            cached_hash_for_merged_mining: None,
            kept: None,
        }
    }

    /// A coherent chain 0..=len, head and executed head at the top.
    fn chain(len: u64) -> (BlockStore, tempfile::TempDir, Vec<Header>) {
        let dir = tempdir().unwrap();
        let store = BlockStore::open(dir.path()).unwrap();
        let mut hs = Vec::new();
        let mut parent = B256::ZERO;
        for n in 0..=len {
            let h = header(n, parent, 0);
            store.update_head(&h, U256::from(n + 1)).unwrap();
            parent = h.hash();
            hs.push(h);
        }
        let top = hs.last().unwrap();
        store.set_exec_head(top.hash(), B256::ZERO).unwrap();
        (store, dir, hs)
    }

    /// Store a header without touching any position key.
    fn stash(store: &BlockStore, h: &Header) {
        store.put_header_with_hash(h.hash(), h).unwrap();
    }

    // -- Validated::prove ------------------------------------------------

    #[test]
    fn proving_a_block_that_is_already_canonical_needs_no_lineage() {
        let (store, _d, hs) = chain(10);
        let v = Validated::prove(&store, hs[10].hash()).unwrap();
        assert_eq!(v.number(), 10);
        assert!(v.lineage().is_empty(), "rewrote entries that were already right");
    }

    #[test]
    fn proving_a_fork_tip_collects_the_whole_fork() {
        let (store, _d, hs) = chain(10);
        // A fork from #7: 8', 9', 10'.
        let a = header(8, hs[7].hash(), 99);
        let b = header(9, a.hash(), 99);
        let c = header(10, b.hash(), 99);
        for h in [&a, &b, &c] {
            stash(&store, h);
        }

        let v = Validated::prove(&store, c.hash()).unwrap();
        let lineage: Vec<u64> = v.lineage().iter().map(|(n, _)| *n).collect();
        assert_eq!(lineage, vec![8, 9, 10], "lineage must be ascending and complete");
        assert_eq!(v.lineage()[0].1, a.hash());
    }

    /// Stall 5, at the type level. The old `ensure_canonical_lineage` wrote
    /// nothing and returned `Ok` when a header was missing, so the canonical
    /// pointer kept naming a block the node did not have. Here it cannot even
    /// be proved, so it cannot be adopted.
    #[test]
    fn a_block_whose_ancestor_is_missing_cannot_be_proved() {
        let (store, _d, hs) = chain(10);
        // #11 links to a #10' we never downloaded.
        let phantom_parent = B256::repeat_byte(0x5a);
        let orphan = header(11, phantom_parent, 0);
        stash(&store, &orphan);

        let err = Validated::prove(&store, orphan.hash()).unwrap_err();
        assert!(
            matches!(err, LineageBreak::MissingHeader { at: 10, .. }),
            "expected a located missing-header break, got {err:?}"
        );
        let _ = hs;
    }

    #[test]
    fn a_block_we_do_not_hold_at_all_cannot_be_proved() {
        let (store, _d, _) = chain(10);
        let err = Validated::prove(&store, B256::repeat_byte(0xaa)).unwrap_err();
        assert!(matches!(err, LineageBreak::MissingHeader { .. }));
    }

    /// Stall 2: a walk truncated by a then-missing header leaves a lower
    /// height stale while a higher one is right. Stopping at "already
    /// canonical" would miss it, so the proof keeps walking until the entry
    /// *below* really is this block's parent.
    #[test]
    fn proving_repairs_a_stale_entry_buried_under_a_correct_one() {
        let (store, _d, hs) = chain(10);
        // #10 is correct; #9 names a sibling. The tip looks fine and is not.
        let sibling = header(9, hs[8].hash(), 77);
        stash(&store, &sibling);
        store.put_canonical_hash(9, sibling.hash()).unwrap();

        let v = Validated::prove(&store, hs[10].hash()).unwrap();
        let repaired: Vec<u64> = v.lineage().iter().map(|(n, _)| *n).collect();
        assert_eq!(repaired, vec![9], "the buried stale entry was not repaired");
    }

    /// The limit of an incremental proof, stated so nobody mistakes it for a
    /// guarantee: the walk stops at the first height where the chain below is
    /// already consistent, so damage deeper than that is invisible from the
    /// tip. That is what `repair_canonical_lineage(hash, depth)` is for, and
    /// what the full coherence sweep is for.
    ///
    /// This is not a defect. An incremental walk that did not stop would
    /// re-verify the whole chain on every block.
    #[test]
    fn proving_does_not_look_below_the_first_consistent_height() {
        let (store, _d, hs) = chain(10);
        // Damage at #5, with #6..#10 internally consistent above it.
        let sibling = header(5, hs[4].hash(), 77);
        stash(&store, &sibling);
        store.put_canonical_hash(5, sibling.hash()).unwrap();

        let v = Validated::prove(&store, hs[10].hash()).unwrap();
        assert!(
            v.lineage().is_empty(),
            "an incremental proof is not a full sweep; it must stop at #9"
        );
    }

    // -- Transition::Adopt -----------------------------------------------

    #[test]
    fn adopting_writes_the_lineage_and_the_head_together() {
        let (store, _d, hs) = chain(10);
        let a = header(8, hs[7].hash(), 99);
        let b = header(9, a.hash(), 99);
        for h in [&a, &b] {
            stash(&store, h);
        }

        let v = Validated::prove(&store, b.hash()).unwrap();
        let cursor = store.apply(&Transition::Adopt { head: v }).unwrap();

        assert_eq!(cursor.validated_head.hash, b.hash());
        assert_eq!(store.head().unwrap(), Some(b.hash()));
        assert_eq!(store.canonical_hash(8).unwrap(), Some(a.hash()));
        assert_eq!(store.canonical_hash(9).unwrap(), Some(b.hash()));
    }

    /// Stall 6, at the type level.
    ///
    /// The whole class is "canonical entries written above the head". Adopting
    /// a shorter chain must clear what the longer one left, and there is no
    /// API that writes canonical entries without saying what the head becomes.
    #[test]
    fn adopting_a_shorter_chain_drops_what_the_longer_one_left_above() {
        let (store, _d, hs) = chain(10);
        // Reorg to a fork tip at #9.
        let a = header(8, hs[7].hash(), 99);
        let b = header(9, a.hash(), 99);
        for h in [&a, &b] {
            stash(&store, h);
        }

        let v = Validated::prove(&store, b.hash()).unwrap();
        store.apply(&Transition::Adopt { head: v }).unwrap();

        assert_eq!(store.canonical_hash(10).unwrap(), None, "#10 was left stranded above the head");
        assert_eq!(store.head().unwrap(), Some(b.hash()));
    }

    #[test]
    fn adopting_leaves_the_store_locally_coherent() {
        let (store, _d, hs) = chain(10);
        let a = header(8, hs[7].hash(), 99);
        let b = header(9, a.hash(), 99);
        for h in [&a, &b] {
            stash(&store, h);
        }
        // Execution must not be left above the new head either.
        store.set_exec_head(hs[7].hash(), B256::ZERO).unwrap();

        let v = Validated::prove(&store, b.hash()).unwrap();
        let cursor = store.apply(&Transition::Adopt { head: v }).unwrap();
        verify_local_coherence(&store, &cursor).expect("adopt left the store incoherent");
    }

    // -- Transition::Retreat ---------------------------------------------

    #[test]
    fn retreating_drops_every_canonical_entry_above_the_new_head() {
        let (store, _d, hs) = chain(10);
        let v = Validated::prove(&store, hs[6].hash()).unwrap();
        let cursor = store.apply(&Transition::Retreat { to: v }).unwrap();

        assert_eq!(cursor.validated_head.number, 6);
        for n in 7..=10 {
            assert_eq!(store.canonical_hash(n).unwrap(), None, "#{n} survived the retreat");
        }
        assert_eq!(store.canonical_hash(6).unwrap(), Some(hs[6].hash()));
    }

    // -- Transition::Executed --------------------------------------------

    #[test]
    fn recording_execution_moves_only_the_executed_head() {
        let (store, _d, hs) = chain(10);
        let head_before = store.head().unwrap();
        let v = Validated::prove(&store, hs[4].hash()).unwrap();
        let root = B256::repeat_byte(0x33);
        let cursor = store.apply(&Transition::Executed { at: v, state_root: root }).unwrap();

        assert_eq!(cursor.executed.number, 4);
        assert_eq!(cursor.state_root, root);
        assert_eq!(store.head().unwrap(), head_before, "execution moved the download head");
    }

    // -- Cursor ----------------------------------------------------------

    #[test]
    fn the_cursor_reads_all_three_keys() {
        let (store, _d, hs) = chain(10);
        store.set_exec_head(hs[6].hash(), B256::repeat_byte(0x77)).unwrap();
        let c = store.cursor().unwrap().unwrap();
        assert_eq!(c.validated_head.number, 10);
        assert_eq!(c.executed.number, 6);
        assert_eq!(c.state_root, B256::repeat_byte(0x77));
        assert_eq!(c.backlog(), 4);
    }

    #[test]
    fn an_empty_store_has_no_cursor() {
        let dir = tempdir().unwrap();
        let store = BlockStore::open(dir.path()).unwrap();
        assert!(store.cursor().unwrap().is_none());
    }

    /// Every transition, from every starting shape, must leave the store
    /// locally coherent. This is the property the seven stalls all broke.
    #[test]
    fn every_transition_leaves_the_store_coherent() {
        for retreat_to in [0u64, 3, 9, 10] {
            let (store, _d, hs) = chain(10);
            let v = Validated::prove(&store, hs[retreat_to as usize].hash()).unwrap();
            let c = store.apply(&Transition::Retreat { to: v }).unwrap();
            // Execution may be left above a retreat; that is the caller's job
            // to fix, but the canonical relations must hold regardless.
            assert_eq!(
                store.canonical_hash(c.validated_head.number).unwrap(),
                Some(c.validated_head.hash)
            );
            assert_eq!(store.canonical_hash(c.validated_head.number + 1).unwrap(), None);
        }
    }

    // -- Execution that never happened ------------------------------------

    /// A coherent chain 0..=len and no executed head: what a node holds before
    /// it has executed anything, as a light node does until its snapshot sync
    /// finishes.
    fn chain_never_executed(len: u64) -> (BlockStore, tempfile::TempDir, Vec<Header>) {
        let dir = tempdir().unwrap();
        let store = BlockStore::open(dir.path()).unwrap();
        let mut hs = Vec::new();
        let mut parent = B256::ZERO;
        for n in 0..=len {
            let h = header(n, parent, 0);
            store.update_head(&h, U256::from(n + 1)).unwrap();
            parent = h.hash();
            hs.push(h);
        }
        assert_eq!(
            store.exec_head().unwrap(),
            None,
            "precondition: nothing executed"
        );
        (store, dir, hs)
    }

    /// A retreat on a node that has executed nothing leaves it having executed
    /// nothing. It used to record the retreat's target as executed: on
    /// mainnet, a block with no state under it, which even a restart could not
    /// undo.
    #[test]
    fn retreating_a_node_that_executed_nothing_records_no_execution() {
        let (store, _d, hs) = chain_never_executed(10);
        let v = Validated::prove(&store, hs[9].hash()).unwrap();
        let cursor = store.apply(&Transition::Retreat { to: v }).unwrap();

        assert_eq!(
            cursor.validated_head.number, 9,
            "the retreat itself happens"
        );
        assert_eq!(
            store.exec_head().unwrap(),
            None,
            "a retreat recorded as executed a block nothing executed"
        );
    }

    /// The same for a reorg: adopting a fork records no execution, at the fork
    /// point or anywhere else.
    #[test]
    fn adopting_a_fork_on_a_node_that_executed_nothing_records_no_execution() {
        let (store, _d, hs) = chain_never_executed(10);
        // A fork from #4: 5', 6', 7'.
        let a = header(5, hs[4].hash(), 99);
        let b = header(6, a.hash(), 99);
        let c = header(7, b.hash(), 99);
        for h in [&a, &b, &c] {
            stash(&store, h);
        }
        let v = Validated::prove(&store, c.hash()).unwrap();
        assert_eq!(
            v.lineage()[0].0,
            5,
            "precondition: the fork rewrites from #5"
        );

        let cursor = store.apply(&Transition::Adopt { head: v }).unwrap();

        assert_eq!(cursor.validated_head.hash, c.hash(), "the fork is adopted");
        assert_eq!(
            store.exec_head().unwrap(),
            None,
            "adopting a fork recorded execution the node never did"
        );
    }

    /// Execution only comes down. When the executed block was off the chain
    /// before a transition that rewrites only above it, moving execution to the
    /// canonical block up there would claim a state never computed. It stays
    /// where it is, for the sync layer's repair (I5), which can see what state
    /// is held.
    #[test]
    fn execution_already_off_the_chain_is_not_moved_up() {
        let (store, _d, hs) = chain(10);
        // Executed at a sibling of #6, which is not canonical.
        let sibling = header(6, hs[5].hash(), 99);
        stash(&store, &sibling);
        let root = B256::repeat_byte(0x66);
        store.set_exec_head(sibling.hash(), root).unwrap();

        // One block on top of the chain: the transition rewrites #11 alone.
        let next = header(11, hs[10].hash(), 0);
        stash(&store, &next);
        let v = Validated::prove(&store, next.hash()).unwrap();
        store.apply(&Transition::Adopt { head: v }).unwrap();

        assert_eq!(
            store.exec_head().unwrap(),
            Some((sibling.hash(), root)),
            "execution was moved off the block it ran at, to one it never ran"
        );
    }

    // -- A heavier branch is adopted once it is executed ---------------------

    /// The blocks this node executed, as the check the sync service installs
    /// reads them from receipts and the trie store: here a set, starting with
    /// `chain`.
    fn executed_set(
        store: &BlockStore,
        chain: &[Header],
    ) -> std::sync::Arc<std::sync::Mutex<std::collections::HashSet<B256>>> {
        let set = std::sync::Arc::new(std::sync::Mutex::new(
            chain
                .iter()
                .map(|h| h.hash())
                .collect::<std::collections::HashSet<_>>(),
        ));
        let seen = set.clone();
        store.set_executed_check(std::sync::Arc::new(move |hash, _| {
            seen.lock().unwrap().contains(&hash)
        }));
        set
    }

    /// A branch off `hs[7]`, #8' to #12', with total difficulties 9 to 13: #10'
    /// is level with the chain's #10 (11), and #11' outweighs it.
    fn branch_from_seven(hs: &[Header]) -> Vec<(Header, U256)> {
        let mut branch = Vec::new();
        let mut parent = hs[7].hash();
        for n in 8..=12u64 {
            let h = header(n, parent, 99);
            parent = h.hash();
            branch.push((h, U256::from(n + 1)));
        }
        branch
    }

    /// Headers arriving as the sync manager stores them, weighed against the
    /// head.
    fn ingest(store: &BlockStore, entries: &[(Header, U256)]) {
        let head = store.head().unwrap();
        let td = head
            .and_then(|h| store.total_difficulty(h).unwrap())
            .unwrap_or_default();
        let refs: Vec<(&Header, U256)> = entries.iter().map(|(h, td)| (h, *td)).collect();
        store.store_headers_batch(&refs, head, td).unwrap();
    }

    /// rskj makes a block its best only once it has executed it, so a heavier
    /// branch known by its headers alone leaves the node where it is. Taken on
    /// its headers, it pulled execution back to the fork point, and with its
    /// bodies withheld it would have held the node there.
    #[test]
    fn a_heavier_branch_waits_until_it_is_executed_past_the_executed_head() {
        let (store, _d, hs) = chain(10);
        let executed = executed_set(&store, &hs);
        let branch = branch_from_seven(&hs);

        ingest(&store, &branch[..4]);
        assert_eq!(
            store.head().unwrap(),
            Some(hs[10].hash()),
            "the head moved to a branch this node has not executed"
        );
        assert_eq!(store.canonical_hash(10).unwrap(), Some(hs[10].hash()));
        assert_eq!(
            store.exec_head().unwrap().map(|(h, _)| h),
            Some(hs[10].hash()),
            "execution was pulled back to the fork point"
        );
        assert_eq!(store.deferred_head(), Some(branch[3].0.hash()));

        // Executed through #10': level with #10, and with no fees on either
        // side a tie keeps the block held first.
        executed
            .lock()
            .unwrap()
            .extend(branch[..3].iter().map(|(h, _)| h.hash()));
        assert_eq!(store.retry_deferred_head().unwrap(), DeferredHead::Waiting);
        assert_eq!(store.head().unwrap(), Some(hs[10].hash()));

        // #11' executed: it outweighs #10, so the branch is the chain now, with
        // execution at its tip and nothing to run again.
        executed.lock().unwrap().insert(branch[3].0.hash());
        assert_eq!(store.retry_deferred_head().unwrap(), DeferredHead::Adopted);
        assert_eq!(store.head().unwrap(), Some(branch[3].0.hash()));
        assert_eq!(store.canonical_hash(10).unwrap(), Some(branch[2].0.hash()));
        assert_eq!(
            store.exec_head().unwrap(),
            Some((branch[3].0.hash(), branch[3].0.state_root))
        );
        assert_eq!(store.deferred_head(), None);
    }

    /// A switch to a branch executed beside the chain as it arrived takes
    /// execution to that branch's executed tip, not back to the fork point to
    /// run its blocks a second time.
    #[test]
    fn a_switch_takes_execution_to_the_executed_tip_of_the_new_branch() {
        let (store, _d, hs) = chain(10);
        let executed = executed_set(&store, &hs);
        let branch = branch_from_seven(&hs);
        // #8' to #11' were executed as they arrived; #12' not yet.
        executed
            .lock()
            .unwrap()
            .extend(branch[..4].iter().map(|(h, _)| h.hash()));

        ingest(&store, &branch);

        assert_eq!(store.head().unwrap(), Some(branch[4].0.hash()));
        assert_eq!(
            store.exec_head().unwrap().map(|(h, _)| h),
            Some(branch[3].0.hash()),
            "execution went back to the fork point, to run again blocks it ran already"
        );
    }

    /// A change of head that takes execution off its branch is recorded as a
    /// switch, from the executed head it found; execution recording its own
    /// progress is no switch. A batch records its progress on a thread of its
    /// own while the sync service stores headers, and the service takes up
    /// the switches and nothing else.
    #[test]
    fn only_a_change_of_head_that_moves_execution_is_a_switch() {
        let (store, _d, hs) = chain(10);
        let executed = executed_set(&store, &hs);
        store.set_exec_head(hs[7].hash(), B256::ZERO).unwrap();

        let mark = store.switch_mark();
        for h in &hs[8..=9] {
            let at = Validated::prove(&store, h.hash()).unwrap();
            store
                .apply(&Transition::Executed {
                    at,
                    state_root: B256::ZERO,
                })
                .unwrap();
        }
        assert_eq!(
            store.take_switches_since(mark),
            None,
            "execution recording its progress was taken for a switch"
        );

        // A branch off #7, executed as it came, outweighs the chain.
        let mark = store.switch_mark();
        let branch = branch_from_seven(&hs);
        executed
            .lock()
            .unwrap()
            .extend(branch[..4].iter().map(|(h, _)| h.hash()));
        ingest(&store, &branch);
        assert_eq!(
            store.take_switches_since(mark),
            Some(ExecutionSwitch {
                from: hs[9].hash(),
                to: branch[3].0.hash(),
                state_root: branch[3].0.state_root,
            }),
            "the switch is from the block execution had got to"
        );
        assert_eq!(
            store.take_switches_since(mark),
            None,
            "a switch is taken up once"
        );
    }

    /// A node with no check installed executes nothing it could run a branch
    /// with, and adopts on headers as it always has.
    #[test]
    fn a_node_without_the_check_adopts_on_headers_as_before() {
        let (store, _d, hs) = chain(10);
        let branch = branch_from_seven(&hs);

        ingest(&store, &branch[..4]);

        assert_eq!(store.head().unwrap(), Some(branch[3].0.hash()));
        assert_eq!(
            store.exec_head().unwrap().map(|(h, _)| h),
            Some(hs[7].hash())
        );
        assert_eq!(store.deferred_head(), None);
    }

    /// A branch that waits is forgotten once the chain the node is on
    /// outgrows it.
    #[test]
    fn a_waiting_branch_the_chain_outgrows_is_forgotten() {
        let (store, _d, hs) = chain(10);
        let _executed = executed_set(&store, &hs);
        let branch = branch_from_seven(&hs);
        ingest(&store, &branch[..4]);
        assert_eq!(store.deferred_head(), Some(branch[3].0.hash()));

        // The chain grows to #12, total difficulty 13 against #11''s 12.
        let mut ours = Vec::new();
        let mut parent = hs[10].hash();
        for n in 11..=12u64 {
            let h = header(n, parent, 0);
            parent = h.hash();
            ours.push((h, U256::from(n + 1)));
        }
        ingest(&store, &ours);

        assert_eq!(store.head().unwrap(), Some(ours[1].0.hash()));
        assert_eq!(store.retry_deferred_head().unwrap(), DeferredHead::Dropped);
        assert_eq!(store.deferred_head(), None);
    }

    /// A block whose total rose once its body showed its uncles, which its
    /// header alone could not, takes the head when weighed again.
    #[test]
    fn a_block_weighed_again_takes_the_head_once_it_outweighs_it() {
        let (store, _d, hs) = chain(10);
        let executed = executed_set(&store, &hs);
        // A sibling of #10, level with it on its header alone.
        let sibling = header(10, hs[9].hash(), 99);
        ingest(&store, &[(sibling.clone(), U256::from(11))]);
        assert_eq!(store.head().unwrap(), Some(hs[10].hash()));

        // Executed, and its uncle counted: 12 now.
        store
            .put_total_difficulty(sibling.hash(), U256::from(12))
            .unwrap();
        executed.lock().unwrap().insert(sibling.hash());

        assert!(store.reconsider_head(sibling.hash()).unwrap());
        assert_eq!(store.head().unwrap(), Some(sibling.hash()));
        assert_eq!(
            store.exec_head().unwrap().map(|(h, _)| h),
            Some(sibling.hash())
        );
    }
}
