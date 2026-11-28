//! `eth_getProof` over the unitrie.
//!
//! # What this returns, and what it does not
//!
//! EIP-1186 defines `accountProof` and `storageProof[].proof` as arrays of
//! **RLP-encoded Merkle-Patricia trie nodes**. RSK has no Merkle-Patricia
//! trie. It has a unitrie: one binary radix trie with path compression
//! holding accounts, code and storage together, with its own node encoding.
//!
//! So the envelope here is EIP-1186's -- `balance`, `nonce`, `codeHash`,
//! `storageHash`, and a `storageProof` entry per requested slot, which are
//! trie-agnostic and useful -- while the proof arrays carry **unitrie node
//! messages**, hex-encoded, deepest node first.
//!
//! **A stock EIP-1186 verifier will not verify these.** It will fail to parse
//! them, which is the safe failure, but a caller expecting Ethereum semantics
//! should know before it starts. Verification is described under "How to
//! check one" below.
//!
//! The encoding is not invented here. rskj already exposes proofs this way --
//! `RskModuleImpl.getTransactionReceiptNodesByHash` returns
//! `HexUtils.toUnformattedJsonHex(node.toMessage())` for each node of
//! `Trie.getNodes(...)` -- so a hex array of `toMessage()` bytes in
//! `findNodes` order is RSK's established shape for a Merkle proof. This
//! method reuses it rather than inventing a second one.
//!
//! rskj does not implement `eth_getProof` at all. Offering it is a deliberate
//! divergence, taken because the question it answers -- prove this balance,
//! this nonce, this slot, against a state root -- is worth answering, and
//! because no standard shape exists that RSK could satisfy instead.
//!
//! # The three paths
//!
//! The unitrie holds everything under one root, which makes the mapping onto
//! EIP-1186's fields more direct than it is on Ethereum, not less:
//!
//! | EIP-1186 | unitrie |
//! |---|---|
//! | `accountProof` | nodes on the path to `account_key(addr)` |
//! | `storageHash` | hash of the node at `storage_prefix_key(addr)` |
//! | `storageProof[i].proof` | nodes on the path to `storage_key(addr, slot)` |
//!
//! `storageHash` deserves a note. On Ethereum it is the root of a *separate*
//! storage trie. RSK has no such thing: an account's storage lives beneath
//! `account_key || 0x00` in the same trie. The hash of the node at that path
//! is the closest true analogue -- it commits to exactly that account's
//! storage and nothing else -- and it is what this returns. An account with
//! no storage has no such node, and gets `0x0…0`, mirroring Ethereum's empty
//! storage root being a constant the caller can recognise.
//!
//! # `codeHash` and `storageHash` are not covered by `accountProof`
//!
//! This is the sharpest thing to know about the method, and it is easy to
//! miss because on Ethereum it is false.
//!
//! `accountProof` proves the **account record**. On Ethereum that record
//! contains `codeHash` and `storageRoot`, so proving the record proves them
//! both. RSK's record is `(nonce, balance, stateFlags)` and contains
//! neither. A caller that verifies `accountProof` against its own header has
//! therefore established the balance and the nonce, and has established
//! *nothing whatever* about the other two fields: this node read them out of
//! its own trie and is asserting them.
//!
//! For a caller that trusts the node, that is fine and is what EIP-1186
//! shaped responses look like. For one that does not -- a wallet -- it is a
//! trust hole in the middle of a proof, and the field's presence makes it
//! look closed.
//!
//! **So neither field is sent unless its proof is.** The fourth parameter,
//! `{"includeCode": true, "includeStorageRoot": true}`, asks for both
//! together: `codeProof` walks to `code_key(addr)` and `storageRootProof` to
//! `storage_prefix_key(addr)`, and the matching hash comes back with it.
//!
//! That is a deliberate departure from EIP-1186's response shape, taken
//! because the alternative is worse. An unprovable value beside a real proof
//! cannot be told apart from a proved one, and the proof next to it makes
//! the whole object look checked -- which is exactly how a reader of this
//! file came to believe `accountProof` covered them. Omission announces
//! itself; a plausible constant does not.
//!
//! Nothing rskj serves changes, because rskj does not implement
//! `eth_getProof` at all. There is no established behaviour here to break,
//! only a draft RSKIP, which moves with it. Both are ordinary keys
//! in the same trie, so both are checked with the same machinery as
//! `accountProof` -- `verify_path` for the code, `verify_node_at` for the
//! storage root, which wants the node's hash rather than its value.
//!
//! Neither is sent unless asked for. A code proof carries the contract's
//! bytes as its leaf, so it is the one path whose size grows with the
//! contract rather than with the depth of the trie.
//!
//! The same two keys map onto `TrieProofRequest`'s `include_code` and
//! `include_storage_root`, so a node with no trie of its own forwards the
//! caller's ask to its peer unchanged rather than deciding for it.
//!
//! # How to check one
//!
//! Walk the returned array from index 0 (the deepest node) upward. For each
//! node, decode the message, recompute its hash with the unitrie's rules, and
//! confirm it appears as the expected child reference of the next node in the
//! array. The last element is the root; its hash must equal the `stateRoot`
//! of the block that was asked about.
//!
//! # Present, absent, incomplete -- three answers, not two
//!
//! A key the trie does not hold is answered with the **divergence path**:
//! the nodes from the root down to the point where the search stops, which
//! is the node that shows on its own terms that the key is not below it. A
//! verifier walks it exactly as it walks an inclusion path, and the last
//! node tells it which of the two it has.
//!
//! This is not a truncated inclusion path. The distinction matters because a
//! shortened inclusion path would look like a divergence path without being
//! one, so the final node is the thing that has to be read: it parts from the
//! key's bits, or the key ends inside it, or the child the key would take is
//! empty, or the key ends at it and it carries no value. Any of those,
//! anchored to the state root by the nodes above it, is a proof of absence.
//!
//! This used to answer an absent key with an empty array, which was a real
//! defect rather than a conservative choice (#392). An empty array is
//! unverifiable, so a client receiving one had learned only that this node
//! said so -- and any node could deny any account by staying quiet. A new
//! wallet's first balance query is for an address that has never been used,
//! so the unprovable case was the common one.
//!
//! **An empty array now means one thing only: this node could not walk the
//! trie.** That is incompleteness, not absence, and it is reported as a JSON-RPC
//! error naming the node that was missing rather than as a result -- because
//! a pruned or partially-synced store that answered "absent" would be stating
//! as fact something it does not know. `rsk_getStateNodes` has always drawn
//! this line; `eth_getProof` now draws it in the same place, and so does the
//! `rsk/64` server (`snap::server::trie_proof`).
//!
//! Three outcomes, and `verify_path` on the client side already distinguishes
//! all three: `Outcome::Value` for present, `Outcome::Absent` for absent, and
//! `ProofError::Empty` for an empty array under a non-empty root.

use crate::server::RpcState;

use crate::types::*;
use alloy_primitives::{Address, B256, U256};
use rustock_trie::{
    account_key, code_key, storage_key, storage_prefix_key, AccountState, TrieKeySlice, TrieNode,
    TrieStore,
};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};

/// Hex-encode a set of raw node messages for the wire.
fn as_hex(nodes: &[alloy_primitives::Bytes]) -> Vec<String> {
    nodes.iter().map(|n| format!("0x{}", hex::encode(n))).collect()
}

/// `keccak256("")`, what `codeHash` reports for an account with no code.
fn empty_code_hash() -> B256 {
    B256::from_slice(&Keccak256::digest([]))
}

/// Every node from the key toward the root, hex-encoded, deepest first --
/// whether or not the trie holds the key.
///
/// `Err` with the hash of the node the store could not produce. That is a
/// third outcome, distinct from both present and absent, and the caller is
/// expected to refuse rather than to fall back on an empty path: see the
/// module note.
fn path_messages(
    root: &TrieNode,
    key: &[u8],
    store: &dyn TrieStore,
) -> Result<Vec<String>, B256> {
    let slice = TrieKeySlice::from_key(key);
    Ok(root
        .try_nodes_toward(&slice, store)?
        .iter()
        .map(|n| format!("0x{}", hex::encode(n.to_message(store))))
        .collect())
}

pub async fn eth_get_proof(id: Value, params: &Value, state: &RpcState) -> JsonRpcResponse {
    let Some(addr) = params
        .get(0)
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<Address>().ok())
    else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing or invalid address");
    };

    // The second parameter is an array of storage slots, and it is required
    // by EIP-1186 even when empty -- a caller that wants only the account
    // sends `[]`.
    let Some(slot_values) = params.get(1).and_then(|v| v.as_array()) else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing storage key array");
    };
    let mut slots: Vec<(String, B256)> = Vec::with_capacity(slot_values.len());
    for v in slot_values {
        // Accepted as a quantity, not a padded hash, for the reason
        // `eth_getStorageAt` accepts one: `0x0` is what wallets send for slot
        // zero, and demanding 64 hex characters makes the method unusable
        // from any of them (#179).
        let Some(raw) = v.as_str() else {
            return JsonRpcResponse::error(id, INVALID_PARAMS, "Storage key must be a string");
        };
        let Some(slot) = crate::helpers::parse_storage_slot(raw) else {
            return JsonRpcResponse::error(
                id,
                INVALID_PARAMS,
                format!("Invalid storage key: {raw}"),
            );
        };
        slots.push((raw.to_string(), slot));
    }

    // A fourth parameter, which EIP-1186 does not define and a caller that
    // has never heard of it never sends (#393).
    //
    // `{"includeCode": true, "includeStorageRoot": true}`. Each asks for the
    // nodes proving one of the two fields this method otherwise reports
    // without evidence, and the keys map one-for-one onto `TrieProofRequest`
    // -- so on a node with no trie, the caller's ask is forwarded rather than
    // reinterpreted.
    //
    // Off by default, and the default is the behaviour that was here before:
    // a node with its own trie still reports `codeHash` and `storageHash`,
    // and still reports them unproved. See the module note on why that is
    // worth asking about.
    let options = params.get(3);
    let flag = |name: &str| -> bool {
        options
            .and_then(|o| o.get(name))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    };
    if let Some(o) = options {
        if !o.is_object() && !o.is_null() {
            return JsonRpcResponse::error(
                id,
                INVALID_PARAMS,
                "The fourth parameter must be an object, e.g. {\"includeCode\": true}",
            );
        }
    }
    let want_code = flag("includeCode");
    let want_storage_root = flag("includeStorageRoot");

    // A node with no trie of its own -- a light client -- asks a peer and
    // checks the answer against a header it validated itself. The local trie
    // wins whenever there is one: no round trip, nothing to verify. Either
    // way the block is read as the state queries read it: a tag, a number,
    // or an EIP-1898 object.
    if state.trie_store.is_none() {
        let named = crate::helpers::block_ref(params.get(2), &state.store)
            .and_then(|(identifier, _)| crate::helpers::information_block(&identifier));
        return match named {
            Ok(block) => {
                light_proof(id, addr, &slots, &block, state, want_code, want_storage_root).await
            }
            Err((code, message)) => JsonRpcResponse::error(id, code, message),
        };
    }

    let (root, _header) = match crate::state::queried_state(&id, params.get(2), state) {
        Ok(found) => found,
        Err(refused) => return refused,
    };
    let Some(trie_store) = state.trie_store.clone() else {
        return JsonRpcResponse::error(id, INTERNAL_ERROR, "State not available");
    };
    let store = &*trie_store;

    // Every read below is the strict one. A node the store cannot produce
    // is reported as an error naming it, not folded into the absent case:
    // `get` would answer `None` for a missing node exactly as it does for a
    // key that is genuinely not there, and this method would then report a
    // zero balance for an account whose record it simply could not reach.
    // That is the same confusion between "not held" and "not learned" that
    // the empty proof array used to cause (#392), one layer down.
    let incomplete = |missing: B256| {
        JsonRpcResponse::error(
            id.clone(),
            INTERNAL_ERROR,
            format!("The state is incomplete here: node {missing:#x} is missing"),
        )
    };

    // The account itself. An address the trie does not hold still gets a
    // well-formed answer -- zero balance, zero nonce, empty code hash -- as
    // Ethereum clients answer the same question, and now with a divergence
    // path that lets the caller check it.
    let acct_key = account_key(&addr);
    let account: Option<AccountState> =
        match root.try_get(&TrieKeySlice::from_key(&acct_key), store) {
            Ok(bytes) => bytes.and_then(|b| AccountState::decode(&b).ok()),
            Err(missing) => return incomplete(missing),
        };
    let (nonce, balance) = match &account {
        Some(a) => (a.nonce, a.balance),
        None => (U256::ZERO, U256::ZERO),
    };

    // Computed only when it can also be proved. Reading the code to hash it
    // is the expensive part of this method for a large contract, and a hash
    // nobody can check is not worth paying for.
    let code_hash = if want_code {
        match root.try_get(&TrieKeySlice::from_key(&code_key(&addr)), store) {
            Ok(Some(code)) if !code.is_empty() => Some(B256::from_slice(&Keccak256::digest(&code))),
            Ok(_) => Some(empty_code_hash()),
            Err(missing) => return incomplete(missing),
        }
    } else {
        None
    };

    // The storage subtree's own node, which is what commits to this
    // account's storage and nothing else.
    //
    // `try_find` and not `try_nodes_toward`: this needs the node *at* the
    // key. The divergence walk returns the node where the search stopped,
    // which for an account with no storage is some ancestor, and hashing
    // that would publish a `storageHash` belonging to a different subtree --
    // a plausible-looking wrong answer, worse than the `0x0...0` an account
    // with no storage is supposed to get.
    //
    // Strict for the same reason every read above is: `nodes_on_path`, which
    // this used, answers `None` both for "no such node" and for "the store
    // could not reach it", so an incomplete store published the
    // empty-storage constant for an account that has storage.
    let storage_hash = if want_storage_root {
        match root.try_find(&TrieKeySlice::from_key(&storage_prefix_key(&addr)), store) {
            Ok(Some(node)) => Some(node.compute_hash(store)),
            Ok(None) => Some(B256::ZERO),
            Err(missing) => return incomplete(missing),
        }
    } else {
        None
    };

    let mut storage_proof: Vec<Value> = Vec::with_capacity(slots.len());
    for (raw, slot) in &slots {
        let key = storage_key(&addr, slot);
        let value = match root.try_get(&TrieKeySlice::from_key(&key), store) {
            Ok(bytes) => bytes.map_or(U256::ZERO, |bytes| {
                // Stored minimally, as `eth_getStorageAt` also finds.
                let mut padded = [0u8; 32];
                let start = 32usize.saturating_sub(bytes.len());
                padded[start..].copy_from_slice(&bytes[bytes.len().saturating_sub(32)..]);
                U256::from_be_bytes(padded)
            }),
            Err(missing) => return incomplete(missing),
        };
        let proof = match path_messages(&root, &key, store) {
            Ok(p) => p,
            Err(missing) => return incomplete(missing),
        };
        storage_proof.push(json!({
            "key": raw,
            "value": format!("{value:#x}"),
            "proof": proof,
        }));
    }

    let account_proof = match path_messages(&root, &acct_key, store) {
        Ok(p) => p,
        Err(missing) => return incomplete(missing),
    };

    let mut answer = serde_json::Map::new();
    answer.insert("address".into(), json!(format!("{addr:#x}")));
    answer.insert("balance".into(), json!(format!("{balance:#x}")));
    answer.insert("nonce".into(), json!(format!("{nonce:#x}")));
    answer.insert("accountProof".into(), json!(account_proof));
    answer.insert("storageProof".into(), json!(storage_proof));
    if let Some(h) = code_hash {
        answer.insert("codeHash".into(), json!(format!("{h:#x}")));
    }
    if let Some(h) = storage_hash {
        answer.insert("storageHash".into(), json!(format!("{h:#x}")));
    }

    // The evidence, and the reason the two fields above travel with it.
    //
    // `accountProof` does not cover either of them. It proves the account
    // record, and on RSK that record is `(nonce, balance, stateFlags)` --
    // no `codeHash` to check hashed bytes against, and no storage root. So a
    // caller that verifies `accountProof` has established the balance and
    // the nonce and has established *nothing* about `codeHash` or
    // `storageHash`.
    //
    // Which is why neither is sent unless its proof is. An unprovable value
    // sitting beside a real proof is worse than an absent one: the caller
    // cannot tell them apart, and the proof next to it makes the whole
    // object look checked. The light path has always reasoned this way --
    // "inventing a plausible constant for either would be worse than saying
    // nothing, because a caller cannot tell an invented value from a proved
    // one" -- and this path used to contradict it.
    if want_code {
        match path_messages(&root, &code_key(&addr), store) {
            Ok(p) => answer.insert("codeProof".into(), json!(p)),
            Err(missing) => return incomplete(missing),
        };
    }
    if want_storage_root {
        match path_messages(&root, &storage_prefix_key(&addr), store) {
            Ok(p) => answer.insert("storageRootProof".into(), json!(p)),
            Err(missing) => return incomplete(missing),
        };
    }

    JsonRpcResponse::success(id, Value::Object(answer))
}

/// `rsk_getStorageNodes(address, [slots], block)`: for each slot, whether the
/// state holds it, its value, and the unitrie nodes from the slot's key up to
/// the state root -- for a slot that is absent as much as for one that is not.
///
/// On Rootstock the absence proof is also the only way to prove a slot holds
/// **zero**, because a slot set to zero is deleted from the trie (rskj
/// `MutableRepository.addStorageBytes`). So this answers both cases with the
/// path toward the key, and says which case it is, so that a path is never
/// read as inclusion when it is not:
///
/// - `present: true`: the deepest node is the slot's own, and holds its value.
/// - `present: false`: the deepest node is where the key leaves the trie: its
///   shared path parts from the key's, or the key ends inside it, or the child
///   the key would take is empty, or the key ends at it and it holds nothing.
///
/// A caller checks the nodes as `eth_getProof`'s are checked, walking the key
/// down from the root, and must reach the same answer as `present`.
///
/// `eth_getProof` now returns the same walk for the same slot (#392); it used
/// to answer an absent key with an empty array, which is what this method was
/// added to work around. The explicit `present` flag is what it still adds:
/// `eth_getProof` leaves the caller to read the deepest node and decide,
/// which is the check it has to do anyway, while this states the conclusion
/// alongside the evidence for it. The block
/// is named in the answer, so the caller knows which header's state root the
/// nodes hang from. A node the store cannot produce is an error, never an
/// absence. A value too long to sit in its node (over 32 bytes, as some of
/// the Bridge's cells are) is stored apart from it: one of those the store
/// has lost is answered as absent, though the slot's node still carries its
/// hash and length, so a caller that walks the nodes, as it must, finds it
/// held and the answer wrong.
pub fn rsk_get_storage_nodes(id: Value, params: &Value, state: &RpcState) -> JsonRpcResponse {
    let Some(addr) = params
        .get(0)
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<Address>().ok())
    else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing or invalid address");
    };
    let Some(slot_values) = params.get(1).and_then(|v| v.as_array()) else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing storage key array");
    };
    let mut slots: Vec<(String, B256)> = Vec::with_capacity(slot_values.len());
    for v in slot_values {
        let Some(raw) = v.as_str() else {
            return JsonRpcResponse::error(id, INVALID_PARAMS, "Storage key must be a string");
        };
        let Some(slot) = crate::helpers::parse_storage_slot(raw) else {
            return JsonRpcResponse::error(
                id,
                INVALID_PARAMS,
                format!("Invalid storage key: {raw}"),
            );
        };
        slots.push((raw.to_string(), slot));
    }

    let (root, header) = match crate::state::queried_state(&id, params.get(2), state) {
        Ok(found) => found,
        Err(refused) => return refused,
    };
    let Some(trie_store) = state.trie_store.clone() else {
        return JsonRpcResponse::error(id, INTERNAL_ERROR, "State not available");
    };
    let store = &*trie_store;

    let mut storage: Vec<Value> = Vec::with_capacity(slots.len());
    for (raw, slot) in &slots {
        let key = TrieKeySlice::from_key(&storage_key(&addr, slot));
        let incomplete = |missing: B256| {
            JsonRpcResponse::error(
                id.clone(),
                INTERNAL_ERROR,
                format!("The state is incomplete here: node {missing:#x} is missing"),
            )
        };
        let nodes = match root.try_nodes_toward(&key, store) {
            Ok(nodes) => nodes,
            Err(missing) => return incomplete(missing),
        };
        let value = match root.try_get(&key, store) {
            Ok(value) => value.filter(|v| !v.is_empty()),
            Err(missing) => return incomplete(missing),
        };
        // Stored minimally, as `eth_getStorageAt` also finds.
        let number = value.as_ref().map_or(U256::ZERO, |bytes| {
            let mut padded = [0u8; 32];
            let start = 32usize.saturating_sub(bytes.len());
            padded[start..].copy_from_slice(&bytes[bytes.len().saturating_sub(32)..]);
            U256::from_be_bytes(padded)
        });
        storage.push(json!({
            "key": raw,
            "present": value.is_some(),
            "value": format!("{number:#x}"),
            "nodes": nodes
                .iter()
                .map(|n| format!("0x{}", hex::encode(n.to_message(store))))
                .collect::<Vec<_>>(),
        }));
    }

    JsonRpcResponse::success(
        id,
        json!({
            "address": format!("{addr:#x}"),
            "blockNumber": format!("{:#x}", header.number),
            "blockHash": format!("{:#x}", header.hash()),
            "stateRoot": format!("{:#x}", header.state_root),
            "storage": storage,
        }),
    )
}

/// The most keys that one `rsk_getStateNodes` call walks along.
pub const MAX_STATE_KEYS: usize = 64;

/// The longest key that `rsk_getStateNodes` walks along, in bytes: more than
/// any key the unitrie holds, the longest being a storage key's 74.
pub const MAX_STATE_KEY: usize = 128;

/// `rsk_getStateNodes(blockHash, [keys])`: for each key, the unitrie nodes of
/// the walk along it from the state root of the block with hash `blockHash`,
/// from where the walk ends up to the root, as [`TrieNode::try_nodes_toward`]
/// gives them; or null if this node doesn't hold the block's header, or the
/// state at its state root.
///
/// A key is the trie's own, raw (`code_key`, `storage_key`, ...), so one call
/// proves a contract's code and its slots together. The walk hangs from the
/// header's own state root and no other, as that is what a caller checks it
/// against, so a header from before wasabi, whose state root is no unitrie's,
/// has no state here. Whether a key is held is the caller's to read from the
/// nodes, as `rsk_getStorageNodes` describes. A node the store cannot produce
/// is an error, never an absence.
pub fn rsk_get_state_nodes(id: Value, params: &Value, state: &RpcState) -> JsonRpcResponse {
    let Some(hash) = params
        .get(0)
        .and_then(|v| v.as_str())
        .and_then(crate::helpers::parse_b256)
    else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing or invalid block hash");
    };
    let Some(listed) = params.get(1).and_then(|v| v.as_array()) else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing key array");
    };
    if listed.len() > MAX_STATE_KEYS {
        return JsonRpcResponse::error(
            id,
            INVALID_PARAMS,
            format!("At most {MAX_STATE_KEYS} keys at a time"),
        );
    }
    let mut keys = Vec::with_capacity(listed.len());
    for key in listed {
        match key.as_str().and_then(parse_state_key) {
            Some(key) => keys.push(key),
            None => {
                return JsonRpcResponse::error(id, INVALID_PARAMS, format!("Invalid key: {key}"))
            }
        }
    }

    let header = match state.store.header(hash) {
        Ok(Some(header)) => header,
        Ok(None) => return JsonRpcResponse::success(id, Value::Null),
        Err(e) => {
            return JsonRpcResponse::error(
                id,
                INTERNAL_ERROR,
                format!("Failed to read the header: {e}"),
            )
        }
    };
    let Some(trie_store) = state.trie_store.clone() else {
        return JsonRpcResponse::success(id, Value::Null);
    };
    let store = &*trie_store;
    let Some(message) = store.get(header.state_root.as_slice()) else {
        return JsonRpcResponse::success(id, Value::Null);
    };
    let root = TrieNode::from_message(&message, store);

    let mut walks: Vec<Vec<String>> = Vec::with_capacity(keys.len());
    for key in &keys {
        match root.try_nodes_toward(&TrieKeySlice::from_key(key), store) {
            Ok(nodes) => walks.push(
                nodes
                    .iter()
                    .map(|n| format!("0x{}", hex::encode(n.to_message(store))))
                    .collect(),
            ),
            Err(missing) => {
                return JsonRpcResponse::error(
                    id,
                    INTERNAL_ERROR,
                    format!("The state is incomplete here: node {missing:#x} is missing"),
                )
            }
        }
    }
    JsonRpcResponse::success(id, json!(walks))
}

/// A trie key as `0x`-prefixed hex, at most [`MAX_STATE_KEY`] bytes.
fn parse_state_key(text: &str) -> Option<Vec<u8>> {
    let digits = text.strip_prefix("0x")?;
    if digits.len() > 2 * MAX_STATE_KEY {
        return None;
    }
    hex::decode(digits).ok()
}

/// Answer `eth_getProof` on a node that holds no state.
///
/// The peer's answer is checked before any of it is used: every path has to
/// hash up to the state root of a header this node validated itself. A peer
/// that lies costs a round trip; it cannot produce a wrong balance.
///
/// The three outcomes are kept distinct, because conflating them is how a
/// light client gets fooled:
///
/// - a verified value, which is the answer;
/// - a verified **absence**, which is also an answer -- the account is not
///   there, and the proof shows it;
/// - an incomplete proof, which is **not** an answer. The peer withheld, or
///   sent a path for a different key. Reporting that as a zero balance would
///   let any peer deny any account by staying quiet, so it is an error.
async fn light_proof(
    id: Value,
    addr: Address,
    slots: &[(String, B256)],
    block_param: &str,
    state: &RpcState,
    want_code: bool,
    want_storage_root: bool,
) -> JsonRpcResponse {
    use rustock_trie::proof::{verify_path, Outcome};

    let Some(fetcher) = state.proof_fetcher.clone() else {
        return JsonRpcResponse::error(
            id,
            INTERNAL_ERROR,
            "State not available: this node holds no trie and has no peer to ask",
        );
    };
    // The SPV path is the one #394 is really about: this node holds no state
    // of its own, so `latest` is whatever header its sync last accepted, and
    // a sync that has stopped leaves that header in place indefinitely.
    if let Some(refused) = crate::state::stale_head(&id, block_param, state) {
        return refused;
    }
    let Some(header) = crate::state::resolve_header(block_param, &state.store) else {
        return JsonRpcResponse::error(id, INTERNAL_ERROR, "Cannot resolve block");
    };
    let state_root = header.state_root;

    let keys: Vec<B256> = slots.iter().map(|(_, s)| *s).collect();
    // The caller's `includeCode` / `includeStorageRoot` go to the peer
    // unchanged (#393). This node adds nothing and asks for nothing on its
    // own account: a light client that did not want a contract's bytecode
    // should not pay for it because an intermediary thought it might.
    //
    // A peer that predates the fields answers with empty paths, and the two
    // fields are omitted -- which is what this method did for every caller
    // before #393, so that is a known shape rather than a new one.
    let fetched = match fetcher
        .fetch_proof(crate::server::ProofAsk {
            block_number: header.number,
            address: addr,
            storage_keys: keys,
            include_code: want_code,
            include_storage_root: want_storage_root,
        })
        .await
    {
        Ok(v) => v,
        Err(e) => return JsonRpcResponse::error(id, INTERNAL_ERROR, e),
    };
    let account_proof = fetched.account;
    let storage_proofs = fetched.storage;

    let account = match verify_path(&account_proof, &account_key(&addr), state_root) {
        Ok(Outcome::Value(bytes)) => AccountState::decode(&bytes).ok(),
        Ok(Outcome::Absent) => None,
        Err(e) => {
            return JsonRpcResponse::error(
                id,
                INTERNAL_ERROR,
                format!("the peer's account proof does not check out: {e}"),
            )
        }
    };
    let (nonce, balance) = match &account {
        Some(a) => (a.nonce, a.balance),
        None => (U256::ZERO, U256::ZERO),
    };

    let mut storage_proof: Vec<Value> = Vec::with_capacity(slots.len());
    for (i, (raw, slot)) in slots.iter().enumerate() {
        let path = storage_proofs.get(i).cloned().unwrap_or_default();
        let value = match verify_path(&path, &storage_key(&addr, slot), state_root) {
            Ok(Outcome::Value(bytes)) => {
                let mut padded = [0u8; 32];
                let start = 32usize.saturating_sub(bytes.len());
                padded[start..].copy_from_slice(&bytes[bytes.len().saturating_sub(32)..]);
                U256::from_be_bytes(padded)
            }
            Ok(Outcome::Absent) => U256::ZERO,
            Err(e) => {
                return JsonRpcResponse::error(
                    id,
                    INTERNAL_ERROR,
                    format!("the peer's proof for slot {raw} does not check out: {e}"),
                )
            }
        };
        storage_proof.push(json!({
            "key": raw,
            "value": format!("{value:#x}"),
            "proof": path.iter().map(|n| format!("0x{}", hex::encode(n))).collect::<Vec<_>>(),
        }));
    }

    // `codeHash` and `storageHash`, from the peer's proofs or not at all.

    //
    // These used to be omitted unconditionally, because the request could not
    // ask for either and inventing a plausible constant is worse than saying
    // nothing: a caller cannot tell an invented value from a proved one. Now
    // they can be asked for, and each is filled only when the peer's path
    // verifies against this node's own state root.
    //
    // A field that does not verify is **omitted, not zeroed**. Zero is a
    // meaningful value for both -- the empty code hash, the empty storage
    // root -- so publishing zero for "the peer did not convince me" would
    // report a contract as having no code. Omission is the only honest
    // answer, and it is the one a caller already has to handle, since a peer
    // predating #393 produces it too.
    let mut answer = serde_json::Map::new();
    answer.insert("address".into(), json!(format!("{addr:#x}")));
    answer.insert("balance".into(), json!(format!("{balance:#x}")));
    answer.insert("nonce".into(), json!(format!("{nonce:#x}")));
    answer.insert(
        "accountProof".into(),
        json!(account_proof
            .iter()
            .map(|n| format!("0x{}", hex::encode(n)))
            .collect::<Vec<_>>()),
    );
    answer.insert("storageProof".into(), json!(storage_proof));

    if !fetched.code.is_empty() {
        match verify_path(&fetched.code, &code_key(&addr), state_root) {
            // Code is stored under its own key, so a proved value is the
            // code and a proved absence is genuinely no code.
            Ok(Outcome::Value(code)) => {
                answer.insert(
                    "codeHash".into(),
                    json!(format!("{:#x}", B256::from_slice(&Keccak256::digest(&code)))),
                );
                answer.insert("codeProof".into(), json!(as_hex(&fetched.code)));
            }
            Ok(Outcome::Absent) => {
                answer.insert("codeHash".into(), json!(format!("{:#x}", empty_code_hash())));
                answer.insert("codeProof".into(), json!(as_hex(&fetched.code)));
            }
            Err(e) => {
                return JsonRpcResponse::error(
                    id,
                    INTERNAL_ERROR,
                    format!("the peer's code proof does not check out: {e}"),
                )
            }
        }
    }

    if !fetched.storage_root.is_empty() {
        // `verify_node_at` and not `verify_path`: `storageHash` is the hash
        // of the *node* at the storage prefix, and that node ordinarily
        // holds no value -- it exists to carry children. Asking for its
        // value would answer "absent" for every account that has storage,
        // and this would then publish the empty-storage constant for them.
        match rustock_trie::proof::verify_node_at(
            &fetched.storage_root,
            &storage_prefix_key(&addr),
            state_root,
        ) {
            Ok(Some(hash)) => {
                answer.insert("storageHash".into(), json!(format!("{hash:#x}")));
                answer.insert("storageRootProof".into(), json!(as_hex(&fetched.storage_root)));
            }
            // No node at the prefix: this account has no storage, and
            // `0x0...0` is the constant Ethereum clients use for that --
            // here a proved one rather than an assumed one.
            Ok(None) => {
                answer.insert("storageHash".into(), json!(format!("{:#x}", B256::ZERO)));
                answer.insert("storageRootProof".into(), json!(as_hex(&fetched.storage_root)));
            }
            Err(e) => {
                return JsonRpcResponse::error(
                    id,
                    INTERNAL_ERROR,
                    format!("the peer's storage-root proof does not check out: {e}"),
                )
            }
        }
    }

    JsonRpcResponse::success(id, Value::Object(answer))
}
