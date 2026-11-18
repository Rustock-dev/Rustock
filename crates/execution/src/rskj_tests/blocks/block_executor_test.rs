//! rskj `co.rsk.core.bc.BlockExecutorTest`: what a block's run comes to,
//! and that the block carries it (`BlockExecutor.executeAndValidate`), run
//! as one list or as its sublists. The miner's own run, which picks a
//! block's transactions and lays them out (`executeForMining`,
//! `executeAndFill`), is not here.

use super::support::header_at;
use crate::hardfork::RskHardforkConfig;
use crate::mining::workload::{address_of, sign};
use crate::processor::{compute_ommers_hash, BlockProcessor, ProcessError};
use alloy_primitives::{Address, Bytes, B256, U256};
use k256::ecdsa::SigningKey;
use rustock_core::types::header::HeaderVersionFields;
use rustock_core::{ordered_tx_trie_root, Block, Header, Transaction};
use rustock_storage::BlockStore;
use rustock_trie::{account_key, AccountState, MemoryTrieStore, TrieKeySlice, TrieNode, TrieStore};
use std::sync::Arc;

/// Testnet's chain id.
const TESTNET: u64 = 31;

/// A height of testnet's where a block runs as its sublists.
const SUBLISTS: u64 = 8_000_000;

/// Where RSKIP126 activates on mainnet: wasabi100.
const RSKIP126: u64 = 1_591_000;

/// The key of `seed`.
fn key(seed: u8) -> SigningKey {
    SigningKey::from_slice(&[seed; 32]).unwrap()
}

/// What a block runs on: a state, and the store it is in.
struct World {
    store: Arc<dyn TrieStore>,
    root: TrieNode,
}

impl World {
    /// The accounts of `accounts`' keys, each holding the wei given.
    fn of(accounts: &[(&SigningKey, u64)]) -> Self {
        let store: Arc<dyn TrieStore> = Arc::new(MemoryTrieStore::new());
        let root = accounts
            .iter()
            .fold(TrieNode::empty(), |root, (key, balance)| {
                let state = AccountState::new(U256::ZERO, U256::from(*balance));
                root.put(
                    &TrieKeySlice::from_key(&account_key(&address_of(key))),
                    &state.encode(),
                    store.as_ref(),
                )
            });
        World { store, root }
    }

    /// `account`'s balance in `state`.
    fn balance(&self, state: &TrieNode, account: Address) -> U256 {
        let key = account_key(&account);
        let encoded = state
            .get(&TrieKeySlice::from_key(&key), self.store.as_ref())
            .expect("the account is there");
        AccountState::decode(&encoded).unwrap().balance
    }
}

/// A transfer of 10 wei to `to` at `nonce`, at a gas price of 1, signed
/// by `from` for testnet.
fn transfer(from: &SigningKey, nonce: u64, to: Address) -> Transaction {
    let mut transaction = Transaction {
        nonce,
        gas_price: U256::from(1),
        gas_limit: U256::from(21_000),
        to: Bytes::copy_from_slice(to.as_slice()),
        value: U256::from(10),
        input: Bytes::new(),
        v: 0,
        r: U256::ZERO,
        s: U256::ZERO,
        cached_rlp: None,
    };
    sign(&mut transaction, from, TESTNET);
    transaction
}

/// A block at `number` of `transactions`, its header laid out with `edges`
/// where they are given, and with testnet's gas limit, which leaves each
/// parallel sublist 1,000,000.
fn block(number: u64, transactions: Vec<Transaction>, edges: Option<Vec<i16>>) -> Block {
    let mut header = header_at(number);
    header.gas_limit = U256::from(12_000_000);
    header.version_fields = edges.map(|edges| HeaderVersionFields {
        version: 1,
        edges: Some(edges),
        base_event: None,
    });
    Block {
        header,
        transactions,
        ommers: vec![],
    }
}

/// A processor of `chain`'s blocks, and the folder its store is in.
fn processor(chain: RskHardforkConfig) -> (BlockProcessor, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let blocks = Arc::new(BlockStore::open(dir.path()).unwrap());
    (BlockProcessor::new(chain, blocks), dir)
}

/// `block`, its header carrying what its run on `world` comes to, its
/// transactions' root taken as the Unitrie's where `unitrie`.
fn carrying(processor: &BlockProcessor, world: &World, mut block: Block, unitrie: bool) -> Block {
    let ran = processor
        .execute_block(&block, &world.root, world.store.clone())
        .expect("the block runs");
    let header = &mut block.header;
    header.transactions_root = ordered_tx_trie_root(&block.transactions, unitrie);
    header.ommers_hash = compute_ommers_hash(&block.ommers);
    header.state_root = ran.state_root_hash;
    header.receipts_root = ran.receipts_root;
    header.gas_used = ran.gas_used;
    header.paid_fees = ran.paid_fees;
    header.logs_bloom = ran.logs_bloom;
    block
}

/// rskj `executeBlockWithoutTransaction`: a block of nothing leaves the
/// state as it was, with no receipts, laid out or not.
#[test]
fn a_block_without_transactions_leaves_the_state_as_it_was() {
    let world = World::of(&[(&key(1), 30_000)]);
    let (processor, _dir) = processor(RskHardforkConfig::testnet());
    for edges in [None, Some(vec![])] {
        let ran = processor
            .execute_block(
                &block(SUBLISTS, vec![], edges),
                &world.root,
                world.store.clone(),
            )
            .unwrap();
        assert!(ran.receipts.is_empty());
        assert_eq!(
            ran.state_root_hash,
            world.root.compute_hash(world.store.as_ref())
        );
    }
}

/// rskj `executeBlockWithTwoTransactions`: two transfers from one sender at
/// a gas price of 1 use 21,000 each, counted up across the block, and pay
/// 42,000 in fees; the sender is left 60,000 less those and the 20 wei
/// sent.
#[test]
fn two_transfers_count_their_gas_up_and_pay_their_fees() {
    let (sender, recipient) = (key(1), key(2));
    let world = World::of(&[(&sender, 60_000)]);
    let (processor, _dir) = processor(RskHardforkConfig::testnet());
    let to = address_of(&recipient);
    let two = block(
        SUBLISTS,
        vec![transfer(&sender, 0, to), transfer(&sender, 1, to)],
        None,
    );
    let ran = processor
        .execute_block(&two, &world.root, world.store.clone())
        .unwrap();
    let gas: Vec<_> = ran
        .receipts
        .iter()
        .map(|receipt| (receipt.gas_used, receipt.cumulative_gas_used))
        .collect();
    assert_eq!(gas, [(21_000, 21_000), (21_000, 42_000)]);
    assert!(ran.receipts.iter().all(|receipt| receipt.status));
    assert_eq!(ran.gas_used, 42_000);
    assert_eq!(ran.paid_fees, U256::from(42_000));
    let left = world.balance(&ran.new_state_root, address_of(&sender));
    assert_eq!(left, U256::from(60_000 - 42_000 - 20));
    assert_eq!(world.balance(&ran.new_state_root, to), U256::from(20));
}

/// rskj `executeBlockWithTxThatMakesBlockInvalidSenderHasNoBalance`: a
/// transfer from an account with nothing to pay for it makes its block
/// invalid.
#[test]
fn a_sender_with_nothing_to_pay_makes_its_block_invalid() {
    let (payer, recipient, penniless) = (key(1), key(2), key(3));
    let world = World::of(&[(&payer, 30_000), (&recipient, 10), (&penniless, 0)]);
    let (processor, _dir) = processor(RskHardforkConfig::testnet());
    let to = address_of(&recipient);
    let paid = block(SUBLISTS, vec![transfer(&payer, 0, to)], None);
    assert!(processor
        .execute_block(&paid, &world.root, world.store.clone())
        .is_ok());
    let unpaid = block(
        SUBLISTS,
        vec![transfer(&payer, 0, to), transfer(&penniless, 0, to)],
        None,
    );
    let refused = processor
        .execute_block(&unpaid, &world.root, world.store.clone())
        .unwrap_err();
    assert!(matches!(refused, ProcessError::Execution(_)), "{refused}");
}

/// rskj `validateBlock`, `invalidBlockBadStateRoot`,
/// `invalidBlockBadReceiptsRoot`, `invalidBlockBadGasUsed`,
/// `invalidBlockBadPaidFees` and `invalidBlockBadLogsBloom`: a block
/// carrying what its run comes to is valid, and one whose state root,
/// receipts root, gas used, fees or logs bloom is off is refused for it,
/// run as one list or as its sublists.
#[test]
fn a_block_is_held_to_what_its_run_comes_to() {
    let (sender, recipient) = (key(1), key(2));
    let world = World::of(&[(&sender, 30_000), (&recipient, 10)]);
    let (processor, _dir) = processor(RskHardforkConfig::testnet());
    for edges in [None, Some(vec![1])] {
        let one = block(
            SUBLISTS,
            vec![transfer(&sender, 0, address_of(&recipient))],
            edges,
        );
        let valid = carrying(&processor, &world, one, true);
        assert!(processor
            .process_block(&valid, &world.root, world.store.clone())
            .is_ok());
        let refused = |off: fn(&mut Header)| {
            let mut block = valid.clone();
            off(&mut block.header);
            processor
                .process_block(&block, &world.root, world.store.clone())
                .unwrap_err()
        };
        assert!(matches!(
            refused(|header| header.state_root.0[0] = header.state_root.0[0].wrapping_add(1)),
            ProcessError::StateRootMismatch { .. }
        ));
        assert!(matches!(
            refused(|header| header.receipts_root.0[0] = header.receipts_root.0[0].wrapping_add(1)),
            ProcessError::ReceiptsRootMismatch { .. }
        ));
        assert!(matches!(
            refused(|header| header.gas_used = 0),
            ProcessError::GasUsedMismatch { .. }
        ));
        assert!(matches!(
            refused(|header| header.paid_fees = U256::ZERO),
            ProcessError::PaidFeesMismatch { .. }
        ));
        assert!(matches!(
            refused(|header| header.logs_bloom.0[0] = header.logs_bloom.0[0].wrapping_add(1)),
            ProcessError::LogsBloomMismatch
        ));
    }
}

/// rskj `validateStateRootWithRskip126DisabledAndValidStateRoot` and
/// `validateStateRootWithRskip126DisabledAndInvalidStateRoot`: below
/// RSKIP126 a block's state root isn't held to its run, so one carrying
/// the state's root and one carrying 0x01020304 both pass
/// (`BlockExecutor.validateStateRoot`, 319-332). From RSKIP126 the second
/// is refused.
#[test]
fn below_rskip126_a_blocks_state_root_is_not_held_to_its_run() {
    let world = World::of(&[(&key(1), 30_000)]);
    let (processor, _dir) = processor(RskHardforkConfig::mainnet());
    for number in [RSKIP126 - 1, RSKIP126] {
        let unitrie = number >= RSKIP126;
        let valid = carrying(&processor, &world, block(number, vec![], None), unitrie);
        assert!(processor
            .process_block(&valid, &world.root, world.store.clone())
            .is_ok());
        let mut other = valid;
        other.header.state_root = B256::left_padding_from(&[1, 2, 3, 4]);
        let other = processor.process_block(&other, &world.root, world.store.clone());
        if unitrie {
            assert!(matches!(other, Err(ProcessError::StateRootMismatch { .. })));
        } else {
            assert!(other.is_ok(), "#{number}");
        }
    }
}

/// Ten transfers, each from one of the first ten keys to the one ten on.
fn ten_transfers(keys: &[SigningKey]) -> Vec<Transaction> {
    (0..10)
        .map(|i| transfer(&keys[i], 0, address_of(&keys[i + 10])))
        .collect()
}

/// rskj `executeParallelBlockAgainstSequentialBlock`,
/// `executeParallelBlocksWithDifferentSubsets` and
/// `executeParallelBlockTwice`: ten transfers that touch nothing of each
/// other's come to one state run as one list, as sublists of four, four
/// and two, as sublists of five and five, and again.
#[test]
fn independent_transfers_come_to_one_state_however_their_block_splits_them() {
    let keys: Vec<SigningKey> = (1..=20).map(key).collect();
    let funded: Vec<_> = keys.iter().map(|key| (key, 60_000)).collect();
    let world = World::of(&funded);
    let (processor, _dir) = processor(RskHardforkConfig::testnet());
    let state = |edges: Vec<i16>| {
        let ten = block(SUBLISTS, ten_transfers(&keys), Some(edges));
        let ran = processor
            .execute_block(&ten, &world.root, world.store.clone())
            .unwrap();
        assert_eq!(ran.receipts.len(), 10);
        ran.state_root_hash
    };
    let as_one_list = state(vec![]);
    assert_eq!(state(vec![4, 8]), as_one_list);
    assert_eq!(state(vec![5]), as_one_list);
    assert_eq!(state(vec![4, 8]), as_one_list, "and again");
}

/// rskj `executeInvalidParallelBlockDueToCollision` and
/// `ifThereIsACollisionBetweenParallelAndSequentialSublistsTxShouldNotBeConsidered`:
/// two accounts each sending to the other collide, each in a parallel
/// sublist of its own, and the block is invalid; with the second in the
/// sequential sublist they don't, and both run.
#[test]
fn parallel_sublists_may_not_touch_what_another_writes() {
    let (one, other) = (key(1), key(2));
    let world = World::of(&[(&one, 60_000), (&other, 60_000)]);
    let (processor, _dir) = processor(RskHardforkConfig::testnet());
    let each_to_the_other = || {
        vec![
            transfer(&one, 0, address_of(&other)),
            transfer(&other, 0, address_of(&one)),
        ]
    };
    let apart = block(SUBLISTS, each_to_the_other(), Some(vec![1, 2]));
    let refused = processor
        .execute_block(&apart, &world.root, world.store.clone())
        .unwrap_err();
    assert!(
        matches!(refused, ProcessError::SublistCollision { .. }),
        "{refused}"
    );
    let one_in_sequence = block(SUBLISTS, each_to_the_other(), Some(vec![1]));
    let ran = processor
        .execute_block(&one_in_sequence, &world.root, world.store.clone())
        .unwrap();
    assert_eq!(ran.receipts.len(), 2);
    assert!(ran.receipts.iter().all(|receipt| receipt.status));
}
