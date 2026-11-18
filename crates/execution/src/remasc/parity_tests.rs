//! REMASC as rskj's `Remasc.processMinersFees` runs it: on the chain of the
//! block it runs in, read up from that block's parent; with the upgrades of
//! that block, read from the network's schedule; and against that block's
//! minimum gas price, whole.

use std::sync::Arc;

use alloy_primitives::{hex, Address, Bloom, Bytes, B256, U256};
use rustock_core::Header;
use rustock_storage::freezer::Freezer;
use rustock_storage::BlockStore;
use rustock_trie::{
    account_key, storage_key, AccountState, MemoryTrieStore, TrieKeySlice, TrieNode,
};

use super::{
    remasc_storage_key, RemascConfig, BURNED_BALANCE_KEY, FEDERATION_BALANCE_KEY,
    REWARD_BALANCE_KEY,
};
use crate::bridge::federation::{
    rsk_address_from_public_key, serialize_federation_multikey, StoredMember,
};
use crate::bridge::storage::{bridge_storage_key, NEW_FEDERATION_KEY, OLD_FEDERATION_KEY};
use crate::executor::ExecutionError;
use crate::hardfork::{RskHardforkConfig, RskNetworkUpgrade};
use crate::precompiles::{BRIDGE_ADDR, REMASC_ADDR};
use crate::RskExecutor;

/// Two federators' public keys.
const KEY_A: &str = "03a5e32151f974c35c5d08af36a8d6af0593248e8e4adca7ed0148192eb2f5d1bf";
const KEY_B: &str = "02f3b5fb3121932db9450121b0a37f3f051dc8b3fd5f1ea5e7cf726947d8f69c28";

/// What REMASC holds before its transaction runs: enough to pay anything.
fn plenty() -> U256 {
    U256::from(1) << 100
}

/// The miner of block `number` on the chain tagged `chain`.
fn miner(chain: u8, number: u64) -> Address {
    let mut bytes = [chain; 20];
    bytes[19] = number as u8;
    Address::from(bytes)
}

/// Block `number` on `parent`, mined by `coinbase`, whose transactions paid
/// `fees`, naming `uncles` uncles.
fn block(number: u64, parent: B256, coinbase: Address, fees: U256, uncles: u64) -> Header {
    Header {
        parent_hash: parent,
        ommers_hash: B256::ZERO,
        beneficiary: coinbase,
        state_root: B256::ZERO,
        transactions_root: B256::ZERO,
        receipts_root: B256::ZERO,
        logs_bloom: Bloom::ZERO,
        extension_data: None,
        difficulty: U256::from(1_000_000),
        number,
        gas_limit: U256::from(6_800_000),
        gas_used: 0,
        timestamp: 1_700_000_000 + number,
        extra_data: Bytes::new(),
        paid_fees: fees,
        minimum_gas_price: U256::ZERO,
        uncle_count: uncles,
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

/// A block store, and the folder it lives in.
fn block_store() -> (tempfile::TempDir, Arc<BlockStore>) {
    let folder = tempfile::tempdir().expect("a folder for the block store");
    let store = Arc::new(BlockStore::open(folder.path()).expect("a block store"));
    (folder, store)
}

/// Keeps blocks `first` to `last`, each on the one before it and the first
/// on `parent`, as `make` builds each from its number and its parent's hash,
/// with its uncles; as the main chain's blocks where `main`. Their hashes, in
/// order.
fn keep(
    store: &BlockStore,
    first: u64,
    parent: B256,
    last: u64,
    main: bool,
    mut make: impl FnMut(u64, B256) -> (Header, Vec<Header>),
) -> Vec<B256> {
    let mut hashes = Vec::new();
    let mut parent = parent;
    for number in first..=last {
        let (header, uncles) = make(number, parent);
        let hash = header.hash();
        store.put_header_with_hash(hash, &header).unwrap();
        if !uncles.is_empty() {
            store.put_body(hash, &[], &uncles).unwrap();
        }
        if main {
            store.put_canonical_hash(number, hash).unwrap();
        }
        hashes.push(hash);
        parent = hash;
    }
    hashes
}

/// Blocks 0 to `last` of the main chain, mined by `miner(0xa0, number)`,
/// whose transactions paid `fees(number)`, with no uncles.
fn main_chain(store: &BlockStore, last: u64, fees: impl Fn(u64) -> U256) -> Vec<B256> {
    keep(store, 0, B256::ZERO, last, true, |number, parent| {
        (
            block(number, parent, miner(0xa0, number), fees(number), 0),
            vec![],
        )
    })
}

/// `account`'s balance in `state`: zero where it has no account.
fn balance(state: &TrieNode, trie: &MemoryTrieStore, account: Address) -> U256 {
    let key = account_key(&account);
    state
        .get(&TrieKeySlice::from_key(&key), trie)
        .map_or(U256::ZERO, |data| {
            AccountState::decode(&data).expect("an account").balance
        })
}

/// The word REMASC keeps under `name` in `state`.
fn remasc_word(state: &TrieNode, trie: &MemoryTrieStore, name: &str) -> U256 {
    let key = storage_key(&REMASC_ADDR, &B256::from(remasc_storage_key(name)));
    state
        .get(&TrieKeySlice::from_key(&key), trie)
        .map_or(U256::ZERO, |value| U256::from_be_slice(&value))
}

/// `state` with an account at `address` holding `balance`.
fn with_account(
    state: TrieNode,
    trie: &MemoryTrieStore,
    address: Address,
    balance: U256,
) -> TrieNode {
    let key = account_key(&address);
    let account = AccountState::new(U256::ZERO, balance).encode();
    state.put(&TrieKeySlice::from_key(&key), &account, trie)
}

/// `state` with `value` at `key` of `address`'s storage, written as rskj
/// writes a word: without its leading zeros.
fn with_storage(
    state: TrieNode,
    trie: &MemoryTrieStore,
    address: Address,
    key: U256,
    value: &[u8],
) -> TrieNode {
    let key = storage_key(&address, &B256::from(key));
    state.put(&TrieKeySlice::from_key(&key), value, trie)
}

/// `state` with a Bridge that keeps `new` as its new federation and `old`,
/// where there is one, as its old.
fn with_federations(
    state: TrieNode,
    trie: &MemoryTrieStore,
    new: &[u8],
    old: Option<&[u8]>,
) -> TrieNode {
    let state = with_account(state, trie, BRIDGE_ADDR, U256::ZERO);
    let new_key = bridge_storage_key(NEW_FEDERATION_KEY);
    let state = with_storage(state, trie, BRIDGE_ADDR, new_key, new);
    match old {
        Some(old) => {
            let old_key = bridge_storage_key(OLD_FEDERATION_KEY);
            with_storage(state, trie, BRIDGE_ADDR, old_key, old)
        }
        None => state,
    }
}

/// Runs REMASC's transaction alone in `executing`, on `hardforks`, from a
/// state where REMASC holds plenty and that `seed` adds to: the state after
/// it, or what refused the block.
fn run_remasc(
    store: Arc<BlockStore>,
    hardforks: RskHardforkConfig,
    executing: &Header,
    seed: impl FnOnce(TrieNode, &MemoryTrieStore) -> TrieNode,
) -> Result<(TrieNode, Arc<MemoryTrieStore>), ExecutionError> {
    let trie = Arc::new(MemoryTrieStore::new());
    let state = with_account(TrieNode::empty(), &trie, REMASC_ADDR, plenty());
    let state = seed(state, &trie);
    let remasc = rustock_core::Transaction::remasc(executing.number);
    let result = RskExecutor::new(hardforks, store).execute_block(
        executing,
        &[(remasc, Address::ZERO)],
        &state,
        trie.clone(),
    )?;
    let after = crate::state::apply_state_changes(
        &state,
        trie.as_ref(),
        &result.state_changes,
        &result.markers,
    );
    Ok((after, trie))
}

/// On regtest (maturity 10, a synthetic span of 5), block 16 pays for block
/// 6. A fork from block 2 has a block 6 of its own, mined by its own miner
/// with fees of its own, and block 16 on that fork pays for that block 6, as
/// rskj finds it up from block 16's parent: of 42,000 / 5 = 8,400, RSK Labs
/// gets 1,680, the federation's 67 are kept, as this regtest has none, and
/// the fork's miner gets 6,653. The miner of the main chain's block 6 gets
/// nothing.
#[test]
fn on_a_fork_remasc_pays_the_block_that_matured_on_the_fork() {
    let (_folder, store) = block_store();
    let fees_at_6 = |fees: u64| move |number: u64| U256::from(if number == 6 { fees } else { 0 });
    let main = main_chain(&store, 15, fees_at_6(21_000));
    let fork = keep(&store, 3, main[2], 15, false, |number, parent| {
        let fees = fees_at_6(42_000)(number);
        (block(number, parent, miner(0xf0, number), fees, 0), vec![])
    });
    let executing = block(16, fork[fork.len() - 1], miner(0xf0, 16), U256::ZERO, 0);

    let (state, trie) = run_remasc(
        store,
        RskHardforkConfig::regtest(),
        &executing,
        |state, _| state,
    )
    .unwrap();

    assert_eq!(balance(&state, &trie, miner(0xf0, 6)), U256::from(6_653));
    assert_eq!(balance(&state, &trie, miner(0xa0, 6)), U256::ZERO);
    assert_eq!(
        remasc_word(&state, &trie, REWARD_BALANCE_KEY),
        U256::from(42_000 - 8_400)
    );
}

/// A fork from block 7 shares block 6 with the main chain, and its block 8
/// names an uncle numbered 6 that no main-chain block names. Block 16 on the
/// fork pays that sibling, as rskj reads the uncles of blocks 7 to 13 of the
/// fork: of block 6's 3,327, its publisher, block 8's miner, gets a tenth,
/// 332; the two miners 1,497 each, with 1 burned; and the sibling, a block
/// late, loses a twentieth of its share, 74, burned too.
#[test]
fn on_a_fork_remasc_pays_the_siblings_the_fork_included() {
    let (_folder, store) = block_store();
    let main = main_chain(&store, 15, |number| {
        U256::from(if number == 6 { 21_000 } else { 0 })
    });
    let sibling = block(6, main[5], miner(0x5b, 6), U256::ZERO, 0);
    let fork = keep(&store, 8, main[7], 15, false, |number, parent| {
        let uncles = if number == 8 {
            vec![sibling.clone()]
        } else {
            vec![]
        };
        let header = block(
            number,
            parent,
            miner(0xf0, number),
            U256::ZERO,
            uncles.len() as u64,
        );
        (header, uncles)
    });
    let executing = block(16, fork[fork.len() - 1], miner(0xf0, 16), U256::ZERO, 0);

    let (state, trie) = run_remasc(
        store,
        RskHardforkConfig::regtest(),
        &executing,
        |state, _| state,
    )
    .unwrap();

    assert_eq!(balance(&state, &trie, miner(0xa0, 6)), U256::from(1_497));
    assert_eq!(balance(&state, &trie, miner(0x5b, 6)), U256::from(1_423));
    assert_eq!(balance(&state, &trie, miner(0xf0, 8)), U256::from(332));
    assert_eq!(
        remasc_word(&state, &trie, BURNED_BALANCE_KEY),
        U256::from(1 + 74)
    );
}

/// The payments REMASC logs in `executing` on regtest, in their order: to
/// whom, and how much.
fn payments_in(store: Arc<BlockStore>, executing: &Header) -> Vec<(Address, U256)> {
    use alloy_rlp::Decodable;
    let trie = Arc::new(MemoryTrieStore::new());
    let state = with_account(TrieNode::empty(), &trie, REMASC_ADDR, plenty());
    let remasc = rustock_core::Transaction::remasc(executing.number);
    let result = RskExecutor::new(RskHardforkConfig::regtest(), store)
        .execute_block(executing, &[(remasc, Address::ZERO)], &state, trie)
        .unwrap();
    let topic = crate::bridge::events::legacy_topic("mining_fee_topic");
    result.tx_results[0]
        .logs
        .iter()
        .filter(|log| log.topics()[0] == topic)
        .map(|log| {
            let mut data = &log.data.data[..];
            alloy_rlp::Header::decode(&mut data).unwrap();
            B256::decode(&mut data).unwrap();
            (
                Address::from_word(log.topics()[1]),
                U256::decode(&mut data).unwrap(),
            )
        })
        .collect()
}

/// rskj pays the siblings in the order of the blocks that included them, the
/// processing block's child first (`Remasc.getSiblingsToReward` over its
/// stack of descendants): their publishers, then the siblings, then the
/// processing block's miner, and the receipt keeps the logs in that order.
/// Block 16 pays block 6, one of whose siblings block 7 included and the
/// other block 9: of block 6's 3,327, each publisher gets 166; each miner
/// 998, but the sibling block 9 included, two blocks late, 899.
#[test]
fn remasc_pays_siblings_in_the_order_of_the_blocks_that_included_them() {
    let (_folder, store) = block_store();
    let main = main_chain(&store, 6, |number| {
        U256::from(if number == 6 { 21_000 } else { 0 })
    });
    let in_7 = block(6, main[5], miner(0x57, 6), U256::ZERO, 0);
    let in_9 = block(6, main[5], miner(0x59, 6), U256::ZERO, 0);
    let after = keep(&store, 7, main[6], 15, true, |number, parent| {
        let uncles = match number {
            7 => vec![in_7.clone()],
            9 => vec![in_9.clone()],
            _ => vec![],
        };
        let header = block(
            number,
            parent,
            miner(0xa0, number),
            U256::ZERO,
            uncles.len() as u64,
        );
        (header, uncles)
    });
    let executing = block(16, after[after.len() - 1], miner(0xa0, 16), U256::ZERO, 0);

    let labs = RemascConfig::regtest().labs_address_at(&RskHardforkConfig::regtest(), 16);
    let paid = |to: Address, amount: u64| (to, U256::from(amount));
    assert_eq!(
        payments_in(store, &executing),
        vec![
            paid(labs, 840),
            paid(miner(0xa0, 7), 166),
            paid(miner(0xa0, 9), 166),
            paid(miner(0x57, 6), 998),
            paid(miner(0x59, 6), 899),
            paid(miner(0xa0, 6), 998),
        ]
    );
}

/// A block REMASC reads that names an uncle the store keeps no body for is a
/// gap in the node's store, which rskj never has: the block can't be run here
/// and now, and no rule refuses it. To pay as though block 8 had no uncles
/// would be to pay what rskj may not.
#[test]
fn uncles_the_store_lacks_are_the_stores_fault_and_not_the_blocks() {
    let (_folder, store) = block_store();
    let main = keep(&store, 0, B256::ZERO, 15, true, |number, parent| {
        let uncles = u64::from(number == 8);
        (
            block(number, parent, miner(0xa0, number), U256::ZERO, uncles),
            vec![],
        )
    });
    let executing = block(16, main[15], miner(0xa0, 16), U256::ZERO, 0);

    let Err(refused) = run_remasc(
        store,
        RskHardforkConfig::regtest(),
        &executing,
        |state, _| state,
    ) else {
        panic!("the block can't be run without block 8's uncles");
    };
    assert!(
        matches!(refused, ExecutionError::Database(_)),
        "{refused:?}"
    );
}

/// Once a main-chain block's body is pruned, its uncles are the freezer's,
/// which keeps them by height for the main chain alone. Block 16 pays from
/// there the sibling the main chain's block 8 included; on a fork whose own
/// block 8 names an uncle the store keeps no body for, block 16 is not
/// answered with the main chain's.
#[test]
fn the_freezer_answers_for_the_main_chains_uncles_alone() {
    let (folder, store) = block_store();
    let freezer = Arc::new(Freezer::open(folder.path().join("freezer")).expect("a freezer"));
    store.set_freezer(freezer.clone());
    let main = keep(&store, 0, B256::ZERO, 15, true, |number, parent| {
        let fees = U256::from(if number == 6 { 21_000 } else { 0 });
        let uncles = u64::from(number == 8);
        (
            block(number, parent, miner(0xa0, number), fees, uncles),
            vec![],
        )
    });
    let sibling = block(6, main[5], miner(0x5b, 6), U256::ZERO, 0);
    freezer.put_uncles(8, &[sibling]).unwrap();
    freezer.sync().unwrap();
    let fork = keep(&store, 8, main[7], 15, false, |number, parent| {
        let uncles = u64::from(number == 8);
        (
            block(number, parent, miner(0xf0, number), U256::ZERO, uncles),
            vec![],
        )
    });

    let on_main = block(16, main[15], miner(0xa0, 16), U256::ZERO, 0);
    let regtest = RskHardforkConfig::regtest;
    let (state, trie) = run_remasc(store.clone(), regtest(), &on_main, |state, _| state).unwrap();
    assert_eq!(balance(&state, &trie, miner(0x5b, 6)), U256::from(1_423));

    let on_fork = block(16, fork[fork.len() - 1], miner(0xf0, 16), U256::ZERO, 0);
    let Err(refused) = run_remasc(store, regtest(), &on_fork, |state, _| state) else {
        panic!("the fork's block 8 names an uncle the store doesn't keep");
    };
    assert!(
        matches!(refused, ExecutionError::Database(_)),
        "{refused:?}"
    );
}

/// Regtest with fingerroot500 and every upgrade after it at `height`.
fn fingerroot500_at(height: u64) -> RskHardforkConfig {
    [
        RskNetworkUpgrade::Fingerroot500,
        RskNetworkUpgrade::Arrowhead600,
        RskNetworkUpgrade::Lovell700,
        RskNetworkUpgrade::Reed800,
        RskNetworkUpgrade::Vetiver900,
    ]
    .into_iter()
    .fold(RskHardforkConfig::regtest(), |ladder, upgrade| {
        ladder.with_hardfork(upgrade, Some(height))
    })
}

/// A federation of one member, whose three keys are `key`, committed at
/// block `created_at`; and the address its member is paid at.
fn federation_of(key: &str, created_at: u64) -> (Vec<u8>, Address) {
    let key: [u8; 33] = hex::decode(key)
        .expect("hex")
        .try_into()
        .expect("a compressed key");
    let member = StoredMember {
        btc: key,
        rsk: key,
        mst: key,
    };
    let address = rsk_address_from_public_key(&key).expect("a key");
    (
        serialize_federation_multikey(&[member], 0, created_at),
        address,
    )
}

/// REMASC runs its federation's rules on the processing block with the
/// executing block's upgrades. On regtest the activation age is 10 blocks
/// before RSKIP383 and 150 from it, which is at block 40 here. A federation
/// committed at block 19 is 10 blocks old at block 29 and 11 at block 30.
/// Block 39 pays for block 29 with the age of 10, which it has reached: the
/// new federation is the active one. Block 40 pays for block 30 with the age
/// of 150: the old one still is, though block 30 itself is below RSKIP383.
/// Each is paid 3,360 of block 29's or block 30's 2,100,000 in fees.
#[test]
fn remasc_holds_a_new_federation_to_the_activation_age_of_the_executing_block() {
    let (new, to_new) = federation_of(KEY_A, 19);
    let (old, to_old) = federation_of(KEY_B, 0);
    let paid_at = |number: u64| {
        let (_folder, store) = block_store();
        let fees = |n: u64| U256::from(if n == number - 10 { 2_100_000 } else { 0 });
        let main = main_chain(&store, number - 1, fees);
        let executing = block(
            number,
            main[main.len() - 1],
            miner(0xa0, number),
            U256::ZERO,
            0,
        );
        let (state, trie) = run_remasc(store, fingerroot500_at(40), &executing, |state, trie| {
            with_federations(state, trie, &new, Some(&old))
        })
        .unwrap();
        (
            balance(&state, &trie, to_new),
            balance(&state, &trie, to_old),
        )
    };
    assert_eq!(
        paid_at(39),
        (U256::from(3_360), U256::ZERO),
        "before RSKIP383"
    );
    assert_eq!(
        paid_at(40),
        (U256::ZERO, U256::from(3_360)),
        "from RSKIP383"
    );
}

/// `value` written as rskj writes a word: without its leading zeros.
fn word(value: U256) -> Vec<u8> {
    let bytes = value.to_be_bytes::<32>();
    let first = bytes.iter().position(|byte| *byte != 0).unwrap_or(31);
    bytes[first..].to_vec()
}

/// From RSKIP85 REMASC holds back a reward under the executing block's
/// minimum gas price times 200,000, the price read whole: at 2^64, which
/// revm's base fee can't hold, a reward one wei short waits in the reward
/// balance and one of exactly that is paid out of it.
#[test]
fn remasc_holds_back_a_reward_by_the_whole_minimum_gas_price() {
    let price = U256::from(1) << 64;
    let least = price * U256::from(200_000);
    for (synthetic, paid) in [(least - U256::from(1), false), (least, true)] {
        let (_folder, store) = block_store();
        let fees = synthetic * U256::from(5);
        let main = main_chain(
            &store,
            15,
            |number| if number == 6 { fees } else { U256::ZERO },
        );
        let mut executing = block(16, main[15], miner(0xa0, 16), U256::ZERO, 0);
        executing.minimum_gas_price = price;
        let (state, trie) = run_remasc(
            store,
            RskHardforkConfig::regtest(),
            &executing,
            |state, _| state,
        )
        .unwrap();
        let left = if paid { fees - synthetic } else { fees };
        assert_eq!(
            remasc_word(&state, &trie, REWARD_BALANCE_KEY),
            left,
            "paid: {paid}"
        );
    }
}

/// Likewise a federator's share: from RSKIP85 it accrues in the federation's
/// balance while it is under the executing block's minimum gas price, read
/// whole, times 50,000, and is paid once it reaches that.
#[test]
fn remasc_holds_back_a_federators_share_by_the_whole_minimum_gas_price() {
    let price = U256::from(1) << 64;
    let least = price * U256::from(50_000);
    // A reward well past its own minimum, whose federation cut, a 125th of
    // it, the balance tops up to the share wanted.
    let synthetic = price * U256::from(2_000_000);
    let cut = synthetic / U256::from(125);
    let (federation, _) = federation_of(KEY_A, 0);
    for (share, paid) in [(least - U256::from(1), false), (least, true)] {
        let (_folder, store) = block_store();
        let fees = synthetic * U256::from(5);
        let main = main_chain(
            &store,
            15,
            |number| if number == 6 { fees } else { U256::ZERO },
        );
        let mut executing = block(16, main[15], miner(0xa0, 16), U256::ZERO, 0);
        executing.minimum_gas_price = price;
        let (state, trie) = run_remasc(
            store,
            RskHardforkConfig::regtest(),
            &executing,
            |state, trie| {
                let state = with_federations(state, trie, &federation, None);
                let kept = remasc_storage_key(FEDERATION_BALANCE_KEY);
                with_storage(state, trie, REMASC_ADDR, kept, &word(share - cut))
            },
        )
        .unwrap();
        let kept = if paid { U256::ZERO } else { share };
        assert_eq!(
            remasc_word(&state, &trie, FEDERATION_BALANCE_KEY),
            kept,
            "paid: {paid}"
        );
    }
}

/// Regtest with orchid and every upgrade after it at `height`.
fn orchid_at(height: u64) -> RskHardforkConfig {
    [
        RskNetworkUpgrade::Orchid,
        RskNetworkUpgrade::Wasabi100,
        RskNetworkUpgrade::TwoToThree,
        RskNetworkUpgrade::Papyrus200,
        RskNetworkUpgrade::Iris300,
        RskNetworkUpgrade::Hop400,
        RskNetworkUpgrade::Fingerroot500,
        RskNetworkUpgrade::Arrowhead600,
        RskNetworkUpgrade::Lovell700,
        RskNetworkUpgrade::Reed800,
        RskNetworkUpgrade::Vetiver900,
    ]
    .into_iter()
    .fold(RskHardforkConfig::regtest(), |ladder, upgrade| {
        ladder.with_hardfork(upgrade, Some(height))
    })
}

/// RSKIP85 comes with orchid wherever the schedule puts it, read at the
/// executing block. With orchid at block 20 and a minimum gas price of 1,
/// block 19 pays block 9's synthetic reward of 4,200 and block 20 holds back
/// block 10's, 4,200 being under 200,000. Of a synthetic reward of
/// 1,000,000, past that, block 19 pays the federator its 8,000 and block 20
/// keeps them in the federation's balance, 8,000 being under 50,000.
#[test]
fn rskip85_comes_with_orchid_where_the_schedule_puts_it() {
    let (federation, federator) = federation_of(KEY_A, 0);
    let run = |number: u64, fees: u64| {
        let (_folder, store) = block_store();
        let main = main_chain(&store, number - 1, |n| {
            U256::from(if n == number - 10 { fees } else { 0 })
        });
        let mut executing = block(
            number,
            main[main.len() - 1],
            miner(0xa0, number),
            U256::ZERO,
            0,
        );
        executing.minimum_gas_price = U256::from(1);
        let (state, trie) = run_remasc(store, orchid_at(20), &executing, |state, trie| {
            with_federations(state, trie, &federation, None)
        })
        .unwrap();
        (
            remasc_word(&state, &trie, REWARD_BALANCE_KEY),
            balance(&state, &trie, federator),
            remasc_word(&state, &trie, FEDERATION_BALANCE_KEY),
        )
    };
    let (reward_balance, _, _) = run(19, 21_000);
    assert_eq!(reward_balance, U256::from(21_000 - 4_200), "before orchid");
    let (reward_balance, _, _) = run(20, 21_000);
    assert_eq!(reward_balance, U256::from(21_000), "from orchid");

    let (_, paid, kept) = run(19, 5_000_000);
    assert_eq!(
        (paid, kept),
        (U256::from(8_000), U256::ZERO),
        "before orchid"
    );
    let (_, paid, kept) = run(20, 5_000_000);
    assert_eq!((paid, kept), (U256::ZERO, U256::from(8_000)), "from orchid");
}

/// rskj's two switches of REMASC, at each network's own heights.
#[test]
fn rskip85_and_rskip218_come_with_orchid_and_iris300() {
    let mainnet = RskHardforkConfig::mainnet();
    assert!(!mainnet.has_rskip85(728_999));
    assert!(mainnet.has_rskip85(729_000));
    assert!(!mainnet.has_rskip218(3_614_799));
    assert!(mainnet.has_rskip218(3_614_800));
    let testnet = RskHardforkConfig::testnet();
    assert!(testnet.has_rskip85(0));
    assert!(!testnet.has_rskip218(2_060_499));
    assert!(testnet.has_rskip218(2_060_500));
    let regtest = RskHardforkConfig::regtest();
    assert!(regtest.has_rskip85(0));
    assert!(regtest.has_rskip218(0));
}

/// RSK Labs' cut goes to its RSKIP218 address from iris300 as the schedule
/// puts it, and from RSKIP218's own height where that is scheduled apart; so
/// does RSKIP85 from its own.
#[test]
fn remascs_switches_follow_the_schedule_and_a_rules_own_height() {
    let config = RemascConfig::mainnet();
    let mainnet = RskHardforkConfig::mainnet();
    let (before, after) = (config.rsk_labs_address, config.rsk_labs_address_rskip218);
    assert_ne!(before, after);
    assert_eq!(config.labs_address_at(&mainnet, 3_614_799), before);
    assert_eq!(config.labs_address_at(&mainnet, 3_614_800), after);
    let later =
        RskHardforkConfig::mainnet().with_hardfork(RskNetworkUpgrade::Iris300, Some(3_700_000));
    assert_eq!(config.labs_address_at(&later, 3_614_800), before);
    assert_eq!(config.labs_address_at(&later, 3_700_000), after);

    let mut apart = config.clone();
    apart.rskip218_activation_height = Some(10);
    apart.rskip85_activation_height = Some(u64::MAX);
    assert_eq!(apart.labs_address_at(&mainnet, 9), before);
    assert_eq!(apart.labs_address_at(&mainnet, 10), after);
    assert!(config.rskip85_active(&mainnet, 729_000));
    assert!(!config.rskip85_active(&mainnet, 728_999));
    assert!(!apart.rskip85_active(&mainnet, 729_000));
}
