//! What the ported tests share: programs written as rskj's VM tests write
//! them, and a chain of blocks for them to run in. The tests of the
//! precompiled contracts and of REMASC use it too.
//!
//! rskj's VM tests write a program as opcode names, compiled by their
//! `BytecodeCompiler`, and step it on a mocked program or run it in blocks
//! of their DSL. Here a program is the code of a contract a transaction
//! calls, or the init code a transaction deploys, in a block rustock's
//! executor runs: the path every block takes.

use std::sync::Arc;

use alloy_primitives::{hex, Address, Bloom, Bytes, B256, U256};
use rustock_core::{Header, Transaction};
use rustock_storage::BlockStore;
use rustock_trie::{AccountState, MemoryTrieStore, TrieKeySlice, TrieNode};

use crate::executor::{
    BlockExecutionResult, ExecutionError, MiningDrops, RskExecutor, TxExecutionResult,
};
use crate::hardfork::RskHardforkConfig;

/// The account that sends the transactions of a test.
pub(crate) const SENDER: Address = Address::repeat_byte(0xa1);
/// The contract a test's program runs as.
pub(crate) const CONTRACT: Address = Address::repeat_byte(0xc0);
/// One RBTC, in wei.
pub(crate) const ONE_RBTC: U256 = U256::from_limbs([1_000_000_000_000_000_000, 0, 0, 0]);
/// The gas a test's transaction is given, unless it says otherwise.
pub(crate) const GAS: u64 = 1_000_000;
/// What [`return_top`] of one word costs: two PUSH2s, an MSTORE and the
/// word of memory it takes, and a PUSH1. RETURN costs nothing.
pub(crate) const RETURN_ONE_WORD_GAS: u64 = 15;
/// Code that logs the top word of the stack, and takes it off: an MSTORE of
/// it at 0, and a LOG0 of those 32 bytes. [`logged`] reads it back.
pub(crate) const LOG_TOP: &str = "PUSH1 0x00 MSTORE PUSH1 0x20 PUSH1 0x00 LOG0";

/// Compiles `code` as rskj's `BytecodeCompiler` does: each word is an
/// opcode's name, in any case, or `0x` and bytes in hex, which go in as
/// they are. rskj's names are read as rskj reads them: DUPN, SWAPN and
/// TXINDEX are its own opcodes (0xa8 to 0xaa), not EIP-8024's.
///
/// Two more words let a program branch: `name:` is a JUMPDEST, and `@name`
/// pushes where that JUMPDEST is, with a PUSH2.
pub(crate) fn compile(code: &str) -> Vec<u8> {
    let tokens: Vec<String> = code
        .split_whitespace()
        .map(str::to_ascii_uppercase)
        .collect();
    let mut labels = std::collections::HashMap::new();
    let mut at = 0;
    for token in &tokens {
        if let Some(label) = token.strip_suffix(':') {
            labels.insert(label.to_string(), at);
        }
        at += if token.starts_with('@') {
            3
        } else if let Some(hex) = token.strip_prefix("0X") {
            hex.len() / 2
        } else {
            1
        };
    }
    let mut bytes = Vec::new();
    for token in &tokens {
        if token.ends_with(':') {
            bytes.push(0x5b); // JUMPDEST
        } else if let Some(label) = token.strip_prefix('@') {
            let Some(at) = labels.get(label) else {
                panic!("no label {label}");
            };
            let at = u16::try_from(*at).expect("a label within 64 KB");
            bytes.push(0x61); // PUSH2
            bytes.extend(at.to_be_bytes());
        } else if let Some(hex) = token.strip_prefix("0X") {
            bytes.extend(hex::decode(hex).expect("hex bytes"));
        } else {
            bytes.push(opcode(token).unwrap_or_else(|| panic!("no opcode {token}")));
        }
    }
    bytes
}

fn opcode(name: &str) -> Option<u8> {
    match name {
        "DUPN" => Some(0xa8),
        "SWAPN" => Some(0xa9),
        "TXINDEX" => Some(0xaa),
        "SHA3" => Some(0x20),
        "SUICIDE" => Some(0xff),
        _ => (0..=255u8)
            .find(|byte| revm::bytecode::OpCode::new(*byte).is_some_and(|op| op.as_str() == name)),
    }
}

/// Code that ends a program by returning the top `words` words of its
/// stack, the top one first.
pub(crate) fn return_top(words: usize) -> Vec<u8> {
    let mut code = Vec::new();
    for word in 0..words {
        code.push(0x61); // PUSH2
        code.extend(((word * 32) as u16).to_be_bytes());
        code.push(0x52); // MSTORE
    }
    code.push(0x61); // PUSH2
    code.extend(((words * 32) as u16).to_be_bytes());
    code.extend([0x60, 0x00, 0xf3]); // PUSH1 0, RETURN
    code
}

/// The words of a program's output.
pub(crate) fn words(output: &[u8]) -> Vec<U256> {
    output.chunks(32).map(U256::from_be_slice).collect()
}

/// The word each log of a transaction holds, in order: what [`LOG_TOP`]
/// logged.
pub(crate) fn logged(result: &TxExecutionResult) -> Vec<U256> {
    let data = result.logs.iter().map(|log| &log.data.data[..]);
    data.map(U256::from_be_slice).collect()
}

/// `value` as a word in hex, for a PUSH32: `0x` and 64 digits.
pub(crate) fn hex_word(value: U256) -> String {
    format!("0x{}", hex::encode(value.to_be_bytes::<32>()))
}

/// PUSH20 of `address`, for [`compile`].
pub(crate) fn push_address(address: Address) -> String {
    format!("PUSH20 0x{}", hex::encode(address))
}

/// A transaction calling `to` with `data`, at a gas price of 0.
pub(crate) fn call(nonce: u64, to: Address, value: U256, gas: u64, data: &[u8]) -> Transaction {
    Transaction {
        nonce,
        gas_price: U256::ZERO,
        gas_limit: U256::from(gas),
        to: Bytes::copy_from_slice(to.as_slice()),
        value,
        input: Bytes::copy_from_slice(data),
        v: 0,
        r: U256::ZERO,
        s: U256::ZERO,
        cached_rlp: None,
    }
}

/// A transaction creating a contract of the init code `data`, at a gas
/// price of 0.
pub(crate) fn create(nonce: u64, gas: u64, data: &[u8]) -> Transaction {
    Transaction {
        to: Bytes::new(),
        ..call(nonce, Address::ZERO, U256::ZERO, gas, data)
    }
}

/// A chain on one network's rules: its state, and the number of the next
/// block, which each block run moves on by one.
pub(crate) struct Chain {
    executor: RskExecutor,
    store: Arc<MemoryTrieStore>,
    root: TrieNode,
    /// The next block's number.
    pub(crate) number: u64,
    blocks: Arc<BlockStore>,
    _blocks: tempfile::TempDir,
}

impl Chain {
    pub(crate) fn new(config: RskHardforkConfig, number: u64) -> Self {
        let folder = tempfile::tempdir().expect("a folder for the block store");
        let blocks = Arc::new(BlockStore::open(folder.path()).expect("a block store"));
        Self {
            executor: RskExecutor::new(config, blocks.clone()),
            store: Arc::new(MemoryTrieStore::new()),
            root: TrieNode::empty(),
            number,
            blocks,
            _blocks: folder,
        }
    }

    /// The chain, its executor changed by `change`: given REMASC's constants
    /// or the Bridge's, say.
    pub(crate) fn map_executor(self, change: impl FnOnce(RskExecutor) -> RskExecutor) -> Self {
        Self {
            executor: change(self.executor),
            ..self
        }
    }

    /// A chain on mainnet's rules whose next block is `number`.
    pub(crate) fn mainnet(number: u64) -> Self {
        Self::new(RskHardforkConfig::mainnet(), number)
    }

    /// Writes an account with `nonce` and `balance`, and `code` where it
    /// has any.
    pub(crate) fn put(&mut self, address: Address, nonce: u64, balance: U256, code: &[u8]) {
        let store = self.store.as_ref();
        let account = AccountState::new(U256::from(nonce), balance).encode();
        let key = TrieKeySlice::from_key(&rustock_trie::account_key(&address));
        self.root = self.root.put(&key, &account, store);
        if !code.is_empty() {
            let key = TrieKeySlice::from_key(&rustock_trie::code_key(&address));
            self.root = self.root.put(&key, code, store);
        }
    }

    /// The next block's header.
    pub(crate) fn header(&self) -> Header {
        Header {
            parent_hash: B256::ZERO,
            ommers_hash: B256::ZERO,
            beneficiary: Address::repeat_byte(0x01),
            state_root: B256::ZERO,
            transactions_root: B256::ZERO,
            receipts_root: B256::ZERO,
            logs_bloom: Bloom::ZERO,
            extension_data: None,
            difficulty: U256::from(1_000_000),
            number: self.number,
            gas_limit: U256::from(6_800_000),
            gas_used: 0,
            timestamp: 1_700_000_000,
            extra_data: Bytes::new(),
            paid_fees: U256::ZERO,
            minimum_gas_price: U256::ZERO,
            uncle_count: 0,
            umm_root: rustock_core::config::ActivationHeights::current()
                .umm_active(self.number)
                .then(Bytes::new),
            version_fields: None,
            bitcoin_merged_mining_header: None,
            bitcoin_merged_mining_merkle_proof: None,
            bitcoin_merged_mining_coinbase_transaction: None,
            cached_hash: None,
            cached_hash_for_merged_mining: None,
            kept: None,
        }
    }

    /// Runs `transactions` in the next block, or says what refuses the block.
    /// A block that runs leaves its state for the next.
    pub(crate) fn try_block(
        &mut self,
        transactions: &[(Transaction, Address)],
    ) -> Result<BlockExecutionResult, ExecutionError> {
        let result = self.executor.execute_block(
            &self.header(),
            transactions,
            &self.root,
            self.store.clone(),
        )?;
        self.apply(&result);
        self.number += 1;
        Ok(result)
    }

    /// Runs `transactions` in the block `header` heads, whose uncles are
    /// `ommers`, and keeps the block as the chain's at its height, as an
    /// imported block is kept: REMASC reads the blocks before it there.
    pub(crate) fn import(
        &mut self,
        header: &Header,
        ommers: &[Header],
        transactions: &[(Transaction, Address)],
    ) -> BlockExecutionResult {
        let store = self.store.clone();
        let result = self
            .executor
            .execute_block(header, transactions, &self.root, store);
        let result = result.expect("the block runs");
        self.apply(&result);
        let hash = header.hash();
        let blocks = &self.blocks;
        blocks
            .put_header_with_hash(hash, header)
            .expect("the header is kept");
        blocks
            .put_canonical_hash(header.number, hash)
            .expect("the height is kept");
        blocks
            .put_body(hash, &[], ommers)
            .expect("the uncles are kept");
        self.number = header.number + 1;
        result
    }

    /// Leaves the state a block that ran left.
    fn apply(&mut self, result: &BlockExecutionResult) {
        self.root = match &result.applied_root {
            Some(root) => root.clone(),
            None => crate::state::apply_state_changes(
                &self.root,
                self.store.as_ref(),
                &result.state_changes,
                &result.markers,
            ),
        };
    }

    /// Runs `transactions` in the next block.
    pub(crate) fn block(
        &mut self,
        transactions: &[(Transaction, Address)],
    ) -> BlockExecutionResult {
        self.try_block(transactions).expect("the block runs")
    }

    /// Runs `transactions` as the miner builds the next block from them,
    /// leaving the chain as it was: what ran, and what the miner left out.
    pub(crate) fn mine(
        &self,
        transactions: &[(Transaction, Address)],
    ) -> Result<(BlockExecutionResult, MiningDrops), ExecutionError> {
        let (header, store) = (self.header(), self.store.clone());
        self.executor
            .execute_block_for_mining(&header, transactions, &self.root, store)
    }

    /// Runs `transaction` from `sender`, alone in the next block.
    pub(crate) fn run(&mut self, sender: Address, transaction: Transaction) -> TxExecutionResult {
        self.block(&[(transaction, sender)]).tx_results.remove(0)
    }

    /// The account at `address`, where there is one.
    pub(crate) fn account(&self, address: Address) -> Option<AccountState> {
        let key = TrieKeySlice::from_key(&rustock_trie::account_key(&address));
        let encoded = self.root.get(&key, self.store.as_ref())?;
        Some(AccountState::decode(&encoded).expect("an account"))
    }

    /// The balance at `address`: zero where there is no account.
    pub(crate) fn balance(&self, address: Address) -> U256 {
        self.account(address)
            .map_or(U256::ZERO, |account| account.balance)
    }

    /// The code at `address`: none where there is none.
    pub(crate) fn code(&self, address: Address) -> Vec<u8> {
        let key = rustock_trie::code_key(&address);
        let code = self
            .root
            .get(&TrieKeySlice::from_key(&key), self.store.as_ref());
        code.map(|code| code.to_vec()).unwrap_or_default()
    }

    /// The word at `slot` of `address`'s storage: zero where none is.
    pub(crate) fn storage(&self, address: Address, slot: u64) -> U256 {
        self.storage_word(address, U256::from(slot))
    }

    /// The word at `key` of `address`'s storage: zero where none is.
    pub(crate) fn storage_word(&self, address: Address, key: U256) -> U256 {
        let key = rustock_trie::storage_key(&address, &B256::from(key));
        let value = self
            .root
            .get(&TrieKeySlice::from_key(&key), self.store.as_ref());
        value.map_or(U256::ZERO, |value| U256::from_be_slice(&value))
    }

    /// Writes `bytes` at `key` of `address`'s storage, as rskj's
    /// `addStorageBytes` writes a value of any length.
    pub(crate) fn put_storage_bytes(&mut self, address: Address, key: U256, bytes: &[u8]) {
        let key = rustock_trie::storage_key(&address, &B256::from(key));
        let store = self.store.as_ref();
        self.root = self.root.put(&TrieKeySlice::from_key(&key), bytes, store);
    }
}

/// What a transaction of [`GAS`] gets from calling a contract whose code is
/// `code`, alone in block `number` of a chain on `config`'s rules.
pub(crate) fn run_code_on(
    config: RskHardforkConfig,
    number: u64,
    code: &[u8],
) -> TxExecutionResult {
    let mut chain = Chain::new(config, number);
    chain.put(SENDER, 0, ONE_RBTC, &[]);
    chain.put(CONTRACT, 1, U256::ZERO, code);
    chain.run(SENDER, call(0, CONTRACT, U256::ZERO, GAS, &[]))
}

/// [`run_code_on`] mainnet's rules.
pub(crate) fn run_code(number: u64, code: &[u8]) -> TxExecutionResult {
    run_code_on(RskHardforkConfig::mainnet(), number, code)
}
