//! rskj's `BridgeSupportTest.ReleaseTransactionInfo` tests of the refund of a
//! legacy peg-in from a multisig sender
//! (`registerBtcTransaction_forRefundableLegacyPeginFromMultisig_*RSKIP305*`)
//! and of the peg-out batch `updateCollections` builds
//! (`updateCollections_whenReleasesInQueue*RSKIP305*`): from RSKIP428 the
//! Bridge logs `pegout_transaction_created` with the value of each output the
//! release spends, and from RSKIP305 it also keeps those values under the
//! release's hash (`BridgeSupport.processReleaseTransactionInfo`). Lovell700
//! is before RSKIP305, and Reed800 brings it; rskj's tests of the latter have
//! every upgrade on, which Vetiver900 is.
//!
//! The refunded peg-in pays mainnet's genesis federation, at the height the
//! pegout index is read from.

use alloy_primitives::{B256, U256};
use bitcoin::Transaction;

use crate::bridge::constants::BridgeConstants;
use crate::bridge::peg::{self, ReleaseRequest};
use crate::bridge::storage::{self, BridgeUtxo};
use crate::bridge::tx as bridge_tx;
use crate::hardfork::{RskNetworkUpgrade, RSK_MAINNET_CHAIN_ID};

use super::super::support::*;

use RskNetworkUpgrade::{Lovell700, Reed800, Vetiver900};

/// One bitcoin as bitcoinj's `VarInt` writes it.
const ONE_BITCOIN_VARINT: [u8; 5] = [0xfe, 0x00, 0xe1, 0xf5, 0x05];

/// `pegout_transaction_created(bytes32 indexed btcTxHash, bytes
/// utxoOutpointValues)`, the values as bitcoinj's `VarInt`s.
fn pegout_transaction_created(release: &Transaction, values: &[u8]) -> Event {
    let mut data = U256::from(32u64).to_be_bytes::<32>().to_vec();
    data.extend_from_slice(&U256::from(values.len()).to_be_bytes::<32>());
    data.extend_from_slice(values);
    data.resize(data.len().div_ceil(32) * 32, 0);
    (
        vec![
            topic("pegout_transaction_created(bytes32,bytes)"),
            B256::from(txid_bytes(release)),
        ],
        data,
    )
}

/// A bitcoin from a two-of-two multisig to mainnet's federation, registered
/// at RSK block 10 of mainnet's ladder up to `last`: its refund, the events
/// the registration logged, and what is kept under the refund's hash.
fn refunded(last: RskNetworkUpgrade) -> (Transaction, Vec<Event>, Option<Vec<u8>>) {
    let config = BridgeConstants::mainnet();
    let cfg = up_to(last, RSK_MAINNET_CHAIN_ID);
    let redeem = multisig(
        &[
            key_from_seed("legacy_pegin_p2sh_multisig_key_1"),
            key_from_seed("legacy_pegin_p2sh_multisig_key_2"),
        ],
        2,
    );
    let pegin = tx(
        vec![p2sh_multisig_spend(&redeem, 2)],
        &[(COIN, genesis_federation_output(&config))],
    );
    let height = config.btc_height_when_pegout_tx_index_activates
        + config.pegout_tx_index_grace_period_in_btc_blocks;
    let mut ctx = bridge_holding(&pegin, height, 10, &config, &cfg);
    let registered = register(&mut ctx, &pegin, i64::from(height), &config, &cfg);
    assert!(registered.processed);
    let refunds = waiting_pegouts(&mut ctx);
    assert_eq!(refunds.len(), 1, "a refund");
    let refund = refunds[0].clone();
    let key = storage::compound_key(
        storage::RELEASES_OUTPOINTS_VALUES_KEY,
        "-",
        &bridge_tx::to_hex(&txid_bytes(&refund)),
    );
    let kept = storage::bridge_load_raw(&mut ctx, key);
    (refund, registered.logs, kept)
}

/// `registerBtcTransaction_forRefundableLegacyPeginFromMultisig_beforeRSKIP305_justEmitsPegoutTransactionCreatedEvent`.
#[test]
fn before_rskip305_a_refund_is_logged_with_what_it_spends_and_nothing_is_kept() {
    let (refund, logs, kept) = refunded(Lovell700);
    assert!(logs.contains(&release_requested(&refund, COIN)), "{logs:?}");
    assert!(
        logs.contains(&pegout_transaction_created(&refund, &ONE_BITCOIN_VARINT)),
        "{logs:?}"
    );
    assert_eq!(kept, None);
}

/// `registerBtcTransaction_forRefundableLegacyPeginFromMultisig_afterRSKIP305_processReleaseTransactionsInfo`.
#[test]
fn from_rskip305_what_a_refund_spends_is_kept_under_its_hash() {
    for last in [Reed800, Vetiver900] {
        let (refund, logs, kept) = refunded(last);
        assert!(
            logs.contains(&release_requested(&refund, COIN)),
            "{last:?} {logs:?}"
        );
        let created = pegout_transaction_created(&refund, &ONE_BITCOIN_VARINT);
        assert!(logs.contains(&created), "{last:?} {logs:?}");
        assert_eq!(kept, Some(ONE_BITCOIN_VARINT.to_vec()), "{last:?}");
    }
}

/// What the federation of the `updateCollections` tests holds, in satoshis,
/// one output each: a batch of the two requests [`REQUESTS`] needs all three,
/// and leaves change.
const OUTPOINT_VALUES: [u64; 3] = [400_000, 90_000, 50_000];

/// [`OUTPOINT_VALUES`] as bitcoinj's `VarInt`s.
const OUTPOINT_VALUES_VARINT: [u8; 13] = [
    0xfe, 0x80, 0x1a, 0x06, 0x00, 0xfe, 0x90, 0x5f, 0x01, 0x00, 0xfd, 0x50, 0xc3,
];

/// The two requests queued, each with the hash of the transaction that made
/// it: 100,000 and 400,000 satoshis.
const REQUESTS: [([u8; 20], u64, [u8; 32]); 2] = [
    ([0x01; 20], 100_000, [0x01; 32]),
    ([0x02; 20], 400_000, [0x02; 32]),
];

/// rskj `BitcoinUtils.getSigHashForPegoutIndex` of a peg-out whose inputs
/// carry no signatures: `hashForSignature(0, redeem, ALL, false)`.
fn index_sighash(tx: &Transaction, redeem: &[u8]) -> [u8; 32] {
    use bitcoin::hashes::Hash;
    let mut copy = tx.clone();
    for (index, input) in copy.input.iter_mut().enumerate() {
        input.script_sig = if index == 0 {
            bitcoin::ScriptBuf::from_bytes(redeem.to_vec())
        } else {
            bitcoin::ScriptBuf::new()
        };
    }
    let mut preimage = bitcoin::consensus::serialize(&copy);
    preimage.extend_from_slice(&1u32.to_le_bytes());
    bitcoin::hashes::sha256d::Hash::hash(&preimage).to_byte_array()
}

/// What rskj's `ReleaseTransactionInfo` tests of `updateCollections` look at
/// once it has run.
struct Batched {
    ctx: Ctx,
    /// The peg-out it built.
    pegout: Transaction,
    /// The RSK block and the creation hash it waits for confirmations under.
    waiting_under: (u64, Option<[u8; 32]>),
    logs: Vec<Event>,
    /// The active federation's redeem script.
    redeem: Vec<u8>,
}

/// rskj's `ReleaseTransactionInfo` set-up for `updateCollections`, at RSK
/// block 0 of mainnet's ladder up to `last`: `BridgeSupportTest`'s active
/// federation, a P2SH ERP federation, holding [`OUTPOINT_VALUES`], the fee
/// per kilobyte 10,000 satoshis, and [`REQUESTS`] queued; then
/// `updateCollections` in the transaction [`RSK_TX_HASH`].
fn batched(last: RskNetworkUpgrade) -> Batched {
    let config = BridgeConstants::mainnet();
    let cfg = up_to(last, RSK_MAINNET_CHAIN_ID);
    let mut ctx = ctx_at(0);
    fund_bridge_but(&mut ctx, 0);
    set_fee_per_kb(&mut ctx, 10_000);
    let members: Vec<[u8; 33]> = (1u8..=9).map(|seed| public_key(&[seed])).collect();
    store_federation(&mut ctx, NEW_FEDERATION, &members, 1, 3000);
    let redeem = erp_redeem(
        &members,
        &keys_of(config.erp_fed_pubkeys),
        config.erp_fed_activation_delay,
    );
    let utxos: Vec<BridgeUtxo> = OUTPOINT_VALUES
        .iter()
        .zip(1u8..)
        .map(|(&value_satoshis, tag)| BridgeUtxo {
            tx_hash: [tag; 32],
            vout: 0,
            value_satoshis,
            height: 1,
            script: p2sh_of(&redeem).to_bytes(),
            coinbase: false,
        })
        .collect();
    storage::store_federation_utxos(&mut ctx, &cfg, 0, &utxos);
    let queue: Vec<ReleaseRequest> = REQUESTS
        .iter()
        .map(
            |&(btc_dest_hash160, amount_satoshis, hash)| ReleaseRequest {
                btc_dest_hash160,
                amount_satoshis,
                rsk_tx_hash: Some(hash),
            },
        )
        .collect();
    storage::bridge_store_bytes_named(
        &mut ctx,
        "releaseRequestQueueWithTxHash",
        &peg::serialize_release_queue_with_hash(&queue),
    );

    in_a_call(&mut ctx, |ctx| {
        peg::update_collections(ctx, 0, &config, &cfg, &tx_context())
    })
    .expect("updateCollections returns");
    let logs = take_logs(&mut ctx);
    let waiting = peg::load_pegout_confirmation_set(&mut ctx, true);
    assert_eq!(waiting.len(), 1, "one peg-out");
    let pegout = bitcoin::consensus::deserialize(&waiting[0].btc_tx_raw).expect("a transaction");
    Batched {
        ctx,
        pegout,
        waiting_under: (waiting[0].rsk_block_height, waiting[0].rsk_tx_hash),
        logs,
        redeem,
    }
}

impl Batched {
    /// What the Bridge keeps under the peg-out's hash.
    fn kept(&mut self) -> Option<Vec<u8>> {
        let hash = bridge_tx::to_hex(&txid_bytes(&self.pegout));
        let key = storage::compound_key(storage::RELEASES_OUTPOINTS_VALUES_KEY, "-", &hash);
        storage::bridge_load_raw(&mut self.ctx, key)
    }

    /// Whether the peg-out's first input's sighash is in the pegout index.
    fn indexed(&mut self) -> bool {
        let sighash = bridge_tx::to_hex(&index_sighash(&self.pegout, &self.redeem));
        let key = storage::compound_key("pegoutTxSigHash", "-", &sighash);
        storage::bridge_load_raw(&mut self.ctx, key) == Some(vec![1])
    }

    /// rskj's assertions that the batch was settled, but for what is kept
    /// under its hash: waiting for confirmations under this call's hash and
    /// block, in the pegout index, logged as requested for all the queue
    /// asked, and as created with what its inputs spend.
    fn assert_settled(&mut self) {
        assert_eq!(self.waiting_under, (0, Some(RSK_TX_HASH)));
        assert!(self.indexed(), "in the pegout index");
        let total = REQUESTS.iter().map(|request| request.1).sum();
        let logs = &self.logs;
        assert!(
            logs.contains(&release_requested(&self.pegout, total)),
            "{logs:?}"
        );
        let created = pegout_transaction_created(&self.pegout, &OUTPOINT_VALUES_VARINT);
        assert!(logs.contains(&created), "{logs:?}");
    }
}

/// `updateCollections_whenReleasesInQueue_beforeRSKIP305_justEmitsPegoutTransactionCreatedEvent`.
#[test]
fn before_rskip305_a_batch_is_logged_with_what_it_spends_and_nothing_is_kept() {
    let mut batched = batched(Lovell700);
    batched.assert_settled();
    assert_eq!(batched.kept(), None);
}

/// `updateCollections_whenReleasesInQueueAndLegacyFed_afterRSKIP305_shouldSetRedeemDataInScriptSigAndProcessReleaseTransactionInfo`:
/// the batch's first input pushes the federation's redeem script last in its
/// scriptSig, and what the batch spends is kept under its hash.
#[test]
fn from_rskip305_a_legacy_federations_batch_keeps_what_it_spends_under_its_hash() {
    for last in [Reed800, Vetiver900] {
        let mut batched = batched(last);
        let pushed_last = match batched.pegout.input[0].script_sig.instructions().last() {
            Some(Ok(bitcoin::script::Instruction::PushBytes(bytes))) => bytes.as_bytes().to_vec(),
            other => panic!("no redeem script pushed last: {other:?}"),
        };
        assert_eq!(pushed_last, batched.redeem, "{last:?}");
        batched.assert_settled();
        assert_eq!(
            batched.kept(),
            Some(OUTPOINT_VALUES_VARINT.to_vec()),
            "{last:?}"
        );
    }
}
