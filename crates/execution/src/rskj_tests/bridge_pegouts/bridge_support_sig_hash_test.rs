//! rskj's `BridgeSupportSigHashTest`: from RSKIP379 (arrowhead600) the
//! sighash of the first input of each peg-out `updateCollections` builds, a
//! batch or a migration, goes to the pegout index; before it, under
//! fingerroot500, none does. The ports run mainnet's Bridge at a block of
//! each and read the index. The SVP fund transaction, which
//! `updateCollections` builds from RSKIP419 (lovell700) and settles as it
//! settles a peg-out, goes there too; the SVP spend transaction, which it
//! builds once the fund transaction is registered, doesn't. Both are logged
//! as peg-outs are from RSKIP428, which lovell700 brings as well.

use alloy_primitives::U256;

use super::support::*;

const ERAS: [(u64, bool); 2] = [(MAINNET_FINGERROOT500, false), (MAINNET_ARROWHEAD600, true)];

/// The redeem script the first input of a peg-out the Bridge built spends:
/// the last push of its scriptSig.
fn spent_redeem(tx: &bitcoin::Transaction) -> Vec<u8> {
    match tx.input[0].script_sig.instructions().last() {
        Some(Ok(bitcoin::script::Instruction::PushBytes(bytes))) => bytes.as_bytes().to_vec(),
        other => panic!("no redeem script pushed last: {other:?}"),
    }
}

/// rskj's `test_pegoutTxIndex_when_pegout_batch_is_created`.
#[test]
fn from_rskip379_a_pegout_batch_built_goes_to_the_pegout_index() {
    for (number, rskip379) in ERAS {
        let mut collections = Collections::mainnet_at(number);
        collections.active(1, 10);
        collections.requests(3);
        collections.update();
        let waiting = collections.waiting();
        assert_eq!(waiting.len(), 1, "{number}");
        let redeem = standard_redeem(&active());
        assert_eq!(
            collections.indexed(&waiting[0], &redeem),
            rskip379,
            "{number}"
        );
    }
}

/// rskj's `test_pegoutTxIndex_when_migration_tx_is_created`.
#[test]
fn from_rskip379_a_migration_built_goes_to_the_pegout_index() {
    for (number, rskip379) in ERAS {
        let mut collections = Collections::migrating(number, STANDARD_MULTISIG, 10, false);
        collections.update();
        let waiting = collections.waiting();
        assert_eq!(waiting.len(), 1, "{number}");
        let redeem = standard_redeem(&retiring());
        assert_eq!(
            collections.indexed(&waiting[0], &redeem),
            rskip379,
            "{number}"
        );
    }
}

/// rskj's `test_pegoutTxIndex_when_migration_and_pegout_batch_tx_are_created`.
#[test]
fn from_rskip379_a_migration_and_a_pegout_batch_built_together_both_go_to_the_pegout_index() {
    for (number, rskip379) in ERAS {
        let mut collections = Collections::migrating(number, STANDARD_MULTISIG, 10, false);
        collections.requests(3);
        collections.update();
        let waiting = collections.waiting();
        assert_eq!(waiting.len(), 2, "{number}");
        let redeems: Vec<Vec<u8>> = waiting.iter().map(spent_redeem).collect();
        assert_ne!(redeems[0], redeems[1], "a migration and a batch");
        for (tx, redeem) in waiting.iter().zip(&redeems) {
            assert_eq!(collections.indexed(tx, redeem), rskip379, "{number}");
        }
    }
}

/// Mainnet's Bridge at lovell700, which brings RSKIP419 and RSKIP428, with
/// the active federation of [`Collections::active`] and its ten UTXOs of a
/// bitcoin, and a federation of five proposed the block before.
fn proposing() -> Collections {
    let number = MAINNET_LOVELL700;
    let mut collections = Collections::mainnet_at(number);
    collections.active(1, 10);
    store_federation(
        &mut collections.bridge.ctx,
        ("proposedFederation", "proposedFederationFormatVersion"),
        &keys(30, 5),
        number - 1,
        P2SH_ERP,
    );
    collections
}

/// A transaction's hash in the order the Bridge's events and cells hold it.
fn display_hash(tx: &bitcoin::Transaction) -> [u8; 32] {
    let mut hash = txid(tx);
    hash.reverse();
    hash
}

/// The logs `updateCollections` leaves when it builds a peg-out `tx` from
/// RSKIP428, in the call whose transaction is `call`: `update_collections`
/// first, as every call of it does, then `release_requested` of `amount`
/// satoshis, then `pegout_transaction_created` with the values of the
/// outputs `tx` spends, `spent`, as bitcoinj's `VarInt`s.
fn assert_logged_as_a_pegout(
    logs: &[revm::primitives::Log],
    call: [u8; 32],
    tx: &bitcoin::Transaction,
    amount: u64,
    spent: &[u8],
) {
    let hash = display_hash(tx);
    assert_eq!(logs.len(), 3, "{logs:?}");
    assert_eq!(
        topics(&logs[0]),
        [event_topic("update_collections(address)")]
    );
    let requested = event_topic("release_requested(bytes32,bytes32,uint256)");
    assert_eq!(topics(&logs[1]), [requested, call, hash]);
    assert_eq!(word(&logs[1].data.data, 0), U256::from(amount));
    let created = event_topic("pegout_transaction_created(bytes32,bytes)");
    assert_eq!(topics(&logs[2]), [created, hash]);
    let data = &logs[2].data.data;
    assert_eq!(word(data, 0), U256::from(32));
    assert_eq!(word(data, 1), U256::from(spent.len()));
    assert_eq!(&data[64..64 + spent.len()], spent);
}

/// From RSKIP419, with a federation proposed the block before and no SVP
/// value kept, `updateCollections` builds the SVP fund transaction
/// (`processSvpFundTransactionUnsigned`), keeps its hash as
/// `svpFundTxHashUnsigned`, and settles it as a peg-out
/// (`settleReleaseRequest`): it waits for its confirmations, its first
/// input's sighash goes to the pegout index (`savePegoutTxSigHash`), which
/// RSKIP379 has on wherever RSKIP419 is, and it is logged as a peg-out of
/// twice two minimum peg-outs, spending one of the active federation's
/// bitcoins.
#[test]
fn from_rskip419_the_svp_fund_transaction_built_goes_to_the_pegout_index() {
    let mut collections = proposing();
    let logs = collections.update();
    let waiting = collections.waiting();
    assert_eq!(waiting.len(), 1);
    let fund = &waiting[0];
    assert!(collections.indexed(fund, &standard_redeem(&active())));
    assert_eq!(
        named(&mut collections.bridge.ctx, "svpFundTxHashUnsigned"),
        Some(rlp_encode_element(&display_hash(fund)))
    );
    let call = collections.bridge.tx.rsk_tx_hash;
    let minimum = collections.bridge.constants.minimum_pegout_tx_value;
    let one_bitcoin = [0xfe, 0x00, 0xe1, 0xf5, 0x05];
    assert_logged_as_a_pegout(&logs, call, fund, 4 * minimum, &one_bitcoin);
}

/// Once the SVP fund transaction is registered (`registerSvpFundTx` keeps it
/// as `svpFundTxSigned` and clears `svpFundTxHashUnsigned`), the next
/// `updateCollections` builds the SVP spend transaction
/// (`processSvpSpendTransactionUnsigned`): version 2, spending the fund
/// transaction's outputs to the proposed federation and to its flyover
/// address, less its fees, to the active federation. It waits for
/// signatures under the call's transaction, its hash is kept as
/// `svpSpendTxHashUnsigned`, `svpFundTxSigned` is cleared, and it is logged
/// as a peg-out of what it pays, spending two minimum peg-outs twice. Unlike
/// the fund transaction it doesn't go to the pegout index.
#[test]
fn from_rskip419_the_svp_spend_transaction_is_logged_and_not_indexed() {
    use crate::bridge::storage::{bridge_storage_key, bridge_store_raw};
    let mut collections = proposing();
    collections.update();
    let fund = collections.waiting().remove(0);
    let ctx = &mut collections.bridge.ctx;
    bridge_store_raw(ctx, bridge_storage_key("svpFundTxHashUnsigned"), None);
    put_named(
        ctx,
        "svpFundTxSigned",
        &rlp_encode_element(&bitcoin::consensus::serialize(&fund)),
    );

    let logs = collections.update();
    let ctx = &mut collections.bridge.ctx;
    let entry = named(ctx, "svpSpendTxWaitingForSignatures").expect("waiting");
    let items = crate::bridge::serialization::rlp_decode_list(&entry).expect("a list");
    let call = collections.bridge.tx.rsk_tx_hash;
    assert_eq!(items.len(), 2);
    assert_eq!(items[0], call);
    let spend: bitcoin::Transaction =
        bitcoin::consensus::deserialize(&items[1]).expect("a transaction");
    assert_eq!(
        named(ctx, "svpSpendTxHashUnsigned"),
        Some(rlp_encode_element(&display_hash(&spend)))
    );
    assert!(named(ctx, "svpFundTxSigned").unwrap_or_default().is_empty());

    assert_eq!(spend.version, bitcoin::transaction::Version::TWO);
    let spent: Vec<bitcoin::OutPoint> = spend
        .input
        .iter()
        .map(|input| input.previous_output)
        .collect();
    let fund_txid = fund.compute_txid();
    assert_eq!(
        spent,
        [
            bitcoin::OutPoint::new(fund_txid, 0),
            bitcoin::OutPoint::new(fund_txid, 1)
        ]
    );
    let minimum = collections.bridge.constants.minimum_pegout_tx_value;
    assert_eq!(spend.output.len(), 1);
    assert_eq!(
        spend.output[0].script_pubkey.as_bytes(),
        address_of(&active()).as_slice()
    );
    let paid = spend.output[0].value.to_sat();
    assert!(0 < paid && paid < 4 * minimum, "{paid}");

    assert!(!collections.indexed(&spend, &spent_redeem(&spend)));
    let two_minimums = [0xfe, 0x00, 0x35, 0x0c, 0x00];
    assert_logged_as_a_pegout(
        &logs,
        call,
        &spend,
        paid,
        &[two_minimums, two_minimums].concat(),
    );
}
