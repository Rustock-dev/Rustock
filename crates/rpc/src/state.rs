use crate::helpers::{parse_b256, parse_block_number, parse_storage_slot, to_hex_bytes, to_hex_u256};
use crate::server::RpcState;
use crate::types::*;
use alloy_primitives::{Address, B256};
use rustock_core::Header;
use rustock_storage::BlockStore;
use rustock_trie::{account_key, code_key, storage_key, AccountState, TrieKeySlice, TrieNode};
use serde_json::{json, Value};

pub fn eth_get_balance(id: Value, params: &Value, state: &RpcState) -> JsonRpcResponse {
    let Some(addr) = params.get(0).and_then(|v| v.as_str()).and_then(|s| s.parse::<Address>().ok()) else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing or invalid address");
    };
    let (root, _header) = match queried_state(&id, params.get(1), state) {
        Ok(found) => found,
        Err(refused) => return refused,
    };

    let key = account_key(&addr);
    match trie_get_account(&root, &key, state) {
        Some(acct) => JsonRpcResponse::success(id, json!(to_hex_u256(&acct.balance))),
        None => JsonRpcResponse::success(id, json!("0x0")),
    }
}

pub fn eth_get_transaction_count(id: Value, params: &Value, state: &RpcState) -> JsonRpcResponse {
    let Some(addr) = params.get(0).and_then(|v| v.as_str()).and_then(|s| s.parse::<Address>().ok()) else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing or invalid address");
    };
    // For "pending", return the pool's pending nonce if available
    if params.get(1).and_then(|v| v.as_str()) == Some("pending") {
        if let Some(pool) = &state.tx_pool {
            if let Some(pending_nonce) = pool.pending_nonce(&addr) {
                return JsonRpcResponse::success(
                    id,
                    json!(format!("0x{:x}", pending_nonce)),
                );
            }
        }
    }

    let (root, _header) = match queried_state(&id, params.get(1), state) {
        Ok(found) => found,
        Err(refused) => return refused,
    };

    let key = account_key(&addr);
    match trie_get_account(&root, &key, state) {
        Some(acct) => JsonRpcResponse::success(id, json!(to_hex_u256(&acct.nonce))),
        None => JsonRpcResponse::success(id, json!("0x0")),
    }
}

pub async fn eth_get_code(id: Value, params: &Value, state: &RpcState) -> JsonRpcResponse {
    let Some(addr) = params.get(0).and_then(|v| v.as_str()).and_then(|s| s.parse::<Address>().ok()) else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing or invalid address");
    };

    // A node with no trie asks a peer and checks the answer, exactly as
    // `eth_getProof` does. Before #393 the proof message could not ask for
    // code, so an SPV node could not serve this at all -- it failed in
    // `queried_state` on having no state, which is also the one thing
    // standing between an SPV wallet and a verified `eth_call`.
    if state.trie_store.is_none() {
        let named = crate::helpers::block_ref(params.get(1), &state.store)
            .and_then(|(identifier, _)| crate::helpers::information_block(&identifier));
        return match named {
            Ok(block) => light_code(id, addr, &block, state).await,
            Err((code, message)) => JsonRpcResponse::error(id, code, message),
        };
    }

    let (root, _header) = match queried_state(&id, params.get(1), state) {
        Ok(found) => found,
        Err(refused) => return refused,
    };

    let key = code_key(&addr);
    let expanded = TrieKeySlice::from_key(&key);

    let trie_store = match &state.trie_store {
        Some(ts) => ts.clone(),
        None => return JsonRpcResponse::success(id, json!("0x")),
    };

    // Strict: a node the store cannot produce is an error naming it, not an
    // answer of "no code". `eth_getCode` reporting `0x` for a contract whose
    // code node was collected is the plausible-wrong-answer failure, and a
    // caller running an EVM on the result would get a silent no-op instead
    // of a visible error.
    match root.try_get(&expanded, &*trie_store) {
        Ok(Some(data)) => JsonRpcResponse::success(id, json!(to_hex_bytes(&data))),
        Ok(None) => JsonRpcResponse::success(id, json!("0x")),
        Err(missing) => JsonRpcResponse::error(
            id,
            INTERNAL_ERROR,
            format!("The state is incomplete here: node {missing:#x} is missing"),
        ),
    }
}

/// `eth_getCode` on a node holding no state: ask a peer, verify, answer.
///
/// The code comes back as a proof of `code_key(address)` against the state
/// root in a header this node validated itself, so a peer that invents code
/// is caught rather than believed. On RSK this is the **only** way to
/// authenticate code: the account record is `(nonce, balance, stateFlags)`
/// and carries no `codeHash` to compare hashed bytes against (#393).
///
/// A proved absence is `0x`, as on a full node. An unverifiable answer is an
/// error, never `0x` -- the two are the same bytes to a caller and mean
/// opposite things to an EVM about to run them.
async fn light_code(
    id: Value,
    addr: Address,
    block_param: &str,
    state: &RpcState,
) -> JsonRpcResponse {
    use rustock_trie::proof::{verify_path, Outcome};

    let Some(fetcher) = state.proof_fetcher.clone() else {
        return JsonRpcResponse::error(
            id,
            INTERNAL_ERROR,
            "State not available: this node holds no trie and has no peer to ask",
        );
    };
    if let Some(refused) = stale_head(&id, block_param, state) {
        return refused;
    }
    let Some(header) = resolve_header(block_param, &state.store) else {
        return JsonRpcResponse::error(id, INTERNAL_ERROR, "Cannot resolve block");
    };

    let fetched = match fetcher
        .fetch_proof(crate::server::ProofAsk {
            block_number: header.number,
            address: addr,
            include_code: true,
            ..Default::default()
        })
        .await
    {
        Ok(v) => v,
        Err(e) => return JsonRpcResponse::error(id, INTERNAL_ERROR, e),
    };

    if fetched.code.is_empty() {
        // Nothing to check. Either the peer predates the field or it could
        // not walk its store; both leave this node unable to answer, and
        // `0x` would be a claim it cannot support.
        return JsonRpcResponse::error(
            id,
            INTERNAL_ERROR,
            "the peer served no code proof, so this node cannot answer for the code",
        );
    }

    match verify_path(&fetched.code, &code_key(&addr), header.state_root) {
        Ok(Outcome::Value(code)) => JsonRpcResponse::success(id, json!(to_hex_bytes(&code))),
        Ok(Outcome::Absent) => JsonRpcResponse::success(id, json!("0x")),
        Err(e) => JsonRpcResponse::error(
            id,
            INTERNAL_ERROR,
            format!("the peer's code proof does not check out: {e}"),
        ),
    }
}

pub fn eth_get_storage_at(id: Value, params: &Value, state: &RpcState) -> JsonRpcResponse {
    let Some(addr) = params.get(0).and_then(|v| v.as_str()).and_then(|s| s.parse::<Address>().ok()) else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing or invalid address");
    };
    // A slot is a quantity, not a hash: `0x0` is what clients send for slot
    // zero, and demanding all 64 hex characters made this method unusable from
    // any ordinary wallet. See #179.
    let Some(slot) = params.get(1).and_then(|v| v.as_str()).and_then(parse_storage_slot) else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing or invalid storage slot");
    };
    let (root, _header) = match queried_state(&id, params.get(2), state) {
        Ok(found) => found,
        Err(refused) => return refused,
    };

    let key = storage_key(&addr, &slot);
    let expanded = TrieKeySlice::from_key(&key);

    let trie_store = match &state.trie_store {
        Some(ts) => ts.clone(),
        None => {
            let zero = B256::ZERO;
            return JsonRpcResponse::success(id, json!(format!("{:#066x}", zero)));
        }
    };

    match root.get(&expanded, &*trie_store) {
        Some(data) => {
            let mut padded = [0u8; 32];
            let start = 32usize.saturating_sub(data.len());
            padded[start..].copy_from_slice(&data[..data.len().min(32)]);
            let val = B256::from(padded);
            JsonRpcResponse::success(id, json!(format!("{:#066x}", val)))
        }
        None => {
            let zero = B256::ZERO;
            JsonRpcResponse::success(id, json!(format!("{:#066x}", zero)))
        }
    }
}

/// Why a block parameter gives no state to read.
pub(crate) enum NoState {
    /// The node has no such block.
    Block,
    /// It has the block, and not the state the block left.
    State(B256),
}

/// The state a query reads. Its block parameter is read as rskj's
/// `BlockRefParam` reads it (`helpers::block_ref`) and looked up as
/// `Web3InformationRetriever.getInformationProvider` looks it up, which
/// answers `-32600` for a block the node doesn't have and for a state it
/// doesn't hold. Otherwise the answer rskj gives in its place.
pub(crate) fn queried_state(
    id: &Value,
    block: Option<&Value>,
    state: &RpcState,
) -> Result<(TrieNode, Header), JsonRpcResponse> {
    let refuse = |(code, message): (i64, String)| JsonRpcResponse::error(id.clone(), code, message);
    let (identifier, _) = crate::helpers::block_ref(block, &state.store).map_err(refuse)?;
    let lookup = crate::helpers::information_block(&identifier).map_err(refuse)?;
    if let Some(refused) = stale_head(id, &lookup, state) {
        return Err(refused);
    }
    state_at(&lookup, state).map_err(|none| {
        refuse(match none {
            NoState::Block => (INVALID_REQUEST, format!("Block {identifier} not found")),
            NoState::State(hash) => (INVALID_REQUEST, stateless(hash)),
        })
    })
}

/// The state a local call runs on, at the block `helpers::call_block` or
/// `estimate_block` gave, or the answer rskj gives where there is none
/// ([`no_execution_state`]).
pub(crate) fn execution_state(
    id: &Value,
    block: &str,
    state: &RpcState,
) -> Result<(TrieNode, Header), JsonRpcResponse> {
    if let Some(refused) = stale_head(id, block, state) {
        return Err(refused);
    }
    state_at(block, state).map_err(|none| no_execution_state(id, block, none))
}

/// What rskj answers a local call that has no state to run on.
/// `ExecutionBlockRetriever` refuses a number that has no block, `-32602`;
/// a state the node doesn't hold is an exception nobody catches
/// (`RepositoryLocator.snapshotAt`), `-32603`.
pub(crate) fn no_execution_state(id: &Value, block: &str, none: NoState) -> JsonRpcResponse {
    let number = block
        .strip_prefix("0x")
        .and_then(|hex| u64::from_str_radix(hex, 16).ok());
    let (code, message) = match (none, number) {
        (NoState::Block, Some(number)) => {
            (INVALID_PARAMS, format!("Invalid block number {number}"))
        }
        (NoState::Block, None) => (INTERNAL_ERROR, "Internal server error".to_string()),
        (NoState::State(hash), _) => (INTERNAL_ERROR, stateless(hash)),
    };
    JsonRpcResponse::error(id.clone(), code, message)
}

/// rskj's words for a block whose state the node doesn't hold. It prints the
/// hash bare (`Keccak256.toString`).
fn stateless(block: B256) -> String {
    format!("State not found for block with hash {}", hex::encode(block))
}

/// Resolves a block parameter to a state root and header.
pub fn resolve_state_root(block_param: &str, state: &RpcState) -> Option<(TrieNode, Header)> {
    state_at(block_param, state).ok()
}

/// [`resolve_state_root`], saying what was missing where it finds no state.
pub(crate) fn state_at(block_param: &str, state: &RpcState) -> Result<(TrieNode, Header), NoState> {
    let store = &state.store;

    // State queries against "latest" must use the EXECUTED head — the
    // download head can be far ahead and its state doesn't exist yet.
    // Fall back to the chain head when no exec head is tracked.
    let header = match block_param {
        "latest" | "pending" => store
            .exec_head()
            .ok()
            .flatten()
            .and_then(|(exec_hash, _)| store.header(exec_hash).ok().flatten())
            .or_else(|| resolve_header(block_param, store)),
        _ => resolve_header(block_param, store),
    }
    .ok_or(NoState::Block)?;
    let no_state = NoState::State(header.hash());
    let Some(trie_store) = state.trie_store.as_ref() else {
        return Err(no_state);
    };

    // Pre-RSKIP126 (wasabi100) headers carry the legacy Ethereum-style state
    // root, which doesn't exist in the Unitrie store. For the executed head
    // the Unitrie root is tracked alongside it; older pre-wasabi blocks
    // cannot be resolved.
    let root_hash = if trie_store.get(header.state_root.as_slice()).is_some() {
        header.state_root
    } else {
        match store.exec_head().ok().flatten() {
            Some((exec_hash, exec_root)) if exec_hash == header.hash() => exec_root,
            _ => return Err(no_state),
        }
    };

    let Some(root_data) = trie_store.get(root_hash.as_slice()) else {
        return Err(no_state);
    };
    let root_node = TrieNode::from_message(&root_data, &**trie_store);
    Ok((root_node, header))
}

pub fn resolve_header(block_param: &str, store: &BlockStore) -> Option<Header> {
    let head_num = store.head().ok()?
        .and_then(|h| store.header(h).ok().flatten())
        .map(|h| h.number)?;

    match block_param {
        "latest" | "pending" => {
            let hash = store.head().ok()??;
            store.header(hash).ok()?
        }
        "earliest" => {
            let hash = store.canonical_hash(0).ok()??;
            store.header(hash).ok()?
        }
        s if s.starts_with("0x") && s.len() == 66 => {
            if let Some(hash) = parse_b256(s) {
                store.header(hash).ok()?
            } else {
                let num = parse_block_number(s, head_num)?;
                let hash = store.canonical_hash(num).ok()??;
                store.header(hash).ok()?
            }
        }
        s => {
            let num = parse_block_number(s, head_num)?;
            let hash = store.canonical_hash(num).ok()??;
            store.header(hash).ok()?
        }
    }
}

pub(crate) fn trie_get_account(root: &TrieNode, key: &[u8], state: &RpcState) -> Option<AccountState> {
    let trie_store = state.trie_store.as_ref()?;
    let expanded = TrieKeySlice::from_key(key);
    let data = root.get(&expanded, &**trie_store)?;
    AccountState::decode(&data).ok()
}

/// Refuse a `latest` read served from a head that has stopped moving (#394).
///
/// A node whose header sync has died — no peers, a wedged follow loop, a
/// partition — keeps answering state queries against the last header it
/// accepted, and every answer is **valid**: real nodes under a real state
/// root in a header it validated itself. They are answers about a block from
/// hours ago, and nothing in them says so. A balance screen rendered from a
/// three-day-old header looks exactly like a current one, which is the
/// "plausible number" failure rather than the "visible error" one.
///
/// This is the omission rule applied to time. The proof machinery separates
/// "proved absent" from "did not learn" for state; a stalled chain is the
/// same failure in the time dimension, and without this it is reported as
/// fact.
///
/// Scope, deliberately narrow:
///
/// * **Only the relative tags.** `latest` and `pending` mean "now", and the
///   node is asserting something about now that it does not know. A caller
///   naming an explicit number or hash is asking about a specific old block
///   and is not being misled, so those are never refused — including by a
///   client that has read the age from `rsk_nodeStatus` and decided to
///   proceed anyway.
/// * **Off unless configured.** `None` is the default, which keeps every
///   method behaving as rskj's does. The right bound is a measured property
///   of the chain's inter-block variance rather than a number to pick, and a
///   server that guessed one would turn a working node into a failing one at
///   the first slow stretch. Until it is measured, the node publishes
///   `headAgeSeconds` through `rsk_nodeStatus` and leaves the policy to the
///   client, which is the half of #394 that needs no guess.
///
/// A future block timestamp is a clock disagreement, not freshness evidence,
/// and is never grounds to refuse: `checked_sub` yields `None` and the read
/// goes through.
pub(crate) fn stale_head(id: &Value, block_param: &str, state: &RpcState) -> Option<JsonRpcResponse> {
    let bound = state.max_head_age?;
    if !matches!(block_param, "latest" | "pending") {
        return None;
    }
    let head = state
        .store
        .head()
        .ok()
        .flatten()
        .and_then(|h| state.store.header(h).ok().flatten())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let age = now.checked_sub(head.timestamp)?;
    if age <= bound {
        return None;
    }
    Some(JsonRpcResponse::error(
        id.clone(),
        STALE_HEAD,
        format!(
            "This node's head #{} is {age}s old, past the {bound}s bound, so \
             '{block_param}' is not served. Ask another node, or name an \
             explicit block number to read this one's view anyway.",
            head.number
        ),
    ))
}
