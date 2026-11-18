//! Forks met the way rskj meets them, at the level of the sync service.
//!
//! Every test here fails on the code before `beside` existed: there a
//! competing block was not executed until the chain switched to it, a heavier
//! branch was adopted on its headers alone, and execution was pulled back to
//! the fork point to run the new branch again on a later tick.

use super::{Beside, BesideFailure};
use crate::manager::SyncManager;
use crate::service::SyncService;
use crate::state::SyncState;
use crate::tests::dummy_header;
use alloy_primitives::{B256, B512, U256};
use rustock_core::events::ChainEvent;
use rustock_core::types::header::Header;
use rustock_core::validation::HeaderVerifier;
use rustock_core::Block;
use rustock_networking::protocol::{P2pMessage, RskMessage, RskSubMessage};
use rustock_storage::BlockStore;
use rustock_trie::{MemoryTrieStore, TrieNode, TrieStore};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const PEER: B512 = B512::repeat_byte(0x01);

/// A node that executes blocks, with one peer, and a stand-in for the block
/// processor that counts what it runs.
struct Node {
    service: SyncService,
    store: Arc<BlockStore>,
    /// How many times each block was executed beside the chain.
    runs: Arc<Mutex<HashMap<B256, usize>>>,
    /// Blocks the stand-in refuses as breaking consensus.
    invalid: Arc<Mutex<HashSet<B256>>>,
    /// Blocks the stand-in fails to run as a local fault would, and how many
    /// more times.
    local_faults: Arc<Mutex<HashMap<B256, usize>>>,
    /// Blocks counted as executed here: those the stand-in ran, and those a
    /// test puts in as run by the miner.
    executed: Arc<Mutex<HashSet<B256>>>,
    /// What the node sent its peer.
    sent: mpsc::Receiver<P2pMessage>,
    /// What it told subscribers.
    events: tokio::sync::broadcast::Receiver<ChainEvent>,
    /// Every best block the miner was told of, in order.
    best: Arc<Mutex<Vec<B256>>>,
    /// The state root every header here names: the empty trie's, held.
    root: B256,
    _dir: tempfile::TempDir,
}

async fn node() -> Node {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(BlockStore::open(dir.path()).unwrap());
    node_over(dir, store).await
}

/// [`node`], over `store`, which keeps its data in `dir`.
async fn node_over(dir: tempfile::TempDir, store: Arc<BlockStore>) -> Node {
    let trie = Arc::new(MemoryTrieStore::new()) as Arc<dyn TrieStore>;
    let empty = TrieNode::empty();
    let root = empty.compute_hash(trie.as_ref());
    trie.put(root.as_slice(), &empty.to_message(trie.as_ref()));

    let peer_store = Arc::new(rustock_networking::peers::PeerStore::new());
    let (to_peer, sent) = mpsc::channel(rustock_networking::peers::PEER_CHANNEL_CAPACITY);
    peer_store.add_peer(PEER, to_peer).await;
    let manager = Arc::new(SyncManager::new(
        store.clone(),
        Arc::new(HeaderVerifier::new()),
        peer_store.clone(),
    ));
    let (_event_tx, event_rx) = mpsc::unbounded_channel();
    let processor = rustock_execution::BlockProcessor::new(
        rustock_execution::RskHardforkConfig::mainnet(),
        store.clone(),
    );
    let events = rustock_core::events::channel();
    let listeners = crate::BestBlockListeners::new();
    let best = Arc::new(Mutex::new(Vec::new()));
    let told = best.clone();
    listeners.add(Arc::new(move |header: &Header| {
        told.lock().unwrap().push(header.hash())
    }));
    let mut service = SyncService::new(manager, peer_store, event_rx)
        .with_block_processor(processor, trie, empty)
        .with_events(Some(events.clone()))
        .with_best_block_listeners(listeners);

    let runs = Arc::new(Mutex::new(HashMap::new()));
    let invalid = Arc::new(Mutex::new(HashSet::new()));
    let local_faults = Arc::new(Mutex::new(HashMap::new()));
    let (counted, refused, faulty) = (runs.clone(), invalid.clone(), local_faults.clone());
    let executed = service.set_test_beside(move |block: &Block| {
        let hash = block.hash();
        *counted.lock().unwrap().entry(hash).or_insert(0) += 1;
        if refused.lock().unwrap().contains(&hash) {
            return Err(BesideFailure::Invalid("refused by the test".into()));
        }
        if let Some(left) = faulty
            .lock()
            .unwrap()
            .get_mut(&hash)
            .filter(|left| **left > 0)
        {
            *left -= 1;
            return Err(BesideFailure::Local(
                "a database fault, made by the test".into(),
            ));
        }
        Ok(())
    });
    service.state = SyncState::Following;
    Node {
        service,
        store,
        runs,
        invalid,
        local_faults,
        executed,
        sent,
        events: events.subscribe(),
        best,
        root,
        _dir: dir,
    }
}

impl Node {
    /// A header naming the one state root the trie holds; `mark` tells
    /// siblings apart.
    fn header(&self, number: u64, parent: B256, difficulty: u64, mark: u8) -> Header {
        let mut header = dummy_header(number, parent, U256::from(difficulty));
        header.state_root = self.root;
        header.extra_data = vec![mark].into();
        header
    }

    /// Genesis and the blocks above it, of the difficulties given, canonical
    /// with their totals and executed to the top. Genesis first.
    fn chain(&self, difficulties: &[u64]) -> Vec<Header> {
        let genesis = self.header(0, B256::ZERO, 1, 0);
        let mut total = 1u64;
        self.store.update_head(&genesis, U256::from(total)).unwrap();
        let mut chain = vec![genesis];
        for (i, difficulty) in difficulties.iter().enumerate() {
            let parent = chain.last().unwrap().hash();
            let header = self.header(i as u64 + 1, parent, *difficulty, 0xA0);
            total += difficulty;
            self.store.update_head(&header, U256::from(total)).unwrap();
            chain.push(header);
        }
        let top = chain.last().unwrap().hash();
        self.store.set_exec_head(top, self.root).unwrap();
        chain
    }

    /// A peer sends `header` as a whole block, with an empty body.
    async fn new_block(&mut self, header: &Header) {
        let block = Block {
            header: header.clone(),
            transactions: Vec::new(),
            ommers: Vec::new(),
        };
        self.service.on_new_block(PEER, block).await;
    }

    /// This node's miner solved `header`, an empty block whose parent is no
    /// longer the head, and kept it beside the chain: stored with its total
    /// and executed against its parent's state, its own run counted as done.
    fn mined_beside(&self, header: &Header, total: u64) {
        let hash = header.hash();
        self.store.put_header_with_hash(hash, header).unwrap();
        self.store.put_body(hash, &[], &[]).unwrap();
        self.store
            .put_total_difficulty(hash, U256::from(total))
            .unwrap();
        self.executed.lock().unwrap().insert(hash);
    }

    fn executed_head(&self) -> Option<B256> {
        self.store.exec_head().unwrap().map(|(hash, _)| hash)
    }

    fn head(&self) -> Option<B256> {
        self.store.head().unwrap()
    }

    fn runs(&self, hash: B256) -> usize {
        self.runs.lock().unwrap().get(&hash).copied().unwrap_or(0)
    }

    /// The hashes of the bodies and the headers the node has asked its peer
    /// for since last asked.
    fn asked(&mut self) -> (Vec<B256>, Vec<(B256, u32)>) {
        let (mut bodies, mut headers) = (Vec::new(), Vec::new());
        while let Ok(message) = self.sent.try_recv() {
            match message {
                P2pMessage::RskMessage(RskMessage {
                    sub_message: RskSubMessage::BodyRequest(request),
                    ..
                }) => bodies.push(request.hash),
                P2pMessage::RskMessage(RskMessage {
                    sub_message: RskSubMessage::BlockHeadersRequest(request),
                    ..
                }) => headers.push((request.query.hash, request.query.count)),
                _ => {}
            }
        }
        (bodies, headers)
    }

    /// The blocks announced to subscribers since last asked: each hash, and
    /// whether it was retracted.
    fn announced(&mut self) -> Vec<(B256, bool)> {
        let mut announced = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            if let ChainEvent::Block(block) = event {
                announced.push((block.hash, block.removed));
            }
        }
        announced
    }
}

/// rskj connects a competing block as it arrives, against its own parent's
/// state (`BlockChainImpl.internalTryToConnect`), and keeps its best block
/// while the competitor is lighter. Before, the block was stored for a miner
/// to name as an uncle and nothing more.
#[tokio::test]
async fn a_competing_block_is_executed_on_arrival_and_not_adopted_while_lighter() {
    let mut node = node().await;
    let chain = node.chain(&[1, 2]); // totals 2 and 4
    let (a1, a2) = (chain[1].hash(), chain[2].hash());

    let b2 = node.header(2, a1, 1, 0xB0); // total 3
    node.new_block(&b2).await;

    assert_eq!(
        node.runs(b2.hash()),
        1,
        "the competing block is executed as it arrives"
    );
    assert_eq!(
        node.head(),
        Some(a2),
        "and, lighter, it does not take the head"
    );
    assert_eq!(node.executed_head(), Some(a2), "nor execution");
    assert_eq!(node.store.canonical_hash(2).unwrap(), Some(a2));
    assert!(
        node.best.lock().unwrap().is_empty(),
        "the miner's best block did not change"
    );
}

/// A heavier branch whose blocks were executed as they came is switched to the
/// moment it outweighs the chain: in the call that delivers the block that
/// tips it, with nothing run again and no tick, grace period or download in
/// between. The miner and subscribers hear of it then too: the old block
/// retracted first, as rskj's logs emitter retracts it.
///
/// Before, the switch pulled execution back to the fork point, and the new
/// branch was run there once a later tick had fetched it.
#[tokio::test]
async fn a_heavier_branch_executed_as_it_came_is_switched_to_at_once() {
    let mut node = node().await;
    let chain = node.chain(&[1, 2]); // totals 2 and 4
    let (a1, a2) = (chain[1].hash(), chain[2].hash());
    let b2 = node.header(2, a1, 1, 0xB0); // total 3
    let b3 = node.header(3, b2.hash(), 2, 0xB0); // total 5

    node.new_block(&b2).await;
    node.new_block(&b3).await;

    assert_eq!(node.head(), Some(b3.hash()));
    assert_eq!(
        node.store.exec_head().unwrap(),
        Some((b3.hash(), node.root)),
        "execution is at the new tip, not the fork point"
    );
    assert_eq!(node.store.canonical_hash(2).unwrap(), Some(b2.hash()));
    assert_eq!(node.store.canonical_hash(3).unwrap(), Some(b3.hash()));
    assert_eq!(
        (node.runs(b2.hash()), node.runs(b3.hash())),
        (1, 1),
        "each block of the new branch ran once, as it arrived, and not again on the switch"
    );
    assert_eq!(node.store.deferred_head(), None);
    assert_eq!(
        *node.best.lock().unwrap(),
        vec![b3.hash()],
        "the miner builds on the new tip"
    );
    assert_eq!(
        node.announced(),
        vec![(a2, true), (b2.hash(), false), (b3.hash(), false)],
        "the block left is retracted, then the branch taken up is announced"
    );
}

/// A block this node mined on a parent that is no longer the head is kept
/// beside the chain by the miner, run against its parent's state, as rskj
/// connects any block (`BlockChainImpl.internalTryToConnect`). Told of it,
/// the sync service weighs it as it weighs a peer's block executed beside the
/// chain: a lighter one stays beside it, and a heavier one is switched to at
/// once, with nothing run again, the block it displaces retracted and the
/// miner told of its new best block.
#[tokio::test]
async fn a_mined_block_kept_beside_the_chain_is_switched_to_when_heavier() {
    let mut node = node().await;
    let chain = node.chain(&[1, 2]); // totals 2 and 4
    let (a1, a2) = (chain[1].hash(), chain[2].hash());

    let lighter = node.header(2, a1, 1, 0xC0); // total 3
    node.mined_beside(&lighter, 3);
    node.service
        .handle_event(crate::SyncEvent::MinedBlock {
            hash: lighter.hash(),
        })
        .await;
    assert_eq!(
        node.head(),
        Some(a2),
        "a lighter mined block stays beside the chain"
    );
    assert_eq!(node.store.exec_head().unwrap(), Some((a2, node.root)));
    assert_eq!(node.store.canonical_hash(2).unwrap(), Some(a2));
    assert!(node.best.lock().unwrap().is_empty());
    assert!(node.announced().is_empty());

    let heavier = node.header(2, a1, 3, 0xC1); // total 5
    node.mined_beside(&heavier, 5);
    node.service
        .handle_event(crate::SyncEvent::MinedBlock {
            hash: heavier.hash(),
        })
        .await;
    assert_eq!(node.head(), Some(heavier.hash()));
    assert_eq!(
        node.store.exec_head().unwrap(),
        Some((heavier.hash(), node.root)),
        "execution is at the mined block, taken up without running it again"
    );
    assert_eq!(node.store.canonical_hash(2).unwrap(), Some(heavier.hash()));
    assert_eq!(
        (node.runs(lighter.hash()), node.runs(heavier.hash())),
        (0, 0),
        "the miner ran them; nothing runs them again"
    );
    assert_eq!(*node.best.lock().unwrap(), vec![heavier.hash()]);
    assert_eq!(
        node.announced(),
        vec![(a2, true), (heavier.hash(), false)],
        "the block left is retracted, then the mined block is announced"
    );
}

/// A block this node mined beside the chain leaves the peg decisions it made
/// that the Bridge logs no event for where a peer's block executed beside the
/// chain leaves its own (`SyncService::unlogged_holder`): the switch to it
/// takes them out, to store under its height, and those of a block the chain
/// doesn't take wait. A block that made none leaves nothing.
#[tokio::test]
async fn a_mined_block_kept_beside_the_chain_leaves_its_peg_decisions_for_the_switch() {
    use rustock_execution::silent::{SilentKind, SilentRecord};

    let mut node = node().await;
    let chain = node.chain(&[1, 2]); // totals 2 and 4
    let (a1, a2) = (chain[1].hash(), chain[2].hash());
    let decision = SilentRecord {
        kind: SilentKind::BtcTxMarkedProcessed,
        block: 2,
        tx_index: 0,
        fields: Vec::new(),
    };
    let hold = node.service.unlogged_holder();

    let lighter = node.header(2, a1, 1, 0xC0); // total 3
    hold(lighter.hash(), 2, vec![decision.clone()]);
    node.mined_beside(&lighter, 3);
    node.service
        .handle_event(crate::SyncEvent::MinedBlock {
            hash: lighter.hash(),
        })
        .await;

    let heavier = node.header(2, a1, 3, 0xC1); // total 5
    hold(heavier.hash(), 2, vec![decision.clone()]);
    hold(a2, 2, Vec::new());
    node.mined_beside(&heavier, 5);
    node.service
        .handle_event(crate::SyncEvent::MinedBlock {
            hash: heavier.hash(),
        })
        .await;
    assert_eq!(node.head(), Some(heavier.hash()));
    let held: Vec<_> = node
        .service
        .beside_unlogged
        .lock()
        .unwrap()
        .clone()
        .into_iter()
        .collect();
    assert_eq!(
        held,
        vec![(lighter.hash(), (2, vec![decision]))],
        "the switch took the heavier block's out; the lighter block's wait"
    );
}

/// A chain heavier by its headers whose bodies never come leaves the head,
/// the canonical chain and execution where they are, as rskj's best block
/// stays on what it has connected, and the node goes on following its own
/// chain. The bodies are asked for. Before, the headers alone took the head
/// and pulled execution back to the fork point, where withheld bodies held
/// the node for good.
#[tokio::test]
async fn heavier_headers_without_bodies_move_neither_the_head_nor_execution() {
    let mut node = node().await;
    let chain = node.chain(&[1, 2]); // totals 2 and 4
    let (a1, a2) = (chain[1].hash(), chain[2].hash());
    let x2 = node.header(2, a1, 1, 0xC0); // total 3
    let x3 = node.header(3, x2.hash(), 9, 0xC0); // total 12

    node.service
        .on_headers_response(PEER, vec![x3.clone(), x2.clone()])
        .await;

    assert_eq!(node.head(), Some(a2), "headers alone do not take the head");
    assert_eq!(node.executed_head(), Some(a2), "nor pull execution back");
    assert_eq!(node.store.canonical_hash(2).unwrap(), Some(a2));
    assert_eq!(
        node.store.deferred_head(),
        Some(x3.hash()),
        "the branch is held back"
    );
    let (bodies, _) = node.asked();
    for wanted in [x2.hash(), x3.hash()] {
        assert!(
            bodies.contains(&wanted),
            "the bodies of the branch are asked for"
        );
    }

    // Its own chain goes on.
    let a3 = node.header(3, a2, 1, 0xA0); // total 5
    node.new_block(&a3).await;
    assert_eq!(node.head(), Some(a3.hash()));
    assert_eq!(node.store.canonical_hash(2).unwrap(), Some(a2));
}

/// A branch held back whose bodies never come is given up once it has made no
/// progress for a minute, so that withholding them cannot keep the node from
/// syncing.
#[tokio::test]
async fn a_held_back_branch_whose_bodies_never_come_is_given_up() {
    let mut node = node().await;
    let chain = node.chain(&[1, 2]);
    let (a1, a2) = (chain[1].hash(), chain[2].hash());
    let x2 = node.header(2, a1, 1, 0xC0);
    let x3 = node.header(3, x2.hash(), 9, 0xC0);
    node.service
        .on_headers_response(PEER, vec![x3.clone(), x2])
        .await;
    assert_eq!(node.store.deferred_head(), Some(x3.hash()));

    node.service.advance_deferred_branch().await;
    assert_eq!(
        node.store.deferred_head(),
        Some(x3.hash()),
        "not given up at once"
    );

    let a_minute_ago = Instant::now()
        .checked_sub(Duration::from_secs(61))
        .expect("the clock has run a minute");
    node.service.deferred_watch = Some((x3.hash(), a_minute_ago));
    node.service.advance_deferred_branch().await;
    assert_eq!(node.store.deferred_head(), None, "given up");
    assert_eq!(node.head(), Some(a2));
    assert_eq!(node.executed_head(), Some(a2));
}

/// A block that breaks consensus is never adopted, however heavy the branch
/// built on it, and is not executed twice. rskj connects no block that
/// fails. Before, the branch was adopted on its headers.
#[tokio::test]
async fn an_invalid_block_beside_the_chain_is_never_adopted_however_heavy_its_branch() {
    let mut node = node().await;
    let chain = node.chain(&[1, 2]);
    let (a1, a2) = (chain[1].hash(), chain[2].hash());
    let y2 = node.header(2, a1, 1, 0xD0);
    let y3 = node.header(3, y2.hash(), 9, 0xD0); // total 12
    node.invalid.lock().unwrap().insert(y2.hash());

    node.new_block(&y2).await;
    node.new_block(&y3).await;
    node.service.advance_deferred_branch().await;
    node.service.take_up_stored_successors().await;

    assert_eq!(node.head(), Some(a2));
    assert_eq!(node.executed_head(), Some(a2));
    assert_eq!(
        node.store.deferred_head(),
        None,
        "a branch through it is given up"
    );
    assert_eq!(node.runs(y2.hash()), 1, "the invalid block ran once");
    assert_eq!(node.runs(y3.hash()), 0, "its child never ran");
}

/// A block this node failed to run for a fault of its own -- its database,
/// not the block -- is tried again, and its branch adopted once it runs. It
/// is not taken for invalid.
#[tokio::test]
async fn a_block_that_failed_for_a_local_fault_is_tried_again() {
    let mut node = node().await;
    let chain = node.chain(&[1, 2]);
    let a1 = chain[1].hash();
    let y2 = node.header(2, a1, 1, 0xD0);
    let y3 = node.header(3, y2.hash(), 9, 0xD0);
    node.local_faults.lock().unwrap().insert(y2.hash(), 1);

    node.new_block(&y2).await;
    assert_eq!(node.runs(y2.hash()), 1);
    node.new_block(&y3).await;

    assert_eq!(node.runs(y2.hash()), 2, "tried again");
    assert_eq!(
        node.executed_head(),
        Some(y3.hash()),
        "and adopted once it ran"
    );
}

/// The mainnet wedge of 2026-09-18, which held a node for three days: it had
/// executed a2 on a branch the network left, and held the winning branch's
/// headers -- b2, b3 -- without having adopted them.
///
/// The reconcile used to adopt the stored branch for being there and pull
/// execution back to the fork point, b1. Now the branch is weighed as rskj
/// weighs one: its bodies are fetched, it is executed beside the chain, and
/// the store adopts it with execution at its tip.
#[tokio::test]
async fn a_branch_stored_by_headers_alone_is_executed_beside_the_chain_and_adopted_at_its_tip() {
    let mut node = node().await;
    let chain = node.chain(&[1, 1]); // b1 total 2, a2 total 3
    let (b1, a2) = (chain[1].hash(), chain[2].hash());
    let b2 = node.header(2, b1, 5, 0xBB);
    let b3 = node.header(3, b2.hash(), 5, 0xBB);
    // As the store held them: headers, by height, with no pointer and no
    // total.
    node.store.put_header(&b2).unwrap();
    node.store.put_header(&b3).unwrap();

    node.service.reconcile_exec_head_with_canonical().await;
    assert_eq!(
        node.store.canonical_hash(3).unwrap(),
        None,
        "not adopted on its headers"
    );
    assert_eq!(
        node.executed_head(),
        Some(a2),
        "and execution not pulled back to b1"
    );

    node.service.take_up_stored_successors().await;
    let (bodies, _) = node.asked();
    assert!(
        bodies.contains(&b2.hash()) && bodies.contains(&b3.hash()),
        "its bodies are asked for"
    );

    // The bodies come, the child's first.
    let mut requests: Vec<(u64, B256)> = node
        .service
        .pending_branch_bodies
        .iter()
        .map(|(id, (hash, ..))| (*id, *hash))
        .collect();
    requests.sort_by_key(|(_, hash)| *hash != b3.hash());
    for (id, _) in requests {
        node.service
            .on_body_response(PEER, id, Vec::new(), Vec::new(), None)
            .await;
    }

    assert_eq!(node.head(), Some(b3.hash()));
    assert_eq!(
        node.store.exec_head().unwrap(),
        Some((b3.hash(), node.root))
    );
    assert_eq!(node.store.canonical_hash(2).unwrap(), Some(b2.hash()));
    assert_eq!((node.runs(b2.hash()), node.runs(b3.hash())), (1, 1));
}

/// The mainnet stalls of 2026-09-21 and 2026-09-24: the node held a block of
/// the network's branch but not its parent -- one block missing, then three.
/// The reconcile used to roll execution and the head back below the fork so
/// that the range would be downloaded again. Now nothing is rolled back: the
/// headers the branch lacks are asked for, ending at the first one missing,
/// once, and when they come the branch's bodies are fetched to execute it
/// beside the chain.
#[tokio::test]
async fn a_block_whose_ancestry_was_never_downloaded_has_it_asked_for_and_nothing_rolled_back() {
    for depth in [1u64, 3] {
        let mut node = node().await;
        // A common chain to the fork point, then ours, `depth` blocks above it.
        let fork_number = 30;
        let mut difficulties = vec![1; fork_number as usize];
        difficulties.extend(std::iter::repeat_n(1, depth as usize));
        let chain = node.chain(&difficulties);
        let fork = chain[fork_number as usize].hash();
        let ours = chain.last().unwrap().hash();
        let top = fork_number + depth;

        // The network's branch, never downloaded, and a child of it that was.
        let mut theirs = Vec::new();
        let mut parent = fork;
        for number in fork_number + 1..=top {
            let header = node.header(number, parent, 9, 0xCC);
            parent = header.hash();
            theirs.push(header);
        }
        let child = node.header(top + 1, parent, 9, 0xCC);
        node.store.put_header(&child).unwrap();

        node.service.reconcile_exec_head_with_canonical().await;
        node.service.take_up_stored_successors().await;
        node.service.take_up_stored_successors().await;

        assert_eq!(
            node.executed_head(),
            Some(ours),
            "depth {depth}: execution stays"
        );
        assert_eq!(node.head(), Some(ours), "depth {depth}: the head stays");
        let (_, headers) = node.asked();
        assert_eq!(
            headers,
            vec![(parent, top as u32)],
            "depth {depth}: the headers ending at the missing parent are asked for, once"
        );

        // They come, newest first.
        let mut answer = theirs.clone();
        answer.reverse();
        node.service.on_headers_response(PEER, answer).await;
        assert_eq!(
            node.executed_head(),
            Some(ours),
            "depth {depth}: still not on headers"
        );
        let (bodies, _) = node.asked();
        for header in &theirs {
            assert!(
                bodies.contains(&header.hash()),
                "depth {depth}: the bodies of #{} are asked for",
                header.number
            );
        }
    }
}

/// A batch executes on a thread of its own and records its progress as it
/// goes, moving the executed head while the service stores headers. That is
/// no switch, and taking it for one dropped what was queued, fetched again
/// the bodies the batch held, and told the miner of a new best block: syncing
/// mainnet from block 0 halted every few thousand blocks on it, on blocks the
/// batch had run.
#[tokio::test]
async fn execution_recording_its_progress_is_not_taken_for_a_switch() {
    let mut node = node().await;
    let chain = node.chain(&[1, 1, 1, 1, 1, 1]);
    node.store
        .set_exec_head(chain[2].hash(), node.root)
        .unwrap();
    for header in &chain[3..] {
        node.store.put_body(header.hash(), &[], &[]).unwrap();
    }
    let queued = (chain[5].hash(), chain[5].clone(), Vec::new(), Vec::new());
    node.service.follow_buffer.insert(5, queued);
    node.service.last_body_height = 6;
    node.announced();

    let before = node.store.switch_mark();
    // The batch records #3 and #4 meanwhile.
    for header in &chain[3..=4] {
        assert!(crate::service::record_execution(
            &node.store,
            header.hash(),
            node.root
        ));
    }
    node.service.take_up_moved_execution(before);

    assert_eq!(node.executed_head(), Some(chain[4].hash()));
    assert_eq!(
        node.announced(),
        Vec::new(),
        "blocks the batch ran were announced as a branch taken up"
    );
    assert!(
        node.best.lock().unwrap().is_empty(),
        "the miner was told of a switch"
    );
    assert!(
        node.service.follow_buffer.contains_key(&5),
        "what was queued was dropped"
    );
    assert_eq!(
        node.service.last_body_height, 6,
        "bodies held already are fetched again"
    );
}

/// A switch the store makes while a batch runs is taken up from the block the
/// batch had got to, not from where execution stood when the call began: the
/// block the batch ran on the branch left is retracted, before the branch
/// taken up is announced.
#[tokio::test]
async fn a_switch_made_while_a_batch_runs_is_taken_up_from_where_the_batch_had_got() {
    let mut node = node().await;
    let chain = node.chain(&[1, 1, 1]); // totals 2, 3 and 4
    let (a1, a2) = (chain[1].hash(), chain[2].hash());
    node.store.set_exec_head(a1, node.root).unwrap();
    let b2 = node.header(2, a1, 1, 0xB0); // total 3
    let b3 = node.header(3, b2.hash(), 2, 0xB0); // total 5
    node.new_block(&b2).await;
    assert_eq!(node.runs(b2.hash()), 1, "#2' is executed beside the chain");
    node.announced();

    let before = node.store.switch_mark();
    // The batch records #2; then #3' comes, is executed beside the chain and
    // outweighs it, and the store switches to its branch.
    assert!(crate::service::record_execution(&node.store, a2, node.root));
    let head = node.head();
    let head_td = node.store.total_difficulty(head.unwrap()).unwrap().unwrap();
    node.store
        .store_headers_batch(&[(&b3, U256::from(5))], head, head_td)
        .unwrap();
    assert_eq!(
        node.store.deferred_head(),
        Some(b3.hash()),
        "held until it is executed"
    );
    node.store.put_body(b3.hash(), &[], &[]).unwrap();
    assert_eq!(node.service.execute_beside(b3.hash()), Beside::Executed);
    node.store.retry_deferred_head().unwrap();
    assert_eq!(node.executed_head(), Some(b3.hash()));
    node.service.take_up_moved_execution(before);

    assert_eq!(
        node.announced(),
        vec![(a2, true), (b2.hash(), false), (b3.hash(), false)],
        "the block the batch ran on the branch left is retracted, then the branch taken up is \
         announced"
    );
    assert_eq!(*node.best.lock().unwrap(), vec![b3.hash()]);
}

/// A block beside the chain whose header is held compressed, holding only the
/// commitment to its RSKIP-351 extension, is neither run nor refused: its
/// bloom and execution edges come beside its body, so the body is dropped and
/// fetched again with them, as a block of the chain's is
/// (`drop_body_without_extension`). Checked without them, testnet's #8,295,561
/// and #8,295,940, blocks of rskj's chain, were refused for want of their
/// edges and remembered as invalid, on 14 November 2026.
#[tokio::test]
async fn a_block_held_without_its_extension_is_fetched_again_not_run_or_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(
        BlockStore::open(dir.path())
            .unwrap()
            .with_activations(rustock_core::config::ActivationHeights::regtest()),
    );
    let mut node = node_over(dir, store).await;
    // Headers as regtest reads them from block 1: versioned, completed.
    let versioned = |header: Header| {
        let extension = crate::tests::an_extension(0);
        crate::tests::compressed(header, &extension)
            .with_extension(&extension)
            .expect("the extension it commits to")
    };
    let genesis = node.header(0, B256::ZERO, 1, 0);
    let a1 = versioned(node.header(1, genesis.hash(), 1, 0xA0));
    let a2 = versioned(node.header(2, a1.hash(), 1, 0xA0));
    for (header, total) in [(&genesis, 1u64), (&a1, 2), (&a2, 3)] {
        node.store.update_head(header, U256::from(total)).unwrap();
    }
    node.store.set_exec_head(a2.hash(), node.root).unwrap();
    // #2', held compressed beside its body, level with #2.
    let b2 = crate::tests::compressed(
        node.header(2, a1.hash(), 1, 0xB0),
        &crate::tests::an_extension(5),
    );
    let head = node.head();
    node.store
        .store_headers_batch(&[(&b2, U256::from(3))], head, U256::from(3))
        .unwrap();
    node.store.put_body(b2.hash(), &[], &[]).unwrap();
    node.asked();

    node.service
        .take_up_competing(vec![b2.hash()], Some(PEER))
        .await;

    assert_eq!(node.runs(b2.hash()), 0, "run without its extension");
    assert!(
        !node.service.invalid_beside.contains(&b2.hash()),
        "taken for invalid"
    );
    assert_eq!(
        node.store.body(b2.hash()).unwrap(),
        None,
        "its body is kept beside a header it can't be run with"
    );
    let (bodies, _) = node.asked();
    assert!(
        bodies.contains(&b2.hash()),
        "its body is not asked for again, with the extension"
    );
}

/// A second peer.
const OTHER: B512 = B512::repeat_byte(0x02);

/// The peer a request for the body of `hash`, beside the chain, is out to.
fn branch_body_asked_of(service: &SyncService, hash: B256) -> Option<B512> {
    service
        .pending_branch_bodies
        .values()
        .find(|(asked, ..)| *asked == hash)
        .map(|(.., peer)| *peer)
}

/// Every request out for a block beside the chain, or for the headers below
/// one, sent half a minute ago: lapsed, as one a peer never answers.
fn lapse_branch_requests(service: &mut SyncService) {
    let lapsed = Instant::now()
        .checked_sub(Duration::from_secs(31))
        .expect("the clock has run half a minute");
    for (_, _, sent, _) in service.pending_branch_bodies.values_mut() {
        *sent = lapsed;
    }
    for (asked, _) in service.branch_headers_asked.values_mut() {
        *asked = lapsed;
    }
}

/// The peer asked for the headers ending at `hash` since last looked: [`PEER`]
/// or [`OTHER`], whose messages arrive in `other`.
fn headers_asked_of(
    node: &mut Node,
    other: &mut mpsc::Receiver<P2pMessage>,
    hash: B256,
) -> Option<B512> {
    let (_, headers) = node.asked();
    if headers.iter().any(|(asked, _)| *asked == hash) {
        return Some(PEER);
    }
    while let Ok(message) = other.try_recv() {
        if let P2pMessage::RskMessage(RskMessage {
            sub_message: RskSubMessage::BlockHeadersRequest(request),
            ..
        }) = message
        {
            if request.query.hash == hash {
                return Some(OTHER);
            }
        }
    }
    None
}

/// A body of a branch beside the chain that the peer asked never sends is
/// asked of another peer once the request has lapsed, as the body pipeline
/// asks another (`retry_stale_body_requests`). It was asked of a peer known to
/// hold the block or else of the first peer connected, every time, and a peer
/// is known to hold a block only once it has announced it or sent it whole:
/// on testnet on 15 November 2026 the network's branch from #8,296,515 came by
/// header sync, its bodies were asked of a peer that never answered, and the
/// node stood off the chain for over an hour.
#[tokio::test]
async fn a_branch_body_a_peer_never_sends_is_asked_of_another_peer() {
    let mut node = node().await;
    let (to_other, _other_inbox) = mpsc::channel(rustock_networking::peers::PEER_CHANNEL_CAPACITY);
    node.service.peer_store.add_peer(OTHER, to_other).await;
    let chain = node.chain(&[1, 2]); // totals 2 and 4
    let a1 = chain[1].hash();
    let x2 = node.header(2, a1, 1, 0xC0); // total 3
    let x3 = node.header(3, x2.hash(), 9, 0xC0); // total 12
    node.service
        .on_headers_response(PEER, vec![x3.clone(), x2.clone()])
        .await;
    assert_eq!(
        node.store.deferred_head(),
        Some(x3.hash()),
        "the branch is held back"
    );
    let first = branch_body_asked_of(&node.service, x2.hash())
        .expect("the body of its first block is asked for");

    // That peer never answers, and the request lapses.
    lapse_branch_requests(&mut node.service);
    node.service.expire_stale_branch_bodies();
    node.service.advance_deferred_branch().await;

    let again = branch_body_asked_of(&node.service, x2.hash()).expect("it is asked for again");
    assert_ne!(again, first, "of another peer");
}

/// The headers below a branch whose ancestry this node lacks, asked of a peer
/// that never sends them, are asked of another peer once the request has
/// lapsed. They too were asked of a peer known to hold the branch's tip or
/// else of the first peer connected, every time.
#[tokio::test]
async fn headers_below_a_branch_a_peer_never_sends_are_asked_of_another_peer() {
    let mut node = node().await;
    let (to_other, mut other_inbox) =
        mpsc::channel(rustock_networking::peers::PEER_CHANNEL_CAPACITY);
    node.service.peer_store.add_peer(OTHER, to_other).await;
    // Our chain to #31; the network's #31, never downloaded, and a child of
    // it that was.
    let chain = node.chain(&[1; 31]);
    let theirs = node.header(31, chain[30].hash(), 9, 0xCC);
    let child = node.header(32, theirs.hash(), 9, 0xCC);
    node.store.put_header(&child).unwrap();

    node.service.take_up_stored_successors().await;
    let first = headers_asked_of(&mut node, &mut other_inbox, theirs.hash())
        .expect("the headers ending at the block it lacks are asked for");

    // That peer never answers, and the request lapses.
    lapse_branch_requests(&mut node.service);
    node.service.take_up_stored_successors().await;

    let again = headers_asked_of(&mut node, &mut other_inbox, theirs.hash())
        .expect("they are asked for again");
    assert_ne!(again, first, "of another peer");
}
