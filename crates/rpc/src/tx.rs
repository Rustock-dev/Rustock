use crate::dto::{ReceiptDto, TransactionDto};
use crate::helpers::{parse_b256, parse_block_number, parse_hex_u32};
use crate::server::RpcState;
use crate::types::*;
use alloy_primitives::{Address, B256};
use serde_json::Value;

/// `eth_getTransactionByHash`, as rskj's `Web3Impl` answers it: the
/// transaction as the main chain holds it (`receiptStore.getInMainChain`),
/// else as the pool holds it pending, with no block, else null. A block that
/// lost a reorg is no answer, though its body is still stored (upstream #343).
pub fn eth_get_transaction_by_hash(id: Value, params: &Value, state: &RpcState) -> JsonRpcResponse {
    let Some(tx_hash) = params.get(0).and_then(|v| v.as_str()).and_then(parse_b256) else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing or invalid tx hash");
    };

    let Some((block_hash, tx_index)) =
        crate::helpers::canonical_tx_location(&state.store, tx_hash)
    else {
        if let Some((tx, sender, hash)) = state
            .tx_pool
            .as_ref()
            .and_then(|pool| pool.get_pending_tx(&tx_hash))
        {
            let dto = TransactionDto::from_tx(&tx, hash, None, None, None, sender);
            return JsonRpcResponse::success(id, serde_json::to_value(dto).unwrap());
        }
        return JsonRpcResponse::success(id, Value::Null);
    };

    let Some((transactions, _ommers)) = state.store.body(block_hash).ok().flatten() else {
        return JsonRpcResponse::success(id, Value::Null);
    };

    let Some(header) = state.store.header(block_hash).ok().flatten() else {
        return JsonRpcResponse::success(id, Value::Null);
    };

    let Some(tx) = transactions.get(tx_index as usize) else {
        return JsonRpcResponse::success(id, Value::Null);
    };

    let sender = recover_sender_or_zero(tx, state);
    let dto = TransactionDto::from_tx(tx, tx_hash, Some(block_hash), Some(header.number), Some(tx_index), sender);
    JsonRpcResponse::success(id, serde_json::to_value(dto).unwrap())
}

pub fn eth_get_transaction_by_block_hash_and_index(id: Value, params: &Value, state: &RpcState) -> JsonRpcResponse {
    let Some(block_hash) = params.get(0).and_then(|v| v.as_str()).and_then(parse_b256) else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing or invalid block hash");
    };
    let Some(index) = params.get(1).and_then(|v| v.as_str()).and_then(parse_hex_u32) else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing or invalid index");
    };

    tx_by_block_and_index(id, block_hash, index, state)
}

pub fn eth_get_transaction_by_block_number_and_index(id: Value, params: &Value, state: &RpcState) -> JsonRpcResponse {
    let Some(bn_str) = params.get(0).and_then(|v| v.as_str()) else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing block number");
    };
    let head_num = head_number(&state.store).unwrap_or(0);
    let Some(number) = parse_block_number(bn_str, head_num) else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Invalid block number");
    };
    let Some(block_hash) = state.store.canonical_hash(number).ok().flatten() else {
        return JsonRpcResponse::success(id, Value::Null);
    };
    let Some(index) = params.get(1).and_then(|v| v.as_str()).and_then(parse_hex_u32) else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing or invalid index");
    };

    tx_by_block_and_index(id, block_hash, index, state)
}

/// `eth_getTransactionReceipt`: the receipt the main chain holds, rskj's
/// `receiptStore.getInMainChain`, else null. A block that lost a reorg is no
/// answer, though its receipts are still stored (upstream #343).
pub fn eth_get_transaction_receipt(id: Value, params: &Value, state: &RpcState) -> JsonRpcResponse {
    let Some(tx_hash) = params.get(0).and_then(|v| v.as_str()).and_then(parse_b256) else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing or invalid tx hash");
    };

    let Some((block_hash, tx_index)) =
        crate::helpers::canonical_tx_location(&state.store, tx_hash)
    else {
        return JsonRpcResponse::success(id, Value::Null);
    };

    let Some(header) = state.store.header(block_hash).ok().flatten() else {
        return JsonRpcResponse::success(id, Value::Null);
    };

    let Some(receipts) = state.store.receipts(block_hash).ok().flatten() else {
        return JsonRpcResponse::success(id, Value::Null);
    };

    let Some(receipt) = receipts.get(tx_index as usize) else {
        return JsonRpcResponse::success(id, Value::Null);
    };

    let Some((transactions, _)) = state.store.body(block_hash).ok().flatten() else {
        return JsonRpcResponse::success(id, Value::Null);
    };

    let Some(tx) = transactions.get(tx_index as usize) else {
        return JsonRpcResponse::success(id, Value::Null);
    };

    let sender = recover_sender_or_zero(tx, state);

    // Compute log_index_start: sum of logs in preceding receipts
    let log_index_start: u32 = receipts[..tx_index as usize]
        .iter()
        .map(|r| r.logs.len() as u32)
        .sum();

    let dto = ReceiptDto::from_receipt(
        receipt, tx, tx_hash, block_hash, header.number, tx_index, sender, log_index_start,
    );
    JsonRpcResponse::success(id, serde_json::to_value(dto).unwrap())
}

// --- helpers ---

fn tx_by_block_and_index(id: Value, block_hash: B256, index: u32, state: &RpcState) -> JsonRpcResponse {
    let Some(header) = state.store.header(block_hash).ok().flatten() else {
        return JsonRpcResponse::success(id, Value::Null);
    };

    let Some((transactions, _)) = state.store.body(block_hash).ok().flatten() else {
        return JsonRpcResponse::success(id, Value::Null);
    };

    let Some(tx) = transactions.get(index as usize) else {
        return JsonRpcResponse::success(id, Value::Null);
    };

    let tx_hash = compute_tx_hash(tx);
    let sender = recover_sender_or_zero(tx, state);

    let dto = TransactionDto::from_tx(tx, tx_hash, Some(block_hash), Some(header.number), Some(index), sender);
    JsonRpcResponse::success(id, serde_json::to_value(dto).unwrap())
}

fn head_number(store: &rustock_storage::BlockStore) -> Option<u64> {
    store.head().ok()?
        .and_then(|h| store.header(h).ok().flatten())
        .map(|h| h.number)
}

/// The canonical transaction hash -- see `helpers::tx_hash`. This one feeds
/// `eth_getTransactionByBlockHashAndIndex` and
/// `eth_getTransactionByBlockNumberAndIndex`, which reported a hash that
/// disagreed with `eth_getBlockBy*` for the same transaction and resolved to
/// nothing.
pub(crate) fn compute_tx_hash(tx: &rustock_core::Transaction) -> B256 {
    tx.tx_hash()
}

/// The sender a transaction was signed by, or the zero address if it
/// cannot be recovered.
///
/// **The chain id has to come from the node's own configuration.** It
/// was hardcoded to 30 here, which is mainnet, so on any other chain
/// every EIP-155 signature failed to recover and every receipt and
/// transaction this module served reported `from` as the zero address:
/// `v` encodes the chain a transaction was signed for, and recovering
/// it against a different one puts the recovery id out of range. That
/// is testnet as much as regtest, and it was silent because the failure
/// path is an `unwrap_or` rather than an error.
///
/// Zero remains the fallback, because a transaction whose signature
/// does not recover is a real thing -- REMASC's has none at all -- and
/// a receipt with an obviously wrong field beats one that fails.
fn recover_sender_or_zero(tx: &rustock_core::Transaction, state: &RpcState) -> Address {
    tx.recover_sender(crate::helpers::chain_id_of(state)).unwrap_or(Address::ZERO)
}


/// `eth_getBlockReceipts(block)` — **every** receipt of one block, in index
/// order, so a caller can rebuild the receipts trie and compare it to the
/// header's `receiptsRoot`.
///
/// The point of the method is the completeness, not the convenience. A Merkle
/// branch out of `rsk_getTransactionReceiptNodesByHash` proves that receipt 3
/// is in the block; it says nothing about a receipt 7 the responder chose not
/// to mention, and a client reconstructing what a block did to an account is
/// defeated by exactly that omission. The only sound check is to take the
/// whole set and rebuild the root, and the whole set is what this returns.
///
/// So the returned array is **the block's entire receipt list, or an error**.
/// It is never a prefix, never a filtered subset, and the empty array means
/// only that the block has no transactions. A caller is expected to check it
/// against `receiptsRoot` rather than to believe it — this method exists to
/// make that check one round trip instead of one per transaction, and a
/// verifying client that skips the check has gained nothing over the old
/// pattern.
///
/// `block` is a number, a tag, or a block hash — the same latitude geth,
/// reth and Nethermind allow, so existing tooling works unchanged. A hash
/// may name a non-canonical block, and this answers for it: the receipts are
/// still that block's, and a caller that wanted the main chain asked by
/// number.
///
/// Null for a block this node does not hold whole. A node that holds the
/// header but not the body or the receipts cannot establish completeness
/// either, so it says nothing rather than answering with what it has: a
/// short array here is indistinguishable from a withheld receipt, which is
/// the failure the method is built to rule out.
///
/// New method, so no rskj behaviour changes. rskj serves no equivalent.
pub fn eth_get_block_receipts(id: Value, params: &Value, state: &RpcState) -> JsonRpcResponse {
    let Some(raw) = params.get(0).and_then(|v| v.as_str()) else {
        return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing block identifier");
    };

    // A hash, or a number/tag. Tried as a hash first because `parse_b256`
    // is the stricter of the two and a 32-byte hash is never a valid height.
    let block_hash = match parse_b256(raw) {
        Some(hash) => hash,
        None => {
            let head = match head_number(&state.store) {
                Some(n) => n,
                None => return JsonRpcResponse::success(id, Value::Null),
            };
            let Some(number) = parse_block_number(raw, head) else {
                return JsonRpcResponse::error(
                    id,
                    INVALID_PARAMS,
                    format!("Invalid block identifier '{raw}'"),
                );
            };
            match state.store.canonical_hash(number).ok().flatten() {
                Some(hash) => hash,
                None => return JsonRpcResponse::success(id, Value::Null),
            }
        }
    };

    let Some(header) = state.store.header(block_hash).ok().flatten() else {
        return JsonRpcResponse::success(id, Value::Null);
    };
    let Some(receipts) = state.store.receipts(block_hash).ok().flatten() else {
        return JsonRpcResponse::success(id, Value::Null);
    };
    let Some((transactions, _)) = state.store.body(block_hash).ok().flatten() else {
        return JsonRpcResponse::success(id, Value::Null);
    };

    // The two lists index the same transactions, so a disagreement means the
    // store is inconsistent for this block. Answering from the shorter of
    // them would produce precisely the truncated set this method promises not
    // to return, so it is an error instead.
    if receipts.len() != transactions.len() {
        return JsonRpcResponse::error(
            id,
            INTERNAL_ERROR,
            format!(
                "Block {block_hash:#x} holds {} receipts for {} transactions; \
                 its receipt set cannot be served whole",
                receipts.len(),
                transactions.len()
            ),
        );
    }

    // `logIndex` runs over the block, not the receipt, so each receipt needs
    // the count of logs before it.
    let mut log_index_start: u32 = 0;
    let mut out: Vec<Value> = Vec::with_capacity(receipts.len());
    for (index, (receipt, tx)) in receipts.iter().zip(transactions.iter()).enumerate() {
        let tx_hash = compute_tx_hash(tx);
        let sender = recover_sender_or_zero(tx, state);
        let dto = ReceiptDto::from_receipt(
            receipt,
            tx,
            tx_hash,
            block_hash,
            header.number,
            index as u32,
            sender,
            log_index_start,
        );
        log_index_start += receipt.logs.len() as u32;
        out.push(serde_json::to_value(dto).unwrap());
    }

    JsonRpcResponse::success(id, Value::Array(out))
}
