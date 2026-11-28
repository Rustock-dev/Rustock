//! An SPV client reconstructing an account's history, end to end.
//!
//! Everything else on `main` shows that a piece works. This shows the pieces
//! together do the thing that motivated them:
//!
//! > a wallet that holds no state learns every block in which its account's
//! > rBTC balance moved, from a node that holds no state either, over proofs
//! > neither of them has to trust.
//!
//! Native rBTC is the hard case. A value transfer emits no log, so it sets no
//! bloom bit, so a client watching the headers sees nothing at all. Nothing
//! smaller than this test exercises the whole route: checkpoint -> headers ->
//! proof fetch -> account trace -> recovered payments, against a node with an
//! empty trie.
//!
//! # Why this is the emulation variant, and not a port
//!
//! The witness branch carries this test in a form that reads
//! `lastModifiedBlock` straight out of the verified account record. That
//! field does not exist on `main`: the account record here is
//! `(nonce, balance, stateFlags)` and carries no witness. The dependency is
//! **real, not incidental** -- it is the consensus change the RSKIP proposes
//! -- so the walk cannot simply be copied over.
//!
//! What `main` has instead is `rsk_lastAccountChange`, which answers the same
//! question by **bisecting** the range: does the record at this height
//! already equal the record at the top? That predicate is false below the
//! change and true from it onward, so the smallest height satisfying it is
//! the block the change happened in. The wallet's walk is identical in shape;
//! only the cost of a hop differs, and this test measures that difference
//! rather than asserting it (see `the_emulation_costs_a_bisection_per_hop`).
//!
//! The emulation is also strictly weaker in one way the test pins down: a
//! bisection finds the *last* change in a range, so two movements that
//! cancel out between probes are invisible to it, where a witness field
//! would name each block. `rsk_lastAccountChange` says which source answered
//! (`witnessSource: "searched"`) precisely so a wallet can tell the user what
//! kind of history it is showing.
//!
//! # The shape
//!
//! A regtest chain is built from genesis. Most of its blocks have nothing to
//! do with account X -- they move value between two other addresses -- and a
//! handful pay X. The test then throws away all knowledge of which blocks
//! those were and recovers them, the way a wallet would:
//!
//! 1. ask the SPV node for X's last change at the head;
//! 2. read the block it names, and prove X's balance there;
//! 3. ask again, bounded above by the block *before* that one;
//! 4. repeat until the answer is "no change in range" with `complete: true`.
//!
//! Each hop lands on exactly one block in which X actually changed, and the
//! wallet never opens the noise.
//!
//! The wallet talks only to the SPV node's RPC. The SPV node holds headers
//! and no trie, and gets every answer from the full node by way of the real
//! `SnapServer::trie_proof`, verifying each one against the state root in a
//! header it has itself.
//!
//! Tracked as #418 and as item 5 of `spv-emulation-mode.md` section 6.

use alloy_primitives::{Address, Bytes, B256, U256};
use k256::ecdsa::SigningKey;
use rustock_core::{Block, Header, Transaction};
use rustock_execution::hardfork::RskHardforkConfig;
use rustock_execution::processor::BlockProcessor;
use rustock_rpc::server::{ProofFetcher, RpcState};
use rustock_storage::BlockStore;
use rustock_sync::snap::server::SnapServer;
use rustock_sync::snap::SnapConfig;
use rustock_trie::{MemoryTrieStore, TrieNode, TrieStore};
use serde_json::json;
use std::sync::Arc;

const CHAIN_ID: u64 = 33;
/// Blocks in the chain. Most of them have nothing to do with X.
const CHAIN_LENGTH: u64 = 24;
/// The blocks that actually pay X. Everything else is noise, and the point
/// of the exercise is that the wallet never visits the noise.
const PAYS_X: &[u64] = &[3, 7, 8, 16];
const PAYMENT: u64 = 1_000_000_000_000_000;

fn x_address() -> Address {
    Address::repeat_byte(0xAB)
}

/// A contract, so the chain has code to prove (#393).
fn contract_address() -> Address {
    Address::repeat_byte(0xC0)
}

/// Its code. The bytes do not have to execute -- what is being proved is
/// that the client gets *these* bytes and can check them against the state
/// root -- but they are a real `PUSH1 0 PUSH1 0 RETURN` so nothing here
/// depends on code being arbitrary.
const CONTRACT_CODE: &[u8] = &[0x60, 0x00, 0x60, 0x00, 0xf3];

// ---------------------------------------------------------------------------
// Building a regtest chain
// ---------------------------------------------------------------------------

fn sender_address(key: &SigningKey) -> Address {
    let pk = key.verifying_key().to_encoded_point(false);
    let hash = <sha3::Keccak256 as sha3::Digest>::digest(&pk.as_bytes()[1..]);
    Address::from_slice(&hash[12..])
}

fn sign_tx(tx: &mut Transaction, key: &SigningKey, chain_id: u64) {
    let hash = tx.signing_hash_eip155(chain_id);
    let (signature, recid): (k256::ecdsa::Signature, k256::ecdsa::RecoveryId) =
        key.sign_prehash_recoverable(hash.as_slice()).unwrap();
    let sig_bytes = signature.to_bytes();
    tx.r = U256::from_be_slice(&sig_bytes[..32]);
    tx.s = U256::from_be_slice(&sig_bytes[32..]);
    tx.v = chain_id * 2 + 35 + recid.to_byte() as u64;
}

fn put_account(
    root: &TrieNode,
    store: &dyn TrieStore,
    addr: &Address,
    nonce: u64,
    balance: U256,
) -> TrieNode {
    let acct = rustock_trie::AccountState::new(U256::from(nonce), balance);
    root.put(
        &rustock_trie::TrieKeySlice::from_key(&rustock_trie::account_key(addr)),
        &acct.encode(),
        store,
    )
}

fn header_at(number: u64, parent: B256) -> Header {
    Header {
        parent_hash: parent,
        ommers_hash: B256::ZERO,
        beneficiary: Address::repeat_byte(0x01),
        state_root: B256::ZERO,
        transactions_root: B256::ZERO,
        receipts_root: B256::ZERO,
        logs_bloom: alloy_primitives::Bloom::ZERO,
        extension_data: None,
        difficulty: U256::from(1_000_000u64),
        number,
        gas_limit: U256::from(6_800_000u64),
        gas_used: 0,
        timestamp: 1_700_000_000 + number,
        extra_data: Bytes::new(),
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

/// Everything the chain left behind.
struct Chain {
    blocks: Arc<BlockStore>,
    trie: Arc<MemoryTrieStore>,
    /// Block number -> X's balance after it, for the blocks that paid X.
    expected: Vec<(u64, U256)>,
    _tmp: tempfile::TempDir,
}

fn build_chain() -> Chain {
    let tmp = tempfile::tempdir().expect("tempdir");
    let blocks = Arc::new(BlockStore::open(tmp.path()).expect("block store"));
    let trie = Arc::new(MemoryTrieStore::new());

    // No witness rule: this is `main`'s account record, three fields and no
    // history. Finding the changes is the wallet's problem, which is the
    // whole point of the exercise.
    let cfg = RskHardforkConfig::for_network(CHAIN_ID);
    let processor = BlockProcessor::new(cfg.clone(), blocks.clone());

    let payer = SigningKey::from_slice(&[7u8; 32]).unwrap();
    let payer_addr = sender_address(&payer);
    let bystander = Address::repeat_byte(0xCD);
    let funding = U256::from(10u64).pow(U256::from(20));

    let mut root = put_account(&TrieNode::empty(), trie.as_ref(), &payer_addr, 0, funding);

    // A contract with code and one storage slot, so the chain has something
    // to exercise the code and storage-root proofs against (#393). Written
    // straight into the trie rather than deployed: this test is about the
    // proof path, and a deployment transaction would only add a way for it
    // to fail for unrelated reasons.
    let contract = contract_address();
    root = put_account(&root, trie.as_ref(), &contract, 1, U256::ZERO);
    root = root.put(
        &rustock_trie::TrieKeySlice::from_key(&rustock_trie::code_key(&contract)),
        CONTRACT_CODE,
        trie.as_ref(),
    );
    // Several slots, not one. With a single slot the trie's path compression
    // leaves no node sitting exactly at the storage prefix, so `storageHash`
    // is legitimately the empty constant and the test would be asserting
    // nothing about the storage-root proof.
    for slot_byte in [1u8, 2, 3, 0x80] {
        let mut slot = [0u8; 32];
        slot[31] = slot_byte;
        root = root.put(
            &rustock_trie::TrieKeySlice::from_key(&rustock_trie::storage_key(
                &contract,
                &B256::from(slot),
            )),
            &[slot_byte],
            trie.as_ref(),
        );
    }

    // A genesis block, carrying the funded state. The chain needs a real #0
    // for the same reason a real chain has one: `rsk_lastAccountChange`
    // searches a range down to a block it can resolve, and a search whose
    // floor does not exist cannot report itself complete. Without this the
    // wallet could never distinguish "X never moved" from "I could not look
    // far enough", which is the distinction the whole method is built on.
    root.save(trie.as_ref(), true);
    let genesis_root = root.compute_hash(trie.as_ref());
    rustock_trie::TrieStore::put(
        trie.as_ref(),
        genesis_root.as_slice(),
        &root.to_message(trie.as_ref()),
    );
    let mut genesis = header_at(0, B256::ZERO);
    genesis.state_root = genesis_root;
    let genesis_hash = genesis.hash();
    blocks.put_header(&genesis).expect("put genesis");
    blocks.put_header_number(genesis_hash, 0).expect("genesis number");
    blocks.put_canonical_hash(0, genesis_hash).expect("genesis canonical");
    blocks.set_head(genesis_hash).expect("genesis head");
    blocks.set_exec_head(genesis_hash, genesis_root).expect("genesis exec head");

    let mut parent = genesis_hash;
    let mut expected: Vec<(u64, U256)> = Vec::new();
    let mut x_balance = U256::ZERO;

    for number in 1..=CHAIN_LENGTH {
        let pays_x = PAYS_X.contains(&number);
        let (to, value) = if pays_x {
            (x_address(), U256::from(PAYMENT))
        } else {
            // Noise: value between two addresses the wallet never asks about.
            (bystander, U256::from(1u64))
        };

        let mut tx = Transaction {
            nonce: number - 1,
            gas_price: U256::ZERO,
            gas_limit: U256::from(21_000u64),
            to: Bytes::copy_from_slice(to.as_slice()),
            value,
            input: Bytes::default(),
            v: 0,
            r: U256::ZERO,
            s: U256::ZERO,
            cached_rlp: None,
        };
        sign_tx(&mut tx, &payer, CHAIN_ID);

        // Two passes, as the node does: a header has to carry the roots its
        // own execution produces, so run it once to learn them and then
        // build the header that commits to them.
        let draft = Block {
            header: header_at(number, parent),
            transactions: vec![tx.clone()],
            ommers: vec![],
        };
        let learned = processor
            .execute_block(&draft, &root, trie.clone())
            .unwrap_or_else(|e| panic!("#{number} must execute: {e:?}"));

        let mut header = header_at(number, parent);
        header.state_root = learned.state_root_hash;
        header.receipts_root = learned.receipts_root;
        header.transactions_root = rustock_core::ordered_tx_trie_root(
            std::slice::from_ref(&tx),
            cfg.has_unitrie_state_root(number),
        );
        header.ommers_hash = rustock_execution::processor::compute_ommers_hash(&[]);
        header.gas_used = learned.gas_used;
        header.paid_fees = learned.paid_fees;
        header.logs_bloom = learned.logs_bloom;

        let block = Block { header: header.clone(), transactions: vec![tx], ommers: vec![] };
        let hash = block.hash();

        let committed = processor
            .process_block(&block, &root, trie.clone())
            .unwrap_or_else(|e| panic!("#{number} must process: {e:?}"));

        blocks.put_header(&header).expect("put header");
        // The full node holds bodies; the SPV node is the one that does not.
        // Storing them is what lets a test check that the blocks the wallet
        // skipped carried real transactions rather than being empty.
        blocks.put_body(hash, &block.transactions, &[]).expect("put body");
        blocks.put_header_number(hash, number).expect("put number");
        blocks.put_canonical_hash(number, hash).expect("canonical");
        blocks.set_head(hash).expect("head");
        blocks.set_exec_head(hash, committed.state_root_hash).expect("exec head");

        root = committed.new_state_root;
        parent = hash;
        if pays_x {
            x_balance += U256::from(PAYMENT);
            expected.push((number, x_balance));
        }
    }

    Chain { blocks, trie, expected, _tmp: tmp }
}

// ---------------------------------------------------------------------------
// The two nodes
// ---------------------------------------------------------------------------

/// The SPV node's link to a peer: the real server code, reached directly
/// rather than over a socket.
///
/// The wire codec is covered by its own round-trip tests; what matters here
/// is that the proof the server actually builds is the one the verifier
/// actually accepts, and that nothing in between quietly substitutes local
/// state for a peer's answer.
struct FullNodePeer(SnapServer);

#[async_trait::async_trait]
impl ProofFetcher for FullNodePeer {
    async fn fetch_proof(
        &self,
        ask: rustock_rpc::server::ProofAsk,
    ) -> Result<rustock_rpc::server::FetchedProof, String> {
        let req = rustock_networking::protocol::rsk::TrieProofRequest {
            id: 1,
            block_number: ask.block_number,
            address: ask.address,
            storage_keys: ask.storage_keys,
            include_code: ask.include_code,
            include_storage_root: ask.include_storage_root,
        };
        let resp = self.0.trie_proof(&req).ok_or("the peer would not answer")?;
        Ok(rustock_rpc::server::FetchedProof {
            account: resp.account_proof,
            storage: resp.storage_proofs,
            code: resp.code_proof,
            storage_root: resp.storage_root_proof,
        })
    }
}

/// The same node, but holding its own trie: what the SPV answers are
/// measured against.
fn full_state(chain: &Chain) -> RpcState {
    let mut state = spv_state(chain);
    state.trie_store = Some(chain.trie.clone() as Arc<dyn TrieStore>);
    state.proof_fetcher = None;
    state
}

fn spv_state(chain: &Chain) -> RpcState {
    let snap_config = SnapConfig { server_enabled: true, ..Default::default() };
    let peer = FullNodePeer(SnapServer::new(
        chain.blocks.clone(),
        chain.trie.clone() as Arc<dyn TrieStore>,
        snap_config,
    ));

    RpcState {
        store: chain.blocks.clone(),
        peer_store: Arc::new(rustock_networking::PeerStore::new()),
        config: Arc::new(rustock_core::config::ChainConfig::regtest()),
        tx_submitter: None,
        // No state of its own. This is what makes it an SPV node, and
        // leaving a trie here would let the answers come from local data and
        // prove nothing.
        trie_store: None,
        proof_fetcher: Some(Arc::new(peer)),
        hardfork_cfg: None,
        filter_store: Arc::new(rustock_rpc::logs::FilterStore::new()),
        tx_pool: None,
        wire_queue_depth: None,
        gas_price: None,
        epoch_store: None,
        snapshots: None,
        miner: None,
        auto_miner: None,
        wallet: None,
        admin_enabled: false,
        // The wallet's walk is a bisection, so the method it depends on has
        // to be switched on. Off by default because one call costs ten to
        // twenty-four state lookups -- which is the cost this test measures.
        account_search_enabled: true,
        bridge_constants: None,
        gc_bounds: None,
        // No bound: the fixture's timestamps are fixed and in the past, so a
        // staleness bound would refuse every `latest` read here (#394).
        max_head_age: None,
        gc_burial: 0,
        gc_pins: None,
        prune_keep_depth: 0,
        prune_max_batch: 0,
        logs_max_blocks: 1000,
        gas_estimator_binary_search: false,
        scoring: None,
        events: None,
    }
}

// ---------------------------------------------------------------------------
// The wallet
// ---------------------------------------------------------------------------

async fn call(state: &RpcState, method: &str, params: serde_json::Value) -> serde_json::Value {
    let req = rustock_rpc::types::JsonRpcRequest {
        jsonrpc: Some("2.0".into()),
        method: method.into(),
        params,
        id: Some(json!(1)),
    };
    let resp = rustock_rpc::server::dispatch_rpc(state, req).await;
    resp.result
        .unwrap_or_else(|| panic!("{method} failed: {:?}", resp.error))
}

/// X's balance at `at`, proved against that block's state root.
///
/// This goes through `eth_getProof` on a node with no trie, so the number
/// comes from the peer's proof and the SPV node's verification of it, not
/// from anything local.
async fn proved_balance(state: &RpcState, addr: Address, at: &str) -> U256 {
    let r = call(state, "eth_getProof", json!([format!("{addr:#x}"), [], at])).await;
    U256::from_str_radix(r["balance"].as_str().unwrap().trim_start_matches("0x"), 16).unwrap()
}

/// One hop of the walk: the newest block at or below `ceiling` in which X
/// changed, and what the search cost.
///
/// `None` with `complete: true` means the range was searched to its bottom
/// and held no change -- the walk's terminating condition. `complete: false`
/// would mean the search floor cut it short, which on this fixture must
/// never happen and is asserted against.
struct Hop {
    block: Option<u64>,
    probes: u64,
}

async fn last_change_at_or_below(state: &RpcState, addr: Address, ceiling: u64) -> Hop {
    let r = call(
        state,
        "rsk_lastAccountChange",
        json!([format!("{addr:#x}"), format!("{ceiling:#x}"), "0x0"]),
    )
    .await;

    assert_eq!(
        r["witnessSource"], "searched",
        "on main there is no witness field, so the answer must come from the search"
    );
    assert_eq!(
        r["complete"], true,
        "the range ran to genesis, so an incomplete answer would mean the search \
         gave up early and the walk below it would silently lose payments"
    );

    Hop {
        block: r["lastModifiedBlock"].as_str().map(|s| {
            u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap()
        }),
        probes: r["probes"].as_u64().unwrap(),
    }
}

// ---------------------------------------------------------------------------
// The journey
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_wallet_recovers_an_accounts_whole_history_through_an_spv_node() {
    let chain = build_chain();
    let spv = spv_state(&chain);
    let x = x_address();

    // Sanity: the chain really did what the fixture claims.
    assert_eq!(chain.expected.len(), PAYS_X.len());
    let final_balance = chain.expected.last().unwrap().1;

    let head = proved_balance(&spv, x, "latest").await;
    assert_eq!(head, final_balance, "the head balance must be proved, not trusted");

    // --- the walk, exactly as a wallet would do it ---
    let mut found: Vec<(u64, U256)> = Vec::new();
    let mut ceiling = CHAIN_LENGTH;
    let mut hops = 0usize;

    loop {
        assert!(hops <= CHAIN_LENGTH as usize, "the walk must terminate");
        let hop = last_change_at_or_below(&spv, x, ceiling).await;
        let Some(block) = hop.block else { break };
        hops += 1;

        let balance = proved_balance(&spv, x, &format!("{block:#x}")).await;
        found.push((block, balance));

        if block == 0 {
            break;
        }
        ceiling = block - 1;
    }

    found.reverse();
    assert_eq!(
        found, chain.expected,
        "the wallet must recover exactly the blocks that paid X, and the balance after each"
    );
    assert_eq!(hops, PAYS_X.len(), "one hop per block that actually changed X");
}

/// **The negative half, and the reason a witness is wanted at all.**
///
/// The blocks the wallet skipped really do contain transactions, and X really
/// is untouched by them. A client comparing balances would have had to open
/// every one, and a client watching blooms would have seen none of them.
#[tokio::test]
async fn the_blocks_the_wallet_skipped_did_not_touch_x() {
    let chain = build_chain();
    let spv = spv_state(&chain);
    let x = x_address();

    let mut previous = U256::ZERO;
    for number in 1..=CHAIN_LENGTH {
        let balance = proved_balance(&spv, x, &format!("{number:#x}")).await;
        if PAYS_X.contains(&number) {
            assert!(balance > previous, "#{number} paid X, so the balance must rise");
        } else {
            assert_eq!(balance, previous, "#{number} did not touch X and must not move it");
            // And the block is not empty -- the noise is real traffic.
            let hash = chain.blocks.canonical_hash(number).unwrap().unwrap();
            let (txs, _) = chain.blocks.body(hash).unwrap().unwrap();
            assert_eq!(txs.len(), 1, "#{number} must carry a transaction to be real noise");
        }
        previous = balance;
    }
}

/// **What the emulation costs, measured rather than claimed.**
///
/// A witness field answers a hop in one proof. The bisection answers it in
/// `probes`, which is logarithmic in the range. This test records the real
/// total for this fixture so that the RSKIP's argument rests on a number
/// produced by running the code, and so that a change making the search
/// quietly linear is caught here rather than in a wallet.
///
/// It asserts a bound, not an exact figure: the exact count depends on where
/// the changes fall, and pinning it would make the test brittle without
/// making it stronger. What matters is that a hop is logarithmic and not a
/// scan.
#[tokio::test]
async fn the_emulation_costs_a_bisection_per_hop() {
    let chain = build_chain();
    let spv = spv_state(&chain);
    let x = x_address();

    let mut total = 0u64;
    let mut hops = 0u64;
    let mut ceiling = CHAIN_LENGTH;
    loop {
        let hop = last_change_at_or_below(&spv, x, ceiling).await;
        total += hop.probes;
        let Some(block) = hop.block else { break };
        hops += 1;
        if block == 0 {
            break;
        }
        ceiling = block - 1;
    }

    // One hop per payment, plus the final "nothing left" answer.
    assert_eq!(hops, PAYS_X.len() as u64);

    // Logarithmic, not linear: a scan of the whole chain once per hop would
    // be CHAIN_LENGTH * hops probes. The bound is generous on purpose.
    let scan = CHAIN_LENGTH * (hops + 1);
    assert!(
        total < scan,
        "the search must beat a scan: {total} probes against {scan} for scanning"
    );
    println!(
        "emulation cost on this fixture: {total} proofs for {hops} changes \
         over {CHAIN_LENGTH} blocks; a witness field would have cost {}",
        hops + 1
    );
}

/// Everything above would read the same if the SPV node were quietly
/// answering from a local trie. It has none — this is what says so.
#[tokio::test]
async fn the_spv_node_has_no_state_of_its_own() {
    let chain = build_chain();
    let spv = spv_state(&chain);
    assert!(spv.trie_store.is_none(), "an SPV node must hold no trie");

    // With no peer either, the same question has no answer at all: the
    // balances above came over the link, not from local data.
    let mut stranded = spv_state(&chain);
    stranded.proof_fetcher = None;
    let req = rustock_rpc::types::JsonRpcRequest {
        jsonrpc: Some("2.0".into()),
        method: "eth_getProof".into(),
        params: json!([format!("{:#x}", x_address()), [], "latest"]),
        id: Some(json!(1)),
    };
    let resp = rustock_rpc::server::dispatch_rpc(&stranded, req).await;
    assert!(resp.result.is_none(), "with no trie and no peer there is nothing to answer from");
}

/// An address that never appears in the chain is proved absent, and the
/// search says so completely rather than leaving it ambiguous (#392).
///
/// This is a new wallet's first screen, so it is worth having in the
/// end-to-end test and not only in the unit coverage.
#[tokio::test]
async fn a_never_used_address_is_answered_completely() {
    let chain = build_chain();
    let spv = spv_state(&chain);
    let stranger = Address::repeat_byte(0x5E);

    assert_eq!(proved_balance(&spv, stranger, "latest").await, U256::ZERO);

    let hop = last_change_at_or_below(&spv, stranger, CHAIN_LENGTH).await;
    assert_eq!(
        hop.block, None,
        "an address that never moved has no last-change block"
    );
}

/// **Code proved over the wire, through the real server (#393).**
///
/// Before this, `TrieProofRequest` carried an address and storage keys and
/// nothing else, so an SPV node could not ask a peer for code at all — and
/// RSK's account record has no `codeHash` to check hashed bytes against, so
/// there was no second route either. A verified `eth_call` was closed at the
/// protocol level, which is what shows an ERC-20 balance without a
/// per-token storage-layout oracle.
///
/// The assertion is not that a field appeared. It is that the SPV node's
/// answer equals the full node's, computed from its own trie — the same
/// standard the balance walk above is held to.
#[tokio::test]
async fn an_spv_node_proves_contract_code_and_the_storage_root() {
    use sha3::Digest;

    let chain = build_chain();
    let spv = spv_state(&chain);
    let contract = contract_address();

    // The two proof fields are opt-in: `accountProof` does not cover
    // `codeHash` or `storageHash` on RSK, so a caller that wants to check
    // them has to ask for the evidence.
    let asking = json!([
        format!("{contract:#x}"),
        [],
        "latest",
        {"includeCode": true, "includeStorageRoot": true}
    ]);
    let r = call(&spv, "eth_getProof", asking.clone()).await;

    let expected = format!(
        "{:#x}",
        B256::from_slice(&sha3::Keccak256::digest(CONTRACT_CODE))
    );
    assert_eq!(
        r["codeHash"].as_str(),
        Some(expected.as_str()),
        "the SPV node must report the hash of the code the peer proved"
    );

    // The storage root is the hash of a *node*, not of a value, so it has to
    // be checked against what a node reading its own trie computes rather
    // than against anything this test works out for itself.
    let full = full_state(&chain);
    let truth = call(&full, "eth_getProof", asking).await;
    assert_eq!(
        r["storageHash"], truth["storageHash"],
        "the proved storage root must equal the one read from the trie"
    );
    assert_eq!(r["codeHash"], truth["codeHash"]);
    assert_ne!(
        r["storageHash"].as_str(),
        Some(format!("{:#x}", B256::ZERO).as_str()),
        "the fixture must leave a node at the storage prefix, or this proves nothing"
    );

    // And the SPV node can hand over the code itself, which is what an EVM
    // needs and what `eth_getCode` could not serve here at all before #393.
    let code = call(
        &spv,
        "eth_getCode",
        json!([format!("{contract:#x}"), "latest"]),
    )
    .await;
    assert_eq!(
        code.as_str(),
        Some(format!("0x{}", hex::encode(CONTRACT_CODE)).as_str()),
        "the SPV node must serve the proved code, not just its hash"
    );
    assert_eq!(
        code,
        call(&full, "eth_getCode", json!([format!("{contract:#x}"), "latest"])).await,
        "and the same bytes a node reading its own trie returns"
    );
}

/// An account with no code is proved to have none, rather than left silent.
///
/// `x` is a plain address that only ever received value. The empty code hash
/// here is a *proved* absence — a divergence path the client verified — not
/// the default a client would assume.
#[tokio::test]
async fn an_account_with_no_code_is_proved_to_have_none() {
    use sha3::Digest;

    let chain = build_chain();
    let spv = spv_state(&chain);

    let r = call(
        &spv,
        "eth_getProof",
        json!([
            format!("{:#x}", x_address()),
            [],
            "latest",
            {"includeCode": true, "includeStorageRoot": true}
        ]),
    )
    .await;

    let empty = format!("{:#x}", B256::from_slice(&sha3::Keccak256::digest([])));
    assert_eq!(r["codeHash"].as_str(), Some(empty.as_str()));

    // `0x` here is a *proved* absence -- the peer sent a divergence path and
    // this node verified it -- not the default a client would assume.
    assert_eq!(
        call(
            &spv,
            "eth_getCode",
            json!([format!("{:#x}", x_address()), "latest"])
        )
        .await,
        json!("0x")
    );
    assert_eq!(
        r["storageHash"].as_str(),
        Some(format!("{:#x}", B256::ZERO).as_str()),
        "and an account with no storage has the empty storage root"
    );
}

/// **The proof fields are opt-in, and silence is the default.**
///
/// A wallet that did not ask for a contract's bytecode must not receive it,
/// and an intermediary must not decide otherwise on its behalf: the SPV
/// node forwards the caller's ask to its peer rather than adding to it. So
/// with no fourth parameter, the peer is never asked and the two fields the
/// peer would have had to prove are absent.
#[tokio::test]
async fn the_proof_fields_are_not_served_unless_asked_for() {
    let chain = build_chain();
    let spv = spv_state(&chain);

    let r = call(
        &spv,
        "eth_getProof",
        json!([format!("{:#x}", contract_address()), [], "latest"]),
    )
    .await;

    assert!(r.get("codeProof").is_none(), "not asked for, not sent");
    assert!(r.get("storageRootProof").is_none());
    assert!(
        r.get("codeHash").is_none(),
        "and the hash goes with its evidence: this node cannot prove it, so it does not assert it"
    );
    assert!(r.get("balance").is_some(), "the rest of the answer is unaffected");
}
